//! 独立体素表面涂绘使用对象事务与整笔历史，不创建Clipmap洞口或临时摆放记录。
use crate::{
    authoring::{EditorAuthoringSession, terrain_editing::NativeFieldInput},
    resources::MapEditorState,
    types::EditorTool,
    ui::terrain_objects::{Mutation, TerrainObjectsEditorState, selected, start},
};
use bevy::prelude::*;
use feathrise_map_content::{MapId, MapTerrainKind, TerrainId};
use feathrise_map_terrain::volume::sparse::paged::{VolumeFieldBrush, VolumeFieldChannel};

#[derive(Resource, Default)]
pub(super) struct NativeFieldGesture {
    serial: u64,
    active: Option<(MapId, TerrainId, VolumeFieldChannel, String, u64, Vec3)>,
    /// 后台单飞期间保留时间暴露，刷印速度不随CPU/IO完成帧数减慢。
    elapsed: f32,
}

pub(super) fn input(
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut selection: ResMut<TerrainObjectsEditorState>,
    mut gesture: ResMut<NativeFieldGesture>,
    ground: Res<crate::authoring::terrain_ground::TerrainGroundCursor>,
    context: crate::viewport::placement_move::MoveInputContext,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
) {
    editor.terrain_volume_field_active = false;
    if gesture.active.is_some()
        && (!mouse.pressed(MouseButton::Left) || editor.save_requested || editor.close_requested)
    {
        selection.brushing = false;
        gesture.active = None;
        gesture.elapsed = 0.;
    }
    let Some(object) = selected(&session, &selection)
        .filter(|object| object.terrain.kind == MapTerrainKind::Voxel)
    else {
        if gesture.active.is_some() {
            selection.brushing = false;
        }
        gesture.active = None;
        gesture.elapsed = 0.;
        return;
    };
    if !crate::authoring::terrain_documents::canvas_writable(&session, &editor)
        || !editor.open
        || session.saving
        || !context.map_commands_available(&editor)
        || context.pointer_over_ui()
        || editor.close_requested
        || editor.save_requested
    {
        if gesture.active.is_some() {
            selection.brushing = false;
        }
        gesture.active = None;
        gesture.elapsed = 0.;
        return;
    }
    let Some(draft) = session.draft.as_ref() else {
        selection.brushing = false;
        gesture.active = None;
        gesture.elapsed = 0.;
        return;
    };
    let field = match editor.tool {
        EditorTool::Surface => editor
            .surface_selected_preset_id
            .clone()
            .map(|id| (VolumeFieldChannel::Surface, id)),
        EditorTool::Puddles => Some((VolumeFieldChannel::Puddles, "puddles".into())),
        EditorTool::Vegetation => draft
            .vegetation
            .as_ref()
            .and_then(|asset| asset.authoring.as_ref())
            .and_then(|authoring| authoring.layers.get(editor.selected_vegetation_layer))
            .filter(|layer| layer.enabled && !layer.locked)
            .map(|layer| {
                (
                    VolumeFieldChannel::Vegetation,
                    layer.output_layer_id.clone(),
                )
            }),
        _ => None,
    };
    let Some((channel, name)) = field else {
        selection.brushing = false;
        gesture.active = None;
        gesture.elapsed = 0.;
        return;
    };
    // 当前工具由真实三维字段接管，不能让同帧二维R8作者写入使对象事务持续过期。
    editor.terrain_volume_field_active = true;
    if !mouse.pressed(MouseButton::Left) {
        gesture.active = None;
        gesture.elapsed = 0.;
        return;
    }
    gesture.elapsed += time.delta_secs().min(0.1);
    if selection.preparing()
        || selection.transforming
        || ground.terrain.as_ref() != Some(&object.id)
    {
        return;
    }
    let Some(point) = editor.brush_cursor_world else {
        return;
    };
    let Some(ray) = ground.ray else {
        return;
    };
    let Some(query) = draft
        .terrain_ground
        .as_ref()
        .filter(|snapshot| snapshot.revision == session.terrain_revision)
        .and_then(|snapshot| snapshot.object_query(&object.id))
    else {
        return;
    };
    let id = object.id.clone();
    let map = draft.map_id.clone();
    let same =
        gesture
            .active
            .as_ref()
            .is_some_and(|(owner, target, old_channel, old_name, _, _)| {
                owner == &map && target == &id && *old_channel == channel && old_name == &name
            });
    if !same {
        gesture.serial = gesture.serial.wrapping_add(1) | (1u64 << 63);
        gesture.active = Some((
            map,
            id.clone(),
            channel,
            name.clone(),
            gesture.serial,
            point,
        ));
    }
    let elapsed = gesture.elapsed;
    let active = gesture.active.as_ref().unwrap();
    let (strength, flow_exposure) = if channel == VolumeFieldChannel::Vegetation {
        if same {
            (
                0.,
                editor.brush_strength.max(0.)
                    * elapsed
                    * crate::authoring::paint::PAINT_RESPONSE_PER_SECOND,
            )
        } else {
            crate::authoring::paint::vegetation_paint_exposure(editor.brush_strength, elapsed, true)
        }
    } else {
        (
            (1. - (-editor.brush_strength * elapsed * 12.).exp()).clamp(0., 1.),
            0.,
        )
    };
    let brush = VolumeFieldBrush {
        channel,
        id: name,
        from: active.5.to_array(),
        to: point.to_array(),
        normal: Vec3::Y.to_array(),
        radius: editor.brush_radius.clamp(0.1, 8.),
        strength,
        flow_exposure,
        erase: keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight),
        source_to_world: Mat3::IDENTITY,
    };
    let input = NativeFieldInput {
        stroke: active.4,
        brush,
        query,
        ray: (ray.origin, ray.direction),
    };
    selection.brushing = false;
    start(
        &mut session,
        &mut editor,
        &mut selection,
        Mutation::Field(id, input),
    );
    selection.brushing = true;
    gesture.elapsed = 0.;
    gesture.active.as_mut().unwrap().5 = point;
}
