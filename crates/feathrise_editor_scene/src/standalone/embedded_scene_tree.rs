//! 工程 3D 场景树的只读分支。复用原搜索和滚动行池，行本身没有作者操作。

use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
    text::FontSourceTemplate,
    ui::RelativeCursorPosition,
};
use feathrise_editor_ui::{localization::EditorLocale, metrics};
use std::sync::Arc;

use crate::{
    SceneEditorState, SceneEditorSystemSet,
    authoring::AuthoringRetirement,
    ui::{
        gltf_browser::EditorGltfBrowserState,
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
    revision: u64,
    generation: u64,
    unavailable: bool,
}

impl ProjectSceneTree {
    pub(crate) fn has_objects(&self) -> bool {
        self.rows.as_ref().is_some_and(|rows| !rows.is_empty())
    }
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
        state.revision = state.revision.wrapping_add(1);
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
        state.revision = state.revision.wrapping_add(1);
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

struct Matches {
    revision: u64,
    query: String,
    rows: Arc<Vec<ProjectSceneTreeRow>>,
    indices: Vec<usize>,
}

#[derive(Default)]
struct ListState {
    filtering: Option<Task<Matches>>,
    matches: Option<Matches>,
    generation: u64,
    query: String,
    count: usize,
    start: usize,
}

fn retire_matches(source: &ProjectSceneTree, retirement: &mut AuthoringRetirement, matches: Option<Matches>) {
    // 当前行表仍有来源持有，只有已经换下的行表可能在这里释放最后一个引用。
    if let Some(matches) = matches
        && source.rows.as_ref().is_none_or(|rows| !Arc::ptr_eq(rows, &matches.rows))
    {
        retirement.push((matches, source.retirement.clone()));
    }
}

fn present(
    source: Res<ProjectSceneTree>,
    browser: Option<Res<EditorGltfBrowserState>>,
    locale: Res<EditorLocale>,
    mut retirement: ResMut<AuthoringRetirement>,
    mut state: Local<ListState>,
    panel: Single<&mut Node, (With<Panel>, Without<Viewport>, Without<Content>, Without<Slot>)>,
    heading: Single<&mut Text, (With<Heading>, Without<Slot>)>,
    viewport: Single<(&mut Node, &mut ScrollPosition), (With<Viewport>, Without<Content>, Without<Slot>)>,
    content: Single<&mut Node, (With<Content>, Without<Slot>)>,
    mut slots: Query<(&Slot, &mut Text, &mut Node), Without<Heading>>,
) {
    let state = &mut *state;
    let query = browser.as_ref().map_or("", |browser| browser.object_search.trim());
    let reset = state.generation != source.generation || state.query != query;
    if reset {
        state.generation = source.generation;
        state.query = query.to_owned();
    }
    if reset || source.rows.as_ref().is_none_or(|rows| rows.is_empty()) {
        retire_matches(&source, &mut retirement, state.matches.take());
    }
    let mut filter_adopted = false;
    if let Some(result) = state.filtering.as_mut().and_then(check_ready) {
        state.filtering = None;
        let retired = if result.revision == source.revision && result.query == query {
            filter_adopted = true;
            state.matches.replace(result)
        } else {
            Some(result)
        };
        retire_matches(&source, &mut retirement, retired);
    }
    let matches = state.matches.as_ref();
    if !query.is_empty() && matches.is_none_or(|result| result.revision != source.revision)
        && state.filtering.is_none()
        && let Some(rows) = source.rows.clone().filter(|rows| !rows.is_empty()) {
        let revision = source.revision;
        let query = query.to_owned();
        state.filtering = Some(AsyncComputeTaskPool::get().spawn(async move {
            let indices = find_matches(&rows, &query);
            Matches { revision, query, rows, indices }
        }));
    }
    // 同一场景和关键字刷新时，沿用上一份完整的行表及索引，避免列表闪空或丢失滚动位置。
    let rows = matches.map(|result| &result.rows).or(source.rows.as_ref())
        .map_or(&[][..], |rows| rows.as_slice());
    let count = if query.is_empty() { rows.len() } else { matches.map_or(0, |result| result.indices.len()) };
    let mut panel = panel.into_inner();
    let mut title = heading.into_inner();
    let (mut viewport, mut scroll) = viewport.into_inner();
    let mut content = content.into_inner();
    let layout_changed = reset || scroll.is_changed() || count != state.count || panel.is_added();
    if !layout_changed && !source.is_changed() && !locale.is_changed() && !filter_adopted { return; }
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
        let item = &rows[matches.map_or(index, |result| result.indices[index])];
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

fn find_matches(rows: &[ProjectSceneTreeRow], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    let mut parents = Vec::new();
    let mut included = vec![false; rows.len()];
    for (index, row) in rows.iter().enumerate() {
        parents.truncate(row.depth as usize);
        let matches = row.category.iter().any(|label| label.to_lowercase().contains(&query))
            || row.name.as_ref().is_some_and(|name| name.to_lowercase().contains(&query))
            || query.strip_prefix('#').is_some_and(|id| id.parse::<u32>().ok() == Some(row.index));
        if matches {
            included[index] = true;
            for &parent in parents.iter().rev() {
                if included[parent] { break; }
                included[parent] = true;
            }
        }
        parents.push(index);
    }
    included.into_iter().enumerate().filter_map(|(index, yes)| yes.then_some(index)).collect()
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
    fn scene_tree_search_retains_context_across_siblings() {
        let make_row = |depth, name: &str| ProjectSceneTreeRow {
            depth, category: ["节点", "Node"], name: Some(name.to_owned()), index: 7,
        };
        let rows = [
            make_row(0, "Root"), make_row(1, "Parent"), make_row(2, "Player"),
            make_row(1, "Decoration"), make_row(0, "Other"), make_row(1, "PLAYER Shadow"),
        ];
        assert_eq!(find_matches(&rows, "player"), vec![0, 1, 2, 4, 5]);
        assert!(find_matches(&rows, "camera").is_empty());
    }

    #[test]
    fn scene_tree_search_keeps_scroll_on_refresh_and_rejects_previous_scope() {
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let mut app = App::new();
        install(&mut app);
        app.init_resource::<EditorLocale>().init_resource::<EditorGltfBrowserState>();
        let world = app.world_mut();
        world.resource_mut::<EditorGltfBrowserState>().object_search = "entry".into();
        world.spawn((Panel, Node::default()));
        world.spawn((Heading, Text::default()));
        world.spawn((Content, Node::default()));
        let viewport = world.spawn((Viewport, Node::default(), ScrollPosition::default())).id();
        let slot = world.spawn((Slot(0), Text::default(), Node::default())).id();
        let rows = |label: &str| Arc::new((0..16).map(|index| ProjectSceneTreeRow {
            depth: 0, category: ["节点", "Node"], name: Some(format!("{label} entry {index}")), index,
        }).collect());
        let wait_for_row = |app: &mut App, expected: &str| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !app.world().get::<Text>(slot).unwrap().0.ends_with(expected) {
                assert!(std::time::Instant::now() < deadline, "未发布预期搜索行 {expected}");
                app.update();
                std::thread::yield_now();
            }
        };

        publish(app.world_mut(), rows("old"));
        wait_for_row(&mut app, "old entry 0");
        app.world_mut().get_mut::<ScrollPosition>(viewport).unwrap().y = 81.0;
        app.update();
        publish(app.world_mut(), rows("new"));
        app.update(); // 本帧刚启动新过滤，仍显示旧表的滚动窗口。
        assert_eq!(app.world().get::<Node>(slot).unwrap().display, Display::Flex);
        assert!(app.world().get::<Text>(slot).unwrap().0.ends_with("old entry 2"));
        assert_eq!(app.world().get::<ScrollPosition>(viewport).unwrap().y, 81.0);
        wait_for_row(&mut app, "new entry 2");
        assert_eq!(app.world().get::<ScrollPosition>(viewport).unwrap().y, 81.0);

        publish(app.world_mut(), rows("discarded"));
        app.update();
        clear(app.world_mut());
        publish(app.world_mut(), rows("next scene"));
        app.update();
        assert_eq!(app.world().get::<Node>(slot).unwrap().display, Display::None);
        assert_eq!(app.world().get::<ScrollPosition>(viewport).unwrap().y, 0.0);
        wait_for_row(&mut app, "next scene entry 0");

        publish(app.world_mut(), Arc::new(Vec::new()));
        app.update();
        assert_eq!(app.world().get::<Node>(slot).unwrap().display, Display::None);
    }
}
