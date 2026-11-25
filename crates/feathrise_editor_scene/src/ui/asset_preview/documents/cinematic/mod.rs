//! 剧情演出使用原文件标签、磁盘版本及历史。中央视口和四条作者轨道共享一个可定位的时间。
mod binding;
mod controls;
mod drag;
mod model;
mod preview;
#[cfg(test)]
mod tests;
#[cfg(feature = "validation")]
pub(crate) mod validation;
mod view;
use super::{DocumentState, types::ViewMode};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use model::{Edit, Form, Item, Kind, Lane, Prepared, Timeline};
use std::sync::Arc;

pub(crate) fn is_path(path: &str) -> bool {
    path.len()
        .checked_sub(".cinematic.json".len())
        .and_then(|i| path.get(i..))
        .is_some_and(|s| s.eq_ignore_ascii_case(".cinematic.json"))
}
pub(crate) use binding::panel as binding_panel;
pub use preview::{CinematicPreview, CinematicPreviewFrame};
pub(crate) use view::FilmImage;
#[derive(Message)]
pub(crate) struct PickCinematicClip {
    pub target: crate::ui::file_manager::ProjectMotionTarget,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    path: String,
    generation: u64,
    revision: u64,
}
struct Job {
    key: Identity,
    task: Task<Result<Prepared, String>>,
    editing: bool,
}
#[derive(Resource, Default)]
struct Studio {
    key: Option<Identity>,
    timeline: Option<Arc<Timeline>>,
    job: Option<Job>,
    notice: String,
    lane: Lane,
    kind: Kind,
    selection: Option<Item>,
    selected_marker: usize,
    cursor: f32,
    playing: bool,
    previewing: bool,
    form: Form,
    form_revision: u64,
    settings: bool,
    control_camera: bool,
    control_player: bool,
    /// 即使作者转回「交互」面板，也能按稳定 ID 绑定最近检查过的演出。
    last_asset: Option<(std::path::PathBuf, String, String)>,
}
impl Studio {
    fn refresh(&mut self) {
        self.form_revision = self.form_revision.wrapping_add(1);
    }
    fn seek(&mut self, time: f32) {
        if let Some(t) = &self.timeline {
            self.cursor = time.clamp(0., t.film.duration);
            self.playing = false;
            self.previewing = true;
        }
    }
    fn select(&mut self, lane: Lane, item: Option<Item>) {
        self.settings = false;
        self.lane = lane;
        self.selection = item;
        if let (Some(t), Some(item)) = (&self.timeline, item) {
            let (form, kind) = Form::from_item(&t.film, item);
            self.notice = t.label(item).into_owned();
            self.cursor = form.at.parse().unwrap_or(self.cursor);
            self.form = form;
            self.kind = kind;
            self.previewing = true;
            self.playing = false;
        } else {
            self.form = Form {
                at: self.cursor.to_string(),
                ..default()
            };
            self.kind = if lane == Lane::Dialogue {
                Kind::Subtitle
            } else {
                Kind::Pose
            };
            if lane != Lane::Actor {
                self.form.target.clear();
            }
        }
        self.refresh();
    }
}
fn active(doc: &DocumentState) -> Option<Identity> {
    if !doc.in_center() {
        return None;
    }
    let d = doc
        .current()
        .filter(|d| is_path(&d.path) && d.mode == ViewMode::Preview)?;
    Some(Identity {
        path: d.path.clone(),
        generation: d.generation,
        revision: d.revision,
    })
}
fn start(s: &mut Studio, documents: &DocumentState, edit: Option<Edit>) -> Result<(), String> {
    let d = documents.current().ok_or("等待演出加载")?;
    let key = active(documents).ok_or("当前演出标签已经改变")?;
    if s.job.is_some() {
        return Err("正在处理上一笔演出编辑，请稍候".into());
    }
    if edit.is_some()
        && (s.key.as_ref() != Some(&key)
            || s.timeline.is_none()
            || documents.file_change_pending(&d.path)
            || documents.is_loading(&d.path))
    {
        return Err("文件正在变更，待当前修订载入后再编辑".into());
    }
    let source = d.source.clone();
    let crlf = d.disk.crlf;
    let editing = edit.is_some();
    s.job = Some(Job {
        key,
        editing,
        task: AsyncComputeTaskPool::get().spawn(async move { model::prepare(source, edit, crlf) }),
    });
    Ok(())
}
fn sync(
    mut s: ResMut<Studio>,
    mut docs: ResMut<DocumentState>,
    editor: Res<crate::resources::SceneEditorState>,
    time: Res<Time<Real>>,
) {
    let key = editor.open.then(|| active(&docs)).flatten();
    if let Some(job) = s.job.as_mut()
        && let Some(reply) = check_ready(&mut job.task)
    {
        let job = s.job.take().unwrap();
        if let Some(document) = docs.documents.iter_mut().find(|d| {
            d.path == job.key.path
                && d.generation == job.key.generation
                && d.revision == job.key.revision
        }) {
            match reply {
                Ok(mut prepared) => {
                    // 已确认的后台编辑即使切走也归入原标签；仅当前修订可发布 UI 与世界预览。
                    if let Some((source, pages)) = prepared.text.take() {
                        document.replace_content(source, pages, default());
                    }
                    let revision = document.revision;
                    if key.as_ref() == Some(&job.key) {
                        let same = s.key.as_ref().is_some_and(|old| {
                            old.path == job.key.path && old.generation == job.key.generation
                        });
                        s.key = Some(Identity {
                            revision,
                            ..job.key
                        });
                        s.last_asset = (!super::source::is_project(&document.path)).then(|| {
                            (
                                docs.root.clone(),
                                prepared.timeline.film.id.clone(),
                                prepared.timeline.film.name.clone(),
                            )
                        });
                        s.timeline = Some(prepared.timeline);
                        if !same {
                            s.cursor = 0.;
                            s.previewing = false;
                            s.playing = false;
                        }
                        let lane = if same { s.lane } else { Lane::Camera };
                        // 撤销、源码编辑和重载都可能改变键数组下标。选择始终跟随本次采用的修订。
                        s.select(lane, None);
                        s.notice.clear(); // 草稿和存盘结果均以原文档回执为准。
                    }
                }
                Err(e) => {
                    if key.as_ref() == Some(&job.key) {
                        s.notice = e;
                        if !job.editing {
                            s.key = Some(job.key);
                            s.timeline = None;
                        }
                    }
                }
            }
        }
    }
    let key = editor.open.then(|| active(&docs)).flatten();
    if key.is_none() {
        s.playing = false;
        s.previewing = false;
    }
    if s.key != key && s.job.is_none() {
        if key.is_some() {
            let _ = start(&mut s, &docs, None);
        } else {
            s.key = None;
            s.timeline = None;
        }
    }
    if s.playing
        && let Some(t) = &s.timeline
    {
        let end = t.film.duration;
        s.cursor = (s.cursor + time.delta_secs()).min(end);
        if s.cursor >= end {
            s.playing = false;
        }
    }
}
pub(super) fn install(app: &mut App) {
    use crate::SceneEditorSystemSet;
    app.init_resource::<Studio>()
        .add_message::<PickCinematicClip>()
        .add_message::<feathrise_editor_documents_model::cinematic_export::OpenCinematicExport>()
        .add_systems(
            Update,
            (
                drag::drag,
                controls::input,
                controls::picker,
                controls::seek,
            )
                .chain()
                .before(super::interaction::actions)
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            sync.after(super::session::drive_tasks)
                .in_set(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (view::mount, view::display, view::form, view::markers)
                .chain()
                .after(super::presentation::sync)
                .in_set(SceneEditorSystemSet::Presentation),
        );
    preview::install(app);
    binding::install(app);
}
