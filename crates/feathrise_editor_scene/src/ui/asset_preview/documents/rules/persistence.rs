//! 保存一次冻结本条记录和文档版本，等实际文件写入回执后才通知作者。取消只恢复本次表单的编辑。
use super::{super::types::Document, *};

struct Checkpoint {
    key: Identity,
    content: u64,
    source: Arc<str>,
    pages: Vec<Arc<str>>,
    selection: Selection,
}
impl Checkpoint {
    fn new(doc: &Document, selection: Selection) -> Self {
        Self {
            key: Identity::of(doc),
            content: doc.content_revision(),
            source: doc.source.clone(),
            pages: doc.pages.clone(),
            selection,
        }
    }
    fn restore(self, doc: &mut Document) -> Selection {
        doc.restore_content(self.source, self.pages, self.content);
        self.selection
    }
}
pub(super) struct Deletion {
    before: Checkpoint,
    sheet: Option<Arc<Sheet>>,
    form: Form,
}
struct Write {
    key: Identity,
    content: u64,
    message: String,
    started: bool,
    deletion: Option<Deletion>,
}
struct Receipt {
    key: Identity,
    selection: Selection,
    message: String,
}
#[derive(Default)]
pub(super) struct Persistence {
    before: Option<Checkpoint>,
    write: Option<Write>,
    receipt: Option<Receipt>,
    pub next: Option<(Selection, bool, Option<Entity>)>,
}

impl Studio {
    /// 时间线传入它实际编辑的文件。绑定与表单共用取消边界，已有输入由作者先保存或取消。
    pub(in super::super) fn stage_bound_rule(
        &mut self,
        doc: &mut Document,
        source: Arc<str>,
        pages: Vec<Arc<str>>,
    ) -> Result<(), String> {
        let same_document =
            |key: &Identity| key.generation == doc.generation && key.path == doc.path;
        if self.key.as_ref().is_some_and(&same_document)
            && (self.has_unsaved_input() || self.has_parked_input() || self.files.before.is_some())
            || self
                .job
                .as_ref()
                .is_some_and(|j| j.change.is_some() && same_document(&j.key))
            || self
                .files
                .write
                .as_ref()
                .is_some_and(|w| same_document(&w.key))
        {
            return Err("工程规则还有未完成的编辑，请先保存或取消修改，再绑定演出。".into());
        }
        // 演出追加在规则页确认或取消，即使侧栏此前停在变量或地点。
        let selection = if self.sel.tab == Tab::Rules && self.scene(Tab::Rules).is_none() {
            self.sel
        } else {
            Selection {
                tab: Tab::Rules,
                ..default()
            }
        };
        self.files.before = Some(Checkpoint::new(doc, selection));
        doc.replace_content(source, pages, default());
        self.track_edit(doc);
        Ok(())
    }
    pub(super) fn forget_stale_checkpoint(&mut self, key: &Identity) {
        if self
            .files
            .receipt
            .as_ref()
            .is_some_and(|receipt| &receipt.key != key)
        {
            self.files.receipt = None;
        }
        if self.files.before.as_ref().is_some_and(|b| &b.key != key) {
            self.files.before = None;
            self.return_to = None;
        }
    }
    pub(super) fn has_unsaved_input(&self) -> bool {
        self.form != self.baseline || self.owns_checkpoint()
    }
    fn owns_checkpoint(&self) -> bool {
        self.files
            .before
            .as_ref()
            .is_some_and(|before| before.selection.tab == self.sel.tab)
    }
    pub(super) fn saving(&self) -> bool {
        self.files.write.is_some() || self.job.as_ref().is_some_and(|j| j.save)
    }
    pub(super) fn deleting_row(&self) -> bool {
        let key = self.key.as_ref();
        self.sel.tab != Tab::Locations
            && !self.editing_item
            && (self.job.as_ref().is_some_and(|job| {
                key == Some(&job.key) && job.change == Some(Change::Delete)
            }) || self.files.write.as_ref().is_some_and(|write| {
                key == Some(&write.key) && write.deletion.is_some()
            }))
    }
    pub(super) fn list_projection(&self) -> (Option<&Sheet>, Selection) {
        // 写入回执前显示完整旧表。写入失败恢复同一份表，画面不会先少一行再补回来。
        if self.deleting_row()
            && let Some(deletion) = self.files.write.as_ref().and_then(|write| write.deletion.as_ref())
        {
            return (deletion.sheet.as_deref(), deletion.before.selection);
        }
        let selection = if self.editing_item && self.sel.item.is_none() {
            self.return_to.unwrap_or(self.sel)
        } else {
            self.sel
        };
        (self.sheet.as_deref(), selection)
    }
    pub(in super::super) fn edit_pending(&self) -> bool {
        self.saving() || self.job.as_ref().is_some_and(|job| job.change.is_some())
    }
    pub(super) fn saved_current(&self) -> Option<&str> {
        let receipt = self.files.receipt.as_ref()?;
        (self.key.as_ref() == Some(&receipt.key)
            && self.sel == receipt.selection
            && !self.has_unsaved_input())
        .then_some(receipt.message.as_str())
    }
    pub(in super::super) fn unsaved_source_key(&self) -> Option<super::super::types::SourceKey> {
        let key = if self.form != self.baseline || self.has_parked_input() {
            self.key.as_ref()?
        } else if let Some(write) = &self.files.write {
            &write.key
        } else {
            &self.job.as_ref().filter(|job| job.change.is_some())?.key
        };
        Some(super::super::types::SourceKey {
            path: key.path.clone(),
            generation: key.generation,
        })
    }
    pub(super) fn remember(&mut self, doc: &Document) {
        if self
            .files
            .before
            .as_ref()
            .is_none_or(|before| !before.key.matches(doc))
        {
            self.files.before = Some(Checkpoint::new(doc, self.sel));
        }
    }
    pub(super) fn track_edit(&mut self, doc: &Document) {
        if let Some(before) = self
            .files
            .before
            .as_mut()
            .filter(|b| b.key.path == doc.path && b.key.generation == doc.generation)
        {
            if doc.content_revision() == before.content {
                self.files.before = None;
            } else {
                before.key.revision = doc.revision;
            }
        }
    }
    pub(super) fn cancel(&mut self, doc: &mut Document) {
        self.editing_item = false;
        if self.owns_checkpoint()
            && let Some(before) = self.files.before.take().filter(|b| b.key.matches(doc))
        {
            self.sel = before.restore(doc);
        }
        if self.sel.item.is_none() {
            self.sel = self.return_to.take().unwrap_or(Selection {
                tab: self.sel.tab,
                item: self
                    .sheet
                    .as_ref()
                    .and_then(|s| self.rows(s, self.sel.tab).first().copied()),
                ..default()
            });
        }
        self.hydrate();
        self.files.receipt = None;
        self.notice = "已取消本次修改".into();
    }
    pub(super) fn retain_deletion(&self, doc: &Document) -> Deletion {
        Deletion {
            before: Checkpoint::new(doc, self.sel),
            sheet: self.sheet.clone(),
            form: self.form.clone(),
        }
    }
    pub(super) fn queue_write(
        &mut self,
        doc: &Document,
        message: String,
        deletion: Option<Deletion>,
    ) {
        self.files.receipt = None;
        self.files.write = Some(Write {
            key: Identity::of(doc),
            content: doc.content_revision(),
            message,
            started: false,
            deletion,
        });
    }
    pub(super) fn select(
        &mut self,
        selection: Selection,
        edit: bool,
        id: Option<Entity>,
        focus: &mut InputFocus,
    ) {
        self.return_to = (edit
            && self.sel.tab == selection.tab
            && self.sel.item.is_some()
            && selection.item.is_none()).then_some(self.sel);
        self.sel = selection;
        self.editing_item = edit;
        self.picker = None;
        self.hydrate();
        self.notice.clear();
        self.files.receipt = None;
        if let Some(entity) = id {
            focus.set(entity, FocusCause::Navigated);
        }
    }
}

pub(super) fn poll(s: &mut Studio, docs: &mut DocumentState, focus: &mut InputFocus) {
    let Some(w) = &s.files.write else { return };
    let doc = docs.documents.iter().position(|d| w.key.matches(d));
    let shown = active(docs).as_ref() == Some(&w.key);
    let owns_form = shown || s.key.as_ref() == Some(&w.key);
    let result = match doc {
        None => Err("保存期间文档已切换版本，请检查当前内容后重新保存".to_owned()),
        Some(_) if docs.saving.as_deref() == Some(&w.key.path) => return,
        Some(i) if w.started && docs.documents[i].saved_revision == w.content => Ok(()),
        Some(i) if w.started => Err(docs.documents[i].status.clone()),
        Some(i) if docs.documents[i].external.warning => {
            Err("磁盘文件已被修改，保存已停止。请先处理文件冲突。".into())
        }
        Some(_) if docs.file_change_pending(&w.key.path) => {
            Err("文件正在移动或重命名，请稍后保存。".into())
        }
        Some(_)
            if docs.io.is_some() || docs.pending_load.is_some() || docs.saved_play_pending() =>
        {
            return;
        }
        Some(i) => {
            // 工作台已在后台按当前修订校验完整的规则文档。复用文档的写盘任务和磁盘冲突检查。
            docs.documents[i].confirm_reload = false;
            docs.documents[i].history.break_group();
            docs.start_save(&w.key.path);
            s.files.write.as_mut().unwrap().started = true;
            return;
        }
    };
    let refresh_list = owns_form && s.deleting_row();
    let w = s.files.write.take().unwrap();
    if refresh_list {
        s.refresh();
    }
    let next = s.files.next.take();
    if result.is_ok()
        && s.files
            .before
            .as_ref()
            .is_some_and(|before| before.key == w.key)
    {
        s.files.before = None;
    }
    if let (Err(error), Some(i), Some(deletion)) = (&result, doc, w.deletion) {
        // 删除未写盘时原条目仍在工程里。恢复本次删除之前的状态，保留更早的规则操作及正在填写的值。
        let d = &mut docs.documents[i];
        let selection = deletion.before.restore(d);
        d.status.clone_from(error);
        s.track_edit(d);
        if owns_form {
            s.key = Some(Identity::of(d));
            s.sel = selection;
            s.sheet = deletion.sheet;
            s.hydrate();
            s.form = deletion.form;
        }
    }
    if !owns_form {
        return;
    }
    match result {
        Ok(()) => {
            s.notice.clear();
            if let Some((selection, edit, id)) = next.filter(|_| s.form == s.baseline) {
                s.select(selection, edit, id.filter(|_| shown), focus);
            } else {
                if s.form == s.baseline {
                    s.editing_item = false;
                }
                s.files.receipt = Some(Receipt {
                    key: w.key,
                    selection: s.sel,
                    message: w.message,
                });
            }
        }
        Err(error) => s.notice = error,
    }
}
