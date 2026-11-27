//! 独立编辑器中的 BSN 可视化文档。标签、历史、保存和磁盘冲突沿用中央文档。

mod canvas;
mod fields;
pub(super) use fields::{Field, input_pending};
#[cfg(feature = "validation")]
pub(crate) mod validation;
mod view;

use super::{DocumentState, io, types::ViewMode};
use crate::{SceneEditorSystemSet, resources::SceneEditorState};
use bevy::{
    input_focus::InputFocus,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
    text::EditableText,
};
use feathrise_editor_ui::EditorButtonClick;
use feathrise_ui_bsn::{UiLayout, UiLayoutAction, UiWidget, WidgetKind, is_rust_ui_document};
use std::{path::Path, sync::Arc};

pub(crate) fn is_candidate_path(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bsn") || ext.eq_ignore_ascii_case("rs"))
}

pub(crate) fn is_document(path: impl AsRef<Path>, source: &str) -> bool {
    path.as_ref().extension().is_some_and(|ext| {
        ext.eq_ignore_ascii_case("bsn")
            // 原生 Rust 只有首行显式声明可编辑时才进入画布，普通源码沿用文本工作区。
            || ext.eq_ignore_ascii_case("rs") && is_rust_ui_document(source)
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    path: String,
    generation: u64,
    revision: u64,
}

fn active(docs: &DocumentState) -> Option<Identity> {
    let doc = docs
        .current()
        .filter(|doc| docs.in_center() && doc.mode == ViewMode::Preview && is_document(&doc.path, &doc.source))?;
    Some(Identity {
        path: doc.path.clone(),
        generation: doc.generation,
        revision: doc.revision,
    })
}

#[derive(Component, Clone, Debug, PartialEq)]
enum Action {
    KeepDraft,
    Reload,
    Source,
    Preview(bool),
    Narrow(bool),
    Design,
    Select(String),
    Insert(WidgetKind),
    Remove,
    Shift(i32),
    Direction,
    Properties,
}

enum Edit {
    Insert(WidgetKind),
    Remove,
    Shift(i32),
    Direction,
    Properties(fields::Values),
    Position(String, Vec2),
}

struct Prepared {
    layout: Arc<UiLayout>,
    document: Option<(Arc<str>, Vec<Arc<str>>, String)>,
}

struct Job {
    key: Identity,
    editing: bool,
    task: Task<Result<Prepared, String>>,
}

#[derive(Resource, Default)]
pub(crate) struct UiLayouts {
    key: Option<Identity>,
    layout: Option<Arc<UiLayout>>,
    job: Option<Job>,
    selected: String,
    interactive: bool,
    narrow: bool,
    design: bool,
    notice: String,
}

impl UiLayouts {
    pub(crate) fn edit_pending(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.editing)
    }

    pub(super) fn edit_pending_for(&self, docs: &DocumentState) -> bool {
        self.job.as_ref().is_some_and(|job| {
            job.editing
                && docs.current().is_some_and(|doc| {
                    doc.path == job.key.path && doc.generation == job.key.generation
                })
        })
    }

    fn editable(&self, docs: &DocumentState) -> bool {
        self.job.is_none()
            && self.key.is_some()
            && self.key == active(docs)
            && self.layout.is_some()
            && self.key.as_ref().is_some_and(|key| {
                !docs.file_change_pending(&key.path) && !docs.is_loading(&key.path)
            })
    }

    fn submit(&mut self, docs: &DocumentState, edit: Edit) {
        if !self.editable(docs) {
            return;
        }
        let key = self.key.clone().expect("当前文档已校验");
        let document = docs.current().expect("当前文档已校验");
        let (before, pages, crlf) = (
            document.source.clone(),
            document.pages.clone(),
            document.disk.crlf,
        );
        let original = self.layout.clone().expect("当前画布已采用");
        let mut selected = self.selected.clone();
        self.notice.clear();
        // 文本编码、节点克隆及文档分块都在后台。前台只采用此前已看到的文档修订。
        self.job = Some(Job {
            key,
            editing: true,
            task: AsyncComputeTaskPool::get().spawn(async move {
                let mut layout = (*original).clone();
                apply(&mut layout.root, &mut selected, edit)?;
                // 无实际变化时不重排源码，也不生成撤销记录。
                if layout == *original {
                    return Ok(Prepared {
                        layout: original,
                        document: None,
                    });
                }
                // Rust 文档只替换受支持的 bsn! 正文，函数、导入和手写业务代码保留原字节。
                let source: Arc<str> = Arc::from(layout.replace_document(&before)?);
                if UiLayout::parse_document(&source)? != layout {
                    return Err("布局包含无法写入界面源码的属性".into());
                }
                let raw = io::edited_source(&pages, &before, &source, crlf);
                let pages = io::source_pages(&raw);
                Ok(Prepared {
                    layout: Arc::new(layout),
                    document: Some((source, pages, selected)),
                })
            }),
        });
    }
}

fn apply(root: &mut UiWidget, selected: &mut String, edit: Edit) -> Result<(), String> {
    match edit {
        Edit::Insert(kind) => {
            let id = (1..)
                .map(|n| format!("widget_{n}"))
                .find(|id| root.find(id).is_none())
                .expect("控件 ID 空间可用");
            let mut widget = UiWidget::new(id.clone(), kind);
            if kind == WidgetKind::Container {
                widget.padding = 8.0;
                widget.gap = 6.0;
                widget.background = Some([42, 50, 58, 255]);
            }
            let slot = if root
                .find(selected)
                .is_some_and(|item| matches!(item.kind, WidgetKind::Container | WidgetKind::Button))
            {
                root.find_mut(selected)
            } else {
                root.parent_mut(selected)
            };
            slot.ok_or("请先选中容器或控件")?.children.push(widget);
            *selected = id;
        }
        Edit::Remove => {
            let parent = root.parent_mut(selected).ok_or("请选择可删除的控件")?;
            parent.children.retain(|child| child.id != *selected);
            selected.clone_from(&parent.id);
        }
        Edit::Shift(delta) => {
            let parent = root.parent_mut(selected).ok_or("请选择可移位的控件")?;
            let index = parent
                .children
                .iter()
                .position(|child| child.id == *selected)
                .ok_or("控件已删除")?;
            let target = index
                .saturating_add_signed(delta as isize)
                .min(parent.children.len() - 1);
            parent.children.swap(index, target);
        }
        Edit::Direction => {
            let widget = root.find_mut(selected).ok_or("请选择容器")?;
            widget.direction = if widget.direction == FlexDirection::Row {
                FlexDirection::Column
            } else {
                FlexDirection::Row
            };
        }
        Edit::Properties(values) => fields::apply(root, selected, &values)?,
        Edit::Position(id, position) => {
            let widget = root.find_mut(&id).ok_or("拖动的控件已移除")?;
            widget.position = Some(position);
            // 拖动转为明确左上坐标，不能叠加旧的右下锚点或居中负边距。
            widget.style.left = Val::Auto;
            widget.style.top = Val::Auto;
            widget.style.right = Val::Auto;
            widget.style.bottom = Val::Auto;
            widget.style.position_type = PositionType::Relative;
            widget.style.margin = UiRect::ZERO;
            *selected = id;
        }
    }
    Ok(())
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<UiLayouts>()
        .add_message::<UiLayoutAction>()
        .add_systems(
            Update,
            (buttons, canvas::input)
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
            (view::mount, view::present)
                .chain()
                .after(super::presentation::sync)
                .in_set(SceneEditorSystemSet::Presentation),
        )
        .add_systems(PostUpdate, view::fit_canvas.before(bevy::ui::UiSystems::Layout));
}

fn buttons(
    editor: Res<SceneEditorState>,
    mut docs: ResMut<DocumentState>,
    mut state: ResMut<UiLayouts>,
    buttons: Query<(&Action, &EditorButtonClick), Changed<EditorButtonClick>>,
    inputs: Query<(&fields::Field, &EditableText)>,
    mut focus: ResMut<InputFocus>,
    mut preview: MessageReader<UiLayoutAction>,
) {
    if !editor.open || editor.close_requested || active(&docs).is_none() {
        preview.clear();
        return;
    }
    if let Some(action) = preview.read().filter(|action| action.preview).last() {
        state.notice = format!("预览触发 {}", action.name);
    }
    let Some(action) = buttons
        .iter()
        .find_map(|(action, click)| click.is_clicked().then(|| action.clone()))
    else {
        return;
    };
    if state.edit_pending() {
        state.notice = "正在采用上一笔界面编辑".into();
        return;
    }
    if inputs.iter().any(|(_, text)| input_pending(text)) {
        return;
    }
    focus.clear();
    let edit = match action {
        Action::KeepDraft | Action::Reload => {
            if let Some(path) = docs.active.clone() {
                if action == Action::KeepDraft {
                    docs.keep_draft(&path);
                } else {
                    docs.reload_document(&path, false);
                }
            }
            return;
        }
        Action::Source => {
            if let Some(doc) = docs.current_mut() {
                doc.mode = ViewMode::Edit;
            }
            return;
        }
        Action::Preview(interactive) => {
            state.interactive = interactive;
            return;
        }
        Action::Narrow(narrow) => {
            state.narrow = narrow;
            state.design = false;
            return;
        }
        Action::Design => {
            state.design = true;
            state.narrow = false;
            return;
        }
        _ if state.interactive => return,
        Action::Select(id) => {
            state.selected = id;
            return;
        }
        Action::Insert(kind) => Edit::Insert(kind),
        Action::Remove => Edit::Remove,
        Action::Shift(delta) => Edit::Shift(delta),
        Action::Direction => Edit::Direction,
        Action::Properties => Edit::Properties(
            inputs
                .iter()
                .map(|(field, input)| (*field, input.value().to_string()))
                .collect(),
        ),
    };
    state.submit(&docs, edit);
}

fn sync(
    mut state: ResMut<UiLayouts>,
    mut docs: ResMut<DocumentState>,
    editor: Res<SceneEditorState>,
) {
    let current = editor.open.then(|| active(&docs)).flatten();
    let result = state
        .bypass_change_detection()
        .job
        .as_mut()
        .and_then(|job| check_ready(&mut job.task));
    if let Some(result) = result {
        let job = state.job.take().expect("回执属于当前后台任务");
        if let Some(doc) = docs.documents.iter_mut().find(|doc| {
            doc.path == job.key.path
                && doc.generation == job.key.generation
                && doc.revision == job.key.revision
        }) {
            match result {
                Ok(ready) => {
                    let mut chosen = state.selected.clone();
                    if let Some((source, pages, selected)) = ready.document {
                        doc.replace_content(source, pages, default());
                        chosen = selected;
                    }
                    if current.as_ref() == Some(&job.key) {
                        let same = state.key.as_ref().is_some_and(|key| {
                            key.path == job.key.path && key.generation == job.key.generation
                        });
                        if !same || ready.layout.root.find(&chosen).is_none() {
                            chosen = ready.layout.root.id.clone();
                        }
                        state.selected = chosen;
                        state.key = Some(Identity {
                            revision: doc.revision,
                            ..job.key
                        });
                        state.layout = Some(ready.layout);
                        state.notice.clear();
                    }
                }
                Err(error) if current.as_ref() == Some(&job.key) => {
                    state.notice = error;
                    if !job.editing {
                        state.key = Some(job.key);
                        state.layout = None;
                    }
                }
                Err(_) => {}
            }
        }
    }
    let current = editor.open.then(|| active(&docs)).flatten();
    if state.job.is_some() || state.key == current {
        return;
    }
    if let (Some(key), Some(doc)) = (current, docs.current()) {
        // 文件或源码修订采用前隐藏上一个界面，避免异步结果落到别的标签。
        state.layout = None;
        state.interactive = false;
        state.notice = "正在读取界面…".into();
        let source = doc.source.clone();
        state.job = Some(Job {
            key,
            editing: false,
            task: AsyncComputeTaskPool::get().spawn(async move {
                Ok(Prepared {
                    layout: Arc::new(UiLayout::parse_document(&source)?),
                    document: None,
                })
            }),
        });
    } else {
        state.key = None;
        state.layout = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_rust_ui_documents_enter_the_canvas() {
        let source = UiLayout::default().to_rust();
        assert!(is_document("ui/menu.rs", &source));
        assert!(is_document("ui/menu.RS", &source));
        assert!(is_document("ui/legacy.bsn", "#root Node {}"));
        assert!(!is_document("src/main.rs", "fn main() {}"));
        assert!(!is_document("src/main.rs", "// example: bsn!\nfn main() {}"));
        assert!(!is_document("notes.txt", &source));
    }

    #[test]
    fn dragging_right_bottom_anchored_widget_commits_one_unambiguous_left_top_position() {
        let mut layout = UiLayout::parse(r#"#root Node { width: percent(100), height: percent(100) }
            Children [ #bar Node { position_type: PositionType::Absolute, right: px(18), bottom: px(14),
                width: px(414), margin: UiRect::left(px(-207)) } UiBind("hud.bar") ]"#).unwrap();
        let mut selected = "root".to_string();
        apply(&mut layout.root, &mut selected, Edit::Position("bar".into(), Vec2::new(40.0, 70.0))).unwrap();
        let bar = layout.root.find("bar").unwrap();
        assert_eq!(bar.position, Some(Vec2::new(40.0, 70.0)));
        assert_eq!((bar.style.right, bar.style.bottom), (Val::Auto, Val::Auto));
        assert_eq!(bar.style.margin, UiRect::ZERO);
        assert_eq!(bar.binding, "hud.bar");
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }
}
