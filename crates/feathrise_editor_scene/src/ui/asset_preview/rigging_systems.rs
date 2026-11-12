//! 原按钮消费、后台结果采用和固定行池表现；绑定本身不进入主帧。

use std::sync::atomic::Ordering;

use bevy::{prelude::*, tasks::futures::check_ready};
use feathrise_editor_glb_model::rigging::{RigBackend, RiggingOperations};
use feathrise_editor_ui::{EditorButtonClick, localization::EditorLocale};

use super::{
    super::{AssetPreviewNavigation, AssetPreviewWorkspace, OpenAssetPreview, PickProjectAnimationClip,
        animation::{AnimationEditorState, PreviewRigAnimation}, workspace},
    context::{Candidate, Context},
    resources::*,
    scene::{RigMotionHelp, RigMotionHost, RigToolHost},
    types::*,
};
use crate::{
    asset_catalog::project_files::EditorProjectFileCatalog,
    resources::{EditorAssetCatalog, EditorTextInputGuard, SceneEditorLayoutState, SceneEditorState},
    ui::{project_browser::{ProjectBrowserState, navigation::choose_animation},
        sidebars::{EditorNavigatorTab, EditorRightSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState},
};

pub(super) fn pick_project_animation(
    mut picks: MessageReader<PickProjectAnimationClip>,
    mut contexts: ParamSet<(Context, (
        ResMut<SceneEditorSidebarState>,
        Option<ResMut<SceneEditorLayoutState>>,
        ResMut<SceneEditorState>,
        ResMut<AssetPreviewWorkspace>,
        ResMut<VegetationPresetEditorState>,
    ))>,
    state: Res<RiggingState>,
    text: Res<EditorTextInputGuard>,
    navigation: Res<AssetPreviewNavigation>,
    preview: Res<super::super::AssetPreviewState>,
    mut animation: ResMut<AnimationEditorState>,
    mut rig_preview: MessageWriter<PreviewRigAnimation>,
) {
    let Some(pick) = picks.read().last() else { return; };
    let context = contexts.p0();
    if !context.can_pick_project_animation() || state.owns_mouse() || text.block_shortcuts {
        return;
    }
    let root = context.asset_root().to_path_buf();
    if root.as_os_str().is_empty() {
        return;
    }
    let source = super::super::animation::checked_clip_path(&root, &pick.asset_path);
    let role = context.active_tab_id().zip(context.candidate()).and_then(|(tab, candidate)| {
        let session = current_session(&state, candidate);
        let model = session.and_then(|session| session.result.as_deref())
            .or_else(|| (candidate.entry == Entry::Skinned).then_some(candidate.path))?;
        Some(PreviewRigAnimation {
            model_path: model.to_owned(),
            animation_path: pick.asset_path.clone(),
            clip_index: pick.clip_index,
            host_path: candidate.path.to_owned(),
            host_generation: candidate.generation,
            host_tab: tab,
            asset_root: root.clone(),
            // 目录索引可能刚采用同路径的新文件，不能沿用旧的只读动画文档。
            recheck_source: true,
        })
    });
    let (mut sidebar, layout, mut editor, mut media, mut workspaces) = contexts.p1();
    let source = match source {
        Ok(source) => source,
        Err(error) => {
            editor.status = error;
            return;
        }
    };
    let properties_path = if let Some(role) = role {
        let path = role.host_path.clone();
        animation.cancel_directory_pick();
        // 选择动作不改变当前角色标签，原接收系统继续验证角色与工程身份。
        rig_preview.write(role);
        path
    } else {
        if let Err(error) = workspace::expand(&mut media, &mut workspaces, &pick.asset_path, &preview, &navigation) {
            editor.status = error;
            return;
        }
        let tab = media.expanded.as_ref().expect("原 GLB 打开已激活媒体标签").id;
        animation.queue_directory_pick(root, source, tab, pick.clip_index);
        pick.asset_path.clone()
    };
    editor.selected_asset = Some(properties_path);
    sidebar.reveal_asset_properties();
    if let Some(mut layout) = layout {
        layout.inspector_visible = true;
    }
}

pub(super) fn actions(
    operations: Res<RiggingOperations>,
    mut contexts: ParamSet<(Context, (
        ResMut<SceneEditorSidebarState>,
        Option<ResMut<SceneEditorLayoutState>>,
        ResMut<SceneEditorState>,
        ResMut<ProjectBrowserState>,
        ResMut<EditorProjectFileCatalog>,
        ResMut<EditorAssetCatalog>,
    ))>,
    mut state: ResMut<RiggingState>,
    mut tree: ResMut<super::super::gltf_tree::TreeState>,
    buttons: Query<(&RigAction, &EditorButtonClick), Changed<EditorButtonClick>>,
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
    let has_motion = has_project_motion(source, session);
    let motion_page = central && has_motion;
    let motion_shown = motion_page && context.motion_properties_shown();
    if action == RigAction::ChooseProjectMotion || (action == RigAction::Launch && has_motion) {
        let from_properties = action == RigAction::ChooseProjectMotion;
        if from_properties != motion_shown {
            return; // 属性与侧栏备用入口只接受来自实际可见位置的点击。
        }
        let path = (!from_properties).then(|| source.path.to_owned());
        let small_preview_root = (!central).then(|| source.root.to_path_buf());
        let (mut sidebar, Some(mut layout), mut editor, mut browser, mut files, mut catalog) = contexts.p1()
        else { return; };
        if let Some(path) = path {
            if let Some(root) = small_preview_root {
                tree.keep_animation_browser_on_open(root, path.clone());
                open.write(OpenAssetPreview { path: path.clone() });
            }
            editor.selected_asset = Some(path);
            sidebar.reveal_asset_properties();
            layout.inspector_visible = true;
        }
        choose_animation(&mut browser, &mut files, &mut catalog, &mut editor, &mut sidebar, &mut layout);
        return;
    }
    if action == RigAction::Launch && state.work.is_none() {
        if state.session.as_ref().is_some_and(|session| session.matches(source)) {
            state.panel_open = true;
        } else {
            inspect(&mut state, source, *operations);
        }
        if !central {
            // 原侧栏可以启动工作流；同一模型移入中央标签后有更大的关节编辑画布。
            open.write(OpenAssetPreview {
                path: source.path.to_owned(),
            });
        }
        let picking_reference = state.session.as_ref().is_some_and(Session::reference_mode);
        let target = picking_reference.then(|| source.path.to_owned());
        let (mut sidebar, layout, mut editor, ..) = contexts.p1();
        if let Some(target) = target {
            // 固定当前模型的属性身份。左栏引用高亮读取独立的参考选择。
            editor.selected_asset = Some(target);
            if picking_reference {
                sidebar.navigator = EditorNavigatorTab::Assets;
            }
            sidebar.reveal_asset_properties();
        }
        if central {
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
    if motion_page || !context.rigging_sidebar_shown() {
        return;
    }
    if action == RigAction::Close && session.is_some() {
        state.cancel_work();
        state.panel_open = false;
        let (mut sidebar, ..) = contexts.p1();
        sidebar.reveal_properties();
        return;
    }
    if session.is_none() || state.work.is_some() {
        return;
    }
    let session = state.session.as_mut().expect("已核对当前绑骨会话");
    match action {
        RigAction::Retry => inspect(&mut state, source, *operations),
        RigAction::SwitchBackend => {
            state.backend = match state.backend {
                RigBackend::Native => RigBackend::Blender,
                RigBackend::Blender => RigBackend::Native,
            };
            inspect(&mut state, source, *operations);
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
            state.work = Some(Work::start_async(Some(source.key()), move |cancel| async move {
                Completion::Binding(input.bind(reference, landmarks, cancel).await)
            }));
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
            // 生成成功后将文件回执和动作入口移入当前模型的属性页。
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

fn inspect(state: &mut RiggingState, source: Candidate<'_>, operations: RiggingOperations) {
    let key = source.key();
    let mut next = Session::new(key.clone());
    if let Some(previous) = state.session.as_ref()
        && previous.reference_mode() && previous.matches(source)
    {
        // 更换后端或重查时重新验证同一显式选择，切模型时从空选择开始。
        next.reference_path.clone_from(&previous.reference_path);
    }
    let previous = state.session.replace(next);
    let backend = state.backend;
    state.panel_open = true;
    state.work = Some(Work::start_async(Some(key.clone()), move |cancel| async move {
        // 旧目录可能很大，移动所有权后在原单飞槽释放。
        drop(previous);
        Completion::Inspection(
            (operations.inspect)(key.root, key.path, backend, crate::authoring::find_blender_executable, cancel)
                .await
                .map(|inspection| {
                    let has_reference = inspection.animations.iter().any(|asset| asset.can_reference);
                    (inspection, has_reference)
                }),
        )
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
        Completion::Binding(Ok(prepared)) => {
            let (joints, vertices) = (prepared.joints, prepared.vertices);
            session.notice = Notice::Publishing;
            // 只有仍与当前模型代次一致的完整阶段结果才可请求原子发布。
            state.work = Some(Work::start(work.source, move |cancel| {
                Completion::Publish {
                    result: prepared.publish(cancel),
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
        }
        Completion::Inspection(Err(error))
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
            Has<RigMarkerTools>,
            Has<RigMotionHelp>,
        ),
        (Or<(With<RigAction>, With<RigPanel>, With<RigMarkerTools>, With<RigMotionHelp>)>, Without<RigText>),
    >,
    mut labels: Query<(&RigText, &mut Text, &mut Node)>,
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    let candidate = context.candidate();
    let session = candidate.and_then(|source| current_session(&state, source));
    let motion_page = context.central() && candidate.is_some_and(|source| has_project_motion(source, session));
    let has_motion = candidate.is_some_and(|source| has_project_motion(source, session));
    let busy = state.work.is_some();
    for (mut node, action, panel, marker_tools, motion_help) in &mut nodes {
        let show = if panel {
            motion_page || session.is_some()
        } else if marker_tools {
            session.is_some_and(|session| session.draft.is_some())
        } else if action == Some(&RigAction::Launch) {
            candidate.is_some() && (has_motion || !busy) && !(motion_page && context.motion_properties_shown())
        } else if motion_help || action == Some(&RigAction::ChooseProjectMotion) {
            // 共用面板在绑骨页时只显示绑骨控制，问号随工程动作入口出现。
            motion_page
        } else if action == Some(&RigAction::Close) {
            session.is_some() && !motion_page
        } else {
            !motion_page && session.is_some_and(|session| match action {
                Some(RigAction::SwitchBackend) => !busy,
                Some(RigAction::Retry) => {
                    !busy
                        && matches!(
                            session.notice,
                            Notice::Failed
                                | Notice::NoReference
                                | Notice::Cancelled
                                | Notice::Saved { .. }
                        )
                }
                Some(RigAction::ResetMarks) => !busy && session.draft.is_some(),
                Some(RigAction::ToggleMirror) => !busy && session.draft.is_some(),
                Some(RigAction::Bind) => {
                    !busy && session.draft.is_some() && session.file().is_some()
                }
                _ => false,
            })
        };
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (kind, mut text, mut node) in &mut labels {
        let wanted = match (kind, session) {
            (RigText::Launch, _) => locale
                .text(
                    if has_motion {
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
            _ => String::new(),
        };
        if text.as_str() != wanted {
            **text = wanted;
        }
        if matches!(kind, RigText::Notice | RigText::Warning) {
            // 工程动作只为真实的生成回执和警告留出空间，普通模型的按钮紧跟标题。
            let display = if motion_page && text.is_empty() { Display::None } else { Display::Flex };
            if node.display != display {
                node.display = display;
            }
        }
    }
}

fn notice(locale: EditorLocale, session: &Session) -> String {
    let key = match session.notice {
        Notice::Inspecting => "rig.status.inspecting",
        Notice::Review => "rig.status.review",
        Notice::Binding => "rig.status.binding",
        Notice::Publishing => "rig.status.publishing",
        Notice::NoReference => "rig.status.no_reference",
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
