//! 普通鼠标、键盘和精确输入都操作同一条场景时间线；插帧才提交作者历史。

use bevy::ui::{InteractionDisabled, RelativeCursorPosition};
use feathrise_editor_core::EditorKeyBinding;
use feathrise_editor_ui::{
    ActiveNumericInput, EditorButtonClick, NumericInputCommitOutcome, NumericInputCommitted,
    NumericInputDisabled, NumericValue, animation_timeline,
};
use super::*;
use crate::{
    authoring::{EditorCommand, scene_motion::SceneMotionPlayback},
    components::SceneEditorViewport,
    resources::{EditorTextInputGuard, SceneEditorNumericScrubMouseGuard},
    viewport::SelectedPlacementViewportState,
};

#[derive(SystemParam)]
pub(super) struct TimelineInput<'w, 's> {
    mouse: Res<'w, ButtonInput<MouseButton>>,
    keyboard: Option<ResMut<'w, ButtonInput<KeyCode>>>,
    text: Option<Res<'w, EditorTextInputGuard>>,
    numeric: Option<Res<'w, ActiveNumericInput>>,
    viewport: Query<'w, 's, &'static RelativeCursorPosition, With<SceneEditorViewport>>,
    lanes: Query<'w, 's, (Entity, &'static controls::Lane, &'static RelativeCursorPosition, &'static ComputedNode, &'static InheritedVisibility)>,
}

impl TimelineInput<'_, '_> {
    fn shortcut(&mut self) -> Option<SceneMotionButton> {
        if self.text.as_ref().is_some_and(|text| text.block_shortcuts)
            || self.numeric.as_ref().is_some_and(|input| input.entity.is_some())
        { return None; }
        let over_timeline = self.lanes.iter().any(|(_, _, cursor, node, visible)| visible.get() && node.size().x > 1.0 && cursor.cursor_over());
        let over_viewport = self.viewport.iter().any(RelativeCursorPosition::cursor_over);
        let keyboard = self.keyboard.as_mut()?;
        let (key, action, _) = [
            (KeyCode::KeyI, SceneMotionButton::InsertBoth, over_timeline || over_viewport),
            (KeyCode::Space, SceneMotionButton::TogglePlay, over_timeline),
            (KeyCode::Delete, SceneMotionButton::DeleteKey, over_timeline),
        ].into_iter().find(|&(key, _, over)| over
            && EditorKeyBinding { key, control: false, shift: false, alt: false }.just_pressed(keyboard))?;
        keyboard.clear_just_pressed(key);
        Some(action)
    }

    fn scrub(&self, workbench: &mut Workbench, duration: f32, track: Option<&SceneMotionTrack>) {
        if !self.mouse.pressed(MouseButton::Left) && !self.mouse.just_released(MouseButton::Left) {
            workbench.ruler_capture = None;
        }
        let Some((entity, lane, cursor, node, _)) = workbench.ruler_capture
            .and_then(|entity| self.lanes.get(entity).ok())
            .or_else(|| self.mouse.just_pressed(MouseButton::Left).then(|| self.lanes.iter().find(
                |(_, _, cursor, node, visible)| visible.get() && node.size().x > 1.0 && cursor.cursor_over())).flatten())
        else { return; };
        let Some(at) = cursor.normalized else { return; };
        let time = animation_timeline::cursor_time(0.0, duration, at.x);
        let hit = lane.0.zip(track).and_then(|(channel, track)| {
            let keys = channel.keys(track);
            let index = animation_timeline::nearest_key(keys, time, 8.0 / node.size().x * duration, |key| key.time)?;
            Some(SelectedKey { channel, time: keys[index].time })
        });
        if lane.0.is_none() {
            workbench.ruler_capture = self.mouse.pressed(MouseButton::Left).then_some(entity);
        }
        let seconds = hit.map_or_else(|| snap_time(f64::from(time)), |key| f64::from(key.time));
        workbench.seek(seconds, duration, hit);
    }
}

fn snap_time(seconds: f64) -> f64 { (seconds * FPS).round() / FPS }

fn new_track(target: &Target, timing: (f32, SceneMotionPlayback), collider: Option<crate::authoring::scene_motion::SceneMotionCollider>) -> SceneMotionTrack {
    SceneMotionTrack {
        placement_id: target.placement_id.clone(), duration_seconds: timing.0, playback: timing.1,
        position: Vec::new(), rotation: Vec::new(), collider,
    }
}

pub(super) fn input(
    context: Context,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut workbench: ResMut<Workbench>,
    mut edits: MessageReader<NumericInputCommitted>,
    fields: Query<(&SceneMotionField, &controls::FieldTarget), Without<NumericInputDisabled>>,
    clicks: Query<(&SceneMotionButton, &EditorButtonClick), (Changed<EditorButtonClick>, Without<InteractionDisabled>)>,
    numeric_mouse: Res<SceneEditorNumericScrubMouseGuard>,
    bounds: Res<SelectedPlacementViewportState>,
    mut timeline: TimelineInput,
) {
    if !context.can_edit(&editor, &session) {
        edits.clear();
        workbench.ruler_capture = None;
        return;
    }
    let Some(scene) = context.scene(&editor, &session) else { edits.clear(); return; };
    let selected = context.record(&editor, &session).map(|(_, record)| record);
    let draft = session.draft.as_ref().expect("已核对当前场景");
    let motion = &draft.motion;
    let click = (!numeric_mouse.blocked()).then(|| clicks.iter().find_map(|(button, click)| click.is_clicked().then_some(*button))).flatten();
    use SceneMotionButton::*;
    if click == Some(Open) {
        if motion.available() {
            workbench.scene = Some(scene.clone());
        } else {
            editor.status = "当前场景没有可写入动画的主场景 GLB".into();
        }
    }
    if click == Some(Close) { workbench.close(); }
    if workbench.scene.as_ref() != Some(scene) { edits.clear(); return; }
    let target = selected.map(|record| Target::new(scene, record));
    workbench.target(target.clone());
    let current = target.as_ref().and_then(|target| motion.get(&target.placement_id));
    let created = motion.tracks().next().is_some();
    let timing_before = motion.timing();
    let mut timing = timing_before;
    let action = click.or_else(|| (created && !numeric_mouse.blocked()).then(|| timeline.shortcut()).flatten());
    if created && !numeric_mouse.blocked() { timeline.scrub(&mut workbench, timing.0, current); }
    if action.is_none() && edits.is_empty() { return; }

    for edit in edits.read() {
        let Ok((field, binding)) = fields.get(edit.entity) else { continue; };
        if &binding.scene != scene || edit.outcome != NumericInputCommitOutcome::Accepted
            || !edit.changed && !edit.edited
        { continue; }
        let NumericValue::F32(value) = edit.value else { continue; };
        if !value.is_finite() { continue; }
        match field {
            SceneMotionField::Frame => workbench.seek(snap_time(f64::from(value) / FPS), timing.0, None),
            SceneMotionField::EndFrame => {
                let duration = snap_time(f64::from(value.max(1.0)) / FPS) as f32;
                let last = motion.tracks().flat_map(|track| track.position.last().into_iter().chain(track.rotation.last()))
                    .map(|key| key.time).fold(0.0, f32::max);
                if duration < last {
                    editor.status = "片段结束帧需包含最后一个关键帧".into();
                    continue;
                }
                timing.0 = duration;
                let seconds = workbench.seconds;
                workbench.seek(seconds, duration, None);
            }
            field => {
                let Some((target, record)) = target.as_ref().zip(selected) else { continue; };
                // 迟到的数值提交只能修改它当时拥有的对象和帧。
                if binding.placement.as_deref() != Some(target.placement_id.as_str())
                    || binding.frame != Some(workbench.seconds) { continue; }
                let Some(pose) = workbench.pose_for(record, scene) else { continue; };
                if let Some((channel, axis)) = field.axis() {
                    let mut axes = match channel { Channel::Position => pose.translation, Channel::Rotation => pose.rotation_degrees };
                    axes[axis] = value;
                    match channel {
                        Channel::Position => workbench.set_position(scene, &target.placement_id, axes),
                        Channel::Rotation => workbench.set_rotation(scene, &target.placement_id, axes),
                    };
                }
            }
        }
    }

    let mut replacement = None;
    if let Some(action) = action {
        let seconds = workbench.seconds;
        match action {
            Add if !created => {
                if let Some((target, collider)) = target.as_ref().zip(selected.and_then(|record| preview::initial_collision(record, scene, &bounds))) {
                    replacement = Some(new_track(target, timing, collider));
                }
            }
            First => workbench.seek(0.0, timing.0, None),
            PreviousFrame => workbench.seek(snap_time(seconds - 1.0 / FPS), timing.0, None),
            NextFrame => workbench.seek(snap_time(seconds + 1.0 / FPS), timing.0, None),
            Last => workbench.seek(f64::from(timing.0), timing.0, None),
            TogglePlay if workbench.playing => workbench.seek(snap_time(seconds), timing.0, None),
            TogglePlay => {
                if seconds >= f64::from(timing.0) { workbench.seek(0.0, timing.0, None); }
                workbench.draft = PoseDraft::default();
                workbench.elapsed = workbench.seconds;
                workbench.playing = true;
            }
            CyclePlayback => {
                timing.1 = match timing.1 {
                    SceneMotionPlayback::Once => SceneMotionPlayback::Loop,
                    SceneMotionPlayback::Loop => SceneMotionPlayback::PingPong,
                    SceneMotionPlayback::PingPong => SceneMotionPlayback::Once,
                };
                workbench.seek(seconds, timing.0, None);
            }
            InsertPosition | InsertRotation | InsertBoth if !workbench.playing => {
                let Some((target, record)) = target.as_ref().zip(selected) else { return; };
                let Some(pose) = workbench.pose_for(record, scene) else { return; };
                let mut track = if let Some(current) = current { current.clone() } else {
                    let Some(collider) = preview::initial_collision(record, scene, &bounds) else { return; };
                    new_track(target, timing, collider)
                };
                let time = seconds as f32;
                for (channel, value) in [(Channel::Position, pose.translation), (Channel::Rotation, pose.rotation_degrees)] {
                    if action == InsertBoth || action == InsertPosition && channel == Channel::Position
                        || action == InsertRotation && channel == Channel::Rotation
                    {
                        track.insert_key(channel == Channel::Rotation, SceneMotionKey { time, value });
                        match channel {
                            Channel::Position => workbench.draft.position = None,
                            Channel::Rotation => workbench.draft.rotation = None,
                        }
                        workbench.selected_key = Some(SelectedKey { channel, time });
                    }
                }
                replacement = Some(track);
            }
            DeleteKey if !workbench.playing => {
                if let Some((key, mut track)) = workbench.selected_key.take().zip(current.cloned()) {
                    let keys = key.channel.keys_mut(&mut track);
                    if let Ok(index) = keys.binary_search_by(|entry| entry.time.total_cmp(&key.time)) {
                        keys.remove(index);
                        replacement = Some(track);
                    }
                }
            }
            ResetPose => workbench.draft = PoseDraft::default(),
            _ => {}
        }
    }

    let track_change = replacement.filter(|replacement| current != Some(replacement))
        .map(|replacement| (current.cloned(), replacement));
    if track_change.is_none() && timing == timing_before { return; }
    if let Some((_, replacement)) = &track_change && let Err(reason) = replacement.validate() {
        editor.status = reason;
        return;
    }
    let session = &mut *session;
    let draft = session.draft.as_mut().expect("已核对当前场景");
    if timing != timing_before {
        draft.motion.set_timing(timing.0, timing.1);
        session.undo.push(EditorCommand::SceneMotionTimingChanged { before: timing_before, after: timing });
    }
    if let Some((before, mut track)) = track_change {
        track.duration_seconds = timing.0;
        track.playback = timing.1;
        let id = track.placement_id.clone();
        let after = Some(track);
        draft.motion.replace(&id, after.clone());
        session.undo.push(EditorCommand::SceneMotionChanged { placement_id: id, before, after });
    }
    session.redo.clear();
    session.mark_changed(&mut editor);
    editor.status = match action {
        Some(InsertPosition) => "已插入位置关键帧",
        Some(InsertRotation) => "已插入旋转关键帧",
        Some(InsertBoth) => "已插入位置和旋转关键帧",
        Some(DeleteKey) => "已删除所选关键帧",
        _ => "场景动画已修改",
    }.into();
}
