//! 底栏摘要、上下文与诊断组装；完成/失败只取结构化证据，不搜索文案关键词。

use bevy::prelude::*;

use super::types::{RuntimeEntry, RuntimePhase, StatusPhase};
use crate::resources::MapEditorState;
use crate::types::{EditorTool, EditorView};
use crate::ui::theme as colors;

pub(super) fn summary(message: &str) -> String {
    let mut result = String::new();
    for (index, character) in message.chars().take(89).enumerate() {
        if index == 88 || character == '\n' || character == '\r' {
            result.push('…'); // 最多检查89个字符，长诊断不在稳定帧整段扫描。
            break;
        }
        result.push(character);
    }
    if result.is_empty() {
        "等待操作".into()
    } else {
        result
    }
}

pub(super) fn copy_detail(
    detail: &str,
    write: impl FnOnce(&str) -> Result<(), bevy::clipboard::ClipboardError>,
) -> (bool, String) {
    match write(detail) {
        Ok(()) => (false, "完整详情已复制".into()),
        Err(error) => (true, format!("复制失败：{error:?}；诊断仍可在正文查看")),
    }
}

pub(super) fn phase_color(phase: StatusPhase) -> Color {
    match phase {
        StatusPhase::Error => colors::DANGER_TEXT,
        StatusPhase::Warning | StatusPhase::Working => colors::TEXT_ACCENT,
        StatusPhase::Success => colors::TEXT_GREEN,
        StatusPhase::Ready => colors::TEXT_MUTED,
    }
}

fn tool_label(tool: EditorTool) -> &'static str {
    match tool {
        EditorTool::Inspect => "选择",
        EditorTool::Domain(_) => "扩展工具",
        EditorTool::Raise => "抬高",
        EditorTool::Lower => "降低",
        EditorTool::Smooth => "平滑",
        EditorTool::Flatten => "整平",
        EditorTool::VolumeDig => "三维挖掘",
        EditorTool::VolumeFill => "三维填充",
        EditorTool::VolumeSmooth => "三维平滑",
        EditorTool::VolumeFlatten => "三维整平",
        EditorTool::Water => "静水绘制",
        EditorTool::RiverSpline => "河流",
        EditorTool::Waterfall => "瀑布",
        EditorTool::PlaceAsset => "资产摆放",
        EditorTool::Building => "建筑",
        EditorTool::Portal => "传送点",
        EditorTool::Surface => "地表绘制",
        EditorTool::Puddles => "积水绘制",
        EditorTool::Vegetation => "植被绘制",
        EditorTool::Boundary => "空气墙",
    }
}

pub(super) fn tool_context(editor: &MapEditorState, locale: feathrise_editor_ui::localization::EditorLocale) -> String {
    let view = match editor.view {
        EditorView::Perspective => "自由",
        EditorView::Top => "顶视",
        EditorView::Front => "前视",
        EditorView::Side => "侧视",
    };
    let tool = locale.editor_label(if editor.placement_transform_enabled {
            "变换"
        } else {
            tool_label(editor.tool)
        });
    let view = locale.editor_label(view);
    let projection = locale.editor_label(if editor.orthographic {
            "正交"
        } else {
            "透视"
        });
    let mut text = format!("{tool} · {view} · {projection}");
    if matches!(
        editor.tool,
        EditorTool::Raise
            | EditorTool::Lower
            | EditorTool::Smooth
            | EditorTool::Flatten
            | EditorTool::VolumeDig
            | EditorTool::VolumeFill
            | EditorTool::VolumeSmooth
            | EditorTool::VolumeFlatten
            | EditorTool::Water
            | EditorTool::Surface
            | EditorTool::Puddles
            | EditorTool::Vegetation
    ) || (matches!(editor.tool, EditorTool::Domain(_)) && editor.domain_brush_active)
    {
        text.push_str(&locale.format("status.tool.radius", &[("value", format!("{:.1}", editor.brush_radius))]));
        if matches!(editor.tool, EditorTool::Domain(_)) {
            text.push_str(&locale.format("status.tool.spacing", &[("value", format!("{:.1}", editor.brush_spacing))]));
        } else {
            text.push_str(&locale.format("status.tool.strength", &[("value", format!("{:.2}", editor.brush_strength))]));
        }
        if matches!(editor.tool, EditorTool::Raise | EditorTool::Lower | EditorTool::Smooth | EditorTool::Flatten) {
            text.push_str(&format!(" · {:.1}m/s", editor.terrain_sculpt_speed));
        }
    }
    text
}

pub(super) fn runtime_detail(entries: &[RuntimeEntry<'_>; 5], locale: feathrise_editor_ui::localization::EditorLocale) -> String {
    let mut result = String::new();
    for entry in entries {
        let Some(phase) = entry.phase else { continue };
        let phase = match phase {
            RuntimePhase::Working => "处理中",
            RuntimePhase::Ready => "已采用",
            RuntimePhase::Disabled => "未启用",
            RuntimePhase::Failed => "失败（未采用本次结果）",
        };
        result.push_str(&locale.format("status.detail.runtime_stage", &[
            ("task", locale.editor_label(entry.label).into()),
            ("stage", locale.editor_label(phase).into()),
        ]));
        if !entry.detail.is_empty() {
            result.push_str("\n");
            result.push_str(entry.detail);
        }
    }
    result
}
