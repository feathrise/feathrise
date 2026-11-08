//! F12 建筑页院落组合卡片和中央视口的轻量框选轮廓。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*; // BSN UI 场景与 ECS 标记。
use bevy::text::FontSourceTemplate; // 与建筑页一致的中文字体。 // 项目统一暗墨、铜金与米白配色。

use crate::ui::scene::FONT_PATH; // F12 共用正式字体路径。

/// 院落卡片按钮的结构化动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CourtyardAction {
    FocusName,      // 让官方中文输入接管名称字段。
    BoxSelect,      // 下一次视口拖拽产生多选。
    ClearSelection, // 显式清除旧选集。
    SaveSelection,  // 请求作者将选集写为全局预设。
    PreviousPreset, // 按目录顺序浏览前一组合。
    NextPreset,     // 按目录顺序浏览后一组合。
    Stamp,          // 进入选定预设落印。
    MoveGroup,      // 进入已选院落副本整体移动。
    CopyGroup,      // 进入已选院落副本整体复制。
    RotateLeft,     // 已选副本左转或印章朝向左转。
    RotateRight,    // 已选副本右转或印章朝向右转。
    CancelMode,     // 放弃本次武装工具。
}

/// 固定按钮不参与全局建筑预设动作分派。
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct CourtyardButton(pub(super) CourtyardAction);

impl Default for CourtyardButton {
    fn default() -> Self {
        Self(CourtyardAction::BoxSelect) // BSN 泛型模板默认值不会处理真实业务。
    }
}

/// 官方中文输入实体；值由独立系统读回院落会话。
#[derive(Component, Clone, Default)]
pub(crate) struct CourtyardNameInput;

/// 当前选集数量与扫描进度。
#[derive(Component, Clone, Default)]
pub(super) struct CourtyardSelectionText;

/// 当前全局组合目录选择。
#[derive(Component, Clone, Default)]
pub(super) struct CourtyardPresetText;

/// 操作步骤和明确失败原因。
#[derive(Component, Clone, Default)]
pub(crate) struct CourtyardStatusText;

/// 当前武装模式与印章角度。
#[derive(Component, Clone, Default)]
pub(crate) struct CourtyardModeText;

/// 中央视口纯视觉选框，不参与鼠标命中。
#[derive(Component, Clone, Default)]
pub(crate) struct CourtyardSelectionRegion;

/// 紧凑两列操作行，长反馈在统一宿主换行，不再堆叠多层卡片。
fn button(label: &str, action: CourtyardAction) -> impl Scene {
    bsn! {
        InteractionButton
        CourtyardButton(action)
        Node {
            height: px(28),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(5)),
            overflow: Overflow::clip_x(),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

fn section(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_ACCENT)
        Node {
            width: percent(100),
            margin: UiRect::top(px(4)),
            padding: UiRect::top(px(6)),
            border: UiRect::top(px(1)),
        }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

pub(crate) fn courtyard_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            flex_shrink: 0.0,
        }
        Children [
            (@crate::ui::context_help::section_header("院落预设", "框选地图中的房屋与装饰，再保存为可重复盖印的院落；也可选择已有院落预设进行盖印。")),
            (
                CourtyardPresetText
                Text("组合预设：等待目录") @feathrise_editor_ui::localization::label_scene("组合预设：等待目录")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0) }
            ),
            (
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [
                    (@button("上一个", CourtyardAction::PreviousPreset)),
                    (@button("下一个", CourtyardAction::NextPreset)),
                ]
            ),
            (@button("盖印所选院落", CourtyardAction::Stamp)),
            (@section("地图选集")),
            (
                CourtyardSelectionText
                Text("当前框选：0 项") @feathrise_editor_ui::localization::label_scene("当前框选：0 项")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                Node { width: percent(100), min_width: px(0) }
            ),
            (
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [
                    (@button("框选对象", CourtyardAction::BoxSelect)),
                    (@button("清空选集", CourtyardAction::ClearSelection)),
                ]
            ),
            (
                Text("组合名称") @feathrise_editor_ui::localization::label_scene("组合名称")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED)
            ),
            (
                InteractionButton
                CourtyardButton(CourtyardAction::FocusName)
                Node {
                    width: percent(100),
                    min_width: px(0),
                    height: px(32),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(5)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    overflow: Overflow::clip_x(),
                }
                BackgroundColor(colors::INPUT_BG)
                BorderColor::all(colors::INPUT_ACTIVE_BORDER)
                Children [(
                    // 保留同一正式 EditableText；隐藏与重显不能重置中文光标或已确认名称。
                    CourtyardNameInput
                    crate::ui::text_input::EditorTextInput
                    bevy::text::EditableText { allow_newlines: false }
                    bevy::text::EditableTextFilter::new(|character| !character.is_control())
                    bevy::text::TextCursorStyle::default()
                    TextLayout::no_wrap()
                    Node { width: percent(100), min_width: px(0), height: px(18), overflow: Overflow::clip_x() }
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                    TextColor(colors::TEXT_PRIMARY)
                )]
            ),
            (@button("保存选集为院落预设", CourtyardAction::SaveSelection)),
            (@crate::ui::context_help::section_header("选中院落", "盖印时旋转印章，否则旋转选中院落。成员仍可单独调整，不执行整组整平。")),
            (
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [
                    (@button("整体移动", CourtyardAction::MoveGroup)),
                    (@button("整体复制", CourtyardAction::CopyGroup)),
                ]
            ),
            (
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [
                    (@button("左转 90°", CourtyardAction::RotateLeft)),
                    (@button("右转 90°", CourtyardAction::RotateRight)),
                ]
            ),
            (@button("取消当前工具", CourtyardAction::CancelMode)),
        ]
    }
}

/// 纯视觉矩形在选框武装期间随鼠标拖拽更新，不截获世界或 UI 输入。
pub(crate) fn selection_region() -> impl Scene {
    bsn! {
        CourtyardSelectionRegion
        Node {
            display: Display::None, // 没有有效拖拽时彻底隐藏。
            position_type: PositionType::Absolute, // 覆盖在视口画面上方。
            left: percent(0), // 实际位置由 Presentation 系统按归一化坐标更新。
            top: percent(0), // 实际位置由 Presentation 系统更新。
            width: percent(0), // 拖拽前没有可见面积。
            height: percent(0), // 拖拽前没有可见面积。
            border: UiRect::all(px(1)), // 铜金细线表达精确选区。
        }
        BackgroundColor(Color::srgba(0.745, 0.529, 0.243, 0.10))
        BorderColor::all(colors::COPPER_BORDER)
    }
}
