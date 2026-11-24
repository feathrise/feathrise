//! 固定文件栏和浮窗共用右键层；固定节点池投影冻结目标，二级新建菜单按屏幕边界翻向。

use super::{components::*, messages::FileManagerContextRequest, resources::FileManagerState, types::*};
use crate::{ui::{external_files::{ExternalEditors, ExternalFileAction, EXTERNAL_MENU_ITEMS},
    menu_style::{self, PANEL_BORDER, PANEL_INSET, ROW_HEIGHT}, theme as colors}, workbench::WorkbenchFloatingPanel};
use bevy::{prelude::*, text::ComputedTextBlock, ui::{ComputedUiRenderTargetInfo, FocusPolicy}};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};

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
            window.selection.select(&window.entries, index..index + 1, false);
        }
        window.anchor = Some(index);
    } else {
        window.selection.clear();
        window.anchor = None;
    }
    window.last_click = None;
    let sources = window.selection.clone();
    let is_directory = if sources.len() == 1 {
        index.and_then(|index| window.entries.get(index)).map(|entry| entry.directory)
    } else if sources.is_empty() { Some(true) } else { None };
    let open_directory = if is_directory == Some(true) && sources.len() == 1 {
        sources.first().unwrap().to_path_buf()
    } else { window.path.clone() };
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
    content_width: f32,
    rows: usize,
}

impl MenuSize {
    fn size(&self, available: Vec2) -> Vec2 {
        Vec2::new(
            menu_style::measured_width(self.content_width),
            menu_style::panel_height(self.rows),
        ).min(available)
    }

    fn clamp_scroll(&self, position: &mut ScrollPosition, height: f32) {
        let maximum = (menu_style::panel_height(self.rows) - height).max(0.);
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
    let parent_row_top = menu.main_rect.min.y + PANEL_INSET
        + (MAIN_ACTIONS.len() - 1) as f32 * menu_style::row_pitch() - scroll;
    if parent_row_top + ROW_HEIGHT <= menu.main_rect.min.y || parent_row_top >= menu.main_rect.max.y {
        menu.submenu = false;
    }
    let y = parent_row_top;
    let origin = Vec2::new(x, y).max(lower).min((upper - sub_size).max(lower));
    menu.submenu_rect = Rect::from_corners(origin, origin + sub_size);
}

pub(super) fn mount(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, arrow: &Handle<Image>) {
    parent.spawn((FileContextBackdrop, WorkbenchFloatingPanel, GlobalZIndex(426), ZIndex(0),
        UiInteraction::default(), FocusPolicy::Block,
        Node { display: Display::None, position_type: PositionType::Absolute, left: px(0), top: px(0),
            width: percent(100), height: percent(100), ..default() }));
    for submenu in [false, true] {
        parent.spawn((FileContextPanel { submenu }, WorkbenchFloatingPanel, GlobalZIndex(426), ZIndex(if submenu { 2 } else { 1 }),
            UiInteraction::default(), FocusPolicy::Block, ScrollPosition::default(),
            Node { display: Display::None, ..menu_style::panel() },
            BackgroundColor(colors::PANEL_BG_STRONG), BorderColor::all(colors::COPPER_BORDER_WEAK)))
            .with_children(|panel| {
                if submenu {
                    for action in NEW_ACTIONS { item(panel, font, FileContextAction::File(action), true, arrow); }
                } else {
                    for action in MAIN_ACTIONS { item(panel, font, FileContextAction::File(action), false, arrow); }
                    for action in EXTRA_ACTIONS { item(panel, font, FileContextAction::Extra(action), false, arrow); }
                    for (_, action) in EXTERNAL_MENU_ITEMS { item(panel, font, FileContextAction::Extra(ExtraAction::External(action)), false, arrow); }
                }
            });
    }
}

fn item(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, action: FileContextAction, submenu: bool, arrow: &Handle<Image>) {
    let mut entity = parent.spawn((FileContextEntry { submenu, action }, menu_style::item()));
    if let FileContextAction::File(action) = action { entity.insert(FileManagerButton { window: None, action }); }
    entity.with_children(|item| {
        item.spawn((FileContextLabel(action), menu_style::label(Text::new(""), font, colors::TEXT_PRIMARY)));
        if matches!(action, FileContextAction::File(FileAction::NewMenu)) {
            item.spawn(menu_style::arrow(arrow));
        }
    });
}

pub(super) fn sync(world: &mut World, state: &mut FileManagerState, english: bool) {
    let available = world.get_resource::<ExternalEditors>().cloned().unwrap_or_default();
    let visible_menu = state.menu.is_some() && !state.suspended && state.prompt.is_none();
    let reset = state.menu.as_ref().is_some_and(|menu| !menu.initialized);
    let mut backdrop = world.query_filtered::<&mut Node, With<FileContextBackdrop>>();
    for mut node in backdrop.iter_mut(world) {
        node.display = if visible_menu { Display::Flex } else { Display::None };
    }
    let mut entries = world.query::<(&FileContextEntry, &UiInteraction, &mut Node, &mut BackgroundColor,
        &mut BorderColor, &mut EditorButtonClick)>();
    for (entry, interaction, mut node, mut background, mut border, mut click) in entries.iter_mut(world) {
        let show = state.menu.as_ref().is_some_and(|menu| visible(entry.action, &menu.target, &available));
        node.display = if show { Display::Flex } else { Display::None };
        let active = visible_menu && enabled(entry.action, state) && state.menu.as_ref().is_some_and(|menu| !entry.submenu || menu.submenu);
        if reset || !active { click.invalidate(); }
        let open_branch = matches!(entry.action, FileContextAction::File(FileAction::NewMenu))
            && state.menu.as_ref().is_some_and(|menu| menu.submenu);
        let interaction = if open_branch && *interaction == UiInteraction::None { UiInteraction::Hovered } else { *interaction };
        let (color, outline) = menu_style::item_colors(active, interaction);
        background.set_if_neq(BackgroundColor(color));
        border.set_if_neq(outline);
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
            // 只有「新建」带右侧箭头，文字仍使用实际字形，不为整栏预留固定宽度。
            let arrow_space = if matches!(entry.action, FileContextAction::File(FileAction::NewMenu)) { menu_style::ARROW_SPACE } else { 0. };
            size.content_width = size.content_width.max(width.ceil() / buffer.scale() + arrow_space);
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
        FileContextAction::File(FileAction::NewMenu) => if english { "New" } else { "新建" },
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
    use bevy::ecs::world::CommandQueue;

    #[test]
    fn file_context_primary_and_submenu_share_entry_style() {
        let mut world = World::new();
        let root = world.spawn_empty().id();
        let mut queue = CommandQueue::default();
        Commands::new(&mut queue, &world).entity(root).with_children(|parent| {
            mount(parent, &Handle::default(), &Handle::default());
        });
        queue.apply(&mut world);
        let mut state = FileManagerState::default();
        open(&mut state, FileManagerContextRequest {
            position: Vec2::ZERO,
            directory: "fixture".into(),
            sources: default(),
            open_directory: "fixture".into(),
            is_directory: Some(true),
        });
        state.menu.as_mut().unwrap().submenu = true;

        let panels: Vec<_> = world.query::<(&FileContextPanel, &Node)>().iter(&world)
            .map(|(_, node)| (node.row_gap, node.border_radius)).collect();
        assert_eq!(panels, vec![(px(2.), BorderRadius::all(px(6.))); 2]);
        let actions = [FileAction::NewWindow, FileAction::NewFolder];
        let rows = actions.map(|action| {
            let (entity, node, children) = world.query::<(Entity, &FileContextEntry, &Node, &Children)>()
                .iter(&world)
                .find_map(|(entity, entry, node, children)| {
                    (entry.action == FileContextAction::File(action)).then_some((entity, node, children))
                }).unwrap();
            assert_eq!(node.height, px(26.));
            assert_eq!(node.padding, UiRect::horizontal(px(8.)));
            assert_eq!(node.border, UiRect::all(px(1.)));
            assert_eq!(node.border_radius, BorderRadius::all(px(3.)));
            let font = world.get::<TextFont>(children[0]).unwrap();
            assert_eq!(font.font_size, 11.0.into());
            entity
        });
        for (interaction, fill, outline) in [
            (UiInteraction::None, colors::BUTTON_DEV_SMALL_NORMAL, colors::COPPER_BORDER_WEAK),
            (UiInteraction::Hovered, colors::BUTTON_DEV_SMALL_HOVERED, colors::COPPER_BORDER),
            (UiInteraction::Pressed, colors::BUTTON_DEV_SMALL_PRESSED, colors::COPPER_BORDER),
        ] {
            for row in rows {
                world.entity_mut(row).insert((interaction, EditorButtonClick::default()));
            }
            sync(&mut world, &mut state, false);
            for row in rows {
                assert_eq!(world.get::<BackgroundColor>(row).unwrap().0, fill);
                assert_eq!(world.get::<BorderColor>(row), Some(&BorderColor::all(outline)));
            }
        }
    }

    #[test]
    fn submenu_uses_its_own_width_and_follows_the_parent_after_scrolling() {
        let mut menu = FileContextMenu {
            target: FileManagerContextRequest {
                position: Vec2::new(480., 190.),
                directory: Default::default(),
                sources: default(),
                open_directory: Default::default(),
                is_directory: None,
            },
            submenu: true,
            initialized: false,
            main_rect: Rect::default(),
            submenu_rect: Rect::default(),
        };
        let sizes = [MenuSize { content_width: 150.5, rows: 16 }, MenuSize { content_width: 81.5, rows: 4 }];
        let mut scroll = ScrollPosition(Vec2::new(0., 96.));
        layout(&mut menu, Vec2::new(500., 300.), &sizes, 1.5, &mut scroll);

        assert_eq!(menu.main_rect.width(), 179.);
        assert_eq!(menu.submenu_rect.width(), 110.);
        assert_eq!(menu.main_rect.max.x, 496.);
        assert_eq!(menu.submenu_rect.max.x, menu.main_rect.min.x + PANEL_BORDER);
        assert_eq!(menu.submenu_rect.min.y, menu.main_rect.min.y + PANEL_INSET + 6. * menu_style::row_pitch() - scroll.y);

        scroll.y = 1000.;
        layout(&mut menu, Vec2::new(130., 240.), &sizes, 1.5, &mut scroll);
        assert_eq!(menu.main_rect.min.x, 4.);
        assert_eq!(menu.main_rect.width(), 122.);
        assert!(menu.submenu_rect.min.x >= 4. && menu.submenu_rect.max.x <= 126.);
        assert_eq!(scroll.y, 288.);
        assert!(!menu.submenu, "父项完全滚出菜单后应收起子菜单");
    }
}
