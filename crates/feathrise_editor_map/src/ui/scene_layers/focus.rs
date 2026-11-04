//! 作者树聚焦复用水域范围和角色缓存；整层角色包围盒在唯一后台槽合并，迟到不抢镜头。

use super::{
    resources::LayerFocusState,
    types::{CameraIdentity, PendingFocus},
};
use crate::{
    authoring::EditorMapDraft,
    resources::{MapEditorState, MapEditorViewportState},
    ui::scene_context_menu::{
        MenuContext, SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectTarget,
    },
    viewport::{LayerCandidateState, LayerTarget, SelectedWaterViewportState, framing_extent},
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};

fn camera_identity(camera: &MapEditorViewportState, editor: &MapEditorState) -> CameraIdentity {
    (
        camera.center,
        camera.extent,
        camera.yaw,
        camera.pitch,
        camera.render_size,
        editor.orthographic,
    )
}

fn apply_bounds(
    bounds: Option<(Vec3, Vec3)>,
    camera: &mut MapEditorViewportState,
    editor: &mut MapEditorState,
) {
    let Some((min, max)) = bounds.filter(|(min, max)| min.is_finite() && max.is_finite()) else {
        editor.status = "当前图层没有可聚焦的内容".into();
        return;
    };
    camera.center = min * 0.5 + max * 0.5;
    camera.extent = framing_extent(
        ((max - min) * 0.5).max(Vec3::splat(2.0)),
        camera.yaw,
        camera.pitch,
        camera.render_size,
        editor.orthographic,
    )
    .max(25.0);
    editor.status = "已聚焦所选作者内容".into();
}

fn canvas_bounds(draft: &EditorMapDraft) -> Option<(Vec3, Vec3)> {
    let manifest = draft.terrain_manifest();
    let min = Vec3::new(
        manifest.world_min[0],
        manifest.terrain.base_height,
        manifest.world_min[1],
    );
    let max = Vec3::new(
        manifest.world_max[0],
        manifest.terrain.base_height + manifest.terrain.height_scale,
        manifest.world_max[1],
    );
    let pose = draft
        .terrain_documents
        .active
        .as_ref()
        .map_or(Mat4::IDENTITY, |active| {
            active.definition.transform.as_transform().to_matrix()
        });
    let center = pose.transform_point3(min * 0.5 + max * 0.5);
    let half = (max - min) * 0.5;
    let half = pose.transform_vector3(Vec3::X).abs() * half.x
        + pose.transform_vector3(Vec3::Y).abs() * half.y
        + pose.transform_vector3(Vec3::Z).abs() * half.z;
    (half.cmpge(Vec3::ZERO).all()).then_some((center - half, center + half))
}

fn still_selected(
    target: &SceneObjectTarget,
    selection: Option<&LayerCandidateState>,
    domain: Option<&crate::game_authoring::map_interaction::MapInteractionState>,
) -> bool {
    match &target.key {
        SceneObjectKey::Domain { kind, id, .. } => match kind {
            crate::game_authoring::map_interaction::DomainItemKind::Layer => selection.is_some_and(|state| matches!(state.selected.as_ref(), Some(LayerTarget::Domain(selected)) if selected == id))
                && domain.is_some_and(|state| state.selected.is_none()),
            crate::game_authoring::map_interaction::DomainItemKind::Object => domain.is_some_and(|state| state.selected.as_ref() == Some(id)),
        },
        _ => false,
    }
}

pub(super) fn update(
    context: MenuContext,
    menu: Res<SceneContextMenuState>,
    mut editor: ResMut<MapEditorState>,
    mut viewport: Option<ResMut<MapEditorViewportState>>,
    mut water: Option<ResMut<SelectedWaterViewportState>>,
    candidates: Option<Res<LayerCandidateState>>,
    domain: Option<Res<crate::game_authoring::map_interaction::MapInteractionState>>,
    mut state: ResMut<LayerFocusState>,
) {
    let Some(camera) = viewport.as_mut() else {
        return;
    };
    if let Some(pending) = state.pending.as_mut()
        && let Some(bounds) = check_ready(&mut pending.task)
    {
        let pending = state.pending.take().unwrap();
        if context.enabled(&editor)
            && pending.target.current(&editor, &context.session)
            && camera.framed_map_id.as_ref() == Some(&pending.target.map)
            && camera_identity(camera, &editor) == pending.camera
            && still_selected(
                &pending.target,
                candidates.as_deref(),
                domain.as_deref(),
            )
        {
            apply_bounds(bounds, camera, &mut editor);
        }
    }
    if let Some(request) = menu.ready.as_ref().filter(|request| {
        request.action == SceneMenuAction::Focus
            && request.target.current(&editor, &context.session)
    }) {
        match &request.target.key {
            SceneObjectKey::Water { kind, id, .. } => {
                if let Some(water) = water.as_mut() {
                    water.request_focus(&request.target.map, *kind, id);
                }
            }
            SceneObjectKey::Vegetation { .. } => apply_bounds(
                context.session.draft.as_ref().and_then(canvas_bounds),
                camera,
                &mut editor,
            ),
            SceneObjectKey::Domain { .. } => {
                state.desired = Some(request.target.clone())
            }
            _ => {}
        }
    }
    // 取消不会丢弃仍在计算的句柄；旧任务完成前继续占用同一槽，新请求只合并最后一个。
    if state.pending.is_none()
        && let Some(target) = state
            .desired
            .take()
            .filter(|target| context.enabled(&editor) && target.current(&editor, &context.session))
    {
        let Some(snapshot) = domain.as_ref().and_then(|state|
            context.session.draft.as_ref().and_then(|draft| state.snapshot_for(draft)).cloned()) else { return; };
        let SceneObjectKey::Domain { kind, id, .. } = target.key.clone() else { return; };
        state.pending = Some(PendingFocus {
            target,
            camera: camera_identity(camera, &editor),
            task: AsyncComputeTaskPool::get().spawn(async move { (snapshot.bounds)(kind, &id) }),
        });
    }
}
