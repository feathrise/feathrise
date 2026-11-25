//! 当前文件的 GLB 内容目录，按页投影有限行。剪贴板持有明确来源的草稿快照。

use super::super::{
    context::AnimationContext,
    data::{AnimationDocument, ContentKind, ContentSpan},
    resources::{AnimationEditorState, PendingAction, SelectionOutput, TaskResult, Work},
    types::Command,
};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    tasks::IoTaskPool,
    text::{EditableText, FontSourceTemplate, TextEdit},
    ui::RelativeCursorPosition,
};
use feathrise_editor_animation_model::{
    assembly::AnimationSystems,
    media::{AssetPreviewState, AssetPreviewWorkspace},
};
use feathrise_editor_core::{EditorTextInput, FONT_PATH, SceneEditorSystemSet};
use feathrise_editor_ui::{
    EditorButtonClick, InteractionButton, colors, localization::label_scene, metrics,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
mod drag;
mod output;
mod view;
use output::export;
pub(crate) use view::scene;
use view::sync;

const ROWS: usize = 6;
#[derive(Component, Clone, Copy, Default)]
struct Body;
#[derive(Component, Clone, Copy, Default)]
struct Output;
#[derive(Component, Clone, Copy, Default)]
struct OutputName;
#[derive(Component, Clone, Copy, Default)]
struct RowText(usize);

#[derive(Clone, Copy, Component, PartialEq, Eq)]
enum Button {
    Open,
    Kind(ContentKind),
    Row(usize),
    Previous,
    Next,
    Copy,
    Paste,
    Adapt,
    Export,
    Confirm,
    Overwrite,
    Cancel,
}

#[derive(Clone)]
pub(super) struct Copied {
    document: Arc<AnimationDocument>,
    spans: Arc<[ContentSpan]>,
    root: PathBuf,
    origin: PathBuf,
}

#[derive(Resource, Default)]
pub(super) struct State {
    open: bool,
    kind: ContentKind,
    page: usize,
    spans: Vec<ContentSpan>,
    anchor: Option<usize>,
    shown: Option<Arc<AnimationDocument>>,
    path: Option<PathBuf>,
    root: PathBuf,
    export: bool,
    exporting: bool,
    confirming: bool,
    show_feedback: bool,
    copied: Option<Copied>,
    drag: Option<Copied>,
    pending_drop: Option<(PathBuf, Copied)>,
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<State>()
        .add_systems(
            Update,
            input
                .in_set(AnimationSystems::Input)
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (sync, view::feedback)
                .chain()
                .in_set(AnimationSystems::Presentation)
                .in_set(SceneEditorSystemSet::Presentation),
        );
    drag::install(app);
}

fn selected(spans: &[ContentSpan], kind: ContentKind, i: usize) -> bool {
    spans
        .iter()
        .any(|s| s.kind == kind && (s.first..=s.last).contains(&i))
}

#[cfg(feature = "validation")]
pub(crate) fn original_button(world: &mut World, action: &str) -> Option<Entity> {
    let button = match action {
        "open" => Button::Open,
        "first" => Button::Row(0),
        "second" => Button::Row(1),
        "copy" => Button::Copy,
        "paste" => Button::Paste,
        "export" => Button::Export,
        "confirm" => Button::Confirm,
        _ => return None,
    };
    world
        .query::<(Entity, &Button, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(e, b, n, v)| {
            (*b == button && n.size().min_element() > 1. && v.get()).then_some(e)
        })
}

impl State {
    fn select(&mut self, row: usize, ctrl: bool, shift: bool) {
        let kind = self.kind;
        let anchor = if shift {
            self.anchor.unwrap_or(row)
        } else {
            row
        };
        if !ctrl {
            self.spans.clear();
        }
        if ctrl && !shift && selected(&self.spans, kind, row) {
            let mut split = Vec::new();
            self.spans.retain(|s| {
                if s.kind != kind || row < s.first || row > s.last {
                    return true;
                }
                if s.first < row {
                    split.push(ContentSpan {
                        last: row - 1,
                        ..*s
                    });
                }
                if s.last > row {
                    split.push(ContentSpan {
                        first: row + 1,
                        ..*s
                    });
                }
                false
            });
            self.spans.extend(split);
        } else {
            self.spans.push(ContentSpan {
                kind,
                first: anchor.min(row),
                last: anchor.max(row),
            });
        }
        self.anchor = Some(anchor);
    }
    pub(super) fn freeze(&self) -> Option<Copied> {
        if self.spans.is_empty() || self.spans.iter().any(|s| s.kind != ContentKind::Animation) {
            return None;
        }
        Some(Copied {
            document: self.shown.clone()?,
            spans: self.spans.clone().into(),
            root: self.root.clone(),
            origin: self.path.clone()?,
        })
    }
}

fn queue_paste(
    state: &mut AnimationEditorState,
    source: Copied,
    adapt: bool,
) -> Result<(), String> {
    if state.busy() {
        return Err("请等待当前动画任务完成".into());
    }
    if state.current().is_some_and(|s| s.pose.is_some()) {
        return Err("请先插入或还原当前临时姿势".into());
    }
    state.stop_rig_preview();
    if !state.can_create_clip() {
        return Err("当前目标没有可写入动画的节点".into());
    }
    state.pending = Some(PendingAction::Command(Command::ImportClips {
        source: source.document,
        rows: source.spans,
        adapt,
    }));
    state.playing = false;
    state.changed();
    Ok(())
}

fn input(
    mut contexts: ParamSet<(AnimationContext, Option<ResMut<InputFocus>>)>,
    mut panel: ResMut<State>,
    mut state: ResMut<AnimationEditorState>,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    buttons: Query<(&Button, &EditorButtonClick), Changed<EditorButtonClick>>,
    body: Query<&RelativeCursorPosition, With<Body>>,
    mut fields: Query<(Entity, &mut EditableText), With<OutputName>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
) {
    let (available, focused) = {
        let c = contexts.p0();
        (
            c.available(&preview, &media)
                && media.active(c.host.tab)
                && !c.host.weights_own_viewport
                && !c.guard.blocked()
                && !c.host.scene_dragging
                && (panel.pending_drop.is_some() || c.host.selected_asset == preview.selected_path),
            c.text_input_focused(),
        )
    };
    if !available || state.busy() {
        return;
    }
    let Some(path) = state.active.clone() else {
        return;
    };
    let Some(doc) = state.sessions.get(&path).map(|s| s.document.clone()) else {
        return;
    };
    if panel.path.as_ref() != Some(&path)
        || !panel
            .shown
            .as_ref()
            .is_some_and(|shown| Arc::ptr_eq(shown, &doc))
    {
        return;
    }
    if panel
        .pending_drop
        .as_ref()
        .is_some_and(|(target, _)| target == &path)
    {
        let (_, source) = panel.pending_drop.take().unwrap();
        panel.show_feedback = true;
        if let Err(error) = queue_paste(&mut state, source, false) {
            state.status = error;
            state.changed();
        }
        return;
    }
    let keys = keys.as_deref();
    let ctrl = keys.is_some_and(|k| k.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]));
    let shift = keys.is_some_and(|k| k.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
    let mut action = buttons
        .iter()
        .find(|(_, c)| c.is_clicked())
        .map(|(b, _)| *b);
    if action.is_none() && panel.open && !focused && ctrl && body.iter().any(|b| b.cursor_over()) {
        if keys.is_some_and(|k| k.just_pressed(KeyCode::KeyC)) {
            action = Some(Button::Copy);
        }
        if keys.is_some_and(|k| k.just_pressed(KeyCode::KeyV)) {
            action = Some(Button::Paste);
        }
        if keys.is_some_and(|k| k.just_pressed(KeyCode::KeyA)) && doc.content_count(panel.kind) > 0
        {
            let kind = panel.kind;
            panel.spans.retain(|s| s.kind != kind);
            panel.spans.push(ContentSpan {
                kind,
                first: 0,
                last: doc.content_count(kind) - 1,
            });
        }
    }
    let Some(action) = action else {
        return;
    };
    state.mouse_owned = true;
    if matches!(
        action,
        Button::Copy
            | Button::Paste
            | Button::Adapt
            | Button::Export
            | Button::Confirm
            | Button::Overwrite
    ) {
        panel.show_feedback = true;
    }
    match action {
        Button::Open => panel.open = !panel.open,
        Button::Kind(kind) => {
            panel.kind = kind;
            panel.page = 0;
            panel.anchor = None;
        }
        Button::Row(i) => {
            let row = panel.page * ROWS + i;
            if row < doc.content_count(panel.kind) {
                panel.select(row, ctrl, shift);
            }
        }
        Button::Previous => panel.page = panel.page.saturating_sub(1),
        Button::Next => {
            panel.page =
                (panel.page + 1).min(doc.content_count(panel.kind).saturating_sub(1) / ROWS)
        }
        Button::Copy => match panel.freeze() {
            Some(copied) => {
                panel.copied = Some(copied);
                state.status = "已复制所选动画，打开目标 GLB 后粘贴".into();
            }
            None => state.status = "请先选择一个或多个动画片段".into(),
        },
        Button::Paste | Button::Adapt => {
            let result = panel
                .copied
                .clone()
                .filter(|c| c.root == panel.root)
                .ok_or_else(|| "请先从来源 GLB 复制动作".to_owned())
                .and_then(|c| queue_paste(&mut state, c, action == Button::Adapt));
            if let Err(error) = result {
                state.status = error;
            }
        }
        Button::Export => {
            if panel.spans.is_empty() {
                state.status = "请先选择要另存的 GLB 内容".into();
            } else if let Ok((entity, mut edit)) = fields.single_mut() {
                let relative = path.strip_prefix(&panel.root).unwrap_or(path.as_path());
                let stem = relative
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("内容");
                edit.clear();
                edit.editor.set_text(
                    &relative
                        .with_file_name(format!("{stem}_所选.glb"))
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
                edit.queue_edit(TextEdit::SelectAll);
                if let Some(mut f) = contexts.p1() {
                    f.set(entity, FocusCause::Navigated);
                }
                panel.export = true;
            }
        }
        Button::Confirm | Button::Overwrite if panel.export => {
            let Ok((_, edit)) = fields.single() else {
                return;
            };
            if edit.is_composing() {
                return;
            }
            let name = edit.value().to_string();
            let root = panel.root.clone();
            let spans = panel.spans.clone();
            let overwrite = state
                .export_overwrite
                .take()
                .filter(|(source, _, _)| {
                    action == Button::Overwrite
                        && std::sync::Weak::ptr_eq(source, &Arc::downgrade(&doc))
                })
                .map(|(_, path, baseline)| (path, baseline));
            let generation = state
                .model_generation
                .as_ref()
                .filter(|(p, _)| p == &path)
                .map(|(_, g)| *g);
            state.selection_written = false;
            let ticket = Arc::downgrade(state.composition_ticket.insert(Arc::new(())));
            state.work = Some(Work {
                path,
                generation,
                saving: true,
                task: IoTaskPool::get().spawn(async move {
                    TaskResult::Exported(export(&root, &name, doc, spans, overwrite, ticket))
                }),
            });
            panel.exporting = true;
            if let Some(mut f) = contexts.p1() {
                f.clear();
            }
        }
        Button::Cancel | Button::Confirm | Button::Overwrite => {
            panel.export = false;
            state.export_overwrite = None;
            if let Some(mut f) = contexts.p1() {
                f.clear();
            }
        }
    }
    state.changed();
}
