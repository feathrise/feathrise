//! 播放、时间轴定位与关键帧事务；固定槽身份在输入前复验，数值一次提交形成一条历史。

use super::super::{AssetPreviewWorkspace, DocumentState, types::AssetPreviewState};
use super::{components::*, context::AnimationContext, data::Keyframe, resources::*};
use bevy::{input::mouse::MouseWheel, prelude::*};
use feathrise_editor_ui::{
    EditorButtonClick, NumericInputCommitOutcome, NumericInputCommitted, NumericValue,
};

pub(super) fn input(
    context: AnimationContext,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    documents: Res<DocumentState>,
    mut wheels: MessageReader<MouseWheel>,
    mut commits: MessageReader<NumericInputCommitted>,
    mut state: ResMut<AnimationEditorState>,
    buttons: Query<(&AnimationButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    channels: Query<(&AnimationChannelButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    fields: Query<&AnimationNumericInput>,
) {
    state.mouse_owned = false;
    state.wheel_owned = false;
    let _wheel = wheels.read().any(|event| {
        context
            .windows
            .single()
            .ok()
            .is_some_and(|(id, _)| id == event.window)
    });
    let available = context.available(&preview, &media)
        && state.visible
        && context.source_matches(&state, &preview, &media, documents.asset_root());
    if available
        && context
            .numeric
            .as_ref()
            .and_then(|n| n.entity)
            .or_else(|| context.numeric_text.as_ref().and_then(|n| n.entity))
            .is_some_and(|entity| fields.contains(entity))
    {
        state.playing = false;
    }
    // 消息始终消费。模态、切源或后台任务期间的旧数字提交不得在恢复后重放。
    for event in commits.read() {
        if !available
            || state.busy()
            || !state.editable()
            || event.outcome != NumericInputCommitOutcome::Accepted
            || !event.changed
        {
            continue;
        }
        let Ok(field) = fields.get(event.entity) else {
            continue;
        };
        if field.binding != state.revision {
            continue;
        }
        let NumericValue::F32(value) = event.value else {
            continue;
        };
        if !value.is_finite() {
            continue;
        }
        if let AnimationNumericField::Value(axis) = field.field {
            if let Some(s) = state.current_mut()
                && let Some(channel) = s.channel.map(|i| &s.document.clips[s.clip].channels[i])
            {
                let node = channel.node;
                let mut pose = s
                    .pose
                    .filter(|(n, _)| *n == node)
                    .map_or_else(|| super::pose::local_at(s, node), |(_, t)| t);
                match channel.path.as_str() {
                    "translation" if axis < 3 => pose.translation[axis] = value,
                    "scale" if axis < 3 => pose.scale[axis] = value,
                    "rotation" if axis < 3 => {
                        let (x, y, z) = pose.rotation.to_euler(EulerRot::XYZ);
                        let mut angles = [x, y, z];
                        angles[axis] = value.to_radians();
                        pose.rotation =
                            Quat::from_euler(EulerRot::XYZ, angles[0], angles[1], angles[2]);
                    }
                    _ => continue,
                }
                s.pose = Some((node, pose));
                state.playing = false;
                state.changed();
                if state.auto_key {
                    action(&mut state, AnimationAction::InsertPose);
                }
            }
            continue;
        }
        if let Some(edit) = numeric_edit(&state, field.field, value) {
            state.pending = Some(PendingAction::Edit(edit));
        }
    }
    if !available {
        state.mouse_owned = state.drag.take().is_some() || state.gesture.take().is_some();
        return;
    }
    if context.guard.blocked()
        || context.numeric.as_ref().is_some_and(|s| s.entity.is_some())
        || context
            .numeric_text
            .as_ref()
            .is_some_and(|s| s.entity.is_some())
        || context
            .map_drag
            .as_ref()
            .is_some_and(|s| s.blocks_other_mouse() || s.press.is_some())
        || context.text.as_ref().is_some_and(|s| s.block_shortcuts)
    {
        return;
    }
    if state.in_place_rig_preview() && buttons.iter().any(|(button, click)| {
        button.action == AnimationAction::UnbindModel
            && click.is_clicked() && button.binding == state.revision
    }) {
        // 结束预览只撤销当前意图，旧读取会在原任务槽结束后退役。
        state.stop_rig_preview();
        state.mouse_owned = true;
        return;
    }
    if state.busy() {
        return;
    }
    for (button, click) in &buttons {
        if click.is_clicked() && button.binding == state.revision {
            if button.action == AnimationAction::BindSelectedModel {
                if state.in_place_rig_preview() {
                    continue;
                }
                if let Some(path) = context
                    .editor
                    .selected_asset
                    .as_ref()
                    .filter(|p| p.to_lowercase().ends_with(".glb"))
                {
                    if let Some(session) = state.current_mut() {
                        session.model_path = Some(path.clone());
                        session.retarget_model = false;
                    }
                    state.model_status = "正在检查所选模型的骨架兼容性".into();
                    state.model_binding_revision = state.model_binding_revision.wrapping_add(1);
                    state.changed();
                } else {
                    state.status = "先在资产列表选中角色GLB，再点击绑定所选模型".into();
                }
            } else {
                action(&mut state, button.action);
            }
            state.mouse_owned = true;
            break;
        }
    }
    for (button, click) in &channels {
        if !click.is_clicked() || button.binding != state.revision {
            continue;
        }
        if let Some(index) = button.index
            && let Some(session) = state.current_mut()
        {
            if session.pose.is_some_and(|(node, _)| {
                node != session.document.clips[session.clip].channels[index].node
            }) {
                continue;
            }
            session.selected_node = Some(session.document.clips[session.clip].channels[index].node);
            session.channel = Some(index);
            session.key = None;
            state.changed();
            state.mouse_owned = true;
        }
        break;
    }
}

fn numeric_edit(
    state: &AnimationEditorState,
    field: AnimationNumericField,
    value: f32,
) -> Option<Edit> {
    let session = state.current()?;
    let channel_index = session.channel?;
    let channel = session
        .document
        .clips
        .get(session.clip)?
        .channels
        .get(channel_index)?;
    let before = if field == AnimationNumericField::Frame {
        channel.keys.get(session.key?).cloned()
    } else {
        channel
            .keys
            .iter()
            .find(|key| (key.time - session.position).abs() < 1e-6)
            .cloned()
    };
    let mut after = before.clone().unwrap_or_else(|| Keyframe {
        time: session.position,
        value: channel.sample(session.position),
        in_tangent: Vec::new(),
        out_tangent: Vec::new(),
    });
    match field {
        AnimationNumericField::Frame => after.time = value.max(0.0) / state.fps as f32,
        AnimationNumericField::Value(index) if channel.path == "rotation" => {
            let q = Quat::from_array(after.value.as_slice().try_into().ok()?).normalize();
            let (x, y, z) = q.to_euler(EulerRot::XYZ);
            let mut angles = [x, y, z];
            *angles.get_mut(index)? = value.to_radians();
            after.value = Quat::from_euler(EulerRot::XYZ, angles[0], angles[1], angles[2])
                .to_array()
                .to_vec();
        }
        AnimationNumericField::Value(index) => *after.value.get_mut(index)? = value,
    }
    Some(Edit::Key {
        clip: session.clip,
        channel: channel_index,
        before,
        after: Some(after),
    })
}

pub(super) fn action(state: &mut AnimationEditorState, action: AnimationAction) {
    use AnimationAction::*;
    if state.in_place_rig_preview() && matches!(
        action,
        Reload | MovePose | RotatePose | ToggleAutoKey | ResetPose | CopyKeys | BindSelectedModel
    ) {
        return;
    }
    if matches!(
        action,
        Save | Undo | Redo | InsertKey | DeleteKey | CycleInterpolation | InsertPose | PasteKeys
    ) && !state.editable()
    {
        return;
    }
    // 临时姿势尚未进入曲线历史；其他曲线事务采用时会清姿势，必须先要求明确插入或还原。
    if state.current().is_some_and(|s| s.pose.is_some()) {
        if action == Undo {
            if let Some(s) = state.current_mut() {
                s.pose = None;
            }
            state.changed();
            return;
        }
        if matches!(
            action,
            Save | Reload
                | Redo
                | InsertKey
                | DeleteKey
                | CycleInterpolation
                | PasteKeys
                | PreviousBone
                | NextBone
                | PreviousClip
                | NextClip
                | FirstFrame
                | PreviousFrame
                | NextFrame
                | LastFrame
                | PreviousKey
                | NextKey
                | TogglePlayback
        ) {
            state.status = "请先插入或还原临时姿势".into();
            return;
        }
    }
    let duration = state.current().map_or(0.0, |s| {
        s.metadata.durations.get(s.clip).copied().unwrap_or(0.0)
    });
    match action {
        FitTimeline => {
            if let Some(s) = state.current_mut() {
                s.window_start = 0.0;
                s.window_span = duration.max(1.0 / 30.0);
            }
            state.changed();
        }
        ZoomIn | ZoomOut => {
            super::timeline::zoom(state, if action == ZoomIn { 1.0 } else { -1.0 }, 0.5);
        }
        ToggleBoneFilter => {
            if let Some(s) = state.current_mut() {
                s.bone_filter = !s.bone_filter;
                s.page = 0;
            }
            state.changed();
        }
        PreviousBone | NextBone => {
            if let Some(s) = state.current_mut() {
                let bones = &s.metadata.bones[s.clip];
                if !bones.is_empty() {
                    let i = s
                        .selected_node
                        .and_then(|node| bones.iter().position(|n| *n == node))
                        .unwrap_or(0);
                    let i = if action == PreviousBone {
                        (i + bones.len() - 1) % bones.len()
                    } else {
                        (i + 1) % bones.len()
                    };
                    s.select_node(bones[i]);
                }
            }
            state.changed();
        }
        MovePose | RotatePose => {
            state.pose_tool = if action == MovePose {
                super::types::PoseTool::Move
            } else {
                super::types::PoseTool::Rotate
            };
            // 数值区跟随工具显示同一骨骼的位置/旋转通道，切工具不修改姿势或新增历史。
            if let Some(s) = state.current_mut()
                && let Some(indices) = s.selected_node
                    .and_then(|node| s.metadata.channels_by_node[s.clip].get(&node))
            {
                let path = if action == MovePose { "translation" } else { "rotation" };
                if let Some(&channel) = indices.iter()
                    .find(|&&i| s.document.clips[s.clip].channels[i].path == path)
                {
                    s.channel = Some(channel);
                    s.key = None;
                }
            }
            state.changed();
        }
        ToggleAutoKey => {
            state.auto_key = !state.auto_key;
            state.changed();
        }
        ResetPose => {
            if let Some(s) = state.current_mut() {
                s.pose = None;
            }
            state.changed();
        }
        InsertPose => {
            if let Some(s) = state.current() {
                if let Some((node, transform)) = s.pose.or_else(|| {
                    s.selected_node
                        .map(|node| (node, super::pose::local_at(s, node)))
                }) {
                    state.pending = Some(PendingAction::Command(super::editing::Command::Pose {
                        clip: s.clip,
                        time: s.position,
                        node,
                        transform,
                    }));
                }
            }
        }
        UnbindModel => {
            if state.in_place_rig_preview() {
                state.stop_rig_preview();
                return;
            }
            if let Some(s) = state.current_mut() {
                s.model_path = None;
                s.retarget_model = false;
            }
            state.model_status.clear();
            state.changed();
        }
        BindSelectedModel => {}
        CopyKeys | PasteKeys => {
            if let (Some(s), Some(source)) = (state.current(), state.active.clone()) {
                let command = if action == CopyKeys {
                    super::editing::Command::Copy {
                        clip: s.clip,
                        selection: s.selected.clone(),
                        source,
                    }
                } else if let Some(clipboard) = state.clipboard.clone() {
                    super::editing::Command::Paste {
                        clip: s.clip,
                        at: s.position,
                        source,
                        clipboard,
                    }
                } else {
                    return;
                };
                state.pending = Some(PendingAction::Command(command));
            }
        }
        TogglePlayback => {
            if state.current().is_some_and(|s| s.pose.is_some()) {
                state.status = "请先插入或还原临时姿势".into();
                return;
            }
            if duration <= 0.0 {
                return;
            }
            if state.position() >= duration {
                state.seek(0.0);
            }
            state.playing = !state.playing;
        }
        ToggleLoop => state.looping = !state.looping,
        ToggleFps => {
            state.fps = if state.fps == 30 { 60 } else { 30 };
            state.changed();
        }
        CycleSpeed => {
            state.speed = if state.speed < 0.75 {
                1.0
            } else if state.speed < 1.5 {
                2.0
            } else {
                0.5
            }
        }
        FirstFrame | PreviousFrame | NextFrame | LastFrame => {
            let at = match action {
                FirstFrame => 0.0,
                LastFrame => duration,
                PreviousFrame => state.position() - 1.0 / state.fps as f32,
                _ => state.position() + 1.0 / state.fps as f32,
            };
            state.seek(at);
            state.playing = false;
        }
        PreviousKey | NextKey => {
            if let Some(session) = state.current_mut()
                && let Some(channel) = session.channel.and_then(|index| {
                    session
                        .document
                        .clips
                        .get(session.clip)?
                        .channels
                        .get(index)
                })
                && !channel.keys.is_empty()
            {
                let index = match (action, session.key) {
                    (PreviousKey, Some(index)) => index.saturating_sub(1),
                    (PreviousKey, None) => channel.keys.len() - 1,
                    (_, Some(index)) => (index + 1).min(channel.keys.len() - 1),
                    _ => 0,
                };
                session.key = Some(index);
                session.position = channel.keys[index].time;
                session.selected =
                    super::types::KeySelection::single(session.channel.unwrap(), session.position);
                if session.position < session.window_start
                    || session.position > session.window_start + session.window_span
                {
                    session.window_start = (session.position - session.window_span * 0.5).max(0.0);
                }
            }
            state.playing = false;
            state.changed();
        }
        PreviousClip | NextClip => {
            if let Some(session) = state.current_mut() {
                let count = session.document.clips.len();
                if count == 0 {
                    return;
                }
                if session.pose.is_some() {
                    return;
                }
                let clip = if action == PreviousClip {
                    (session.clip + count - 1) % count
                } else {
                    (session.clip + 1) % count
                };
                session.select_clip(clip);
            }
            state.playing = false;
            state.changed();
        }
        PreviousChannelPage | NextChannelPage => {
            if let Some(s) = state.current_mut() {
                let count = if s.bone_filter {
                    s.selected_node
                        .and_then(|n| s.metadata.channels_by_node[s.clip].get(&n))
                        .map_or(0, Vec::len)
                } else {
                    s.document.clips[s.clip].channels.len()
                };
                s.page = if action == PreviousChannelPage {
                    s.page.saturating_sub(1)
                } else {
                    (s.page + 1).min(count.saturating_sub(1) / CHANNEL_SLOTS)
                };
            }
            state.changed();
        }
        Save => {
            if state.current().is_some_and(Session::dirty) {
                state.pending = Some(PendingAction::Save);
            }
        }
        Reload => {
            if state.current().is_some_and(|s| !s.dirty()) {
                state.pending = Some(PendingAction::Reload);
            }
        }
        Undo => {
            if state.current().is_some_and(|s| s.cursor > 0) {
                state.pending = Some(PendingAction::Undo);
            }
        }
        Redo => {
            if state.current().is_some_and(|s| s.cursor < s.history.len()) {
                state.pending = Some(PendingAction::Redo);
            }
        }
        InsertKey | DeleteKey | CycleInterpolation => {
            if action == DeleteKey
                && let Some(s) = state.current()
                && !s.selected.0.is_empty()
            {
                state.pending = Some(PendingAction::Command(super::editing::Command::Delete {
                    clip: s.clip,
                    selection: s.selected.clone(),
                }));
                return;
            }
            let Some(session) = state.current() else {
                return;
            };
            let Some(channel_index) = session.channel else {
                return;
            };
            let Some(channel) = session
                .document
                .clips
                .get(session.clip)
                .and_then(|c| c.channels.get(channel_index))
            else {
                return;
            };
            let edit = match action {
                CycleInterpolation => Edit::Interpolation {
                    clip: session.clip,
                    channel: channel_index,
                    before: channel.interpolation.clone(),
                    after: match channel.interpolation.as_str() {
                        "LINEAR" => "STEP",
                        "STEP" => "CUBICSPLINE",
                        _ => "LINEAR",
                    }
                    .to_owned(),
                },
                DeleteKey => {
                    let Some(before) = session.key.and_then(|i| channel.keys.get(i)).cloned()
                    else {
                        return;
                    };
                    Edit::Key {
                        clip: session.clip,
                        channel: channel_index,
                        before: Some(before),
                        after: None,
                    }
                }
                _ => {
                    let time = (session.position * state.fps as f32).round() / state.fps as f32;
                    let near = channel.keys.partition_point(|key| key.time < time - 1e-6);
                    if channel
                        .keys
                        .get(near)
                        .is_some_and(|key| (key.time - time).abs() < 1e-6)
                    {
                        state.status = "当前帧已有关键帧，请选择菱形修改数值".to_owned();
                        return;
                    }
                    let value = channel.sample(time);
                    let key = Keyframe {
                        time,
                        in_tangent: Vec::new(),
                        out_tangent: Vec::new(),
                        value,
                    };
                    Edit::Key {
                        clip: session.clip,
                        channel: channel_index,
                        before: None,
                        after: Some(key),
                    }
                }
            };
            state.pending = Some(PendingAction::Edit(edit));
        }
    }
}
