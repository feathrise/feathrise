//! 左侧场景树主 GLB 根行、类型图标和摆放列表布局。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN Scene与UI布局组件。
use bevy::text::FontSourceTemplate; // 静态按钮和状态使用项目中文字体。
use crate::ui::theme as colors; // 与右栏资产浏览器共享水墨铜边配色。

use super::{
    PlacementTreeExpander, SceneEditorGltfBrowserList, SceneEditorGltfBrowserListContent,
    SceneEditorGltfBrowserRow, SceneEditorGltfBrowserRowText, SceneEditorGltfBrowserStatusText,
    systems::GltfRowCaption,
};
use crate::components::SceneEditorGltfBrowserRowGlyph;
use crate::ui::list_layout::SceneObjectListScroll;
use crate::ui::scene_context_menu::SceneObjectRow;
use crate::ui::scene::FONT_PATH; // 复用主编辑器字体常量。
use crate::ui::scene_sidebar::{
    TREE_BRANCH_GAP, TREE_CAPTION_GAP, TREE_ICON_SIZE, TREE_ROOT_BORDER, tree_object_branch_button, tree_root_row_left,
};

#[derive(Component, Clone, Default)]
pub(crate) struct SceneTreeRootRow;
#[derive(Component, Clone, Default)]
pub(crate) struct SceneTreeContents;

/// 类型图标不独占输入，由树行或其展开控件接收点击。
pub(super) fn gltf_row_icon() -> impl Scene {
    bsn! { SceneEditorGltfBrowserRowGlyph ImageNode::default() Pickable::IGNORE
        Node { width: px(TREE_ICON_SIZE), height: px(TREE_ICON_SIZE), flex_shrink: 0.0 } }
}

/// 主场景 GLB 复用窄箭头槽，固定在可滚动子树上方。
pub(crate) fn scene_tree_root_row() -> impl Scene {
    let row = SceneEditorGltfBrowserRow { slot: 0, entry_index: None, entries_revision: 0, main_glb: None };
    bsn! { SceneTreeRootRow ~{Name::new("主场景根")} InteractionButton SceneObjectRow::default() bevy::ui::RelativeCursorPosition::default()
        ~{row}
        Node { display: Display::None, width: percent(100), min_width: px(0), height: px(25), flex_shrink: 0.0,
            padding: UiRect { left: tree_root_row_left(), right: px(5), ..UiRect::ZERO }, align_items: AlignItems::Center, column_gap: px(TREE_BRANCH_GAP),
            border: UiRect::all(px(TREE_ROOT_BORDER)), border_radius: BorderRadius::all(px(3)), overflow: Overflow::clip_x() }
        BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @tree_object_branch_button() PlacementTreeExpander::default() bevy::ui::InteractionDisabled Visibility::Hidden Pickable::IGNORE--
            GltfRowCaption Pickable::IGNORE
            Node { align_items: AlignItems::Center, column_gap: px(TREE_CAPTION_GAP), flex_grow: 1.0, min_width: px(0), overflow: Overflow::clip_x() }
            Children [
                @gltf_row_icon()--
                SceneEditorGltfBrowserRowText Text("") Pickable::IGNORE TextLayout::no_wrap()
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_ACCENT) Node { min_width: px(0), overflow: Overflow::clip_x() }
            ]
        ]
    }
}

/// 构造右栏上半区的已摆放实例与Blender网格浏览器。
pub(super) fn gltf_browser_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100), // 与右栏完整宽度一致。
            min_width: px(0),
            flex_shrink: 0.0, // 下方资产列表自行使用剩余弹性空间。
            flex_direction: FlexDirection::Column,
            row_gap: px(3), // 与主右栏分组密度一致。
            overflow: Overflow::clip(), // 超长路径和极低窗口不越出右栏。
        }
        Children [
            @crate::ui::scene_layers::panel()-- // 独立作者内容与摆放共用同一场景树。
            @crate::ui::subscenes::panel()-- // 子场景引用属于正式场景树，加载期间也保留可选来源行。

                SceneEditorGltfBrowserList SceneObjectListScroll
                bevy::ui::RelativeCursorPosition::default() ScrollPosition::default()
                Node {
                    display: Display::None, width: percent(100), min_width: px(0),
                    height: px(0), flex_shrink: 0.0, overflow: Overflow::scroll_y(),
                }
                // 内容层为完整树留出滚动范围，可见行由固定实体池渲染。
                Children [
                    SceneEditorGltfBrowserListContent
                    Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0 }
                ]
            --

                SceneEditorGltfBrowserStatusText
                ~{crate::ui::status_bar::PanelStatusText("场景资产")} feathrise_editor_ui::feedback::GlobalPanelStatus
                Text("场景对象：等待场景草稿") @feathrise_editor_ui::localization::label_scene("场景对象：等待场景草稿")
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 与动态行共享字体资产。
                    font_size: px(ui_metrics::FONT_HINT), // 状态保持可读字号，不靠缩字容纳长路径。
                }
                TextColor(colors::TEXT_HINT) // 摘要弱于可点击树行。
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node {
                    display: Display::None, // 就绪且零摆放时不保留空分支；真实等待/错误由状态系统显式展开。
                    width: percent(100), // 长加载摘要在右栏内裁切。
                    min_width: px(0),
                    flex_shrink: 0.0, // 不被列表弹性挤掉。
                    overflow: Overflow::clip(), // 选择路径过长也不撑开布局。
                    padding: UiRect::all(px(3)), // 摘要高度只由真实文字与内边距决定。
                }

        ]
    }
}
