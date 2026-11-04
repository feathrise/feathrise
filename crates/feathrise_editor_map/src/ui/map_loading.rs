//! 编辑器专属切图提示：底栏显示真实进度，完整诊断进入详情，正式就绪前隔离作者输入。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::scene::template_value;
use bevy::text::FontSourceTemplate;
use bevy::ui::FocusPolicy;
use feathrise_editor_ui::feedback::PanelStatusText;
use feathrise_scene_core::state::{ScenePhase, LoadingProgress, LoadingStage};

use super::scene::FONT_PATH;
use super::theme as colors;
use crate::components::{
    MapEditorMapLoadingDetail, MapEditorMapLoadingFill, MapEditorMapLoadingInputBlocker,
    MapEditorMapLoadingPanel, MapEditorMapLoadingPercent, MapEditorMapLoadingTitle,
    MapLoadingRecoveryButton,
};
use crate::resources::{MapEditorState, MapLoadingRecovery};

/// 加载只占底栏一行；长名称裁切，完整阶段与失败原因由原详情窗口收集。
pub(super) fn map_loading_panel() -> impl Scene {
    bsn! {
        MapEditorMapLoadingPanel
        Node {
            display: Display::None, height: px(22), min_height: px(22), max_height: px(22),
            flex_grow: 1.0, min_width: px(0), align_items: AlignItems::Center,
            column_gap: px(4), overflow: Overflow::clip_x(),
        }
        Children [(
            // 裁切宿主与自然宽度文字分开，字形不能跨过百分比或恢复按钮。
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() }
            template_value(FocusPolicy::Pass)
            Children [(
                MapEditorMapLoadingTitle
                Text("正在切换地图")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY)
                TextLayout::no_wrap()
                template_value(FocusPolicy::Pass)
                Node { flex_shrink: 0.0 }
            )]
        ), (
            // 极窄窗口优先给真实百分比、恢复和详情；进度轨道允许收缩至零。
            Node { width: px(80), min_width: px(0), height: px(3), overflow: Overflow::clip() }
            BackgroundColor(colors::TRACK_BG)
            template_value(FocusPolicy::Pass)
            Children [(
                MapEditorMapLoadingFill
                Node { width: percent(0), height: percent(100) }
                BackgroundColor(colors::TEXT_ACCENT)
                template_value(FocusPolicy::Pass)
            )]
        ), (
            MapEditorMapLoadingPercent
            Text("0% · 0秒")
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_VALUE)
            TextLayout::no_wrap()
            template_value(FocusPolicy::Pass)
            Node { flex_shrink: 0.0 }
        ),
            (recovery_button("重试", MapLoadingRecoveryButton::Retry)),
            (recovery_button("取消", MapLoadingRecoveryButton::Cancel)),
        (
            MapEditorMapLoadingDetail
            PanelStatusText("地图加载")
            Text("")
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_MUTED)
            Node { display: Display::None } // 仅作为统一详情的数据源，不在底栏重复铺开说明和长路径。
        )]
    }
}

fn recovery_button(label: &'static str, action: MapLoadingRecoveryButton) -> impl Scene {
    bsn! {
        Button
        MapLoadingRecoveryButton::from(action)
        Node {
            height: px(22), min_height: px(22), flex_shrink: 0.0,
            padding: UiRect::horizontal(px(4)), border: px(1),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
            TextLayout::no_wrap()
            template_value(FocusPolicy::Pass)
        )]
    }
}

/// 透明鼠标门保留外壳可见；顶栏退出入口仍可用，避免失败时困在加载工作区。
pub(super) fn map_loading_input_blocker() -> impl Scene {
    bsn! {
        Interaction::default() // 纯遮挡节点仍参与官方ui_focus；不套用普通按钮的悬停背景。
        MapEditorMapLoadingInputBlocker
        FocusPolicy::default() // BSN枚举沿用组件默认构造，首个同步帧按显隐设置Pass或Block。
        GlobalZIndex(1900)
        Node {
            display: Display::None, position_type: PositionType::Absolute,
            left: px(0), right: px(0), top: px(28), bottom: px(0), // 对齐28px顶栏，工具栏不能留下可点透的旧14px缝隙。
        }
    }
}

pub(crate) fn suppress_map_loading_shortcuts(
    editor: Res<MapEditorState>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
) {
    if editor.map_transition_in_progress {
        // F12是关闭/取消等待入口；其余边沿不能作用于背后的旧草稿，按住状态仍交给原输入系统。
        let author_edges: Vec<_> = keyboard
            .get_just_pressed()
            .chain(keyboard.get_just_released())
            .filter(|key| **key != KeyCode::F12)
            .copied()
            .collect();
        for key in author_edges {
            keyboard.clear_just_pressed(key);
            keyboard.clear_just_released(key);
        }
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn sync_map_loading(
    editor: Res<MapEditorState>,
    progress: Option<Res<LoadingProgress>>,
    recovery: Option<Res<MapLoadingRecovery>>,
    mut started: Local<(u64, Option<std::time::Instant>)>,
    mut nodes: Query<
        (
            &mut Node,
            Has<MapEditorMapLoadingPanel>,
            Has<MapEditorMapLoadingInputBlocker>,
            Has<MapEditorMapLoadingFill>,
            Option<&mut Interaction>,
            Option<&mut FocusPolicy>,
        ),
        Or<(
            With<MapEditorMapLoadingPanel>,
            With<MapEditorMapLoadingInputBlocker>,
            With<MapEditorMapLoadingFill>,
        )>,
    >,
    mut texts: Query<
        (
            &mut Text,
            &mut TextColor,
            Has<MapEditorMapLoadingTitle>,
            Has<MapEditorMapLoadingDetail>,
        ),
        Or<(
            With<MapEditorMapLoadingTitle>,
            With<MapEditorMapLoadingDetail>,
            With<MapEditorMapLoadingPercent>,
        )>,
    >,
) {
    let visible = editor.open && editor.map_transition_in_progress;
    let epoch = recovery.as_ref().map_or(0, |state| state.epoch);
    if started.0 != epoch || !visible { *started = (epoch, None); }
    if visible { started.1.get_or_insert_with(std::time::Instant::now); }
    let recovering = recovery.as_ref().is_some_and(|state| state.request.is_some());
    let failed = progress
        .as_ref()
        .is_some_and(|value| value.stage == LoadingStage::Failed);
    let percent = progress.as_ref().map_or(0.0, |value| value.percent());
    for (mut node, panel, blocker, fill, interaction, focus_policy) in &mut nodes {
        if panel || blocker {
            let display = if visible {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
        if fill && node.width != Val::Percent(percent) {
            node.width = Val::Percent(percent);
        }
        if blocker {
            if let Some(mut policy) = focus_policy {
                let wanted = if visible {
                    FocusPolicy::Block
                } else {
                    FocusPolicy::Pass
                };
                if *policy != wanted {
                    *policy = wanted;
                }
            }
            if !visible
                && let Some(mut interaction) = interaction
                && *interaction != Interaction::None
            {
                *interaction = Interaction::None; // 隐藏同帧释放鼠标，不把旧按下留给下一次切图。
            }
        }
    }
    if !visible {
        for (mut text, _, _, is_detail) in &mut texts {
            if is_detail && !text.is_empty() { text.clear(); }
        } // 结束后移除本次加载的详情源，避免旧失败原因残留在下一张地图。
        return;
    }
    let title = progress.as_ref().map_or_else(
        || "正在切换地图".to_owned(),
        |value| {
            format!(
                "{}“{}”",
                if failed {
                    "无法打开"
                } else {
                    "正在打开"
                },
                value.target_display_name(ScenePhase::Active)
            )
        },
    );
    let stage_detail = if recovering || editor.pending_map_transition.is_some() {
        editor.status.as_str()
    } else {
        progress.as_ref().map_or(editor.status.as_str(), |value| value.label.as_str())
    };
    let elapsed = started.1.as_ref().map_or(0, |start| start.elapsed().as_secs());
    let percent_text = if recovering {
        format!("返回中 · {elapsed}秒")
    } else if failed {
        format!("失败 · {elapsed}秒")
    } else {
        format!("{percent:.0}% · {elapsed}秒")
    };
    let detail = format!(
        "{title}\n{stage_detail}\n{percent:.0}% · 已耗时 {elapsed} 秒\n取消会保留草稿与其他标签；失败后可重试"
    );
    // 固定三个文本、三个布局节点；稳定帧不扫描地图，不新建资产或后台任务。
    for (mut text, mut color, is_title, is_detail) in &mut texts {
        let value = if is_title {
            title.as_str()
        } else if is_detail {
            detail.as_str()
        } else {
            percent_text.as_str()
        };
        if text.as_str() != value {
            **text = value.to_owned();
        }
        if is_title {
            let wanted = if failed {
                colors::DANGER_TEXT
            } else {
                colors::TEXT_PRIMARY
            };
            if color.0 != wanted {
                color.0 = wanted;
            }
        }
    }
}

#[cfg(test)]
#[path = "map_loading_tests.rs"]
mod tests;
