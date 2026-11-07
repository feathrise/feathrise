//! 设置唯一模态壳：顶部常规/快捷键页、各自滚动正文和共用固定操作区。

use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use super::settings_button;
use crate::components::{
    MapEditorSettingsButton, MapEditorSettingsPanel, MapEditorSettingsScaleText,
};
use crate::types::EditorSettingsAction;
use crate::ui::scene::FONT_PATH;
use crate::ui::context_help::section_header as help_header;
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use feathrise_editor_core::{EditorColorTheme, EditorLocalePreference};

fn fixed_backdrop_colors() -> impl Scene {
    // ButtonColors没有Default/Template，沿共享按钮配色的原构造方式插入完整组件。
    bevy::ecs::template::template(|_| Ok(feathrise_editor_ui::ButtonColors::new(
        feathrise_editor_ui::colors::DIALOG_BACKDROP,
        feathrise_editor_ui::colors::DIALOG_BACKDROP,
        feathrise_editor_ui::colors::DIALOG_BACKDROP,
    )))
}

pub(super) fn settings_panel() -> impl Scene {
    bsn! {
        MapEditorSettingsPanel
        Node {
            display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: percent(100), height: percent(100),
            padding: px(8), align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        }
        FocusPolicy::default() GlobalZIndex(900)
        Children [(
            Button MapEditorSettingsButton(EditorSettingsAction::Close)
            feathrise_editor_ui::EditorDialogBackdrop
            // 遮罩Button只承载点外关闭；悬停、按压都保持同一遮罩色，不能照普通按钮增亮。
            fixed_backdrop_colors()
            Node {
                position_type: PositionType::Absolute, left: px(0), top: px(0),
                width: percent(100), height: percent(100),
            }
            BackgroundColor(feathrise_editor_ui::colors::DIALOG_BACKDROP)
        ), (
            SettingsFrame template_value(FocusPolicy::Block)
            // 两页共用标题、页签和底栏；操作说明只由标题问号展开，不挤占设置值正文。
            template_value(Node {
                width: px(540), max_width: percent(100), height: px(540), max_height: percent(100),
                ..feathrise_editor_ui::dialog_panel_node()
            })
            // 标题、正文、底栏与日志/详情/关闭确认共用同一套边界和背景，不再维护设置专属外框。
            BackgroundColor(feathrise_editor_ui::colors::DIALOG_BG)
            BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
            Children [(
                SettingsHeader
                template_value(feathrise_editor_ui::dialog_header_node())
                BackgroundColor(feathrise_editor_ui::colors::DIALOG_HEADER_BG)
                BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
                Children [(
                    Node { flex_grow: 1.0, min_width: px(0) }
                    Children [(help_header("设置", "更改即时生效并自动保存；完成只关闭面板。快捷键页面点击对应绑定后录入，冲突和录入进度仍在页面内明确显示。"))]
                ), (close_button())]
            ), (
                SettingsTabs
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
                    column_gap: px(6), row_gap: px(4), flex_wrap: FlexWrap::Wrap,
                    padding: UiRect { left: px(12), right: px(12), top: px(4), bottom: px(0) } }
                Children [
                    (page_button("常规", EditorSettingsAction::Open, 72.0)),
                    (page_button("快捷键", EditorSettingsAction::OpenShortcuts, 84.0)),
                ]
            ), (
                SettingsBody SettingsGeneralOnly crate::workbench::WorkbenchSettingsBody RelativeCursorPosition::default() ScrollPosition::default()
                template_value(Node {
                    flex_grow: 1.0, overflow: Overflow::scroll_y(),
                    ..feathrise_editor_ui::dialog_body_node()
                })
                Children [
                    (locale_group()), (viewport_group()), (super::mesh_lod::panel()), (scale_group()), (theme_group()), (snapping_group()), (layout_group()),
                ]
            ), (
                super::shortcuts_scene::shortcut_page()
            ), (
                SettingsFooter
                template_value(feathrise_editor_ui::dialog_footer_node())
                BackgroundColor(feathrise_editor_ui::colors::DIALOG_HEADER_BG)
                BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
                Children [
                    (
                        SettingsGeneralOnly
                        Node { min_width: px(0), flex_shrink: 0.0 }
                        Children [(utility_button("重置缩放", EditorSettingsAction::SetScale(100), 90.0))]
                    ), (
                        SettingsShortcutsOnly
                        Node { display: Display::None, min_width: px(0), flex_shrink: 0.0 }
                        // 与常规页重置同宽，避免窄窗切页时底栏使用不同的折行阈值。
                        Children [(utility_button("恢复默认", EditorSettingsAction::ResetShortcuts, 90.0))]
                    ),
                    (utility_button("完成", EditorSettingsAction::Close, 72.0)),
                ]
            )]
        )]
    }
}

pub(super) fn group_node() -> Node {
    Node {
        width: percent(100),
        min_width: px(0),
        flex_shrink: 0.0,
        flex_direction: FlexDirection::Column,
        row_gap: px(6),
        padding: UiRect::all(px(8)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(4)),
        ..default()
    }
}

fn locale_group() -> impl Scene {
    bsn! {
        SettingsGroup template_value(group_node())
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (help_header("界面语言", "立即切换编辑器界面语言，重启后恢复此选择。")),
            (
                Node { width: percent(100), min_width: px(0), flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6) }
                Children [
                    (settings_button("跟随系统", EditorSettingsAction::SetLocale(EditorLocalePreference::System), 112.0)),
                    (settings_button("简体中文", EditorSettingsAction::SetLocale(EditorLocalePreference::Chinese), 84.0)),
                    (settings_button("English", EditorSettingsAction::SetLocale(EditorLocalePreference::English), 84.0)),
                ]
            )
        ]
    }
}

fn scale_group() -> impl Scene {
    bsn! {
        SettingsGroup template_value(group_node())
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (help_header("界面缩放", "手动与自动均为60%–200%，加减每次5%。自动按窗口尺寸调整，加减可微调自动比例。只影响编辑器界面，不影响运行界面或三维渲染比例。")),
            (
                Node { width: percent(100), min_width: px(0), align_items: AlignItems::Center, column_gap: px(6) }
                Children [
                    (settings_button("−", EditorSettingsAction::DecreaseScale, 32.0)),
                    (
                        // 字形由子实体的真实行高居中，不在Text自身固定高度上伪装对齐。
                        Node { flex_grow: 1.0, min_width: px(0), height: px(28),
                            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
                        Children [(
                            MapEditorSettingsScaleText Text("100%") TextLayout::no_wrap()
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(18) }
                            TextColor(colors::TEXT_ACCENT)
                        )]
                    ),
                    (settings_button("+", EditorSettingsAction::IncreaseScale, 32.0)),
                ]
            ), (
                Node { width: percent(100), min_width: px(0), flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(4) }
                Children [
                    (settings_button("75%", EditorSettingsAction::SetScale(75), 58.0)),
                    (settings_button("100%", EditorSettingsAction::SetScale(100), 58.0)),
                    (settings_button("125%", EditorSettingsAction::SetScale(125), 58.0)),
                    (settings_button("150%", EditorSettingsAction::SetScale(150), 58.0)),
                    (settings_button("自动", EditorSettingsAction::SetAutoScale, 58.0)),
                ]
            )
        ]
    }
}

/// 视口偏好独立于地图作者数据；置于首组，让各倍率下都能直接找到开关。
fn viewport_group() -> impl Scene {
    bsn! {
        SettingsGroup template_value(group_node())
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (help_header("视口显示", "开启时按钮高亮；父地图与子地图生效，退出编辑后仍保留。")),
            (settings_button("显示地形背面", EditorSettingsAction::ToggleTerrainBackfaces, 148.0)),
        ]
    }
}

fn theme_group() -> impl Scene {
    bsn! {
        SettingsGroup template_value(group_node())
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (help_header("主题配色", "只影响编辑器工具界面；运行界面、地图和模型颜色保持原样。")),
            (
                SettingsThemeText Text("当前：中性深灰") feathrise_editor_ui::localization::label_scene("当前：中性深灰")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY) TextLayout::linebreak(LineBreak::AnyCharacter)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            ), (
                Node { width: percent(100), min_width: px(0), flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(4) }
                Children [
                    (settings_button("中性深灰", EditorSettingsAction::SetColorTheme(EditorColorTheme::NeutralGray), 84.0)),
                    (settings_button("石墨灰＋雾蓝", EditorSettingsAction::SetColorTheme(EditorColorTheme::GraphiteBlue), 108.0)),
                    (settings_button("暖墨铜金", EditorSettingsAction::SetColorTheme(EditorColorTheme::WarmCopper), 84.0)),
                ]
            )
        ]
    }
}

/// 吸附沿用公共数值框和原偏好事务，随常规设置正文滚动且不进入地图作者历史。
fn snapping_group() -> impl Scene {
    bsn! {
        SettingsGroup template_value(group_node())
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (help_header("变换吸附", "工具条切换网格、吸附和移动模式，Y切换常开。Ctrl临时反转；左拖调值，中键精确输入，自动保存。建筑连接与90°规则优先；移动模式只改变位置取整基准。")),
            (super::snapping::input_slot(super::snapping::SnapField::Translation)),
            (super::snapping::input_slot(super::snapping::SnapField::Rotation)),
            (super::snapping::input_slot(super::snapping::SnapField::Scale)),
            (utility_button("恢复吸附默认值", EditorSettingsAction::ResetSnapping, 130.0)),
        ]
    }
}

/// 布局恢复复用原动作，只恢复两栏偏好；独立卡片避免改变两页共用固定底栏的高度。
fn layout_group() -> impl Scene {
    bsn! {
        SettingsGroup template_value(group_node())
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (help_header("侧栏布局", "侧栏宽度和显隐自动保存，下次启动恢复。恢复布局只重置两栏偏好，不改变其他设置或地图作者数据。")),
            (utility_button("恢复默认布局", EditorSettingsAction::ResetLayout, 112.0)),
        ]
    }
}

fn page_button(label: &str, action: EditorSettingsAction, width: f32) -> impl Scene {
    let style = feathrise_editor_ui::dialog_button_style(false);
    bsn! {
        Button SettingsPageButton MapEditorSettingsButton(action)
        template_value(Node { width: px(width), min_width: px(0), ..style.node })
        feathrise_editor_ui::dialog_button_colors_scene(false)
        template_value(BackgroundColor(style.colors.normal)) BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

pub(super) fn utility_button(label: &str, action: EditorSettingsAction, width: f32) -> impl Scene {
    let style = feathrise_editor_ui::dialog_button_style(false);
    bsn! {
        Button SettingsUtilityButton MapEditorSettingsButton(action)
        template_value(Node { width: px(width), min_width: px(0), ..style.node })
        feathrise_editor_ui::dialog_button_colors_scene(false)
        template_value(BackgroundColor(style.colors.normal)) BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

fn close_button() -> impl Scene {
    let style = feathrise_editor_ui::dialog_close_button_style();
    bsn! {
        Button SettingsUtilityButton MapEditorSettingsButton(EditorSettingsAction::Close)
        template_value(style.node) feathrise_editor_ui::dialog_button_colors_scene(false)
        template_value(BackgroundColor(style.colors.normal)) BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
        Children [(
            Text("×") TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}
