//! 连续时间轴的鼠标捕获与快捷键；手势只预览，释放时提交一次后台事务。

use feathrise_editor_ui::UiInteraction;
use super::super::{AssetPreviewWorkspace, DocumentState, types::AssetPreviewState};
use super::{components::*, context::AnimationContext, editing::Command, resources::*, types::*};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    ui::RelativeCursorPosition,
};
use feathrise_editor_core::EditorKeyBinding;

pub(super) fn zoom(state: &mut AnimationEditorState, lines: f32, anchor: f32) {
    let fps = state.fps as f32;
    if let Some(s) = state.current_mut() {
        let fixed = s.window_start + s.window_span * anchor;
        s.window_span = (s.window_span * (-lines * 0.2).exp()).clamp(2.0 / fps, 100_000.0);
        s.window_start = (fixed - s.window_span * anchor).max(0.0);
    }
    state.changed();
}

fn nearest(keys: &[super::data::Keyframe], time: f32, tolerance: f32) -> Option<usize> {
    let right = keys.partition_point(|key| key.time < time);
    [right.checked_sub(1), (right < keys.len()).then_some(right)]
        .into_iter()
        .flatten()
        .filter(|&i| (keys[i].time - time).abs() <= tolerance)
        .min_by(|&a, &b| {
            (keys[a].time - time)
                .abs()
                .total_cmp(&(keys[b].time - time).abs())
        })
}

pub(super) fn input(
    context: AnimationContext,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    documents: Res<DocumentState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keyboard: Option<ResMut<ButtonInput<KeyCode>>>,
    mut wheels: MessageReader<MouseWheel>,
    mut state: ResMut<AnimationEditorState>,
    tracks: Query<(
        Entity,
        &AnimationTrack,
        &ComputedNode,
        &RelativeCursorPosition,
        &UiInteraction,
    )>,
) {
    let wheel: f32 = wheels
        .read()
        .filter(|e| {
            context
                .windows
                .single()
                .ok()
                .is_some_and(|(id, _)| id == e.window)
        })
        .map(|e| {
            if e.unit == MouseScrollUnit::Pixel {
                e.y / 27.0
            } else {
                e.y
            }
        })
        .filter(|y| y.is_finite())
        .sum();
    let available = state.visible()
        && !state.weight_rest_active()
        && context.available(&preview, &media)
        && context.source_matches(&state, &preview, &media, documents.asset_root());
    let pressed = |key| keyboard.as_ref().is_some_and(|keys| keys.pressed(key));
    let just = |key| keyboard.as_ref().is_some_and(|keys| keys.just_pressed(key));
    let ctrl = pressed(KeyCode::ControlLeft) || pressed(KeyCode::ControlRight);
    let shift = pressed(KeyCode::ShiftLeft) || pressed(KeyCode::ShiftRight);
    if !available || just(KeyCode::Escape) {
        state.mouse_owned |= state.gesture.take().is_some();
        return;
    }
    let busy = state.busy();
    if busy {
        state.mouse_owned |= state.gesture.take().is_some();
    }
    if let Some(mut gesture) = state.gesture.take() {
        state.mouse_owned = true;
        if gesture.binding != state.revision || state.source() != Some(gesture.source.as_path()) {
            return;
        }
        let Ok((_, _, node, cursor, _)) = tracks.get(gesture.entity) else {
            return;
        };
        if let Some(at) = cursor.normalized {
            let s = state.current().unwrap();
            let time = (s.window_start + (at.x + 0.5) * s.window_span).max(0.0);
            let delta = ((at.x - gesture.cursor.x) * s.window_span * state.fps as f32).round()
                / state.fps as f32;
            match &mut gesture.kind {
                TimelineGestureKind::Scrub => {
                    let fps = state.fps as f32;
                    state.seek((time * fps).round() / fps);
                }
                TimelineGestureKind::Pan { start } => {
                    if let Some(s) = state.current_mut() {
                        s.window_start =
                            (*start - (at.x - gesture.cursor.x) * s.window_span).max(0.0);
                    }
                }
                TimelineGestureKind::Move { delta: value, .. } => {
                    // 四像素以内视为点击抖动，避免长动作缩小时轻点就移动一帧。
                    *value = if ((at.x - gesture.cursor.x) * node.size().x).abs() < 4.0 {
                        0.0
                    } else {
                        delta
                    };
                }
                TimelineGestureKind::Box {
                    end_row, end_time, ..
                } => {
                    *end_time = time;
                    *end_row = ((gesture.row as f32
                        + (at.y - gesture.cursor.y) * node.size().y
                            / (24.0 / node.inverse_scale_factor()))
                    .round() as isize)
                        .clamp(0, CHANNEL_SLOTS as isize - 1)
                        as usize;
                }
            }
        }
        if mouse.pressed(gesture.button) {
            state.gesture = Some(gesture);
            return;
        }
        match gesture.kind {
            TimelineGestureKind::Move { selection, delta } if delta != 0.0 && !state.retargeting() => {
                let clip = state.active_clip();
                state.pending = Some(PendingAction::Command(Command::Move {
                    clip,
                    selection,
                    delta,
                }));
            }
            TimelineGestureKind::Box {
                mut additive,
                end_row,
                end_time,
            } => {
                if let Some(s) = state.current_mut() {
                    for (row, channel) in s.visible_channels().into_iter().enumerate() {
                        if row >= gesture.row.min(end_row) && row <= gesture.row.max(end_row) {
                            additive.0.push((
                                channel,
                                gesture.time.min(end_time),
                                gesture.time.max(end_time),
                            ));
                        }
                    }
                    s.selected = additive;
                }
            }
            _ => {}
        }
        state.changed();
        return;
    }
    if context.guard.blocked()
        || context.numeric.as_ref().is_some_and(|s| s.entity.is_some())
        || context
            .numeric_text
            .as_ref()
            .is_some_and(|s| s.entity.is_some())
        || context.text.as_ref().is_some_and(|s| s.block_shortcuts)
        || context
            .scene_drag
            .as_ref()
            .is_some_and(|s| s.blocks_other_mouse() || s.press.is_some())
    {
        return;
    }
    let hovered = tracks.iter().find(|(_, track, node, cursor, interaction)| {
        track.binding == state.revision
            && node.size().min_element() > 0.0
            && cursor.cursor_over()
            && **interaction != UiInteraction::None
    });
    // 快捷键只在时间轴悬停时处理，不抢资产列表、搜索框或场景撤销。
    let Some((entity, track, node, cursor, _)) = hovered else {
        return;
    };
    state.wheel_owned = true;
    // 只消费已声明的完整和弦，Alt/Super及多余修饰键仍归原焦点所有；鼠标点击仍可自然切焦。
    let shortcut = (!context.text_input_focused()).then(|| keyboard.as_ref()).flatten().and_then(|keys| {
        [
            (KeyCode::KeyC, true, false, AnimationAction::CopyKeys),
            (KeyCode::KeyV, true, false, AnimationAction::PasteKeys),
            (KeyCode::KeyZ, true, false, AnimationAction::Undo),
            (KeyCode::KeyZ, true, true, AnimationAction::Redo),
            (KeyCode::Delete, false, false, AnimationAction::DeleteKey),
            (KeyCode::Space, false, false, AnimationAction::TogglePlayback),
            (KeyCode::Home, false, false, AnimationAction::FitTimeline),
        ].into_iter().find_map(|(key, control, shift, action)| {
            EditorKeyBinding { key, control, shift, alt: false }.just_pressed(keys).then_some((key, action))
        })
    });
    if let Some((key, action)) = shortcut {
        // 同屏场景稍后仍读取键盘；时间轴受理后消费该边沿，空历史或只读拒绝也不能转成场景命令。
        if let Some(keys) = keyboard.as_mut() {
            keys.clear_just_pressed(key);
        }
        if !busy {
            super::control::action(&mut state, action);
        }
        return;
    }
    if busy {
        return;
    } // 后台工作中只保留悬停的输入归属，不定位、滚动或启动第二个作者事务。
    let Some(at) = cursor.normalized else {
        return;
    };
    if wheel != 0.0 {
        if shift {
            if let Some(s) = state.current_mut() {
                s.window_start = (s.window_start - wheel * s.window_span * 0.1).max(0.0);
            }
            state.changed();
        } else {
            zoom(&mut state, wheel, (at.x + 0.5).clamp(0.0, 1.0));
        }
        return;
    }
    let button = if mouse.just_pressed(MouseButton::Middle) {
        MouseButton::Middle
    } else if mouse.just_pressed(MouseButton::Left) {
        MouseButton::Left
    } else {
        return;
    };
    if [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
        .into_iter()
        .any(|other| other != button && (mouse.pressed(other) || mouse.just_released(other)))
    {
        return;
    }
    let s = state.current().unwrap();
    if s.pose.is_some() && button == MouseButton::Left {
        state.status = "请先插入或还原临时姿势".into();
        return;
    }
    let time = (s.window_start + (at.x + 0.5) * s.window_span).max(0.0);
    let mut kind = TimelineGestureKind::Scrub;
    if button == MouseButton::Middle {
        kind = TimelineGestureKind::Pan {
            start: s.window_start,
        };
    } else if let Some(row) = track.row.filter(|_| !state.retargeting()) {
        let Some(channel) = s.visible_channels().get(row).copied() else {
            return;
        };
        let keys = &s.document.clips[s.clip].channels[channel].keys;
        let hit = nearest(keys, time, 8.0 / node.size().x * s.window_span);
        if let Some(index) = hit {
            let key_time = keys[index].time;
            let s = state.current_mut().unwrap();
            if ctrl {
                s.selected.toggle(channel, key_time);
            } else if !s.selected.contains(channel, key_time) {
                s.selected = KeySelection::single(channel, key_time);
            }
            s.channel = Some(channel);
            s.key = Some(index);
            if s.pose.is_none() {
                s.position = key_time;
            }
            kind = TimelineGestureKind::Move {
                selection: s.selected.clone(),
                delta: 0.0,
            };
        } else {
            kind = TimelineGestureKind::Box {
                additive: if ctrl {
                    s.selected.clone()
                } else {
                    KeySelection::default()
                },
                end_row: row,
                end_time: time,
            };
        }
    } else {
        let fps = state.fps as f32;
        state.seek((time * fps).round() / fps);
    }
    state.playing = false;
    state.mouse_owned = true;
    state.gesture = Some(TimelineGesture {
        entity,
        source: state.source().expect("已核对时间轴来源").to_owned(),
        binding: state.revision,
        cursor: at,
        time,
        row: track.row.unwrap_or(0),
        button,
        kind,
    });
}

#[cfg(test)]
#[path = "timeline_scene_shortcut_tests.rs"]
mod scene_shortcut_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(row: Option<usize>) -> (App, Entity) {
        let (mut app, _, _) = super::super::editor_tests::app_context();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, input);
        let revision = app.world().resource::<AnimationEditorState>().revision;
        app.world_mut()
            .resource_mut::<AnimationEditorState>()
            .current_mut()
            .unwrap()
            .window_span = 2.0;
        let entity = app
            .world_mut()
            .spawn((
                AnimationTrack {
                    row,
                    binding: revision,
                },
                ComputedNode {
                    size: Vec2::new(400., 24.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::new(-0.5, 0.0)),
                },
                UiInteraction::Hovered,
            ))
            .id();
        (app, entity)
    }
    fn cursor(app: &mut App, entity: Entity, x: f32, y: f32) {
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(entity)
            .unwrap()
            .normalized = Some(Vec2::new(x, y));
    }
    #[test]
    fn real_drag_submits_once_on_release_and_escape_cancels() {
        let (mut app, track) = fixture(Some(0));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(matches!(
            app.world()
                .resource::<AnimationEditorState>()
                .gesture
                .as_ref()
                .unwrap()
                .kind,
            TimelineGestureKind::Move { .. }
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        cursor(&mut app, track, -0.25, 0.);
        app.update();
        assert!(
            app.world()
                .resource::<AnimationEditorState>()
                .pending
                .is_none()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert!(
            matches!(app.world().resource::<AnimationEditorState>().pending,Some(PendingAction::Command(Command::Move {delta,..})) if (delta-0.5).abs()<1e-5)
        );
        let (mut app, track) = fixture(Some(0));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        cursor(&mut app, track, 0.2, 0.);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        let state = app.world().resource::<AnimationEditorState>();
        assert!(state.gesture.is_none());
        assert!(state.pending.is_none());
    }
    #[test]
    fn box_selection_and_scrub_are_distinct_and_shortcuts_require_hover() {
        let (mut app, track) = fixture(Some(0));
        cursor(&mut app, track, -0.4, 0.);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        cursor(&mut app, track, 0.6, 0.);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        let state = app.world().resource::<AnimationEditorState>();
        assert!(!state.current().unwrap().selected.0.is_empty());
        assert!(state.pending.is_none());
        assert_eq!(state.position(), 0.0);
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(track)
            .unwrap()
            .cursor_over = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Delete);
        app.update();
        assert!(
            app.world()
                .resource::<AnimationEditorState>()
                .pending
                .is_none()
        );
        let (mut app, track) = fixture(None);
        cursor(&mut app, track, 0., 0.);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert_eq!(
            app.world().resource::<AnimationEditorState>().position(),
            1.0
        );
        app.world_mut()
            .resource_mut::<crate::resources::SceneEditorState>()
            .open = false;
        app.update();
        assert!(
            app.world()
                .resource::<AnimationEditorState>()
                .gesture
                .is_none()
        );
    }
}
