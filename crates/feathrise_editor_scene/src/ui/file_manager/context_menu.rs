//! 固定文件栏和浮窗共用右键层；固定节点池投影冻结目标，二级新建菜单按屏幕边界翻向。

use super::{components::*, messages::FileManagerContextRequest, resources::FileManagerState, types::*};
use crate::{ui::{external_files::{ExternalEditors, ExternalFileAction, EXTERNAL_MENU_ITEMS}, theme as colors}, workbench::WorkbenchFloatingPanel};
use bevy::{prelude::*, text::ComputedTextBlock, ui::{ComputedUiRenderTargetInfo, FocusPolicy}};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, UiInteraction};

const ROW_HEIGHT: f32 = 26.;
const PANEL_PADDING: f32 = 4.;
const PANEL_BORDER: f32 = 1.;
const ROW_PADDING: f32 = 8.;
const PANEL_INSET: f32 = PANEL_PADDING + PANEL_BORDER;
const MAIN_ACTIONS: [FileAction; 7] = [FileAction::NewWindow, FileAction::Cut, FileAction::Copy,
    FileAction::Paste, FileAction::Rename, FileAction::Delete, FileAction::NewMenu];
const EXTRA_ACTIONS: [ExtraAction; 4] = [ExtraAction::CopyAbsolute, ExtraAction::CopyRelative,
    ExtraAction::Favorite, ExtraAction::References];
const NEW_ACTIONS: [FileAction; 4] = [FileAction::NewFolder, FileAction::NewText, FileAction::NewMarkdown, FileAction::NewGlb];

pub(super) fn open(state: &mut FileManagerState, target: FileManagerContextRequest) {
    state.menu = Some(FileContextMenu { target, submenu: false, initialized: false, main_rect: Rect::default(), submenu_rect: Rect::default() });
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

#[derive(Default)]
struct MenuSize {
    label_width: f32,
    rows: usize,
}

impl MenuSize {
    fn size(&self, available: Vec2) -> Vec2 {
        Vec2::new(
            (self.label_width + 2. * (PANEL_INSET + ROW_PADDING)).ceil(),
            self.rows as f32 * ROW_HEIGHT + 2. * PANEL_INSET,
        ).min(available)
    }

    fn clamp_scroll(&self, position: &mut ScrollPosition, height: f32) {
        let maximum = (self.rows as f32 * ROW_HEIGHT + 2. * PANEL_INSET - height).max(0.);
        let y = position.y.clamp(0., maximum);
        if position.y != y { position.y = y; }
    }
}

fn layout(menu: &mut FileContextMenu, viewport: Vec2, sizes: &[MenuSize; 2], scale: f32, main_scroll: &mut ScrollPosition) {
    // 菜单限制在编辑器内容范围内；小窗口用正文滚动保留所有命令，不向屏幕外溢出。
    let lower = Vec2::new(4., 44.);
    let upper = Vec2::new((viewport.x - 4.).max(5.), (viewport.y - 28.).max(45.));
    let available = (upper - lower).max(Vec2::ONE);
    let main_size = sizes[0].size(available);
    let origin = menu.target.position.max(lower).min((upper - main_size).max(lower));
    menu.main_rect = Rect::from_corners(origin, origin + main_size);
    let sub_size = sizes[1].size(available);
    let right = menu.main_rect.max.x - PANEL_BORDER;
    let left = menu.main_rect.min.x - sub_size.x + PANEL_BORDER;
    // 两边都放不下时，使用空隙更大的一侧，以保留更多父菜单的可点击区域。
    let room_right = upper.x - right;
    let room_left = menu.main_rect.min.x + PANEL_BORDER - lower.x;
    let x = if sub_size.x <= room_right || (sub_size.x > room_left && room_right >= room_left) {
        right
    } else {
        left
    };
    sizes[0].clamp_scroll(main_scroll, main_size.y);
    // UI 布局把滚动偏移向下取整到物理像素，子菜单用同一偏移对齐父项。
    let scroll = (main_scroll.y * scale).floor() / scale;
    let parent_row_top = menu.main_rect.min.y + PANEL_INSET + (MAIN_ACTIONS.len() - 1) as f32 * ROW_HEIGHT - scroll;
    if parent_row_top + ROW_HEIGHT <= menu.main_rect.min.y || parent_row_top >= menu.main_rect.max.y {
        menu.submenu = false;
    }
    let y = parent_row_top;
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
                padding: UiRect::all(px(PANEL_PADDING)), border: UiRect::all(px(PANEL_BORDER)), border_radius: BorderRadius::all(px(3)),
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
            padding: UiRect::horizontal(px(ROW_PADDING)), border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x(), ..default() },
        BackgroundColor(colors::PANEL_BG_STRONG)));
    if let FileContextAction::File(action) = action { entity.insert(FileManagerButton { window: None, action }); }
    entity.with_child((FileContextLabel(action), super::scene::label("", font),
        Node { min_width: px(0), overflow: Overflow::clip_x(), ..default() }));
}

pub(super) fn sync(world: &mut World, state: &mut FileManagerState, english: bool) {
    let available = world.get_resource::<ExternalEditors>().cloned().unwrap_or_default();
    let visible_menu = state.menu.is_some() && !state.suspended && state.prompt.is_none();
    let reset = state.menu.as_ref().is_some_and(|menu| !menu.initialized);
    let mut backdrop = world.query_filtered::<&mut Node, With<FileContextBackdrop>>();
    for mut node in backdrop.iter_mut(world) {
        node.display = if visible_menu { Display::Flex } else { Display::None };
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
        let wanted_color = if enabled(label.0, state) { colors::TEXT_PRIMARY } else { colors::TEXT_MUTED };
        if color.0 != wanted_color { color.0 = wanted_color; }
        let wanted = label_text(label.0, english, state.menu.as_ref().is_some_and(|menu| menu.target.is_directory == Some(true)));
        if text.as_str() != wanted { text.0 = wanted.into(); }
    }
}

pub(super) fn place(
    mut state: ResMut<FileManagerState>,
    roots: Query<&ComputedUiRenderTargetInfo, With<crate::components::SceneEditorRoot>>,
    entries: Query<(&FileContextEntry, &Node)>,
    labels: Query<(&ChildOf, &ComputedTextBlock), With<FileContextLabel>>,
    mut panels: Query<(&FileContextPanel, &mut Node, &mut ScrollPosition), Without<FileContextEntry>>,
) {
    let suspended = state.suspended || state.prompt.is_some();
    let mut sizes = [MenuSize::default(), MenuSize::default()];
    let mut rects = None;
    if let Some(menu) = state.menu.as_mut().filter(|_| !suspended)
        && let Ok(target) = roots.single()
        && target.physical_size().min_element() > 0
    {
        // 原文字管线在本帧已经塑形 NoWrap 标签（包括尚未展开的子菜单），无需另测字体或先显示占位宽度。
        let mut ready = true;
        for (parent, text) in &labels {
            let Ok((entry, node)) = entries.get(parent.parent()) else { continue; };
            if node.display == Display::None { continue; }
            let buffer = text.buffer();
            let width = buffer.full_width();
            if width <= 0. || text.needs_rerender(false, false) {
                ready = false;
                break;
            }
            let size = &mut sizes[usize::from(entry.submenu)];
            size.label_width = size.label_width.max(width.ceil() / buffer.scale());
            size.rows += 1;
        }
        if ready && sizes[0].rows > 0 && sizes[1].rows > 0 {
            for (panel, _, mut scroll) in &mut panels {
                if !menu.initialized { scroll.y = 0.; }
                if !panel.submenu { layout(menu, target.logical_size(), &sizes, target.scale_factor(), &mut scroll); }
            }
            rects = Some([Some(menu.main_rect), menu.submenu.then_some(menu.submenu_rect)]);
            menu.initialized = true;
        }
    }
    for (panel, mut node, mut scroll) in &mut panels {
        let index = usize::from(panel.submenu);
        if let Some(rect) = rects.and_then(|rects| rects[index]) {
            let (left, top, width, height) = (px(rect.min.x), px(rect.min.y), px(rect.width()), px(rect.height()));
            if (node.display, node.left, node.top, node.width, node.height) != (Display::Flex, left, top, width, height) {
                node.display = Display::Flex;
                node.left = left;
                node.top = top;
                node.width = width;
                node.height = height;
            }
            if panel.submenu {
                sizes[index].clamp_scroll(&mut scroll, rect.height());
            }
        } else if node.display != Display::None {
            node.display = Display::None;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submenu_uses_its_own_width_and_follows_the_parent_after_scrolling() {
        let mut menu = FileContextMenu {
            target: FileManagerContextRequest {
                position: Vec2::new(480., 190.),
                directory: Default::default(),
                sources: vec![],
                open_directory: Default::default(),
                is_directory: None,
            },
            submenu: true,
            initialized: false,
            main_rect: Rect::default(),
            submenu_rect: Rect::default(),
        };
        let sizes = [MenuSize { label_width: 150.5, rows: 16 }, MenuSize { label_width: 81.5, rows: 4 }];
        let mut scroll = ScrollPosition(Vec2::new(0., 96.));
        layout(&mut menu, Vec2::new(500., 300.), &sizes, 1.5, &mut scroll);

        assert_eq!(menu.main_rect.width(), 177.);
        assert_eq!(menu.submenu_rect.width(), 108.);
        assert_eq!(menu.main_rect.max.x, 496.);
        assert_eq!(menu.submenu_rect.max.x, menu.main_rect.min.x + PANEL_BORDER);
        assert_eq!(menu.submenu_rect.min.y, menu.main_rect.min.y + PANEL_INSET + 6. * ROW_HEIGHT - scroll.y);

        scroll.y = 1000.;
        layout(&mut menu, Vec2::new(130., 240.), &sizes, 1.5, &mut scroll);
        assert_eq!(menu.main_rect.min.x, 4.);
        assert_eq!(menu.main_rect.width(), 122.);
        assert!(menu.submenu_rect.min.x >= 4. && menu.submenu_rect.max.x <= 126.);
        assert_eq!(scroll.y, 258.);
        assert!(!menu.submenu, "父项完全滚出菜单后应收起子菜单");
    }
}
