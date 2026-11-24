//! 普通鼠标、键盘和精确输入都操作同一条场景时间线；插帧才提交作者历史。

use bevy::{clipboard::Clipboard, ui::{InteractionDisabled, RelativeCursorPosition}};
use feathrise_editor_core::EditorKeyBinding;
use feathrise_editor_ui::{
    ActiveNumericInput, EditorButtonClick, NumericInputCommitOutcome, NumericInputCommitted,
    NumericInputDisabled, NumericValue, animation_timeline,
};
use super::*;
use crate::{
    authoring::{AuthoringRetirement, EditorCommand, scene_motion::{DEFAULT_SCENE_ANIMATION_NAME, SceneMotionPlayback, SceneMotionTiming}},
    components::SceneEditorViewport,
    resources::{EditorTextInputGuard, SceneEditorNumericScrubMouseGuard},
    ui::scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectKey},
    viewport::SelectedPlacementViewportState,
};

#[derive(SystemParam)]
pub(super) struct TimelineInput<'w, 's> {
    mouse: Res<'w, ButtonInput<MouseButton>>,
    keyboard: Option<ResMut<'w, ButtonInput<KeyCode>>>,
    text: Option<Res<'w, EditorTextInputGuard>>,
    numeric: Option<Res<'w, ActiveNumericInput>>,
    viewport: Query<'w, 's, &'static RelativeCursorPosition, With<SceneEditorViewport>>,
    timeline: Query<'w, 's, (&'static RelativeCursorPosition, &'static InheritedVisibility), With<controls::TimelineHitArea>>,
    lanes: Query<'w, 's, (Entity, &'static controls::Lane, &'static RelativeCursorPosition, &'static ComputedNode, &'static InheritedVisibility)>,
}

impl TimelineInput<'_, '_> {
    fn additive_track_selection(&self) -> bool {
        self.keyboard.as_ref().is_some_and(|keys| keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]))
    }

    fn shortcut(&mut self) -> Option<SceneMotionButton> {
        if self.text.as_ref().is_some_and(|text| text.block_shortcuts)
            || self.numeric.as_ref().is_some_and(|input| input.entity.is_some())
        { return None; }
        // 标题也属于时间轴。点选轨道后可以在原处按 I，无需把指针移进格线。
        let over_timeline = self.timeline.iter().any(|(cursor, visible)| visible.get() && cursor.cursor_over());
        let over_viewport = self.viewport.iter().any(RelativeCursorPosition::cursor_over);
        let keyboard = self.keyboard.as_mut()?;
        let (key, action, _) = [
            (KeyCode::KeyI, SceneMotionButton::InsertKey, over_timeline || over_viewport),
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

fn new_track(target: &Target, timing: SceneMotionTiming, collider: Option<crate::authoring::scene_motion::SceneMotionCollider>) -> SceneMotionTrack {
    SceneMotionTrack {
        placement_id: target.placement_id.clone(), duration_seconds: timing.duration_seconds, playback: timing.playback,
        position: Vec::new(), rotation: Vec::new(), scale: Vec::new(), collider,
    }
}

pub(super) fn input(
    context: Context,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut workbench: ResMut<Workbench>,
    mut retirement: ResMut<AuthoringRetirement>,
    mut edits: MessageReader<NumericInputCommitted>,
    mut tree_opens: MessageReader<OpenSceneAnimation>,
    menu: Res<SceneContextMenuState>,
    fields: Query<(&SceneMotionField, &controls::FieldTarget), Without<NumericInputDisabled>>,
    clicks: Query<(&SceneMotionButton, &EditorButtonClick), (Changed<EditorButtonClick>, Without<InteractionDisabled>)>,
    numeric_mouse: Res<SceneEditorNumericScrubMouseGuard>,
    bounds: Res<SelectedPlacementViewportState>,
    mut timeline: TimelineInput,
    mut clipboard: ResMut<Clipboard>,
) {
    if !context.can_edit(&editor, &session) {
        edits.clear();
        tree_opens.clear();
        workbench.ruler_capture = None;
        return;
    }
    let Some(scene) = context.scene(&editor, &session) else {
        edits.clear();
        tree_opens.clear();
        return;
    };
    let draft = session.draft.as_ref().expect("已核对当前场景");
    let motion = &draft.motion;
    let can_click = !numeric_mouse.blocked();
    let click = can_click.then(|| clicks.iter().find_map(|(button, click)| click.is_clicked().then_some(*button))).flatten();
    let open_from_tree = tree_opens.read().find_map(|request| {
        if !can_click || !request.0.current(&editor, &session) { return None; }
        match &request.0.key {
            SceneObjectKey::MainSceneAnimation(id) => Some(id.as_str()),
            _ => None,
        }
    });
    use SceneMotionButton::*;
    let menu_action = menu.ready.as_ref().filter(|request| can_click
        && request.target.current(&editor, &session) && request.target.allows(request.action));
    let create = menu_action.is_some_and(|request| request.action == SceneMenuAction::NewSceneAnimation)
        || (click == Some(Add) && workbench.scene.as_ref() == Some(scene));
    let duration_edit = if let Some(clip_id) = motion.active_clip_id()
        .filter(|id| workbench.scene.as_ref() == Some(scene) && workbench.clip_id.as_deref() == Some(*id))
    {
        let before = motion.timing();
        let mut after = before;
        // 先处理失焦已提交的字段，再切换工作台；旧字段始终属于原片段。
        for edit in edits.read() {
            let Ok((field, binding)) = fields.get(edit.entity) else { continue; };
            if &binding.0 != scene || binding.1 != clip_id || edit.outcome != NumericInputCommitOutcome::Accepted
                || !edit.changed && !edit.edited
            { continue; }
            let NumericValue::F32(value) = edit.value else { continue; };
            if !value.is_finite() { continue; }
            match field {
                SceneMotionField::Frame => workbench.seek(snap_time(f64::from(value) / FPS), after.duration_seconds, None),
                SceneMotionField::EndFrame => {
                    let duration = snap_time(f64::from(value.max(1.0)) / FPS) as f32;
                    let last = motion.tracks().flat_map(|track| [track.position.last(), track.rotation.last(), track.scale.last()].into_iter().flatten())
                        .map(|key| key.time).fold(0.0, f32::max);
                    if duration < last {
                        editor.status = "片段结束帧需包含最后一个关键帧".into();
                        continue;
                    }
                    after.duration_seconds = duration;
                    let seconds = workbench.seconds;
                    workbench.seek(seconds, duration, None);
                }
            }
        }
        (before != after).then(|| (clip_id.to_owned(), before, after))
    } else { edits.clear(); None };
    if let Some((clip_id, before, after)) = duration_edit {
        let session = &mut *session;
        session.draft.as_mut().expect("已核对当前场景").motion.set_timing(after);
        session.undo.push(EditorCommand::SceneMotionTimingChanged { clip_id, before, after });
        retirement.push(std::mem::take(&mut session.redo));
        session.mark_changed(&mut editor);
        editor.status = "场景动画已修改".into();
    }

    let edit = if create {
        Some(session.new_scene_motion_clip(DEFAULT_SCENE_ANIMATION_NAME).map(|id| {
            let draft = session.draft.as_mut().expect("已核对当前场景");
            draft.motion.select_clip(&id);
            workbench.open(&draft.scene_id, Some(&id));
            "已新建场景动画"
        }))
    } else if let Some(request) = menu_action
        && let SceneObjectKey::MainSceneAnimation(id) = &request.target.key
    {
        match request.action {
            SceneMenuAction::Rename if request.name != request.target.name =>
                Some(session.rename_scene_motion_clip(id, &request.name).map(|()| "已改动画名称")),
            SceneMenuAction::Copy => {
                let active = session.draft.as_ref().and_then(|draft| draft.motion.active_clip_id()).map(str::to_owned);
                Some(session.duplicate_scene_motion_clip(id).map(|_| {
                    // 复制只生成新片段，继续在当前工作台上编辑原片段。
                    if let Some(active) = active {
                        session.draft.as_mut().expect("已核对当前场景").motion.select_clip(&active);
                    }
                    "已复制场景动画"
                }))
            }
            SceneMenuAction::Delete => Some(session.remove_scene_motion_clip(id).map(|()| {
                if workbench.clip_id.as_deref() == Some(id.as_str()) { workbench.close(); }
                "已删除场景动画"
            })),
            _ => None,
        }
    } else { None };
    if let Some(result) = edit {
        match result {
            Ok(status) => {
                retirement.push(std::mem::take(&mut session.redo));
                session.mark_changed(&mut editor);
                editor.status = status.into();
            }
            Err(reason) => editor.status = reason,
        }
        return;
    }

    if click == Some(CopyId) {
        let draft = session.draft.as_ref().expect("已核对当前场景");
        // 属性栏复制当前树行的稳定 ID，与时间轴正在编辑的片段无关。
        if let Some(id) = context.browser.selected_scene_animation(&draft.scene_id)
            .filter(|id| draft.motion.clip_name_for(id).is_some())
        {
            editor.status = match clipboard.set_text(id) {
                Ok(()) => "已复制动画 ID".into(),
                Err(error) => format!("复制动画 ID 失败：{error}"),
            };
        }
        return;
    }
    if click == Some(Open) || open_from_tree.is_some() {
        let draft = session.draft.as_mut().expect("已核对当前场景");
        if draft.motion.available() {
            let id = open_from_tree.or_else(|| context.browser.selected_scene_animation(&draft.scene_id));
            if let Some(id) = id && !draft.motion.select_clip(id) { return; }
            workbench.open(&draft.scene_id, draft.motion.active_clip_id());
        } else {
            editor.status = "当前场景没有可写入动画的主场景 GLB".into();
        }
    }
    if click == Some(Close) { workbench.close(); }
    let Some(scene) = context.scene(&editor, &session).filter(|scene| workbench.scene.as_ref() == Some(*scene))
        else { return; };
    let selected = context.record(&editor, &session).map(|(_, record)| record);
    let motion = &session.draft.as_ref().expect("已核对当前场景").motion;
    let Some(clip_id) = motion.active_clip_id().filter(|id| workbench.clip_id.as_deref() == Some(*id)) else { return; };
    let target = selected.map(|record| Target::new(scene, record));
    workbench.target(target.clone());
    let current = target.as_ref().and_then(|target| motion.get(&target.placement_id));
    let overview = workbench.shows_objects(selected.is_some());
    let timing_before = motion.timing();
    let mut timing = timing_before;
    let action = click.or_else(|| can_click.then(|| timeline.shortcut()).flatten());
    if can_click { timeline.scrub(&mut workbench, timing.duration_seconds, current.filter(|_| !overview)); }
    if action.is_none() { return; }

    let mut replacement = None;
    if let Some(action) = action {
        let seconds = workbench.seconds;
        let duration = timing.duration_seconds;
        match action {
            ShowObjects => {
                workbench.show_objects = true;
                workbench.selected_key = None;
            }
            PreviousObjects if overview => workbench.object_page = super::overview::page(
                workbench.object_page, motion.keyed_track_count(),
            ).saturating_sub(1),
            NextObjects if overview => workbench.object_page = super::overview::page(
                workbench.object_page.saturating_add(1), motion.keyed_track_count(),
            ),
            First => workbench.seek(0.0, duration, None),
            PreviousFrame => workbench.seek(snap_time(seconds - 1.0 / FPS), duration, None),
            NextFrame => workbench.seek(snap_time(seconds + 1.0 / FPS), duration, None),
            Last => workbench.seek(f64::from(duration), duration, None),
            TogglePlay if workbench.playing => workbench.seek(snap_time(seconds), duration, None),
            TogglePlay => {
                if seconds >= f64::from(duration) { workbench.seek(0.0, duration, None); }
                workbench.draft = PoseDraft::default();
                workbench.elapsed = workbench.seconds;
                workbench.playing = true;
            }
            CyclePlayback => {
                timing.playback = match timing.playback {
                    SceneMotionPlayback::Once => SceneMotionPlayback::Loop,
                    SceneMotionPlayback::Loop => SceneMotionPlayback::PingPong,
                    SceneMotionPlayback::PingPong => SceneMotionPlayback::Once,
                };
                workbench.seek(seconds, duration, None);
            }
            ToggleAutoPlay => timing.auto_play = !timing.auto_play,
            SelectPosition if !overview => workbench.select_track(Channel::Position, timeline.additive_track_selection()),
            SelectRotation if !overview => workbench.select_track(Channel::Rotation, timeline.additive_track_selection()),
            SelectScale if !overview => workbench.select_track(Channel::Scale, timeline.additive_track_selection()),
            InsertKey if !overview && !workbench.playing && workbench.has_keying_tracks() => {
                let Some((target, record)) = target.as_ref().zip(selected) else { return; };
                let Some(pose) = workbench.pose_for(record, scene) else { return; };
                let mut track = if let Some(current) = current { current.clone() } else {
                    let Some(collider) = preview::initial_collision(record, scene, &bounds) else { return; };
                    new_track(target, timing, collider)
                };
                let time = seconds as f32;
                for (channel, value) in [(Channel::Position, pose.translation), (Channel::Rotation, pose.rotation_degrees), (Channel::Scale, pose.scale)] {
                    if workbench.keys_channel(channel) {
                        SceneMotionTrack::insert_key(channel.keys_mut(&mut track), SceneMotionKey { time, value });
                        match channel {
                            Channel::Position => workbench.draft.position = None,
                            Channel::Rotation => workbench.draft.rotation = None,
                            Channel::Scale => workbench.draft.scale = None,
                        }
                        workbench.selected_key = Some(SelectedKey { channel, time });
                    }
                }
                replacement = Some(track);
            }
            DeleteKey if !overview && !workbench.playing => {
                if let Some((key, mut track)) = workbench.selected_key.take().zip(current.cloned()) {
                    let keys = key.channel.keys_mut(&mut track);
                    if let Ok(index) = keys.binary_search_by(|entry| entry.time.total_cmp(&key.time)) {
                        keys.remove(index);
                        replacement = Some(track);
                    }
                }
            }
            ResetPose if !overview => workbench.draft = PoseDraft::default(),
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
    let clip_id = clip_id.to_owned();
    let session = &mut *session;
    let draft = session.draft.as_mut().expect("已核对当前场景");
    if timing != timing_before {
        draft.motion.set_timing(timing);
        session.undo.push(EditorCommand::SceneMotionTimingChanged { clip_id: clip_id.clone(), before: timing_before, after: timing });
    }
    if let Some((before, mut track)) = track_change {
        track.duration_seconds = timing.duration_seconds;
        track.playback = timing.playback;
        let id = track.placement_id.clone();
        let after = (!track.is_empty()).then_some(track);
        draft.motion.replace(&id, after.clone());
        session.undo.push(EditorCommand::SceneMotionChanged { clip_id, placement_id: id, before, after });
    }
    retirement.push(std::mem::take(&mut session.redo));
    session.mark_changed(&mut editor);
    editor.status = match action {
        Some(InsertKey) => "已为所选轨道插入关键帧",
        Some(DeleteKey) => "已删除所选关键帧",
        Some(ToggleAutoPlay) if timing.auto_play => "启动时自动播放场景动画",
        Some(ToggleAutoPlay) => "场景动画已设为手动播放",
        _ => "场景动画已修改",
    }.into();
}
