//! 工程交互与场景属性共用工程规则的独占创建、后台加载和文档回执。
use crate::{
    SceneEditorSystemSet,
    asset_catalog::project_files::EditorProjectFileCatalog,
    authoring::EditorAuthoringSession,
    components::SceneEditorPane,
    resources::SceneEditorState,
    types::EditorPane,
    ui::asset_preview::documents::{
        DocumentState, ProjectRulesSection, load_sidebar_project_rules_document,
        set_sidebar_project_rules_error, set_sidebar_project_rules_page, sidebar_project_rules_ready,
    },
    ui::sidebars::{EditorLeftSidebarTab, SceneConfigPage, SceneEditorSidebarState, current_scene_config_id},
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_editor_documents_model::project_rules::{EMPTY, PATH};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// 作者最近一次显式打开意图，连续切分区时共用同一个 I/O 槽。
#[derive(Resource, Default)]
pub(super) struct RuleOpenRequest(pub(super) bool);

#[derive(PartialEq, Eq)]
struct Context {
    project: PathBuf,
    assets: PathBuf,
    generation: Option<u64>,
}

impl Context {
    fn current(catalog: Option<&EditorProjectFileCatalog>) -> Option<Self> {
        let bound = feathrise_editor_core::editor_project_paths();
        Some(Self {
            project: catalog
                .and_then(|c| c.project_root())
                .or_else(|| bound.map(|p| p.project_root()))?
                .to_owned(),
            assets: catalog
                .and_then(|c| c.asset_root())
                .or_else(|| bound.map(|p| p.asset_root()))?
                .to_owned(),
            generation: catalog.and_then(|c| c.source_context()),
        })
    }
}

struct OpenTask {
    context: Context,
    cancelled: Arc<AtomicBool>,
    work: Task<Result<(PathBuf, bool), String>>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<RuleOpenRequest>().add_systems(
        Update,
        drive
            .after(super::systems::handle_tabs)
            .before(SceneEditorSystemSet::Tasks),
    );
}

fn drive(
    mut job: Local<Option<OpenTask>>,
    mut request: ResMut<RuleOpenRequest>,
    sidebar: Res<SceneEditorSidebarState>,
    pages: Res<super::GameplaySidebarState>,
    authoring: Option<Res<EditorAuthoringSession>>,
    catalog: Option<Res<EditorProjectFileCatalog>>,
    panes: Query<(&SceneEditorPane, &Node)>,
    mut editor: ResMut<SceneEditorState>,
    mut documents: Option<ResMut<DocumentState>>,
) {
    let current = || Context::current(catalog.as_deref());
    let inspector = panes.iter().any(|(pane, node)| {
        pane.0 == EditorPane::Inspector && node.display != Display::None
    });
    let page = if !editor.open || editor.close_requested || !inspector {
        None
    } else if sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay) {
        pages.page.section().map(|tab| (tab, None))
    } else {
        let tab = match sidebar.scene_config_page() {
            Some(SceneConfigPage::Variables) => Some(ProjectRulesSection::Variables),
            Some(SceneConfigPage::Triggers) => Some(ProjectRulesSection::Rules),
            _ => None,
        };
        // 浏览选择不能成为写入身份，只有已采用的作者草稿才能打开当前场景规则。
        tab.and_then(|tab| {
            let scene = current_scene_config_id(&editor, authoring.as_deref()?)?;
            Some((tab, Some(scene)))
        })
    };
    if documents.as_mut().is_some_and(|docs| set_sidebar_project_rules_page(docs, page))
        && page.is_some()
    {
        // 若先前的打开已取消但尚未回收，记住重回页面的意图，等旧任务退出再重试。
        request.0 = true;
    }
    if let Some(active) = job.as_mut() {
        let valid = page.is_some() && current().as_ref() == Some(&active.context);
        if !valid {
            active.cancelled.store(true, Ordering::Release);
        } else if !active.cancelled.load(Ordering::Acquire) {
            request.0 = false; // 同一个文件正在准备，回执按当前工具选择表单。
        }
        let Some(result) = check_ready(&mut active.work) else {
            return;
        };
        if let Some(documents) = documents.as_deref_mut()
            && valid
            && !active.cancelled.load(Ordering::Acquire)
        {
            match result {
                Ok((path, created)) => {
                    // 加载与当前属性/交互页分离，后台文件回执不会改变中央文档或已选作用域。
                    editor.status =
                        match load_sidebar_project_rules_document(documents, &path) {
                            Ok(()) if created => "已创建工程规则文件".into(),
                            Ok(()) => "已找到工程规则文件".into(),
                            Err(error) => {
                                set_sidebar_project_rules_error(documents, error.clone());
                                error
                            }
                        };
                }
                Err(error) => {
                    set_sidebar_project_rules_error(documents, error.clone());
                    editor.status = error;
                }
            }
        }
        *job = None;
    }
    if !std::mem::take(&mut request.0) || page.is_none() {
        return;
    }
    let Some(documents) = documents.as_deref_mut() else {
        return;
    };
    // 同一个工程中选择其他表单直接复用已读的文档，首次读取才走磁盘。
    if sidebar_project_rules_ready(documents) {
        set_sidebar_project_rules_error(documents, String::new());
        return;
    }
    let Some(context) = current() else {
        editor.status = "请先打开要编辑规则的独立工程".into();
        set_sidebar_project_rules_error(documents, editor.status.clone());
        return;
    };
    set_sidebar_project_rules_error(documents, String::new());
    let root = context.project.clone();
    let assets = context.assets.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    editor.status = "正在打开工程规则…".into();
    *job = Some(OpenTask {
        context,
        cancelled,
        work: IoTaskPool::get()
            .spawn(async move { open_or_create(&root, &assets, &cancel) }),
    });
}

fn checked_assets(project: &Path, assets: &Path) -> Result<PathBuf, String> {
    let project = project
        .canonicalize()
        .map_err(|e| format!("无法核对工程目录：{e}"))?;
    let root = assets
        .canonicalize()
        .map_err(|e| format!("无法核对主资产根：{e}"))?;
    if !root.starts_with(project) || !root.is_dir() {
        return Err("当前主资产根已经离开工程目录".into());
    }
    Ok(root)
}

fn existing(root: &Path, path: &Path) -> Result<Option<PathBuf>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {
            // 文件浏览器使用磁盘上的真实拼写，两个入口必须找到同一份已打开的草稿。
            let actual = path
                .canonicalize()
                .map_err(|e| format!("无法定位工程规则：{e}"))?;
            if actual.parent() != Some(root) {
                return Err("工程规则已经离开主资产根".into());
            }
            Ok(Some(actual))
        }
        Ok(_) => Err("project_rules.json 必须是主资产根内的普通文件".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("无法读取工程规则：{e}")),
    }
}

/// 打开已有内容从不改写字节。并发首次创建时采用先发布的版本，完整写入后才通知文档编辑器。
pub(crate) fn open_or_create(
    project: &Path,
    assets: &Path,
    cancel: &AtomicBool,
) -> Result<(PathBuf, bool), String> {
    let root = checked_assets(project, assets)?;
    let path = root.join(PATH);
    if let Some(actual) = existing(&root, &path)? {
        return Ok((actual, false));
    }
    let created = feathrise_editor_assets::files::atomic_create(&path, EMPTY.as_bytes(), || {
        if cancel.load(Ordering::Acquire) || checked_assets(project, assets)? != root {
            return Err("打开工程规则的上下文已改变".into());
        }
        Ok(())
    });
    match created {
        Ok(()) => Ok((path, true)),
        Err(error) => existing(&root, &path)?
            .map(|actual| (actual, false))
            .ok_or(error),
    }
}
