//! 「交互」在同一个工程规则草稿中新增带稳定演出 ID 的动作，保留已有规则、外部版本及保存回执。
use super::*;
use crate::ui::file_manager::OpenManagedFile;
use bevy::{
    input_focus::InputFocus,
    tasks::IoTaskPool,
    text::{EditableText, FontSourceTemplate},
};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, colors as c};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::atomic::AtomicBool};
#[derive(Component, Clone, Copy, PartialEq)]
pub(super) enum BindingField {
    Event,
    Target,
}
#[derive(Component, Clone, Default)]
pub(super) struct Bind;
#[derive(Component, Clone, Default)]
struct BoundFilm;
fn field(kind: BindingField, label: &'static str, initial: &'static str) -> impl Scene {
    bsn! {Node{width:percent(100),column_gap:px(3),height:px(24),flex_shrink:0.0,align_items:AlignItems::Center}
    Children[Text(label) TextColor(c::TEXT_MUTED) TextFont{font:FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),font_size:px(11)}
    -- ~{kind} crate::ui::text_input::EditorTextInput ~{bevy::input_focus::tab_navigation::TabIndex(0)} ~{bevy::ui::FocusPolicy::Block}
    bevy::text::TextCursorStyle::default() ~{EditableText{allow_newlines:false,..EditableText::new(initial)}}
    TextLayout::no_wrap() TextFont{font:FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),font_size:px(11)}
    TextColor(c::TEXT_VALUE) BackgroundColor(c::INPUT_BG) BorderColor::all(c::COPPER_BORDER_WEAK)
    Node{flex_grow:1.0,min_width:px(25),height:px(22),border:px(1),padding:UiRect::horizontal(px(3)),overflow:Overflow::clip()}]}
}
pub(crate) fn panel() -> impl Scene {
    bsn! {Node{width:percent(100),flex_direction:FlexDirection::Column,flex_shrink:0.0,row_gap:px(2)}
    Children[
    @crate::ui::context_help::section_header("剧情演出触发","打开一个剧情演出后，在此填写触发事件与可选目标。进入地点可用 location.entered 配合地点 ID，完成任务可用 quest.completed 配合任务 ID。确认会打开工程规则草稿，保存后在游戏生效。")--
    BoundFilm Text("请先双击一个剧情演出资产") TextColor(c::TEXT_MUTED) TextFont{font:FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),font_size:px(11)}
    -- @field(BindingField::Event,"事件","location.entered")-- @field(BindingField::Target,"目标","")--
    Bind InteractionButton Node{height:px(23),padding:UiRect::horizontal(px(5)),align_items:AlignItems::Center} BackgroundColor(c::SLOT_BG)
    Children[Text("将演出绑定到事件") TextColor(c::TEXT_PRIMARY) TextFont{font:FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),font_size:px(11)} Pickable::IGNORE]
    ]}
}
struct Intent {
    root: PathBuf,
    project: PathBuf,
    id: String,
    event: String,
    target: String,
}
struct Pending {
    intent: Intent,
    phase: Phase,
}
enum Phase {
    Open(Task<Result<(PathBuf, bool), String>>),
    Waiting(PathBuf),
    Edit {
        key: Identity,
        result: Task<Result<(Arc<str>, Vec<Arc<str>>), String>>,
    },
}
pub(super) fn insert(
    source: &str,
    event: &str,
    target: &str,
    id: &str,
    crlf: bool,
) -> Result<(Arc<str>, Vec<Arc<str>>), String> {
    let mut v: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
    let rules = v
        .get_mut("rules")
        .and_then(Value::as_array_mut)
        .ok_or("工程规则需要 rules 数组")?;
    let mut when = json!({"event":event});
    if !target.is_empty() {
        when["target"] = target.into();
    }
    let action = json!({"request":"cinematic.play","target":id});
    if rules
        .iter()
        .any(|r| r["when"] == when && r["actions"].as_array().is_some_and(|a| a.contains(&action)))
    {
        return Err("这个事件已经绑定到所选演出".into());
    }
    rules.push(json!({"id":format!("play_{}",bevy::asset::uuid::Uuid::new_v4().simple()),"when":when,"actions":[action]}));
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n";
    let data = if crlf {
        text.replace('\n', "\r\n")
    } else {
        text.clone()
    };
    Ok((text.into(), super::super::io::source_pages(&data)))
}
pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        drive
            .in_set(crate::SceneEditorSystemSet::Tasks)
            .after(super::sync)
            .after(super::super::project_files::open_managed),
    );
}
fn drive(
    s: Res<Studio>,
    mut docs: ResMut<DocumentState>,
    mut editor: ResMut<crate::resources::SceneEditorState>,
    mut focus: ResMut<InputFocus>,
    buttons: Query<&EditorButtonClick, (With<Bind>, Changed<EditorButtonClick>)>,
    fields: Query<(&BindingField, &EditableText)>,
    mut open: MessageWriter<OpenManagedFile>,
    mut pending: Local<Option<Pending>>,
    mut labels: Query<&mut Text, With<BoundFilm>>,
) {
    let selected = s
        .last_asset
        .as_ref()
        .filter(|(root, _, _)| root == &docs.root);
    for mut t in &mut labels {
        let value = selected.map_or("请先双击一个剧情演出资产", |(_, _, name)| {
            name.as_str()
        });
        if t.as_str() != value {
            **t = value.into();
        }
    }
    if !editor.open || editor.close_requested {
        pending.take();
        return;
    }
    if let Some(job) = pending.as_mut() {
        if docs.root != job.intent.root || docs.project_root.as_ref() != Some(&job.intent.project) {
            pending.take();
            return;
        }
        match &mut job.phase {
            Phase::Open(work) => {
                if let Some(r) = check_ready(work) {
                    match r {
                        Ok((path, _)) => {
                            open.write(OpenManagedFile { path: path.clone() });
                            job.phase = Phase::Waiting(path);
                        }
                        Err(e) => {
                            editor.status = e;
                            pending.take();
                        }
                    }
                }
            }
            Phase::Waiting(path) => {
                if docs.in_center()
                    && let Some(d) = docs
                        .current()
                        .filter(|d| d.disk.project_rules && d.disk.path == *path)
                {
                    let src = d.source.clone();
                    let err = docs.file_change_pending(&d.path) || docs.is_loading(&d.path);
                    if err {
                        editor.status = "规则文件正在重新载入或移动，请重试".into();
                        pending.take();
                        return;
                    }
                    let crlf = d.disk.crlf;
                    let (e, t, id) = (
                        job.intent.event.clone(),
                        job.intent.target.clone(),
                        job.intent.id.clone(),
                    );
                    let key = Identity {
                        path: d.path.clone(),
                        generation: d.generation,
                        revision: d.revision,
                    };
                    job.phase = Phase::Edit {
                        key,
                        result: AsyncComputeTaskPool::get()
                            .spawn(async move { insert(&src, &e, &t, &id, crlf) }),
                    };
                } else if docs.expanded.as_ref().is_none_or(|tab| {
                    tab.path != feathrise_editor_documents_model::project_rules::PATH
                }) || docs.io.is_none()
                    && docs.pending_load.is_none()
                    && !docs.error.is_empty()
                {
                    editor.status = if docs.error.is_empty() {
                        "规则标签已切换或打开被中止，可重新绑定".into()
                    } else {
                        docs.error.clone()
                    };
                    pending.take();
                }
            }
            Phase::Edit { key, result } => {
                if let Some(result) = check_ready(result) {
                    if let Some(d) = docs.documents.iter_mut().find(|d| {
                        d.path == key.path
                            && d.generation == key.generation
                            && d.revision == key.revision
                    }) {
                        match result {
                            Ok((src, pages)) => {
                                d.replace_content(src, pages, default());
                                d.mode = ViewMode::Edit;
                                editor.status = "已将演出加入工程规则草稿，按 Ctrl+S 保存".into();
                            }
                            Err(e) => editor.status = e,
                        }
                    } else {
                        editor.status = "工程规则在绑定期间已变化，请重新操作".into();
                    }
                    pending.take();
                }
            }
        }
        return;
    }
    if !buttons.iter().any(EditorButtonClick::is_clicked) {
        return;
    }
    let Some((root, id, _)) = selected else {
        editor.status = "请先在工程资产内打开要绑定的剧情演出".into();
        return;
    };
    let Some(project) = docs.project_root.clone() else {
        return;
    };
    let root = root.clone();
    let mut event = String::new();
    let mut target = String::new();
    for (k, input) in &fields {
        if input.is_composing() || !input.pending_edits.is_empty() || input.pending_paste.is_some()
        {
            editor.status = "请先完成当前输入".into();
            return;
        }
        let value = input.value().to_string().trim().to_owned();
        match k {
            BindingField::Event => event = value,
            BindingField::Target => target = value,
        }
    }
    if event.is_empty() {
        editor.status = "请输入触发事件名称".into();
        return;
    }
    focus.clear();
    let (p, a) = (project.clone(), root.clone());
    let work = IoTaskPool::get().spawn(async move {
        crate::ui::gameplay_sidebar::project_rules::open_or_create(
            &p,
            &a,
            false,
            &AtomicBool::new(false),
        )
    });
    *pending = Some(Pending {
        intent: Intent {
            project,
            root,
            id: id.clone(),
            event,
            target,
        },
        phase: Phase::Open(work),
    });
}
