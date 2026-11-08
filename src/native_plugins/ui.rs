//! 原生插件工具行与公共右侧预览；声明文档工作台的插件复用中央作者区。
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::NativeTools};
use bevy::prelude::*;
use feathrise_editor_map::game_authoring::{
    DomainModuleHost, DomainToolLabels, DomainToolPreviewHost, DomainToolsHost,
    map_api::ui::sidebars::*,
};
use feathrise_editor_ui::{colors as c, *};
use feathrise_plugin_api::{Field, FieldKind, Request};

pub(super) fn mount(
    mut commands: Commands,
    modules: Query<Entity, With<DomainModuleHost>>,
    hosts: Query<Entity, With<DomainToolsHost>>,
    roots: Query<Entity, With<ToolHeader>>,
    panels: Query<&ToolPanel>,
    state: Res<NativeTools>,
    assets: Res<AssetServer>,
    mut installed: Local<usize>,
    mut labels: ResMut<DomainToolLabels>,
) {
    let (Ok(module), Ok(host)) = (modules.single(), hosts.single()) else {
        return;
    };
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    if *installed != state.tools.len() {
        *installed = state.tools.len();
        for root in &roots {
            commands.entity(root).despawn();
        }
        commands.entity(module).with_children(|parent| {
            parent
                .spawn((
                    ToolHeader,
                    Node {
                        column_gap: px(TOOL_MODULE_TAB_GAP),
                        flex_shrink: 0.,
                        ..default()
                    },
                ))
                .with_children(|row| {
                    for (&index, tool) in &state.tools {
                        button(
                            row,
                            &font,
                            &tool.snapshot.tool,
                            index,
                            UiAction::OpenTool,
                            false,
                        );
                    }
                });
        });
    }
    for (&index, tool) in &state.tools {
        if panels.iter().any(|panel| panel.index == index) {
            continue;
        }
        labels.0.insert(tool.domain, tool.snapshot.tool.clone());
        commands.entity(host).with_children(|parent| {
            // 常驻页面保留滚动、输入和预览宿主；切工具只改变公共页签身份。
            parent.spawn((ToolPanel { index, revision:0 },
                MapEditorSidebarPage(EditorSidebarTab::Left(EditorLeftSidebarTab::Domain(tool.domain))),
                Node { display:Display::None, width:percent(100), min_width:px(0), min_height:px(0), flex_grow:1.,
                    flex_basis:px(0), flex_direction:FlexDirection::Column, overflow:Overflow::scroll_y(), row_gap:px(5), ..default() },
                ScrollPosition::default(), bevy::ui::RelativeCursorPosition::default(),
                feathrise_editor_map::game_authoring::map_api::components::MapEditorRightPanelScroll,
            )).with_children(|panel| {
                panel.spawn((ToolBody(index), Node { width:percent(100), min_width:px(0), flex_shrink:0., flex_direction:FlexDirection::Column, row_gap:px(5), ..default() }));
                // 唯一预览画布独立于可重建表单，回复刷新不能回收画布。
                panel.spawn((ToolPreviewHost(index), DomainToolPreviewHost(tool.domain), Node {
                    display:Display::None, width:percent(100), min_width:px(0), height:px(if tool.snapshot.workbench.is_some() {370}else{240}), flex_shrink:0., flex_direction:FlexDirection::Column, ..default()
                }));
            });
        });
    }
}

pub(super) fn append(
    mut commands: Commands,
    assets: Res<AssetServer>,
    state: Res<NativeTools>,
    sidebar: Res<MapEditorSidebarState>,
    focus: Res<bevy::input_focus::InputFocus>,
    fields: Query<&ToolText>,
    mut panels: Query<&mut ToolPanel>,
    bodies: Query<(Entity, &ToolBody, Option<&Children>)>,
) {
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for mut panel in &mut panels {
        let index = panel.index;
        let Some(tool) = state.tools.get(&index) else {
            continue;
        };
        if panel.revision == state.ui_revision
            || !sidebar.tool_page_visible(EditorLeftSidebarTab::Domain(tool.domain))
        {
            continue;
        }
        // 输入未结束时保留输入根，后台回复不能吞掉当前字段。
        if state.input_pending
            || focus
                .get()
                .is_some_and(|e| fields.get(e).is_ok_and(|f| f.index == index))
        {
            continue;
        }
        let Some((body, _, children)) = bodies.iter().find(|(_, b, _)| b.0 == index) else {
            continue;
        };
        panel.revision = state.ui_revision;
        if let Some(children) = children {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }
        let snapshot = &tool.snapshot;
        if snapshot.workbench.is_some() {
            commands.entity(body).with_children(|body| {
                body.spawn(caption(&font, &format!("{}预览", snapshot.tool), 13.));
            });
            continue;
        }
        commands.entity(body).with_children(|body| {
            body.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(3),
                row_gap: px(3),
                flex_shrink: 0.,
                ..default()
            })
            .with_children(|tabs| {
                button(
                    tabs,
                    &font,
                    "目录",
                    index,
                    UiAction::Request(Request::Activate { document: 0 }),
                    snapshot.active == 0,
                );
                for tab in &snapshot.tabs {
                    workspace_tabs::spawn_preset_tab(
                        tabs,
                        &font,
                        &format!("{}{}", tab.title, if tab.dirty { " *" } else { "" }),
                        ToolButton {
                            index,
                            action: UiAction::Request(Request::Activate { document: tab.id }),
                        },
                        ToolButton {
                            index,
                            action: UiAction::Close(tab.id),
                        },
                        snapshot.active == tab.id,
                        tool.close_confirmation == Some(tab.id),
                    );
                }
            });
            body.spawn((
                caption(
                    &font,
                    tool.fault.as_deref().unwrap_or(&snapshot.status),
                    12.,
                ),
                Node {
                    flex_shrink: 0.,
                    ..default()
                },
            ));
            if tool.busy() {
                body.spawn(caption(&font, "正在处理…", 11.));
            }
            body.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(5),
                row_gap: px(5),
                flex_shrink: 0.,
                ..default()
            })
            .with_children(|row| {
                for action in snapshot.actions.iter().filter(|a| a.enabled) {
                    button(
                        row,
                        &font,
                        &action.label,
                        index,
                        UiAction::Request(Request::Action {
                            document: snapshot.active,
                            action: action.id.clone(),
                            value: String::new(),
                        }),
                        false,
                    );
                }
            });
            if snapshot.active == 0 {
                input(
                    body,
                    &font,
                    ToolText {
                        index,
                        document: 0,
                        field: String::new(),
                        baseline: snapshot.search.clone(),
                        multiline: false,
                    },
                    "搜索名称或ID",
                );
                for item in &snapshot.rows {
                    button(
                        body,
                        &font,
                        &item.label,
                        index,
                        UiAction::Request(Request::Action {
                            document: 0,
                            action: "edit".into(),
                            value: item.id.clone(),
                        }),
                        false,
                    );
                }
                body.spawn(Node {
                    column_gap: px(5),
                    ..default()
                })
                .with_children(|row| {
                    if snapshot.page > 0 {
                        button(
                            row,
                            &font,
                            "上一页",
                            index,
                            UiAction::Request(Request::Search {
                                value: snapshot.search.clone(),
                                page: snapshot.page - 1,
                            }),
                            false,
                        );
                    }
                    row.spawn(caption(
                        &font,
                        &format!("{} / {}", snapshot.page + 1, snapshot.pages.max(1)),
                        12.,
                    ));
                    if snapshot.page + 1 < snapshot.pages {
                        button(
                            row,
                            &font,
                            "下一页",
                            index,
                            UiAction::Request(Request::Search {
                                value: snapshot.search.clone(),
                                page: snapshot.page + 1,
                            }),
                            false,
                        );
                    }
                });
            } else {
                body.spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(5),
                    flex_shrink: 0.,
                    ..default()
                })
                .with_children(|row| {
                    for section in &snapshot.sections {
                        button(
                            row,
                            &font,
                            &section.label,
                            index,
                            UiAction::Request(Request::Action {
                                document: snapshot.active,
                                action: "section".into(),
                                value: section.id.clone(),
                            }),
                            snapshot.section == section.id,
                        );
                    }
                });
                for field in &snapshot.fields {
                    render_field(
                        body,
                        &font,
                        index,
                        snapshot.active,
                        field,
                        tool.choices.as_deref(),
                    );
                }
                if let Some(preview) = &snapshot.preview {
                    if let Some(path) = &preview.icon {
                        body.spawn((
                            ImageNode::new(assets.load(path.clone())),
                            Node {
                                width: px(64),
                                height: px(64),
                                flex_shrink: 0.,
                                ..default()
                            },
                        ));
                    }
                }
            }
        });
    }
}

pub(super) fn render_field(
    body: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    index: usize,
    document: u32,
    field: &Field,
    choices: Option<&str>,
) {
    match &field.kind {
        FieldKind::Text {
            readonly: false, ..
        } => input(
            body,
            font,
            ToolText {
                index,
                document,
                field: field.id.clone(),
                baseline: field.value.clone(),
                multiline: matches!(
                    field.kind,
                    FieldKind::Text {
                        multiline: true,
                        ..
                    }
                ),
            },
            &field.label,
        ),
        FieldKind::Asset { extensions } => {
            body.spawn(caption(font, &field.label, 11.));
            row(body, |row| {
                text_input(
                    row,
                    font,
                    ToolText {
                        index,
                        document,
                        field: field.id.clone(),
                        baseline: field.value.clone(),
                        multiline: false,
                    },
                    Node {
                        flex_basis: px(260),
                        flex_grow: 1.,
                        flex_shrink: 1.,
                        ..default()
                    },
                );
                button(
                    row,
                    font,
                    if extensions.iter().any(|e| e == "glb") {
                        "使用选中 GLB"
                    } else {
                        "使用选中图片"
                    },
                    index,
                    UiAction::Adopt {
                        document,
                        field: field.id.clone(),
                        extensions: extensions.clone(),
                    },
                    false,
                );
                button(
                    row,
                    font,
                    "定位",
                    index,
                    UiAction::Locate {
                        document,
                        field: field.id.clone(),
                    },
                    false,
                );
            });
        }
        FieldKind::Picker { action } => button(
            body,
            font,
            &field.value,
            index,
            UiAction::Request(Request::Action {
                document,
                action: action.clone(),
                value: String::new(),
            }),
            false,
        ),
        FieldKind::Choice { options } | FieldKind::ChoiceButtons { options } => {
            let inline = matches!(field.kind, FieldKind::ChoiceButtons { .. });
            if !inline {
                let selected = options
                    .iter()
                    .find(|o| o.id == field.value)
                    .map(|o| o.label.as_str())
                    .unwrap_or(&field.value);
                button(
                    body,
                    font,
                    &format!("{}：{}（选择）", field.label, selected),
                    index,
                    UiAction::Choices(field.id.clone()),
                    false,
                );
            }
            if inline || choices == Some(field.id.as_str()) {
                row(body, |row| {
                    for option in options {
                        button(
                            row,
                            font,
                            &option.label,
                            index,
                            UiAction::Request(Request::Edit {
                                document,
                                field: field.id.clone(),
                                value: option.id.clone(),
                            }),
                            option.id == field.value,
                        );
                    }
                });
            }
        }
        FieldKind::Number { min, max, step } => {
            let root = spawn_numeric_scrub_field(
                body,
                ToolNumber {
                    index,
                    document,
                    field: field.id.clone(),
                },
                font.clone(),
                NumericScrubFieldOptions::new(
                    &field.label,
                    "",
                    NumericValue::U32(field.value.parse().unwrap_or(*min as u32)),
                    NumericInputSpec::u32()
                        .with_min(NumericValue::U32(*min as u32))
                        .with_max(NumericValue::U32(*max as u32)),
                    NumericInputScrubConfig::linear(
                        0.6,
                        *step as f32,
                        *min as f32,
                        *max as f32,
                        *step as f32,
                    ),
                )
                .with_style(NumericScrubFieldStyle {
                    width: 260.,
                    height: 28.,
                    label_font_size: metrics::FONT_HINT,
                    value_font_size: metrics::FONT_CONTROL,
                    ..default()
                }),
            );
            body.commands()
                .entity(root)
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.min_width = px(0);
                    n.max_width = percent(100);
                    n.flex_shrink = 0.;
                });
        }
        FieldKind::Decimal { min, max, step, precision } => {
            // 沿用作者数值控件的精确输入和一次拖动提交；小数不能走u32转换。
            let root = spawn_numeric_scrub_field(
                body,
                ToolNumber { index, document, field: field.id.clone() },
                font.clone(),
                NumericScrubFieldOptions::new(
                    &field.label, "",
                    NumericValue::F32(field.value.parse().unwrap_or(*min as f32)),
                    NumericInputSpec::f32(*precision)
                        .with_min(NumericValue::F32(*min as f32))
                        .with_max(NumericValue::F32(*max as f32)),
                    NumericInputScrubConfig::linear(0.6, *step as f32, *min as f32,
                        *max as f32, *step as f32),
                ).with_style(NumericScrubFieldStyle {
                    width: 260., height: 28., label_font_size: metrics::FONT_HINT,
                    value_font_size: metrics::FONT_CONTROL, ..default()
                }),
            );
            body.commands().entity(root).entry::<Node>().and_modify(|mut n| {
                n.min_width = px(0); n.max_width = percent(100); n.flex_shrink = 0.;
            });
        }
        _ => {
            body.spawn((
                caption(
                    font,
                    &if field.label.is_empty() {
                        field.value.clone()
                    } else {
                        format!("{}：{}", field.label, field.value)
                    },
                    11.,
                ),
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_shrink: 0.,
                    ..default()
                },
            ));
        }
    }
}
pub(super) fn row(parent: &mut ChildSpawnerCommands, body: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            width: percent(100),
            min_width: px(0),
            flex_shrink: 0.,
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(5),
            row_gap: px(5),
            ..default()
        })
        .with_children(body);
}
pub(super) fn input(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    field: ToolText,
    label: &str,
) {
    parent
        .spawn(Node {
            width: percent(100),
            min_width: px(0),
            column_gap: px(6),
            align_items: AlignItems::Start,
            flex_shrink: 0.,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                caption(font, label, 10.5),
                Node {
                    width: px(104),
                    max_width: percent(38),
                    height: px(metrics::label_height(label, 104.) + 4.),
                    flex_shrink: 0.,
                    padding: UiRect::top(px(4)),
                    ..default()
                },
            ));
            text_input(
                row,
                font,
                field,
                Node {
                    flex_grow: 1.,
                    flex_basis: px(0),
                    ..default()
                },
            );
        });
}
pub(super) fn text_input(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    field: ToolText,
    layout: Node,
) {
    let multiline = field.multiline;
    let editable = bevy::text::EditableText {
        max_characters: Some(8192),
        allow_newlines: false,
        ..bevy::text::EditableText::new(&field.baseline)
    };
    parent.spawn((
        field,
        feathrise_editor_core::EditorTextInput,
        editable,
        TextFont {
            font: font.clone().into(),
            font_size: metrics::FONT_CONTROL.into(),
            ..default()
        },
        TextColor(c::TEXT_VALUE),
        Node {
            min_width: px(0),
            min_height: px(if multiline { 56 } else { 24 }),
            padding: UiRect::all(px(3)),
            border: UiRect::all(px(1)),
            overflow: Overflow::clip(),
            ..layout
        },
        BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL),
        BorderColor::all(c::COPPER_BORDER_WEAK),
    ));
}
pub(super) fn caption(font: &Handle<Font>, label: &str, size: f32) -> impl Bundle {
    (
        Text::new(label),
        TextFont {
            font: font.clone().into(),
            font_size: metrics::font_size(size).into(),
            ..default()
        },
        TextColor(c::TEXT_PRIMARY),
        Pickable::IGNORE,
    )
}
pub(super) fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    index: usize,
    action: UiAction,
    selected: bool,
) {
    if matches!(action, UiAction::OpenTool) {
        // 工具入口复用原生页签外壳和定宽，保留插件原动作；正文表单继续使用原按钮。
        parent.spawn((
            InteractionButton,
            ToolButton { index, action },
            Node {
                width: px(TOOL_MODULE_TAB_WIDTH), flex_grow: 0., flex_basis: Val::Auto,
                flex_shrink: 0., ..sidebar_tab_node()
            },
            BorderColor::all(c::COPPER_BORDER_WEAK),
            BackgroundColor(sidebar_tab_background(Interaction::None, selected)),
            ButtonColors::new(
                sidebar_tab_background(Interaction::None, selected),
                sidebar_tab_background(Interaction::Hovered, selected),
                sidebar_tab_background(Interaction::Pressed, selected),
            ),
        )).with_child((
            Text::new(label),
            TextFont { font: font.clone().into(), font_size: metrics::FONT_CONTROL.into(), ..default() },
            TextColor(sidebar_tab_text_color(selected)), Pickable::IGNORE,
        ));
        return;
    }
    let navigation =
        matches!(&action,UiAction::Request(Request::Action { action,.. }) if action=="section");
    let primary =
        matches!(&action,UiAction::Request(Request::Action { action,.. }) if action=="save");
    let emphasized = selected || primary;
    parent
        .spawn((
            InteractionButton,
            ToolButton { index, action },
            Node {
                min_height: px(if navigation { 24 } else { 26 }),
                min_width: px(0),
                max_width: percent(100),
                padding: UiRect::axes(px(8), px(4)),
                border: if navigation {
                    UiRect::bottom(px(2))
                } else {
                    UiRect::all(px(if emphasized { 2 } else { 1 }))
                },
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(if navigation { 0 } else { 3 })),
                ..default()
            },
            BorderColor::all(if emphasized {
                c::COPPER_BORDER
            } else {
                c::COPPER_BORDER_WEAK
            }),
            BackgroundColor(if emphasized {
                c::SELECT_BG
            } else {
                c::BUTTON_DEV_SMALL_NORMAL
            }),
            ButtonColors::new(
                if emphasized {
                    c::SELECT_BG
                } else {
                    c::BUTTON_DEV_SMALL_NORMAL
                },
                c::BUTTON_DEV_SMALL_HOVERED,
                c::BUTTON_DEV_SMALL_PRESSED,
            ),
        ))
        .with_child(caption(font, label, 12.));
}

/// 工具入口沿用同排选中反馈，切属性只记忆模块，不抢回正文。
pub(super) fn sync_buttons(
    state: Res<NativeTools>,
    sidebar: Res<MapEditorSidebarState>,
    mut buttons: Query<(
        &ToolButton,
        &Interaction,
        &Children,
        &mut BackgroundColor,
        &mut ButtonColors,
    )>,
    mut texts: Query<&mut TextColor>,
) {
    for (button, interaction, children, mut background, mut colors) in &mut buttons {
        if !matches!(button.action, UiAction::OpenTool) {
            continue;
        }
        let selected = state
            .tools
            .get(&button.index)
            .is_some_and(|t| sidebar.left == EditorLeftSidebarTab::Domain(t.domain));
        let palette = ButtonColors::new(
            sidebar_tab_background(Interaction::None, selected),
            sidebar_tab_background(Interaction::Hovered, selected),
            sidebar_tab_background(Interaction::Pressed, selected),
        );
        if *colors != palette {
            *colors = palette;
        }
        // 公共按钮只在鼠标状态变化时着色；工具切换也要立即回显活动底色。
        let wanted = match interaction {
            Interaction::Pressed => palette.pressed,
            Interaction::Hovered => palette.hovered,
            Interaction::None => palette.normal,
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
        for child in children.iter() {
            if let Ok(mut color) = texts.get_mut(child) {
                let wanted = sidebar_tab_text_color(selected);
                if color.0 != wanted { color.0 = wanted; }
            }
        }
    }
}
