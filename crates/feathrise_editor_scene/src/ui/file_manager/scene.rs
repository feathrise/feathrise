//! 文件窗口复用作者字体、暗色面板和释放点击；每窗固定六十四行，不随目录大小增加实体。

use super::{components::*, resources::FileManagerState, types::FileAction};
use crate::{components::SceneEditorRoot, ui::{scene::FONT_PATH, theme as colors}, workbench::WorkbenchFloatingPanel};
use bevy::{prelude::*, ui::{FocusPolicy, RelativeCursorPosition}, input_focus::tab_navigation::TabIndex};
use feathrise_editor_ui::{InteractionButton, UiInteraction, metrics};

pub(super) fn label(value: &str, font: &Handle<Font>) -> impl Bundle {
    (Text::new(value), TextFont { font: font.clone().into(), font_size: metrics::FONT_CONTROL.into(), ..default() },
        TextColor(colors::TEXT_PRIMARY), TextLayout::no_wrap(), FocusPolicy::Pass, Pickable::IGNORE)
}

fn icon(image: Handle<Image>) -> impl Bundle {
    // 文件图标复用原树形列表资产，不再依赖中文字体是否覆盖特殊符号。
    (ImageNode { image, color: colors::TEXT_PRIMARY, ..default() },
        Node { width: px(16), height: px(16), flex_shrink: 0., ..default() }, Pickable::IGNORE, FocusPolicy::Pass)
}

fn button(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, window: Option<u64>, action: FileAction) {
    parent.spawn((InteractionButton, FileManagerButton { window, action },
        Node { height: px(26), min_width: px(26), flex_shrink: 0., align_items: AlignItems::Center,
            justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(6)), border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)), ..default() },
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(colors::COPPER_BORDER_WEAK)))
        .with_child((ButtonLabel(action), label(action.label(false), font)));
}

fn input(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, marker: impl Bundle, value: &str) -> Entity {
    parent.spawn((marker, crate::ui::text_input::EditorTextInput, TabIndex(0), FocusPolicy::Block,
        feathrise_editor_ui::single_line_input(value, 4096), bevy::text::EditableTextFilter::new(|character| !character.is_control()),
        bevy::text::TextCursorStyle::default(), TextLayout::no_wrap(),
        Node { height: px(28), min_width: px(0), flex_grow: 1., flex_basis: px(0),
            padding: UiRect::horizontal(px(6)), border: UiRect::all(px(1)), overflow: Overflow::clip(), ..default() },
        BackgroundColor(colors::INPUT_BG), BorderColor::all(colors::COPPER_BORDER_WEAK),
        TextFont { font: font.clone().into(), font_size: metrics::FONT_CONTROL.into(), ..default() }, TextColor(colors::TEXT_VALUE))).id()
}

pub(super) fn mount(
    mut commands: Commands,
    roots: Query<Entity, With<SceneEditorRoot>>,
    windows: Query<(Entity, &FileWindowRoot)>,
    tasks: Query<(Entity, &FileTaskButton)>,
    bars: Query<Entity, With<DesktopTaskbar>>,
    state: Res<FileManagerState>,
    assets: Res<AssetServer>,
) {
    let Ok(root) = roots.single() else { return; };
    let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
    let folder_image: Handle<Image> = assets.load("icons/editor/tree_folder.png");
    let file_image: Handle<Image> = assets.load("icons/editor/tree_file.png");
    if bars.is_empty() {
        commands.entity(root).with_children(|parent| {
            super::context_menu::mount(parent, &font);
            parent.spawn((DesktopTaskbar, WorkbenchFloatingPanel, GlobalZIndex(422), FocusPolicy::Block,
                Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(24), height: px(32),
                    align_items: AlignItems::Center, column_gap: px(4), padding: UiRect::horizontal(px(8)), border: UiRect::top(px(1)),
                    overflow: Overflow::clip(), ..default() }, BackgroundColor(colors::PANEL_BG_STRONG), BorderColor::all(colors::COPPER_BORDER_WEAK)))
                .with_children(|bar| {
                    bar.spawn((OperationStatus, label("", &font),
                        Node { position_type: PositionType::Absolute, right: px(100), max_width: percent(45), min_width: px(0), overflow: Overflow::clip_x(), ..default() }));
                    bar.spawn((Node { position_type: PositionType::Absolute, right: px(6), ..default() },)).with_children(|row| button(row, &font, None, FileAction::CancelTask));
                });
            // 确认期间由透明背景接管空白点击；不能仅靠业务鼠标门让官方文本焦点落到背后的文件。
            parent.spawn((NameDialogBackdrop, WorkbenchFloatingPanel, GlobalZIndex(428), FocusPolicy::Block, UiInteraction::default(),
                Node { display: Display::None, position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), ..default() }));
            parent.spawn((NameDialog, WorkbenchFloatingPanel, GlobalZIndex(429), FocusPolicy::Block, UiInteraction::default(),
                Node { display: Display::None, position_type: PositionType::Absolute, width: px(440), min_height: px(150),
                    flex_direction: FlexDirection::Column, padding: UiRect::all(px(12)), row_gap: px(12), border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)), ..default() },
                BackgroundColor(feathrise_editor_ui::colors::DIALOG_BG), BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)))
                .with_children(|dialog| {
                    dialog.spawn((NameDialogTitle, label("文件操作", &font), Node { min_height: px(26), ..default() }));
                    dialog.spawn((NameDialogBody, ScrollPosition::default(),
                        Node { display: Display::None, width: percent(100), min_height: px(0), max_height: px(240),
                            flex_direction: FlexDirection::Column, flex_shrink: 1., overflow: Overflow::scroll_y(), ..default() }))
                        .with_children(|body| {
                            body.spawn((NameDialogBodyText, label("", &font), Node { width: percent(100), min_width: px(0), flex_shrink: 0., ..default() }))
                                .insert(TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter));
                        });
                    input(dialog, &font, FileNameInput, "");
                    dialog.spawn((Node { justify_content: JustifyContent::FlexEnd, column_gap: px(6), ..default() },)).with_children(|row| {
                        button(row, &font, None, FileAction::Confirm); button(row, &font, None, FileAction::Cancel);
                    });
                });
            parent.spawn((DragLabel, GlobalZIndex(427), label("", &font),
                Node { display: Display::None, position_type: PositionType::Absolute, min_width: px(0), max_width: px(520),
                    padding: UiRect::all(px(8)), border: UiRect::all(px(1)), overflow: Overflow::clip(), ..default() },
                BackgroundColor(colors::PANEL_BG_STRONG), BorderColor::all(colors::COPPER_BORDER)))
                .insert(TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter));
        });
    }
    for (entity, window) in &windows {
        if !state.windows.iter().any(|candidate| candidate.id == window.id) { commands.entity(entity).despawn(); }
    }
    for (entity, task) in &tasks {
        if !state.windows.iter().any(|candidate| candidate.id == task.window) { commands.entity(entity).despawn(); }
    }
    for window in &state.windows {
        if !windows.iter().any(|(_, existing)| existing.id == window.id) {
            let id = window.id;
            commands.entity(root).with_children(|parent| {
                parent.spawn((FileWindowRoot { id }, WorkbenchFloatingPanel, UiInteraction::default(), FocusPolicy::Block,
                    GlobalZIndex(423), ZIndex(0), Node { position_type: PositionType::Absolute, width: px(window.rect.width()), height: px(window.rect.height()),
                        left: px(window.rect.min.x), top: px(window.rect.min.y), flex_direction: FlexDirection::Column, border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(3)), overflow: Overflow::clip(), ..default() },
                    BackgroundColor(colors::PANEL_BG), BorderColor::all(colors::COPPER_BORDER_WEAK)))
                    .with_children(|panel| {
                        panel.spawn((FileTitleBar { window: id }, InteractionButton,
                            Node { width: percent(100), height: px(36), flex_shrink: 0., align_items: AlignItems::Center,
                                column_gap: px(4), padding: UiRect::horizontal(px(8)), ..default() }, BackgroundColor(colors::PANEL_BG_STRONG)))
                            .with_children(|header| {
                                header.spawn(icon(folder_image.clone()));
                                header.spawn((WindowTitle(id), label("文件", &font), Node { flex_grow: 1., min_width: px(0), overflow: Overflow::clip_x(), ..default() }));
                                button(header, &font, Some(id), FileAction::Minimize);
                                button(header, &font, Some(id), FileAction::Maximize);
                                button(header, &font, Some(id), FileAction::Close);
                            });
                        panel.spawn((Node { width: percent(100), height: px(38), flex_shrink: 0., align_items: AlignItems::Center,
                            column_gap: px(4), padding: UiRect::horizontal(px(8)), ..default() },)).with_children(|navigation| {
                            button(navigation, &font, Some(id), FileAction::Back);
                            button(navigation, &font, Some(id), FileAction::Forward);
                            button(navigation, &font, Some(id), FileAction::Up);
                            input(navigation, &font, PathInput { window: id }, &crate::ui::external_files::platform_path(&window.path).to_string_lossy());
                            button(navigation, &font, Some(id), FileAction::Refresh);
                        });
                        super::browser::spawn_toolbar(panel, &font, id);
                        panel.spawn((FileList { window: id }, UiInteraction::default(), RelativeCursorPosition::default(),
                            Node { width: percent(100), min_height: px(0), flex_grow: 1., flex_direction: FlexDirection::Column,
                            padding: UiRect::horizontal(px(8)), overflow: Overflow::clip(), ..default() },)).with_children(|body| {
                            for slot in 0..64 {
                                body.spawn((FileRow { window: id, slot, index: None, path: None, revision: 0 }, InteractionButton,
                                    Node { display: Display::None, width: percent(100), height: px(26), flex_shrink: 0., align_items: AlignItems::Center,
                                        padding: UiRect::horizontal(px(6)), column_gap: px(6), border_radius: BorderRadius::all(px(2)), ..default() },
                                    BackgroundColor(colors::PANEL_BG)))
                                    .with_children(|row| {
                                        row.spawn((FileRowIcon, icon(file_image.clone())));
                                        row.spawn((RowLabel, label("", &font), Node { min_width: px(0), overflow: Overflow::clip_x(), ..default() }));
                                    });
                            }
                        });
                        panel.spawn((WindowStatus(id), label("", &font), Node { width: percent(100), height: px(26), flex_shrink: 0.,
                            padding: UiRect::horizontal(px(8)), overflow: Overflow::clip_x(), ..default() }));
                        // 独立边角命中条只占 5px；不把正文整块当作缩放手柄。
                        for edges in [1_u8, 2, 4, 8, 5, 6, 9, 10] {
                            let corner = edges.count_ones() == 2;
                            let mut node = Node { position_type: PositionType::Absolute,
                                width: if corner || edges & 3 != 0 { px(if corner { 10 } else { 5 }) } else { percent(100) },
                                height: if corner || edges & 12 != 0 { px(if corner { 10 } else { 5 }) } else { percent(100) }, ..default() };
                            if edges & 1 != 0 { node.left = px(0); }
                            if edges & 2 != 0 { node.right = px(0); }
                            if edges & 4 != 0 { node.top = px(0); }
                            if edges & 8 != 0 { node.bottom = px(0); }
                            if edges == 1 || edges == 2 { node.top = px(0); }
                            if edges == 4 || edges == 8 { node.left = px(0); }
                            panel.spawn((FileResizeHandle { window: id, edges }, InteractionButton, node));
                        }
                    });
            });
        }
        if !tasks.iter().any(|(_, task)| task.window == window.id) {
            if let Ok(bar) = bars.single() {
                commands.entity(bar).with_children(|row| {
                    row.spawn((FileTaskButton { window: window.id }, InteractionButton,
                        Node { height: px(26), max_width: px(180), min_width: px(70), align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(8)), column_gap: px(4), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
                        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(colors::COPPER_BORDER_WEAK)))
                        .with_children(|task| {
                            task.spawn(icon(folder_image.clone()));
                            task.spawn((TaskTitle(window.id), label(&window.path.file_name().unwrap_or_default().to_string_lossy(), &font),
                                Node { min_width: px(0), overflow: Overflow::clip_x(), ..default() }));
                        });
                });
            }
        }
    }
}
