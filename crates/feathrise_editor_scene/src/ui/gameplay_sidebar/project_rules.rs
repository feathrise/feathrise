//! 从右侧原位加载工程规则，首次创建独占发布，编辑复用文档草稿和保存回执。
use crate::{
    SceneEditorSystemSet,
    asset_catalog::project_files::EditorProjectFileCatalog,
    components::SceneEditorPane,
    resources::SceneEditorState,
    types::EditorPane,
    ui::asset_preview::documents::{
        DocumentState, open_sidebar_project_rules_section, select_loaded_sidebar_project_rules_section,
        set_sidebar_project_rules_error, set_sidebar_project_rules_visible,
    },
    ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState},
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
    catalog: Option<Res<EditorProjectFileCatalog>>,
    panes: Query<(&SceneEditorPane, &Node)>,
    mut editor: ResMut<SceneEditorState>,
    mut documents: Option<ResMut<DocumentState>>,
) {
    let current = || Context::current(catalog.as_deref());
    let visible = editor.open
        && !editor.close_requested
        && sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay);
    let section = pages.page.section().filter(|_| visible);
    if let Some(documents) = documents.as_mut() {
        let inspector = panes
            .iter()
            .any(|(pane, node)| pane.0 == EditorPane::Inspector && node.display != Display::None);
        set_sidebar_project_rules_visible(documents, inspector && section.is_some());
    }
    if let Some(active) = job.as_mut() {
        let valid = section.is_some() && current().as_ref() == Some(&active.context);
        if !valid {
            active.cancelled.store(true, Ordering::Release);
        } else if !active.cancelled.load(Ordering::Acquire) {
            request.0 = false; // 同一个文件正在准备，回执按当前工具选择表单。
        }
        let Some(result) = check_ready(&mut active.work) else {
            return;
        };
        if let (Some(section), Some(documents)) = (section, documents.as_deref_mut())
            && valid
            && !active.cancelled.load(Ordering::Acquire)
        {
            match result {
                Ok((path, created)) => {
                    // 交互入口只加载右侧工作区，中央已有的文档选择不参与。
                    editor.status =
                        match open_sidebar_project_rules_section(documents, &path, section) {
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
    let (true, Some(section)) = (std::mem::take(&mut request.0), section) else {
        return;
    };
    let Some(documents) = documents.as_deref_mut() else {
        return;
    };
    // 同一个工程中选择其他表单直接复用已读的文档，首次读取才走磁盘。
    if select_loaded_sidebar_project_rules_section(documents, section) {
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
