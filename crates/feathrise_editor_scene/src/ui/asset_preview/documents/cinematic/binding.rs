//! 从当前四轨演出创建事件规则；进入触发规则工作台后沿用原文档的保存与撤销。
use super::super::rules::{RevealBoundRule, Studio as RulesStudio};
use super::*;
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
pub(super) struct ToggleBinding;
#[derive(Component, Clone, Default)]
struct BindingOptions;

pub(super) fn toggle_button() -> impl Scene {
    bsn! {ToggleBinding InteractionButton
    Node{height:px(22),padding:UiRect::horizontal(px(5)),align_items:AlignItems::Center,border:px(1),border_radius:BorderRadius::all(px(3))}
    BackgroundColor(c::SLOT_BG) BorderColor::all(c::COPPER_BORDER_WEAK)
    Children[Text("绑定触发规则…") TextColor(c::TEXT_PRIMARY) TextFont{font:FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),font_size:px(11)} Pickable::IGNORE]}
}
fn field(kind: BindingField, label: &'static str, initial: &'static str) -> impl Scene {
    bsn! {Node{width:percent(100),column_gap:px(3),height:px(24),flex_shrink:0.0,align_items:AlignItems::Center}
    Children[Text(label) TextColor(c::TEXT_MUTED) TextFont{font:FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),font_size:px(11)}
    -- ~{kind} crate::ui::text_input::EditorTextInput ~{bevy::input_focus::tab_navigation::TabIndex(0)} ~{bevy::ui::FocusPolicy::Block}
    bevy::text::TextCursorStyle::default() ~{EditableText{allow_newlines:false,..EditableText::new(initial)}}
    TextLayout::no_wrap() TextFont{font:FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),font_size:px(11)}
    TextColor(c::TEXT_VALUE) BackgroundColor(c::INPUT_BG) BorderColor::all(c::COPPER_BORDER_WEAK)
    Node{flex_grow:1.0,min_width:px(25),height:px(22),border:px(1),padding:UiRect::horizontal(px(3)),overflow:Overflow::clip()}]}
}
pub(super) fn panel() -> impl Scene {
    bsn! {BindingOptions Node{display:Display::None,width:percent(100),flex_direction:FlexDirection::Column,flex_shrink:0.0,row_gap:px(2)}
    Children[
    @field(BindingField::Event,"事件","location.entered")-- @field(BindingField::Target,"目标（可选）","")--
    Bind InteractionButton Node{height:px(23),padding:UiRect::horizontal(px(5)),align_items:AlignItems::Center} BackgroundColor(c::SLOT_BG)
    Children[Text("创建触发规则") TextColor(c::TEXT_PRIMARY) TextFont{font:FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),font_size:px(11)} Pickable::IGNORE]
    ]}
}
/// 选项属于展开它的演出标签。切到另一个演出，旧目标不会被当成当前绑定。
fn toggle(
    docs: Res<DocumentState>,
    buttons: Query<&EditorButtonClick, (Changed<EditorButtonClick>, With<ToggleBinding>)>,
    mut nodes: Query<
        (&mut Node, Has<ToggleBinding>),
        Or<(With<ToggleBinding>, With<BindingOptions>)>,
    >,
    mut fields: Query<(&BindingField, &mut EditableText)>,
    mut opened: Local<Option<(String, u64)>>,
) {
    let doc = docs.current().filter(|d| {
        docs.in_center()
            && is_path(&d.path)
            && !super::super::source::is_project(&d.path)
            && d.mode == ViewMode::Preview
    });
    let owns = |opened: &Option<(String, u64)>| {
        doc.is_some_and(|d| {
            opened
                .as_ref()
                .is_some_and(|(p, g)| p == &d.path && *g == d.generation)
        })
    };
    if buttons.iter().any(EditorButtonClick::is_clicked)
        && let Some(d) = doc
    {
        if owns(&opened) {
            *opened = None;
        } else {
            *opened = Some((d.path.clone(), d.generation));
            for (kind, mut text) in &mut fields {
                let initial = if *kind == BindingField::Event {
                    "location.entered"
                } else {
                    ""
                };
                crate::ui::text_input::replace_single_line_value(&mut text, initial, None);
            }
        }
    }
    for (mut node, button) in &mut nodes {
        let display = if (button && doc.is_some()) || owns(&opened) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}
struct Intent {
    root: PathBuf,
    project: PathBuf,
    id: String,
    event: String,
    target: String,
}
struct Pending {
    film: Identity,
    intent: Intent,
    phase: Phase,
}
enum Phase {
    Open(Task<Result<(PathBuf, bool), String>>),
    Waiting(PathBuf),
    Edit {
        key: Identity,
        result: Task<Result<InsertedRule, String>>,
    },
}
pub(super) struct InsertedRule {
    pub index: usize,
    pub source: Arc<str>,
    pages: Vec<Arc<str>>,
}
pub(super) fn insert(
    source: &str,
    event: &str,
    target: &str,
    id: &str,
    crlf: bool,
) -> Result<InsertedRule, String> {
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
    let index = rules.len();
    rules.push(json!({"id":format!("play_{}",bevy::asset::uuid::Uuid::new_v4().simple()),"when":when,"actions":[action]}));
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n";
    let data = if crlf {
        text.replace('\n', "\r\n")
    } else {
        text.clone()
    };
    Ok(InsertedRule {
        index,
        source: text.into(),
        pages: super::super::io::source_pages(&data),
    })
}
pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        (toggle, drive)
            .in_set(crate::SceneEditorSystemSet::Tasks)
            .after(super::sync)
            .after(super::super::project_files::open_managed),
    );
}
fn drive(
    s: Res<Studio>,
    mut rules: ResMut<RulesStudio>,
    mut docs: ResMut<DocumentState>,
    mut editor: ResMut<crate::resources::SceneEditorState>,
    mut focus: ResMut<InputFocus>,
    buttons: Query<&EditorButtonClick, (With<Bind>, Changed<EditorButtonClick>)>,
    fields: Query<(&BindingField, &EditableText)>,
    mut reveal: MessageWriter<RevealBoundRule>,
    mut pending: Local<Option<Pending>>,
    mut sidebar: ResMut<crate::ui::sidebars::SceneEditorSidebarState>,
    mut pages: ResMut<crate::ui::gameplay_sidebar::GameplaySidebarState>,
) {
    if !editor.open || editor.close_requested {
        pending.take();
        return;
    }
    if let Some(job) = pending.as_mut() {
        if docs.root != job.intent.root
            || docs.project_root.as_ref() != Some(&job.intent.project)
            || active(&docs).as_ref() != Some(&job.film)
        {
            pending.take();
            return;
        }
        match &mut job.phase {
            Phase::Open(work) => {
                if let Some(r) = check_ready(work) {
                    match r.and_then(|(path, _)| {
                        super::super::rules::load_sidebar_project_rules_document(&mut docs, &path)?;
                        Ok(path)
                    }) {
                        Ok(path) => job.phase = Phase::Waiting(path),
                        Err(e) => {
                            editor.status = e;
                            pending.take();
                        }
                    }
                }
            }
            Phase::Waiting(path) => {
                if let Some(d) = docs
                    .rules_sidebar_document()
                    .filter(|d| d.disk.path == *path)
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
                } else if !docs.sidebar_rules.error.is_empty() {
                    editor.status.clone_from(&docs.sidebar_rules.error);
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
                            Ok(added) => {
                                if let Err(error) =
                                    rules.stage_bound_rule(d, added.source, added.pages)
                                {
                                    editor.status = error;
                                    pending.take();
                                    return;
                                }
                                d.mode = ViewMode::Preview;
                                // 索引对应本次追加后的确切修订。由表单定位并交给作者显式保存当前文件。
                                reveal.write(RevealBoundRule {
                                    path: d.path.clone(),
                                    generation: d.generation,
                                    revision: d.revision,
                                    index: added.index,
                                });
                                sidebar.left = crate::ui::sidebars::EditorLeftSidebarTab::Gameplay;
                                sidebar.right = sidebar.left.work_page();
                                pages.page =
                                    crate::ui::gameplay_sidebar::GameplaySidebarPage::Rules;
                                super::super::select_loaded_sidebar_project_rules_section(
                                    &mut docs,
                                    super::super::ProjectRulesSection::Rules,
                                );
                                editor.status = "已绑定演出，点击「保存规则」写入工程".into();
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
    // 绑定的是此刻展示的有效修订。标签刚切换或源文件重载时不能沿用上个演出的 ID。
    let current = active(&docs)
        .filter(|k| !super::super::source::is_project(&k.path) && s.key.as_ref() == Some(k));
    let Some((owner, film)) = current.zip(s.timeline.as_ref()) else {
        editor.status = "请等当前工程演出载入后再绑定触发规则".into();
        return;
    };
    let Some(project) = docs.project_root.clone() else {
        return;
    };
    let root = docs.root.clone();
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
            &AtomicBool::new(false),
        )
    });
    *pending = Some(Pending {
        intent: Intent {
            project,
            root,
            id: film.film.id.clone(),
            event,
            target,
        },
        film: owner,
        phase: Phase::Open(work),
    });
}
