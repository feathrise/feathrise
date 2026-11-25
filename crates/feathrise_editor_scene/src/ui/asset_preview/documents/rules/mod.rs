//! 主资产根的工程规则在原中央文档内提供变量、规则与地点表单；其它同名文件继续作为普通 JSON。
mod controls;
mod types;
mod values;
mod view;
mod worker;
use super::{DocumentState, components::DocumentSource, types::ViewMode};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use std::sync::Arc;
use types::*;
use worker::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    path: String,
    generation: u64,
    revision: u64,
}
fn active(docs: &DocumentState) -> Option<Identity> {
    let doc = docs.current().filter(|d| {
        docs.visible && docs.in_center() && d.disk.project_rules && d.mode == ViewMode::Preview
    })?;
    Some(Identity {
        path: doc.path.clone(),
        generation: doc.generation,
        revision: doc.revision,
    })
}
#[derive(Default, Clone, Copy)]
enum After {
    #[default]
    Stay,
    Save,
    Source,
}
struct Job {
    key: Identity,
    task: Task<Result<Reply, String>>,
    editing: bool,
    after: After,
}
#[derive(Resource, Default)]
struct Studio {
    key: Option<Identity>,
    sheet: Option<Arc<Sheet>>,
    job: Option<Job>,
    sel: Selection,
    form: Form,
    baseline: Form,
    revision: u64,
    notice: String,
    suggest: Option<(Field, usize)>,
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
        self.suggest = None;
        self.refresh();
    }
    fn select(&mut self, tab: Tab, index: Option<usize>) {
        self.sel = Selection {
            tab,
            item: index,
            ..default()
        };
        self.notice.clear();
        self.hydrate();
    }
}
fn start(
    s: &mut Studio,
    docs: &DocumentState,
    input: Option<Submission>,
    after: After,
) -> Result<(), String> {
    let doc = docs.current().ok_or("工程规则尚未载入")?;
    let key = active(docs).ok_or("工程规则已切换")?;
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
        editing: input.is_some(),
        after,
        task: AsyncComputeTaskPool::get()
            .spawn(async move { worker::prepare(source, old, sel, want, input, pages) }),
    });
    Ok(())
}
fn finish(docs: &mut DocumentState, after: After, focus: &mut InputFocus, source: Option<Entity>) {
    match after {
        After::Stay => {}
        After::Save => docs.request_save(),
        After::Source => {
            if let Some(doc) = docs.current_mut() {
                doc.mode = ViewMode::Edit;
                if let Some(entity) = source {
                    focus.set(entity, FocusCause::Navigated);
                }
            }
        }
    }
}
fn sync(
    mut s: ResMut<Studio>,
    mut docs: ResMut<DocumentState>,
    mut focus: ResMut<InputFocus>,
    source: Query<Entity, With<DocumentSource>>,
) {
    if s.job.is_some()
        && let Some(job) = s.job.as_mut()
        && let Some(result) = check_ready(&mut job.task)
    {
        let job = s.job.take().unwrap();
        let shown = active(&docs).as_ref() == Some(&job.key);
        // 已经确认的编辑即使切走仍只进入提交时的那份草稿。重载、撤销或其它写入会使旧修订失效。
        if let Some(d) = docs.documents.iter_mut().find(|d| {
            d.path == job.key.path
                && d.generation == job.key.generation
                && d.revision == job.key.revision
        }) {
            match result {
                Ok(reply) => {
                    if let Some((text, pages)) = reply.text {
                        d.replace_content(text, pages, default());
                    }
                    let revision = d.revision;
                    if shown {
                        s.key = Some(Identity {
                            revision,
                            ..job.key
                        });
                        s.sheet = Some(reply.sheet);
                        s.sel = reply.selection;
                        s.notice.clear();
                        s.hydrate();
                        finish(&mut docs, job.after, &mut focus, source.single().ok());
                    }
                }
                Err(e) => {
                    if shown {
                        s.notice = e;
                        if !job.editing {
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
        let _ = start(&mut s, &docs, None, After::Stay);
    }
}
#[derive(Component)]
pub(super) struct AuthorButton;
/// 交互页显式打开工程规则时恢复可视表单，使用磁盘读取时已确认的主根身份。
pub(crate) fn show_project_rules_form(docs: &mut DocumentState, path: &std::path::Path) {
    if let Some(doc) = docs
        .documents
        .iter_mut()
        .find(|d| d.disk.project_rules && d.disk.path == path)
    {
        doc.mode = ViewMode::Preview;
    }
}
pub(super) fn install(app: &mut App) {
    use crate::SceneEditorSystemSet as Sets;
    app.init_resource::<Studio>()
        .add_systems(
            Update,
            controls::input
                .before(super::interaction::actions)
                .in_set(Sets::Input),
        )
        .add_systems(
            Update,
            sync.after(super::session::drive_tasks).in_set(Sets::Tasks),
        );
    view::install(app);
}
