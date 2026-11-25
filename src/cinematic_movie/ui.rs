use super::*;
use bevy::{input_focus::tab_navigation::TabIndex, text::EditableText, ui::FocusPolicy};
use feathrise_editor_scene::workbench::WorkbenchHeader;
use feathrise_editor_ui::{InteractionButton, UiInteraction, colors as c};
use std::collections::BTreeMap;

#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct Status;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Export,
    Replace,
    Close,
}
#[derive(Component)]
pub(super) struct ReplaceLabel;
#[derive(Component, Default)]
pub(super) struct ChoiceRow(Option<MovieChoicePrompt>);
#[derive(Component)]
pub(super) struct ChoiceButton {
    key: String,
    id: String,
}
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Field {
    File,
    Size,
    Fps,
    Hold,
    Scene,
    Instance,
    Choices,
    Sound,
    Encoder,
}

fn label(font: &Handle<Font>, value: &str) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font: font.clone().into(),
            font_size: 13.0.into(),
            ..default()
        },
        TextColor(c::TEXT_PRIMARY),
        Pickable::IGNORE,
    )
}
pub(super) fn open(
    mut commands: Commands,
    mut requests: MessageReader<OpenCinematicExport>,
    mut state: ResMut<MoviePanel>,
    header: Query<Entity, With<WorkbenchHeader>>,
    existing: Query<Entity, With<Panel>>,
    assets: Res<AssetServer>,
) {
    let Some(request) = requests.read().last().cloned() else {
        return;
    };
    if state.running.is_some() {
        return;
    }
    let Ok(header) = header.single() else { return };
    for panel in &existing {
        commands.entity(panel).despawn();
    }
    let stem = request
        .asset_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("宣传片")
        .trim_end_matches(".cinematic.json");
    let output = state
        .project
        .root
        .join(format!("{stem}.mp4"))
        .to_string_lossy()
        .into_owned();
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    state.document = Some(request);
    state.choice = None;
    state.replace = false;
    state.status.clear();
    let backdrop = Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        width: Val::Vw(100.),
        height: Val::Vh(100.),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    let panel = Node {
        width: Val::Vw(92.),
        max_width: px(740),
        max_height: Val::Vh(98.),
        padding: UiRect::all(px(14)),
        row_gap: px(5),
        overflow: Overflow::scroll_y(),
        ..feathrise_editor_ui::dialog_panel_node()
    };
    commands.entity(header).with_children(|header| {
        header
            .spawn((
                Panel,
                Name::new("CinematicExportDialog"),
                GlobalZIndex(4480),
                FocusPolicy::Block,
                UiInteraction::None,
                backdrop,
                BackgroundColor(c::DIALOG_BACKDROP),
            ))
            .with_children(|overlay| {
                overlay
                    .spawn((
                        panel,
                        BackgroundColor(c::DIALOG_BG),
                        BorderColor::all(c::DIALOG_BORDER),
                        FocusPolicy::Block,
                    ))
                    .with_children(|panel| export_form(panel, &font, &output));
            });
    });
}

fn export_form(panel: &mut ChildSpawnerCommands, font: &Handle<Font>, output: &str) {
    panel.spawn(label(font, "导出宣传片 · H.264 MP4"));
    let fields = [
        (Field::File, "输出文件", output),
        (Field::Size, "画面尺寸", "1920x1080"),
        (Field::Fps, "每秒帧数", "30"),
        (Field::Hold, "每句对白停留（秒）", "2"),
        (Field::Scene, "场景 ID（空为工程开局）", ""),
        (Field::Instance, "子场景实例（可选）", ""),
        (Field::Choices, "Yarn 分支", "{}"),
        (Field::Sound, "配乐文件（可选）", ""),
        (Field::Encoder, "FFmpeg 路径（可选）", ""),
    ];
    for (index, (field, title, value)) in fields.into_iter().enumerate() {
        panel
            .spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(8),
                min_height: px(29),
                flex_shrink: 0.,
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    label(font, title),
                    Node {
                        width: px(168),
                        flex_shrink: 0.,
                        ..default()
                    },
                ));
                let input = Node {
                    flex_grow: 1.,
                    flex_basis: px(0),
                    height: px(29),
                    min_width: px(0),
                    padding: UiRect::axes(px(6), px(4)),
                    overflow: Overflow::clip_x(),
                    border: UiRect::all(px(1)),
                    ..default()
                };
                row.spawn((
                    field,
                    feathrise_editor_core::EditorTextInput,
                    EditableText {
                        allow_newlines: false,
                        ..EditableText::new(value)
                    },
                    TabIndex(index as i32),
                    input,
                    TextFont {
                        font: font.clone().into(),
                        font_size: 13.0.into(),
                        ..default()
                    },
                    TextColor(c::TEXT_PRIMARY),
                    TextLayout::no_wrap(),
                    BackgroundColor(c::INPUT_BG),
                    BorderColor::all(c::COPPER_BORDER_WEAK),
                ));
            });
    }
    panel.spawn(label(
        font,
        "遇到未指定的 Yarn 分支时，可直接点选要拍摄的对白。",
    ));
    panel.spawn((Status, label(font, "")));
    panel.spawn((
        ChoiceRow::default(),
        Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(6),
            row_gap: px(4),
            ..default()
        },
    ));
    panel
        .spawn(Node {
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            for (action, title) in [
                (Action::Replace, "允许覆盖：关"),
                (Action::Close, "关闭 / 取消"),
                (Action::Export, "导出 MP4"),
            ] {
                let style = feathrise_editor_ui::dialog_button_style(action == Action::Export);
                row.spawn((
                    action,
                    InteractionButton,
                    style.node,
                    style.colors,
                    BackgroundColor(style.colors.normal),
                    BorderColor::all(c::DIALOG_BORDER),
                ))
                .with_children(|button| {
                    let mut text = button.spawn(label(font, title));
                    if action == Action::Replace {
                        text.insert(ReplaceLabel);
                    }
                });
            }
        });
}

pub(super) fn actions(
    mut commands: Commands,
    mut state: ResMut<MoviePanel>,
    buttons: Query<(&Action, &UiInteraction), Changed<UiInteraction>>,
    fields: Query<(&Field, &EditableText)>,
    panels: Query<Entity, With<Panel>>,
) {
    for (&action, &interaction) in &buttons {
        if interaction != UiInteraction::Pressed {
            continue;
        }
        if let Some(job) = &state.running {
            if action == Action::Close {
                job.control.cancel();
                state.status = "正在取消摄影和编码".into();
            }
            continue;
        }
        match action {
            Action::Close => {
                for e in &panels {
                    commands.entity(e).despawn();
                }
                state.document = None;
            }
            Action::Replace => state.replace = !state.replace,
            Action::Export => {
                let values = fields
                    .iter()
                    .map(|(&k, v)| (k, v.value().to_string()))
                    .collect();
                match options(&state, &values) {
                    Ok(v) => launch(&mut state, v),
                    Err(error) => state.status = error,
                }
            }
        }
    }
}
fn options(state: &MoviePanel, fields: &BTreeMap<Field, String>) -> Result<Options, String> {
    let asset = state.document.as_ref().ok_or("没有打开演出")?;
    let value = |f| fields.get(&f).map_or("", |s| s.trim());
    let opt = |f| {
        let s = value(f);
        (!s.is_empty()).then(|| s.to_owned())
    };
    let (w, h) = value(Field::Size)
        .split_once(['x', 'X', '×'])
        .ok_or("画面尺寸示例为 1920x1080")?;
    let settings = MovieSettings {
        width: w.trim().parse().map_err(|_| "宽度无效")?,
        height: h.trim().parse().map_err(|_| "高度无效")?,
        fps: value(Field::Fps).parse().map_err(|_| "帧率无效")?,
        seconds_per_line: value(Field::Hold).parse().map_err(|_| "对白时长无效")?,
        instance: opt(Field::Instance),
        choices: serde_json::from_str(opt(Field::Choices).as_deref().unwrap_or("{}"))
            .map_err(|e| format!("Yarn 分支 JSON：{e}"))?,
    };
    settings.validate()?;
    if value(Field::File).is_empty() {
        return Err("请填写输出 MP4 文件路径".into());
    }
    Ok(Options {
        asset: asset.asset_path.clone(),
        root: asset.asset_root.clone(),
        output: value(Field::File).into(),
        scene: opt(Field::Scene),
        settings,
        ffmpeg: opt(Field::Encoder).map(Into::into),
        sound: opt(Field::Sound).map(|p| state.project.root.join(p)),
        replace: state.replace,
    })
}

/// 直接展示当前 Yarn 程序开放的按钮。选择稳定行 ID 后继续同一导出配置，先前选择仍保留。
pub(super) fn choice_buttons(
    mut commands: Commands,
    state: Res<MoviePanel>,
    mut rows: Query<(Entity, &mut ChoiceRow)>,
    assets: Res<AssetServer>,
) {
    if !state.is_changed() {
        return;
    }
    for (row, mut rendered) in &mut rows {
        if rendered.0 == state.choice {
            continue;
        }
        rendered.0 = state.choice.clone();
        commands.entity(row).despawn_children();
        if let Some(prompt) = &state.choice {
            let font =
                feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
            commands.entity(row).with_children(|row| {
                for option in &prompt.options {
                    let style = feathrise_editor_ui::dialog_button_style(true);
                    row.spawn((
                        ChoiceButton {
                            key: prompt.key.clone(),
                            id: option.id.clone(),
                        },
                        InteractionButton,
                        style.node,
                        style.colors,
                        BackgroundColor(style.colors.normal),
                        BorderColor::all(c::DIALOG_BORDER),
                    ))
                    .with_child(label(&font, &option.text));
                }
            });
        }
    }
}
pub(super) fn select_choice(
    mut state: ResMut<MoviePanel>,
    buttons: Query<(&ChoiceButton, &UiInteraction), Changed<UiInteraction>>,
    mut fields: Query<(&Field, &mut EditableText)>,
) {
    if state.running.is_some() {
        return;
    }
    let Some((selection, _)) = buttons.iter().find(|(choice, action)| {
        **action == UiInteraction::Pressed
            && state
                .choice
                .as_ref()
                .is_some_and(|prompt| prompt.key == choice.key)
    }) else {
        return;
    };
    let values: BTreeMap<_, _> = fields
        .iter()
        .map(|(&k, v)| (k, v.value().to_string()))
        .collect();
    let result = options(&state, &values).and_then(|mut opts| {
        opts.settings
            .choices
            .insert(selection.key.clone(), selection.id.clone());
        let json = serde_json::to_string(&opts.settings.choices).map_err(|e| e.to_string())?;
        if let Some((_, mut entry)) = fields
            .iter_mut()
            .find(|(field, _)| **field == Field::Choices)
        {
            entry.clear();
            entry.editor.set_text(&json);
        }
        Ok(opts)
    });
    match result {
        Ok(opts) => launch(&mut state, opts),
        Err(error) => state.status = error,
    }
}
pub(super) fn refresh(
    state: Res<MoviePanel>,
    mut query: Query<&mut Text, With<Status>>,
    mut replace: Query<&mut Text, (With<ReplaceLabel>, Without<Status>)>,
    mut overlay: Option<ResMut<feathrise_editor_core::EditorHostOverlay>>,
    mut guard: Option<ResMut<feathrise_editor_core::EditorTextInputGuard>>,
) {
    if state.document.is_none() {
        return;
    }
    if let Some(overlay) = &mut overlay {
        overlay.active = true
    }
    if let Some(guard) = &mut guard {
        guard.block_shortcuts = true
    }
    if state.is_changed() {
        for mut t in &mut query {
            t.0 = state.status.clone()
        }
        for mut t in &mut replace {
            t.0 = format!("允许覆盖：{}", if state.replace { "开" } else { "关" });
        }
    }
}
