//! 归属已核验的文件桌面夹具：真实目录导航、资产筛选、单击/双击、片段和原共享服务的窗口内验收。

use super::{
    FileAction, FileList, FileManagerState, FileResizeHandle, FileRow, Run, browser_rect,
    button, click, drag, evidence, key, move_cursor, path_ready, raw_click, right_click,
    select, visible, window_event,
};
use crate::{
    components::{SceneEditorLayoutButton, SceneEditorPane},
    resources::{SceneEditorLayoutState, SceneEditorState},
    types::{EditorLayoutAction, EditorPane},
    ui::{
        asset_preview::{AssetPreviewWorkspace, AssetSearchState},
        asset_shortcuts::{AssetShortcuts, ShortcutRoot},
        file_manager::browser::{
            BrowserAction, BrowserButton, BrowserChoice, BrowserOverlay, BrowserSessions,
            ClipExpander, RowHint,
        },
        sidebars::{
            EditorNavigatorTab, EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarState,
            SceneEditorSidebarTabButton,
        },
    },
};
use bevy::{
    input::{keyboard::Key, mouse::{MouseScrollUnit, MouseWheel}, touch::TouchPhase},
    input_focus::InputFocus, prelude::*, text::EditableText, window::Ime,
};
use feathrise_editor_core::{AssetBrowserViewMode, EditorSettings};
use feathrise_editor_documents_model::search::components::{SearchAction, SearchBody, SearchField};
use feathrise_editor_documents_model::search::replacement::components::{ReplaceAction, ReplacePanel};
use feathrise_editor_project_browser_model::AssetTypeFilter;
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};

const ROOT: &str = "浏览一致性";
const IMAGE: &str = "浏览一致性/swatch.png";
const MOTIONS: &str = "浏览一致性/motions.glb";
const NOTES: &str = "浏览一致性/notes.md";
const TOKEN: &str = "FILE_MANAGER_SEARCH_TOKEN";
const MISSING_JSON: &str = "config/文件管理验收缺失.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Up,
    Enter,
    DirectoryList,
    Models,
    Images,
    ImagePick,
    ImageOpen,
    Thumbnails,
    ThumbnailWait,
    Animations,
    Clips,
    ClipCapture,
    ClipPick,
    ClipWait,
    All,
    GridCompact,
    GridCompactRead,
    GridScrollDown,
    GridScrollUp,
    GridMaximize,
    GridMaximizeRead,
    GridRestore,
    GridRestoreSize,
    NotePick,
    NoteRead,
    FavoriteMenu,
    FavoritePick,
    FavoriteWait,
    SceneTree,
    HideNavigator,
    VerifyHidden,
    Compact,
    LeaveFolder,
    ParentFolder,
    Favorites,
    FavoritesVerify,
    Recent,
    RecentVerify,
    FavoritesAgain,
    FavoritesLocate,
    FavoritesLocated,
    RestoreSize,
    ChangeView,
    VerifyView,
    Search,
    Query,
    Submit,
    SearchWait,
    SearchCapture,
    ReplacePreview,
    ReplaceWait,
    Missing,
    MissingWait,
    DismissMissing,
    ShowNavigator,
    FixedFiles,
    VerifyReturnedSearch,
    Done,
}

#[derive(Resource)]
struct Assets {
    step: Step,
    since: Instant,
    filter_step: u8,
    typed: u8,
    original_height: Option<f32>,
    fixed_view: Option<AssetBrowserViewMode>,
    grid_size: Option<Vec2>,
    compact_grid: Option<GridMeasure>,
}

#[derive(Clone, Copy)]
struct GridMeasure {
    window_width: f32,
    card: Vec2,
    first: Vec2,
    columns: usize,
    visible: usize,
}

impl GridMeasure {
    fn proof(self) -> serde_json::Value {
        json!({"window_width":self.window_width,"card":[self.card.x,self.card.y],
            "first_card_offset":[self.first.x,self.first.y],"first_row_columns":self.columns,"visible_cards":self.visible})
    }
}

pub(super) fn advance(world: &mut World, run: &mut Run, window: Entity) -> Result<bool, String> {
    if !world.contains_resource::<Assets>() {
        world.insert_resource(Assets {
            step: Step::Up,
            since: Instant::now(),
            filter_step: 0,
            typed: 0,
            original_height: None,
            fixed_view: None,
            grid_size: None,
            compact_grid: None,
        });
        run.pointer = None;
    }
    world.resource_scope(|world, mut state: Mut<Assets>| {
        if state.since.elapsed() > Duration::from_secs(40) { return Err(format!("文件窗资产一致性阶段超时：{:?}，目录={:?}，状态={}",
            state.step, names(world, run.source_window.unwrap_or(0)), world.resource::<SceneEditorState>().status)); }
        let id = run.source_window.ok_or("资产一致性验收缺少已创建的源窗口")?;
        let assets = run.project.join("assets");
        let fixture = assets.join(ROOT);
        let next = match state.step {
            Step::Up => {
                if path_ready(world, id, &assets) { Some(Step::Enter) }
                else { button(world, run, window, Some(id), FileAction::Up); None }
            }
            Step::Enter => {
                if path_ready(world, id, &fixture) { Some(Step::DirectoryList) }
                else { click_file(world, run, window, id, &fixture); None }
            }
            Step::DirectoryList => {
                if world.resource::<BrowserSessions>().validation_view(id) != Some(AssetBrowserViewMode::List) {
                    tool(world, run, window, id, BrowserAction::View);
                    return Ok(false);
                }
                if !directory_labels(world, id, &fixture)? { return Ok(false); }
                Some(Step::Models)
            }
            Step::Models => {
                if !filter(world, run, &mut state.filter_step, window, id, AssetTypeFilter::Model) { return Ok(false); }
                if names(world, id).as_deref() != Some(&["roles".to_owned()]) { return Ok(false); }
                run.steps.insert("floating_asset_model_filter".into(), json!({"folder":fixture,"entries":["roles"],
                    "classification":"shared DirectoryWorker GLB metadata; animation-only motions.glb excluded"}));
                Some(Step::Images)
            }
            Step::Images => {
                if !filter(world, run, &mut state.filter_step, window, id, AssetTypeFilter::Image) { return Ok(false); }
                if names(world, id).as_deref() != Some(&["swatch.png".to_owned()]) { return Ok(false); }
                run.steps.insert("floating_asset_image_filter".into(), json!({"entries":["swatch.png"]}));
                Some(Step::ImagePick)
            }
            Step::ImagePick => {
                if !select(world, run, window, id, &assets.join(IMAGE)) { return Ok(false); }
                if world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(IMAGE)
                    || world.resource::<SceneEditorSidebarState>().right != EditorRightSidebarTab::Assets { return Ok(false); }
                run.steps.insert("floating_asset_single_click_properties".into(), json!({"path":IMAGE,"right":"Assets"}));
                Some(Step::ImageOpen)
            }
            Step::ImageOpen => {
                if central(world, IMAGE) {
                    run.steps.insert("floating_asset_double_click_central".into(), json!({"path":IMAGE,"source":"原OpenManagedFile/媒体中央标签"}));
                    Some(Step::Thumbnails)
                } else { click_file(world, run, window, id, &assets.join(IMAGE)); None }
            }
            Step::Thumbnails => {
                if world.resource::<BrowserSessions>().validation_view(id) == Some(AssetBrowserViewMode::Thumbnails) { Some(Step::ThumbnailWait) }
                else { tool(world, run, window, id, BrowserAction::View); None }
            }
            Step::ThumbnailWait => {
                let Some(row) = file_row(world, id, &assets.join(IMAGE)) else { return Ok(false); };
                let Some((handle, bytes, size)) = crate::ui::asset_thumbnails::validation_cached_thumbnail(world, IMAGE) else { return Ok(false); };
                let shown = world.get::<Children>(row).is_some_and(|children| children.iter().any(|child|
                    world.get::<ImageNode>(child).is_some_and(|image| image.image == handle)
                    && world.get::<ComputedNode>(child).is_some_and(|node| node.size().min_element() >= 70.)));
                if !shown || bytes == 0 || size != UVec2::splat(256) { return Ok(false); }
                let Some(mut proof) = evidence::capture(world, &mut run.capture, &run.work.join("files-assets-thumbnails.png"))? else { return Ok(false); };
                proof["asset"] = json!(IMAGE); proof["source_bytes"] = json!(bytes); proof["texture"] = json!([size.x,size.y]);
                proof["cache"] = json!("原asset_thumbnails唯一缓存与GPU烘焙完成回执");
                run.screenshots.insert("files-assets-thumbnails.png".into(), proof);
                run.steps.insert("floating_shared_thumbnail_ready".into(), json!({"asset":IMAGE,"texture":[256,256],"actual_row_handle":true}));
                Some(Step::Animations)
            }
            Step::Animations => {
                if !filter(world, run, &mut state.filter_step, window, id, AssetTypeFilter::Animation) { return Ok(false); }
                if names(world, id).as_deref() != Some(&["motions.glb".to_owned()]) { return Ok(false); }
                Some(Step::Clips)
            }
            Step::Clips => {
                let Some(row) = file_row(world, id, &assets.join(MOTIONS)) else { return Ok(false); };
                let arrow = visible(world, |arrow: &ClipExpander| arrow.row == row);
                if !click(world, &mut run.click, window, arrow) { return Ok(false); }
                Some(Step::ClipCapture)
            }
            Step::ClipCapture => {
                if visible(world, |choice: &BrowserChoice| choice.window == id && choice.clip == Some(0)).is_none()
                    || visible(world, |choice: &BrowserChoice| choice.window == id && choice.clip == Some(1)).is_none() { return Ok(false); }
                let Some(mut proof) = evidence::capture(world, &mut run.capture, &run.work.join("files-assets-clips.png"))? else { return Ok(false); };
                proof["asset"] = json!(MOTIONS); proof["original_clip_indices"] = json!([0,1]);
                run.screenshots.insert("files-assets-clips.png".into(), proof);
                Some(Step::ClipPick)
            }
            Step::ClipPick => {
                let choice = visible(world, |choice: &BrowserChoice| choice.window == id && choice.clip == Some(1));
                if !click(world, &mut run.click, window, choice) { return Ok(false); }
                Some(Step::ClipWait)
            }
            Step::ClipWait => {
                let Some((path, index, _, playing)) = crate::ui::asset_preview::animation::active_project_clip(world) else { return Ok(false); };
                if !path.ends_with(MOTIONS) || index != 1 || !central(world, MOTIONS) { return Ok(false); }
                run.steps.insert("floating_original_animation_clip".into(), json!({"asset":MOTIONS,"clip_index":index,"playing":playing,
                    "input":"浮窗GLB片段箭头→真实第二片段按钮","left_sidebar_dependency":false}));
                Some(Step::All)
            }
            Step::All => {
                if !filter(world, run, &mut state.filter_step, window, id, AssetTypeFilter::All) { return Ok(false); }
                if !names(world, id).is_some_and(|names| names.iter().any(|name| name == "notes.md")) { return Ok(false); }
                Some(Step::GridCompact)
            }
            Step::GridCompact => {
                let Some(rect) = browser_rect(world, id) else { return Ok(false); };
                let original = *state.grid_size.get_or_insert(rect.size());
                if !resize_window(world, run, window, id, Vec2::new(480., original.y)) { return Ok(false); }
                Some(Step::GridCompactRead)
            }
            Step::GridCompactRead => {
                let Some(grid) = measure_grid(world, id)? else { return Ok(false); };
                if !directory_labels(world, id, &fixture)? { return Ok(false); }
                if grid.window_width >= state.grid_size.unwrap().x - 20. || grid.visible >= 7 || grid.columns < 2 {
                    return Err(format!("缩窄后的网格未形成可滚动夹具：{}", grid.proof()));
                }
                let name = "files-assets-grid-compact.png";
                let Some(mut proof) = evidence::capture(world, &mut run.capture, &run.work.join(name))? else { return Ok(false); };
                proof["grid"] = grid.proof();
                run.screenshots.insert(name.into(), proof);
                state.compact_grid = Some(grid);
                Some(Step::GridScrollDown)
            }
            Step::GridScrollDown => {
                let columns = state.compact_grid.unwrap().columns;
                if grid_first(world, id) != Some(columns) {
                    if state.typed == 0 && wheel_grid(world, run, window, id, -1.) { state.typed = 1; }
                    return Ok(false);
                }
                if visible(world, |row: &FileRow| row.window == id && row.slot == 0 && row.index == Some(columns)).is_none() {
                    return Ok(false);
                }
                Some(Step::GridScrollUp)
            }
            Step::GridScrollUp => {
                if grid_first(world, id) != Some(0) {
                    if state.typed == 0 && wheel_grid(world, run, window, id, 1.) { state.typed = 1; }
                    return Ok(false);
                }
                run.pointer = None;
                Some(Step::GridMaximize)
            }
            Step::GridMaximize => {
                if !button(world, run, window, Some(id), FileAction::Maximize) { return Ok(false); }
                Some(Step::GridMaximizeRead)
            }
            Step::GridMaximizeRead => {
                if !world.resource::<FileManagerState>().windows.iter().any(|browser| browser.id == id && browser.maximized) { return Ok(false); }
                let Some(grid) = measure_grid(world, id)? else { return Ok(false); };
                if !directory_labels(world, id, &fixture)? { return Ok(false); }
                let compact = state.compact_grid.unwrap();
                if grid.columns <= compact.columns || grid.visible <= compact.visible
                    || grid.card.distance(compact.card) > 1.5 || grid.first.distance(compact.first) > 1.5 {
                    return Err(format!("最大化必须从同一起点增加列数、保持卡片宽高：缩窄 {}，最大化 {}", compact.proof(), grid.proof()));
                }
                let name = "files-assets-grid-maximized.png";
                let Some(mut proof) = evidence::capture(world, &mut run.capture, &run.work.join(name))? else { return Ok(false); };
                proof["grid"] = grid.proof();
                run.screenshots.insert(name.into(), proof);
                run.steps.insert("floating_thumbnail_grid".into(), json!({"compact":compact.proof(),"maximized":grid.proof(),
                    "directory_labels":["empty","roles"],"directory_list_and_thumbnail_icons":true,
                    "one_wheel_step_entries":compact.columns,"verified":true}));
                Some(Step::GridRestore)
            }
            Step::GridRestore => {
                if !button(world, run, window, Some(id), FileAction::Maximize) { return Ok(false); }
                Some(Step::GridRestoreSize)
            }
            Step::GridRestoreSize => {
                if world.resource::<FileManagerState>().windows.iter().any(|browser| browser.id == id && browser.maximized) { return Ok(false); }
                if !resize_window(world, run, window, id, state.grid_size.unwrap()) { return Ok(false); }
                Some(Step::NotePick)
            }
            Step::NotePick => {
                if !select(world, run, window, id, &assets.join(NOTES)) { return Ok(false); }
                if world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(NOTES)
                    || world.resource::<SceneEditorSidebarState>().right != EditorRightSidebarTab::Assets { return Ok(false); }
                if !central(world, MOTIONS) { return Err("单击Markdown属性时意外切换了固定的中央动作标签".into()); }
                run.steps.insert("floating_markdown_single_click_properties".into(), json!({"path":NOTES,"right":"Assets","central_remains":MOTIONS}));
                Some(Step::NoteRead)
            }
            Step::NoteRead => {
                let Some((source, dirty)) = crate::ui::asset_preview::documents::validation::loaded_text(world, NOTES) else {
                    // 与固定栏相同，活动媒体中央标签固定其来源；只有显式双击文档才切换中央并读取全文。
                    click_file(world, run, window, id, &assets.join(NOTES)); return Ok(false);
                };
                if source.trim() != TOKEN || dirty { return Err("浮窗Markdown没有由原文档读取器采用实际夹具正文".into()); }
                run.steps.insert("floating_markdown_loaded".into(), json!({"path":NOTES,"source":source,"dirty":dirty,"input":"单击属性→真实双击中央→原文档读取"}));
                Some(Step::FavoriteMenu)
            }
            Step::FavoriteMenu => {
                let Some(row) = file_row(world, id, &assets.join(NOTES)) else { return Ok(false); };
                let Some(position) = world.get::<UiGlobalTransform>(row).map(|transform| transform.translation.as_dvec2()) else { return Ok(false); };
                run.pointer = Some(position);
                if !right_click(world, &mut run.right, window, position) { return Ok(false); }
                if !world.resource::<FileManagerState>().context_menu_open() { return Err("浮窗资产右键没有打开真实文件菜单".into()); }
                Some(Step::FavoritePick)
            }
            Step::FavoritePick => {
                let item = text_button(world, "收藏／取消收藏");
                // 与通用右键命令一致，释放帧仍停留菜单项，不能回到打开菜单时冻结的文件行。
                run.pointer = item.and_then(|item| world.get::<UiGlobalTransform>(item)).map(|transform| transform.translation.as_dvec2());
                if !click(world, &mut run.click, window, item) { return Ok(false); }
                Some(Step::FavoriteWait)
            }
            Step::FavoriteWait => {
                if !world.resource::<AssetShortcuts>().is_favorite(NOTES, false) { return Ok(false); }
                run.steps.insert("floating_context_favorite".into(), json!({"path":NOTES,"source":"原SetAssetFavorite服务"}));
                run.pointer = None;
                Some(Step::SceneTree)
            }
            Step::SceneTree => {
                let tab = visible(world, |tab: &SceneEditorSidebarTabButton| tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree));
                if !click(world, &mut run.click, window, tab) { return Ok(false); }
                Some(Step::HideNavigator)
            }
            Step::HideNavigator => {
                let toggle = visible(world, |button: &SceneEditorLayoutButton| button.0 == EditorLayoutAction::Content);
                if !click(world, &mut run.click, window, toggle) { return Ok(false); }
                Some(Step::VerifyHidden)
            }
            Step::VerifyHidden => {
                let fixed_view = world.resource::<EditorSettings>().asset_browser_view_mode;
                verify_fixed_sidebar(world, fixed_view)?;
                state.fixed_view = Some(fixed_view);
                Some(Step::Compact)
            }
            Step::Compact => {
                let Some(rect) = browser_rect(world, id) else { return Ok(false); };
                state.original_height.get_or_insert(rect.height());
                if !resize_height(world, run, window, id, 240.) { return Ok(false); }
                Some(Step::LeaveFolder)
            }
            Step::LeaveFolder => {
                if world.resource::<SceneEditorSidebarState>().navigator != EditorNavigatorTab::SceneTree { return Ok(false); }
                // 先让标签落到首屏以外，否则仅检查选择无法证明滚动定位。
                if file_row(world, id, &assets.join(NOTES)).is_some() { return Err("收藏定位夹具缩小窗口后仍将文档显示在首屏".into()); }
                if !button(world, run, window, Some(id), FileAction::Up) { return Ok(false); }
                Some(Step::ParentFolder)
            }
            Step::ParentFolder => {
                // 必须先实际离开目标目录，才能证明收藏定位重新扫描并选中文件。
                if !path_ready(world, id, &assets) { return Ok(false); }
                Some(Step::Favorites)
            }
            Step::Favorites => {
                if !tool(world, run, window, id, BrowserAction::Favorites) { return Ok(false); }
                Some(Step::FavoritesVerify)
            }
            Step::FavoritesVerify => {
                if !shortcut_heading(world, id, "收藏") || shortcut_button(world, id, NOTES).is_none() { return Ok(false); }
                verify_fixed_sidebar(world, state.fixed_view.unwrap())?;
                Some(Step::Recent)
            }
            Step::Recent => {
                if !tool(world, run, window, id, BrowserAction::Recent) { return Ok(false); }
                Some(Step::RecentVerify)
            }
            Step::RecentVerify => {
                if !shortcut_heading(world, id, "最近访问") || shortcut_heading(world, id, "收藏")
                    || shortcut_button(world, id, NOTES).is_none() || shortcut_button(world, id, ROOT).is_none() { return Ok(false); }
                verify_fixed_sidebar(world, state.fixed_view.unwrap())?;
                run.steps.insert("floating_recent_service".into(), json!({"path":NOTES,"folder":ROOT,"left":"SceneTree","shared_records":true,
                    "input":"收藏打开时直接点击最近访问"}));
                Some(Step::FavoritesAgain)
            }
            Step::FavoritesAgain => {
                if !tool(world, run, window, id, BrowserAction::Favorites) { return Ok(false); }
                Some(Step::FavoritesLocate)
            }
            Step::FavoritesLocate => {
                if !path_ready(world, id, &assets) || !shortcut_heading(world, id, "收藏")
                    || shortcut_heading(world, id, "最近访问") { return Ok(false); }
                let item = shortcut_button(world, id, NOTES);
                if !click(world, &mut run.click, window, item) { return Ok(false); }
                Some(Step::FavoritesLocated)
            }
            Step::FavoritesLocated => {
                let target = assets.join(NOTES);
                let selected = world.resource::<FileManagerState>().windows.iter()
                    .any(|browser| browser.id == id && browser.first > 0 && browser.selection.len() == 1 && browser.selection.contains(&target));
                if !path_ready(world, id, &fixture) || !selected || file_row(world, id, &target).is_none() { return Ok(false); }
                if world.resource::<SceneEditorSidebarState>().navigator != EditorNavigatorTab::SceneTree {
                    return Err("活动窗收藏定位意外切换了固定左栏".into());
                }
                run.steps.insert("floating_favorites_locate_without_sidebar".into(), json!({"path":NOTES,"left":"SceneTree",
                    "from":assets,"to":fixture,"selected":target,"scrolled_into_view":true,"service_parent":"当前浮动文件窗口"}));
                Some(Step::RestoreSize)
            }
            Step::RestoreSize => {
                let height = state.original_height.ok_or("缺少缩放前的窗口高度")?;
                if !resize_height(world, run, window, id, height) { return Ok(false); }
                Some(Step::ChangeView)
            }
            Step::ChangeView => {
                if !tool(world, run, window, id, BrowserAction::View) { return Ok(false); }
                Some(Step::VerifyView)
            }
            Step::VerifyView => {
                if world.resource::<BrowserSessions>().validation_view(id) != Some(AssetBrowserViewMode::List) { return Ok(false); }
                verify_fixed_sidebar(world, state.fixed_view.unwrap())?;
                Some(Step::Search)
            }
            Step::Search => {
                if !tool(world, run, window, id, BrowserAction::Search) { return Ok(false); }
                Some(Step::Query)
            }
            Step::Query => {
                if !inside::<SearchBody>(world, id) { return Ok(false); }
                verify_fixed_sidebar(world, state.fixed_view.unwrap())?;
                if !input_text(world, run, &mut state.typed, window, TOKEN) { return Ok(false); }
                Some(Step::Submit)
            }
            Step::Submit => {
                let submit = visible(world, |action: &SearchAction| *action == SearchAction::Submit);
                if !click(world, &mut run.click, window, submit) { return Ok(false); }
                Some(Step::SearchWait)
            }
            Step::SearchWait => {
                let Some((hits, paths)) = world.resource::<AssetSearchState>().validation_result() else { return Ok(false); };
                if hits == 0 || !paths.iter().any(|path| path == NOTES) { return Ok(false); }
                if !inside::<SearchBody>(world, id) || world.resource::<SceneEditorSidebarState>().navigator != EditorNavigatorTab::SceneTree { return Err("搜索结果依赖固定左栏资产页或离开了浮动文件窗口".into()); }
                run.steps.insert("floating_search_without_sidebar".into(), json!({"query":TOKEN,"hits":hits,"files":paths,"left":"SceneTree","service_parent":"当前浮动文件窗口"}));
                Some(Step::SearchCapture)
            }
            Step::SearchCapture => {
                let Some(proof) = evidence::capture(world, &mut run.capture, &run.work.join("files-assets-services.png"))? else { return Ok(false); };
                run.screenshots.insert("files-assets-services.png".into(), proof);
                Some(Step::ReplacePreview)
            }
            Step::ReplacePreview => {
                let replace = visible(world, |action: &ReplaceAction| *action == ReplaceAction::Open);
                if !click(world, &mut run.click, window, replace) { return Ok(false); }
                Some(Step::ReplaceWait)
            }
            Step::ReplaceWait => {
                if visible(world, |_: &ReplacePanel| true).is_none() { return Ok(false); }
                if world.resource::<SceneEditorSidebarState>().navigator != EditorNavigatorTab::SceneTree {
                    return Err("浮窗打开替换预览时改动了固定左栏".into());
                }
                run.steps.insert("floating_search_replace_preview".into(), json!({"visible_central_preview":true,"left":"SceneTree"}));
                Some(Step::Missing)
            }
            Step::Missing => {
                if !tool(world, run, window, id, BrowserAction::Missing) { return Ok(false); }
                Some(Step::MissingWait)
            }
            Step::MissingWait => {
                let Some((hits, paths)) = world.resource::<AssetSearchState>().validation_result() else { return Ok(false); };
                if hits == 0 || !paths.iter().any(|path| path == MISSING_JSON) { return Ok(false); }
                if !inside::<SearchBody>(world, id) { return Err("缺失检查结果没有留在活动窗的共享表单".into()); }
                verify_fixed_sidebar(world, state.fixed_view.unwrap())?;
                run.steps.insert("floating_missing_assets".into(), json!({"hits":hits,"files":paths,"fixture":MISSING_JSON}));
                Some(Step::DismissMissing)
            }
            Step::DismissMissing => {
                if !tool(world, run, window, id, BrowserAction::Missing) { return Ok(false); }
                Some(Step::ShowNavigator)
            }
            Step::ShowNavigator => {
                if inside::<SearchBody>(world, id) || world.resource::<AssetSearchState>().is_open() {
                    return Err("再次点击浮窗缺失检查后，共享表单仍打开".into());
                }
                verify_fixed_sidebar(world, state.fixed_view.unwrap())?;
                let toggle = visible(world, |button: &SceneEditorLayoutButton| button.0 == EditorLayoutAction::Content);
                if !click(world, &mut run.click, window, toggle) { return Ok(false); }
                Some(Step::FixedFiles)
            }
            Step::FixedFiles => {
                let tab = visible(world, |tab: &SceneEditorSidebarTabButton| tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets));
                if !click(world, &mut run.click, window, tab) { return Ok(false); }
                Some(Step::VerifyReturnedSearch)
            }
            Step::VerifyReturnedSearch => {
                if world.resource::<SceneEditorSidebarState>().navigator != EditorNavigatorTab::Assets { return Ok(false); }
                if visible(world, |_: &SearchBody| true).is_some() {
                    return Err("重新打开固定文件栏时显示了已经收起的浮窗搜索表单".into());
                }
                run.steps.insert("floating_toolbar_sidebar_isolation".into(), json!({
                    "tools":["search","missing_assets","favorites","recent","view"],"fixed_sidebar_hidden":true,
                    "fixed_navigator":"SceneTree","fixed_view":format!("{:?}",state.fixed_view.unwrap()),"returned_search_closed":true
                }));
                Some(Step::Done)
            }
            Step::Done => {
                run.steps.insert("floating_asset_capabilities_verified".into(), json!({"files":[IMAGE,MOTIONS,NOTES],"shared_directory_backend":true,
                    "properties":true,"central_preview":true,"original_clip":1,"shared_thumbnail":true,"favorites":true,"recent":true,
                    "content_search":true,"replace_preview":true,"missing_assets":true}));
                return Ok(true);
            }
        };
        if let Some(next) = next { state.step = next; state.since = Instant::now(); state.typed = 0; state.filter_step = 0; }
        Ok(false)
    })
}

fn verify_fixed_sidebar(world: &mut World, view: AssetBrowserViewMode) -> Result<(), String> {
    let layout = world.resource::<SceneEditorLayoutState>();
    if layout.content_visible || layout.content_shown()
        || world.resource::<SceneEditorSidebarState>().navigator != EditorNavigatorTab::SceneTree
        || world.resource::<EditorSettings>().asset_browser_view_mode != view
        || visible(world, |pane: &SceneEditorPane| pane.0 == EditorPane::Content).is_some()
    {
        return Err("活动文件窗工具改变了固定左栏显隐、页面或列表视图".into());
    }
    Ok(())
}

fn names(world: &World, id: u64) -> Option<Vec<String>> {
    let window = world
        .get_resource::<FileManagerState>()?
        .windows
        .iter()
        .find(|window| window.id == id && !window.loading)?;
    Some(
        window
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect(),
    )
}
fn central(world: &World, path: &str) -> bool {
    world
        .get_resource::<AssetPreviewWorkspace>()
        .is_some_and(|media| media.expanded.as_ref().is_some_and(|tab| tab.path == path))
}
fn file_row(world: &mut World, id: u64, path: &Path) -> Option<Entity> {
    visible(world, |row: &FileRow| {
        row.window == id && row.path.as_deref() == Some(path)
    })
}
fn click_file(world: &mut World, run: &mut Run, window: Entity, id: u64, path: &Path) -> bool {
    let row = file_row(world, id, path);
    click(world, &mut run.click, window, row)
}
fn resize_height(world: &mut World, run: &mut Run, window: Entity, id: u64, height: f32) -> bool {
    let Some(rect) = browser_rect(world, id) else { return false; };
    resize_window(world, run, window, id, Vec2::new(rect.width(), height))
}

fn resize_window(world: &mut World, run: &mut Run, window: Entity, id: u64, size: Vec2) -> bool {
    let handle = visible(world, |handle: &FileResizeHandle| handle.window == id && handle.edges == 10);
    let Some(handle) = handle else { return false; };
    let Some(rect) = browser_rect(world, id) else { return false; };
    let Some(transform) = world.get::<UiGlobalTransform>(handle) else { return false; };
    let Some(node) = world.get::<ComputedNode>(handle) else { return false; };
    let offset = (size - rect.size()) / node.inverse_scale_factor;
    let destination = (transform.translation + offset).as_dvec2();
    drag(world, &mut run.drag, window, Some(handle), destination)
}

fn directory_labels(world: &mut World, id: u64, fixture: &Path) -> Result<bool, String> {
    for name in ["empty", "roles"] {
        let Some(entity) = file_row(world, id, &fixture.join(name)) else { return Ok(false); };
        let Some(children) = world.get::<Children>(entity) else { return Ok(false); };
        if !children.iter().any(|child| world.get::<ImageNode>(child)
            .and_then(|image| image.image.path()).is_some_and(|path| path.path().ends_with("tree_folder.png"))) {
            return Ok(false);
        }
        let Some(hint) = children.iter().find(|child| world.get::<RowHint>(*child).is_some()) else { return Ok(false); };
        if world.get::<Node>(hint).is_none_or(|node| node.display != Display::None) {
            return Err(format!("目录卡 {name} 仍显示多余的类型行"));
        }
        let labels: Vec<_> = children.iter().filter_map(|child| {
            world.get::<Text>(child).filter(|_| world.get::<Node>(child).is_some_and(|node| node.display != Display::None))
                .map(|text| text.as_str())
        }).collect();
        if labels.as_slice() != [name] { return Err(format!("目录条目必须只显示名称 {name}，实际 {labels:?}")); }
    }
    Ok(true)
}

fn measure_grid(world: &mut World, id: u64) -> Result<Option<GridMeasure>, String> {
    let Some(rect) = browser_rect(world, id) else { return Ok(None); };
    let Some(list) = visible(world, |list: &FileList| list.window == id) else { return Ok(None); };
    let list_node = world.get::<ComputedNode>(list).unwrap();
    let scale = list_node.inverse_scale_factor;
    let list_origin = (world.get::<UiGlobalTransform>(list).unwrap().translation - list_node.size() * 0.5) * scale;
    let mut cards: Vec<_> = world.query::<(&FileRow, &Node, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>().iter(world)
        .filter(|(row, node, computed, _, visibility)| row.window == id && row.index.is_some()
            && node.display == Display::Flex && computed.size().min_element() > 1. && visibility.get())
        .map(|(row, _, node, transform, _)| (row.slot, node.size() * scale, (transform.translation - node.size() * 0.5) * scale - list_origin))
        .collect();
    cards.sort_by_key(|card| card.0);
    let Some(&(0, card, first)) = cards.first() else { return Ok(None); };
    let columns = cards.iter().take_while(|(_, _, offset)| (offset.y - first.y).abs() <= 1.5).count();
    if cards.iter().enumerate().any(|(slot, &(actual, size, offset))| {
        let column = (slot % columns) as f32;
        let row = (slot / columns) as f32;
        actual != slot || size.distance(card) > 1.5 || offset.x < first.x - 1.5
            || offset.y < first.y + row * card.y - 1.5 || offset.x < first.x + column * card.x - 1.5
    }) || first.x < 0. || first.x > 16. || first.y.abs() > 1.5 {
        return Err(format!("文件卡片应从正文左上按行排列：{cards:?}"));
    }
    Ok(Some(GridMeasure { window_width: rect.width(), card, first, columns, visible: cards.len() }))
}

fn grid_first(world: &World, id: u64) -> Option<usize> {
    world.resource::<FileManagerState>().windows.iter().find(|browser| browser.id == id).map(|browser| browser.first)
}

fn wheel_grid(world: &mut World, run: &mut Run, window: Entity, id: u64, y: f32) -> bool {
    let Some(list) = visible(world, |list: &FileList| list.window == id) else { return false; };
    let Some(transform) = world.get::<UiGlobalTransform>(list) else { return false; };
    let position = transform.translation.as_dvec2();
    run.pointer = Some(position);
    move_cursor(world, window, position);
    window_event(world, MouseWheel { unit: MouseScrollUnit::Line, x: 0., y, window, phase: TouchPhase::Moved });
    true
}
fn tool(world: &mut World, run: &mut Run, window: Entity, id: u64, action: BrowserAction) -> bool {
    let target = visible(world, |button: &BrowserButton| {
        button.window == id && button.action == action
    });
    click(world, &mut run.click, window, target)
}
fn filter(
    world: &mut World,
    run: &mut Run,
    step: &mut u8,
    window: Entity,
    id: u64,
    kind: AssetTypeFilter,
) -> bool {
    if *step == 0 {
        if tool(world, run, window, id, BrowserAction::Filter) {
            *step = 1;
        }
        return false;
    }
    if *step == 1 {
        let choice = visible(world, |choice: &BrowserChoice| {
            choice.window == id && choice.filter == Some(kind)
        });
        if click(world, &mut run.click, window, choice) {
            *step = 2;
        }
        return false;
    }
    world
        .resource::<FileManagerState>()
        .windows
        .iter()
        .any(|window| window.id == id && !window.loading)
}
fn text_button(world: &mut World, value: &str) -> Option<Entity> {
    let label = visible(world, |text: &Text| text.as_str() == value)?;
    world.get::<ChildOf>(label).map(ChildOf::parent)
}

fn shortcut_heading(world: &mut World, id: u64, title: &str) -> bool {
    world.query::<(Entity, &Text, &ComputedNode, &InheritedVisibility)>().iter(world)
        .any(|(entity, text, node, visible)| text.as_str() == title && visible.get()
            && node.size().min_element() > 1. && mounted_in_service(world, id, entity))
}

fn shortcut_button(world: &mut World, id: u64, path: &str) -> Option<Entity> {
    let candidates: Vec<_> = world
        .query::<(Entity, &Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, text, node, visible)| {
            text.as_str().ends_with(path) && visible.get() && node.size().min_element() > 1.
        })
        .map(|(entity, ..)| entity)
        .collect();
    candidates.into_iter().find_map(|label| {
        let button = world.get::<ChildOf>(label)?.parent();
        let mut current = Some(button);
        for _ in 0..16 {
            let entity = current?;
            if world.get::<ShortcutRoot>(entity).is_some() {
                return mounted_in_service(world, id, entity).then_some(button);
            }
            current = world.get::<ChildOf>(entity).map(ChildOf::parent);
        }
        None
    })
}
fn inside<C: Component>(world: &mut World, id: u64) -> bool {
    world
        .query_filtered::<Entity, With<C>>()
        .iter(world)
        .any(|panel| mounted_in_service(world, id, panel))
}

fn mounted_in_service(world: &World, id: u64, entity: Entity) -> bool {
    let mut current = Some(entity);
    for _ in 0..16 {
        let Some(entity) = current else { break; };
        if world.get::<BrowserOverlay>(entity).is_some_and(|overlay| overlay.window == id && overlay.service) {
            return true;
        }
        current = world.get::<ChildOf>(entity).map(ChildOf::parent);
    }
    false
}
fn input_text(
    world: &mut World,
    run: &mut Run,
    typed: &mut u8,
    window: Entity,
    value: &str,
) -> bool {
    let Some(input) = visible(world, |field: &SearchField| *field == SearchField::Query) else {
        return false;
    };
    if run.click.is_some_and(|(entity, _)| entity == input) {
        raw_click(world, &mut run.click, window, input);
        return false;
    }
    if world.resource::<InputFocus>().get() != Some(input) {
        raw_click(world, &mut run.click, window, input);
        return false;
    }
    if *typed == 0 {
        key(world, window, KeyCode::End, Key::End);
        let current = world
            .get::<EditableText>(input)
            .map(|text| text.value().to_string())
            .unwrap_or_default();
        for _ in current.chars() {
            key(world, window, KeyCode::Backspace, Key::Backspace);
        }
        *typed = 1;
        return false;
    }
    if *typed == 1 {
        if world
            .get::<EditableText>(input)
            .is_none_or(|text| !text.value().to_string().is_empty())
        {
            return false;
        }
        super::window_event(
            world,
            Ime::Commit {
                window,
                value: value.into(),
            },
        );
        *typed = 2;
        return false;
    }
    world
        .get::<EditableText>(input)
        .is_some_and(|text| text.value().to_string() == value)
}
