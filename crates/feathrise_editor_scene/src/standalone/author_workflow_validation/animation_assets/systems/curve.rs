//! 从窗口拖出切线，核对区间采样、单笔历史、撤销和重做；主验收随后保存并从磁盘重读。
use super::*;
use bevy::math::DVec2;
use std::path::Path;

#[derive(Clone, Copy, Default, Debug)]
enum Step {
    #[default]
    Cubic,
    SelectKey,
    Toggle,
    Drag,
    Edited,
    Undo,
    Redo,
    Image,
}

#[derive(Resource, Default)]
struct Check {
    step: Step,
    drag: Option<(Entity, DVec2, DVec2, u8)>,
    before: Option<(bridge::AnimationProbe, f32)>,
    after: Option<(bridge::AnimationProbe, f32)>,
}

pub(super) fn advance(
    world: &mut World,
    state: &mut Fixture,
    source: &Path,
    id: bridge::KeyIdentity,
) -> Result<bool, String> {
    world.init_resource::<Check>();
    world.resource_scope(|world, mut check: Mut<Check>| {
        let Some(probe) = bridge::animation_probe(world, source, Some(id))? else {
            return Ok(false);
        };
        let author = world.resource::<AnimationEditorState>();
        let channel =
            &author.active_document().ok_or("缺少活动曲线")?.clips[id.clip].channels[id.channel];
        let cubic = channel.interpolation == "CUBICSPLINE";
        let next = channel
            .keys
            .get(id.key + 1)
            .ok_or("曲线验收需要所选键的后继关键帧")?;
        let midpoint = channel.sample((probe.key.time + next.time) * 0.5)[0];
        match check.step {
            Step::Cubic => {
                if cubic && state.click.is_none() {
                    check.step = Step::SelectKey;
                } else {
                    click_animation_button(
                        world,
                        state,
                        bridge::AnimationCommand::CycleInterpolation,
                    )?;
                }
            }
            Step::SelectKey => {
                if probe.key_selected && state.click.is_none() {
                    check.step = Step::Toggle;
                } else {
                    click_animation_button(world, state, bridge::AnimationCommand::NextKey)?;
                }
            }
            Step::Toggle => {
                if click_window_button(world, state, "OpenCurve", |w| {
                    bridge::curve_control(w, false)
                })? {
                    check.step = Step::Drag;
                }
            }
            Step::Drag => {
                let Some(grip) = bridge::curve_control(world, true) else {
                    return Ok(false);
                };
                let point = world
                    .get::<UiGlobalTransform>(grip)
                    .ok_or("缺少真实手柄坐标")?
                    .translation
                    .as_dvec2();
                let direction = if let Some(graph) = world
                    .get::<ChildOf>(grip)
                    .and_then(|p| world.get::<UiGlobalTransform>(p.parent()))
                {
                    if point.y < f64::from(graph.translation.y) {
                        1.0
                    } else {
                        -1.0
                    }
                } else {
                    return Err("切线手柄缺少曲线画布".into());
                };
                if check.drag.is_none() {
                    check.before = Some((probe, midpoint));
                }
                let window = world
                    .query_filtered::<Entity, With<PrimaryWindow>>()
                    .single(world)
                    .map_err(|e| e.to_string())?;
                if crate::ui::file_manager_validation::drag(
                    world,
                    &mut check.drag,
                    window,
                    Some(grip),
                    point + DVec2::Y * 20.0 * direction,
                ) {
                    check.step = Step::Edited;
                }
            }
            Step::Edited => {
                let (before, sample) = check.before.as_ref().unwrap();
                if probe.cursor == before.cursor || !bridge::input_ready(world) {
                    return Ok(false);
                }
                if probe.cursor != before.cursor + 1
                    || probe.history_len != before.history_len + 1
                    || probe.key.out_tangent[0] == before.key.out_tangent[0]
                    || probe.key.out_tangent[1..] != before.key.out_tangent[1..]
                    || midpoint == *sample
                    || probe.key.time != before.key.time
                    || probe.key.value != before.key.value
                    || probe.key.in_tangent != before.key.in_tangent
                {
                    return Err(format!(
                        "窗口拖动曲线未精确修改一笔出切线：{:?} → {:?}, 采样 {sample} → {midpoint}",
                        before.key, probe.key
                    ));
                }
                check.after = Some((probe, midpoint));
                state.pulse = Some(command(world, state, bridge::AnimationCommand::Undo)?);
                check.step = Step::Undo;
            }
            Step::Undo | Step::Redo => {
                let undo = matches!(check.step, Step::Undo);
                let (expected, sample) = if undo {
                    check.before.as_ref()
                } else {
                    check.after.as_ref()
                }
                .unwrap();
                if probe.cursor != expected.cursor {
                    return Ok(false);
                }
                if probe.key != expected.key || midpoint != *sample {
                    return Err("撤销或重做未恢复完整曲线及中间采样".into());
                }
                if undo {
                    state.pulse = Some(command(world, state, bridge::AnimationCommand::Redo)?);
                    check.step = Step::Redo;
                } else {
                    let (before, initial_sample) = check.before.as_ref().unwrap();
                    trace(state, json!({"curve_handle_drag": {
                        "x_midpoint_before": initial_sample, "x_midpoint_after": midpoint,
                        "history_cursor_before": before.cursor, "history_cursor_after": probe.cursor,
                        "undo_and_redo_restored_samples": true
                    }}));
                    state.curve_edit = Some(probe);
                    capture_mode(world, "curve-tangents.png")?;
                    check.step = Step::Image;
                }
            }
            Step::Image => return mode_captured(world),
        }
        Ok(false)
    })
}
