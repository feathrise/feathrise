//! F4 动态地图监控面板实体创建。
//! 【功能设计】尺寸、位置、两级页签、六类植被LOD控件及正文层级复刻F3，数据域覆盖完整动态地图草稿。

use feathrise_editor_ui::InteractionButton;
use bevy::prelude::*;
use bevy::text::LineHeight;
use bevy::ui::FocusPolicy;

use super::helpers::{
    DIAGNOSTIC_FONT_SIZE, DIAGNOSTIC_LINE_HEIGHT, resource_monitor_content_node,
    resource_monitor_panel_node, resource_monitor_subtab_node, resource_monitor_tab_node,
    resource_monitor_tabs_node, resource_monitor_text_node,
};
#[cfg(test)]
use super::lod_controls::EditorVegetationLodControls;
use super::lod_controls::spawn_vegetation_lod_controls;
use crate::components::{
    MapEditorMonitorGroupButton, MapEditorMonitorPageText, MapEditorMonitorPanel,
    MapEditorMonitorSubTabRow, MapEditorMonitorTabButton,
};
use crate::monitor_resources::{EditorMapMonitorGroup, EditorMapMonitorPage};

#[derive(Component)]
pub(super) struct MonitorTabLabel(pub EditorMapMonitorPage);

/// 创建默认隐藏、默认选中“总览”的F3同款F4面板。
pub(super) fn spawn_editor_map_monitor_panel(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let font = feathrise_editor_core::editor_font(&asset_server, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf");
    spawn_editor_map_monitor_panel_with_font(&mut commands, font);
}

fn spawn_editor_map_monitor_panel_with_font(commands: &mut Commands, font: Handle<Font>) {
    commands
        .spawn((
            MapEditorMonitorPanel,
            feathrise_editor_ui::EditorThemeRoot, // F4独立根属于编辑器，共享按钮反馈不外溢到F3游戏监控。
            resource_monitor_panel_node(),
            BackgroundColor(Color::srgba(0.035, 0.030, 0.024, 0.88)),
            BorderColor::all(Color::srgba(0.745, 0.529, 0.243, 0.56)),
            Visibility::Hidden,
            FocusPolicy::Pass,
            // F4 需要盖住 F12 的 420 层主体；除层级外与 F3 的 260 层面板保持一致。
            GlobalZIndex(425),
        ))
        .with_children(|panel| {
            panel.spawn((
                feathrise_editor_ui::localization::editor_label("动态地图监控 [F4] · 0.5s刷新 · 选择分页查看"),
                TextFont {
                    font: font.clone().into(),
                    font_size: 14.0.into(),
                    ..default()
                },
                TextColor(Color::srgba(0.922, 0.886, 0.804, 0.94)),
                TextShadow {
                    offset: Vec2::new(1.0, 1.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.75),
                },
                LineHeight::RelativeToFont(DIAGNOSTIC_LINE_HEIGHT),
                FocusPolicy::Pass,
            ));

            panel
                .spawn((resource_monitor_tabs_node(), FocusPolicy::Pass))
                .with_children(|tabs| {
                    for group in EditorMapMonitorGroup::ALL {
                        let selected = group == EditorMapMonitorGroup::Overview; // 首次打开先给出全图资产摘要。
                        tabs.spawn((
                            InteractionButton,
                            MapEditorMonitorGroupButton { group },
                            resource_monitor_tab_node(),
                            BackgroundColor(if selected {
                                Color::srgba(0.38, 0.24, 0.10, 0.94)
                            } else {
                                Color::srgba(0.055, 0.044, 0.035, 0.86)
                            }),
                            BorderColor::all(if selected {
                                Color::srgba(0.88, 0.66, 0.31, 0.92)
                            } else {
                                Color::srgba(0.52, 0.38, 0.19, 0.52)
                            }),
                            FocusPolicy::Block,
                        ))
                        .with_child((
                            Text::new(group.label()),
                            TextFont {
                                font: font.clone().into(),
                                font_size: 13.0.into(),
                                ..default()
                            },
                            TextColor(Color::srgba(0.94, 0.88, 0.73, 0.96)),
                            FocusPolicy::Pass,
                        ));
                    }
                });

            let mut subtab_row_node = resource_monitor_tabs_node(); // 二级按钮池固定创建，但总览默认不占布局高度。
            subtab_row_node.display = Display::None; // 首次页面状态与EditorMapMonitorState::default保持一致。
            panel
                .spawn((
                    MapEditorMonitorSubTabRow,
                    subtab_row_node,
                    FocusPolicy::Pass,
                ))
                .with_children(|tabs| {
                    for page in EditorMapMonitorPage::SUBTABS {
                        let belongs_to_default_group =
                            page.group() == EditorMapMonitorGroup::Overview;
                        let selected = page == EditorMapMonitorPage::Overview;
                        tabs.spawn((
                            InteractionButton,
                            MapEditorMonitorTabButton { page },
                            resource_monitor_subtab_node(belongs_to_default_group),
                            BackgroundColor(if selected {
                                Color::srgba(0.38, 0.24, 0.10, 0.94)
                            } else {
                                Color::srgba(0.055, 0.044, 0.035, 0.86)
                            }),
                            BorderColor::all(if selected {
                                Color::srgba(0.88, 0.66, 0.31, 0.92)
                            } else {
                                Color::srgba(0.52, 0.38, 0.19, 0.52)
                            }),
                            FocusPolicy::Block,
                        ))
                        .with_child((
                            MonitorTabLabel(page),
                            Text::new(page.display_label(None)),
                            TextFont {
                                font: font.clone().into(),
                                font_size: 12.0.into(),
                                ..default()
                            },
                            TextColor(Color::srgba(0.94, 0.88, 0.73, 0.96)),
                            FocusPolicy::Pass,
                        ));
                    }
                });

            spawn_vegetation_lod_controls(panel, &font);

            panel
                .spawn((
                    resource_monitor_content_node(),
                    BackgroundColor(Color::srgba(0.044, 0.036, 0.030, 0.64)),
                    BorderColor::all(Color::srgba(0.520, 0.380, 0.190, 0.38)),
                    FocusPolicy::Pass,
                ))
                .with_children(|content| {
                    for page in EditorMapMonitorPage::ALL {
                        let initial =
                            format!("{}\n等待当前动态地图草稿的缓存数据...", page.label()); // 每个固定页面都已接入统一低频报告，不再保留伪占位分类。
                        content.spawn((
                            MapEditorMonitorPageText { page },
                            Text::new(initial),
                            TextFont {
                                font: font.clone().into(),
                                font_size: DIAGNOSTIC_FONT_SIZE.into(),
                                ..default()
                            },
                            TextColor(Color::srgba(0.922, 0.886, 0.804, 0.94)),
                            TextShadow {
                                offset: Vec2::new(1.0, 1.0),
                                color: Color::srgba(0.0, 0.0, 0.0, 0.75),
                            },
                            TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                            LineHeight::RelativeToFont(DIAGNOSTIC_LINE_HEIGHT),
                            resource_monitor_text_node(page == EditorMapMonitorPage::Overview),
                            FocusPolicy::Pass,
                        ));
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn panel_clones_f3_hierarchy_and_defaults_to_overview() {
        let mut world = World::new();
        world
            .run_system_once(|mut commands: Commands| {
                spawn_editor_map_monitor_panel_with_font(&mut commands, Handle::<Font>::default());
            })
            .expect("F4 编辑器地图监控应能在最小 World 中创建");

        let panel_visibility = *world
            .query_filtered::<&Visibility, With<MapEditorMonitorPanel>>()
            .single(&world)
            .expect("F4 必须只有一个面板根");
        let group_count = world
            .query_filtered::<(), With<MapEditorMonitorGroupButton>>()
            .iter(&world)
            .count();
        let subtab_count = world
            .query_filtered::<(), With<MapEditorMonitorTabButton>>()
            .iter(&world)
            .count();
        let subtab_display = world
            .query_filtered::<&Node, With<MapEditorMonitorSubTabRow>>()
            .single(&world)
            .expect("F4 必须只有一个二级页签行")
            .display;
        let lod_display = world
            .query_filtered::<&Node, With<EditorVegetationLodControls>>()
            .single(&world)
            .expect("F4 六个植被页必须共用一组 F3 同款 LOD 控件")
            .display;
        let mut page_query = world.query::<(&MapEditorMonitorPageText, &Node)>();
        let pages = page_query
            .iter(&world)
            .map(|(page, node)| (page.page, node.display))
            .collect::<Vec<_>>();
        let selected_background = BackgroundColor(Color::srgba(0.38, 0.24, 0.10, 0.94));
        let mut group_query = world.query::<(&MapEditorMonitorGroupButton, &BackgroundColor)>();
        let selected_groups = group_query
            .iter(&world)
            .filter_map(|(button, background)| {
                (*background == selected_background).then_some(button.group)
            })
            .collect::<Vec<_>>();
        let mut tab_query = world.query::<(&MapEditorMonitorTabButton, &Node, &BackgroundColor)>();
        let visible_tabs = tab_query
            .iter(&world)
            .filter_map(|(button, node, _)| (node.display != Display::None).then_some(button.page))
            .collect::<Vec<_>>();
        let selected_tabs = tab_query
            .iter(&world)
            .filter_map(|(button, _, background)| {
                (*background == selected_background).then_some(button.page)
            })
            .collect::<Vec<_>>();

        assert_eq!(panel_visibility, Visibility::Hidden);
        assert_eq!(group_count, EditorMapMonitorGroup::ALL.len());
        assert_eq!(subtab_count, EditorMapMonitorPage::SUBTABS.len());
        assert_eq!(pages.len(), EditorMapMonitorPage::ALL.len());
        assert_eq!(subtab_display, Display::None);
        assert_eq!(lod_display, Display::None);
        assert_eq!(selected_groups, vec![EditorMapMonitorGroup::Overview]);
        assert!(visible_tabs.is_empty());
        assert!(selected_tabs.is_empty());
        assert_eq!(
            pages
                .iter()
                .filter(|(_, display)| *display != Display::None)
                .count(),
            1
        );
        assert!(pages.iter().any(|(page, display)| {
            *page == EditorMapMonitorPage::Overview && *display == Display::Flex
        }));
    }
}
