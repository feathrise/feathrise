//! 固定文件栏和浮窗共用右键层；固定节点池投影冻结目标，二级新建菜单按屏幕边界翻向。

use super::{components::*, messages::FileManagerContextRequest, resources::FileManagerState, types::*};
use crate::{ui::{external_files::{ExternalEditors, ExternalFileAction, EXTERNAL_MENU_ITEMS}, theme as colors}, workbench::WorkbenchFloatingPanel};
use bevy::{prelude::*, ui::FocusPolicy};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, UiInteraction};

const ROW_HEIGHT: f32 = 26.;
const MAIN_ACTIONS: [FileAction; 7] = [FileAction::NewWindow, FileAction::Cut, FileAction::Copy,
    FileAction::Paste, FileAction::Rename, FileAction::Delete, FileAction::NewMenu];
const EXTRA_ACTIONS: [ExtraAction; 4] = [ExtraAction::CopyAbsolute, ExtraAction::CopyRelative,
    ExtraAction::Favorite, ExtraAction::References];
const NEW_ACTIONS: [FileAction; 4] = [FileAction::NewFolder, FileAction::NewText, FileAction::NewMarkdown, FileAction::NewGlb];

pub(super) fn open(state: &mut FileManagerState, target: FileManagerContextRequest, viewport: Vec2) {
    let mut menu = FileContextMenu { target, submenu: false, initialized: false, main_rect: Rect::default(), submenu_rect: Rect::default() };
    layout(&mut menu, viewport, MAIN_ACTIONS.len() + EXTRA_ACTIONS.len() + 1, 0.);
    state.menu = Some(menu);
    state.gesture = None;
    state.drag = None;
    state.release_guard = true;
}

pub(super) fn target_for_window(window: &mut FileWindow, index: Option<usize>, position: Vec2) -> FileManagerContextRequest {
    if let Some(index) = index.filter(|index| *index < window.entries.len()) {
        let entry = &window.entries[index];
        // 右键已选项保留多选；右键未选项只选这一项，Ctrl/Shift 不改写右键语义。
        if !window.selection.contains(&entry.path) {
            window.selection.clear();
            window.selection.insert(entry.path.clone());
        }
        window.anchor = Some(index);
    } else {
        window.selection.clear();
        window.anchor = None;
    }
    window.last_click = None;
    let sources: Vec<_> = window.selection.iter().cloned().collect();
    let is_directory = if sources.len() == 1 {
        index.and_then(|index| window.entries.get(index)).map(|entry| entry.directory)
    } else if sources.is_empty() { Some(true) } else { None };
    let open_directory = if is_directory == Some(true) && sources.len() == 1 { sources[0].clone() } else { window.path.clone() };
    FileManagerContextRequest { position, directory: window.path.clone(), sources, open_directory, is_directory }
}

pub(super) fn enabled(action: FileContextAction, state: &FileManagerState) -> bool {
    let Some(menu) = &state.menu else { return false; };
    let target = &menu.target;
    match action {
        FileContextAction::File(FileAction::Copy | FileAction::Cut | FileAction::Delete) => !target.sources.is_empty(),
        FileContextAction::File(FileAction::Rename) => target.sources.len() == 1,
        FileContextAction::File(FileAction::Paste) => !state.clipboard.is_empty() && !target.directory.as_os_str().is_empty(),
        FileContextAction::Extra(ExtraAction::References) => target.sources.len() == 1 && target.is_directory == Some(false),
        FileContextAction::Extra(_) => target.sources.len() <= 1 && !target.directory.as_os_str().is_empty(),
        _ => !target.directory.as_os_str().is_empty(),
    }
}

fn visible(action: FileContextAction, target: &FileManagerContextRequest, available: &ExternalEditors) -> bool {
    match action {
        FileContextAction::Extra(ExtraAction::External(action)) => available.visible(action, target.is_directory == Some(false)),
        _ => true,
    }
}

fn layout(menu: &mut FileContextMenu, viewport: Vec2, rows: usize, scroll: f32) {
    // 菜单限制在编辑器内容范围内；小窗口用正文滚动保留所有命令，不向屏幕外溢出。
    let lower = Vec2::new(4., 44.);
    let upper = Vec2::new((viewport.x - 4.).max(5.), (viewport.y - 28.).max(45.));
    let available = (upper - lower).max(Vec2::ONE);
    let main_size = Vec2::new(300., rows as f32 * ROW_HEIGHT + 10.).min(available);
    let origin = menu.target.position.max(lower).min((upper - main_size).max(lower));
    menu.main_rect = Rect::from_corners(origin, origin + main_size);
    let sub_size = Vec2::new(228., NEW_ACTIONS.len() as f32 * ROW_HEIGHT + 10.).min(available);
    let right = menu.main_rect.max.x - 1.;
    let left = menu.main_rect.min.x - sub_size.x + 1.;
    let x = if right + sub_size.x <= upper.x { right } else { left };
    let y = menu.main_rect.min.y + 5. + (MAIN_ACTIONS.len() - 1) as f32 * ROW_HEIGHT - scroll;
    let origin = Vec2::new(x, y).max(lower).min((upper - sub_size).max(lower));
    menu.submenu_rect = Rect::from_corners(origin, origin + sub_size);
}

pub(super) fn mount(parent: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    parent.spawn((FileContextBackdrop, WorkbenchFloatingPanel, GlobalZIndex(426), ZIndex(0),
        UiInteraction::default(), FocusPolicy::Block,
        Node { display: Display::None, position_type: PositionType::Absolute, left: px(0), top: px(0),
            width: percent(100), height: percent(100), ..default() }));
    for submenu in [false, true] {
        parent.spawn((FileContextPanel { submenu }, WorkbenchFloatingPanel, GlobalZIndex(426), ZIndex(if submenu { 2 } else { 1 }),
            UiInteraction::default(), FocusPolicy::Block, ScrollPosition::default(),
            Node { display: Display::None, position_type: PositionType::Absolute, flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(4)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::scroll_y(), ..default() },
            BackgroundColor(colors::PANEL_BG_STRONG), BorderColor::all(colors::COPPER_BORDER_WEAK)))
            .with_children(|panel| {
                if submenu {
                    for action in NEW_ACTIONS { item(panel, font, FileContextAction::File(action), true); }
                } else {
                    for action in MAIN_ACTIONS { item(panel, font, FileContextAction::File(action), false); }
                    for action in EXTRA_ACTIONS { item(panel, font, FileContextAction::Extra(action), false); }
                    for (_, action) in EXTERNAL_MENU_ITEMS { item(panel, font, FileContextAction::Extra(ExtraAction::External(action)), false); }
                }
            });
    }
}

fn item(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, action: FileContextAction, submenu: bool) {
    let mut entity = parent.spawn((FileContextEntry { submenu, action }, InteractionButton,
        Node { width: percent(100), height: px(ROW_HEIGHT), flex_shrink: 0., align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)), border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x(), ..default() },
        BackgroundColor(colors::PANEL_BG_STRONG)));
    if let FileContextAction::File(action) = action { entity.insert(FileManagerButton { window: None, action }); }
    entity.with_child((FileContextLabel(action), super::scene::label("", font),
        Node { min_width: px(0), overflow: Overflow::clip_x(), ..default() }));
}

pub(super) fn sync(world: &mut World, state: &mut FileManagerState, viewport: Vec2, english: bool) {
    let available = world.get_resource::<ExternalEditors>().cloned().unwrap_or_default();
    let visible_menu = state.menu.is_some() && !state.suspended && state.prompt.is_none();
    let reset = state.menu.as_ref().is_some_and(|menu| !menu.initialized);
    let mut scroll_query = world.query::<(&FileContextPanel, &mut ScrollPosition)>();
    let mut scroll = 0.;
    for (panel, mut position) in scroll_query.iter_mut(world) {
        if reset { position.y = 0.; }
        if !panel.submenu { scroll = position.y; }
    }
    if let Some(menu) = &mut state.menu {
        let rows = MAIN_ACTIONS.len() + EXTRA_ACTIONS.len() + EXTERNAL_MENU_ITEMS.iter()
            .filter(|(_, action)| available.visible(*action, menu.target.is_directory == Some(false))).count();
        layout(menu, viewport, rows, scroll);
    }
    let mut panels = world.query_filtered::<(&mut Node, Option<&FileContextPanel>, Option<&FileContextBackdrop>), Or<(With<FileContextPanel>, With<FileContextBackdrop>)>>();
    for (mut node, panel, backdrop) in panels.iter_mut(world) {
        if backdrop.is_some() { node.display = if visible_menu { Display::Flex } else { Display::None }; }
        if let Some(panel) = panel {
            node.display = if visible_menu && state.menu.as_ref().is_some_and(|menu| !panel.submenu || menu.submenu) { Display::Flex } else { Display::None };
            if let Some(menu) = &state.menu {
                let rect = if panel.submenu { menu.submenu_rect } else { menu.main_rect };
                node.left = px(rect.min.x); node.top = px(rect.min.y); node.width = px(rect.width()); node.height = px(rect.height());
            }
        }
    }
    let mut entries = world.query::<(&FileContextEntry, &UiInteraction, &mut Node, &mut BackgroundColor, &mut EditorButtonClick)>();
    for (entry, interaction, mut node, mut background, mut click) in entries.iter_mut(world) {
        let show = state.menu.as_ref().is_some_and(|menu| visible(entry.action, &menu.target, &available));
        node.display = if show { Display::Flex } else { Display::None };
        let active = visible_menu && enabled(entry.action, state) && state.menu.as_ref().is_some_and(|menu| !entry.submenu || menu.submenu);
        if reset || !active { click.invalidate(); }
        background.0 = if !active { colors::PANEL_BG_STRONG } else { match interaction {
            UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
            UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            UiInteraction::None => colors::PANEL_BG_STRONG,
        } };
    }
    let mut labels = world.query::<(&FileContextLabel, &mut Text, &mut TextColor)>();
    for (label, mut text, mut color) in labels.iter_mut(world) {
        color.0 = if enabled(label.0, state) { colors::TEXT_PRIMARY } else { colors::TEXT_MUTED };
        let wanted = label_text(label.0, english, state.menu.as_ref().is_some_and(|menu| menu.target.is_directory == Some(true)));
        if text.as_str() != wanted { text.0 = wanted.into(); }
    }
    if let Some(menu) = &mut state.menu { menu.initialized = true; }
}

fn label_text(action: FileContextAction, english: bool, directory: bool) -> &'static str {
    match action {
        FileContextAction::File(FileAction::NewFolder) => if english { "Folder" } else { "文件夹" },
        FileContextAction::File(action) => action.label(english),
        FileContextAction::Extra(action) => match (action, english) {
            (ExtraAction::CopyAbsolute, false) => "复制绝对路径", (ExtraAction::CopyAbsolute, true) => "Copy absolute path",
            (ExtraAction::CopyRelative, false) => "复制相对路径", (ExtraAction::CopyRelative, true) => "Copy relative path",
            (ExtraAction::Favorite, false) => "收藏／取消收藏", (ExtraAction::Favorite, true) => "Toggle favorite",
            (ExtraAction::References, false) => "查看引用", (ExtraAction::References, true) => "Find references",
            (ExtraAction::External(action), _) => match (action, english, directory) {
                (ExternalFileAction::VsCode(_), false, _) => "用 VS Code 打开", (ExternalFileAction::VsCode(_), true, _) => "Open in VS Code",
                (ExternalFileAction::VsCodeProject(_), false, true) => "用 VS Code 打开工程", (ExternalFileAction::VsCodeProject(_), false, false) => "用 VS Code 打开工程并定位文件",
                (ExternalFileAction::VsCodeProject(_), true, true) => "Open project in VS Code", (ExternalFileAction::VsCodeProject(_), true, false) => "Open project in VS Code at file",
                (ExternalFileAction::Zed(_), false, _) => "用 Zed 打开", (ExternalFileAction::Zed(_), true, _) => "Open in Zed",
                (ExternalFileAction::ZedProject(_), false, true) => "用 Zed 打开工程", (ExternalFileAction::ZedProject(_), false, false) => "用 Zed 打开工程并定位文件",
                (ExternalFileAction::ZedProject(_), true, true) => "Open project in Zed", (ExternalFileAction::ZedProject(_), true, false) => "Open project in Zed at file",
                (ExternalFileAction::RustRover(_), false, _) => "用 RustRover 打开", (ExternalFileAction::RustRover(_), true, _) => "Open in RustRover",
                (ExternalFileAction::RustRoverProject(_), false, true) => "用 RustRover 打开工程", (ExternalFileAction::RustRoverProject(_), false, false) => "用 RustRover 打开工程并定位文件",
                (ExternalFileAction::RustRoverProject(_), true, true) => "Open project in RustRover", (ExternalFileAction::RustRoverProject(_), true, false) => "Open project in RustRover at file",
                (ExternalFileAction::Folder, false, _) => "打开所在文件夹", (ExternalFileAction::Folder, true, _) => "Open containing folder",
                (_, false, _) => "外部打开", (_, true, _) => "Open externally",
            },
        },
    }
}
