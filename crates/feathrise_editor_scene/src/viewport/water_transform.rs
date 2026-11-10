//! 静水在左侧“移动”工具中使用世界Y轴手柄；实时水位和一笔撤销沿用数字框作者事务。

use super::{
    LayerCandidateState, LayerTarget, SelectedWaterViewportState,
    modes::{ViewportMode, ViewportModeState},
    placement_move::{MoveAxis, MoveCamera, MoveHandle, MoveInputContext, PlacementMoveState, handle_point, handle_translation_mode, axis_colors},
};
use crate::{
    authoring::{EditorAuthoringSession, WaterLevelScrubState, abort_water_level_scrub, begin_water_level_scrub, finish_water_level_scrub, set_water_level},
    components::SceneEditorViewport,
    constants::{MAX_WATER_LEVEL_METERS, MIN_WATER_LEVEL_METERS},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    types::{WaterAuthoringFeatureKind, WaterLevelSetRequest},
    ui::terrain_objects::TerrainObjectsEditorState,
};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use feathrise_author_tools::{AuthorTool, TransformPreview};
use crate::viewport::tool_preview::AuthorToolFrame;

mod identity;
use identity::{Source, origin, selected_layer};

struct Drag {
    source: Source,
    scrub: WaterLevelScrubState, // 视口独立事务，不能借用数字框全局scrub的收口或取消信号。
    id: String,
    label: String,
    selection_revision: u64,
    camera: MoveCamera,
    size: Vec2,
    origin: Vec3,
    point: Vec3,
    cursor: Vec2,
    snap: feathrise_editor_core::EditorSnapSettings,
}

#[derive(Resource, Default)]
struct WaterMoveState {
    drag: Option<Drag>,
    hover: bool, // 只表示本帧可写Y轴命中；不取得公共鼠标门，也不创建水位事务。
    releasing: bool,
    continuation: Option<(String, u64, u64)>, // 本次写入前修订与真实换选票据，随后定点续接选择身份。
}

/// 水位临时值由本工具私有事务持有；快照无需读取或改变其鼠标释放门。
pub(crate) fn transaction_active(world: &World) -> bool {
    world.get_resource::<WaterMoveState>().is_some_and(|state| state.drag.is_some())
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<WaterMoveState>()
        .add_systems(Update, (input, continue_selection).chain()
            .in_set(crate::SceneEditorSystemSet::Input)
            .after(crate::authoring::placement_batch::sync_batch_selection)
            .before(super::placement_move::handle_placement_move)
            .before(super::control_editor_viewport))
        .add_systems(Update, draw.in_set(crate::SceneEditorSystemSet::Presentation));
}

/// 自己的水位换代不冒充用户重新点选；外部换选或跨画布仍保留原票据失效语义。
fn continue_selection(
    mut state: ResMut<WaterMoveState>, session: Res<EditorAuthoringSession>,
    mut candidates: ResMut<LayerCandidateState>,
) {
    if let Some((id, before, ticket)) = state.continuation.take() {
        candidates.continue_water_level_revision(&session, &id, before, ticket);
    }
}

fn cancel(editor: &mut SceneEditorState, session: &mut EditorAuthoringSession, drag: &mut Drag, reason: &str) {
    if drag.source.matches(session, &drag.id) && drag.scrub.before != drag.scrub.after {
        abort_water_level_scrub(editor, session, &mut drag.scrub, reason);
    } else {
        editor.status = reason.into(); // 换画布或来源后绝不按相同map/layer ID回滚新文档。
    }
}

#[allow(clippy::too_many_arguments)]
fn input(
    mut editor: ResMut<SceneEditorState>, mut session: ResMut<EditorAuthoringSession>,
    terrains: Res<TerrainObjectsEditorState>, mode: Res<ViewportModeState>, context: MoveInputContext,
    mut state: ResMut<WaterMoveState>, mut ownership: ResMut<PlacementMoveState>,
    selected: Res<SelectedWaterViewportState>, viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<(&ComputedNode, &RelativeCursorPosition), With<SceneEditorViewport>>,
    mouse: Res<ButtonInput<MouseButton>>, mut keys: ResMut<ButtonInput<KeyCode>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
) {
    state.hover = false; // 每帧先清空，UI遮挡、失焦、换选和静默释放均不能保留旧高亮。
    let down = mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle]);
    let released = [MouseButton::Left, MouseButton::Right, MouseButton::Middle].into_iter()
        .any(|button| mouse.just_released(button));
    if state.releasing {
        if !down && !released { state.releasing = false; }
        return;
    }
    if state.drag.is_some() && keys.just_pressed(KeyCode::F12) {
        editor.toggle_close_request();
        keys.clear_just_pressed(KeyCode::F12); // 移动工具随后消费键盘边沿，关闭意图必须先明确收口。
    }
    let camera_cursor = viewport.as_deref().zip(nodes.single().ok()).and_then(|(viewport, (node, cursor))| {
        MoveCamera::new(&editor, viewport, node.inverse_scale_factor())
            .map(|camera| (camera, cursor, node.size(), viewport.framed_scene_id.as_ref()))
    });
    let available = context.pointer_available(&editor) && mode.mode == ViewportMode::Move
        && !session.saving && !session.vegetation_cross_busy;
    if let Some(mut drag) = state.drag.take() {
        ownership.capture_mouse(); // 拖动、取消与release帧始终先于相机和选择取得公共门。
        let escaped = keys.just_pressed(KeyCode::Escape);
        let chord = mouse.any_pressed([MouseButton::Right, MouseButton::Middle]);
        let target_valid = drag.source.matches(&session, &drag.id)
            && session.revision == drag.scrub.expected_revision
            && context.candidates.selection_revision == drag.selection_revision
            && selected_layer(&editor, &session, &context.candidates, &terrains)
                .is_some_and(|layer| layer.id == drag.id && layer.water_level == drag.scrub.after);
        // 已受理保存先把当前有效预览合并入历史，再让原保存系统冻结同一作者修订。
        if editor.save_requested && target_valid && editor.open && !editor.close_requested
            && !session.saving && !escaped && !chord {
            finish_water_level_scrub(&mut editor, &mut session, &mut drag.scrub, &drag.label);
            state.releasing = down || released;
            return;
        }
        let current = available && target_valid && camera_cursor.as_ref().is_some_and(|(camera, cursor, size, map)|
            *camera == drag.camera && *size == drag.size && cursor.normalized.is_some()
                && *map == session.draft.as_ref().map(|draft| &draft.scene_id));
        if !current || escaped || chord {
            cancel(&mut editor, &mut session, &mut drag, "已取消静水体高度移动");
            state.releasing = down || released;
            return;
        }
        let normalized = camera_cursor.as_ref().unwrap().1.normalized.unwrap();
        let moved = ((normalized - drag.cursor) * drag.camera.size * drag.camera.inverse_scale).length_squared() > 4.0;
        let value = if moved {
            drag.camera.ray(normalized).and_then(|ray| handle_point(ray, drag.origin, MoveHandle::Axis(MoveAxis::Y)))
                .and_then(|point| {
                    let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
                    handle_translation_mode(drag.origin, point - drag.point, MoveHandle::Axis(MoveAxis::Y),
                        if drag.snap.active(control) { drag.snap.translation_meters } else { 0.0 },
                        drag.snap.mode, Vec2::ZERO, Vec2::ZERO)
                }).map(|position| {
                    let up_y = drag.source.frame.transform_vector3(Vec3::Y).y;
                    (drag.scrub.before + (position.y - drag.origin.y) / up_y)
                        .clamp(MIN_WATER_LEVEL_METERS, MAX_WATER_LEVEL_METERS)
                })
        } else { Some(drag.scrub.before) }; // 回到抓取起点必须恢复未量化raw值，不能留下上一次预览。
        let Some(value) = value.filter(|value| value.is_finite()) else {
            cancel(&mut editor, &mut session, &mut drag, "Y轴与视线接近平行，已取消静水体高度移动");
            state.releasing = down || released;
            return;
        };
        let request = WaterLevelSetRequest { target: drag.scrub.target.as_ref().unwrap().clone(), water_level: value };
        let result = session.draft.as_mut().and_then(|draft| draft.water.as_mut())
            .ok_or_else(|| "当前静水文档已失效".to_owned()).and_then(|water| set_water_level(water, &request));
        match result {
            Ok((_, before, after)) if before != after => {
                let before_revision = session.revision;
                session.mark_water_definition_changed(&mut editor); // 沿用既有single-flight，不新增后台槽或R8重采样。
                drag.scrub.after = after;
                drag.scrub.expected_revision = session.revision;
                state.continuation = Some((drag.id.clone(), before_revision, drag.selection_revision));
                editor.status = format!("正在移动静水体“{}”高度：{after:.3}m；Esc 取消", drag.label);
            }
            Ok(_) => {}
            Err(error) => {
                cancel(&mut editor, &mut session, &mut drag, &error);
                state.releasing = down || released;
                return;
            }
        }
        if mouse.just_released(MouseButton::Left) {
            finish_water_level_scrub(&mut editor, &mut session, &mut drag.scrub, &drag.label);
            state.releasing = true;
        } else if mouse.pressed(MouseButton::Left) {
            state.drag = Some(drag);
        } else {
            cancel(&mut editor, &mut session, &mut drag, "静水体高度拖动丢失按键，已取消");
            state.releasing = down || released;
        }
        return;
    }
    if !available || editor.save_requested || guard.blocked() || ownership.owns_mouse()
        || context.pointer_over_ui()
        || mouse.any_pressed([MouseButton::Right, MouseButton::Middle]) {
        return;
    }
    let Some(layer) = selected_layer(&editor, &session, &context.candidates, &terrains) else { return; };
    let (id, label, level) = (layer.id.clone(), layer.label.clone(), layer.water_level);
    let draft = session.draft.as_ref().unwrap();
    let Some(source) = Source::capture(draft, &id) else { return; };
    let Some(origin) = origin(&session, &selected, &id, level, source.frame) else { return; };
    let Some((camera, cursor, size, _)) = camera_cursor.filter(|(_, cursor, _, map)|
        cursor.cursor_over() && *map == Some(&draft.scene_id)) else { return; };
    let Some(normalized) = cursor.normalized else { return; };
    if !camera.picks_axis(origin, normalized, MoveAxis::Y) { return; }
    state.hover = true;
    if !mouse.just_pressed(MouseButton::Left) { return; } // 悬停只投影显示；沿原按下边沿才开始作者事务。
    let Some(point) = camera.ray(normalized).and_then(|ray| handle_point(ray, origin, MoveHandle::Axis(MoveAxis::Y))) else { return; };
    let mut scrub = WaterLevelScrubState::default();
    if let Err(reason) = begin_water_level_scrub(&editor, &session, &mut scrub) {
        editor.status = reason;
        return;
    }
    state.drag = Some(Drag { source, scrub, id, label,
        selection_revision: context.candidates.selection_revision, camera, size, origin, point,
        cursor: normalized, snap: context.snap_settings() });
    ownership.capture_mouse();
    editor.status = "正在拖动静水体高度 Y；Ctrl 切换吸附，Esc 取消".into();
}

#[allow(clippy::too_many_arguments)]
fn draw(
    editor: Res<SceneEditorState>, session: Res<EditorAuthoringSession>, terrains: Res<TerrainObjectsEditorState>,
    candidates: Res<LayerCandidateState>, mode: Res<ViewportModeState>, state: Res<WaterMoveState>,
    selected: Res<SelectedWaterViewportState>, viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<&ComputedNode, With<SceneEditorViewport>>, mut tools: ResMut<AuthorToolFrame>,
) {
    if !editor.open || editor.close_requested || mode.mode != ViewportMode::Move { return; }
    let Some(layer) = selected_layer(&editor, &session, &candidates, &terrains) else { return; };
    let point = if let Some(drag) = state.drag.as_ref() {
        if !drag.source.matches(&session, &layer.id) { return; }
        drag.origin + drag.source.frame.transform_vector3(Vec3::Y) * (layer.water_level - drag.scrub.before)
    } else {
        let Some(source) = Source::capture(session.draft.as_ref().unwrap(), &layer.id) else { return; };
        let Some(point) = origin(&session, &selected, &layer.id, layer.water_level, source.frame) else { return; };
        point
    };
    let (Some(viewport), Ok(node)) = (viewport, nodes.single()) else { return; };
    let Some(camera) = MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()) else { return; };
    let Some(length) = camera.handle_length(point) else { return; };
    if camera.axis_segment(point, MoveAxis::Y, length).is_some() {
        tools.push(AuthorTool::Transform(TransformPreview::Move {
            spatial: false,
            origin: point.to_array(), length, axes: 0b010,
            active_axes: if state.drag.is_some() || state.hover { 0b010 } else { 0 }, plane: false,
            plane_active: false, plane_cross: false, plane_color: MoveAxis::Y.color().to_linear().to_f32_array(),
            color_active: Color::srgb(0.92, 0.72, 0.40).to_linear().to_f32_array(), axis_colors: axis_colors(),
            anchor: None, marker_radius: None,
        }));
    } // 只画可写的Y轴，X/Z移动和旋转缩放不能暗中改作者水位或覆盖画布。
}

#[cfg(test)]
#[path = "water_transform/tests.rs"]
mod tests;
