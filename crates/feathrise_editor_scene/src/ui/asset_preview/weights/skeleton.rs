//! 在权重画布叠加当前蒙皮实例的骨架与所选骨骼。

use super::{
    super::types::{AssetPreviewCamera, AssetPreviewCanvas},
    asset::MAX_JOINTS,
    preview::{self, WeightPreview},
    resources::WeightEditor,
    systems::{Gate, Target},
};
use crate::{
    resources::SceneEditorState,
    ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    viewport::placement_move::MoveCamera,
};
use bevy::{
    camera::visibility::{RenderLayers, VisibilitySystems},
    gizmos::config::GizmoConfigStore,
    mesh::skinning::SkinnedMesh,
    prelude::*,
    transform::TransformSystems,
};
use feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ASSET_PREVIEW_LAYER;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct SkeletonGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct SelectedBoneGizmos;

pub(super) fn install(app: &mut App) {
    app.init_gizmo_group::<SkeletonGizmos>()
        .init_gizmo_group::<SelectedBoneGizmos>()
        .add_systems(Startup, configure)
        .add_systems(
            PostUpdate,
            draw.after(preview::update)
                .after(TransformSystems::Propagate)
                .after(VisibilitySystems::VisibilityPropagate),
        );
}

fn configure(mut store: ResMut<GizmoConfigStore>) {
    // 权重热力图仍显示在模型表面，关节连线在同一预览相机中穿透该表面。
    let (config, _) = store.config_mut::<SkeletonGizmos>();
    config.render_layers = RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER);
    config.depth_bias = -1.0;
    config.line.width = 1.5;
    let (config, _) = store.config_mut::<SelectedBoneGizmos>();
    config.render_layers = RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER);
    config.depth_bias = -1.0;
    config.line.width = 3.5;
}

fn draw(
    target: Target,
    gate: Gate,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    state: Res<WeightEditor>,
    preview: Res<WeightPreview>,
    skins: Query<(&SkinnedMesh, &InheritedVisibility)>,
    transforms: Query<&GlobalTransform>,
    canvases: Query<&ComputedNode, With<AssetPreviewCanvas>>,
    cameras: Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
    mut skeleton: Gizmos<SkeletonGizmos>,
    mut highlight: Gizmos<SelectedBoneGizmos>,
) {
    if sidebar.right != EditorRightSidebarTab::Weights
        || !target.preview.render_visible
        || !gate.open(&editor)
    {
        return;
    }
    let Some(session) = state
        .session
        .as_ref()
        .filter(|session| session.show_skeleton && !session.stale && target.matches(&session.key))
    else {
        return;
    };
    // 显隐传播后直接选当前已显示的蒙皮实例，静置和动作交换时都使用画面的真实关节。
    let Some(skinned) = preview
        .surface_entities(&session.key, session.surface)
        .filter_map(|entity| skins.get(entity).ok())
        .find_map(|(skin, visibility)| visibility.get().then_some(skin))
    else {
        return;
    };
    let joints = &session.doc.surfaces[session.surface].skin.joints;
    if skinned.joints.len() != joints.len() {
        return;
    }
    // 关节数在加载时校验，绘制只读取当前图元的完整姿态。
    let mut positions = [Vec3::ZERO; MAX_JOINTS];
    for (position, &joint) in positions.iter_mut().zip(&skinned.joints) {
        let Ok(transform) = transforms.get(joint) else {
            return;
        };
        *position = transform.translation();
        if !position.is_finite() {
            return;
        }
    }

    let neutral = Color::srgba(0.87, 0.93, 0.99, 0.85);
    let selected = Color::srgb(0.15, 1.0, 1.0);
    let active = usize::from(session.joint);
    for (index, joint) in joints.iter().enumerate() {
        if let Some(parent) = joint.parent.map(usize::from) {
            let (start, end) = (positions[parent], positions[index]);
            if start == end {
                continue;
            }
            // 选中左大腿时高亮髋到膝的骨段，起点方框同时标识没有下级的末端关节。
            if parent == active {
                highlight.line(start, end, selected);
            } else {
                skeleton.line(start, end, neutral);
            }
        }
    }
    let (Ok(canvas), Some(camera)) = (canvases.single(), target.preview.camera) else {
        return;
    };
    let Ok((projection, view)) = cameras.get(camera) else {
        return;
    };
    let Some(corners) = MoveCamera::from_projection(
        projection,
        view,
        canvas.size(),
        canvas.inverse_scale_factor(),
    )
    .and_then(|camera| camera.screen_square(positions[active], 5.0)) else {
        return;
    };
    for i in 0..corners.len() {
        highlight.line(corners[i], corners[(i + 1) % corners.len()], selected);
    }
}

#[cfg(feature = "validation")]
pub(super) fn submitted(world: &World) -> bool {
    let handles = world.resource::<bevy::gizmos::GizmoHandles>();
    handles
        .handles()
        .get(&std::any::TypeId::of::<SelectedBoneGizmos>())
        .is_some_and(Option::is_some)
}
