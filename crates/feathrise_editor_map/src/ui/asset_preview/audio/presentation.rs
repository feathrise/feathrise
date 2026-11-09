//! 只按真实试听状态同步固定播放器节点，不随音频采样频率重排UI。

use feathrise_editor_ui::UiInteraction;
use super::super::types::{AssetPreviewContent, AssetPreviewPhase, AssetPreviewState};
use super::{components::*, resources::AudioPreviewState, types::*};
use crate::ui::theme as colors;
use bevy::{prelude::*, ui::InteractionDisabled};

fn time(seconds: f64) -> String {
    let seconds = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

pub(super) fn sync(
    mut commands: Commands,
    state: Res<AudioPreviewState>,
    preview: Res<AssetPreviewState>,
    mut nodes: Query<
        (&mut Node, Option<&AudioPanel>, Option<&AudioFill>),
        Or<(With<AudioPanel>, With<AudioFill>)>,
    >,
    mut buttons: Query<(
        Entity,
        &mut AudioButton,
        &UiInteraction,
        &mut BackgroundColor,
        Has<InteractionDisabled>,
        Option<&mut feathrise_editor_ui::EditorButtonClick>,
    )>,
    mut tracks: Query<&mut AudioTrack>,
    mut labels: Query<(&AudioLabel, &mut Text)>,
) {
    let shown = matches!(preview.content, AssetPreviewContent::Audio { .. });
    let ready = shown
        && state.data.is_some()
        && matches!(
            preview.content,
            AssetPreviewContent::Audio {
                phase: AssetPreviewPhase::Ready
            }
        );
    let duration = state.data.as_ref().map_or(0.0, |data| data.duration());
    let position = if duration > 0.0 {
        (state.position / duration) as f32
    } else {
        0.0
    };
    for (mut node, panel, fill) in &mut nodes {
        if panel.is_some() {
            let display = if shown { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        } else if let Some(fill) = fill {
            let value = match fill.0 {
                SliderField::Position => position,
                SliderField::Volume => state.volume,
            };
            let width = percent(value.clamp(0.0, 1.0) * 100.0);
            if node.width != width {
                node.width = width;
            }
        }
    }
    for (entity, mut button, interaction, mut background, disabled, click) in &mut buttons {
        if button.key != state.key {
            if let Some(mut click) = click { click.invalidate(); } // 按住时切音频不得把播放/停止改派给新会话。
            button.key = state.key.clone();
        }
        let enabled =
            shown && state.key.is_some() && (ready || button.action == AudioAction::Reload);
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        background.set_if_neq(BackgroundColor(
            if enabled && *interaction == UiInteraction::Pressed {
                colors::SELECT_BG_PRESSED
            } else if enabled && *interaction == UiInteraction::Hovered {
                colors::SELECT_BG_HOVER
            } else {
                colors::SLOT_BG
            },
        ));
    }
    for mut track in &mut tracks {
        if track.key != state.key {
            track.key = state.key.clone();
        }
    }
    for (kind, mut text) in &mut labels {
        let value = match kind {
            AudioLabel::Playback => "音频试听".into(),
            AudioLabel::Time => {
                if state.data.is_some() {
                    format!("{} / {}", time(state.position), time(duration))
                } else {
                    format!("{} / --:--", time(state.position))
                }
            }
            AudioLabel::Volume => format!("音量 {:.0}%", state.volume * 100.0),
            AudioLabel::Metadata => state.data.as_ref().map(|data| format!("{}声道 · {}Hz", data.channels.get(), data.sample_rate.get())).unwrap_or_default(),
            AudioLabel::Details => if state.waiting.is_some() {
                "正在准备音频输出…"
            } else {
                "" // 手势说明由音频标题旁?展示；底栏只接收真实等待状态。
            }
            .into(),
            AudioLabel::Button(AudioAction::Toggle) => {
                if state.playing { "暂停" } else { "播放" }.into()
            }
            AudioLabel::Button(AudioAction::Stop) => "停止".into(),
            AudioLabel::Button(AudioAction::Reload) => "重新载入".into(),
        };
        if text.as_str() != value {
            **text = value;
        }
    }
}
