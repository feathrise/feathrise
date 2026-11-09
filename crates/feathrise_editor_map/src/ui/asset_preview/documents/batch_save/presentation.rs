//! 报告仅在轻量结果变化时更新最多八行，静止帧不读草稿或磁盘。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::DocumentBatchSaveState, types::FileOutcome};
use crate::ui::theme as colors;
use bevy::prelude::*;

pub(super) fn sync(
    state: Res<DocumentBatchSaveState>,
    mut roots: Query<&mut Node, With<SaveReport>>,
    mut texts: Query<
        (&mut Text, Has<SaveReportSummary>),
        Or<(With<SaveReportSummary>, With<SaveReportBody>)>,
    >,
    mut buttons: Query<(&UiInteraction, &mut BackgroundColor), With<HideSaveReport>>,
    changed: Query<(), (With<HideSaveReport>, Changed<UiInteraction>)>,
    added: Query<(), Added<SaveReport>>,
) {
    if !state.is_changed() && changed.is_empty() && added.is_empty() {
        return;
    }
    // 回执正文由公共底栏收集；整个旧卡片一起收起，不能留下按钮、边框或滚动空洞。
    let display = Display::None;
    for mut node in &mut roots {
        if node.display != display {
            node.display = display;
        }
    }
    let count = |outcome| {
        state
            .files
            .iter()
            .filter(|file| file.outcome == outcome)
            .count()
    };
    let summary = format!(
        "{} · {} · 成功{} / 失败{} / 无需保存{} / 共{}",
        state.scope,
        if state.running {
            "保存中"
        } else {
            "保存结果"
        },
        count(FileOutcome::Saved),
        count(FileOutcome::Failed),
        count(FileOutcome::Unmodified),
        state.files.len()
    );
    let mut body = state.hint.clone();
    for file in &state.files {
        let outcome = match file.outcome {
            FileOutcome::Pending => "进行中",
            FileOutcome::Saved => "成功",
            FileOutcome::Failed => "失败",
            FileOutcome::Unmodified => "无需保存",
        };
        // 展示字符串也固定预算；磁盘错误与会话状态仍保留完整原因供后续单文件查看。
        body.push_str(&format!(
            "\n[{outcome}] {}：{}",
            clipped(super::super::source::display_path(&file.path), 160),
            clipped(&file.detail, 240)
        ));
    }
    for (mut text, heading) in &mut texts {
        let value = if !state.shown { "" } else if heading { &summary } else { &body };
        if text.as_str() != value {
            **text = value.to_owned();
        }
    }
    for (interaction, mut color) in &mut buttons {
        color.set_if_neq(BackgroundColor(if *interaction == UiInteraction::None {
            colors::SLOT_BG
        } else {
            colors::SELECT_BG_HOVER
        }));
    }
}

fn clipped(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let mut text: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        text.push('…');
    }
    text
}
