//! 原按钮消费、后台结果采用和固定行池表现；绑定本身不进入主帧。

use std::sync::atomic::Ordering;

use bevy::{prelude::*, tasks::futures::check_ready};
use feathrise_editor_ui::{EditorButtonClick, localization::EditorLocale};

use super::{
    super::{OpenAssetPreview, animation::PreviewRigAnimation},
    backend::{self, RigBackend},
    context::{Candidate, Context},
    resources::*,
    scene::{RigMotionHost, RigToolHost},
    types::*,
};
use crate::{
    resources::{EditorAssetCatalog, SceneEditorLayoutState, SceneEditorState},
    ui::sidebars::{EditorNavigatorTab, EditorRightSidebarTab, SceneEditorSidebarState},
};

pub(super) fn actions(
    mut contexts: ParamSet<(Context, (ResMut<SceneEditorSidebarState>, Option<ResMut<SceneEditorLayoutState>>, ResMut<SceneEditorState>))>,
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
    let context = contexts.p0();
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
    let central = context.central();
    let session = current_session(&state, source);
    let motion_page = central && has_project_motion(source, session);
    if action == RigAction::Launch && state.work.is_none() {
        // 已蒙皮模型直接使用属性里的工程动作，隐藏右栏时才使用左树里的备用入口。
        if source.entry == Entry::Skinned && motion_page && context.motion_properties_shown() {
            return;
        }
        if state.session.as_ref().is_some_and(|session| session.matches(source)) {
            state.panel_open = true;
        } else {
            inspect(&mut state, source);
        }
        if !central {
            // 原侧栏可以启动工作流；同一模型移入中央标签后有更大的关节编辑画布。
            open.write(OpenAssetPreview {
                path: source.path.to_owned(),
            });
        }
        let picking_reference = state.session.as_ref().is_some_and(Session::reference_mode);
        let show_motion = central && has_project_motion(source, state.session.as_ref());
        let target = (picking_reference || show_motion).then(|| source.path.to_owned());
        let (mut sidebar, layout, mut editor) = contexts.p1();
        if let Some(target) = target {
            // 固定当前模型的属性身份。左栏引用高亮读取独立的参考选择。
            editor.selected_asset = Some(target);
            if picking_reference {
                sidebar.navigator = EditorNavigatorTab::Assets;
            }
            sidebar.reveal_asset_properties();
        }
        if central && !show_motion {
            sidebar.rigging_available = true;
            sidebar.right = EditorRightSidebarTab::Rigging;
        }
        if let Some(mut layout) = layout {
            // 绑骨的两个侧栏恢复到原工作布局，预览继续使用同一个画布。
            if let Some((content, _)) = layout.focus_restore.take() {
                layout.content_visible = content;
            }
            layout.inspector_visible = true;
            layout.content_visible |= picking_reference;
        }
        return;
    }
    if !(if motion_page {
        context.motion_properties_shown()
    } else {
        context.rigging_sidebar_shown()
    }) {
        return;
    }
    if action == RigAction::ChooseProjectMotion {
        // 查看模型属性只改变展示，收到这个明确动作后才启动目录 IO。
        if motion_page && source.entry == Entry::Skinned && state.work.is_none() && can_choose_project_motion(session) {
            inspect(&mut state, source);
        }
        return;
    }
    if action == RigAction::Close && (motion_page || session.is_some()) {
        state.cancel_work();
        state.panel_open = false;
        let (mut sidebar, _, _) = contexts.p1();
        sidebar.reveal_properties();
        return;
    }
    if session.is_none() || state.work.is_some() {
        return;
    }
    let session = state.session.as_mut().expect("已核对当前绑骨会话");
    match action {
        RigAction::Retry if source.entry == Entry::Unskinned => inspect(&mut state, source),
        RigAction::SwitchBackend if source.entry == Entry::Unskinned => {
            state.backend = match state.backend {
                RigBackend::Native => RigBackend::Blender,
                RigBackend::Blender => RigBackend::Native,
            };
            inspect(&mut state, source);
        }
        RigAction::ResetMarks => {
            if let Some(draft) = &mut session.draft {
                draft.points.clone_from(&draft.suggested);
                draft.selected = None;
            }
        }
        RigAction::ToggleMirror if session.draft.is_some() => {
            state.independent_markers = !state.independent_markers;
        }
        RigAction::PickFile if session.model_for_preview().is_some() => {
            session.toggle_picker(Picker::File)
        }
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
            state.work = Some(Work::start_async(Some(source.key()), move |cancel| async move {
                Completion::Binding(backend::bind(&input, &reference, &landmarks, cancel).await)
            }));
        }
        RigAction::Preview => {
            let (Some(tab), Some(model_path), Some(file)) = (
                context.active_tab_id(),
                session.model_for_preview(),
                session.file(),
            )
            else {
                return;
            };
            if session.clip < file.clips.len() {
                preview.write(PreviewRigAnimation {
                    // 预览来源与画布宿主分开。即使同帧切走标签，旧按钮也不能把动作送到新模型。
                    host_tab: tab,
                    host_path: source.path.to_owned(),
                    host_generation: source.generation,
                    asset_root: source.root.to_path_buf(),
                    model_path: model_path.to_owned(),
                    animation_path: file.path.clone(),
                    clip_index: session.clip,
                });
            }
        }
        _ => {}
    }
}

pub(super) fn sync_sidebar(
    mut commands: Commands,
    mut contexts: ParamSet<(Context, ResMut<SceneEditorSidebarState>)>,
    state: Res<RiggingState>,
    tool_host: Query<Entity, With<RigToolHost>>,
    mut motion_host: Query<(Entity, &mut Node), With<RigMotionHost>>,
    panels: Query<(Entity, &ChildOf), With<RigPanel>>,
) {
    let (available, motion, owns_properties) = {
        let context = contexts.p0();
        let source = context.central().then(|| context.candidate()).flatten();
        let session = source.and_then(|source| current_session(&state, source));
        let motion = source.is_some_and(|source| has_project_motion(source, session));
        (session.is_some() && !motion, motion, context.owns_properties())
    };
    if let (Ok((host, mut node)), Ok(tool), Ok((entity, parent))) =
        (motion_host.single_mut(), tool_host.single(), panels.single())
    {
        let display = if motion && owns_properties { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let wanted = if motion { host } else { tool };
        if parent.parent() != wanted {
            // 生成成功后同一个项目动作选择器进入属性页，保留当前片段和分页状态。
            commands.entity(entity).insert(ChildOf(wanted));
        }
    }
    let mut sidebar = contexts.p1();
    let tool_selected = available && matches!(sidebar.right,
        EditorRightSidebarTab::Tools | EditorRightSidebarTab::Environment);
    if sidebar.rigging_available == available && !tool_selected {
        return; // 作者可切到属性查看，稳定会话不抢回右栏。
    }
    let open_session = !sidebar.rigging_available && available;
    sidebar.rigging_available = available;
    if open_session || tool_selected {
        sidebar.right = EditorRightSidebarTab::Rigging;
    } else if !available && sidebar.right == EditorRightSidebarTab::Rigging {
        if motion && owns_properties {
            sidebar.reveal_asset_properties();
        } else {
            sidebar.right = sidebar.left.work_page();
        }
    }
}

fn has_project_motion(source: Candidate<'_>, session: Option<&Session>) -> bool {
    source.entry == Entry::Skinned || session.is_some_and(|session| session.result.is_some())
}

fn current_session<'a>(state: &'a RiggingState, source: Candidate<'_>) -> Option<&'a Session> {
    state
        .session
        .as_ref()
        .filter(|session| state.panel_open && session.matches(source))
}

fn can_choose_project_motion(session: Option<&Session>) -> bool {
    session.is_none_or(|session| {
        matches!(session.notice, Notice::Failed | Notice::NoClips | Notice::Cancelled)
    })
}

fn inspect(state: &mut RiggingState, source: Candidate<'_>) {
    let key = source.key();
    let mut next = Session::new(key.clone(), source.entry);
    if let Some(previous) = state.session.as_ref()
        && previous.reference_mode() && previous.matches(source)
    {
        // 更换后端或重查时重新验证同一显式选择，切模型时从空选择开始。
        next.reference_path.clone_from(&previous.reference_path);
    }
    let previous = state.session.replace(next);
    let entry = source.entry;
    let backend = state.backend;
    state.panel_open = true;
    state.work = Some(Work::start_async(Some(key.clone()), move |cancel| async move {
        // 旧目录可能很大，移动所有权后在原单飞槽释放。
        drop(previous);
        match entry {
            Entry::Unskinned => Completion::Inspection(
                backend::inspect(&key.root, &key.path, backend, cancel)
                    .await
                    .map(|inspection| {
                        let has_reference = inspection.animations.iter().any(|asset| asset.can_reference);
                        (inspection, has_reference)
                    }),
            ),
            Entry::Skinned => Completion::Catalog(backend::catalog(&key.root, &cancel)),
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
        Completion::Inspection(Ok((inspection, has_reference))) => {
            if !LANDMARKS.iter().all(|(key, _)| {
                inspection
                    .landmarks
                    .get(*key)
                    .is_some_and(|point| Vec3::from_array(*point).is_finite())
            }) {
                failure(session, "关节点结果缺失或坐标无效".into());
                return;
            }
            session.notice = if has_reference {
                Notice::Review
            } else {
                Notice::NoReference
            };
            session.animations = inspection.animations;
            session.resolve_reference();
            session.warnings = inspection.warnings;
            session.draft = Some(MarkerDraft {
                input: inspection.input,
                suggested: inspection.landmarks.clone(),
                frame: RigFrame::from_landmarks(&inspection.landmarks),
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
            if session.selected_file.is_some() {
                session.toggle_picker(Picker::File);
            }
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
    let session = candidate.and_then(|source| current_session(&state, source));
    let motion_page = context.central() && candidate.is_some_and(|source| has_project_motion(source, session));
    let skinned = candidate.is_some_and(|source| source.entry == Entry::Skinned);
    let busy = state.work.is_some();
    for (mut node, action, panel, choices, marker_tools) in &mut nodes {
        let show = if panel {
            motion_page || session.is_some()
        } else if choices {
            session.is_some_and(|session| session.picker.is_some()) && !busy
        } else if marker_tools {
            session.is_some_and(|session| session.draft.is_some())
        } else if action == Some(&RigAction::Launch) {
            candidate.is_some() && !busy && !(skinned && motion_page && context.motion_properties_shown())
        } else if action == Some(&RigAction::ChooseProjectMotion) {
            skinned && motion_page && !busy && can_choose_project_motion(session)
        } else if action == Some(&RigAction::Close) {
            session.is_some()
        } else {
            session.is_some_and(|session| match action {
                Some(RigAction::SwitchBackend) => !busy && session.entry == Entry::Unskinned,
                Some(RigAction::Retry) => {
                    !busy
                        && session.entry == Entry::Unskinned
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
                Some(RigAction::ToggleMirror) => !busy && session.draft.is_some(),
                Some(RigAction::Bind) => {
                    !busy && session.draft.is_some() && session.file().is_some()
                }
                Some(RigAction::PickFile) => {
                    !busy && session.model_for_preview().is_some() && !session.animations.is_empty()
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
                        "rig.motion.choose_project"
                    } else {
                        "rig.launch"
                    },
                )
                .to_owned(),
            (RigText::Title, _) => locale
                .text(if motion_page { "rig.motion.title" } else { "rig.title" })
                .to_owned(),
            (RigText::Close, _) => locale
                .text(if busy {
                    "rig.cancel"
                } else if motion_page {
                    "rig.motion.collapse"
                } else {
                    "rig.close"
                })
                .to_owned(),
            (RigText::Backend, _) => locale
                .text(match state.backend {
                    RigBackend::Native => "rig.backend.native",
                    RigBackend::Blender => "rig.backend.blender",
                })
                .to_owned(),
            (RigText::Mirror, _) => locale.text(if state.independent_markers {
                "rig.mirror.off"
            } else {
                "rig.mirror.on"
            }).to_owned(),
            (RigText::Notice, Some(session)) => notice(locale, session),
            (RigText::Notice, None) if motion_page => locale.text("rig.motion.idle").to_owned(),
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
            (RigText::Reference, Some(session)) => match (session.file(), &session.reference_path) {
                (Some(file), _) => locale.format("rig.file.reference", &[("file", file.path.clone())]),
                (None, Some(path)) => locale.format("rig.file.reference.incompatible", &[("file", path.clone())]),
                (None, None) => locale.text("rig.file.reference.pick").to_owned(),
            },
            (RigText::File, Some(session)) => locale.format(
                "rig.file.animation",
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
