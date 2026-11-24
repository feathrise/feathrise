//! 工程资料只在中央标签中显示，复用工作区标签、原生文本输入和编辑器控件。

use super::{
    TAB, TITLE,
    components::*,
    resources::{IconPicker, ProjectSettingsState},
};
use crate::project::metadata::IconEdit;
use bevy::{
    input_focus::tab_navigation::TabIndex,
    prelude::*,
    text::{EditableText, EditableTextFilter, LineBreak, TextCursorStyle, TextReadWriteMode},
    ui::{FocusPolicy, InteractionDisabled},
};
use feathrise_editor_core::{EditorTextInput, WorkspaceDomainState};
use feathrise_editor_scene::game_authoring::scene_api::ui::vegetation_preset_editor::{
    PresetWorkspaceEditorHost, PresetWorkspaceTabHost, VegetationPresetEditorState,
};
use feathrise_editor_ui::{
    InteractionButton, colors as c,
    localization::{EditorLocale, editor_label},
    workspace_tabs,
};

pub(super) fn mount(
    mut commands: Commands,
    hosts: Query<Entity, Added<PresetWorkspaceEditorHost>>,
    server: Res<AssetServer>,
    state: Res<ProjectSettingsState>,
) {
    if hosts.is_empty() {
        return;
    }
    let font = feathrise_editor_core::editor_font(&server, feathrise_editor_core::FONT_PATH);
    for host in &hosts {
        commands.entity(host).with_children(|host| {
            // 表单根不参与预设动态树重建，切换标签时保留正在编辑的文本和图标。
            host.spawn((
                SettingsBody,
                Node {
                    display: Display::None,
                    width: percent(100),
                    max_width: px(650),
                    min_width: px(0),
                    flex_shrink: 0.,
                    align_self: AlignSelf::Center,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(16)),
                    row_gap: px(10),
                    ..default()
                },
            ))
            .with_children(|body| {
                body.spawn(label(&font, "工程名称", c::TEXT_PRIMARY));
                body.spawn(field(&font, Field::Name, &state.name));
                body.spawn(label(&font, "工程图标", c::TEXT_PRIMARY));
                body.spawn(wrapping_row()).with_children(|row| {
                    row.spawn((
                        SettingsIcon,
                        ImageNode::default(),
                        Pickable::IGNORE,
                        Node {
                            display: Display::None,
                            width: px(64),
                            height: px(64),
                            flex_shrink: 0.,
                            ..default()
                        },
                    ));
                    row.spawn(Node {
                        flex_grow: 1.,
                        min_width: px(0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5),
                        ..default()
                    })
                    .with_children(|detail| {
                        detail.spawn((
                            SettingsIconPath,
                            Text::new(""),
                            text_font(&font),
                            TextColor(c::TEXT_MUTED),
                            Pickable::IGNORE,
                        ));
                        detail.spawn(wrapping_row()).with_children(|choices| {
                            button(choices, &font, Action::ChooseIcon, "选择图标…", false);
                            button(choices, &font, Action::ResetIcon, "恢复默认图标", false);
                        });
                    });
                });
                body.spawn(label(&font, "工程简介", c::TEXT_PRIMARY));
                body.spawn(field(&font, Field::Description, &state.description));
                body.spawn(wrapping_row()).with_children(|row| {
                    button(row, &font, Action::Save, "保存设置", true);
                    button(row, &font, Action::Reload, "重新读取工程设置", false);
                });
                body.spawn((
                    SettingsStatus,
                    Text::new(""),
                    text_font(&font),
                    TextColor(c::TEXT_PRIMARY),
                    Pickable::IGNORE,
                ));
            });
        });
    }
}

fn wrapping_row() -> Node {
    Node {
        align_items: AlignItems::Center,
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(8),
        row_gap: px(5),
        flex_shrink: 0.,
        ..default()
    }
}

fn text_font(font: &Handle<Font>) -> TextFont {
    TextFont {
        font: font.clone().into(),
        font_size: 12.0.into(),
        ..default()
    }
}

fn label(font: &Handle<Font>, name: &'static str, color: Color) -> impl Bundle {
    (
        editor_label(name),
        text_font(font),
        TextColor(color),
        Pickable::IGNORE,
    )
}

fn field(font: &Handle<Font>, name: Field, value: &str) -> impl Bundle {
    let multiline = matches!(name, Field::Description);
    let text = if multiline {
        EditableText {
            allow_newlines: true,
            visible_lines: Some(5.0),
            ..EditableText::new(value)
        }
    } else {
        feathrise_editor_ui::single_line_input(value, 256)
    };
    (
        name,
        EditorTextInput,
        text,
        EditableTextFilter::new(move |character| {
            !character.is_control() || multiline && character == '\n'
        }),
        Node {
            width: percent(100),
            min_width: px(0),
            min_height: px(if multiline { 74 } else { 30 }),
            max_height: px(if multiline { 200 } else { 30 }),
            flex_shrink: 0.,
            padding: UiRect::axes(px(7), px(5)),
            border: UiRect::all(px(1)),
            overflow: Overflow::clip(),
            ..default()
        },
        TextLayout {
            linebreak: if multiline {
                LineBreak::WordOrCharacter
            } else {
                LineBreak::NoWrap
            },
            ..default()
        },
        text_font(font),
        TextColor(c::TEXT_PRIMARY),
        BackgroundColor(c::INPUT_BG),
        BorderColor::all(c::COPPER_BORDER_WEAK),
        TextCursorStyle {
            color: c::TEXT_PRIMARY,
            selection_color: c::SELECT_BG_HOVER,
            unfocused_selection_color: c::SELECT_BG,
            selection_radius: 0.,
            selected_text_color: Some(c::TEXT_PRIMARY),
        },
        TextReadWriteMode::ReadOnly,
        FocusPolicy::Block,
        TabIndex(if multiline { 1 } else { 0 }),
    )
}

fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    action: Action,
    name: &'static str,
    primary: bool,
) {
    let style = feathrise_editor_ui::dialog_button_style(primary);
    parent
        .spawn((
            InteractionButton,
            action,
            style.node,
            style.colors,
            BackgroundColor(style.colors.normal),
            BorderColor::all(c::DIALOG_BORDER),
        ))
        .with_child(label(font, name, c::TEXT_PRIMARY));
}

type TabView = (Entity, u64, bool, bool, bool, EditorLocale);

#[allow(clippy::too_many_arguments)] // 各身份固定，不与工程的普通预设和独立工具页竞争实体。
pub(super) fn present(
    mut commands: Commands,
    state: Res<ProjectSettingsState>,
    picker: NonSend<IconPicker>,
    workspace: Res<VegetationPresetEditorState>,
    domains: Res<WorkspaceDomainState>,
    server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    locale: Option<Res<EditorLocale>>,
    hosts: Query<Entity, With<PresetWorkspaceTabHost>>,
    tab_roots: Query<Entity, With<SettingsTab>>,
    mut bodies: Query<&mut Node, (With<SettingsBody>, Without<SettingsIcon>, Without<Action>)>,
    mut icon_nodes: Query<
        (&mut ImageNode, &mut Node),
        (With<SettingsIcon>, Without<SettingsBody>, Without<Action>),
    >,
    mut actions: Query<
        (Entity, &Action, &mut Node, Has<InteractionDisabled>),
        (Without<SettingsBody>, Without<SettingsIcon>),
    >,
    mut fields: Query<&mut TextReadWriteMode, With<Field>>,
    mut texts: Query<
        (&mut Text, Has<SettingsStatus>),
        Or<(With<SettingsStatus>, With<SettingsIconPath>)>,
    >,
    mut cache: Local<Option<TabView>>,
) {
    let active = state.open && workspace.active_tab == TAB;
    let locale = locale.as_deref().copied().unwrap_or_default();
    if let Ok(host) = hosts.single() {
        let shown = state.open.then_some((
            host,
            domains.presented_revision,
            active,
            state.dirty(),
            state.confirm_close,
            locale,
        ));
        if *cache != shown {
            *cache = shown;
            for root in &tab_roots {
                commands.entity(root).try_despawn();
            }
            if state.open {
                let font =
                    feathrise_editor_core::editor_font(&server, feathrise_editor_core::FONT_PATH);
                let title = format!(
                    "{}{}",
                    locale.editor_label(TITLE),
                    if state.dirty() { "*" } else { "" }
                );
                let confirming = state.confirm_close;
                // 跟随原工作区重建后再追加，始终使用现有标签排版、横滚与关闭反馈。
                commands.entity(host).with_children(|parent| {
                    parent
                        .spawn((SettingsTab, workspace_tabs::tab_list_node()))
                        .with_children(|tab| {
                            workspace_tabs::spawn_preset_tab(
                                tab,
                                &font,
                                &title,
                                Action::Activate,
                                Action::Close,
                                active,
                                confirming,
                            );
                        });
                });
            }
        }
    }
    for mut node in &mut bodies {
        let display = if active { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let busy = state.busy(&picker);
    let mode = if active && state.snapshot.is_some() && !busy {
        TextReadWriteMode::Editable
    } else {
        TextReadWriteMode::ReadOnly
    };
    for mut field in &mut fields {
        field.set_if_neq(mode);
    }
    let has_icon = match &state.icon {
        IconEdit::Import(_) => true,
        IconEdit::Remove => false,
        IconEdit::Keep => state
            .snapshot
            .as_ref()
            .is_some_and(|saved| saved.metadata.icon.is_some()),
    };
    for (entity, action, mut node, disabled) in &mut actions {
        let available = match action {
            Action::Activate => state.open,
            Action::Close => state.open && !busy,
            Action::ChooseIcon => active && state.snapshot.is_some() && !busy,
            Action::ResetIcon => active && has_icon && !busy,
            Action::Save => active && state.dirty() && !busy,
            Action::Reload => active && state.allow_reload && !busy,
        };
        if *action == Action::Reload {
            let display = if state.allow_reload {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
        if available && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if !available && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    if !active {
        return;
    }
    for (mut image, mut node) in &mut icon_nodes {
        let size = state
            .preview
            .as_ref()
            .and_then(|handle| images.get(handle).map(|image| (handle, image.size())));
        let display = if size.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some((handle, dimensions)) = size {
            if &image.image != handle {
                image.image = handle.clone();
            }
            let dimensions = dimensions.as_vec2() * (64. / dimensions.max_element().max(1) as f32);
            if node.width != px(dimensions.x) {
                node.width = px(dimensions.x);
            }
            if node.height != px(dimensions.y) {
                node.height = px(dimensions.y);
            }
        }
    }
    let path = match &state.icon {
        IconEdit::Import(path) => Some(path.as_path()),
        IconEdit::Keep => state
            .snapshot
            .as_ref()
            .and_then(|saved| saved.metadata.icon.as_deref()),
        IconEdit::Remove => None,
    };
    for (mut text, status) in &mut texts {
        if status {
            if text.as_str() != state.status {
                state.status.clone_into(&mut text.0);
            }
        } else if let Some(path) = path {
            let label = path.to_string_lossy();
            if text.as_str() != label.as_ref() {
                label.as_ref().clone_into(&mut text.0);
            }
        } else {
            let label = locale.editor_label("默认图标");
            if text.as_str() != label {
                label.clone_into(&mut text.0);
            }
        }
    }
}
