//! 原按钮消费、后台结果采用和固定行池表现；绑定本身不进入主帧。

use std::sync::atomic::Ordering;

use bevy::{prelude::*, tasks::futures::check_ready};
use feathrise_editor_ui::{EditorButtonClick, localization::EditorLocale};

use super::{
    super::{OpenAssetPreview, animation::PreviewRigAnimation},
    backend,
    context::{Candidate, Context},
    resources::*,
    types::*,
};
use crate::resources::EditorAssetCatalog;

pub(super) fn actions(
    context: Context,
    mut state: ResMut<RiggingState>,
    buttons: Query<(&RigAction, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut preview: MessageWriter<PreviewRigAnimation>,
    mut open: MessageWriter<OpenAssetPreview>,
) {
    let Some(action) = buttons
        .iter()
        .find_map(|(action, click)| click.is_clicked().then_some(*action))
    else {
        return;
    };
    let Some(source) = context.candidate() else {
        return;
    };
    if !context.can_input()
        || context
            .mouse_guard
            .as_ref()
            .is_some_and(|guard| guard.blocked())
        || state.owns_mouse()
    {
        return;
    }
    if action == RigAction::Launch && state.work.is_none() {
        if state
            .session
            .as_ref()
            .is_some_and(|session| session.matches(source))
        {
            state.panel_open = !state.panel_open;
        } else {
            inspect(&mut state, source);
        }
        if state.panel_open && !context.central() {
            // 原侧栏可以启动工作流；同一模型移入中央标签后有更大的关节编辑画布。
            open.write(OpenAssetPreview {
                path: source.path.to_owned(),
            });
        }
        return;
    }
    if !state.panel_open
        || state
            .session
            .as_ref()
            .is_none_or(|session| !session.matches(source))
    {
        return;
    }
    if action == RigAction::Close {
        state.cancel_work();
        state.panel_open = false;
        return;
    }
    if state.work.is_some() {
        return;
    }
    let session = state.session.as_mut().expect("已核对当前绑骨会话");
    match action {
        RigAction::Retry => inspect(&mut state, source),
        RigAction::ResetMarks => {
            if let Some(draft) = &mut session.draft {
                draft.points.clone_from(&draft.suggested);
                draft.selected = None;
            }
        }
        RigAction::PickFile => session.toggle_picker(Picker::File),
        RigAction::PickClip if session.model_for_preview().is_some() => {
            session.toggle_picker(Picker::Clip)
        }
        RigAction::Choose(row) if row < PICKER_ROWS => session.choose(row),
        RigAction::Previous => session.page = session.page.saturating_sub(1),
        RigAction::Next if (session.page + 1) * PICKER_ROWS < session.choice_count() => {
            session.page += 1
        }
        RigAction::Bind => {
            let Some(file) = session.file().filter(|file| file.can_reference) else {
                return;
            };
            let Some(draft) = &session.draft else {
                return;
            };
            let reference = file.path.clone();
            let input = draft.input.clone();
            let landmarks = draft.points.clone();
            session.error = None;
            session.notice = Notice::Binding;
            session.picker = None;
            state.work = Some(Work::start(Some(source.key()), move |cancel| {
                Completion::Binding(backend::bind(&input, &reference, &landmarks, cancel))
            }));
        }
        RigAction::Preview => {
            let (Some(model_path), Some(file)) = (session.model_for_preview(), session.file())
            else {
                return;
            };
            if session.clip < file.clips.len() {
                preview.write(PreviewRigAnimation {
                    model_path: model_path.to_owned(),
                    animation_path: file.path.clone(),
                    clip_index: session.clip,
                });
            }
        }
        _ => {}
    }
}

fn inspect(state: &mut RiggingState, source: Candidate<'_>) {
    let key = source.key();
    let previous = state
        .session
        .replace(Session::new(key.clone(), source.entry));
    let entry = source.entry;
    state.panel_open = true;
    state.work = Some(Work::start(Some(key.clone()), move |cancel| {
        // 旧目录可能很大，移动所有权后在原单飞槽释放。
        drop(previous);
        match entry {
            Entry::Unskinned => Completion::Inspection(
                backend::inspect(&key.root, &key.path, cancel).map(|inspection| {
                    let references = inspection
                        .animations
                        .iter()
                        .enumerate()
                        .filter_map(|(index, asset)| asset.can_reference.then_some(index))
                        .collect();
                    (inspection, references)
                }),
            ),
            Entry::Skinned => Completion::Catalog(backend::catalog(&key.root, cancel)),
        }
    }));
}

pub(super) fn poll(
    context: Context,
    mut state: ResMut<RiggingState>,
    mut catalog: Option<ResMut<EditorAssetCatalog>>,
) {
    if state.work.is_none() {
        return;
    }
    let source = context.candidate();
    let panel_open = state.panel_open;
    let Some(work) = &mut state.work else {
        return;
    };
    let current = panel_open
        && work
            .source
            .as_ref()
            .is_some_and(|key| source.is_some_and(|source| source.same_generation(key)));
    if !current {
        work.cancelled.store(true, Ordering::Release);
    }
    let Some(completion) = check_ready(&mut work.task) else {
        return;
    };
    let work = state.work.take().expect("完成值属于当前单飞槽");
    if matches!(completion, Completion::Disposed) {
        return;
    }
    let Some(session) = &mut state.session else {
        retire(&mut state, completion);
        return;
    };
    if !current || work.cancelled.load(Ordering::Acquire) {
        if work.source.as_ref() == Some(&session.source) {
            session.notice = Notice::Cancelled;
        }
        retire(&mut state, completion);
        return;
    }
    match completion {
        Completion::Inspection(Ok((inspection, reference_indices))) => {
            if !LANDMARKS.iter().all(|(key, _)| {
                inspection
                    .landmarks
                    .get(*key)
                    .is_some_and(|point| Vec3::from_array(*point).is_finite())
            }) {
                failure(session, "关节点结果缺失或坐标无效".into());
                return;
            }
            session.selected_file = reference_indices.first().copied();
            session.notice = if session.selected_file.is_some() {
                Notice::Review
            } else {
                Notice::NoReference
            };
            session.reference_indices = reference_indices;
            session.animations = inspection.animations;
            session.warnings = inspection.warnings;
            session.draft = Some(MarkerDraft {
                input: inspection.input,
                suggested: inspection.landmarks.clone(),
                points: inspection.landmarks,
                selected: None,
            });
        }
        Completion::Catalog(Ok(animations)) => {
            // 可预览任意目录条目，由预览的骨架验证说明具体不兼容原因。
            session.selected_file = (!animations.is_empty()).then_some(0);
            session.notice = if session.selected_file.is_some() {
                Notice::ChooseAnimation
            } else {
                Notice::NoClips
            };
            session.animations = animations;
        }
        Completion::Binding(Ok(prepared)) => {
            let (joints, vertices) = (prepared.joints, prepared.vertices);
            session.notice = Notice::Publishing;
            // 只有仍与当前模型代次一致的完整阶段结果才可请求原子发布。
            state.work = Some(Work::start(work.source, move |cancel| {
                Completion::Publish {
                    result: backend::publish(prepared, cancel),
                    joints,
                    vertices,
                }
            }));
        }
        Completion::Publish {
            result: Ok(published),
            joints,
            vertices,
        } => {
            let parent = published
                .path
                .rsplit_once('/')
                .map_or("", |(parent, _)| parent);
            if let Some(catalog) = &mut catalog
                && catalog.current_folder == parent
                && catalog.pending_reveal_revision().is_none()
            {
                // 原目录刷新不改选中资产；作者已经换目录或发起定位时，保留其最新浏览意图。
                catalog.requested_revision = catalog.requested_revision.wrapping_add(1);
            }
            session.result = Some(published.path);
            session.warnings = published.warnings;
            session.draft = None;
            session.notice = Notice::Saved { joints, vertices };
            session.clip = 0;
        }
        Completion::Inspection(Err(error))
        | Completion::Catalog(Err(error))
        | Completion::Binding(Err(error))
        | Completion::Publish {
            result: Err(error), ..
        } => failure(session, error),
        Completion::Disposed => {}
    }
}

fn retire(state: &mut RiggingState, completion: Completion) {
    state.work = Some(Work::start(None, move |_| {
        drop(completion);
        Completion::Disposed
    }));
}

fn failure(session: &mut Session, error: String) {
    warn!("自动绑骨后台任务失败：{error}");
    session.notice = Notice::Failed;
    session.error = Some(error.chars().take(320).collect());
}

pub(super) fn present(
    context: Context,
    state: Res<RiggingState>,
    locale: Option<Res<EditorLocale>>,
    mut nodes: Query<
        (
            &mut Node,
            Option<&RigAction>,
            Has<RigPanel>,
            Has<RigChoices>,
            Has<RigMarkerTools>,
        ),
        Or<(
            With<RigAction>,
            With<RigPanel>,
            With<RigChoices>,
            With<RigMarkerTools>,
        )>,
    >,
    mut labels: Query<(&RigText, &mut Text)>,
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    let candidate = context.candidate();
    let session = state.session.as_ref().filter(|session| {
        state.panel_open && candidate.is_some_and(|source| session.matches(source))
    });
    let busy = state.work.is_some();
    for (mut node, action, panel, choices, marker_tools) in &mut nodes {
        let show = if panel {
            session.is_some()
        } else if choices {
            session.is_some_and(|session| session.picker.is_some()) && !busy
        } else if marker_tools {
            session.is_some_and(|session| session.draft.is_some())
        } else if action == Some(&RigAction::Launch) {
            candidate.is_some() && !busy
        } else {
            session.is_some_and(|session| match action {
                Some(RigAction::Close) => true,
                Some(RigAction::Retry) => {
                    !busy
                        && matches!(
                            session.notice,
                            Notice::Failed
                                | Notice::NoReference
                                | Notice::NoClips
                                | Notice::Cancelled
                                | Notice::Saved { .. }
                        )
                }
                Some(RigAction::ResetMarks) => !busy && session.draft.is_some(),
                Some(RigAction::Bind) => {
                    !busy && session.draft.is_some() && session.file().is_some()
                }
                Some(RigAction::PickFile) => {
                    !busy
                        && !session.animations.is_empty()
                        && (!session.reference_mode() || !session.reference_indices.is_empty())
                }
                Some(RigAction::PickClip) => {
                    !busy
                        && session.model_for_preview().is_some()
                        && session.file().is_some_and(|file| !file.clips.is_empty())
                }
                Some(RigAction::Preview) => {
                    !busy
                        && session.model_for_preview().is_some()
                        && session
                            .file()
                            .is_some_and(|file| session.clip < file.clips.len())
                }
                Some(RigAction::Choose(row)) => !busy && session.choice(*row).is_some(),
                Some(RigAction::Previous) => !busy && session.picker.is_some() && session.page > 0,
                Some(RigAction::Next) => {
                    !busy
                        && session.picker.is_some()
                        && (session.page + 1) * PICKER_ROWS < session.choice_count()
                }
                _ => false,
            })
        };
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (kind, mut text) in &mut labels {
        let wanted = match (kind, session) {
            (RigText::Launch, _) => locale
                .text(
                    if candidate.is_some_and(|source| source.entry == Entry::Skinned) {
                        "rig.launch.preview"
                    } else {
                        "rig.launch"
                    },
                )
                .to_owned(),
            (RigText::Close, _) => locale
                .text(if busy { "rig.cancel" } else { "rig.close" })
                .to_owned(),
            (RigText::Notice, Some(session)) => notice(locale, session),
            (RigText::Warning, Some(session)) => {
                session.warnings.first().map_or(String::new(), |warning| {
                    locale.format(
                        "rig.warning",
                        &[("message", warning.chars().take(180).collect())],
                    )
                })
            }
            (RigText::SelectedMarker, Some(session)) => session
                .draft
                .as_ref()
                .and_then(|draft| draft.selected.map(|index| (draft, index)))
                .and_then(|(draft, index)| {
                    draft
                        .points
                        .get(LANDMARKS[index].0)
                        .map(|point| (index, point))
                })
                .map_or_else(
                    || locale.text("rig.marker.help").to_owned(),
                    |(index, point)| {
                        format!(
                            "{} · {:.3} / {:.3} / {:.3}",
                            locale.text(LANDMARKS[index].1),
                            point[0],
                            point[1],
                            point[2]
                        )
                    },
                ),
            (RigText::File, Some(session)) => locale.format(
                if session.reference_mode() {
                    "rig.file.reference"
                } else {
                    "rig.file.animation"
                },
                &[(
                    "file",
                    session
                        .file()
                        .map_or(String::new(), |file| file.path.clone()),
                )],
            ),
            (RigText::Clip, Some(session)) => locale.format(
                "rig.clip",
                &[(
                    "clip",
                    session
                        .file()
                        .and_then(|file| file.clips.get(session.clip))
                        .cloned()
                        .unwrap_or_default(),
                )],
            ),
            (RigText::Choice(row), Some(session)) => {
                session.choice(*row).unwrap_or_default().to_owned()
            }
            (RigText::Page, Some(session)) => locale.format(
                "rig.page",
                &[
                    ("page", (session.page + 1).to_string()),
                    (
                        "pages",
                        session.choice_count().div_ceil(PICKER_ROWS).to_string(),
                    ),
                ],
            ),
            _ => String::new(),
        };
        if text.as_str() != wanted {
            **text = wanted;
        }
    }
}

fn notice(locale: EditorLocale, session: &Session) -> String {
    let key = match session.notice {
        Notice::Inspecting => "rig.status.inspecting",
        Notice::Cataloguing => "rig.status.cataloguing",
        Notice::Review => "rig.status.review",
        Notice::Binding => "rig.status.binding",
        Notice::Publishing => "rig.status.publishing",
        Notice::ChooseAnimation => "rig.status.choose",
        Notice::NoReference => "rig.status.no_reference",
        Notice::NoClips => "rig.status.no_clips",
        Notice::Cancelled => "rig.status.cancelled",
        Notice::Failed => {
            return locale.format(
                "rig.status.error",
                &[("message", session.error.clone().unwrap_or_default())],
            );
        }
        Notice::Saved { joints, vertices } => {
            return locale.format(
                "rig.status.saved",
                &[
                    ("file", session.result.clone().unwrap_or_default()),
                    ("joints", joints.to_string()),
                    ("vertices", vertices.to_string()),
                ],
            );
        }
    };
    locale.text(key).to_owned()
}
