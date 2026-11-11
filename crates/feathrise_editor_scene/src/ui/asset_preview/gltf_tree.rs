//! 中央 GLB 标签的只读左栏结构。沿用场景树页、共享滚轮和连续虚拟列表。

use super::{AssetPreviewState, AssetPreviewWorkspace, DocumentState, types::AssetPreviewControls};
use crate::{
    SceneEditorSystemSet,
    components::SceneEditorSceneContext,
    resources::{SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        continuous_list,
        scene::FONT_PATH,
        sidebars::{EditorNavigatorTab, SceneEditorSidebarState},
        theme as colors,
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
    },
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, Task, futures::check_ready},
    text::FontSourceTemplate,
    ui::RelativeCursorPosition,
};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction, localization::EditorLocale, metrics};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[path = "gltf_tree_data.rs"]
pub(super) mod data;

const MAX_ROW_SLOTS: usize = 256;

#[derive(Component, Clone, Default)]
pub(crate) struct GltfSceneTreePanel;

#[derive(Component, Clone, Default)]
pub(crate) struct EditorSceneTreePanel;

#[derive(Component, Clone, Default)]
pub(crate) struct GltfSceneTreeScroll;

#[derive(Component, Clone, Default)]
pub(super) struct PreviewChrome;

#[derive(Component, Clone, Default)]
pub(super) struct PreviewRoot;

#[derive(Component, Clone, Default)]
pub(super) struct InlineChromeHost;

#[derive(Component, Clone, Default)]
struct TreeChromeHost;

#[derive(Component, Clone, Default)]
struct Header;

#[derive(Component, Clone, Default)]
struct List;

#[derive(Component)]
struct Slot {
    index: usize,
    bound: Option<(u64, usize, EditorLocale)>,
}

#[derive(Clone, PartialEq, Eq)]
struct Source {
    root: PathBuf,
    path: String,
    generation: u64,
}

impl Source {
    fn key(&self) -> (&Path, &str, u64) {
        (&self.root, &self.path, self.generation)
    }
}

type Rows = Result<Vec<data::Row>, String>;

struct Work {
    source: Option<Source>,
    cancel: Arc<AtomicBool>,
    task: Task<Option<Rows>>,
}

#[derive(Resource, Default)]
struct TreeState {
    last_central_glb: Option<u64>,
    selected: Option<Source>,
    complete: Option<(Source, Rows)>,
    work: Option<Work>,
    revision: u64,
}

impl TreeState {
    fn result(&self) -> Option<&Rows> {
        let (source, rows) = self.complete.as_ref()?;
        (Some(source) == self.selected.as_ref()).then_some(rows)
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<TreeState>()
        .add_systems(
            Update,
            select_root_properties
                .after(super::opening::open)
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            update
                .after(super::loading::poll_asset_preview_loading)
                .in_set(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (mount_chrome, present)
                .chain()
                .after(super::mounting::mount_asset_preview_panel)
                .in_set(SceneEditorSystemSet::Presentation),
        );
}

/// 仅固定的中央标签决定树的数据来源。侧栏文件选择和独立资产预览不能抢占它。
pub(crate) fn central_glb(media: &AssetPreviewWorkspace, tab: PresetWorkspaceTab) -> Option<&str> {
    if !media.active(tab) {
        return None;
    }
    let path = media.expanded.as_ref()?.path.as_str();
    is_glb(path).then_some(path)
}

pub(super) fn is_glb(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
}

/// 右栏文件选择独立于中央标签。只有作者正在查看该模型的属性时才显示它的工程动作。
pub(crate) fn owns_properties(
    media: &AssetPreviewWorkspace,
    tab: PresetWorkspaceTab,
    selected_asset: Option<&str>,
) -> bool {
    central_glb(media, tab).is_some_and(|path| selected_asset == Some(path))
}

fn select_root_properties(
    mut editor: ResMut<SceneEditorState>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    media: Res<AssetPreviewWorkspace>,
    workspace: Res<VegetationPresetEditorState>,
    tree: Res<TreeState>,
    layout: Option<Res<SceneEditorLayoutState>>,
    guard: Option<Res<SceneEditorNumericScrubMouseGuard>>,
    hosted: Option<Res<crate::workbench::WorkbenchModel>>,
    root: Query<&EditorButtonClick, (With<PreviewRoot>, Changed<EditorButtonClick>)>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || editor.scene_switch_confirmation.is_some()
        || editor.pending_scene_transition.is_some()
        || !sidebar.scene_tree_visible()
        || layout.is_some_and(|layout| !layout.content_shown())
        || guard.is_some_and(|guard| guard.blocked())
        || hosted.is_some_and(|host| !host.visible || host.modal || host.busy)
        || tree.last_central_glb != media.expanded.as_ref().map(|entry| entry.id)
        || !root.single().is_ok_and(EditorButtonClick::is_clicked)
    {
        return;
    }
    let Some(path) = central_glb(&media, workspace.active_tab) else { return };
    // 文件根只切右侧属性来源。当前画布目标、相机和动作播放不参与这次选择。
    if editor.selected_asset.as_deref() != Some(path) {
        editor.selected_asset = Some(path.to_owned());
    }
    sidebar.reveal_asset_properties();
}

fn update(
    editor: Res<SceneEditorState>,
    documents: Res<DocumentState>,
    workspace: Res<VegetationPresetEditorState>,
    media: Res<AssetPreviewWorkspace>,
    preview: Res<AssetPreviewState>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    layout: Option<Res<SceneEditorLayoutState>>,
    mut state: ResMut<TreeState>,
) {
    let current = central_glb(&media, workspace.active_tab);
    if editor.open && !editor.close_requested {
        let tab = current.and_then(|_| media.expanded.as_ref().map(|entry| entry.id));
        if state.last_central_glb != tab {
            state.last_central_glb = tab;
            if tab.is_some() && sidebar.navigator != EditorNavigatorTab::SceneTree {
                // 仅新激活的模型标签带到场景树。用户随后进入工程找参考时，稳定标签不抢回左栏。
                sidebar.navigator = EditorNavigatorTab::SceneTree;
            }
        }
    }
    let root = documents.asset_root();
    let wanted = current.and_then(|path| {
        if !editor.open
            || editor.close_requested
            || root.as_os_str().is_empty()
            || preview.selected_path.as_deref() != Some(path)
            || preview.external_preview.is_some()
        {
            return None;
        }
        Some((root, path, preview.model.as_ref()?.generation))
    });
    if state.selected.as_ref().map(Source::key) != wanted {
        state.selected = wanted.map(|(root, path, generation)| Source {
            root: root.to_owned(),
            path: path.to_owned(),
            generation,
        });
        state.revision = state.revision.wrapping_add(1);
    }

    let mut stale = None;
    if let Some(work) = &mut state.work {
        if work.source.as_ref().map(Source::key) != wanted {
            work.cancel.store(true, Ordering::Release);
        }
        let Some(result) = check_ready(&mut work.task) else {
            return;
        };
        let work = state.work.take().unwrap();
        if !work.cancel.load(Ordering::Acquire) && work.source == state.selected {
            state.complete = work.source.zip(result);
            state.revision = state.revision.wrapping_add(1);
        } else {
            stale = result;
        }
    }
    if state.complete.as_ref().map(|(source, _)| source.key()) == wanted && stale.is_none() {
        return;
    }
    let previous = state.complete.take();
    // 只有作者实际切到左侧场景树才首次读取。相同 GLB 切去工程或场景目录保留已完成结果。
    let visible =
        sidebar.scene_tree_visible() && layout.as_ref().is_none_or(|layout| layout.content_shown());
    let source = state.selected.clone().filter(|_| visible);
    if source.is_none() && previous.is_none() && stale.is_none() {
        return;
    }
    let request = source.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    // 当前读取完全结束才归还并发槽。旧树和迟到结果也在线程池释放，不在作者帧析构。
    let task = IoTaskPool::get().spawn(async move {
        drop((previous, stale));
        let source = request?;
        let bytes = match data::read_json(&source.root, &source.path, &worker_cancel) {
            Ok(bytes) => bytes,
            Err(error) => return Some(Err(error)),
        };
        // 文件读完后让出 IO worker。完整 JSON 解析和深度优先展开使用计算池。
        Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { data::decode(&bytes, &worker_cancel) })
                .await,
        )
    });
    state.work = Some(Work {
        source,
        cancel,
        task,
    });
}

fn mount_chrome(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    media: Res<AssetPreviewWorkspace>,
    workspace: Res<VegetationPresetEditorState>,
    chrome: Query<(Entity, &ChildOf), With<PreviewChrome>>,
    mut inline: Query<(Entity, &mut Node), With<InlineChromeHost>>,
    tree: Query<Entity, With<TreeChromeHost>>,
    mut root: Query<(&mut Pickable, &UiInteraction, &mut BackgroundColor), With<PreviewRoot>>,
    mut controls: Query<&mut Node, (With<AssetPreviewControls>, Without<InlineChromeHost>)>,
) {
    let (Ok((chrome, parent)), Ok((home, mut inline))) = (chrome.single(), inline.single_mut())
    else { return };
    let to_tree = (editor.open && central_glb(&media, workspace.active_tab).is_some())
        .then(|| tree.single().ok()).flatten();
    let target = to_tree.unwrap_or(home);
    if parent.parent() != target {
        commands.entity(chrome).insert(ChildOf(target));
    }
    // 空宿主也退出布局，中央高度交给视图工具条和画布。
    let display = if to_tree.is_some() { Display::None } else { Display::Flex };
    if inline.display != display {
        inline.display = display;
    }
    if let Ok((mut pickable, interaction, mut background)) = root.single_mut() {
        // 同一标题在小预览仍是普通文案。只有它位于 GLB 左树时才提供文件根点击。
        pickable.set_if_neq(if to_tree.is_some() { Pickable::default() } else { Pickable::IGNORE });
        let color = match (to_tree.is_some(), interaction) {
            (false, _) => Color::NONE,
            (true, UiInteraction::Pressed) => colors::SELECT_BG_PRESSED,
            (true, UiInteraction::Hovered) => colors::SELECT_BG_HOVER,
            (true, UiInteraction::None) => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        background.set_if_neq(BackgroundColor(color));
    }
    let wrap = if to_tree.is_some() { FlexWrap::Wrap } else { FlexWrap::NoWrap };
    for mut control in &mut controls {
        if control.flex_wrap != wrap {
            control.flex_wrap = wrap;
            control.height = if to_tree.is_some() { Val::Auto } else { px(26) };
            control.row_gap = if to_tree.is_some() { px(3) } else { px(0) };
        }
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! {
        GltfSceneTreePanel SceneEditorSceneContext
        Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_direction: FlexDirection::Column, overflow: Overflow::clip() }
        Children [
            GltfSceneTreeScroll RelativeCursorPosition::default() ScrollPosition::default()
            Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0,
                flex_basis: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), overflow: Overflow::scroll_y() }
            Children [
                TreeChromeHost Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column }
            --
                Header Text("") TextLayout { linebreak: LineBreak::AnyCharacter }
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED)
                Node { display: Display::None, min_width: px(0), padding: UiRect::all(px(4)), flex_shrink: 0.0 }
            --
                List Node { width: percent(100), min_width: px(0), height: px(0),
                    position_type: PositionType::Relative, flex_shrink: 0.0 }
            ]
        ]
    }
}

fn localized(locale: EditorLocale, chinese: &'static str, english: &'static str) -> &'static str {
    match locale {
        EditorLocale::Chinese => chinese,
        EditorLocale::English => english,
    }
}

fn label(row: &data::Row, locale: EditorLocale) -> (String, Color) {
    use data::Kind::*;
    let index = row.index;
    let (mut title, count, heading) = match row.kind {
        Scene => (
            format!("{} {index}", localized(locale, "场景", "Scene")),
            None,
            true,
        ),
        Node => (
            format!("{} {index}", localized(locale, "节点", "Node")),
            None,
            false,
        ),
        Joint => (
            format!("{} {index}", localized(locale, "骨骼", "Joint")),
            None,
            false,
        ),
        Mesh(count) => (
            format!("{} {index}", localized(locale, "网格", "Mesh")),
            Some((count, "图元", "primitives")),
            false,
        ),
        Skin(count) => (
            format!("{} {index}", localized(locale, "蒙皮", "Skin")),
            Some((count, "关节", "joints")),
            false,
        ),
        LooseNodes => (
            localized(locale, "未加入场景的节点", "Unassigned nodes").to_owned(),
            None,
            true,
        ),
        LooseAssets => (
            localized(locale, "未挂接的资源", "Unassigned assets").to_owned(),
            None,
            true,
        ),
    };
    if !row.name.is_empty() {
        if matches!(row.kind, Node) {
            title.clone_from(&row.name);
        } else {
            title.push_str(" · ");
            title.push_str(&row.name);
        }
    }
    if let Some((count, zh, en)) = count {
        use std::fmt::Write;
        let _ = write!(title, "  ({count} {})", localized(locale, zh, en));
    }
    (
        title,
        if heading {
            colors::TEXT_PRIMARY
        } else {
            colors::TEXT_VALUE
        },
    )
}

fn bind_slot(
    slot: &mut Slot,
    node: &mut Node,
    text: &mut Text,
    color: &mut TextColor,
    entries: &[data::Row],
    start: usize,
    capacity: usize,
    revision: u64,
    locale: EditorLocale,
) {
    let index = start + slot.index;
    let entry = (slot.index < capacity)
        .then(|| entries.get(index))
        .flatten();
    let Some(row) = entry else {
        if slot.bound.take().is_some() {
            node.display = Display::None;
        }
        return;
    };
    let binding = (revision, index, locale);
    if slot.bound == Some(binding) {
        return;
    }
    slot.bound = Some(binding);
    let (title, row_color) = label(row, locale);
    text.0 = title;
    color.0 = row_color;
    node.display = Display::Flex;
    node.top = px(index as f32 * metrics::ROW_HEIGHT);
    // 深骨链仍有连续缩进，同时给窄栏留出节点名空间。
    let indent = row.depth.min(8) * 9 + row.depth.saturating_sub(8).min(22) * 3;
    node.padding = UiRect {
        left: px(4.0 + indent as f32),
        top: px(4),
        ..default()
    };
}

fn present(
    state: Res<TreeState>,
    sidebar: Res<SceneEditorSidebarState>,
    locale: Option<Res<EditorLocale>>,
    assets: Option<Res<AssetServer>>,
    mut commands: Commands,
    mut reset: Local<Option<(Entity, u64)>>,
    mut headers: Query<(&mut Text, &mut Node), (With<Header>, Without<Slot>)>,
    mut views: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<GltfSceneTreeScroll>,
    >,
    mut lists: Query<
        (
            Entity,
            &mut Node,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Children>,
        ),
        (With<List>, Without<Slot>, Without<Header>),
    >,
    mut slots: Query<(&mut Slot, &mut Node, &mut Text, &mut TextColor), Without<List>>,
) {
    if !sidebar.scene_tree_visible() {
        return;
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let entries = state
        .result()
        .and_then(|result| result.as_ref().ok())
        .map_or(&[][..], Vec::as_slice);
    let heading = match state.result() {
        Some(Err(error)) => Cow::Owned(format!(
            "{} · {error}",
            localized(locale, "无法读取 GLB 结构", "GLB tree unavailable")
        )),
        Some(Ok(rows)) if rows.is_empty() => Cow::Borrowed(localized(
            locale,
            "GLB 没有场景或节点",
            "GLB has no scenes or nodes",
        )),
        Some(Ok(_)) => Cow::Borrowed(""), // 文件名称已经由迁入的原预览标题显示。
        None => Cow::Borrowed(localized(
            locale,
            "正在读取 GLB 结构…",
            "Reading GLB structure…",
        )),
    };
    for (mut header, mut node) in &mut headers {
        if header.as_str() != heading.as_ref() {
            header.0.clear();
            header.0.push_str(&heading);
        }
        let display = if heading.is_empty() { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
    }
    let (
        Ok((viewport, viewport_transform, mut scroll)),
        Ok((list, mut node, computed, transform, children)),
    ) = (views.single_mut(), lists.single_mut())
    else {
        return;
    };
    let new_height = px(continuous_list::content_height(
        entries.len(),
        1,
        metrics::ROW_HEIGHT,
        0.0,
    ));
    if node.height != new_height {
        node.height = new_height;
    }
    let revision = (list, state.revision);
    let start = if *reset != Some(revision) {
        scroll.0 = Vec2::ZERO;
        *reset = Some(revision);
        0
    } else {
        continuous_list::first_visible(
            computed,
            transform,
            viewport,
            viewport_transform,
            &scroll,
            1,
            metrics::ROW_HEIGHT,
            entries.len(),
        )
    };
    let height = viewport.size().y * viewport.inverse_scale_factor();
    let capacity = continuous_list::pool_capacity(height, 1, metrics::ROW_HEIGHT)
        .min(MAX_ROW_SLOTS)
        .min(entries.len());
    for (mut slot, mut node, mut text, mut color) in &mut slots {
        bind_slot(
            &mut slot,
            &mut node,
            &mut text,
            &mut color,
            entries,
            start,
            capacity,
            state.revision,
            locale,
        );
    }
    let count = children.map_or(0, |children| children.len());
    if count >= capacity {
        return;
    }
    let Some(assets) = assets else { return };
    let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
    for index in count..capacity {
        let mut slot = Slot { index, bound: None };
        let mut node = Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            width: percent(100),
            min_width: px(0),
            height: px(metrics::ROW_HEIGHT),
            align_items: AlignItems::Center,
            overflow: Overflow::clip_x(),
            ..default()
        };
        let mut text = Text::new("");
        let mut color = TextColor(colors::TEXT_VALUE);
        bind_slot(
            &mut slot,
            &mut node,
            &mut text,
            &mut color,
            entries,
            start,
            capacity,
            state.revision,
            locale,
        );
        commands.spawn((
            slot,
            node,
            text,
            color,
            TextLayout::no_wrap(),
            Pickable::IGNORE,
            ChildOf(list),
            TextFont {
                font: font.clone().into(),
                font_size: metrics::FONT_BODY.into(),
                ..default()
            },
        ));
    }
}

#[cfg(feature = "validation")]
pub(crate) fn validation_header(world: &mut World) -> Option<(Entity, bool)> {
    let (header, parent) = world.query_filtered::<(Entity, &ChildOf), With<PreviewChrome>>()
        .single(world).ok()?;
    Some((header, world.get::<TreeChromeHost>(parent.parent()).is_some()))
}

#[cfg(feature = "validation")]
pub(crate) fn validation_root(world: &mut World) -> Option<Entity> {
    validation_header(world)?.1.then_some(())?;
    world.query_filtered::<Entity, With<PreviewRoot>>().single(world).ok()
}

#[cfg(feature = "validation")]
pub(crate) fn validation_rows(world: &mut World) -> Option<(String, Vec<String>)> {
    if !world
        .get_resource::<SceneEditorSidebarState>()?
        .scene_tree_visible()
    {
        return None;
    }
    let state = world.get_resource::<TreeState>()?;
    state.result()?.as_ref().ok()?;
    let path = state.selected.as_ref()?.path.clone();
    let revision = state.revision;
    let root = world
        .query_filtered::<(&Node, &ComputedNode), With<GltfSceneTreePanel>>()
        .single(world)
        .ok()?;
    if root.0.display != Display::Flex || root.1.size().y <= 0.0 {
        return None;
    }
    let (view, view_transform) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<GltfSceneTreeScroll>>()
        .single(world)
        .ok()?;
    let viewport_top = view_transform.translation.y - view.size().y * 0.5;
    let viewport_height = view.size().y;
    let inverse = view.inverse_scale_factor();
    let (list, list_transform) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<List>>()
        .single(world)
        .ok()?;
    // 结构行在工具组下方。用实际列表位置核对裁剪，不能把工具组的高度算成可见结构行。
    let list_top = list_transform.translation.y - list.size().y * 0.5;
    let top = (viewport_top - list_top) * inverse;
    let bottom = top + viewport_height * inverse;
    let mut rows: Vec<_> = world
        .query::<(&Slot, &Text, &Node)>()
        .iter(world)
        .filter_map(|(slot, text, node)| {
            let (binding, index, _) = slot.bound?;
            let row_top = index as f32 * metrics::ROW_HEIGHT;
            (binding == revision
                && node.display == Display::Flex
                && row_top < bottom
                && row_top + metrics::ROW_HEIGHT > top)
                .then(|| (index, text.0.clone()))
        })
        .collect();
    rows.sort_unstable_by_key(|(index, _)| *index);
    (!rows.is_empty()).then(|| (path, rows.into_iter().map(|(_, label)| label).collect()))
}
