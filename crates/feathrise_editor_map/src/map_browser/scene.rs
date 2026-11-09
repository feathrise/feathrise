//! 虚拟文件夹共用模态框和地图拖拽名称浮层。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use bevy::prelude::*; // BSN Scene、文本和UI布局组件。
use bevy::text::{FontSourceTemplate, LineHeight}; // 中文字段使用项目字体与真实单行度量。
use bevy::ui::{FocusPolicy, RelativeCursorPosition}; // 可见模态阻止点透，正文与反馈独立滚动。
use crate::ui::theme as colors; // 复用F12暗墨铜金视觉令牌。

use crate::components::{
    MapBrowserDragPreview, MapBrowserDragPreviewText, MapFolderDialog, MapFolderDialogNameGroup,
    MapFolderDialogNameText, MapFolderDialogStatusText, MapFolderDialogTitle,
}; // 动态显隐与文字更新全部依赖稳定marker。
use crate::types::EditorAction; // 输入焦点、确认和取消使用结构化动作。
use crate::ui::management_dialogs::{DialogKind, components::*, scene as modal};
use crate::ui::scene::FONT_PATH;

/// 创建、重命名和删除空文件夹共用同一个阻止点透模态层。
pub(crate) fn map_folder_dialog() -> impl Scene {
    bsn! {
        MapFolderDialog UiInteraction::default() FocusPolicy::default()
        Node {
            display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: percent(100), height: percent(100),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::all(px(8)),
        }
        GlobalZIndex(445)
        BackgroundColor(colors::OVERLAY_BG)
        Children [
            ManagementPanel(DialogKind::Folder)
            ~{modal::panel_node()}
            BackgroundColor(colors::PANEL_BG_STRONG)
            BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [

                    MapFolderDialogTitle Text("虚拟地图文件夹") @feathrise_editor_ui::localization::label_scene("虚拟地图文件夹")
                    TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(13) }
                    TextColor(colors::TEXT_ACCENT)
                    Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                --

                    ManagementScroll(DialogKind::Folder)
                    ScrollPosition::default() RelativeCursorPosition::default()
                    ~{modal::body_node()}
                    Children [

                            FolderTargetText Text("等待冻结目标") @feathrise_editor_ui::localization::label_scene("等待冻结目标")
                            TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(11) }
                            TextColor(colors::TEXT_PRIMARY)
                            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                        --

                            MapFolderDialogNameGroup
                            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: px(4) }
                            Children [
                                @modal::caption("显示名称 · 支持中文")--

                                    InteractionButton
                                    crate::components::MapEditorActionButton(EditorAction::FocusMapFolderName)
                                    crate::components::MapEditorCustomActionStyle
                                    Node {
                                        width: percent(100), height: px(26), min_width: px(0), flex_shrink: 0.0,
                                        align_items: AlignItems::Center,
                                        padding: UiRect::horizontal(px(6)), border: px(1),
                                        border_radius: BorderRadius::all(px(3)),
                                    }
                                    BackgroundColor(colors::INPUT_BG)
                                    BorderColor::all(colors::INPUT_ACTIVE_BORDER)
                                    Children [
                                        // 保留唯一官方名称根、原焦点动作及Unicode/IME协议，只修正实际行高居中。
                                        MapFolderDialogNameText
                                        bevy::text::EditableText { allow_newlines: false }
                                        crate::ui::text_input::EditorTextInput
                                        bevy::text::EditableTextFilter::new(|character| !character.is_control())
                                        bevy::text::TextCursorStyle::default()
                                        TextLayout::no_wrap() ~{LineHeight::Px(16.0)}
                                        Node { width: percent(100), min_width: px(0), height: px(16), overflow: Overflow::clip_x() }
                                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(11) }
                                        TextColor(colors::TEXT_PRIMARY)
                                    ]

                            ]
                        --
                        @modal::caption("影响范围")--

                            FolderConsequencesText Text("")
                            TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(10.5) }
                            TextColor(colors::TEXT_MUTED)
                            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }

                    ]
                --

                    ManagementScroll(DialogKind::Folder) ManagementFeedback(DialogKind::Folder)
                    ScrollPosition::default() RelativeCursorPosition::default()
                    ~{modal::feedback_node()}
                    BackgroundColor(colors::PANEL_BG_SECTION)
                    Children [
                        MapFolderDialogStatusText Text("等待操作") @feathrise_editor_ui::localization::label_scene("等待操作")
                        TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(10.5) }
                        TextColor(colors::TEXT_PRIMARY)
                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                    ]
                --

                    ManagementFooter(DialogKind::Folder)
                    ~{modal::footer_node()}
                    Children [
                        @modal::button("取消", EditorAction::CancelMapFolderDialog)--
                        @modal::folder_confirm_button()
                    ]

            ]
        ]
    }
}

/// 构造不参与命中的地图或文件夹名称拖拽浮层。
pub(crate) fn map_browser_drag_preview() -> impl Scene {
    bsn! {
        MapBrowserDragPreview // 唯一浮层根由拖拽状态原地移动。
        Pickable::IGNORE ~{FocusPolicy::Pass} // 浮层与文字从创建起就不参与鼠标命中。
        Node {
            display: Display::None, // 只有超过六物理像素阈值后显示。
            position_type: PositionType::Absolute, // 布局后只平移浮层，避免重复应用编辑器缩放。
            left: px(0), top: px(0),
            width: px(240), max_width: percent(95), min_width: px(0), // 明确正文可用宽度，不能把中文压缩成空方框。
            min_height: px(26), // 单行拖拽卡保持明确落点反馈。
            flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(8), px(5)), // 换行后文字四周仍留有空隙。
            border: UiRect::all(px(1)), // 与有效落点形成同一铜边语言。
        }
        GlobalZIndex(450) // 高于所有编辑器模态和主体，但仍不参与命中。
        BackgroundColor(colors::PANEL_BG_STRONG) // 暗墨浮层不遮蔽名称可读性。
        BorderColor::all(colors::COPPER_BORDER) // 铜边明确正在拖动目录对象。
        Children [
            MapBrowserDragPreviewText // 显示冻结名称及无效落点原因。
            Pickable::IGNORE ~{FocusPolicy::Pass}
            Text("拖动目录项") @feathrise_editor_ui::localization::label_scene("拖动目录项") // 隐藏首帧占位。
            TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 } // 正文使用明确宽度度量，长名称与拒绝原因自动换行。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 中文地图名使用项目字体。
                font_size: px(10), // 紧凑浮层字号。
            }
            TextColor(colors::TEXT_ACCENT) // 铜金文字突出拖拽身份。
            TextShadow { offset: Vec2::splat(1.0) } // 与公共提示一致，提高中文笔画在地图背景上的可读性。
        ]
    }
}
