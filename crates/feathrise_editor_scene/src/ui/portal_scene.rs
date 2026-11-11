//! 玩法页传送点双列表的BSN静态外壳；真实条目行由`portal_list`功能域维护。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN Scene与紧凑纵向布局组件。
use bevy::text::FontSourceTemplate; // 当前/目标场景摘要使用项目中文字体。
use crate::ui::theme as colors; // 传送作者面板复用水墨铜边和米白文字。

use crate::components::{
    SceneEditorCurrentPortalActions, SceneEditorCurrentPortalList, SceneEditorCurrentPortalListTitle,
    SceneEditorPortalNameInput, SceneEditorPortalNameInputText, SceneEditorPortalSelectionPanel,
    SceneEditorPortalTargetText, SceneEditorTargetPortalList, SceneEditorTargetPortalListTitle,
}; // BSN只声明稳定容器与标题marker，运行时不按名称查找实体。
use crate::types::EditorAction; // 名称与删除入口继续使用结构化静态动作。

use super::scene::{FONT_PATH, flex_editor_button}; // 名称与删除按钮保持与其他作者操作一致的等宽密度。

fn step(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_ACCENT)
        TextLayout { linebreak: LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

/// 当前源点检查与新点目标准备分区；选择目标不会改写已有源点的有向连接。
pub(super) fn portal_authoring_panel() -> impl Scene {
    bsn! {
        SceneEditorPortalSelectionPanel
        ~{crate::ui::scene_inspector::SelectedToolProperties::new(crate::ui::scene_inspector::SelectedToolPropertiesKind::Portal)}
        Node {
            display: Display::None, // 页面显隐由原传送Presentation唯一负责，Select中的Inspect不收起属性。
            width: percent(100), // 双列表完整使用玩法页正文宽度。
            min_width: px(0),
            flex_direction: FlexDirection::Column, // 当前源点与目标落点按明确上下关系排列。
            row_gap: px(3), // 紧凑间距把主体面积继续留给中央视口。
            flex_shrink: 0.0, // 低高度窗口由玩法页外层滚动访问完整内容。
        }
        Children [
            @step("1 · 当前源点（已有）")--

                SceneEditorCurrentPortalListTitle
                Text("当前场景传送点 · 等待作者数据") @feathrise_editor_ui::localization::label_scene("当前场景传送点 · 等待作者数据")
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 标题与摘要共用同一字体句柄。
                    font_size: px(9.0), // 分组标题不与主工具按钮争抢视觉层级。
                }
                TextColor(colors::TEXT_ACCENT)
            --

                SceneEditorCurrentPortalList
                Node {
                    display: Display::None, // 零传送点时完整列表完全退出布局。
                    width: percent(100), // 列表适配固定260px左栏正文。
                    height: px(0), // 一项起按全部真实行数增长。
                    flex_direction: FlexDirection::Column, // 源点按作者数组顺序纵向排列。
                    row_gap: px(2), // 相邻源点保持两像素可辨边界。
                    padding: UiRect::all(px(0)), // 行自身已经提供内边距。
                    flex_shrink: 0.0, // 不压扁二十像素点击行。
                }
            --

                SceneEditorCurrentPortalActions
                Node {
                    display: Display::None, // 当前场景存在源点后才显示删除入口，空图不制造无效操作行。
                    width: percent(100), // 名称输入与操作按钮占满正文宽度。
                    flex_direction: FlexDirection::Column, // 输入框位于应用/删除按钮上方。
                    row_gap: px(3), // 名称与动作保持紧凑但可辨的分组间距。
                    flex_shrink: 0.0, // 低高度继续由玩法页滚动访问。
                }
                Children [

                        Text("所选源点名称") @feathrise_editor_ui::localization::label_scene("所选源点名称") // 只编辑已有源点名称，不改写目标落点。
                        TextFont {
                            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 中文说明使用项目字体。
                            font_size: px(ui_metrics::FONT_HINT), // 辅助说明弱于列表主信息。
                        }
                        TextColor(colors::TEXT_HINT) // 使用提示色避免抢占列表层级。
                    --

                        InteractionButton // 点击后启用键盘与中文IME输入。
                        SceneEditorPortalNameInput // 稳定marker供Presentation更新聚焦边框。
                        crate::components::SceneEditorActionButton(EditorAction::PortalNameFocus) // 结构化聚焦动作。
                        Node {
                            width: percent(100), // 输入框使用完整正文宽度。
                            height: px(24), // 与紧凑传送列表行保持相近密度。
                            align_items: AlignItems::Center, // 输入文字垂直居中。
                            padding: UiRect::horizontal(px(5)), // 光标不紧贴铜边。
                            border: UiRect::all(px(1)), // 明确这是可编辑字段。
                            overflow: Overflow::clip_x(), // 长名称不撑宽左栏。
                        }
                        BackgroundColor(colors::INPUT_BG) // 使用统一输入框底色。
                        BorderColor::all(colors::COPPER_BORDER_WEAK) // 未聚焦时使用弱铜边。
                        Children [
                            SceneEditorPortalNameInputText // 动态回显作者名称、IME候选和光标。
                            bevy::text::EditableText { allow_newlines: false } // 官方Unicode与IME输入组件。
                            crate::ui::text_input::EditorTextInput // 编辑器焦点生命周期身份。
                            bevy::text::EditableTextFilter::new(|c| !char::is_control(c) && c != '|') // 名称不接受控制字符或列表保留分隔符。
                            bevy::text::TextCursorStyle::default() // 官方光标与选区绘制。
                            TextLayout::no_wrap() // 单行文本由官方滚动保持光标可见。
                            Node { width: percent(100), min_height: px(22), overflow: Overflow::clip_x() } // 空值保持点击区域。
                            TextFont {
                                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 中文名称必须使用完整字库。
                                font_size: px(ui_metrics::FONT_CONTROL), // 名称比辅助说明更醒目。
                            }
                            TextColor(colors::TEXT_PRIMARY) // 输入内容使用主文字色。
                        ]
                    --

                        Node {
                            width: percent(100), // 两个动作等分完整正文宽度。
                            column_gap: px(3), // 应用和删除保留明确边界。
                        }
                        Children [
                            @flex_editor_button("应用名称", EditorAction::PortalNameApply)--
                            @flex_editor_button("删除源点", EditorAction::PortalDeleteSelected)
                        ]

                ]
            --
            @step("2 · 新点的目标场景与落点")--

                SceneEditorTargetPortalListTitle
                Text("目标场景落点 · 尚未选择场景") @feathrise_editor_ui::localization::label_scene("目标场景落点 · 尚未选择场景")
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 目标标题继续使用项目中文字体。
                    font_size: px(9.0), // 与当前场景标题保持相同视觉层级。
                }
                TextColor(colors::TEXT_ACCENT)
            --

                SceneEditorTargetPortalList
                Node {
                    display: Display::None, // 目标清单未就绪时不保留空白行高。
                    width: percent(100), // 默认出生点和具体传送点共用完整宽度。
                    height: px(0), // 由目标场景全部真实落点数动态增长。
                    flex_direction: FlexDirection::Column, // 默认出生点固定为完整列表首行。
                    row_gap: px(2), // 每个可选落点保持明确点击间隔。
                    padding: UiRect::all(px(0)), // 行内自行提供铜边安全距离。
                    flex_shrink: 0.0, // 列表行不随窗口高度压缩。
                }
            --
            @step("3 · 新点连接预备")--

                SceneEditorPortalTargetText
                Text("请先在场景页高亮目标场景，再选择到达点") @feathrise_editor_ui::localization::label_scene("请先在场景页高亮目标场景，再选择到达点")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), padding: UiRect::all(px(4)), border: UiRect::all(px(1)), flex_shrink: 0.0 }
                BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK) // 长场景名与落点自然换行，不裁掉连接信息。

        ]
    }
}
