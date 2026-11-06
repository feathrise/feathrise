//! 工程与资产右键菜单共用固定节点及边界适配；沿用F12主题，不改变列表或预览布局。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*; // 原生节点、交互及变更检测。
use bevy::ui::FocusPolicy; // 透明遮罩阻止数字框和旧列表点透。
use crate::ui::theme as colors; // 统一使用项目已定义的颜色。

use super::components::{AssetMenuBackdrop, AssetMenuCopyButton, AssetMenuExternalButton, AssetMenuPopup, AssetMenuReferencesButton, AssetMenuFavoriteButton};
use crate::ui::external_files::{ExternalEditors, ExternalFileAction, EXTERNAL_MENU_ITEMS};
use super::helpers::{project_target_is_current, sized_menu_rect, target_is_current}; // 显示前复验Tasks阶段可能已经更新的目录。
use super::resources::AssetContextMenuState; // 唯一菜单会话。
use super::types::{AssetMenuSource, AssetPathCopyKind};
use crate::components::MapEditorRoot; // 菜单作为F12根的子节点，关闭时自然隐藏。
use crate::resources::{EditorAssetCatalog, MapEditorState}; // 只读来源和会话状态。
use crate::ui::sidebars::MapEditorSidebarState; // 离开左侧资产页立即撤下菜单。
use crate::{asset_catalog::project_files::EditorProjectFileCatalog, ui::project_browser::{ProjectBrowserContent, ProjectBrowserState, ProjectBrowserView, ProjectFileViewport}};

#[derive(SystemParam)]
pub(super) struct ProjectMenuView<'w, 's> {
    catalog: Option<Res<'w, EditorProjectFileCatalog>>,
    viewport: Option<Res<'w, ProjectFileViewport>>,
    scrolls: Query<'w, 's, (&'static ProjectBrowserContent, &'static ScrollPosition)>,
    browser: Option<Res<'w, ProjectBrowserState>>,
}

/// 编辑器根首次出现时创建一次菜单，之后只切显隐和位置。
pub(super) fn setup_menu(
    mut commands: Commands,                     // 固定菜单项只创建一次。
    roots: Query<Entity, Added<MapEditorRoot>>, // 不在每次右键时重复增删实体。
    assets: Res<AssetServer>,                   // 菜单中文使用项目正式字体。
) {
    if roots.is_empty() {
        return;
    } // 稳定帧不额外请求字体句柄。
    let font = feathrise_editor_core::editor_font(&assets, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"); // 与资产行相同字体。
    for root in &roots {
        commands.entity(root).with_children(|parent| {
            parent
                .spawn((
                    AssetMenuBackdrop, // 整个临时菜单的固定根。
                    Node {
                        display: Display::None,                // 没有菜单时完全退出布局及命中。
                        position_type: PositionType::Absolute, // 不占F12三栏Flex空间。
                        left: px(0),                           // 覆盖F12根的完整区域。
                        top: px(0),                            // 不受资产页滚动位置影响。
                        width: percent(100),                   // 关闭点击覆盖整个编辑器。
                        height: percent(100), // 避免窗口和全屏用固定尺寸留下点透区。
                        ..default()           // 其余保持普通透明节点默认值。
                    },
                    GlobalZIndex(426), // 高于F4与编辑器主体，低于正式新建/删除/冲突模态。
                    Interaction::default(), // 空白处点击可关闭且不会落入底层控件。
                    FocusPolicy::Block, // 即使背景完全透明也阻止旧数字框获得按下。
                ))
                .with_children(|overlay| {
                    overlay
                        .spawn((
                            AssetMenuPopup, // 真实小菜单独立于全屏命中遮罩。
                            Node {
                                position_type: PositionType::Absolute,   // 由当前右键锚点定位。
                                width: px(144), // 初始两种复制和文件夹三项，安装缓存就绪后按可见项调整。
                                height: px(86), // 三项24px按钮、行间距及边框内边距。
                                flex_direction: FlexDirection::Column,
                                row_gap: px(2),
                                padding: UiRect::all(px(4)), // 菜单边缘保持统一紧凑留白。
                                border: UiRect::all(px(1)), // 使用项目铜色细边。
                                border_radius: BorderRadius::all(px(4)), // 小型生产工具菜单圆角。
                                ..default()     // 不参与列表测量或滚动。
                            },
                            BackgroundColor(colors::PANEL_BG_STRONG), // 菜单文字不与底下资产行混在一起。
                            BorderColor::all(colors::COPPER_BORDER_WEAK), // 浮层外沿沿用弱灰分隔色。
                            FocusPolicy::Block,                       // 面板内边距同样不能点透。
                        ))
                        .with_children(|popup| {
                            for kind in AssetPathCopyKind::ALL {
                                popup
                                    .spawn((
                                        Button,              // 沿用Bevy左键交互语义。
                                        AssetMenuCopyButton(kind), // 两种形式均复制右键冻结的文件或目录。
                                        Node {
                                            width: percent(100),                     // 菜单项填满小面板的可用宽度。
                                            height: px(24), flex_shrink: 0.0,
                                            padding: UiRect::horizontal(px(8)), // 文字左侧提供一致留白。
                                            align_items: AlignItems::Center, // 窗口/全屏都严格垂直居中。
                                            border: UiRect::all(px(1)),      // 悬停时用铜边反馈。
                                            border_radius: BorderRadius::all(px(2)), // 内层圆角小于面板。
                                            ..default() // 保持单行按钮，不伸展外层资产页。
                                        },
                                        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), // 普通状态沿用F12按钮。
                                        BorderColor::all(colors::COPPER_BORDER_WEAK), // 默认弱化内层边框。
                                    ))
                                    .with_child((
                                        Text::new(kind.label()),
                                        TextFont {
                                            font: font.clone().into(), // 使用已验证的中文字体句柄。
                                            font_size: ui_metrics::FONT_CONTROL.into(),    // 与F12紧凑密度保持一致。
                                            ..default()                // 不引入另一套文字排版。
                                        },
                                        TextColor(colors::TEXT_PRIMARY), // 米白字与暗墨背景保持可读。
                                        FocusPolicy::Pass,               // 点击文字交给父复制按钮处理。
                                    )); // 固定文字节点不随菜单打开而重建。
                            }
                            popup.spawn((Button, AssetMenuFavoriteButton,
                                Node { width: percent(100), height: px(24), flex_shrink: 0.0,
                                    padding: UiRect::horizontal(px(8)), align_items: AlignItems::Center,
                                    border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)), ..default() },
                                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                BorderColor::all(colors::COPPER_BORDER_WEAK), FocusPolicy::Block,
                            )).with_child((Text::new("收藏"),
                                TextFont { font: font.clone().into(), font_size: ui_metrics::FONT_CONTROL.into(), ..default() },
                                TextLayout::no_wrap(), TextColor(colors::TEXT_PRIMARY), FocusPolicy::Pass));
                            popup.spawn((Button, AssetMenuReferencesButton,
                                Node { display: Display::None, width: percent(100), height: px(24), flex_shrink: 0.0,
                                    padding: UiRect::horizontal(px(8)), align_items: AlignItems::Center,
                                    border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)), ..default() },
                                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                BorderColor::all(colors::COPPER_BORDER_WEAK), FocusPolicy::Block,
                            )).with_child((Text::new("查看引用"),
                                TextFont { font: font.clone().into(), font_size: ui_metrics::FONT_CONTROL.into(), ..default() },
                                TextLayout::no_wrap(), TextColor(colors::TEXT_PRIMARY), FocusPolicy::Pass));
                            for (label, action) in EXTERNAL_MENU_ITEMS {
                                popup.spawn((Button, AssetMenuExternalButton(action),
                                    Node { display: if action.editor().is_some() { Display::None } else { Display::Flex }, width: percent(100), height: px(24), flex_shrink: 0.0,
                                        padding: UiRect::horizontal(px(8)), align_items: AlignItems::Center,
                                        border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)), ..default() },
                                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                    BorderColor::all(colors::COPPER_BORDER_WEAK), FocusPolicy::Block,
                                )).with_child((Text::new(label),
                                    TextFont { font: font.clone().into(), font_size: ui_metrics::FONT_CONTROL.into(), ..default() },
                                    TextLayout::no_wrap(), TextColor(colors::TEXT_PRIMARY), FocusPolicy::Pass));
                            }
                        }); // 菜单项仅属于当前小面板。
                }); // 全屏遮罩没有装饰背景，不遮暗编辑内容。
        }); // 跟随编辑器根生命周期自动清理。
    }
}

/// 目录采用、切页和窗口缩放后原位更新菜单，不扫描整个目录。
pub(super) fn sync_menu(
    mut menu: ResMut<AssetContextMenuState>, // Tasks刚采用新目录时也能撤销旧目标。
    catalog: Res<EditorAssetCatalog>,        // 只读取修订与单条来源。
    project: ProjectMenuView,
    editor: Res<MapEditorState>,             // 关闭F12或切图不遗留菜单。
    sidebar: Res<MapEditorSidebarState>,     // 菜单只属于资产页。
    layout: Option<Res<crate::resources::MapEditorLayoutState>>,
    roots: Query<Ref<ComputedNode>, With<MapEditorRoot>>, // 使用实际UI物理尺寸及最终倍率。
    available: Res<ExternalEditors>, // 只读后台完成的三份安装身份，菜单内不执行文件IO。
    shortcuts: Option<Res<crate::ui::asset_shortcuts::AssetShortcuts>>,
    favorites: Query<&Children, With<AssetMenuFavoriteButton>>,
    mut favorite_nodes: Query<&mut Node, (With<AssetMenuFavoriteButton>, Without<AssetMenuBackdrop>, Without<AssetMenuPopup>, Without<AssetMenuExternalButton>, Without<AssetMenuReferencesButton>)>,
    mut nodes: Query<
        (&mut Node, Option<&AssetMenuBackdrop>),
        (Or<(With<AssetMenuBackdrop>, With<AssetMenuPopup>)>, Without<AssetMenuReferencesButton>, Without<AssetMenuFavoriteButton>),
    >, // 一个查询修改两类Node，避免B0001重叠借用。
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (Or<(With<AssetMenuCopyButton>, With<AssetMenuExternalButton>, With<AssetMenuReferencesButton>, With<AssetMenuFavoriteButton>)>, Changed<Interaction>),
    >, // 固定菜单项仅按交互边沿回显。
    mut external_nodes: Query<(&AssetMenuExternalButton, &mut Node, Option<&Children>),
        (Without<AssetMenuBackdrop>, Without<AssetMenuPopup>, Without<AssetMenuReferencesButton>, Without<AssetMenuFavoriteButton>)>, // 与面板Node查询互斥，避免B0001。
    mut reference_nodes: Query<&mut Node, (With<AssetMenuReferencesButton>, Without<AssetMenuBackdrop>, Without<AssetMenuPopup>, Without<AssetMenuFavoriteButton>)>,
    mut labels: Query<&mut Text>, // 同一固定节点按文件/目录切文案，不重建菜单。
) {
    let project_scroll = project.scrolls.iter().find(|(view, _)| view.0 == ProjectBrowserView::ProjectFiles)
        .map(|(_, scroll)| scroll);
    if menu.target.as_ref().is_some_and(|target| {
        !sidebar.assets_browser_visible()
            || layout.as_ref().is_some_and(|layout| !layout.content_shown())
            || !match &target.source {
                AssetMenuSource::Assets => project.browser.as_ref().is_none_or(|browser| browser.assets_visible())
                    && target_is_current(target, &catalog, &editor),
                AssetMenuSource::Project { .. } => project.browser.as_ref().is_some_and(|browser| !browser.assets_visible())
                    && project.catalog.as_ref().zip(project.viewport.as_ref()).zip(project_scroll)
                        .is_some_and(|((catalog, viewport), scroll)| project_target_is_current(target, catalog, viewport, scroll, &editor)),
            }
    }) {
        menu.target = None; // 过期行不在新目录上继续展示“复制路径”。
        menu.captured_this_frame = true; // 同一帧刚关菜单也不能补触发底层鼠标。
    }
    let asset_target = menu.target.as_ref().is_none_or(|target| matches!(&target.source, AssetMenuSource::Assets));
    let file = menu.target.as_ref().and_then(|target| match &target.source {
        AssetMenuSource::Assets => catalog.entries.get(target.index),
        AssetMenuSource::Project { .. } => project.catalog.as_ref().and_then(|catalog| catalog.entries.get(target.index)),
    }).is_some_and(|entry| !entry.is_directory); // 文件定位首行，目录只打开当前项目。
    let favorite = asset_target && menu.target.as_ref().is_some_and(|target| shortcuts.as_ref()
        .is_some_and(|shortcuts| shortcuts.is_favorite(&target.path, !file)));
    for mut node in &mut favorite_nodes {
        let display = if asset_target { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    for children in &favorites {
        let value = if favorite { "取消收藏" } else { "收藏" };
        for child in children {
            if let Ok(mut text) = labels.get_mut(*child) && text.0 != value { text.0 = value.into(); }
        }
    }
    for mut node in &mut reference_nodes {
        let display = if asset_target && file { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    for (action, mut node, children) in &mut external_nodes {
        let display = if available.visible(action.0, file) { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        let label = if file {
            EXTERNAL_MENU_ITEMS.iter().find(|(_, item)| *item == action.0).map(|(label, _)| *label)
        } else {
            match action.0 {
                ExternalFileAction::VsCodeProject(_) => Some("用 VS Code 打开工程"),
                ExternalFileAction::ZedProject(_) => Some("用 Zed 打开工程"),
                ExternalFileAction::RustRoverProject(_) => Some("用 RustRover 打开工程"),
                _ => None,
            }
        }; // 目录不声称定位文件；切回文件时恢复原文案。
        if let (Some(label), Some(children)) = (label, children) {
            for child in children {
                if let Ok(mut text) = labels.get_mut(*child) && text.0 != label { text.0 = label.into(); }
            }
        }
    }
    if let Ok(root) = roots.single() {
        if menu.is_changed() || root.is_changed() || available.is_changed() {
            let viewport = root.size() * root.inverse_scale_factor(); // 所有位置和宽高一次换算成逻辑像素。
            let base_rows = if asset_target { if file { 4 } else { 3 } } else { 2 };
            let rect = sized_menu_rect(menu.anchor, viewport, available.menu_size(file, base_rows, 144.0));
            for (mut node, backdrop) in &mut nodes {
                if backdrop.is_some() {
                    node.display = if menu.target.is_some() {
                        Display::Flex
                    } else {
                        Display::None
                    }; // 不增删节点，不撑开列表。
                } else {
                    node.left = px(rect.min.x); // 右边缘自动向内收。
                    node.top = px(rect.min.y); // 底边缘自动向上收。
                    node.width = px(rect.width()); // 极窄根也不允许菜单越界。
                    node.height = px(rect.height()); // 高倍率或极低窗口保持在根内。
                }
            }
        }
    }
    for (interaction, mut background, mut border) in &mut buttons {
        background.0 = match interaction {
            Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED, // 按下反馈只影响菜单项。
            Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED, // 悬停明确显示可复制。
            Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,     // 离开恢复原样。
        };
        *border = BorderColor::all(if *interaction == Interaction::None {
            colors::COPPER_BORDER_WEAK
        } else {
            colors::COPPER_BORDER
        }); // 文案与铜边反馈一致。
    }
}
