//! 试听按钮与两条滑杆只改会话；冻结文件身份并发布统一鼠标所有权。

use feathrise_editor_ui::UiInteraction;
use super::super::{
    AssetPreviewWorkspace,
    types::{AssetPreviewContent, AssetPreviewPhase, AssetPreviewState},
};
use super::{components::*, context::AudioContext, resources::AudioPreviewState, types::*};
use bevy::{input::mouse::MouseWheel, prelude::*, ui::RelativeCursorPosition};

fn apply_slider(state: &mut AudioPreviewState, field: SliderField, fraction: f32) {
    if !fraction.is_finite() {
        return;
    }
    let fraction = fraction.clamp(0.0, 1.0);
    match field {
        SliderField::Position => {
            let duration = state.data.as_ref().map_or(0.0, |data| data.duration());
            state.seek(duration * fraction as f64);
        }
        SliderField::Volume => state.volume = fraction,
    }
}

pub(super) fn input(
    context: AudioContext,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    documents: Res<super::super::DocumentState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheels: MessageReader<MouseWheel>,
    mut state: ResMut<AudioPreviewState>,
    buttons: Query<(&AudioButton, &feathrise_editor_ui::EditorButtonClick), Changed<feathrise_editor_ui::EditorButtonClick>>,
    tracks: Query<(
        Entity,
        &AudioTrack,
        &ComputedNode,
        &RelativeCursorPosition,
        &UiInteraction,
    )>,
) {
    state.mouse_owned = false;
    state.wheel_owned = false;
    let wheel = wheels.read().fold(false, |seen, event| {
        seen || context
            .windows
            .single()
            .ok()
            .is_some_and(|(id, _)| id == event.window)
    });
    let visible = context.owner(&preview, &media).is_some()
        && state.key.as_ref().is_some_and(|key| {
            preview.selected_path.as_deref() == Some(key.path.as_str())
                && key.root == documents.asset_root()
        })
        && context
            .numeric
            .as_ref()
            .is_none_or(|active| active.entity.is_none())
        && context
            .map_drag
            .as_ref()
            .is_none_or(|drag| !drag.blocks_other_mouse() && drag.press.is_none())
        && context
            .text
            .as_ref()
            .is_none_or(|guard| !guard.block_shortcuts);
    if !visible || (context.guard.blocked() && state.drag.is_none()) {
        state.mouse_owned = state.drag.take().is_some();
        return;
    }
    let ready = matches!(
        preview.content,
        AssetPreviewContent::Audio {
            phase: AssetPreviewPhase::Ready
        }
    ) && state.data.is_some();
    if let Some(drag) = &state.drag {
        let track = tracks.get(drag.entity).ok();
        if !ready || state.key.as_ref() != Some(&drag.key) || track.is_none() {
            state.drag = None;
            state.mouse_owned = true;
            return;
        }
        let field = drag.field;
        let (_, _, _, cursor, _) = track.unwrap();
        state.mouse_owned = true;
        state.wheel_owned = wheel;
        if mouse.pressed(MouseButton::Left) {
            if let Some(at) = cursor.normalized {
                apply_slider(&mut state, field, at.x + 0.5);
            }
        } else {
            state.drag = None;
        }
        return; // release及三键和弦继续沿原鼠标门等待静默帧。
    }
    for (button, interaction) in &buttons {
        if !interaction.is_clicked() || button.key != state.key {
            continue;
        }
        match button.action {
            AudioAction::Reload => {
                state.retry = true;
                state.playing = false;
            }
            AudioAction::Toggle if ready => {
                if !state.playing
                    && (state
                        .data
                        .as_ref()
                        .is_some_and(|data| state.position >= data.duration())
                        || state
                            .cursor
                            .as_ref()
                            .is_some_and(|cursor| cursor.finished()))
                {
                    state.seek(0.0);
                }
                state.playing = !state.playing;
            }
            AudioAction::Stop if ready => {
                state.playing = false;
                state.seek(0.0);
            }
            _ => continue,
        }
        state.mouse_owned = true;
    }
    for (entity, track, node, cursor, interaction) in &tracks {
        if !ready
            || track.key != state.key
            || node.size().min_element() <= 0.0
            || !cursor.cursor_over()
            || *interaction == UiInteraction::None
        {
            continue;
        }
        state.wheel_owned |= wheel; // 滑杆上的滚轮不改变世界/外层滚动；拖动为唯一改值入口。
        if !mouse.just_pressed(MouseButton::Left)
            || [MouseButton::Right, MouseButton::Middle]
                .into_iter()
                .any(|key| mouse.pressed(key) || mouse.just_released(key))
        {
            continue;
        }
        let Some(at) = cursor.normalized else {
            continue;
        };
        let Some(key) = state.key.clone() else {
            continue;
        };
        state.drag = Some(SliderDrag {
            entity,
            key,
            field: track.field,
        });
        state.mouse_owned = true;
        apply_slider(&mut state, track.field, at.x + 0.5);
        break;
    }
}
