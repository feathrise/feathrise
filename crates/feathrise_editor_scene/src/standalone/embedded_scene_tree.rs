//! 工程 3D 场景树的只读分支，按原层级顺序通过滚动行池展示。

use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::RelativeCursorPosition,
};
use feathrise_editor_ui::{localization::EditorLocale, metrics};
use std::sync::Arc;

use crate::{
    SceneEditorState, SceneEditorSystemSet,
    authoring::AuthoringRetirement,
    ui::{
        list_layout::{self, SceneObjectListScroll},
        scene::FONT_PATH,
        sidebars::SceneEditorSidebarState,
        theme,
    },
};

/// 分类使用作者界面语言，工程自己设置的名称原样显示。
pub struct ProjectSceneTreeRow {
    pub depth: u32,
    pub category: [&'static str; 2],
    pub name: Option<String>,
    pub index: u32,
}

#[derive(Resource, Default)]
pub(crate) struct ProjectSceneTree {
    rows: Option<Arc<Vec<ProjectSceneTreeRow>>>,
    retirement: Arc<()>,
    generation: u64,
    unavailable: bool,
}

pub fn install(app: &mut App) {
    app.init_resource::<ProjectSceneTree>()
        .init_resource::<AuthoringRetirement>()
        .add_systems(Update, present.in_set(SceneEditorSystemSet::Presentation));
}

pub fn visible(world: &World) -> bool {
    world.get_resource::<SceneEditorState>().is_some_and(|editor| editor.open)
        && world.get_resource::<SceneEditorSidebarState>().is_some_and(SceneEditorSidebarState::scene_tree_visible)
}

pub fn publish(world: &mut World, rows: Arc<Vec<ProjectSceneTreeRow>>) {
    let previous = if let Some(mut state) = world.get_resource_mut::<ProjectSceneTree>() {
        state.unavailable = false;
        state.rows.replace(rows)
    } else {
        Some(rows)
    };
    if let Some(previous) = previous {
        retire_rows(world, previous);
    }
}

pub fn ready_to_request(world: &World) -> bool {
    world.get_resource::<ProjectSceneTree>()
        .is_some_and(|tree| Arc::strong_count(&tree.retirement) == 1)
}

pub fn retire_rows(world: &mut World, rows: Arc<Vec<ProjectSceneTreeRow>>) {
    // 查询只有在已有队列排空时继续。晚到的回包和清空中的显示表仍由原通信容量兜住，
    // 释放行表后才释放票据，避免不断把大正文堆进作者回收队列。
    let credit = world.get_resource::<ProjectSceneTree>().map(|tree| tree.retirement.clone());
    world.get_resource_or_init::<AuthoringRetirement>().push((rows, credit));
}

pub fn unavailable(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<ProjectSceneTree>() {
        state.unavailable = true;
    }
}

pub fn clear(world: &mut World) {
    let previous = world.get_resource_mut::<ProjectSceneTree>().and_then(|mut state| {
        state.generation = state.generation.wrapping_add(1);
        state.unavailable = false;
        state.rows.take()
    });
    if let Some(previous) = previous {
        retire_rows(world, previous);
    }
}

#[derive(Component, Clone, Copy, Default)]
struct Panel;

#[derive(Component, Clone, Copy, Default)]
struct Heading;

#[derive(Component, Clone, Copy, Default)]
struct Viewport;

#[derive(Component, Clone, Copy, Default)]
struct Content;

#[derive(Component, Clone, Copy)]
struct Slot(usize);

fn row(index: usize) -> impl Scene {
    bsn! {
        ~{Slot(index)} Text::default() TextLayout::no_wrap()
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
        TextColor(theme::TEXT_PRIMARY)
        Node { display: Display::None, position_type: PositionType::Absolute, width: percent(100),
            height: px(24), overflow: Overflow::clip_x(), align_items: AlignItems::Center }
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! {
        Panel Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: px(3) }
        Children [
            Heading Text::default() TextLayout::no_wrap()
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_HINT) }
                TextColor(theme::TEXT_MUTED)
                Node { width: percent(100), overflow: Overflow::clip_x(), padding: UiRect::left(px(5)) }--
            Viewport SceneObjectListScroll RelativeCursorPosition::default() ScrollPosition::default()
                Node { display: Display::None, width: percent(100), height: px(0), flex_shrink: 0.0, overflow: Overflow::scroll_y() }
                Children [
                    Content Node { width: percent(100), height: px(0), flex_shrink: 0.0 }
                    Children [@row(0)-- @row(1)-- @row(2)-- @row(3)-- @row(4)-- @row(5)-- @row(6)-- @row(7)-- @row(8)-- @row(9)]
                ]
        ]
    }
}

#[derive(Default)]
struct ListState {
    generation: u64,
    count: usize,
    start: usize,
}

fn present(
    source: Res<ProjectSceneTree>,
    locale: Res<EditorLocale>,
    mut state: Local<ListState>,
    panel: Single<&mut Node, (With<Panel>, Without<Viewport>, Without<Content>, Without<Slot>)>,
    heading: Single<&mut Text, (With<Heading>, Without<Slot>)>,
    viewport: Single<(&mut Node, &mut ScrollPosition), (With<Viewport>, Without<Content>, Without<Slot>)>,
    content: Single<&mut Node, (With<Content>, Without<Slot>)>,
    mut slots: Query<(&Slot, &mut Text, &mut Node), Without<Heading>>,
) {
    let state = &mut *state;
    // 同一场景刷新完整行表时保留滚动位置，切换场景后从第一行开始。
    let reset = state.generation != source.generation;
    state.generation = source.generation;
    let rows = source.rows.as_deref().map_or(&[][..], Vec::as_slice);
    let count = rows.len();
    let mut panel = panel.into_inner();
    let mut title = heading.into_inner();
    let (mut viewport, mut scroll) = viewport.into_inner();
    let mut content = content.into_inner();
    let layout_changed = reset || scroll.is_changed() || count != state.count || panel.is_added();
    if !layout_changed && !source.is_changed() && !locale.is_changed() { return; }
    let english = *locale == EditorLocale::English;
    let caption = match (rows.is_empty() && source.unavailable, english) {
        (true, false) => "工程对象暂不可用",
        (true, true) => "Project objects unavailable",
        (false, false) => "工程对象（只读）",
        (false, true) => "Project objects (read-only)",
    };
    let display = if count > 0 || rows.is_empty() && source.unavailable { Display::Flex } else { Display::None };
    if panel.display != display { panel.display = display; }
    if title.as_str() != caption { title.0 = caption.to_owned(); }

    if layout_changed {
        state.start = list_layout::fit_scene_object_list(count, reset, None, &mut viewport, &mut content, &mut scroll);
        state.count = count;
    }
    for (slot, mut text, mut node) in &mut slots {
        let index = state.start + slot.0;
        if index >= count {
            if node.display != Display::None { node.display = Display::None; }
            continue;
        }
        let item = &rows[index];
        let category = item.category[usize::from(english)];
        let caption = match item.name.as_deref() {
            Some(name) => format!("{category} · {name}"),
            None => format!("{category} #{}", item.index),
        };
        text.set_if_neq(Text(caption));
        let top = px(index as f32 * list_layout::SCENE_OBJECT_ROW_PITCH);
        // 深树的缩进在侧栏宽度的一部分内收敛，留出阅读名称的空间。
        let left = percent((item.depth as f32 * 4.0).min(40.0));
        if node.display != Display::Flex || node.top != top || node.padding.left != left {
            node.display = Display::Flex;
            node.top = top;
            node.padding = UiRect { left, right: px(5), ..default() };
        }
    }
}

#[cfg(feature = "validation")]
pub fn received_rows(world: &World) -> &[ProjectSceneTreeRow] {
    world.get_resource::<ProjectSceneTree>()
        .and_then(|tree| tree.rows.as_deref())
        .map_or(&[], Vec::as_slice)
}

#[cfg(feature = "validation")]
pub fn rendered_rows(world: &mut World) -> Vec<(String, Val)> {
    if !visible(world) { return Vec::new(); }
    world.query_filtered::<(&Text, &Node, &ComputedNode), With<Slot>>().iter(world)
        .filter(|(_, node, computed)| node.display != Display::None && computed.size().min_element() > 0.0)
        .map(|(text, node, _)| (text.0.clone(), node.padding.left))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_tree_replacement_and_clear_wait_for_background_retirement() {
        let mut world = World::new();
        world.init_resource::<ProjectSceneTree>();
        let first = Arc::new(Vec::new());
        let second = Arc::new(Vec::new());
        let first_ref = Arc::downgrade(&first);
        let second_ref = Arc::downgrade(&second);
        publish(&mut world, first);
        assert!(ready_to_request(&world));

        publish(&mut world, second);
        assert!(!ready_to_request(&world));
        assert!(first_ref.upgrade().is_some());
        clear(&mut world);
        assert!(world.resource::<ProjectSceneTree>().rows.is_none());
        assert!(second_ref.upgrade().is_some());

        // 销毁回收资源等价于消费其持有权，确认前台没有另留旧行表与节流票据。
        drop(world.remove_resource::<AuthoringRetirement>());
        assert!(first_ref.upgrade().is_none());
        assert!(second_ref.upgrade().is_none());
        assert!(ready_to_request(&world));
    }

    #[test]
    fn scene_tree_keeps_scroll_on_refresh_and_resets_for_next_scene() {
        let mut app = App::new();
        install(&mut app);
        app.init_resource::<EditorLocale>();
        let world = app.world_mut();
        world.spawn((Panel, Node::default()));
        world.spawn((Heading, Text::default()));
        world.spawn((Content, Node::default()));
        let viewport = world.spawn((Viewport, Node::default(), ScrollPosition::default())).id();
        let slot = world.spawn((Slot(0), Text::default(), Node::default())).id();
        let rows = |label: &str| Arc::new((0..16).map(|index| ProjectSceneTreeRow {
            depth: 0, category: ["节点", "Node"], name: Some(format!("{label} entry {index}")), index,
        }).collect());
        publish(app.world_mut(), rows("old"));
        app.update();
        assert!(app.world().get::<Text>(slot).unwrap().0.ends_with("old entry 0"));
        app.world_mut().get_mut::<ScrollPosition>(viewport).unwrap().y = 81.0;
        app.update();
        publish(app.world_mut(), rows("new"));
        app.update();
        assert_eq!(app.world().get::<Node>(slot).unwrap().display, Display::Flex);
        assert!(app.world().get::<Text>(slot).unwrap().0.ends_with("new entry 2"));
        assert_eq!(app.world().get::<ScrollPosition>(viewport).unwrap().y, 81.0);

        clear(app.world_mut());
        publish(app.world_mut(), rows("next scene"));
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(viewport).unwrap().y, 0.0);
        assert!(app.world().get::<Text>(slot).unwrap().0.ends_with("next scene entry 0"));

        publish(app.world_mut(), Arc::new(Vec::new()));
        app.update();
        assert_eq!(app.world().get::<Node>(slot).unwrap().display, Display::None);
    }
}
