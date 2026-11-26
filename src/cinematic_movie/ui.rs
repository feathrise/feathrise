//! 宣传片对话框用当前场景填初值，普通导出确认文件、场景和画质即可。
use super::*;
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::{InputFocus, tab_navigation::TabIndex},
    text::EditableText,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_scene::workbench::WorkbenchHeader;
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, UiInteraction, colors as c};
use std::collections::BTreeMap;

#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct Body;
#[derive(Component)]
pub(super) struct Advanced;
#[derive(Component)]
pub(super) struct Status;
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Options,
    Export,
    Replace,
    Close,
}
#[derive(Component)]
pub(super) struct ButtonLabel(Action);
#[derive(Component, Default)]
pub(super) struct ChoiceRow(Option<MovieChoicePrompt>);
#[derive(Component)]
pub(super) struct ChoiceButton {
    key: String,
    id: String,
}
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    let scene = request.scene.clone().unwrap_or_default();
    state.document = Some(request);
    state.choice = None;
    state.advanced = false;
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
        max_width: px(680),
        max_height: Val::Vh(92.),
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
                    .with_children(|panel| export_form(panel, &font, &output, &scene));
            });
    });
}

fn export_form(panel: &mut ChildSpawnerCommands, font: &Handle<Font>, output: &str, scene: &str) {
    panel
        .spawn((
            feathrise_editor_ui::dialog_header_node(),
            BackgroundColor(c::DIALOG_HEADER_BG),
            BorderColor::all(c::DIALOG_BORDER),
        ))
        .with_child(label(font, "导出宣传片 · H.264 MP4"));
    let fields = [
        (Field::File, "输出文件", output),
        (Field::Scene, "场景 ID（可留空）", scene),
        (Field::Size, "画面尺寸", "1920x1080"),
        (Field::Fps, "每秒帧数", "30"),
    ];
    let advanced = [
        (Field::Hold, "每句对白停留（秒）", "2"),
        (Field::Instance, "子场景实例（可选）", ""),
        (Field::Sound, "配乐文件（可选）", ""),
        (Field::Encoder, "FFmpeg 路径（可选）", ""),
        (Field::Choices, "Yarn 分支 JSON", "{}"),
    ];
    // 标题和常用操作保持可见。短窗仅滚动设置，取消与导出入口不会被正文裁切。
    panel
        .spawn((
            Body,
            RelativeCursorPosition::default(),
            ScrollPosition::default(),
            Node {
                max_height: Val::Vh(70.),
                overflow: Overflow::scroll_y(),
                ..feathrise_editor_ui::dialog_body_node()
            },
        ))
        .with_children(|body| {
            for (index, (field, title, value)) in fields.into_iter().enumerate() {
                field_row(body, font, index, field, title, value);
            }
            body.spawn((
                Advanced,
                Visibility::Hidden,
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.,
                    row_gap: px(8),
                    ..default()
                },
            ))
            .with_children(|group| {
                for (index, (field, title, value)) in advanced.into_iter().enumerate() {
                    field_row(group, font, fields.len() + index, field, title, value);
                }
            });
            body.spawn((
                Status,
                Name::new("CinematicExportStatus"),
                label(font, ""),
                TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                Node {
                    flex_shrink: 0.,
                    ..default()
                },
            ));
            body.spawn((
                ChoiceRow::default(),
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(6),
                    row_gap: px(4),
                    flex_shrink: 0.,
                    ..default()
                },
            ));
        });
    panel
        .spawn((
            feathrise_editor_ui::dialog_footer_node(),
            BorderColor::all(c::DIALOG_BORDER),
        ))
        .with_children(|row| {
            for (action, name, title) in [
                (Action::Options, "CinematicExportOptions", "高级设置"),
                (Action::Replace, "CinematicExportReplace", "允许覆盖：关"),
                (Action::Close, "CinematicExportClose", "关闭 (Esc)"),
                (Action::Export, "CinematicExportStart", "导出 MP4"),
            ] {
                let style = feathrise_editor_ui::dialog_button_style(action == Action::Export);
                row.spawn((
                    action,
                    Name::new(name),
                    InteractionButton,
                    style.node,
                    style.colors,
                    BackgroundColor(style.colors.normal),
                    BorderColor::all(c::DIALOG_BORDER),
                ))
                .with_child((ButtonLabel(action), label(font, title)));
            }
        });
}

fn field_row(
    p: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    index: usize,
    field: Field,
    title: &str,
    value: &str,
) {
    p.spawn(Node {
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
            Name::new(format!("CinematicExport{field:?}")),
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

pub(super) fn actions(
    mut commands: Commands,
    mut state: ResMut<MoviePanel>,
    buttons: Query<(&Action, &EditorButtonClick), Changed<EditorButtonClick>>,
    fields: Query<(Entity, &Field, &EditableText)>,
    panels: Query<Entity, With<Panel>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    mut overlay: Option<ResMut<feathrise_editor_core::EditorHostOverlay>>,
    mut guard: Option<ResMut<feathrise_editor_core::EditorTextInputGuard>>,
) {
    if state.document.is_none() {
        return;
    }
    let action = if keys.just_pressed(KeyCode::Escape) {
        Some(Action::Close)
    } else {
        buttons
            .iter()
            .find_map(|(action, click)| click.is_clicked().then_some(*action))
    };
    // 官方字段已接收本帧文字。直到关闭释放帧结束前，快捷键和鼠标仍由当前浮层持有。
    if let Some(overlay) = &mut overlay {
        overlay.active = true;
    }
    if let Some(guard) = &mut guard {
        guard.block_shortcuts = true;
    }
    let edges: Vec<_> = keys
        .get_just_pressed()
        .chain(keys.get_just_released())
        .copied()
        .collect();
    for key in edges {
        keys.clear_just_pressed(key);
        keys.clear_just_released(key);
    }
    let Some(action) = action else { return };
    if fields.iter().any(|(_, _, t)| {
        t.is_composing() || t.pending_paste.is_some() || !t.pending_edits.is_empty()
    }) {
        state.status = "请先完成当前输入。".into();
        return;
    }
    if let Some(job) = &state.running {
        if action == Action::Close {
            job.control.cancel();
            state.status = "正在取消摄影和编码".into();
        }
        return;
    }
    if focus.get().is_some_and(|id| fields.contains(id)) {
        focus.clear();
    }
    match action {
        Action::Close => {
            for e in &panels {
                commands.entity(e).despawn();
            }
            state.document = None;
        }
        Action::Options => state.advanced = !state.advanced,
        Action::Replace => state.replace = !state.replace,
        Action::Export => {
            let values = fields
                .iter()
                .map(|(_, &k, v)| (k, v.value().to_string()))
                .collect();
            match options(&state, &values) {
                Ok(v) => launch(&mut state, v),
                Err(error) => state.status = error,
            }
        }
    }
}

pub(super) fn scroll(
    state: Res<MoviePanel>,
    mut wheel: MessageReader<MouseWheel>,
    mut bodies: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<Body>>,
) {
    let (lines, pixels) = wheel.read().filter(|wheel| wheel.y.is_finite()).fold(
        (0., 0.),
        |(lines, pixels), wheel| match wheel.unit {
            MouseScrollUnit::Line => (lines + wheel.y, pixels),
            MouseScrollUnit::Pixel => (lines, pixels + wheel.y),
        },
    );
    if state.document.is_none() || (lines == 0. && pixels == 0.) {
        return;
    }
    if let Ok((cursor, node, mut scroll)) = bodies.single_mut()
        && cursor.cursor_over()
    {
        let scale = node.inverse_scale_factor();
        let max = ((node.content_size().y - node.size().y) * scale).max(0.);
        scroll.y = (scroll.y - lines * 24. - pixels * scale).clamp(0., max);
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
    buttons: Query<(&ChoiceButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut fields: Query<(&Field, &mut EditableText)>,
    mut focus: ResMut<InputFocus>,
) {
    if state.document.is_none() || state.running.is_some() {
        return;
    }
    let Some((selection, _)) = buttons.iter().find(|(choice, action)| {
        action.is_clicked()
            && state
                .choice
                .as_ref()
                .is_some_and(|prompt| prompt.key == choice.key)
    }) else {
        return;
    };
    if fields.iter().any(|(_, field)| {
        field.is_composing() || field.pending_paste.is_some() || !field.pending_edits.is_empty()
    }) {
        state.status = "请先完成当前输入。".into();
        return;
    }
    focus.clear();
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
    mut labels: Query<(&ButtonLabel, &mut Text), Without<Status>>,
    mut advanced: Query<(&mut Node, &mut Visibility), With<Advanced>>,
) {
    if state.document.is_none() || !state.is_changed() {
        return;
    }
    for (mut node, mut visible) in &mut advanced {
        node.display = if state.advanced {
            Display::Flex
        } else {
            Display::None
        };
        visible.set_if_neq(if state.advanced {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for mut t in &mut query {
        if t.as_str() != state.status {
            t.0.clone_from(&state.status);
        }
    }
    for (ButtonLabel(action), mut t) in &mut labels {
        let title = match action {
            Action::Options if state.advanced => "收起高级设置",
            Action::Options => "高级设置",
            Action::Replace if state.replace => "允许覆盖：开",
            Action::Replace => "允许覆盖：关",
            Action::Close if state.running.is_some() => "取消导出 (Esc)",
            Action::Close => "关闭 (Esc)",
            Action::Export => "导出 MP4",
        };
        if t.as_str() != title {
            t.0 = title.into();
        }
    }
}
