//! 独立地形只响应明确的所选对象；一帧至多提交一份参数，不克隆源数组。
use bevy::prelude::*;
use crate::{authoring::{EditorAuthoringSession, terrain_documents}, resources::SceneEditorState, types::EditorTool,
    ui::terrain_objects::{TerrainObjectsEditorState, Mutation, selected, start},
    viewport::{modes::{ViewportMode, ViewportModeState}, placement_move::MoveInputContext}};
use super::{ObjectBrushInput, resources::BrushGesture};
pub(super) fn brush(mut editor: ResMut<SceneEditorState>, mut session: ResMut<EditorAuthoringSession>,
    mut selection: ResMut<TerrainObjectsEditorState>, mut gesture: ResMut<BrushGesture>,
    modes: Res<ViewportModeState>, context: MoveInputContext,
    sparse: Option<Res<crate::authoring::SparseTerrainAuthoringState>>,
    ground: Res<crate::authoring::terrain_ground::TerrainGroundCursor>,
    source: Option<Res<crate::viewport::VegetationGroundCursor>>,
    viewport: Query<&bevy::ui::RelativeCursorPosition, With<crate::components::SceneEditorViewport>>,
    mouse: Res<ButtonInput<MouseButton>>, time: Res<Time<Real>>) {
    let geometry = matches!(editor.tool, EditorTool::Raise | EditorTool::Lower | EditorTool::Smooth | EditorTool::Flatten) || editor.tool.is_volume_sculpt();
    let height_tool = geometry && !editor.tool.is_volume_sculpt();
    let usable = editor.open && modes.mode == ViewportMode::Edit && !session.saving && geometry
        && terrain_documents::canvas_writable(&session, &editor) && context.scene_commands_available(&editor)
        && !context.pointer_over_ui() && !editor.save_requested && !editor.close_requested
        && viewport.single().is_ok_and(bevy::ui::RelativeCursorPosition::cursor_over)
        && source.as_deref().is_none_or(|source| source.source_authoring_available())
        && (!height_tool || ground.input_authority().is_some());
    if !mouse.pressed(MouseButton::Left) || !usable {
        gesture.active = None; gesture.input_authority = None; gesture.tool = None;
        selection.brushing = false; return;
    }
    if height_tool && mouse.just_pressed(MouseButton::Left) {
        // 可先等待后台真点，但这笔只能来自真实按下；暂停/失焦恢复不能把仍按住当作新笔。
        gesture.input_authority = ground.input_authority(); gesture.tool = Some(editor.tool);
    }
    if height_tool && (gesture.input_authority != ground.input_authority() || gesture.tool != Some(editor.tool)) {
        gesture.active = None; gesture.input_authority = None; gesture.tool = None;
        selection.brushing = false; return;
    }
    if selection.preparing() || selection.transforming { return; }
    let Some(draft) = session.draft.as_ref().filter(|draft| draft.manifest.terrains.is_some()) else { return; };
    let Some(object) = selected(&session, &selection) else { return; };
    if object.terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap && !editor.tool.is_volume_sculpt() {
        // Clipmap高度使用已载入作者会话及局部实时页；逐刷印导出版本会触发整实例重建和选择闪烁。
        gesture.active = None; selection.brushing = sparse.as_ref().is_some_and(|state| !state.document_can_park()); return;
    }
    let supported = if editor.tool.is_volume_sculpt() {
        object.terrain.kind.supports_volume_sculpt()
    } else {
        object.terrain.kind.supports_height_sculpt()
    };
    if !supported {
        gesture.active = None; selection.brushing = false; return;
    } // 工具记忆或迟到选择也须核验作者能力，不能对导入GLB不断启动必失败的后台事务。
    if !object.enabled || selection.locked.contains(&object.id) || selection.hidden.contains(&object.id) {
        gesture.active = None; selection.brushing = false; return;
    }
    // 编辑模式会清理检查器候选；写权限绑定已载入的所选作者画布，不能依赖检查器选中项。
    if editor.terrain_canvas_target.as_ref() != Some(&object.id) { return; }
    let center = if height_tool {
        // 连续移动时完成点仍属于本笔原位置；严格hover可为空，不能因此饿死雕刻，也不能用display补写。
        ground.paint_hit().filter(|hit| hit.terrain == object.id).map(|hit| hit.point)
    } else {
        (ground.terrain.as_ref() == Some(&object.id)).then_some(editor.brush_cursor_world).flatten()
    };
    let Some(center) = center else { return; };
    let map = draft.scene_id.clone(); let id = object.id.clone();
    let valid = gesture.active.as_ref().is_some_and(|(owner, target, _, _)| owner == &map && target == &id);
    if !valid { gesture.serial = gesture.serial.wrapping_add(1); gesture.active = Some((map, id.clone(), gesture.serial, center)); }
    let (_, _, stroke, plane_point) = gesture.active.as_ref().unwrap();
    let input = ObjectBrushInput { stroke: *stroke, tool: editor.tool, center, radius: editor.brush_radius,
        strength: editor.brush_strength, speed: editor.terrain_sculpt_speed, delta_seconds: time.delta_secs().min(0.05),
        plane_point: *plane_point, shape: editor.volume_brush_shape, half_height: editor.volume_brush_half_height };
    selection.brushing = false; // 只有本域可在保持整笔所有权时提交下一份有界事务。
    start(&mut session, &mut editor, &mut selection, Mutation::Brush(id, input));
    selection.brushing = true;
}
