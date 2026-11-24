//! 左侧场景树的显示预览与摆放列表布局；显示入口不依赖地形对象。

use feathrise_editor_ui::UiInteraction;
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
    TREE_BRANCH_GAP, TREE_CAPTION_GAP, TREE_ICON_SIZE, TREE_ROOT_BORDER, load_tree_branch_images, sync_tree_branch_icon, tree_branch_button,
    tree_branch_fill, tree_object_branch_button, tree_root_row_left, tree_row_left,
};

#[derive(Component, Clone, Default)] // 场景显示入口独立于场景对象行。
pub(crate) struct SceneBaseGltfRow;
#[derive(Component, Clone, Default)] // 显示入口也使用场景树共用箭头。
pub(crate) struct SceneBaseGltfArrow;
#[derive(Component, Clone, Default)] // 预览按钮区随场景显示入口展开或收起。
pub(crate) struct SceneBaseTerrainDetails;
#[derive(Component, Clone, Default)]
pub(crate) struct SceneTreeRootRow;
#[derive(Component, Clone, Default)]
pub(crate) struct SceneTreeContents;

pub(crate) fn sync_scene_base_gltf_row(
    session: Res<crate::authoring::EditorAuthoringSession>,
    editor: Res<crate::resources::SceneEditorState>,
    shading: Res<crate::resources::TerrainShadingSession>,
    server: Option<Res<AssetServer>>,
    image_assets: Option<Res<Assets<Image>>>,
    mut images: Local<Option<[Handle<Image>; 2]>>,
    mut rows: Query<
        (&UiInteraction, &mut Node, &mut BackgroundColor),
        (With<SceneBaseGltfRow>, Without<SceneBaseTerrainDetails>), // 与下方按钮区的可变Node查询静态互斥。
    >,
    mut arrows: Query<(&UiInteraction, &Children, &mut BackgroundColor), (With<SceneBaseGltfArrow>, Without<SceneBaseGltfRow>)>,
    mut icons: Query<&mut ImageNode>,
    mut details: Query<
        &mut Node,
        (With<SceneBaseTerrainDetails>, Without<SceneBaseGltfRow>),
    >,
) {
    // 标题随场景树常驻；预览设置只在当前场景草稿就绪后可展开，包括零地形场景。
    let available = editor.open && !editor.scene_transition_in_progress
        && session.draft.as_ref().is_some_and(|draft| {
            editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
        });
    let expanded = available && shading.preview_expanded;
    let row_display = if editor.open { Display::Flex } else { Display::None };
    for (interaction, mut node, mut background) in &mut rows {
        if node.display != row_display {
            node.display = row_display;
        } // 稳定帧不重复改布局。
        let color = if expanded {
            colors::SELECT_BG
        } else {
            match interaction {
                UiInteraction::Pressed if available => colors::BUTTON_DEV_SMALL_PRESSED,
                UiInteraction::Hovered if available => colors::BUTTON_DEV_SMALL_HOVERED,
                _ => colors::SLOT_BG,
            }
        };
        if background.0 != color {
            background.0 = color; // 只在交互或选择真实改变时更新UI。
        }
    }
    if images.is_none() && image_assets.is_some() && let Some(server) = server.as_deref() {
        *images = Some(load_tree_branch_images(server));
    }
    for (interaction, children, mut fill) in &mut arrows {
        fill.set_if_neq(tree_branch_fill(available, *interaction));
        if let Some(images) = &*images && let Some(child) = children.first()
            && let Ok(mut image) = icons.get_mut(*child)
        {
            sync_tree_branch_icon(!expanded, available, images, &mut image);
        }
    }
    let details_display = if expanded { Display::Flex } else { Display::None };
    for mut node in &mut details {
        if node.display != details_display {
            node.display = details_display;
        }
    }
}

/// 场景显示是场景级工具，标题可独立于滚动的设置区布局。
pub(crate) fn scene_display_button() -> impl Scene {
    bsn! {
        InteractionButton SceneBaseGltfRow
        Node {
            display: Display::None, // 首次界面同步后随编辑器一起显示。
            width: percent(100), min_width: px(0), flex_shrink: 0.0,
            padding: UiRect { left: tree_row_left(0), right: px(5), top: px(2), bottom: px(2) },
            align_items: AlignItems::Center, column_gap: px(TREE_BRANCH_GAP),
            border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(colors::SLOT_BG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @tree_branch_button() SceneBaseGltfArrow--
            Text("场景显示 · 网格 / 碰撞 / 着色") Pickable::IGNORE
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
            TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() }
        ]
    }
}

/// 类型图标不独占输入，由树行或其展开控件接收点击。
pub(super) fn gltf_row_icon() -> impl Scene {
    bsn! { SceneEditorGltfBrowserRowGlyph ImageNode::default() Pickable::IGNORE
        Node { width: px(TREE_ICON_SIZE), height: px(TREE_ICON_SIZE), flex_shrink: 0.0 } }
}

/// 主场景 GLB 复用窄箭头槽，固定在可滚动子树上方。
pub(crate) fn scene_tree_root_row() -> impl Scene {
    let row = SceneEditorGltfBrowserRow { slot: 0, entry_index: None, entries_revision: 0, main_glb: None };
    bsn! { SceneTreeRootRow InteractionButton SceneObjectRow::default() bevy::ui::RelativeCursorPosition::default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // 真实App启动时两个可变Node查询必须互斥，不能只做纯标签测试。
    fn scene_display_panel_stays_visible_without_terrain() {
        let draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        let scene_id = draft.scene_id.clone();
        let mut app = App::new(); // 不需要窗口或GPU也能检验SystemParam初始化。
        app.init_resource::<crate::authoring::EditorAuthoringSession>()
            .init_resource::<crate::resources::SceneEditorState>()
            .init_resource::<crate::resources::TerrainShadingSession>()
            .add_systems(Update, sync_scene_base_gltf_row); // 执行引发B0001的真实生产系统。
        app.world_mut()
            .resource_mut::<crate::authoring::EditorAuthoringSession>()
            .draft = Some(draft);
        let editor = &mut app
            .world_mut()
            .resource_mut::<crate::resources::SceneEditorState>();
        editor.open = true;
        editor.selected_scene_id = Some(scene_id); // 编辑器正查看同一场景。
        app.world_mut()
            .resource_mut::<crate::resources::TerrainShadingSession>()
            .preview_expanded = true;
        let row = app
            .world_mut()
            .spawn((
                SceneBaseGltfRow,
                UiInteraction::None,
                Node::default(),
                BackgroundColor::default(),
            ))
            .id(); // 按钮根唯一持有Node。
        let details = app
            .world_mut()
            .spawn((SceneBaseTerrainDetails, Node::default()))
            .id(); // 属性根持有另一份Node。
        app.update(); // 旧版此处在实际系统初始化时抛出B0001。
        assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::Flex);
        assert_eq!(app.world().get::<Node>(details).unwrap().display, Display::Flex);
        {
            let mut session = app.world_mut().resource_mut::<crate::authoring::EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
            draft.manifest.terrains = Some(Vec::new().into());
        }
        app.update(); // 删除最后一个地形时保留展开的预览。
        assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::Flex);
        assert_eq!(app.world().get::<Node>(details).unwrap().display, Display::Flex);
        app.world_mut().resource_mut::<crate::resources::SceneEditorState>()
            .scene_transition_in_progress = true;
        app.update(); // 切换期间保留标题，旧场景的控制区暂时收起。
        assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::Flex);
        assert_eq!(app.world().get::<Node>(details).unwrap().display, Display::None);
        app.world_mut().resource_mut::<crate::authoring::EditorAuthoringSession>().draft = None;
        app.world_mut().resource_mut::<crate::resources::SceneEditorState>()
            .scene_transition_in_progress = false;
        app.update(); // 工作台没有场景草稿时也保留入口。
        assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::Flex);
        assert_eq!(app.world().get::<Node>(details).unwrap().display, Display::None);
        app.world_mut().resource_mut::<crate::resources::SceneEditorState>().open = false;
        app.update();
        assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::None);
    }
}
