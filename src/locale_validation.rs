//! 真实窗口双语专项，验证首页、作者进程、文档草稿和重开。只操作本轮独占夹具。

use crate::workspace_ui::{WorkspaceCommand, WorkspaceModel};
use bevy::{
    input::{ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    text::TextLayoutInfo,
    ui::InteractionDisabled,
    window::PrimaryWindow,
};
use feathrise_editor_core::{EditorLocalePreference, EditorSettings};
use feathrise_editor_map::standalone::validation_fixture as documents;
use feathrise_editor_ui::localization::{EditorLocale, EditorSystemLocale};
use serde_json::{Value, json};
use std::{fs, io::Write, path::{Path, PathBuf}, time::{Duration, Instant}};

const OWNER: &[u8] = b"feathrise-locale-validation-v1";
const ENV: &str = "FEATHRISE_LOCALE_VALIDATION_DIR";

#[path = "locale_validation_details.rs"]
mod details;
use details::DetailStage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    HomeStartup, HomeChineseSettings, HomeSelectChinese, HomeCloseChineseSettings,
    HomeNewChinese, HomeDefaultChinese, HomeCloseNewChinese,
    HomeSettings, SelectEnglish, HomeCapture, HomeCloseSettings, WaitForAuthor, Returned,
    HomeNewEnglish, HomeDefaultEnglish, HomeCloseNewEnglish,
    MeshTab, MeshCollapsed, SelectMeshMap, EnterMeshMap, MeshTabAfterMap, MeshExpand, MeshExpanded, ProjectTab, ProjectContext, ProjectContextCapture,
    AssetRoot, AssetContext, AssetFavorite, AssetCapture, OpenShortcuts, ShowShortcuts, ShortcutsCapture, CloseShortcuts,
    BuildMenu, BuildBackend, StartBuild, StopTooltip, OpenBuildDetails, BuildDetails, CloseBuildDetails,
    QuitBuildConfirm, QuitBuildCapture,
    StopBuild, RestoreEditor3d, OpenDocument,
    Details(DetailStage),
    AuthorEnglish, AuthorCapture, LoadDocument, DocumentCapture, AuthorSettings, SelectChinese, AuthorChinese, DiscardDraft,
    Reopen, Quit, Done,
}

#[derive(Resource)]
struct Run {
    root: PathBuf,
    panels: bool,
    details: bool,
    quit_build: bool,
    author_prefs_existed: bool,
    stage: Stage,
    started: Instant,
    mouse: Option<(Entity, bool)>,
    draft: Option<Value>,
    capture: Option<async_channel::Receiver<Result<bool, String>>>,
    capture_attempts: u8,
    next_capture: Instant,
}

pub(crate) fn install(app: &mut App, project: Option<&Path>) -> Result<(), String> {
    let Some(root) = std::env::var_os(ENV).map(PathBuf::from) else { return Ok(()) };
    if !root.is_absolute() || !fs::symlink_metadata(&root).is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink()) {
        return Err("语言验收要求本轮自有绝对目录".into());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let pref = std::env::var_os("FEATHRISE_PREFERENCES_FILE").map(PathBuf::from);
    let plugins = std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").map(PathBuf::from);
    if fs::read(root.join(".locale-owner")).ok().as_deref() != Some(OWNER)
        || !pref.is_some_and(|path| path.canonicalize().ok() == Some(root.join("prefs.json")))
        || !plugins.is_some_and(|path| path.canonicalize().ok() == Some(root.join("plugins")))
        || fs::read(root.join("plugins/.feathrise-plugin-fixture-owner")).ok().as_deref()
            != Some(b"feathrise-single-binary-validation-v1")
    {
        return Err("语言验收的个人配置、插件目录或归属标记未隔离".into());
    }
    let fixture = root.join("project");
    let assets = fixture.join("assets");
    if fixture.canonicalize().ok().as_ref() != Some(&fixture)
        || assets.canonicalize().ok().as_ref() != Some(&assets)
        || fs::read(assets.join(documents::CLOSE_DOCUMENT_PATH)).ok().as_deref() != Some(OWNER)
    {
        return Err("语言验收的工程及普通文本必须属于本轮夹具".into());
    }
    let reopen = std::env::var_os("FEATHRISE_LOCALE_VALIDATION_REOPEN").is_some();
    let panels = std::env::var("FEATHRISE_LOCALE_VALIDATION_PANELS").is_ok_and(|value| value == "1");
    let details = std::env::var("FEATHRISE_LOCALE_VALIDATION_DETAILS").is_ok_and(|value| value == "1");
    let quit_build = std::env::var("FEATHRISE_LOCALE_VALIDATION_QUIT_BUILD").is_ok_and(|value| value == "1");
    if quit_build && (!cfg!(windows) || !panels || details || reopen) {
        return Err("退出构建语言验收要求 Windows 上独立开启面板专项".into());
    }
    let stage = if let Some(project) = project {
        if project != fixture || root.join("author.json").exists() {
            return Err("拒绝在非本轮普通文档夹具上验证语言".into());
        }
        documents::install_locale_document_probe(app);
        Stage::AuthorEnglish
    } else if reopen {
        if !root.join("exercise.json").is_file() || root.join("reopen.json").exists() {
            return Err("重开验收需使用刚通过的本轮配置".into());
        }
        Stage::Reopen
    } else {
        if root.join("exercise.json").exists() || root.join("home-en.png").exists() {
            return Err("语言验收目录已有证据".into());
        }
        // 只向已核验的新夹具写正式地图格式，正常预检与打开都走生产入口。
        if fs::read_dir(&assets).map_err(|error| error.to_string())?.count() != 1 {
            return Err("语言验收需要新的空资产根和本轮文本夹具".into());
        }
        feathrise_project::seed::create(&assets, "语言验收地图")?;
        if details {
            details::prepare_plugin(&root)?;
        }
        Stage::HomeStartup
    };
    let now = Instant::now();
    let author_prefs_existed = root.join("prefs.author.json").is_file();
    app.insert_resource(Run {
        root, panels, details, quit_build, author_prefs_existed, stage, started: now, mouse: None, draft: None,
        capture: None, capture_attempts: 0, next_capture: now,
    })
        .add_systems(First, drive);
    Ok(())
}

/// 复用代码迭代专项的最后退出门。真实后台任务仍由生产路径取消并回收。
#[cfg(windows)]
pub(crate) fn close_drain_held(world: &World) -> bool {
    world.get_resource::<Run>().is_some_and(|run| {
        run.quit_build && matches!(run.stage, Stage::QuitBuildConfirm | Stage::QuitBuildCapture)
    })
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        if run.stage == Stage::Done {
            return;
        }
        let stage = run.stage;
        if let Err(error) = advance(world, &mut run) {
            let status = world.get_resource::<WorkspaceModel>().map(|model| model.status.as_str()).unwrap_or_default();
            let status = status.to_owned();
            let visible: Vec<_> = world.query::<(&Text, &InheritedVisibility, &ComputedNode, &TextLayoutInfo)>()
                .iter(world).filter(|(_, visible, node, layout)| visible.get() && !node.is_empty() && !layout.glyphs.is_empty())
                .take(60).map(|(text, _, _, _)| text.0.clone()).collect();
            let _ = report(&run, &format!("failure-{:?}", run.stage), json!({"error":error,"status":status,"visible":visible}));
            error!("Locale validation {:?}: {error}. Workspace status: {status}", run.stage);
            run.stage = Stage::Done;
            world.write_message(AppExit::error());
        } else if run.stage != stage {
            run.started = Instant::now();
        }
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<(), String> {
    // 宿主给作者进程留下独立的完整取证期限，避免先退出导致丢失失败阶段。
    let timeout = if run.stage == Stage::WaitForAuthor { 90 } else { 32 };
    if run.started.elapsed() > Duration::from_secs(timeout) {
        return Err("等待生产窗口、文案或截图回执超时".into());
    }
    let Some(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next() else { return Ok(()) };
    let locale = *world.resource::<EditorLocale>();
    let preference = world.resource::<EditorSettings>().ui_locale;
    let system = world.resource::<EditorSystemLocale>().0;
    let home_scale = if run.author_prefs_existed { 1.5 } else { 1.0 };
    let stage = run.stage;
    match stage {
        Stage::HomeStartup if visible_text(world, system.editor_label("新建工程…")) => {
            // 分别核对配置档和已排版的系统语言，避免中文 Windows 上把手动中文误判为跟随系统。
            if preference != EditorLocalePreference::System || locale != system {
                return Err("无语言记录时未采用跟随系统档".into());
            }
            let effective = if locale == EditorLocale::Chinese { "zh-CN" } else { "en-US" };
            report(run, "startup", json!({"passed":true,"author_preferences_existed":run.author_prefs_existed,
                "preference":preference.as_str(),"effective":effective}))?;
            run.stage = match (run.panels, locale) {
                (true, EditorLocale::Chinese) => Stage::HomeNewChinese,
                (true, EditorLocale::English) => Stage::HomeChineseSettings,
                (false, _) => Stage::HomeSettings,
            };
        }
        Stage::HomeChineseSettings => {
            if click(world, run, window, "Settings") { run.stage = Stage::HomeSelectChinese; }
        }
        Stage::HomeSelectChinese => {
            if click(world, run, window, "简体中文") { run.stage = Stage::HomeCloseChineseSettings; }
        }
        Stage::HomeCloseChineseSettings if locale == EditorLocale::Chinese => {
            if click(world, run, window, "完成") { run.stage = Stage::HomeNewChinese; }
        }
        Stage::HomeNewChinese => {
            if click(world, run, window, "新建工程…") { run.stage = Stage::HomeDefaultChinese; }
        }
        Stage::HomeDefaultChinese if visible_text(world, "工程名称") && default_name_visible(world) => {
            if capture(world, run, "new-project-zh")? { run.stage = Stage::HomeCloseNewChinese; }
        }
        Stage::HomeCloseNewChinese => {
            if click(world, run, window, "取消") { run.stage = Stage::HomeSettings; }
        }
        Stage::HomeSettings if visible_text(world, locale.editor_label("新建工程…")) => {
            world.get_mut::<Window>(window).unwrap().resolution.set(900., 720.);
            if click(world, run, window, locale.editor_label("设置")) { run.stage = Stage::SelectEnglish; }
        }
        Stage::SelectEnglish => {
            if click(world, run, window, "English") { run.stage = Stage::HomeCapture; }
        }
        Stage::HomeCapture if locale == EditorLocale::English
            && preference == EditorLocalePreference::English && visible_text(world, "Language")
            && visible_text(world, "Project ▼") => {
            let scale = world.resource::<UiScale>().0;
            let width = world.get::<Window>(window).unwrap().width();
            if (scale - home_scale).abs() < 0.001 && width <= 901. && capture(world, run, "home-en")? {
                run.stage = Stage::HomeCloseSettings;
            }
        }
        Stage::HomeCloseSettings => {
            if click(world, run, window, "Done") {
                // 窄窗证据已经落盘，工程阶段展开两栏以核对真实作者页签和文档。
                world.get_mut::<Window>(window).unwrap().resolution.set(1280., 900.);
                // 宿主只有收到真实个人偏好写入回执，才会启动读取它的候选作者。
                if run.details {
                    run.stage = Stage::Details(DetailStage::PluginMenu);
                } else if run.panels {
                    run.stage = Stage::HomeNewEnglish;
                } else {
                    world.write_message(WorkspaceCommand::Open(run.root.join("project")));
                    run.stage = Stage::WaitForAuthor;
                }
            }
        }
        Stage::HomeNewEnglish => {
            if click(world, run, window, "New Project…") { run.stage = Stage::HomeDefaultEnglish; }
        }
        Stage::HomeDefaultEnglish if visible_text(world, "Project Name") && default_name_visible(world) => {
            if capture(world, run, "new-project-en")? { run.stage = Stage::HomeCloseNewEnglish; }
        }
        Stage::HomeCloseNewEnglish => {
            if click(world, run, window, "Cancel") {
                world.write_message(WorkspaceCommand::Open(run.root.join("project")));
                run.stage = Stage::WaitForAuthor;
            }
        }
        Stage::WaitForAuthor if run.root.join("author.json").is_file()
            && world.resource::<WorkspaceModel>().project.as_ref().is_some_and(|project| project.root == run.root.join("project")) => {
            if !world.get::<Window>(window).unwrap().title.starts_with("保存 · ") {
                return Err("工程标题未原样显示自有中文工程名".into());
            }
            world.write_message(WorkspaceCommand::CloseProject);
            run.stage = Stage::Returned;
        }
        Stage::Returned if world.resource::<WorkspaceModel>().project.is_none()
            && locale == EditorLocale::Chinese && visible_text(world, "新建工程…") => {
            let author: Value = serde_json::from_slice(&fs::read(run.root.join("author.json")).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
            let preferences = read_saved(run)?;
            if preferences["editor"]["ui_locale"] != "zh-CN" {
                return Err("返回首页未读到作者保存的中文偏好".into());
            }
            report(run, "exercise", json!({"passed":true,"author":author,"preferences":preferences,"home_scale":home_scale,"project_display_name":"保存"}))?;
            world.write_message(WorkspaceCommand::QuitApp);
            run.stage = Stage::Quit;
        }
        Stage::AuthorEnglish if locale == EditorLocale::English
            && preference == EditorLocalePreference::English
            && visible_text(world, "Maps") && visible_text(world, "Scene")
            && visible_text(world, "Inspector") && visible_text(world, "保存 ▼") => {
            if read_saved(run)?["editor"]["ui_locale"] != "en-US" {
                return Err("作者未从真实落盘偏好继承英文".into());
            }
            run.stage = Stage::AuthorCapture;
        }
        Stage::AuthorCapture => {
            if capture(world, run, "author-en")? {
                run.stage = if run.details {
                    Stage::Details(DetailStage::OpenSettings)
                } else if run.panels {
                    Stage::BuildMenu
                } else {
                    Stage::OpenDocument
                };
            }
        }
        Stage::Details(detail) => details::advance(world, run, window, detail)?,
        Stage::MeshTab => {
            if click(world, run, window, "Mesh") { run.stage = Stage::MeshCollapsed; }
        }
        Stage::MeshCollapsed if visible_text(world, "Add Mesh ▶") => {
            if capture(world, run, "mesh-collapsed-en")? { run.stage = Stage::SelectMeshMap; }
        }
        Stage::SelectMeshMap => {
            if click(world, run, window, "空地图") { run.stage = Stage::EnterMeshMap; }
        }
        Stage::EnterMeshMap => {
            // 双击会先显示地图属性，进入后再切回原右栏网格页。
            if click(world, run, window, "空地图") { run.stage = Stage::MeshTabAfterMap; }
        }
        Stage::MeshTabAfterMap if visible_containing(world, "Current Map: 空地图") => {
            if click(world, run, window, "Mesh") { run.stage = Stage::MeshExpand; }
        }
        Stage::MeshExpand => {
            if click(world, run, window, "Add Mesh ▶") { run.stage = Stage::MeshExpanded; }
        }
        Stage::MeshExpanded if visible_text(world, "Add Mesh ▼") => {
            if visible_text(world, "Size X") && visible_text(world, "Red") && visible_text(world, "Roughness")
                && capture(world, run, "mesh-expanded-en")? { run.stage = Stage::ProjectTab; }
        }
        Stage::ProjectTab => {
            if click(world, run, window, "Project") { run.stage = Stage::ProjectContext; }
        }
        Stage::ProjectContext => {
            if pointer_click(world, run, window, "editor-project.json", MouseButton::Right) {
                run.stage = Stage::ProjectContextCapture;
            }
        }
        Stage::ProjectContextCapture if visible_text(world, "Copy Relative Path")
            && visible_text(world, "Copy Absolute Path") && visible_text(world, "Open Containing Folder") => {
            if capture(world, run, "project-context-en")? {
                escape(world, window);
                run.stage = Stage::AssetRoot;
            }
        }
        Stage::AssetRoot if !visible_text(world, "Copy Relative Path") => {
            if click(world, run, window, "assets") { run.stage = Stage::AssetContext; }
        }
        Stage::AssetContext if visible_text(world, "Browse Folders") => {
            if pointer_click(world, run, window, "maps", MouseButton::Right) { run.stage = Stage::AssetFavorite; }
        }
        Stage::AssetFavorite => {
            if click(world, run, window, "Add to Favorites") { run.stage = Stage::AssetCapture; }
        }
        Stage::AssetCapture if ["Search Contents", "Check Missing Assets", "Thumbnails", "Check Paths",
            "▼ Favorites (1)", "[Folder] maps"]
            .into_iter().all(|label| visible_text(world, label))
            && visible_containing(world, "▶ Recent · ")
            && has_text(world, |text| text.starts_with("Project · ") && text.ends_with(" items")) => {
            if capture(world, run, "asset-browser-en")? { run.stage = Stage::OpenShortcuts; }
        }
        Stage::OpenShortcuts => {
            if click(world, run, window, "Settings") { run.stage = Stage::ShowShortcuts; }
        }
        Stage::ShowShortcuts => {
            if click(world, run, window, "Shortcuts") { run.stage = Stage::ShortcutsCapture; }
        }
        Stage::ShortcutsCapture if ["Viewport Navigation", "Frame Selected", "Map Editing", "Add Mesh",
            "Brush Settings", "Decrease Brush Radius", "Increase Brush Strength"]
            .into_iter().all(|label| visible_text(world, label)) => {
            if capture(world, run, "shortcuts-en")? { run.stage = Stage::CloseShortcuts; }
        }
        Stage::CloseShortcuts => {
            if click(world, run, window, "Done") { run.stage = Stage::OpenDocument; }
        }
        Stage::BuildMenu => {
            if click(world, run, window, "Debug · DX ▼") { run.stage = Stage::BuildBackend; }
        }
        Stage::BuildBackend => {
            if click(world, run, window, "Native Cargo") { run.stage = Stage::StartBuild; }
        }
        Stage::StartBuild => {
            if click(world, run, window, "Project 3D") { run.stage = Stage::StopTooltip; }
        }
        Stage::StopTooltip if run.quit_build && visible_text(world, "Stop")
            && fs::read(run.root.join("project/.locale-build-started")).ok().as_deref() == Some(OWNER) => {
            world.write_message(WorkspaceCommand::QuitApp);
            run.stage = Stage::QuitBuildConfirm;
        }
        Stage::QuitBuildConfirm if click(world, run, window, "Quit Editor") => {
            run.stage = Stage::QuitBuildCapture;
        }
        Stage::QuitBuildCapture => {
            let status = "Stopping the project and its builds before quitting the editor…";
            if visible_text(world, status) && capture(world, run, "quit-build-en")? {
                report(run, "quit-build", json!({"passed":true,"status":status,"real_build_started":true,
                    "final_exit_held_for_screenshot":true}))?;
                run.stage = Stage::Done;
            }
        }
        Stage::StopTooltip if run.root.join("project/.locale-build-started").is_file() => {
            if let Some(entity) = button(world, "Stop") {
                move_to(world, window, entity);
                if visible_containing(world, "Stop the current Project 3D build.")
                    && capture(world, run, "stop-build-en")? {
                    run.stage = Stage::OpenBuildDetails;
                }
            }
        }
        Stage::OpenBuildDetails => {
            if click(world, run, window, "Details") { run.stage = Stage::BuildDetails; }
        }
        Stage::BuildDetails if visible_text(world, "Status Details")
            && visible_containing(world, "Building the project. The first build may take a while.")
            && visible_containing(world, "Project 3D: Starting")
            && visible_containing(world, "Saving code starts a build")
            && (visible_containing(world, "Code builds automatically when you save source files.")
                || visible_containing(world, "Code: Debug / Native Cargo. Saving source files starts a build.")) => {
            if capture(world, run, "build-status-en")? { run.stage = Stage::CloseBuildDetails; }
        }
        Stage::CloseBuildDetails => {
            if click(world, run, window, "Details") {
                run.stage = if run.details { Stage::Details(DetailStage::OpenLog) } else { Stage::StopBuild };
            }
        }
        Stage::StopBuild => {
            if click(world, run, window, "Stop") { run.stage = Stage::RestoreEditor3d; }
        }
        Stage::RestoreEditor3d if visible_text(world, "Preview")
            && world.resource::<crate::viewport_mode::ViewportModeState>().selected == crate::viewport_mode::ViewportMode::EmbeddedWorld => {
            if click(world, run, window, "Editor 3D") {
                run.stage = if run.details { Stage::Details(DetailStage::Editor3d) } else { Stage::MeshTab };
            }
        }
        Stage::OpenDocument => {
            documents::request_close_document(world)?;
            run.stage = Stage::LoadDocument;
        }
        Stage::LoadDocument if documents::close_document_ready(world)? => {
            run.draft = Some(documents::edit_close_document(world)?);
            run.stage = Stage::DocumentCapture;
        }
        Stage::DocumentCapture if visible_text(world, "Document · feathrise-close-validation.txt *")
            && visible_text(world, "Return to Sidebar") && visible_text(world, "Reload") => {
            if capture(world, run, "document-en")? {
                run.stage = Stage::AuthorSettings;
            }
        }
        Stage::AuthorSettings => {
            if click(world, run, window, "Settings") { run.stage = Stage::SelectChinese; }
        }
        Stage::SelectChinese => {
            if click(world, run, window, "简体中文") { run.stage = Stage::AuthorChinese; }
        }
        Stage::AuthorChinese if locale == EditorLocale::Chinese
            && preference == EditorLocalePreference::Chinese && visible_text(world, "界面语言") => {
            if documents::close_document_snapshot(world) != run.draft {
                return Err("切换语言改变了作者文档草稿或历史".into());
            }
            if capture(world, run, "author-zh")? {
                documents::redo_locale_document(world)?;
                let edited = documents::close_document_snapshot(world).ok_or("重做后文档消失")?;
                report(run, "author", json!({"passed":true,"inherited":"en-US","switched":"zh-CN","panels_exercised":run.panels,
                    "details_exercised":run.details,"draft":run.draft,"after_redo":edited}))?;
                run.stage = Stage::DiscardDraft;
            }
        }
        Stage::DiscardDraft => {
            if click(world, run, window, "不保存并关闭") { run.stage = Stage::Done; }
        }
        Stage::Reopen if locale == EditorLocale::Chinese && visible_text(world, "新建工程…") => {
            if capture(world, run, "reopen-zh")? {
                let preferences = read_saved(run)?;
                if preferences["editor"]["ui_locale"] != "zh-CN" { return Err("重开偏好不匹配".into()); }
                report(run, "reopen", json!({"passed":true,"preferences":preferences}))?;
                world.write_message(WorkspaceCommand::QuitApp);
                run.stage = Stage::Quit;
            }
        }
        Stage::Quit => {
            if click(world, run, window, "退出编辑器") { run.stage = Stage::Done; }
        }
        _ => {}
    }
    Ok(())
}

fn visible_text(world: &mut World, label: &str) -> bool {
    has_text(world, |text| text == label)
}

fn visible_containing(world: &mut World, fragment: &str) -> bool {
    has_text(world, |text| text.contains(fragment))
}

fn has_text(world: &mut World, predicate: impl Fn(&str) -> bool) -> bool {
    world.query::<(&Text, &InheritedVisibility, &ComputedNode, &TextLayoutInfo)>()
        .iter(world).any(|(text, visible, node, layout)| {
            predicate(text.as_str()) && visible.get() && !node.is_empty() && !layout.glyphs.is_empty()
        })
}

fn default_name_visible(world: &mut World) -> bool {
    world.query::<(&bevy::text::EditableText, &InheritedVisibility, &ComputedNode)>()
        .iter(world).any(|(text, visible, node)| visible.get() && !node.is_empty() && text.value().to_string() == "NewProject")
}

fn button(world: &mut World, label: &str) -> Option<Entity> {
    let texts: Vec<_> = world.query::<(Entity, &Text)>().iter(world)
        .filter_map(|(entity, text)| (text.as_str().trim() == label).then_some(entity)).collect();
    let mut right_mesh: Option<(Entity, f32)> = None;
    for mut entity in texts {
        for _ in 0..24 {
            if world.get::<Button>(entity).is_some()
                && world.get::<InheritedVisibility>(entity).is_some_and(|visible| visible.get())
                && world.get::<ComputedNode>(entity).is_some_and(|node| !node.is_empty())
                && world.get::<InteractionDisabled>(entity).is_none()
            {
                // Mesh 工具和右侧 Mesh 标签同名，验收选用最右侧的真实标签按钮。
                if label != "Mesh" { return Some(entity); }
                if let Some(transform) = world.get::<UiGlobalTransform>(entity) {
                    let x = transform.translation.x;
                    if right_mesh.is_none_or(|(_, previous)| x > previous) { right_mesh = Some((entity, x)); }
                }
                break;
            }
            let Some(parent) = world.get::<ChildOf>(entity) else { break };
            entity = parent.parent();
        }
    }
    right_mesh.map(|(entity, _)| entity)
}

fn click(world: &mut World, run: &mut Run, window: Entity, label: &str) -> bool {
    pointer_click(world, run, window, label, MouseButton::Left)
}

fn pointer_click(world: &mut World, run: &mut Run, window: Entity, label: &str, mouse_button: MouseButton) -> bool {
    let candidate = button(world, label);
    click_entity(world, run, window, candidate, mouse_button)
}

fn click_entity(world: &mut World, run: &mut Run, window: Entity, candidate: Option<Entity>, mouse_button: MouseButton) -> bool {
    let Some(entity) = run.mouse.map(|(entity, _)| entity).or(candidate) else { return false };
    if !move_to(world, window, entity) { return false; }
    match run.mouse {
        None => run.mouse = Some((entity, false)),
        Some((_, false)) if world.get::<Interaction>(entity) == Some(&Interaction::Hovered) => {
            world.write_message(MouseButtonInput { button: mouse_button, state: ButtonState::Pressed, window });
            run.mouse = Some((entity, true));
        }
        Some((_, true)) if mouse_button == MouseButton::Right || world.get::<Interaction>(entity) == Some(&Interaction::Pressed) => {
            world.write_message(MouseButtonInput { button: mouse_button, state: ButtonState::Released, window });
            run.mouse = None;
            return true;
        }
        _ => {}
    }
    false
}

fn move_to(world: &mut World, window: Entity, entity: Entity) -> bool {
    let Some(transform) = world.get::<UiGlobalTransform>(entity) else { return false };
    let position = transform.translation.as_dvec2();
    world.get_mut::<Window>(window).unwrap().set_physical_cursor_position(Some(position));
    true
}

fn escape(world: &mut World, window: Entity) {
    press_key(world, window, KeyCode::Escape, Key::Escape);
}

fn press_key(world: &mut World, window: Entity, key_code: KeyCode, logical_key: Key) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        world.write_message(KeyboardInput { key_code, logical_key: logical_key.clone(),
            state, text: None, repeat: false, window });
    }
}

fn capture(world: &mut World, run: &mut Run, name: &str) -> Result<bool, String> {
    if let Some(receiver) = &run.capture {
        return match receiver.try_recv() {
            Ok(result) => {
                run.capture = None;
                if result.as_ref().is_ok_and(|accepted| *accepted) {
                    run.capture_attempts = 0;
                }
                result
            }
            Err(async_channel::TryRecvError::Empty) => {
                if Instant::now().saturating_duration_since(run.next_capture) > Duration::from_secs(12) {
                    Err(format!("{name} 截图未收到 RenderApp 回执"))
                } else {
                    Ok(false)
                }
            }
            Err(async_channel::TryRecvError::Closed) => Err("GPU截图回执丢失".into()),
        };
    }
    if Instant::now() < run.next_capture {
        return Ok(false);
    }
    if run.capture_attempts >= 16 {
        return Err(format!("{name} 未读回包含可见界面的 GPU 像素"));
    }
    run.capture_attempts += 1;
    run.next_capture = Instant::now() + Duration::from_millis(250);
    let path = run.root.join(format!("{name}.png"));
    let first_frame = run.root.join(format!("{name}-clear.png"));
    let (sender, receiver) = async_channel::bounded(1);
    run.capture = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        let image = event.image.clone();
        let path = path.clone();
        let first_frame = first_frame.clone();
        let sender = sender.clone();
        IoTaskPool::get().spawn(async move {
            let result = image.try_into_dynamic().map_err(|error| error.to_string())
                .and_then(|image| {
                    // 冷管线可能先返回清屏色，保留首张失败帧并等待实际 UI。
                    let pixels = image.to_rgba8();
                    let mut colors = std::collections::HashSet::new();
                    let rendered = pixels.pixels().step_by(11).any(|pixel| {
                        colors.insert(pixel.0);
                        colors.len() > 64
                    });
                    if rendered {
                        image.save(path).map_err(|error| error.to_string())?;
                    } else if !first_frame.exists() {
                        image.save(first_frame).map_err(|error| error.to_string())?;
                    }
                    Ok(rendered)
                });
            let _ = sender.send(result).await;
        }).detach();
    });
    Ok(false)
}

fn read_saved(run: &Run) -> Result<Value, String> {
    let bytes = fs::read(run.root.join("prefs.author.json")).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn report(run: &Run, name: &str, value: Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    fs::OpenOptions::new().write(true).create_new(true).open(run.root.join(format!("{name}.json")))
        .and_then(|mut file| file.write_all(&bytes)).map_err(|error| error.to_string())
}
