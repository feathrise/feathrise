//! 插件声明的原生工作台：复用中央标签、分组帮助、连续虚拟网格及原控件。
use super::{
    components::*,
    resources::NativeTools,
    ui::{button, caption, render_field, row, text_input},
};
use bevy::prelude::*;
use feathrise_editor_core::{
    MapEditorPresetWorkspaceScroll, PresetWorkspaceDynamicRoot, PresetWorkspaceTab,
    WorkspaceDomainState,
};
use feathrise_editor_map::game_authoring::map_api::ui::vegetation_preset_editor::{
    PresetWorkspaceEditorHost, PresetWorkspaceTabHost, VegetationPresetEditorState,
};
use feathrise_editor_ui::{colors as c, context_help, workspace_tabs};
use feathrise_plugin_api::{CatalogView, FieldGroup, FieldKind, Request, Snapshot};

pub(super) fn append(
    mut commands: Commands,
    assets: Res<AssetServer>,
    state: Res<NativeTools>,
    workspace: Res<VegetationPresetEditorState>,
    domains: Res<WorkspaceDomainState>,
    tabs: Query<Entity, With<PresetWorkspaceTabHost>>,
    bodies: Query<Entity, With<PresetWorkspaceEditorHost>>,
    roots: Query<Entity, With<ToolWorkspaceRoot>>,
    focus: Res<bevy::input_focus::InputFocus>,
    fields: Query<&ToolText>,
    gestures: Option<Res<super::timeline::Gestures>>,
    mut cache: Local<(u64, u64, Option<PresetWorkspaceTab>)>,
) {
    let (Ok(tabs), Ok(body)) = (tabs.single(), bodies.single()) else {
        return;
    };
    let wanted = (
        state.ui_revision,
        domains.presented_revision,
        Some(workspace.active_tab),
    );
    if *cache == wanted || state.input_pending || focus.get().is_some_and(|e| fields.contains(e))
        || gestures.as_ref().is_some_and(|gestures| gestures.active()) {
        return;
    }
    *cache = wanted;
    for root in &roots {
        commands.entity(root).try_despawn();
    }
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    commands.entity(tabs).with_children(|parent| {
        parent
            .spawn((
                ToolWorkspaceRoot,
                PresetWorkspaceDynamicRoot,
                workspace_tabs::tab_list_node(),
            ))
            .with_children(|tabs| {
                for (&index, tool) in &state.tools {
                    let Some(view) = &tool.snapshot.workbench else {
                        continue;
                    };
                    if tool.library_open {
                        workspace_tabs::spawn_preset_tab(
                            tabs,
                            &font,
                            &view.library_title,
                            ToolButton {
                                index,
                                action: UiAction::Tab(0),
                            },
                            ToolButton {
                                index,
                                action: UiAction::CloseLibrary,
                            },
                            workspace.active_tab == PresetWorkspaceTab::Domain(tool.domain, 0),
                            false,
                        );
                    }
                    for tab in &tool.snapshot.tabs {
                        workspace_tabs::spawn_preset_tab(
                            tabs,
                            &font,
                            &format!("{}{}", tab.title, if tab.dirty { "*" } else { "" }),
                            ToolButton {
                                index,
                                action: UiAction::Tab(tab.id),
                            },
                            ToolButton {
                                index,
                                action: UiAction::Close(tab.id),
                            },
                            workspace.active_tab
                                == PresetWorkspaceTab::Domain(tool.domain, tab.id.into()),
                            tool.close_confirmation == Some(tab.id),
                        );
                    }
                }
            });
    });
    let PresetWorkspaceTab::Domain(domain, id) = workspace.active_tab else {
        return;
    };
    let Some((&index, tool)) = state
        .tools
        .iter()
        .find(|(_, t)| t.domain == domain && t.snapshot.workbench.is_some())
    else {
        return;
    };
    commands.entity(body).with_children(|parent| {
        parent
            .spawn((
                ToolWorkspaceRoot,
                PresetWorkspaceDynamicRoot,
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.,
                    row_gap: px(5),
                    ..default()
                },
            ))
            .with_children(|root| {
                if id != tool.snapshot.active as u64 {
                    root.spawn(caption(&font, "正在打开文档…", 11.));
                    return;
                }
                render(
                    root,
                    &font,
                    index,
                    &tool.snapshot,
                    tool.fault.as_deref(),
                    &assets,
                );
            });
    });
}
fn action(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    index: usize,
    s: &Snapshot,
    id: &str,
) {
    if let Some(a) = s.actions.iter().find(|a| a.id == id && a.enabled) {
        button(
            parent,
            font,
            &a.label,
            index,
            UiAction::Request(Request::Action {
                document: s.active,
                action: a.id.clone(),
                value: String::new(),
            }),
            id == "menu" && s.workbench.as_ref().is_some_and(|v| v.menu_open),
        );
    }
}
fn render(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    index: usize,
    s: &Snapshot,
    fault: Option<&str>,
    assets: &AssetServer,
) {
    let view = s.workbench.as_ref().unwrap();
    if s.active == 0 {
        root.spawn(caption(font, &view.library_title, 13.));
    } else {
        // 身份与操作分行；窄工作区不能用按钮总宽把中文名称挤成单字竖排并撑高整排控件。
        row(root, |header| {
            header.spawn((
                caption(font, &view.title, 13.),
                TextLayout::no_wrap(),
                Node {
                    flex_grow: 1.,
                    flex_shrink: 1.,
                    flex_basis: px(200),
                    min_width: px(144),
                    max_width: percent(100),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            header.spawn((
                caption(font, &view.badge, 11.),
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 0.,
                    ..default()
                },
            ));
        });
        row(root, |header| {
            for id in &view.primary {
                action(header, font, index, s, id);
            }
            action(header, font, index, s, "menu");
        });
        if view.menu_open {
            row(root, |menu| {
                for id in &view.menu {
                    action(menu, font, index, s, id);
                }
            });
        }
    }
    // 沿用原全局状态反馈，校验长消息不再挤占编辑正文。
    root.spawn((
        caption(font, fault.unwrap_or(&s.status), 10.5),
        feathrise_editor_ui::PanelStatusText("插件工作台"),
        feathrise_editor_ui::feedback::GlobalPanelStatus,
    ));
    if s.active > 0 {
        root.spawn((
            Node {
                width: percent(100),
                min_width: px(0),
                flex_shrink: 0.,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(6),
                row_gap: px(6),
                padding: UiRect::bottom(px(8)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(c::COPPER_BORDER_WEAK),
        ))
        .with_children(|nav| {
            for section in &s.sections {
                button(
                    nav,
                    font,
                    &section.label,
                    index,
                    UiAction::Request(Request::Action {
                        document: s.active,
                        action: "section".into(),
                        value: section.id.clone(),
                    }),
                    s.section == section.id,
                );
            }
        });
    }
    // 预设库先展示分类等筛选控件，再搜索/新建和结果；文档内的内容候选仍保留原顺序。
    if s.active == 0 { groups(root, font, index, s, assets); }
    if let Some(list) = &view.catalog {
        if s.active > 0 {
            root.spawn((
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(7),
                    padding: UiRect::top(px(8)),
                    border: UiRect::top(px(1)),
                    flex_shrink: 0.,
                    ..default()
                },
                BorderColor::all(c::COPPER_BORDER_WEAK),
            ))
            .with_children(|body| {
                body.spawn((
                    caption(font, &list.title, 13.),
                    Node {
                        padding: UiRect::right(px(22)),
                        ..default()
                    },
                ));
                if !list.help.is_empty() {
                    context_help::spawn_help_button(body, font, &list.title, &list.help);
                }
                action(body, font, index, s, "close_templates");
                catalog_controls(body, font, index, s, list);
            });
        } else {
            catalog_controls(root, font, index, s, list);
        }
    }
    super::timeline::render(root, font, index, s);
    if s.active > 0 { groups(root, font, index, s, assets); }
}
fn groups(root: &mut ChildSpawnerCommands, font: &Handle<Font>, index: usize, s: &Snapshot, assets: &AssetServer) {
    for group in &s.workbench.as_ref().unwrap().groups {
        // 空标题用于库级内联筛选，字段自身仍保留分类标签，不再重复一级目录标题。
        if group.title.is_empty() {
            for id in &group.fields {
                if let Some(field) = s.fields.iter().find(|field| &field.id == id) {
                    render_field(root, font, index, s.active, field, None);
                }
            }
            continue;
        }
        root.spawn((
            Node {
                width: percent(100),
                min_width: px(0),
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                padding: UiRect::top(px(8)),
                border: UiRect::top(px(1)),
                flex_shrink: 0.,
                ..default()
            },
            BorderColor::all(c::COPPER_BORDER_WEAK),
        ))
        .with_children(|body| {
            body.spawn((
                caption(font, &group.title, 13.),
                Node {
                    max_width: percent(100),
                    padding: UiRect::right(px(22)),
                    ..default()
                },
            ));
            if !group.help.is_empty() {
                context_help::spawn_help_button(body, font, &group.title, &group.help);
            }
            for id in &group.fields {
                if let Some(field) = s.fields.iter().find(|f| &f.id == id) {
                    render_field(body, font, index, s.active, field, None);
                }
            }
            if let Some(path) = group_icon(s, group) {
                body.spawn((
                    ImageNode::new(assets.load(path.to_owned())),
                    Node {
                        width: px(72),
                        height: px(72),
                        flex_shrink: 0.,
                        ..default()
                    },
                ));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_plugin_api::{Field, Preview, Timeline, Workbench};

    #[derive(Resource)]
    struct View(Snapshot);

    fn spawn_view(mut commands: Commands, assets: Res<AssetServer>, view: Res<View>) {
        commands.spawn(Node::default()).with_children(|root| {
            render(root, &Handle::default(), 3, &view.0, None, &assets);
        });
    }

    #[test]
    fn declared_asset_group_renders_its_icon_locate_and_full_timeline_controls() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<Font>()
            .insert_resource(View(Snapshot {
                active: 7,
                fields: vec![
                    Field {
                        id: "text/IconPath".into(),
                        label: "图标".into(),
                        value: "ui/custom.png".into(),
                        kind: FieldKind::Asset {
                            extensions: vec!["png".into()],
                        },
                    },
                    Field {
                        id: "icon".into(),
                        label: "普通文字".into(),
                        value: "ui/custom.png".into(),
                        kind: FieldKind::Label,
                    },
                ],
                preview: Some(Preview {
                    revision: 1,
                    model: None,
                    icon: Some("ui/custom.png".into()),
                    details: String::new(),
                    runtime: None,
                }),
                workbench: Some(Workbench {
                    groups: vec![
                        FieldGroup {
                            title: "实际资源组".into(),
                            help: String::new(),
                            fields: vec!["text/IconPath".into()],
                        },
                        FieldGroup {
                            title: "同名普通文字组".into(),
                            help: String::new(),
                            fields: vec!["icon".into()],
                        },
                    ],
                    ..default()
                }),
                timeline: Some(Timeline {
                    duration: 1.,
                    cursor: 0.,
                    playing: false,
                    zoom: 2.,
                    pan: 0.2,
                    span: None,
                    lanes: vec![],
                }),
                ..default()
            }))
            .add_systems(Startup, spawn_view);
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world.query::<&ImageNode>().iter(world).count(),
            1,
            "缩略图只属于实际匹配的Asset分组"
        );
        assert!(world.query::<&ToolButton>().iter(world).any(|button| {
            matches!(&button.action, UiAction::Locate { document: 7, field } if field == "text/IconPath")
        }));
        assert!(world.query::<&ToolButton>().iter(world).any(|button| {
            matches!(&button.action, UiAction::Request(Request::Action { document: 7, action, .. }) if action == "timeline_fit")
        }));
    }
}

/// 缩略图跟随当前分组实际声明的资源字段，不约定任何游戏的图标字段ID。
fn group_icon<'a>(snapshot: &'a Snapshot, group: &FieldGroup) -> Option<&'a str> {
    let path = snapshot.preview.as_ref()?.icon.as_deref()?;
    group
        .fields
        .iter()
        .any(|id| {
            snapshot.fields.iter().any(|field| {
                &field.id == id
                    && matches!(field.kind, FieldKind::Asset { .. })
                    && field.value == path
            })
        })
        .then_some(path)
}
fn catalog_controls(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    index: usize,
    s: &Snapshot,
    list: &CatalogView,
) {
    row(root, |tools| {
        tools.spawn(caption(font, "搜索", 10.5));
        text_input(
            tools,
            font,
            ToolText {
                index,
                document: 0,
                field: String::new(),
                baseline: s.search.clone(),
                multiline: false,
            },
            Node {
                min_width: px(140),
                flex_basis: px(180),
                flex_grow: 1.,
                flex_shrink: 1.,
                ..default()
            },
        );
        if s.active == 0 {
            for a in &s.actions {
                action(tools, font, index, s, &a.id);
            }
        }
    });
    catalog(root, font, index, s, list);
}
fn catalog(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    index: usize,
    s: &Snapshot,
    list: &CatalogView,
) {
    let columns = list.columns as usize;
    parent
        .spawn((
            ToolCatalog {
                index,
                view: list.clone(),
            },
            Node {
                width: percent(100),
                height: px(list.total.div_ceil(columns as u32).max(1) as f32 * 31.),
                position_type: PositionType::Relative,
                flex_shrink: 0.,
                ..default()
            },
        ))
        .with_children(|root| {
            if list.total == 0 {
                root.spawn(caption(
                    font,
                    &format!("没有匹配项；可修改搜索或新建{}预设。", s.tool),
                    11.,
                ));
            }
            for (i, item) in s.rows.iter().enumerate() {
                let position = list.offset as usize + i;
                let column = position % columns;
                root.spawn((
                    Button,
                    ToolButton {
                        index,
                        action: UiAction::Request(Request::Action {
                            document: s.active,
                            action: list.action.clone(),
                            value: item.id.clone(),
                        }),
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        top: px((position / columns) as f32 * 31.),
                        left: percent(column as f32 * 100. / columns as f32),
                        right: percent((columns - column - 1) as f32 * 100. / columns as f32),
                        height: px(29),
                        min_width: px(0),
                        padding: UiRect::horizontal(px(7)),
                        margin: UiRect::right(px(if column + 1 == columns { 0 } else { 6 })),
                        align_items: AlignItems::Center,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(c::PANEL_BG),
                    feathrise_editor_ui::ButtonColors::new(
                        c::PANEL_BG,
                        c::SELECT_BG_HOVER,
                        c::SELECT_BG_PRESSED,
                    ),
                ))
                .with_child((caption(font, &item.label, 10.5), TextLayout::no_wrap()));
            }
        });
}
/// 与旧目录使用同一140px列宽、31px行距及64行实体预算；完整目录只留在后台。
pub(super) fn scroll(
    mut state: ResMut<NativeTools>,
    lists: Query<(&ToolCatalog, &ComputedNode, &UiGlobalTransform)>,
    viewport: Query<(&ComputedNode, &UiGlobalTransform), With<MapEditorPresetWorkspaceScroll>>,
) {
    let Ok((viewport, view)) = viewport.single() else {
        return;
    };
    for (list, computed, transform) in &lists {
        let Some(tool) = state.tools.get_mut(&list.index) else {
            continue;
        };
        if tool.busy() {
            continue;
        }
        let width = computed.size().x * computed.inverse_scale_factor();
        if width <= 0. {
            continue;
        }
        let columns = ((width / 146.) as u8).clamp(1, 6);
        let first = (((view.translation.y - viewport.size().y * 0.5 - transform.translation.y
            + computed.size().y * 0.5)
            * computed.inverse_scale_factor())
        .max(0.)
            / 31.) as u32;
        let offset = first.min(list.view.total.saturating_sub(1) / columns as u32) * columns as u32;
        if columns != list.view.columns || offset != list.view.offset {
            let _ = tool.send(Request::Browse { offset, columns }, false);
        }
    }
}
