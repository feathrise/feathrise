//! 文件标签菜单沿用资产路径菜单的暗墨铜边、紧凑尺寸与根内定位。
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, resources::AssetTabMenuState};
use crate::ui::asset_context_menu::helpers::sized_menu_rect;
use crate::ui::external_files::{ExternalEditors, EXTERNAL_MENU_ITEMS};
use crate::ui::asset_context_menu::AssetPathCopyKind;
use crate::{
    components::MapEditorRoot,
    ui::{scene::FONT_PATH, theme as colors},
};
use bevy::{prelude::*, ui::FocusPolicy};

/// 固定菜单树只在编辑器根建立时创建，右击仅切换显隐。
pub(super) fn setup(
    mut commands: Commands,
    roots: Query<Entity, Added<MapEditorRoot>>,
    assets: Res<AssetServer>,
) {
    if roots.is_empty() {
        return;
    }
    let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
    for root in &roots {
        commands.entity(root).with_children(|parent| {
            parent
                .spawn((
                    TabMenuBackdrop,
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    GlobalZIndex(426),
                    Interaction::default(),
                    FocusPolicy::Block,
                ))
                .with_children(|overlay| {
                    overlay
                        .spawn((
                            TabMenuPopup,
                            Node {
                                position_type: PositionType::Absolute,
                                width: px(168), // 初始三项批量动作，文件路径与安装缓存就绪后再扩展。
                                height: px(86),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(2),
                                padding: UiRect::all(px(4)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(colors::PANEL_BG_STRONG),
                            BorderColor::all(colors::COPPER_BORDER_WEAK),
                            FocusPolicy::Block,
                        ))
                        .with_children(|popup| {
                            for kind in AssetPathCopyKind::ALL {
                                popup.spawn((InteractionButton, CopyFilePathButton(kind),
                                    Node { display: Display::None, width: percent(100), height: px(24), flex_shrink: 0.0,
                                        padding: UiRect::horizontal(px(8)), align_items: AlignItems::Center,
                                        border_radius: BorderRadius::all(px(2)), ..default() },
                                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), FocusPolicy::Block,
                                )).with_child((feathrise_editor_ui::localization::editor_label(kind.label()),
                                    TextFont { font: font.clone().into(), font_size: ui_metrics::FONT_CONTROL.into(), ..default() },
                                    TextLayout::no_wrap(), TextColor(colors::TEXT_PRIMARY), FocusPolicy::Pass));
                            }
                            for (label, action) in EXTERNAL_MENU_ITEMS {
                                popup.spawn((InteractionButton, ExternalFileButton(action),
                                    Node { display: if action.editor().is_some() { Display::None } else { Display::Flex }, width: percent(100), height: px(24), flex_shrink: 0.0,
                                        padding: UiRect::horizontal(px(8)), align_items: AlignItems::Center,
                                        border_radius: BorderRadius::all(px(2)), ..default() },
                                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), FocusPolicy::Block,
                                )).with_child((feathrise_editor_ui::localization::editor_label(label),
                                    TextFont { font: font.clone().into(), font_size: ui_metrics::FONT_CONTROL.into(), ..default() },
                                    TextLayout::no_wrap(), TextColor(colors::TEXT_PRIMARY), FocusPolicy::Pass));
                            }
                            popup
                                .spawn((
                                    InteractionButton,
                                    SaveAllDraftsButton,
                                    Node {
                                        width: percent(100), height: px(24), flex_shrink: 0.0,
                                        padding: UiRect::horizontal(px(8)), align_items: AlignItems::Center,
                                        border_radius: BorderRadius::all(px(2)), ..default()
                                    },
                                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), FocusPolicy::Block,
                                ))
                                .with_child((
                                    feathrise_editor_ui::localization::editor_label("保存全部草稿"),
                                    TextFont { font: font.clone().into(), font_size: ui_metrics::FONT_CONTROL.into(), ..default() },
                                    TextColor(colors::TEXT_PRIMARY), FocusPolicy::Pass,
                                ));
                            popup
                                .spawn((
                                    InteractionButton,
                                    CloseAllTabsButton,
                                    Node {
                                        width: percent(100),
                                        height: px(24),
                                        flex_shrink: 0.0,
                                        padding: UiRect::horizontal(px(8)),
                                        align_items: AlignItems::Center,
                                        border_radius: BorderRadius::all(px(2)),
                                        ..default()
                                    },
                                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                    FocusPolicy::Block,
                                ))
                                .with_child((
                                    feathrise_editor_ui::localization::editor_label("关闭全部"),
                                    TextFont {
                                        font: font.clone().into(),
                                        font_size: ui_metrics::FONT_CONTROL.into(),
                                        ..default()
                                    },
                                    TextColor(colors::TEXT_PRIMARY),
                                    FocusPolicy::Pass,
                                ));
                            popup
                                .spawn((
                                    InteractionButton,
                                    CloseUnmodifiedTabsButton,
                                    Node {
                                        width: percent(100),
                                        height: px(24),
                                        flex_shrink: 0.0,
                                        padding: UiRect::horizontal(px(8)),
                                        align_items: AlignItems::Center,
                                        border_radius: BorderRadius::all(px(2)),
                                        ..default()
                                    },
                                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                    FocusPolicy::Block,
                                ))
                                .with_child((
                                    feathrise_editor_ui::localization::editor_label("关闭未修改"),
                                    TextFont {
                                        font: font.clone().into(),
                                        font_size: ui_metrics::FONT_CONTROL.into(),
                                        ..default()
                                    },
                                    TextColor(colors::TEXT_PRIMARY),
                                    FocusPolicy::Pass,
                                ));
                        });
                });
        });
    }
}

pub(super) fn sync(
    menu: Res<AssetTabMenuState>,
    available: Res<ExternalEditors>,
    roots: Query<Ref<ComputedNode>, With<MapEditorRoot>>,
    mut nodes: Query<
        (&mut Node, Has<TabMenuBackdrop>),
        Or<(With<TabMenuBackdrop>, With<TabMenuPopup>)>,
    >,
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            Or<(With<CloseAllTabsButton>, With<CloseUnmodifiedTabsButton>, With<SaveAllDraftsButton>, With<ExternalFileButton>, With<CopyFilePathButton>)>,
            Changed<Interaction>,
        ),
    >,
    mut external_buttons: Query<(&ExternalFileButton, &mut Node), (Without<TabMenuPopup>, Without<TabMenuBackdrop>, Without<CopyFilePathButton>)>,
    mut copy_buttons: Query<&mut Node, (With<CopyFilePathButton>, Without<TabMenuPopup>, Without<TabMenuBackdrop>, Without<ExternalFileButton>)>,
) {
    if let Ok(root) = roots.single()
        && (menu.is_changed() || root.is_changed() || available.is_changed())
    {
        let rect = sized_menu_rect(
            menu.anchor,
            root.size() * root.inverse_scale_factor(),
            if menu.path.is_some() { available.menu_size(true, 5, 168.0) } else { Vec2::new(168.0, 86.0) },
        );
        for (mut node, backdrop) in &mut nodes {
            if backdrop {
                node.display = if menu.open {
                    Display::Flex
                } else {
                    Display::None
                };
            } else {
                node.left = px(rect.min.x);
                node.top = px(rect.min.y);
                node.width = px(rect.width());
                node.height = px(rect.height());
            }
        }
    }
    for (action, mut node) in &mut external_buttons {
        let display = if menu.path.is_some() && available.visible(action.0, true) { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    for mut node in &mut copy_buttons {
        let display = if menu.path.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    for (interaction, mut background) in &mut buttons {
        background.0 = match interaction {
            Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
            Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
        };
    }
}
