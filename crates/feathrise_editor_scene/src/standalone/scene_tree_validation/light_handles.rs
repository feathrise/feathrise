//! 真实窗口沿当前可见的光域边界按下、移动、松手，核对临时组件、数值输入以及单笔历史。

use super::*;
use crate::{components::SceneEditorViewport, viewport::placement_move::{LightHandle, PlacementMoveState, validation_drag_points}};
use feathrise_scene_content::ScenePlacementRecord;

#[derive(Resource)]
struct Stroke {
    window: Entity,
    end: Vec2,
    before: ScenePlacementRecord,
}

fn selected(world: &World) -> Result<&ScenePlacementRecord, String> {
    let id = world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref().ok_or("手柄换选丢失")?;
    world.resource::<EditorAuthoringSession>().draft.as_ref().and_then(|d| d.placements.placements.iter().find(|r| r.id == id))
        .ok_or_else(|| "找不到灯具作者记录".into())
}

pub(super) fn frame(world: &mut World, spot: bool) -> Result<(), String> {
    let record = selected(world)?;
    let origin = Vec3::from_array(record.translation);
    let forward = crate::authoring::placement_pose::placement_rotation(record) * Vec3::NEG_Z;
    let mut view = world.resource_mut::<SceneEditorViewportState>();
    view.center = origin + if spot { forward * 5.0 } else { Vec3::ZERO };
    view.extent = if spot { 35.0 } else { 40.0 };
    view.yaw = 0.7;
    view.pitch = 0.32;
    Ok(())
}

fn points(world: &mut World, handle: LightHandle, goal: f32) -> Option<(Vec2, Vec2)> {
    let (start, end) = validation_drag_points(world, handle, goal)?;
    let (node, transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<SceneEditorViewport>>().single(world).ok()?;
    let physical = |p| transform.translation + node.size() * p;
    Some((physical(start), physical(end)))
}

fn input_shows(world: &mut World, handle: LightHandle, expected: f32) -> bool {
    let field = Field::Light(handle.field());
    world.query::<(&Field, &NumericInputValue)>().iter(world).any(|(f, value)| *f == field &&
        matches!(value.0, NumericValue::F32(actual) if (actual - expected).abs() < 0.08))
}

fn scene_light_shows(world: &mut World, id: &str, handle: LightHandle, expected: f32) -> bool {
    world.query::<(&ScenePlacementInstance, Option<&PointLight>, Option<&SpotLight>)>().iter(world).any(|(instance, point, spot)| {
        if !instance.is_active() || instance.id() != id { return false; }
        let value = match handle {
            LightHandle::Range => point.map(|p| p.range).or_else(|| spot.map(|s| s.range)),
            LightHandle::Inner => spot.map(|s| s.inner_angle.to_degrees()),
            LightHandle::Outer => spot.map(|s| s.outer_angle.to_degrees()),
        };
        value.is_some_and(|v| (v - expected).abs() < 0.08)
    })
}

pub(super) fn advance(world: &mut World, spot: bool, index: usize, phase: u8, revision: &mut u64, pulse: &mut Option<Entity>) -> Result<Step, String> {
    let (handle, goal) = if !spot { (LightHandle::Range, 9.0) } else {
        [(LightHandle::Range, 14.0), (LightHandle::Inner, 15.0), (LightHandle::Outer, 32.0)][index]
    };
    let next = Step::Handles(index, phase + 1);
    if phase == 0 {
        let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world).map_err(|_| "光域手柄缺少主窗口")?;
        let (start, end) = points(world, handle, goal).ok_or_else(|| format!("当前取景未找到可拖的 {handle:?} 光域边界"))?;
        let before = selected(world)?.clone();
        world.insert_resource(Stroke { window, end, before });
        move_cursor(world, window, start.as_dvec2());
        return Ok(next);
    }
    let stroke = world.resource::<Stroke>();
    let (window, end) = (stroke.window, stroke.end);
    let id = stroke.before.id.clone();
    let before = handle.value(&stroke.before).ok_or("拖动前缺少光学值")?;
    let session = world.resource::<EditorAuthoringSession>();
    match phase {
        1 => mouse(world, window, ButtonState::Pressed),
        2 => {
            let scene = &session.draft.as_ref().unwrap().scene_id;
            if world.resource::<PlacementMoveState>().light_preview(scene, &id).is_none() {
                return Err(format!("按下可见的 {handle:?} 时未获得鼠标：{}", world.resource::<SceneEditorState>().status));
            }
            move_cursor(world, window, end.as_dvec2());
        }
        3 => {
            let scene = &session.draft.as_ref().unwrap().scene_id;
            let preview = world.resource::<PlacementMoveState>().light_preview(scene, &id).and_then(|r| handle.value(r));
            if session.revision != *revision || selected(world)? != &stroke.before || !preview.is_some_and(|v| (v - goal).abs() < 0.08) {
                return Err(format!("{handle:?} 拖动中的草稿、修订或光域预览不符，目标 {goal}，实际 {preview:?}"));
            }
            if !input_shows(world, handle, goal) || !scene_light_shows(world, &id, handle, goal) { return Ok(Step::Handles(index, phase)); }
            if !spot && index == 0 {
                key(world, KeyCode::Escape, Key::Escape, None)?;
                return Ok(Step::Handles(index, 7));
            }
            mouse(world, window, ButtonState::Released);
        }
        4..=6 => {
            if session.revision == *revision { return Ok(Step::Handles(index, phase)); }
            let expected = if phase == 5 { before } else { goal };
            if session.revision != revision.wrapping_add(1) || !handle.value(selected(world)?).is_some_and(|v| (v - expected).abs() < 0.08) {
                return Err(format!("{handle:?} 松手、撤销或重做未形成单笔作者历史"));
            }
            if !input_shows(world, handle, expected) || !scene_light_shows(world, &id, handle, expected) { return Ok(Step::Handles(index, phase)); }
            let applied = world.resource::<EditorAuthoringSession>().revision;
            if phase == 6 {
                *revision = applied;
                world.remove_resource::<Stroke>();
                // 让截图呈现三种光域本色，鼠标留在画布上会高亮刚拖过的边界。
                move_cursor(world, window, Vec2::splat(4.0).as_dvec2());
                return Ok(if spot && index < 2 { Step::Handles(index + 1, 0) } else { Step::WaitRenderer });
            }
            let action = if phase == 4 { crate::types::EditorAction::Undo } else { crate::types::EditorAction::Redo };
            // 手柄在松手后的静默帧才归还输入。真实历史按钮恢复可用后，再消费这次修订与点击。
            let Some(button) = crate::standalone::scene_format_validation::action_button(world, action) else { return Ok(Step::Handles(index, phase)); };
            *revision = applied;
            world.entity_mut(button).insert(EditorButtonClick::triggered()); *pulse = Some(button);
        }
        7 => {
            let scene = &session.draft.as_ref().unwrap().scene_id;
            if world.resource::<PlacementMoveState>().light_preview(scene, &id).is_some() || selected(world)? != &stroke.before || session.revision != *revision {
                return Err("Esc 没有取消手柄并保留原作者修订".into());
            }
            if !input_shows(world, handle, before) || !scene_light_shows(world, &id, handle, before) { return Ok(Step::Handles(index, phase)); }
            mouse(world, window, ButtonState::Released);
        }
        8 => { world.remove_resource::<Stroke>(); return Ok(Step::Handles(index + 1, 0)); }
        _ => unreachable!(),
    }
    Ok(next)
}
