//! 目录筛选与动作选择真窗口验收。仅在通过宿主归属核对的自建工程启用。

mod evidence;

use super::{
    AssetPreviewState, AssetPreviewWorkspace,
    animation::{active_project_clip, project_clip_properties, rig_animation_identity, rig_animation_probe},
    rigging::RigAction,
    rigging_validation::{click_window_entity, fixture_focus, move_cursor, window_event},
    sidebar::validation_details_text,
    types::{AssetPreviewContent, AssetPreviewPhase},
};
use crate::{
    asset_catalog::{AssetTypeFilter, EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::{SceneEditorActionButton, SceneEditorAssetFolderText, SceneEditorAssetRow, SceneEditorLayoutButton, SceneEditorPane, SceneEditorPaneSplitter, SceneEditorSettingsButton},
    constants::EDITOR_CONTENT_MIN_WIDTH,
    resources::{SceneEditorLayoutState, SceneEditorSettingsState, SceneEditorState},
    types::{EditorAction, EditorLayoutAction, EditorPane, EditorSettingsAction},
    ui::{
        asset_thumbnails::{ViewModeButton, validation_visible_thumbnail},
        context_help::ContextHelpMounted,
        file_manager::{
            FileAction, FileManagerButton, FileManagerState, FileRow,
            browser::{BrowserAction, BrowserButton, BrowserChoice, BrowserSessions, ClipExpander},
        },
        project_browser::{
            ProjectAnimationClip, ProjectAnimationExpand, ProjectBrowserState, ProjectBrowserView,
            ProjectFileFilter, ProjectFileRow, ProjectTypeFilterButton, ProjectTypeFilterOption,
        },
        sidebars::{EditorNavigatorTab, EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarState, SceneEditorSidebarTabButton},
        tooltips::{EditorTooltipTarget, TooltipAction},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
    workbench::{WorkbenchHeader, WorkbenchHeaderItem},
};
use bevy::{
    app::AppExit,
    input::{ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput},
    input_focus::InputFocus,
    math::DVec2,
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    text::{EditableText, TextLayoutInfo},
    ui::{InteractionDisabled, UiGlobalTransform},
    window::{Ime, PrimaryWindow},
};
use feathrise_editor_core::{AssetBrowserViewMode, EditorSettings};
use feathrise_editor_documents_model::search::components::{SearchAction, SearchBody, SearchReferenceControls, SearchTextControls};
use feathrise_editor_ui::{UiInteraction, context_help::ContextHelpButton, localization::EditorLocale};
use serde_json::{Map, Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const HERO: &str = "roles/nested/hero.glb";
const STATIC: &str = "roles/nested/static.glb";
const MOTIONS: &str = "motions.glb";
const PROJECT_NAME_QUERY: &str = "Cargo.toml";
const ASSET_NAME_QUERY: &str = "动作";
const TOOLS_HELP: &str = "工程工具";
const TOOLS: &[&str] = &["内容搜索", "缺失检查", "收藏", "最近访问", "刷新", "缩略图"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    OpenDirectory,
    FocusProjectFilter,
    TypeProjectFilter,
    ClearProjectFilter,
    FilterSkeleton,
    OpenAssets,
    OpenRoles,
    OpenNested,
    SkeletonCapture,
    SelectHero,
    OpenHero,
    HeroReady,
    RestoreDirectory,
    FilterAll,
    FocusAssetFilter,
    ComposeAssetFilter,
    CommitAssetFilter,
    ClearAssetFilter,
    UpToRoles,
    UpToAssets,
    Narrow,
    OpenToolsHelp,
    ReadToolsHelp,
    OpenContentSearch,
    OpenMissingCheck,
    CloseMissingCheck,
    CollapseFavorites,
    ExpandRecent,
    CollapseRecent,
    OpenNarrowMenu,
    NarrowCapture,
    CloseNarrowMenu,
    LeaveAssetRoot,
    ShowSceneTree,
    CollapseDirectory,
    OpenRigHelp,
    ReadRigHelp,
    ChooseAnimation,
    SelectPickerFile,
    CapturePicker,
    OpenPickerClips,
    PickClipToCancel,
    CancelPicker,
    VerifyPickerCanceled,
    ReopenPicker,
    UsePickerThumbnails,
    OpenPickerThumbnails,
    ClipMenuCapture,
    ChooseSecond,
    CapturePendingClip,
    ConfirmClip,
    ObservePlayback,
    PlaybackCapture,
    ReopenDirectory,
    ReopenFiles,
    ReenterAssets,
    FilterOrdinaryAnimation,
    UseThumbnails,
    ThumbnailsEnabled,
    ThumbnailsDisabled,
    SelectOrdinaryMotion,
    VerifyOrdinaryMotion,
    Replace,
    AwaitReplacement,
    ChooseModel,
    ReplacementCapture,
    ReturnProjectRoot,
    VerifyProjectRoot,
    OpenFrameSettings,
    EnableEditorFps,
    CloseFrameSettings,
    VerifyEditorFps,
    Complete,
}

#[derive(Resource)]
struct Run {
    work: PathBuf,
    assets: PathBuf,
    original_motion: Vec<u8>,
    replacement_size: u64,
    stage: Stage,
    started: Instant,
    stage_started: Instant,
    click: Option<(Entity, bool)>,
    splitter_x: Option<DVec2>,
    tab: Option<(u64, usize)>,
    picker: Option<u64>,
    playback: Option<PlaybackWitness>,
    animation_revision: Option<u64>,
    capture: Option<evidence::CaptureTask>,
    help_ready: Option<(Entity, URect, u8)>,
    replacement: Option<Task<Result<(), String>>>,
    replacement_revision: Option<u64>,
    steps: Map<String, Value>,
    screenshots: Map<String, Value>,
}

struct PlaybackWitness {
    started: Instant,
    first_seconds: f32,
    last_pose: (f32, Quat),
    frames: usize,
    updated_frames: usize,
    hold_frames: usize,
    longest_hold_frames: usize,
    last_change: Instant,
    longest_gap: Duration,
}

pub fn install(app: &mut App, work: PathBuf, assets: PathBuf) -> Result<(), String> {
    let source = assets.join(MOTIONS);
    if source.canonicalize().ok().as_ref() != Some(&source) {
        return Err("动作验收夹具必须位于自有资产根".into());
    }
    let original_motion = fs::read(source).map_err(|error| error.to_string())?;
    let replacement_size = fs::metadata(assets.join(STATIC)).map_err(|error| error.to_string())?.len();
    let now = Instant::now();
    app.insert_resource(Run {
        work, assets, original_motion, replacement_size, stage: Stage::OpenDirectory, started: now, stage_started: now,
        click: None, splitter_x: None, tab: None, picker: None, playback: None, animation_revision: None,
        capture: None, help_ready: None, replacement: None,
        replacement_revision: None, steps: Map::new(), screenshots: Map::new(),
    })
    .add_systems(Update, fixture_focus.before(crate::SceneEditorSystemSet::Input))
    .add_systems(Last, drive);
    Ok(())
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        let result = if run.started.elapsed() > Duration::from_secs(110)
            || run.stage_started.elapsed() > Duration::from_secs(23)
        {
            Err(format!("等待 {:?} 超时", run.stage))
        } else {
            advance(world, &mut run)
        };
        let error = match result {
            Ok(Some(Stage::Complete)) => {
                run.stage = Stage::Complete;
                None
            }
            Ok(Some(next)) => {
                run.stage = next;
                run.stage_started = Instant::now();
                return;
            }
            Ok(None) => return,
            Err(message) => Some(message),
        };
        let directory = world.get_resource::<EditorAssetCatalog>().map(|catalog| json!({
            "folder":catalog.current_folder,
            "requested":catalog.requested_revision,
            "applied":catalog.applied_revision,
            "filter":format!("{:?}",catalog.adopted_type_filter),
            "entries":catalog.entries.iter().map(|entry| &entry.asset_path).collect::<Vec<_>>(),
        }));
        let project = world.get_resource::<EditorProjectFileCatalog>().map(|catalog| json!({
            "folder":catalog.current_folder,
            "loading":catalog.is_loading(),
            "filter":format!("{:?}",catalog.adopted_type_filter),
            "entries":catalog.entries.iter().map(|entry| &entry.asset_path).collect::<Vec<_>>(),
        }));
        let navigation = world.get_resource::<SceneEditorSidebarState>()
            .map(|sidebar| format!("{:?}", sidebar.navigator));
        let preview = world.get_resource::<AssetPreviewState>().map(|preview| json!({
            "path":preview.selected_path,
            "model_ready":matches!(preview.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. }),
            "active_hero_tab":active_hero(world),
        }));
        let report = json!({
            "passed":error.is_none(),"error":error,"stage":format!("{:?}",run.stage),
            "pid":std::process::id(),"elapsed_seconds":run.started.elapsed().as_secs_f32(),
            "source":"真实 Feathrise 编辑器 + Bevy WindowEvent",
            "os_input_verified":false,
            "steps":run.steps,"screenshots":run.screenshots,
            "last_asset_directory":directory,"last_project_directory":project,
            "navigator":navigation,"preview":preview,
            "animation_diagnostic":super::animation::rig_animation_diagnostic(world),
        });
        let written = evidence::report(&run.work.join("window.json"), &report);
        if let Err(error) = &written { error!("写入工程目录验收报告失败：{error}"); }
        world.write_message(if error.is_none() && written.is_ok() { AppExit::Success } else { AppExit::error() });
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<Option<Stage>, String> {
    let Some(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next() else {
        return Ok(None);
    };
    let next = match run.stage {
        Stage::OpenDirectory => {
            if !world.get_resource::<SceneEditorState>().is_some_and(|editor| {
                editor.open && editor.pending_scene_transition.is_none()
            }) || !click(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
            }) {
                return Ok(None);
            }
            Stage::FocusProjectFilter
        }
        Stage::FocusProjectFilter => {
            if !project_paths(world).is_some_and(|paths| paths.contains(&"README.md".into()))
                || !click_filename_filter(world, run, window)
            {
                return Ok(None);
            }
            Stage::TypeProjectFilter
        }
        Stage::TypeProjectFilter => {
            if !filename_filter_has_focus(world) { return Ok(None); }
            key(world, window, KeyCode::KeyC, Key::Character(PROJECT_NAME_QUERY.into()), Some(PROJECT_NAME_QUERY));
            Stage::ClearProjectFilter
        }
        Stage::ClearProjectFilter => {
            if world.resource::<EditorProjectFileCatalog>().filter != PROJECT_NAME_QUERY { return Ok(None); }
            let Some(paths) = project_paths(world) else { return Ok(None); };
            if !paths.contains(&PROJECT_NAME_QUERY.into()) || paths.contains(&"README.md".into()) {
                return Err("工程目录没有采用按文件名筛选的结果".into());
            }
            run.steps.insert("project_filename_filter".into(), json!(paths));
            clear_filename_filter(world, window, PROJECT_NAME_QUERY);
            Stage::FilterSkeleton
        }
        Stage::FilterSkeleton => {
            if !project_paths(world).is_some_and(|paths| {
                paths.contains(&"src".into()) && paths.contains(&"assets".into()) && paths.contains(&"README.md".into())
            })
                && run.click.is_none()
            {
                return Ok(None);
            }
            if !choose_filter(world, run, window, AssetTypeFilter::Skeleton) { return Ok(None); }
            Stage::OpenAssets
        }
        Stage::OpenAssets => {
            let Some(paths) = project_paths(world).filter(|paths| paths == &["assets"]) else { return Ok(None); };
            let row = project_row(world, "assets");
            if !click_entity(world, run, window, row) { return Ok(None); }
            run.steps.insert("skeleton_project_root".into(), json!(paths));
            Stage::OpenRoles
        }
        Stage::OpenRoles => {
            let Some(paths) = asset_paths(world, "").filter(|paths| paths == &["roles"]) else { return Ok(None); };
            let row = asset_row(world, "roles");
            if !click_entity(world, run, window, row) { return Ok(None); }
            run.steps.insert("skeleton_asset_root".into(), json!(paths));
            Stage::OpenNested
        }
        Stage::OpenNested => {
            let Some(paths) = asset_paths(world, "roles").filter(|paths| paths == &["roles/nested"]) else {
                return Ok(None);
            };
            let row = asset_row(world, "roles/nested");
            if !click_entity(world, run, window, row) { return Ok(None); }
            run.steps.insert("skeleton_roles".into(), json!(paths));
            Stage::SkeletonCapture
        }
        Stage::SkeletonCapture => {
            let Some(paths) = asset_paths(world, "roles/nested").filter(|paths| paths == &[HERO]) else { return Ok(None); };
            if asset_row(world, HERO).is_none() || !capture(world, run, "skeleton.png")? {
                return Ok(None);
            }
            run.steps.insert("skeleton_nested".into(), json!(paths));
            Stage::SelectHero
        }
        Stage::SelectHero | Stage::OpenHero => {
            let row = asset_row(world, HERO);
            if !click_entity(world, run, window, row) { return Ok(None); }
            if run.stage == Stage::SelectHero { Stage::OpenHero } else { Stage::HeroReady }
        }
        Stage::HeroReady => {
            let Some(tab) = active_hero(world) else {
                // 真窗口在编译着色器时首组双击可能超过正常双击窗口。仍从原两次点击重试。
                return Ok((run.stage_started.elapsed() > Duration::from_millis(650)).then_some(Stage::SelectHero));
            };
            if !world.get_resource::<AssetPreviewState>().is_some_and(|preview| {
                matches!(preview.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. })
            }) { return Ok(None); }
            run.tab = Some(tab);
            Stage::RestoreDirectory
        }
        Stage::RestoreDirectory => {
            // 原模型标签打开后显示自己的场景树。通过原工程页签返回目录，保留中央角色。
            if !world.resource::<SceneEditorSidebarState>().assets_browser_visible()
                && !click(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                    tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
                })
            {
                return Ok(None);
            }
            Stage::FilterAll
        }
        Stage::FilterAll => {
            if !choose_filter(world, run, window, AssetTypeFilter::All) { return Ok(None); }
            Stage::FocusAssetFilter
        }
        Stage::FocusAssetFilter => {
            if !asset_paths(world, "roles/nested").is_some_and(|paths| paths == [HERO, STATIC])
                || !click_filename_filter(world, run, window)
            {
                return Ok(None);
            }
            Stage::ComposeAssetFilter
        }
        Stage::ComposeAssetFilter => {
            if !filename_filter_has_focus(world) || !world.get::<Window>(window).is_some_and(|window| window.ime_enabled) {
                return Ok(None);
            }
            window_event(world, Ime::Preedit { window, value: "dongzuo".into(), cursor: Some((7, 7)) });
            Stage::CommitAssetFilter
        }
        Stage::CommitAssetFilter => {
            let Some(input) = world.resource::<InputFocus>().get().and_then(|entity| world.get::<EditableText>(entity)) else {
                return Ok(None);
            };
            if !input.is_composing() { return Ok(None); }
            if !world.resource::<EditorAssetCatalog>().filename_filter.is_empty() {
                return Err("未确认的输入法候选触发了目录筛选".into());
            }
            window_event(world, Ime::Commit { window, value: ASSET_NAME_QUERY.into() });
            Stage::ClearAssetFilter
        }
        Stage::ClearAssetFilter => {
            if world.resource::<EditorAssetCatalog>().filename_filter != ASSET_NAME_QUERY { return Ok(None); }
            let Some(paths) = asset_paths(world, "roles/nested") else { return Ok(None); };
            if !paths.is_empty() { return Err("资产目录没有采用中文文件名筛选".into()); }
            run.steps.insert("asset_filename_ime_filter".into(), json!(ASSET_NAME_QUERY));
            clear_filename_filter(world, window, ASSET_NAME_QUERY);
            Stage::UpToRoles
        }
        Stage::UpToRoles => {
            let Some(paths) = asset_paths(world, "roles/nested").filter(|paths| paths == &[HERO, STATIC]) else {
                return Ok(None);
            };
            if !click(world, run, window, |button: &SceneEditorActionButton| button.0 == EditorAction::AssetFolderUp) {
                return Ok(None);
            }
            run.steps.insert("all_nested".into(), json!(paths));
            Stage::UpToAssets
        }
        Stage::UpToAssets => {
            let Some(paths) = asset_paths(world, "roles").filter(|paths| paths.contains(&"roles/empty".into())) else {
                return Ok(None);
            };
            if !click(world, run, window, |button: &SceneEditorActionButton| button.0 == EditorAction::AssetFolderUp) {
                return Ok(None);
            }
            run.steps.insert("all_roles".into(), json!(paths));
            Stage::Narrow
        }
        Stage::Narrow => {
            let Some(paths) = asset_paths(world, "").filter(|paths| {
                [MOTIONS, "roles", "empty", "swatch.png", "beep.wav", "sample.avi"]
                    .iter().all(|path| paths.iter().any(|entry| entry == path))
            }) else { return Ok(None); };
            if !narrow(world, run, window) { return Ok(None); }
            run.steps.insert("all_asset_root".into(), json!(paths));
            Stage::OpenToolsHelp
        }
        Stage::OpenToolsHelp => {
            if !click(world, run, window, |button: &ContextHelpButton| button.0 == TOOLS_HELP) {
                return Ok(None);
            }
            Stage::ReadToolsHelp
        }
        Stage::ReadToolsHelp => {
            let Some((copy, region, glyphs)) = ready_help(world, run) else { return Ok(None); };
            if !TOOLS.iter().all(|&tool| copy.contains(tool)) {
                return Err(format!("工程工具帮助未列出整行操作：{copy}"));
            }
            run.steps.insert("asset_tools_help".into(), json!(copy));
            run.steps.insert("asset_tools_help_glyphs".into(), json!(glyphs));
            if !capture_with_text(world, run, "asset-tools-help.png", Some(region))? { return Ok(None); }
            key(world, window, KeyCode::Escape, Key::Escape, None);
            Stage::OpenContentSearch
        }
        Stage::OpenContentSearch => {
            if mounted_help(world).is_some() { return Ok(None); }
            let Some(layout) = asset_tools_layout(world)? else { return Ok(None); };
            if !click(world, run, window, |action: &SearchAction| *action == SearchAction::Toggle) {
                return Ok(None);
            }
            run.steps.insert("asset_tools_one_row".into(), layout);
            Stage::OpenMissingCheck
        }
        Stage::OpenMissingCheck => {
            if visible(world, |_: &SearchTextControls| true).is_none()
                || !click(world, run, window, |action: &SearchAction| *action == SearchAction::MissingAssets)
            {
                return Ok(None);
            }
            Stage::CloseMissingCheck
        }
        Stage::CloseMissingCheck => {
            if visible(world, |_: &SearchReferenceControls| true).is_none()
                || !click(world, run, window, |action: &SearchAction| *action == SearchAction::MissingAssets)
            {
                return Ok(None);
            }
            Stage::CollapseFavorites
        }
        Stage::CollapseFavorites => {
            if visible(world, |_: &SearchBody| true).is_some()
                || !click(world, run, window, |target: &EditorTooltipTarget| target.0 == TooltipAction::AssetFavorites)
            {
                return Ok(None);
            }
            Stage::ExpandRecent
        }
        Stage::ExpandRecent => {
            let locale = *world.resource::<EditorLocale>();
            if visible(world, |text: &Text| text.0 == locale.editor_label("检查路径")).is_some()
                || !click(world, run, window, |target: &EditorTooltipTarget| target.0 == TooltipAction::AssetRecent)
            {
                return Ok(None);
            }
            Stage::CollapseRecent
        }
        Stage::CollapseRecent => {
            let locale = *world.resource::<EditorLocale>();
            if visible(world, |text: &Text| text.0 == locale.editor_label("清空")).is_none()
                || !click(world, run, window, |target: &EditorTooltipTarget| target.0 == TooltipAction::AssetRecent)
            {
                return Ok(None);
            }
            run.steps.insert("compact_sections_clicked".into(), json!(true));
            Stage::OpenNarrowMenu
        }
        Stage::OpenNarrowMenu => {
            let locale = *world.resource::<EditorLocale>();
            if visible(world, |text: &Text| text.0 == locale.editor_label("清空")).is_some() {
                return Ok(None);
            }
            if !click(world, run, window, |_: &ProjectTypeFilterButton| true) { return Ok(None); }
            Stage::NarrowCapture
        }
        Stage::NarrowCapture => {
            let Some(layout) = narrow_menu(world, window) else { return Ok(None); };
            if !capture(world, run, "narrow-menu.png")? { return Ok(None); }
            run.steps.insert("narrow_menu".into(), layout);
            Stage::CloseNarrowMenu
        }
        Stage::CloseNarrowMenu => {
            if !click(world, run, window, |option: &ProjectTypeFilterOption| option.0 == AssetTypeFilter::All) {
                return Ok(None);
            }
            Stage::LeaveAssetRoot
        }
        Stage::LeaveAssetRoot => {
            if !click(world, run, window, |button: &SceneEditorActionButton| button.0 == EditorAction::AssetFolderUp) {
                return Ok(None);
            }
            Stage::ShowSceneTree
        }
        Stage::ShowSceneTree => {
            if project_paths(world).is_none() || !click(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree)
            }) {
                return Ok(None);
            }
            Stage::CollapseDirectory
        }
        Stage::CollapseDirectory => {
            if !world.resource::<SceneEditorSidebarState>().scene_tree_visible()
                || !click(world, run, window, |button: &SceneEditorLayoutButton| button.0 == EditorLayoutAction::Content)
            {
                return Ok(None);
            }
            Stage::OpenRigHelp
        }
        Stage::OpenRigHelp => {
            if world.resource::<SceneEditorLayoutState>().content_shown() || !hero_properties_visible(world, run)
                || !click(world, run, window, |button: &ContextHelpButton| button.0 == "工程动作")
            {
                return Ok(None);
            }
            Stage::ReadRigHelp
        }
        Stage::ReadRigHelp => {
            let Some((copy, region, glyphs)) = ready_help(world, run) else { return Ok(None); };
            if !["选择动画", "窗口", "片段", "当前模型"].iter().all(|&term| copy.contains(term)) {
                return Err(format!("工程动作帮助缺少选择和播放说明：{copy}"));
            }
            run.steps.insert("project_motion_help".into(), json!(copy));
            run.steps.insert("project_motion_help_glyphs".into(), json!(glyphs));
            if !capture_with_text(world, run, "project-motion-help.png", Some(region))? { return Ok(None); }
            key(world, window, KeyCode::Escape, Key::Escape, None);
            Stage::ChooseAnimation
        }
        Stage::ChooseAnimation => {
            // 用标题栏和场景树的正常操作收起左栏，从右侧模型属性实际点击入口。
            if mounted_help(world).is_some()
                || world.resource::<SceneEditorLayoutState>().content_shown()
                || active_hero(world) != run.tab
                || !click(world, run, window, |action: &RigAction| *action == RigAction::ChooseProjectMotion)
            {
                return Ok(None);
            }
            Stage::SelectPickerFile
        }
        Stage::SelectPickerFile => {
            let Some(id) = picker_directory(world, run)? else { return Ok(None); };
            run.picker = Some(id);
            let row = picker_row(world, id);
            if !click_entity(world, run, window, row) { return Ok(None); }
            Stage::CapturePicker
        }
        Stage::CapturePicker => {
            picker_keeps_model(world, run)?;
            let Some(id) = picker_directory(world, run)? else { return Ok(None); };
            let Some(state) = world.resource::<FileManagerState>().windows.iter().find(|candidate| candidate.id == id) else { return Ok(None); };
            if !state.selection.first().is_some_and(|path| path.ends_with(MOTIONS))
                || !capture(world, run, "animation-picker.png")?
            {
                return Ok(None);
            }
            run.steps.insert("right_sidebar_animation_picker".into(), json!({
                "opened_from_hidden_scene_tree":true,"asset_type":"Animation","model_tab":run.tab,
                "window":id,"selected":MOTIONS,"keeps_model_properties":true,
            }));
            Stage::OpenPickerClips
        }
        Stage::OpenPickerClips | Stage::OpenPickerThumbnails => {
            picker_keeps_model(world, run)?;
            let Some(id) = picker_directory(world, run)? else { return Ok(None); };
            if run.stage == Stage::OpenPickerThumbnails
                && world.resource::<BrowserSessions>().validation_view(id) != Some(AssetBrowserViewMode::Thumbnails)
            {
                return Ok(None);
            }
            let expander = picker_row(world, id)
                .and_then(|row| visible(world, |expand: &ClipExpander| expand.row == row));
            if !click_entity(world, run, window, expander) { return Ok(None); }
            if run.stage == Stage::OpenPickerClips { Stage::PickClipToCancel } else { Stage::ClipMenuCapture }
        }
        Stage::PickClipToCancel => {
            picker_keeps_model(world, run)?;
            if !click_picker_clip(world, run, window, 0) { return Ok(None); }
            Stage::CancelPicker
        }
        Stage::CancelPicker => {
            picker_keeps_model(world, run)?;
            // 确认入口出现，证明选中了候选片段；用原取消按钮关闭，后台不能收到此意图。
            let id = run.picker;
            if visible(world, |button: &FileManagerButton| button.window == id && button.action == FileAction::ChooseClip).is_none()
                || !click(world, run, window, |button: &FileManagerButton| button.window == id && button.action == FileAction::CancelPicker)
            {
                return Ok(None);
            }
            Stage::VerifyPickerCanceled
        }
        Stage::VerifyPickerCanceled => {
            picker_keeps_model(world, run)?;
            if picker_window_id(world).is_some() { return Ok(None); }
            run.steps.insert("animation_picker_cancel_keeps_model".into(), json!(true));
            Stage::ReopenPicker
        }
        Stage::ReopenPicker => {
            picker_keeps_model(world, run)?;
            if !click(world, run, window, |action: &RigAction| *action == RigAction::ChooseProjectMotion) {
                return Ok(None);
            }
            Stage::UsePickerThumbnails
        }
        Stage::UsePickerThumbnails => {
            picker_keeps_model(world, run)?;
            let Some(id) = picker_directory(world, run)? else { return Ok(None); };
            if Some(id) != run.picker {
                let state = world.resource::<FileManagerState>();
                if state.windows.iter().find(|candidate| candidate.id == id).is_some_and(|candidate| !candidate.selection.is_empty()) {
                    return Err("重开动画选择窗沿用了已取消的文件选择".into());
                }
                run.picker = Some(id);
                run.steps.insert("animation_picker_reopened_clean".into(), json!(true));
            }
            if world.resource::<BrowserSessions>().validation_view(id) == Some(AssetBrowserViewMode::Thumbnails) {
                return Ok(Some(Stage::OpenPickerThumbnails));
            }
            if !click(world, run, window, |button: &BrowserButton| button.window == id && button.action == BrowserAction::View) {
                return Ok(None);
            }
            Stage::OpenPickerThumbnails
        }
        Stage::ClipMenuCapture => {
            picker_keeps_model(world, run)?;
            if [0, 1].into_iter().any(|index| picker_clip(world, run.picker, index).is_none())
                || !capture(world, run, "clip-menu.png")?
            {
                return Ok(None);
            }
            run.steps.insert("clip_arrow_keeps_model_properties".into(), json!(true));
            Stage::ChooseSecond
        }
        Stage::ChooseSecond => {
            picker_keeps_model(world, run)?;
            if !click_picker_clip(world, run, window, 1) { return Ok(None); }
            Stage::CapturePendingClip
        }
        Stage::CapturePendingClip => {
            picker_keeps_model(world, run)?;
            let id = run.picker;
            if visible(world, |button: &FileManagerButton| button.window == id && button.action == FileAction::ChooseClip).is_none()
                || !capture(world, run, "animation-picker-confirm.png")?
            {
                return Ok(None);
            }
            Stage::ConfirmClip
        }
        Stage::ConfirmClip => {
            picker_keeps_model(world, run)?;
            let id = run.picker;
            if !click(world, run, window, |button: &FileManagerButton| button.window == id && button.action == FileAction::ChooseClip) {
                return Ok(None);
            }
            run.steps.insert("animation_picker_confirms_clip_explicitly".into(), json!(true));
            Stage::ObservePlayback
        }
        Stage::ObservePlayback => {
            if picker_window_id(world).is_some() { return Ok(None); }
            let Some(proof) = observe_motion(world, run)? else { return Ok(None); };
            run.steps.insert("second_clip".into(), proof);
            Stage::PlaybackCapture
        }
        Stage::PlaybackCapture => {
            if !capture(world, run, "second-clip.png")? { return Ok(None); }
            Stage::ReopenDirectory
        }
        Stage::ReopenDirectory => {
            // 正常浏览仍使用原文件页签和目录选择。选择器不能替代此入口或预先切换左栏。
            if !click(world, run, window, |button: &SceneEditorLayoutButton| button.0 == EditorLayoutAction::Content) {
                return Ok(None);
            }
            Stage::ReopenFiles
        }
        Stage::ReopenFiles => {
            if !world.resource::<SceneEditorLayoutState>().content_shown()
                || !click(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                    tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
                })
            {
                return Ok(None);
            }
            Stage::ReenterAssets
        }
        Stage::ReenterAssets => {
            if project_paths(world).is_none() { return Ok(None); }
            let row = project_row(world, "assets");
            if !click_entity(world, run, window, row) { return Ok(None); }
            Stage::FilterOrdinaryAnimation
        }
        Stage::FilterOrdinaryAnimation => {
            if asset_paths(world, "").is_none() || !choose_filter(world, run, window, AssetTypeFilter::Animation) {
                return Ok(None);
            }
            Stage::UseThumbnails
        }
        Stage::UseThumbnails => {
            let Some(paths) = asset_paths(world, "").filter(|paths| paths == &[MOTIONS]) else { return Ok(None); };
            if !world.resource::<SceneEditorSidebarState>().assets_browser_visible()
                || !world.resource::<SceneEditorLayoutState>().content_shown()
                || world.resource::<EditorAssetCatalog>().adopted_type_filter != AssetTypeFilter::Animation
                || !hero_properties_visible(world, run)
            {
                return Err("手动打开左侧动画目录时跳离了原角色属性".into());
            }
            let Some(expander) = animation_expander(world) else { return Ok(None); };
            if world.get::<Node>(expander).is_none_or(|node| node.position_type != PositionType::Relative)
                || !click(world, run, window, |_: &ViewModeButton| true)
            {
                return Ok(None);
            }
            let revision = world.resource::<EditorAssetCatalog>().applied_revision;
            run.animation_revision = Some(revision);
            run.steps.insert("animation_list".into(), json!({"entries":paths,"revision":revision}));
            Stage::ThumbnailsEnabled
        }
        Stage::ThumbnailsEnabled => {
            if world.resource::<EditorSettings>().asset_browser_view_mode != AssetBrowserViewMode::Thumbnails {
                return Ok(None);
            }
            if !run.screenshots.contains_key("compact-browser.png") && !capture(world, run, "compact-browser.png")? {
                return Ok(None);
            }
            if !click(world, run, window, |_: &ViewModeButton| true) { return Ok(None); }
            Stage::ThumbnailsDisabled
        }
        Stage::ThumbnailsDisabled => {
            if world.resource::<EditorSettings>().asset_browser_view_mode != AssetBrowserViewMode::List {
                return Ok(None);
            }
            let revision = world.resource::<EditorAssetCatalog>().applied_revision;
            if Some(revision) != run.animation_revision {
                return Err("同一缩略图按钮恢复列表时改变了已采用的目录".into());
            }
            if !click(world, run, window, |_: &ViewModeButton| true) { return Ok(None); }
            run.steps.insert("thumbnails_toggled_off".into(), json!(true));
            Stage::SelectOrdinaryMotion
        }
        Stage::SelectOrdinaryMotion => {
            let Some(paths) = asset_paths(world, "").filter(|paths| paths == &[MOTIONS]) else { return Ok(None); };
            let Some(expander) = animation_expander(world) else { return Ok(None); };
            if world.resource::<EditorSettings>().asset_browser_view_mode != AssetBrowserViewMode::Thumbnails
                || world.get::<Node>(expander).is_none_or(|node| node.position_type != PositionType::Absolute)
            {
                return Ok(None);
            }
            let revision = world.resource::<EditorAssetCatalog>().applied_revision;
            if Some(revision) != run.animation_revision {
                return Err("列表与缩略图切换改变了已采用的动画筛选结果".into());
            }
            let row = asset_row(world, MOTIONS);
            if !click_entity(world, run, window, row) { return Ok(None); }
            run.steps.insert("animation_thumbnails".into(), json!({"entries":paths,"revision":revision}));
            Stage::VerifyOrdinaryMotion
        }
        Stage::VerifyOrdinaryMotion => {
            if world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(MOTIONS)
                || world.resource::<SceneEditorSidebarState>().right != EditorRightSidebarTab::Assets
                || active_hero(world) != run.tab
                || !validation_details_text(world).is_some_and(|text| text.contains("网格 0") && text.contains("动画 2"))
            {
                return Ok(None);
            }
            if visible(world, |clip: &ProjectAnimationClip| clip.clip_index.is_some()).is_some() {
                return Err("普通文件卡点击时打开了片段菜单".into());
            }
            run.steps.insert("ordinary_motion_properties".into(), json!({
                "path":MOTIONS,"model_tab":run.tab,"asset_type":"Animation",
            }));
            Stage::Replace
        }
        Stage::Replace => {
            if run.replacement.is_none() {
                run.replacement_revision = Some(world.resource::<EditorAssetCatalog>().applied_revision);
                let (work, assets, original) = (run.work.clone(), run.assets.clone(), run.original_motion.clone());
                run.replacement = Some(IoTaskPool::get().spawn(async move {
                    evidence::replace_clip_file(&work, &assets, &original)
                }));
            }
            let Some(result) = run.replacement.as_mut().and_then(check_ready) else { return Ok(None); };
            result?;
            Stage::AwaitReplacement
        }
        Stage::AwaitReplacement => {
            let catalog = world.resource::<EditorAssetCatalog>();
            let before = run.replacement_revision.unwrap_or(0);
            if catalog.applied_revision <= before || !asset_paths(world, "").is_some_and(|paths| paths.is_empty()) {
                return Ok(None);
            }
            run.steps.insert("replacement".into(), json!({
                "previous_revision":before,"applied_revision":catalog.applied_revision,"entries":[],"manual_refresh":false,
            }));
            Stage::ChooseModel
        }
        Stage::ChooseModel => {
            if !choose_filter(world, run, window, AssetTypeFilter::Model) { return Ok(None); }
            Stage::ReplacementCapture
        }
        Stage::ReplacementCapture => {
            let Some(paths) = asset_paths(world, "").filter(|paths| paths == &["roles", MOTIONS]) else {
                return Ok(None);
            };
            let Some(row) = asset_row(world, MOTIONS) else { return Ok(None); };
            let Some((size, texture)) = validation_visible_thumbnail(world, row, MOTIONS) else { return Ok(None); };
            if size != run.replacement_size || size == run.original_motion.len() as u64 {
                return Err("缩略图就绪后仍指向替换前的源文件".into());
            }
            if !capture(world, run, "replaced.png")? { return Ok(None); }
            run.steps.insert("reclassified_model".into(), json!(paths));
            run.steps.insert("replacement_thumbnail".into(), json!({
                "asset":MOTIONS,"source_bytes":size,"texture_size":texture.to_array(),
            }));
            Stage::ReturnProjectRoot
        }
        Stage::ReturnProjectRoot => {
            if !click(world, run, window, |button: &SceneEditorActionButton| button.0 == EditorAction::AssetFolderUp) {
                return Ok(None);
            }
            Stage::VerifyProjectRoot
        }
        Stage::VerifyProjectRoot => {
            if project_paths(world).is_none() || visible(world, |text: &Text| text.0 == "..").is_some() {
                return Ok(None);
            }
            run.steps.insert("asset_parent_returns_project_root".into(), json!(true));
            if cfg!(windows) { Stage::OpenFrameSettings } else { Stage::Complete }
        }
        Stage::OpenFrameSettings => {
            // 作者进程的独占偏好初次启动，先核对实际标题栏隐藏，再通过原标题栏按钮打开设置。
            let hidden = world.query::<(&Name, &Node)>().iter(world)
                .find_map(|(name, node)| (name.as_str() == "编辑器帧率").then_some(node.display == Display::None));
            let Some(hidden) = hidden else { return Ok(None); };
            if !hidden || world.resource::<EditorSettings>().show_editor_fps {
                return Err("新验收工程默认显示了编辑器帧率".into());
            }
            if !click(world, run, window, |button: &SceneEditorSettingsButton| button.0 == EditorSettingsAction::Open) {
                return Ok(None);
            }
            Stage::EnableEditorFps
        }
        Stage::EnableEditorFps => {
            if !world.resource::<SceneEditorSettingsState>().open
                || !click(world, run, window, |button: &SceneEditorSettingsButton| button.0 == EditorSettingsAction::ToggleEditorFpsDisplay)
            {
                return Ok(None);
            }
            Stage::CloseFrameSettings
        }
        Stage::CloseFrameSettings => {
            if !world.resource::<EditorSettings>().show_editor_fps { return Ok(None); }
            key(world, window, KeyCode::Escape, Key::Escape, None);
            Stage::VerifyEditorFps
        }
        Stage::VerifyEditorFps => {
            if world.resource::<SceneEditorSettingsState>().open { return Ok(None); }
            let Some(proof) = editor_fps_layout(world)? else { return Ok(None); };
            if !capture(world, run, "title-fps.png")? { return Ok(None); }
            run.steps.insert("editor_title_fps".into(), proof);
            Stage::Complete
        }
        Stage::Complete => return Ok(None),
    };
    Ok(Some(next))
}

fn project_paths(world: &World) -> Option<Vec<String>> {
    let browser = world.get_resource::<ProjectBrowserState>()?;
    let catalog = world.get_resource::<EditorProjectFileCatalog>()?;
    (browser.view == ProjectBrowserView::ProjectFiles && catalog.current_folder.is_empty()
        && !catalog.is_loading() && catalog.feedback.as_ref().is_some_and(|feedback| feedback.result.is_ok()))
        .then(|| catalog.entries.iter().map(|entry| entry.asset_path.clone()).collect())
}

fn asset_paths(world: &World, folder: &str) -> Option<Vec<String>> {
    let browser = world.get_resource::<ProjectBrowserState>()?;
    let catalog = world.get_resource::<EditorAssetCatalog>()?;
    (browser.assets_visible() && catalog.current_folder == folder && catalog.applied_revision == catalog.requested_revision
        && catalog.feedback.as_ref().is_some_and(|feedback| feedback.result.is_ok()))
        .then(|| catalog.entries.iter().map(|entry| entry.asset_path.clone()).collect())
}

fn project_row(world: &mut World, path: &str) -> Option<Entity> {
    let index = world.resource::<EditorProjectFileCatalog>().entries.iter().position(|entry| entry.asset_path == path)?;
    visible(world, |row: &ProjectFileRow| row.entry_index == Some(index))
}

fn asset_row(world: &mut World, path: &str) -> Option<Entity> {
    let index = world.resource::<EditorAssetCatalog>().entries.iter().position(|entry| entry.asset_path == path)?;
    visible(world, |row: &SceneEditorAssetRow| row.entry_index == Some(index))
}

fn animation_expander(world: &mut World) -> Option<Entity> {
    let index = world.resource::<EditorAssetCatalog>().entries.iter().position(|entry| entry.asset_path == MOTIONS)?;
    visible(world, |expand: &ProjectAnimationExpand| expand.0.is_some_and(|binding| binding.index == Some(index)))
}

fn picker_window_id(world: &World) -> Option<u64> {
    world.get_resource::<FileManagerState>()?.windows.iter()
        .find(|window| window.is_animation_picker()).map(|window| window.id)
}

fn picker_directory(world: &World, run: &Run) -> Result<Option<u64>, String> {
    let Some(state) = world.get_resource::<FileManagerState>() else { return Ok(None); };
    let mut windows = state.windows.iter().filter(|window| window.is_animation_picker());
    let Some(picker) = windows.next() else { return Ok(None); };
    if windows.next().is_some() || picker.minimized {
        return Err("选择动画应只弹出一个当前可操作的文件窗口".into());
    }
    if let Some(error) = &picker.error { return Err(format!("动画选择窗目录读取失败：{error}")); }
    if picker.loading { return Ok(None); }
    let platform_path = crate::ui::external_files::platform_path;
    if platform_path(&picker.path) != platform_path(&run.assets) {
        return Err(format!("左侧位于工程根时，动画选择窗应从资产根开始，实际为 {}", picker.path.display()));
    }
    let entries: Vec<_> = picker.entries.iter().filter(|entry| !entry.directory)
        .map(|entry| entry.name.as_str()).collect();
    if entries != [MOTIONS] {
        return Err(format!("动画选择窗没有筛出实际动作文件：{entries:?}"));
    }
    Ok(Some(picker.id))
}

fn picker_keeps_model(world: &World, run: &Run) -> Result<(), String> {
    let layout = world.resource::<SceneEditorLayoutState>();
    if layout.content_shown() || !world.resource::<SceneEditorSidebarState>().scene_tree_visible()
        || world.resource::<ProjectBrowserState>().view != ProjectBrowserView::ProjectFiles
    {
        return Err("动画选择窗改变了原固定栏的显隐、场景树页签或文件目录".into());
    }
    if !layout.inspector_shown() || !hero_properties_visible(world, run) {
        return Err("选择动画候选时跳离了右侧原模型属性或中央标签".into());
    }
    if rig_animation_identity(world).is_some() || active_project_clip(world).is_some() {
        return Err("尚未按下选择确认时，候选片段已被应用到当前模型".into());
    }
    Ok(())
}

fn picker_row(world: &mut World, id: u64) -> Option<Entity> {
    visible(world, |row: &FileRow| row.window == id
        && row.path.as_ref().is_some_and(|path| path.ends_with(MOTIONS)))
}

fn picker_clip(world: &mut World, id: Option<u64>, index: usize) -> Option<Entity> {
    let id = id?;
    visible(world, |clip: &BrowserChoice| clip.window == id && clip.clip == Some(index))
}

fn click_picker_clip(world: &mut World, run: &mut Run, window: Entity, index: usize) -> bool {
    let target = picker_clip(world, run.picker, index);
    click_entity(world, run, window, target)
}

fn active_hero(world: &World) -> Option<(u64, usize)> {
    let media = world.get_resource::<AssetPreviewWorkspace>()?;
    let workspace = world.get_resource::<VegetationPresetEditorState>()?;
    let tab = media.expanded.as_ref()?;
    (media.active(workspace.active_tab) && tab.path == HERO).then(|| (tab.id, media.tabs().count()))
}

fn hero_properties_visible(world: &World, run: &Run) -> bool {
    active_hero(world) == run.tab
        && world.resource::<SceneEditorState>().selected_asset.as_deref() == Some(HERO)
        && world.resource::<SceneEditorSidebarState>().right == EditorRightSidebarTab::Assets
}

fn observe_motion(world: &mut World, run: &mut Run) -> Result<Option<Value>, String> {
    if !hero_properties_visible(world, run) {
        return Err("选择片段时跳离了原角色属性或标签".into());
    }
    let Some((path, index, _, true)) = active_project_clip(world) else { return Ok(None); };
    if path != run.assets.join(MOTIONS) || index != 1 {
        return Err("目录第二个片段没有在原角色标签中播放".into());
    }
    let Some((tab, host, model, motion, index)) = rig_animation_identity(world) else { return Ok(None); };
    if Some(tab) != run.tab.map(|(id, _)| id) || host != HERO || model != HERO || motion != MOTIONS || index != 1 {
        return Err("工程动作预览请求采用了错误的源、目标或片段".into());
    }
    let Some(pose) = rig_animation_probe(world) else {
        return if run.playback.is_some() { Err("播放期间丢失了可见角色姿态".into()) } else { Ok(None) };
    };
    if pose.model_path != HERO || pose.clip_index != 1 || pose.clip_name != "抬臂" {
        return Err("已显示角色发布的片段不是第二段抬臂".into());
    }
    let Some((_, rotation)) = pose.joints.iter().find(|(name, _)| name == "mixamorig:LeftArm") else {
        return Err("实际蒙皮模型缺少用于比对的左臂关节".into());
    };
    let now = Instant::now();
    let Some(playback) = run.playback.as_mut() else {
        run.playback = Some(PlaybackWitness {
            started: now, first_seconds: pose.time, last_pose: (pose.time, *rotation),
            frames: 0, updated_frames: 0, hold_frames: 0, longest_hold_frames: 0,
            last_change: now, longest_gap: Duration::ZERO,
        });
        return Ok(None);
    };
    // 每帧观察真正可见容器。时间轴可以继续走，只有发布秒数和实际左臂旋转一起变化才算新姿态。
    playback.frames += 1;
    playback.longest_gap = playback.longest_gap.max(now.duration_since(playback.last_change));
    let (previous_time, previous_rotation) = playback.last_pose;
    if (pose.time - previous_time).abs() > 1e-5
        && !rotation.abs_diff_eq(previous_rotation, 1e-5)
        && !rotation.abs_diff_eq(-previous_rotation, 1e-5)
    {
        playback.updated_frames += 1;
        playback.hold_frames = 0;
        playback.last_change = now;
    } else {
        playback.hold_frames += 1;
        playback.longest_hold_frames = playback.longest_hold_frames.max(playback.hold_frames);
    }
    playback.last_pose = (pose.time, *rotation);
    let elapsed = now.duration_since(playback.started);
    if elapsed < Duration::from_millis(800) { return Ok(None); }
    // 允许偶发后台调度延迟，同时捕获稳定的隔帧发布或更稀疏的停顿。
    if playback.frames < 12 || playback.updated_frames * 4 < playback.frames * 3 {
        return Err(format!("窗口连续播放 {:.0} 毫秒，仅 {}/{} 帧更新可见左臂，最长连续停留 {} 帧",
            elapsed.as_secs_f32() * 1000.0, playback.updated_frames, playback.frames, playback.longest_hold_frames));
    }
    let Some(properties) = project_clip_properties(world) else { return Ok(None); };
    let (label, parameters) = (&properties.label, &properties.parameters);
    if !label.contains("2/2") || !label.contains("抬臂") || !parameters.contains("30 fps")
        || !parameters.contains("1×") || !parameters.contains("循环")
    {
        return Err(format!("右侧属性缺少当前动作与参数：{label} / {parameters}"));
    }
    if properties.title_lines != 1 || (properties.title_width - properties.section_width).abs() > 1.0 {
        return Err(format!(
            "属性动作短标题应使用整栏宽度并保持一行，实际 {} 行，标题宽度 {:.1} / {:.1} 像素",
            properties.title_lines, properties.title_width, properties.section_width,
        ));
    }
    Ok(Some(json!({
        "model_tab_id":tab,"host":HERO,"source":MOTIONS,"clip_index":1,"clip_name":pose.clip_name,
        "first_sample_seconds":playback.first_seconds,"second_sample_seconds":pose.time,"left_arm_rotation_changed":true,
        "cadence":{
            "observed_ms":elapsed.as_secs_f32()*1000.0,"window_frames":playback.frames,
            "updated_pose_frames":playback.updated_frames,"longest_pose_hold_frames":playback.longest_hold_frames,
            "longest_pose_gap_ms":playback.longest_gap.as_secs_f32()*1000.0,
        },
        "bound_joints":pose.joints.len(),"visible_model_root":format!("{:?}",pose.root),
        "property_label":label,"property_parameters":parameters,
        "property_title_lines":properties.title_lines,
        "property_title_width_px":properties.title_width,"property_section_width_px":properties.section_width,
    })))
}

fn mounted_help(world: &mut World) -> Option<Entity> {
    visible(world, |_: &ContextHelpMounted| true)
}

fn ready_help(world: &mut World, run: &mut Run) -> Option<(String, URect, usize)> {
    let body = mounted_help(world)?;
    let glyphs = world.get::<TextLayoutInfo>(body)?.glyphs.len();
    let scroll = world.get::<ChildOf>(body)?.parent();
    let rect = |entity| {
        let size = world.get::<ComputedNode>(entity)?.size();
        let center = world.get::<UiGlobalTransform>(entity)?.translation;
        Some(Rect::from_center_size(center, size))
    };
    let (body_rect, scroll_rect) = (rect(body)?, rect(scroll)?);
    let min = (body_rect.min.max(scroll_rect.min) + Vec2::splat(2.0)).ceil().max(Vec2::ZERO);
    let max = (body_rect.max.min(scroll_rect.max) - Vec2::splat(2.0)).floor();
    if glyphs == 0 || min.cmpge(max).any() {
        run.help_ready = None;
        return None;
    }
    let region = URect::from_corners(min.as_uvec2(), max.as_uvec2());
    let stable = run.help_ready.filter(|&(entity, bounds, _)| entity == body && bounds == region)
        .map_or(1, |(_, _, frames)| (frames + 1).min(3));
    run.help_ready = Some((body, region, stable));
    // 等待重挂正文、字形与滚动裁剪连续稳定，GPU 回读仍需确认正文实际画出来。
    if stable < 3 { return None; }
    Some((world.get::<Text>(body)?.0.clone(), region, glyphs))
}

fn editor_fps_layout(world: &mut World) -> Result<Option<Value>, String> {
    let Some(fps) = visible(world, |name: &Name| name.as_str() == "编辑器帧率") else { return Ok(None); };
    let Some(text) = world.get::<Text>(fps).map(|text| text.0.clone()) else { return Ok(None); };
    let Some(value) = text.strip_suffix(" FPS").and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0) else { return Ok(None); };
    let Some(more) = visible(world, |name: &Name| name.as_str() == "更多操作") else { return Ok(None); };
    let Some(drag) = visible(world, |item: &WorkbenchHeaderItem| *item == WorkbenchHeaderItem::Spacer) else { return Ok(None); };
    let parent = |entity| world.get::<ChildOf>(entity).map(ChildOf::parent);
    let header = parent(fps).filter(|header| world.get::<WorkbenchHeader>(*header).is_some())
        .ok_or("编辑器帧率读数没有进入原工作台顶栏")?;
    if parent(more) != Some(header) || parent(drag) != Some(header) {
        return Err("帧率、更多操作与拖动区不在同一标题栏".into());
    }
    let rect = |entity| {
        let size = world.get::<ComputedNode>(entity)?.size();
        let center = world.get::<UiGlobalTransform>(entity)?.translation;
        Some(Rect::from_center_size(center, size))
    };
    let (Some(fps_rect), Some(more_rect), Some(drag_rect), Some(header_rect)) =
        (rect(fps), rect(more), rect(drag), rect(header)) else { return Ok(None); };
    // 使用布局后的物理边界，防止标签误入拖动区或覆盖更多操作命中框。
    if more_rect.max.x > fps_rect.min.x + 1.0 || fps_rect.max.x > drag_rect.min.x + 1.0
        || fps_rect.min.y < header_rect.min.y - 1.0 || fps_rect.max.y > header_rect.max.y + 1.0
    {
        return Err(format!("标题栏帧率布局越界：{more_rect:?} / {fps_rect:?} / {drag_rect:?}"));
    }
    Ok(Some(json!({
        "hidden_by_default":true,"label":text,"fps":value,
        "more_right_px":more_rect.max.x,"fps_rect_px":[fps_rect.min.to_array(),fps_rect.max.to_array()],
        "drag_left_px":drag_rect.min.x,
    })))
}

fn visible<T: Component>(world: &mut World, matches: impl Fn(&T) -> bool) -> Option<Entity> {
    world.query::<(Entity, &T, &Node, &ComputedNode, Option<&InteractionDisabled>, Option<&InheritedVisibility>)>()
        .iter(world).find_map(|(entity, marker, node, layout, disabled, visible)| {
            (matches(marker) && node.display != Display::None && layout.size().min_element() > 1.0
                && disabled.is_none() && visible.is_none_or(|value| value.get())).then_some(entity)
        })
}

fn click<T: Component>(world: &mut World, run: &mut Run, window: Entity, matches: impl Fn(&T) -> bool) -> bool {
    let entity = run.click.map(|(entity, _)| entity).or_else(|| visible(world, matches));
    click_entity(world, run, window, entity)
}

fn click_entity(world: &mut World, run: &mut Run, window: Entity, entity: Option<Entity>) -> bool {
    let target = run.click.map(|(entity, _)| entity).or(entity);
    click_window_entity(world, &mut run.click, window, target)
}

fn asset_tools_layout(world: &mut World) -> Result<Option<Value>, String> {
    let mut positions = Vec::new();
    let mut row_bottom = f32::NEG_INFINITY;
    for action in [
        TooltipAction::AssetSearch, TooltipAction::AssetMissingCheck,
        TooltipAction::AssetFavorites, TooltipAction::AssetRecent,
        TooltipAction::Editor(EditorAction::RefreshAssets), TooltipAction::AssetThumbnails,
    ] {
        let Some(entity) = visible(world, |target: &EditorTooltipTarget| target.0 == action) else { return Ok(None); };
        let center = world.get::<UiGlobalTransform>(entity).unwrap().translation;
        row_bottom = row_bottom.max(center.y + world.get::<ComputedNode>(entity).unwrap().size().y / 2.0);
        positions.push(center.to_array());
    }
    if !positions.windows(2).all(|pair| pair[0][0] < pair[1][0] && (pair[0][1] - pair[1][1]).abs() <= 1.0) {
        return Err("六个资产工具没有在窄栏依序排在同一行".into());
    }
    let Some(help) = visible(world, |button: &ContextHelpButton| button.0 == TOOLS_HELP) else { return Ok(None); };
    let help_x = world.get::<UiGlobalTransform>(help).unwrap().translation.x;
    if positions.last().is_some_and(|last| help_x <= last[0]) {
        return Err("窄栏工程工具问号没有放在整行工具右侧".into());
    }
    let Some(path) = visible(world, |_: &SceneEditorAssetFolderText| true) else { return Ok(None); };
    let path_top = world.get::<UiGlobalTransform>(path).unwrap().translation.y
        - world.get::<ComputedNode>(path).unwrap().size().y / 2.0;
    if path_top + 1.0 < row_bottom {
        return Err("资产路径没有独占工具栏下方的一行".into());
    }
    Ok(Some(json!(positions)))
}

fn click_filename_filter(world: &mut World, run: &mut Run, window: Entity) -> bool {
    let Some(input) = visible(world, |_: &ProjectFileFilter| true) else { return false; };
    let position = world.get::<UiGlobalTransform>(input).unwrap().translation.as_dvec2();
    move_cursor(world, window, position);
    // 文件名框使用原生文本焦点，驱动真实指针事件并在后续帧读取焦点。
    match run.click {
        None => run.click = Some((input, false)),
        Some((_, false)) => {
            mouse(world, window, ButtonState::Pressed);
            run.click = Some((input, true));
        }
        Some((_, true)) => {
            mouse(world, window, ButtonState::Released);
            run.click = None;
            return true;
        }
    }
    false
}

fn filename_filter_has_focus(world: &World) -> bool {
    world.resource::<InputFocus>().get().is_some_and(|entity| world.get::<ProjectFileFilter>(entity).is_some())
}

fn key(world: &mut World, window: Entity, key_code: KeyCode, logical_key: Key, text: Option<&str>) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        window_event(world, KeyboardInput {
            key_code, logical_key: logical_key.clone(), state, window, repeat: false,
            text: text.filter(|_| state == ButtonState::Pressed).map(Into::into),
        });
    }
}

fn clear_filename_filter(world: &mut World, window: Entity, query: &str) {
    for _ in query.chars() {
        key(world, window, KeyCode::Backspace, Key::Backspace, None);
    }
}

fn mouse(world: &mut World, window: Entity, state: ButtonState) {
    window_event(world, MouseButtonInput { window, button: MouseButton::Left, state });
}

fn choose_filter(world: &mut World, run: &mut Run, window: Entity, target: AssetTypeFilter) -> bool {
    let option = visible(world, |option: &ProjectTypeFilterOption| option.0 == target);
    let opening = run.click.is_some_and(|(entity, _)| world.get::<ProjectTypeFilterButton>(entity).is_some());
    if opening || option.is_none() {
        click(world, run, window, |_: &ProjectTypeFilterButton| true);
        false
    } else {
        click_entity(world, run, window, option)
    }
}

fn narrow(world: &mut World, run: &mut Run, window: Entity) -> bool {
    if let Some(origin) = run.splitter_x {
        let physical_scale = world.get::<Window>(window).unwrap().scale_factor() * world.resource::<UiScale>().0;
        // 真实分隔条的下限由生产布局代码裁剪，验收只提供鼠标移动量。
        move_cursor(world, window, origin - DVec2::new(180.0 * f64::from(physical_scale), 0.0));
        if world.resource::<SceneEditorLayoutState>().content_width > EDITOR_CONTENT_MIN_WIDTH + 1.0 { return false; }
        mouse(world, window, ButtonState::Released);
        run.splitter_x = None;
        return true;
    }
    let Some(entity) = visible(world, |splitter: &SceneEditorPaneSplitter| splitter.0 == EditorPane::Content) else {
        return false;
    };
    let center = world.get::<UiGlobalTransform>(entity).unwrap().affine().translation.as_dvec2();
    move_cursor(world, window, center);
    if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Hovered) {
        mouse(world, window, ButtonState::Pressed);
        run.splitter_x = Some(center);
    }
    false
}

fn narrow_menu(world: &mut World, window: Entity) -> Option<Value> {
    let pane = visible(world, |pane: &SceneEditorPane| pane.0 == EditorPane::Content)?;
    let width = world.get::<ComputedNode>(pane)?.size().x;
    let window = world.get::<Window>(window)?;
    let scale = window.scale_factor() * world.resource::<UiScale>().0;
    let bounds = Vec2::new(window.physical_width() as f32, window.physical_height() as f32);
    if width / scale > EDITOR_CONTENT_MIN_WIDTH + 2.0 { return None; }
    let options = world.query::<(&ProjectTypeFilterOption, &ComputedNode, &UiGlobalTransform, &Node)>()
        .iter(world)
        .filter(|(_, size, _, node)| size.size().min_element() > 1.0 && node.display != Display::None)
        .map(|(option, size, transform, _)| (option.0, Rect::from_center_size(transform.affine().translation, size.size())))
        .collect::<Vec<_>>();
    if options.len() != 9 || options.iter().any(|(_, rect)| {
        rect.min.cmplt(Vec2::ZERO).any() || rect.max.cmpgt(bounds + Vec2::ONE).any()
    }) { return None; }
    Some(json!({
        "sidebar_logical_width":width / scale, "window_physical_size":bounds.to_array(),
        "options":options.iter().map(|(filter, rect)| json!({
            "filter":format!("{filter:?}"),"min":rect.min.to_array(),"max":rect.max.to_array()
        })).collect::<Vec<_>>(),
    }))
}

fn capture(world: &mut World, run: &mut Run, filename: &str) -> Result<bool, String> {
    capture_with_text(world, run, filename, None)
}

fn capture_with_text(world: &mut World, run: &mut Run, filename: &str, region: Option<URect>) -> Result<bool, String> {
    if let Some(proof) = evidence::capture(world, &mut run.capture, &run.work.join(filename), region)? {
        run.screenshots.insert(filename.into(), proof);
        return Ok(true);
    }
    Ok(false)
}
