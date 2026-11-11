//! 设置内快捷键页的四组固定动作、真实键位按钮和问号帮助；说明不再单独占用一行。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use super::scene::utility_button;
use crate::components::{
    SceneEditorSettingsButton, SceneEditorShortcutBindingText, SceneEditorShortcutNoticeText,
    SceneEditorShortcutSettingsBody, SceneEditorShortcutSettingsPanel,
};
use crate::types::EditorSettingsAction;
use crate::ui::scene::FONT_PATH;
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, Justify, LineBreak};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use bevy::ui_widgets::ScrollbarThumb;
use feathrise_editor_core::EditorShortcutAction;

const SHORTCUT_HELP: &str = "添加网格打开顶部网格工具的同一面板，可在中央文档和预览页使用；其他动作仅中央场景工作区生效。文字、数值输入和弹框期间暂停。\n录入主键，可同时按Ctrl、Shift、Alt；冲突不会覆盖已有绑定。Esc先取消录入，再关闭设置；功能键、删除和文本控制键保留。\n移动：轴或XZ方框；旋转：Y轴圆环；缩放：中心方块。Ctrl吸附默认1米、15°、0.1倍率，实际步长取吸附设置；Esc取消变换。\n手柄操作期间暂停取景与工具切换；固定鼠标导航不参与改键。右键旋转、中键平移、滚轮缩放。运行时和文档键位不受本页影响。";

pub(super) fn shortcut_page() -> impl Scene {
    use EditorShortcutAction::*;
    bsn! {
        SceneEditorShortcutSettingsPanel
        ~{Node { display: Display::None, flex_direction: FlexDirection::Row,
            flex_grow: 1.0, column_gap: px(4), ..feathrise_editor_ui::dialog_body_node() }}
        Children [
            SceneEditorShortcutSettingsBody RelativeCursorPosition::default() ScrollPosition::default()
            Node { flex_basis: px(0), min_width: px(0), min_height: px(0), flex_grow: 1.0,
                flex_direction: FlexDirection::Column, row_gap: px(8),
                overflow: Overflow::scroll_y() }
            Children [
                @notice_card()--
                @shortcut_group("视图导航", &[FocusSelected, FrameScene])--
                @shortcut_group("场景编辑", &[Save, Undo, Redo, AddObjectMenu])--
                @shortcut_group("笔刷参数", &[DecreaseRadius, IncreaseRadius, DecreaseStrength, IncreaseStrength])--
                @shortcut_group("变换工具", &[MoveSelected, RotateSelected, ScaleSelected])
            ]
        --
            ShortcutScrollbarTrack RelativeCursorPosition::default() ~{FocusPolicy::Block}
            Node { width: px(8), min_width: px(8), flex_shrink: 0.0,
                border_radius: BorderRadius::all(px(4)) }
            BackgroundColor(colors::PANEL_BG_SECTION)
            Children [
                // Bevy 0.19原生滑块不能带Node；其真实几何由ScrollbarPlugin在布局后维护。
                ShortcutScrollbarThumb
                ~{ScrollbarThumb { border_radius: BorderRadius::all(px(4)), border: UiRect::all(px(0)) }}
                BackgroundColor(colors::COPPER_BORDER)
            ]
        ]
    }
}

fn notice_card() -> impl Scene {
    bsn! {
        // 只在有真实反馈时显示；长冲突说明随正文滚动，不能固定占满矮窗口。
        ShortcutNoticeCard
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
            padding: UiRect::axes(px(4), px(3)), border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            SceneEditorShortcutNoticeText Text("")
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
            TextColor(colors::TEXT_MUTED) TextLayout::linebreak(LineBreak::AnyCharacter)
            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
        ]
    }
}

fn shortcut_group(label: &str, actions: &[EditorShortcutAction]) -> impl Scene {
    let rows = actions
        .iter()
        .copied()
        .map(shortcut_row)
        .collect::<Vec<_>>();
    (
        bsn! {
            ShortcutSettingsGroup
            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
                flex_direction: FlexDirection::Column, row_gap: px(4) }
        },
        bevy::scene::RelatedScenes::<Children>::new((
            // 每个有效分组标题只有一个问号，复用完整教程，不再添加底部整行“展开说明”。
            bevy::scene::EntityScene(crate::ui::context_help::section_header(label, SHORTCUT_HELP)),
            rows,
        )),
    )
}

fn shortcut_row(action: EditorShortcutAction) -> impl Scene {
    let label = action.label();
    bsn! {
        ShortcutSettingsRow(action)
        Node { width: percent(100), min_width: px(0), min_height: px(28), flex_shrink: 0.0,
            // 三列始终属于同一行；长组合键仅在自己的单元格内换行，不能把按键拆到下一行。
            align_items: AlignItems::Center, column_gap: px(4) }
        Children [
            // 长英文动作在动作列内换行，保留同一行的改键和清除按钮。
            Node { width: percent(36), min_width: px(72), flex_shrink: 0.0 }
            Children [
                Text(label) @feathrise_editor_ui::localization::label_scene(label) TextLayout::linebreak(LineBreak::WordBoundary)
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY)
                Node { width: percent(100), min_width: px(0) }
            ]
        --
            InteractionButton SceneEditorSettingsButton(EditorSettingsAction::RebindShortcut(action))
            // 完整组合键不靠裁字适配窄窗；按钮及文字按真实行高折行，仍能点中。
            Node { flex_basis: px(0), min_width: px(0), min_height: px(28), flex_grow: 1.0,
                padding: UiRect::axes(px(3), px(4)),
                align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                border: px(1), border_radius: BorderRadius::all(px(3)) }
            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [
                SceneEditorShortcutBindingText(action) Text("未绑定") @feathrise_editor_ui::localization::label_scene("未绑定")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) TextShadow { offset: Vec2::splat(1.0) }
                TextLayout::new(Justify::Center, LineBreak::AnyCharacter)
                // 显式占用按钮内宽，避免仅设百分比max_width时得到零宽文字排版。
                Node { width: percent(100), min_width: px(0) }
            ]
        -- @utility_button("清除", EditorSettingsAction::UnbindShortcut(action), 34.0)]
    }
}
