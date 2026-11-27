//! BSN 文档的层级、画布和属性面板，挂在独立工作台的中央文档容器中。

use super::super::components::DocumentCenterHost;
use super::{
    Action, DocumentState, Identity, UiLayouts, active,
    canvas::CanvasItem,
    fields::{self, Field, length_text},
};
use crate::components::SceneEditorRightPanelScroll;
use bevy::{
    prelude::*,
    text::EditableText,
    ui::{Outline, RelativeCursorPosition},
};
use feathrise_editor_ui::{
    colors, dialog_compact_button_style, localization::EditorLocale, spawn_custom_text_button,
};
use feathrise_ui_bsn::{LayoutContext, UiWidget, WidgetKind, spawn_layout};

#[derive(Component)]
pub(super) struct Panel;

pub(super) fn mount(mut commands: Commands, hosts: Query<Entity, Added<DocumentCenterHost>>) {
    for host in &hosts {
        commands.entity(host).with_child((
            Panel,
            Name::new("feathrise.ui_bsn"),
            Node {
                display: Display::None,
                width: percent(100),
                height: percent(100),
                min_width: px(0),
                min_height: px(0),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::all(px(8)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(colors::PANEL_BG_STRONG),
        ));
    }
}

fn kind_label(locale: EditorLocale, kind: WidgetKind) -> &'static str {
    locale.text(match kind {
        WidgetKind::Container => "ui_bsn.container",
        WidgetKind::Text => "ui_bsn.text",
        WidgetKind::Image => "ui_bsn.image",
        WidgetKind::Button => "ui_bsn.button",
        WidgetKind::Binding => "ui_bsn.binding",
    })
}
fn label(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, value: impl Into<String>) {
    parent.spawn((
        Text::new(value),
        TextFont {
            font: font.clone().into(),
            font_size: 12.0.into(),
            ..default()
        },
        TextColor(colors::TEXT_PRIMARY),
        Pickable::IGNORE,
    ));
}

fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    text: &str,
    action: Action,
    primary: bool,
) {
    spawn_custom_text_button(
        parent,
        action,
        font.clone(),
        text,
        dialog_compact_button_style(primary),
    );
}

fn line() -> Node {
    Node {
        width: percent(100),
        align_items: AlignItems::Center,
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(5),
        row_gap: px(4),
        flex_shrink: 0.0,
        ..default()
    }
}

fn pane(width: Val, grow: f32) -> impl Bundle {
    (
        Node {
            width,
            flex_grow: grow,
            flex_shrink: 0.0,
            min_width: px(0),
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::all(px(7)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        BackgroundColor(Color::srgb(0.075, 0.085, 0.102)),
        RelativeCursorPosition::default(),
        ScrollPosition::default(),
        SceneEditorRightPanelScroll,
    )
}

pub(super) fn present(
    mut commands: Commands,
    server: Res<AssetServer>,
    docs: Res<DocumentState>,
    state: Res<UiLayouts>,
    locale: Option<Res<EditorLocale>>,
    mut views: Query<(Entity, Ref<Panel>, &mut Node)>,
    inputs: Query<(&Field, &EditableText)>,
    mut shown_form: Local<Option<(Identity, String)>>,
    mut shown_status: Local<String>,
) {
    let identity = active(&docs);
    let doc = docs.current().filter(|_| identity.is_some());
    let receipt = doc.map_or("", |doc| doc.status.as_str());
    let conflict = doc.is_some_and(|doc| doc.external.warning);
    let confirm_reload = doc.is_some_and(|doc| doc.confirm_reload);
    let changed = shown_status.as_str() != receipt;
    if changed {
        *shown_status = receipt.to_owned();
    }
    let language_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    let locale = locale.as_deref().copied().unwrap_or_default();
    let enabled = doc.is_some();
    for (host, marker, mut node) in &mut views {
        let display = if enabled {
            Display::Flex
        } else {
            Display::None
        };
        let toggled = node.display != display;
        if toggled {
            node.display = display;
        }
        if !enabled {
            if toggled {
                commands.entity(host).despawn_children();
                *shown_form = None;
            }
            continue;
        }
        if !toggled && !marker.is_added() && !state.is_changed() && !language_changed && !changed {
            continue;
        }
        // 回执和属性错误会刷新面板，同一修订、同一控件的未应用输入继续用于纠正。
        let form = identity
            .as_ref()
            .filter(|_| !state.interactive && state.key == identity && state.layout.is_some())
            .map(|key| (key.clone(), state.selected.clone()));
        let draft = if form.is_some() && form == *shown_form {
            inputs
                .iter()
                .map(|(field, value)| (*field, value.value().to_string()))
                .collect()
        } else {
            fields::Values::new()
        };
        *shown_form = form;
        let font = feathrise_editor_core::editor_font(&server, feathrise_editor_core::FONT_PATH);
        commands
            .entity(host)
            .despawn_children()
            .with_children(|body| {
                body.spawn(line()).with_children(|bar| {
                    button(bar, &font, locale.text("ui_bsn.save"), Action::Save, true);
                    button(bar, &font, locale.text("ui_bsn.undo"), Action::Undo, false);
                    button(bar, &font, locale.text("ui_bsn.redo"), Action::Redo, false);
                    button(
                        bar,
                        &font,
                        locale.text("ui_bsn.source"),
                        Action::Source,
                        false,
                    );
                    if conflict {
                        button(
                            bar,
                            &font,
                            locale.text("label.keep-draft"),
                            Action::KeepDraft,
                            false,
                        );
                    }
                    if conflict || confirm_reload {
                        let reload = if confirm_reload {
                            "document.reload.confirm"
                        } else {
                            "label.reload"
                        };
                        button(bar, &font, locale.text(reload), Action::Reload, false);
                    }
                    if !receipt.is_empty() && !receipt.starts_with("原文预览 ·") {
                        label(bar, &font, locale.editor_notice(receipt));
                    }
                });
                if !state.notice.is_empty() {
                    label(body, &font, locale.editor_label(&state.notice));
                }
                let Some(layout) = state.layout.as_deref().filter(|_| state.key == identity) else {
                    return;
                };
                body.spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    flex_grow: 1.0,
                    column_gap: px(6),
                    ..default()
                })
                .with_children(|columns| {
                    columns.spawn(pane(px(148), 0.0)).with_children(|left| {
                        if !state.interactive {
                            label(left, &font, locale.text("ui_bsn.widgets"));
                            left.spawn(line()).with_children(|palette| {
                                for kind in [
                                    WidgetKind::Container,
                                    WidgetKind::Text,
                                    WidgetKind::Image,
                                    WidgetKind::Button,
                                    WidgetKind::Binding,
                                ] {
                                    button(
                                        palette,
                                        &font,
                                        kind_label(locale, kind),
                                        Action::Insert(kind),
                                        false,
                                    );
                                }
                            });
                        }
                        label(left, &font, locale.text("ui_bsn.hierarchy"));
                        tree(left, &font, &layout.root, &state.selected, 0, locale);
                    });
                    columns.spawn(pane(px(0), 1.0)).with_children(|center| {
                        center.spawn(line()).with_children(|bar| {
                            button(
                                bar,
                                &font,
                                locale.text("ui_bsn.edit"),
                                Action::Preview(false),
                                !state.interactive,
                            );
                            button(
                                bar,
                                &font,
                                locale.text("ui_bsn.fit"),
                                Action::Narrow(false),
                                !state.narrow,
                            );
                            button(
                                bar,
                                &font,
                                locale.text("ui_bsn.narrow"),
                                Action::Narrow(true),
                                state.narrow,
                            );
                            button(
                                bar,
                                &font,
                                locale.text("ui_bsn.preview"),
                                Action::Preview(true),
                                state.interactive,
                            );
                        });
                        center
                            .spawn((
                                Node {
                                    width: if state.narrow { px(320) } else { percent(100) },
                                    max_width: percent(100),
                                    min_height: px(260),
                                    flex_shrink: 0.0,
                                    align_self: AlignSelf::Center,
                                    padding: UiRect::all(px(8)),
                                    flex_direction: FlexDirection::Column,
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.05, 0.06, 0.08)),
                            ))
                            .with_children(|board| {
                                let context = LayoutContext {
                                    font: &font,
                                    assets: &server,
                                    text_color: colors::TEXT_PRIMARY,
                                    button_color: Color::srgb(0.20, 0.26, 0.33),
                                    interactive: state.interactive,
                                    preview: true,
                                };
                                spawn_layout(
                                    board,
                                    layout,
                                    &context,
                                    |entity, widget| {
                                        if !state.interactive {
                                            entity.insert(CanvasItem(widget.id.clone()));
                                            if widget.id == state.selected {
                                                entity.insert(Outline {
                                                    width: px(1),
                                                    offset: px(1),
                                                    color: Color::srgb(0.47, 0.78, 1.0),
                                                });
                                            }
                                        }
                                    },
                                    |children, widget| {
                                        // 绑定的真实值由工程提供。独立编辑器用绑定 ID 预览其布局。
                                        label(children, &font, format!("‹{}›", widget.content));
                                    },
                                );
                            });
                    });
                    columns.spawn(pane(px(208), 0.0)).with_children(|right| {
                        let Some(widget) = layout.root.find(&state.selected) else {
                            return;
                        };
                        inspector(
                            right,
                            &font,
                            widget,
                            parent_id(&layout.root, &widget.id),
                            !state.interactive,
                            &draft,
                            locale,
                        );
                    });
                });
            });
    }
}
fn parent_id<'a>(widget: &'a UiWidget, id: &str) -> Option<&'a str> {
    if widget.children.iter().any(|child| child.id == id) {
        return Some(&widget.id);
    }
    widget
        .children
        .iter()
        .find_map(|child| parent_id(child, id))
}

fn tree(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    item: &UiWidget,
    selected: &str,
    depth: usize,
    locale: EditorLocale,
) {
    let name = match item.kind {
        WidgetKind::Text | WidgetKind::Button | WidgetKind::Binding if !item.content.is_empty() => {
            item.content.clone()
        }
        _ => format!("{} {}", kind_label(locale, item.kind), item.id),
    };
    parent
        .spawn(Node {
            padding: UiRect::left(px(7.0 * depth as f32)),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|row| {
            button(
                row,
                font,
                &name,
                Action::Select(item.id.clone()),
                item.id == selected,
            )
        });
    for child in &item.children {
        tree(parent, font, child, selected, depth + 1, locale);
    }
}

fn inspector(
    panel: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    widget: &UiWidget,
    parent: Option<&str>,
    editing: bool,
    draft: &fields::Values,
    locale: EditorLocale,
) {
    let input = |panel: &mut ChildSpawnerCommands, field: Field, title: &str, value: &str| {
        label(panel, font, title);
        field.input(panel, font, draft.get(&field).map_or(value, String::as_str));
    };
    label(
        panel,
        font,
        format!("{} · {}", kind_label(locale, widget.kind), widget.id),
    );
    if !editing {
        return;
    }
    panel.spawn(line()).with_children(|row| {
        if parent.is_some() {
            button(
                row,
                font,
                locale.text("ui_bsn.up"),
                Action::Shift(-1),
                false,
            );
            button(
                row,
                font,
                locale.text("ui_bsn.down"),
                Action::Shift(1),
                false,
            );
            button(
                row,
                font,
                locale.text("ui_bsn.remove"),
                Action::Remove,
                false,
            );
        }
        if widget.kind == WidgetKind::Container {
            button(
                row,
                font,
                if widget.direction == FlexDirection::Row {
                    locale.text("ui_bsn.row")
                } else {
                    locale.text("ui_bsn.column")
                },
                Action::Direction,
                false,
            );
        }
    });
    button(
        panel,
        font,
        locale.text("ui_bsn.apply"),
        Action::Properties,
        true,
    );
    input(panel, Field::Id, locale.text("ui_bsn.id"), &widget.id);
    if widget.kind == WidgetKind::Binding {
        input(
            panel,
            Field::Content,
            locale.text("ui_bsn.binding_id"),
            &widget.content,
        );
    }
    if let Some(parent) = parent {
        input(panel, Field::Parent, locale.text("ui_bsn.parent"), parent);
    }
    input(
        panel,
        Field::Width,
        locale.text("ui_bsn.width"),
        &length_text(widget.width),
    );
    input(
        panel,
        Field::Height,
        locale.text("ui_bsn.height"),
        &length_text(widget.height),
    );
    input(
        panel,
        Field::Padding,
        locale.text("ui_bsn.padding"),
        &widget.padding.to_string(),
    );
    input(
        panel,
        Field::Gap,
        locale.text("ui_bsn.gap"),
        &widget.gap.to_string(),
    );
    if parent.is_some() {
        let (x, y) = widget
            .position
            .map(|pos| (pos.x.to_string(), pos.y.to_string()))
            .unwrap_or_default();
        input(panel, Field::X, locale.text("ui_bsn.x"), &x);
        input(panel, Field::Y, locale.text("ui_bsn.y"), &y);
    }
    let color = widget
        .background
        .map(|[r, g, b, a]| format!("#{r:02X}{g:02X}{b:02X}{a:02X}"))
        .unwrap_or_default();
    input(
        panel,
        Field::Background,
        locale.text("ui_bsn.background"),
        &color,
    );
    if matches!(widget.kind, WidgetKind::Text | WidgetKind::Button) {
        input(
            panel,
            Field::Content,
            locale.text("ui_bsn.content"),
            &widget.content,
        );
        input(
            panel,
            Field::FontSize,
            locale.text("ui_bsn.font_size"),
            &widget.font_size.to_string(),
        );
    }
    if widget.kind == WidgetKind::Image {
        input(
            panel,
            Field::Content,
            locale.text("ui_bsn.image_path"),
            &widget.content,
        );
    }
    if widget.kind == WidgetKind::Button {
        input(
            panel,
            Field::Action,
            locale.text("ui_bsn.action_id"),
            &widget.action,
        );
    }
}
