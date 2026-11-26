//! 交互侧栏中的工程规则表单、中央高级源码及原文档工具共用一份文档会话。
mod controls;
mod location_draw;
mod persistence;
mod types;
mod values;
mod view;
mod worker;
use super::{
    DocumentState,
    components::DocumentSource,
    types::{Document, SourceKey, ViewMode},
};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use std::{path::Path, sync::Arc};
pub(crate) use types::Tab as ProjectRulesSection;
const RULES_SHEET_PADDING: f32 = 9.0;
use types::*;
use worker::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    path: String,
    generation: u64,
    revision: u64,
}
impl Identity {
    fn of(doc: &super::types::Document) -> Self {
        Self {
            path: doc.path.clone(),
            generation: doc.generation,
            revision: doc.revision,
        }
    }
    fn matches(&self, doc: &super::types::Document) -> bool {
        self.generation == doc.generation && self.revision == doc.revision && self.path == doc.path
    }
}
#[derive(Default)]
pub(super) struct SidebarDocument {
    pub(super) visible: bool,
    pub(super) path: Option<String>,
    pub(super) pending: Option<(String, Option<(SourceKey, u64)>)>,
    pub(super) loading: bool,
    pub(super) error: String,
    section: Option<ProjectRulesSection>,
}

impl SidebarDocument {
    pub(super) fn reserved_path(&self) -> Option<&str> {
        (self.visible || self.pending.is_some() || self.loading)
            .then_some(self.path.as_deref())
            .flatten()
    }
}

impl DocumentState {
    pub(super) fn rules_sidebar_document(&self) -> Option<&Document> {
        let path = self.sidebar_rules.path.as_deref()?;
        self.documents
            .iter()
            .find(|d| d.path == path && d.disk.project_rules)
    }

    pub(super) fn rules_in_sidebar(&self) -> bool {
        self.sidebar_rules.visible
    }
}

fn active_document(docs: &DocumentState) -> Option<&Document> {
    if !docs.rules_in_sidebar() {
        return None;
    }
    docs.rules_sidebar_document().filter(|doc| {
        !(docs.visible
            && docs.in_center()
            && docs.active.as_deref() == Some(&doc.path)
            && doc.mode == ViewMode::Edit)
    })
}

fn active_document_mut(docs: &mut DocumentState) -> Option<&mut Document> {
    let index = active_document(docs)
        .and_then(|doc| docs.documents.iter().position(|d| d.path == doc.path))?;
    docs.documents.get_mut(index)
}

fn active(docs: &DocumentState) -> Option<Identity> {
    active_document(docs).map(Identity::of)
}

fn shown_document<'a>(s: &Studio, docs: &'a DocumentState) -> Option<&'a Document> {
    if docs
        .sidebar_rules
        .section
        .is_some_and(|tab| tab != s.sel.tab)
        || !docs.sidebar_rules.error.is_empty()
    {
        return None;
    }
    active_document(docs).filter(|doc| s.key.as_ref().is_some_and(|key| key.matches(doc)))
}
/// 时间线只定位刚写入该文档修订的那条绑定；后台读取完成后由同一表单选中它。
#[derive(Message, Clone)]
pub(super) struct RevealBoundRule {
    pub path: String,
    pub generation: u64,
    pub revision: u64,
    pub index: usize,
}
struct Job {
    key: Identity,
    task: Task<Result<Reply, String>>,
    change: Option<Change>,
    save: bool,
    submitted: Option<Form>,
}
/// 工具切换只暂存尚未提交的字段；正文、历史和正在写盘的事务始终只有一份。
struct PageDraft {
    selection: Selection,
    form: Form,
    baseline: Form,
    alternate: AlternateInputs,
    return_to: Option<Selection>,
}
#[derive(Resource, Default)]
pub(super) struct Studio {
    key: Option<Identity>,
    sheet: Option<Arc<Sheet>>,
    job: Option<Job>,
    sel: Selection,
    editing_item: bool,
    form: Form,
    alternate: AlternateInputs,
    baseline: Form,
    revision: u64,
    notice: String,
    suggest: Option<(Field, usize)>,
    return_to: Option<Selection>,
    files: persistence::Persistence,
    parked: [Option<PageDraft>; 3],
}
impl Studio {
    fn refresh(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    fn hydrate(&mut self) {
        self.form = self
            .sheet
            .as_ref()
            .map(|s| Form::load(&s.model, &self.sel))
            .unwrap_or_default();
        self.baseline = self.form.clone();
        self.alternate = default();
        self.suggest = None;
        self.refresh();
    }

    fn switch_section(&mut self, tab: Tab, focus: &mut InputFocus) {
        if self.sel.tab == tab {
            return;
        }
        self.parked[self.sel.tab as usize] = self.editing_item.then(|| PageDraft {
            selection: self.sel,
            form: std::mem::take(&mut self.form),
            baseline: std::mem::take(&mut self.baseline),
            alternate: std::mem::take(&mut self.alternate),
            return_to: self.return_to.take(),
        });
        if let Some(saved) = self.parked[tab as usize].take() {
            self.sel = saved.selection;
            self.notice.clear();
            if let Some(sheet) = self
                .sheet
                .as_ref()
                .filter(|_| saved.selection.item.is_some())
            {
                // 其它工具的编辑与撤销共用同一份正文。回到原表单时绝不能沿用旧行号覆盖邻近条目。
                self.sel.item = sheet.position(tab, &saved.baseline.id);
                let latest = Form::load(&sheet.model, &self.sel);
                if latest != saved.baseline {
                    if saved.form == saved.baseline {
                        self.editing_item = self.sel.item.is_some();
                        self.hydrate();
                        return;
                    }
                    self.notice = if self.sel.item.is_some() {
                        "原条目已有其它修改，已保留填写。请用不同标识保存，或取消后重新编辑。"
                    } else {
                        "原条目已不存在，已保留填写。可作为新条目保存，也可以取消。"
                    }
                    .into();
                    self.sel.item = None;
                }
            }
            self.form = saved.form;
            self.baseline = saved.baseline;
            self.alternate = saved.alternate;
            self.return_to = saved.return_to;
            self.editing_item = true;
            self.suggest = None;
            self.refresh();
        } else {
            let item = self
                .sheet
                .as_ref()
                .is_some_and(|sheet| sheet.len(tab) != 0)
                .then_some(0);
            self.select(
                Selection {
                    tab,
                    item,
                    ..default()
                },
                false,
                None,
                focus,
            );
        }
    }

    fn has_parked_input(&self) -> bool {
        self.parked.iter().flatten().any(|p| p.form != p.baseline)
    }
}
fn start(
    s: &mut Studio,
    docs: &DocumentState,
    input: Option<Submission>,
    save: bool,
) -> Result<(), String> {
    let doc = active_document(docs).ok_or("工程规则尚未载入")?;
    let key = Identity::of(doc);
    if s.job.is_some() {
        return Err("正在处理上一笔规则编辑".into());
    }
    if input.is_some()
        && (s.key.as_ref() != Some(&key)
            || docs.is_loading(&doc.path)
            || docs.file_change_pending(&doc.path))
    {
        return Err("文件正在变更，完成后可继续编辑".into());
    }
    let source = doc.source.clone();
    let old = s.sheet.clone();
    let sel = s.sel;
    let pages = input.as_ref().map(|_| doc.pages.clone());
    let want = sel
        .item
        .and_then(|i| old.as_ref()?.id(sel.tab, i))
        .map(str::to_owned);
    s.job = Some(Job {
        key,
        change: input.as_ref().map(|submission| submission.change),
        save,
        submitted: input.as_ref().map(|submission| submission.form.clone()),
        task: AsyncComputeTaskPool::get()
            .spawn(async move { worker::prepare(source, old, sel, want, input, pages) }),
    });
    Ok(())
}
fn sync(
    mut s: ResMut<Studio>,
    mut docs: ResMut<DocumentState>,
    mut focus: ResMut<InputFocus>,
    mut reveal: MessageReader<RevealBoundRule>,
    mut pending: Local<Option<RevealBoundRule>>,
    fields: Query<&bevy::text::EditableText, With<Field>>,
) {
    if let Some(latest) = reveal.read().last() {
        *pending = Some(latest.clone());
    }
    if s.saving() {
        persistence::poll(&mut s, &mut docs, &mut focus);
    }
    if s.job.is_some()
        && let Some(job) = s.job.as_mut()
        && let Some(result) = check_ready(&mut job.task)
    {
        let job = s.job.take().unwrap();
        let shown = active(&docs).as_ref() == Some(&job.key);
        // 切走中央页只停放视图。回执仍属于这份表单，保留提交后继续输入的字段。
        let owns_form = shown || s.key.as_ref() == Some(&job.key);
        // 已经确认的编辑即使切走仍只进入提交时的那份草稿。重载、撤销或其它写入会使旧修订失效。
        if let Some(d) = docs.documents.iter_mut().find(|d| {
            d.path == job.key.path
                && d.generation == job.key.generation
                && d.revision == job.key.revision
        }) {
            match result {
                Ok(reply) => {
                    let deletion =
                        (job.change == Some(Change::Delete)).then(|| s.retain_deletion(d));
                    if let Some((text, pages)) = reply.text {
                        if owns_form {
                            s.remember(d);
                        }
                        d.replace_content(text, pages, default());
                        if owns_form {
                            s.track_edit(d);
                        }
                    }
                    if job.save && d.dirty() {
                        let noun = reply.selection.tab.noun();
                        let id = job.submitted.as_ref().map(|f| f.id.trim()).unwrap_or("");
                        let status = if job.change == Some(Change::Delete) {
                            let original = s
                                .sheet
                                .as_ref()
                                .and_then(|sheet| sheet.id(s.sel.tab, s.sel.item?))
                                .unwrap_or(id);
                            format!("已删除{noun}「{original}」")
                        } else {
                            format!("{noun}「{id}」已保存")
                        };
                        s.queue_write(d, status, deletion);
                    }
                    let revision = d.revision;
                    if owns_form {
                        s.key = Some(Identity {
                            revision,
                            ..job.key
                        });
                        s.sheet = Some(reply.sheet);
                        s.sel = reply.selection;
                        // 写盘准备期间作者可能已输入下一个值，只采用已确认版本为新基线。
                        let keep_fields =
                            matches!(job.change, Some(Change::RuleUp | Change::RuleDown))
                                && s.form != s.baseline;
                        let newer = (keep_fields
                            || (job.change != Some(Change::Delete)
                                && job.submitted.as_ref().is_some_and(|f| f != &s.form)))
                        .then(|| std::mem::take(&mut s.form));
                        s.notice.clear();
                        s.hydrate();
                        if let Some(newer) = newer {
                            s.form = newer;
                        }
                        if job.save && !d.dirty() {
                            if let Some((selection, edit, id)) =
                                s.files.next.take().filter(|_| s.form == s.baseline)
                            {
                                s.select(selection, edit, id.filter(|_| shown), &mut focus);
                            } else {
                                s.editing_item = s.form != s.baseline;
                                s.notice = format!("当前{}没有需要保存的修改", s.sel.tab.noun());
                            }
                        }
                    }
                }
                Err(e) => {
                    if owns_form {
                        s.files.next = None;
                        s.notice = e;
                        if job.change.is_none() {
                            s.key = Some(job.key);
                            s.sheet = None;
                            s.hydrate();
                        }
                    }
                }
            }
        }
    }
    if let Some(key) = active(&docs)
        && s.key.as_ref() != Some(&key)
        && s.job.is_none()
    {
        if s.key
            .as_ref()
            .is_some_and(|old| old.path != key.path || old.generation != key.generation)
        {
            s.parked = default();
            s.editing_item = false;
            s.return_to = None;
        }
        s.forget_stale_checkpoint(&key);
        let _ = start(&mut s, &docs, None, false);
    }
    if let Some(section) = docs.sidebar_rules.section
        && active(&docs)
            .as_ref()
            .is_some_and(|key| s.key.as_ref() == Some(key))
        && s.job.is_none()
        && !s.saving()
        && !fields
            .iter()
            .any(|f| f.is_composing() || f.pending_paste.is_some() || !f.pending_edits.is_empty())
    {
        docs.sidebar_rules.section = None;
        if s.sel.tab != section && focus.get().is_some_and(|id| fields.get(id).is_ok()) {
            focus.clear();
        }
        s.switch_section(section, &mut focus);
    }
    if let Some(want) = pending.as_ref() {
        let target = Identity {
            path: want.path.clone(),
            generation: want.generation,
            revision: want.revision,
        };
        if !docs.documents.iter().any(|doc| target.matches(doc)) {
            *pending = None;
        } else if active(&docs).as_ref() == Some(&target)
            && s.key.as_ref() == Some(&target)
            && s.job.is_none()
        {
            if s.sheet
                .as_ref()
                .is_some_and(|sheet| want.index < sheet.len(Tab::Rules))
            {
                s.select(
                    Selection {
                        tab: Tab::Rules,
                        item: Some(want.index),
                        ..default()
                    },
                    true,
                    None,
                    &mut focus,
                );
            }
            *pending = None;
        }
    }
}
#[derive(Component)]
pub(super) struct AuthorButton;

/// 交互目录打开工程规则时只预留后台缓存，不激活、不关闭中央标签。
pub(crate) fn open_sidebar_project_rules_section(
    docs: &mut DocumentState,
    path: &Path,
    section: ProjectRulesSection,
) -> Result<(), String> {
    load_sidebar_project_rules_document(docs, path)?;
    docs.sidebar_rules.section = Some(section);
    Ok(())
}

pub(super) fn load_sidebar_project_rules_document(
    docs: &mut DocumentState,
    path: &Path,
) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| n.eq_ignore_ascii_case(feathrise_editor_documents_model::project_rules::PATH))
        .ok_or("工程规则路径无效")?;
    let key = docs
        .documents
        .iter()
        .find(|d| d.disk.project_rules && d.disk.path == path)
        .map_or_else(|| name.to_owned(), |d| d.path.clone());
    docs.reserve_slot(&key)?;
    docs.sidebar_rules.path = Some(key.clone());
    docs.sidebar_rules.error.clear();
    if !docs.documents.iter().any(|doc| doc.path == key)
        && !(docs.sidebar_rules.loading
            && docs
                .loading
                .as_ref()
                .is_some_and(|request| request.path == key))
    {
        docs.sidebar_rules.pending = Some((key, None));
    }
    Ok(())
}

/// 仅复用已由原文档校验过的规则文件；首次进入或加载失败仍走后台打开。
pub(crate) fn select_loaded_sidebar_project_rules_section(
    docs: &mut DocumentState,
    section: ProjectRulesSection,
) -> bool {
    let Some(doc) = docs.rules_sidebar_document() else {
        return false;
    };
    if docs.file_change_pending(&doc.path) {
        return false;
    }
    docs.sidebar_rules.section = Some(section);
    docs.sidebar_rules.error.clear();
    true
}

/// 先读再改，稳定帧不把文档标为已变更，也不刷新中央预览。
pub(crate) fn set_sidebar_project_rules_visible(docs: &mut ResMut<DocumentState>, visible: bool) {
    if docs.sidebar_rules.visible != visible {
        docs.sidebar_rules.visible = visible;
    }
}

pub(crate) fn set_sidebar_project_rules_error(docs: &mut DocumentState, error: String) {
    if docs.sidebar_rules.error != error {
        docs.sidebar_rules.error = error;
    }
}
fn protect_form_input(
    s: Res<Studio>,
    mut docs: ResMut<DocumentState>,
    fields: Query<Ref<bevy::text::EditableText>, With<Field>>,
) {
    if !s.is_changed() && !fields.iter().any(|field| field.is_changed()) {
        return;
    }
    let key = if s.editing_item
        && fields.iter().any(|field| {
            field.is_composing() || field.pending_paste.is_some() || !field.pending_edits.is_empty()
        }) {
        s.key.as_ref().map(|k| super::types::SourceKey {
            path: k.path.clone(),
            generation: k.generation,
        })
    } else {
        s.unsaved_source_key()
    };
    if docs.rules_form_input != key {
        docs.rules_form_input = key;
    }
}
pub(super) fn install(app: &mut App) {
    use crate::SceneEditorSystemSet as Sets;
    app.init_resource::<Studio>()
        .add_message::<RevealBoundRule>()
        .add_systems(
            Update,
            (controls::input, protect_form_input)
                .chain()
                .after(bevy::text::apply_text_edits)
                .before(crate::ui::file_manager::FileManagerInput)
                .before(super::interaction::actions)
                .in_set(Sets::Input),
        )
        .add_systems(
            Update,
            sync.after(super::session::drive_tasks).in_set(Sets::Tasks),
        );
    view::install(app);
    location_draw::install(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_editor_documents_model::project_rules::RulesForm;

    fn variable_sheet(source: &str) -> Arc<Sheet> {
        Arc::new(Sheet::new(RulesForm::parse(source).unwrap()))
    }

    #[test]
    fn parked_rule_form_never_overwrites_a_neighbor_after_history_moves_its_row() {
        let mut s = Studio {
            sheet: Some(variable_sheet(
                r#"{"version":1,"variables":{"a":{"default":1},"b":{"default":2}}}"#,
            )),
            sel: Selection {
                item: Some(1),
                ..default()
            },
            editing_item: true,
            ..default()
        };
        let mut focus = InputFocus::default();
        s.hydrate();
        s.form.default_value = "3".into();
        s.switch_section(Tab::Rules, &mut focus);
        s.sheet = Some(variable_sheet(
            r#"{"version":1,"variables":{"b":{"default":2},"a":{"default":1}}}"#,
        ));
        s.switch_section(Tab::Variables, &mut focus);
        assert_eq!(s.sel.item, Some(0));
        assert_eq!((&*s.form.id, &*s.form.default_value), ("b", "3"));

        s.switch_section(Tab::Rules, &mut focus);
        s.sheet = Some(variable_sheet(
            r#"{"version":1,"variables":{"a":{"default":1}}}"#,
        ));
        s.switch_section(Tab::Variables, &mut focus);
        assert!(s.sel.item.is_none());
        assert_eq!((&*s.form.id, &*s.form.default_value), ("b", "3"));
        assert!(s.notice.contains("原条目已不存在"));
    }
}
