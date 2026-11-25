//! 工程级规则从交互侧栏进入既有中央文档，首次创建独占发布，后续编辑复用正文草稿和保存回执。
use crate::{
    SceneEditorSystemSet, asset_catalog::project_files::EditorProjectFileCatalog,
    resources::SceneEditorState, ui::file_manager::OpenManagedFile,
    ui::asset_preview::documents::{DocumentState, show_project_rules_form},
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    text::FontSourceTemplate,
};
use feathrise_editor_documents_model::project_rules::{EMPTY, EXAMPLE, PATH};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, colors, metrics};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Component, Clone, Copy)]
pub(crate) struct RuleButton(pub(crate) bool);

fn button(label: &'static str, sample: bool) -> impl Scene {
    bsn! { InteractionButton ~{RuleButton(sample)}
        Node { height: px(24), padding: UiRect::horizontal(px(7)), align_items: AlignItems::Center, border: px(1) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [Text(label) @feathrise_editor_ui::localization::label_scene(label) Pickable::IGNORE
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)]
    }
}

pub(super) fn panel() -> impl Scene {
    bsn! { Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
        Children [
            @crate::ui::context_help::section_header("工程变量与触发规则", "在表单中添加变量、地点和规则。规则可监听变量变化或游戏事件，并设置变量、启动任务或播放演出。按 Ctrl+S 校验并保存。")--
            Node { width: percent(100), flex_wrap: FlexWrap::Wrap, column_gap: px(4), row_gap: px(3) }
            Children [@button("编辑变量与规则", false)-- @button("从示例创建", true)]
        ]
    }
}

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
    app.add_message::<OpenManagedFile>()
        .add_systems(Update, drive.in_set(SceneEditorSystemSet::Input));
}

fn drive(
    mut job: Local<Option<OpenTask>>,
    buttons: Query<(&EditorButtonClick, &RuleButton), Changed<EditorButtonClick>>,
    catalog: Option<Res<EditorProjectFileCatalog>>,
    mut editor: ResMut<SceneEditorState>,
    mut documents: ResMut<DocumentState>,
    mut open: MessageWriter<OpenManagedFile>,
) {
    let current = || Context::current(catalog.as_deref());
    if let Some(active) = job.as_mut() {
        let valid =
            editor.open && !editor.close_requested && current().as_ref() == Some(&active.context);
        if !valid {
            active.cancelled.store(true, Ordering::Release);
        }
        let Some(result) = check_ready(&mut active.work) else {
            return;
        };
        if valid && !active.cancelled.load(Ordering::Acquire) {
            match result {
                Ok((path, created)) => {
                    // 已开的源码标签仍持有未存草稿；从交互入口返回表单时继续使用这份文档。
                    show_project_rules_form(&mut documents, &path);
                    open.write(OpenManagedFile { path });
                    editor.status = if created {
                        "已创建工程规则文件"
                    } else {
                        "已找到工程规则文件"
                    }
                    .into();
                }
                Err(error) => editor.status = error,
            }
        }
        *job = None;
    }
    let Some((_, button)) = buttons.iter().find(|(click, _)| click.is_clicked()) else {
        return;
    };
    if !editor.open || editor.close_requested {
        return;
    }
    let Some(context) = current() else {
        editor.status = "请先打开要编辑规则的独立工程".into();
        return;
    };
    let root = context.project.clone();
    let assets = context.assets.clone();
    let sample = button.0;
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    *job = Some(OpenTask {
        context,
        cancelled,
        work: IoTaskPool::get()
            .spawn(async move { open_or_create(&root, &assets, sample, &cancel) }),
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
            let actual = path.canonicalize().map_err(|e| format!("无法定位工程规则：{e}"))?;
            if actual.parent() != Some(root) { return Err("工程规则已经离开主资产根".into()); }
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
    sample: bool,
    cancel: &AtomicBool,
) -> Result<(PathBuf, bool), String> {
    let root = checked_assets(project, assets)?;
    let path = root.join(PATH);
    if let Some(actual) = existing(&root, &path)? {
        return Ok((actual, false));
    }
    let content = if sample { EXAMPLE } else { EMPTY };
    let created = feathrise_editor_assets::files::atomic_create(&path, content.as_bytes(), || {
        if cancel.load(Ordering::Acquire) || checked_assets(project, assets)? != root {
            return Err("打开工程规则的上下文已改变".into());
        }
        Ok(())
    });
    match created {
        Ok(()) => Ok((path, true)),
        Err(error) => existing(&root, &path)?.map(|actual| (actual, false)).ok_or(error),
    }
}
