//! 调权入口、目标身份、单槽任务、历史及保存回执。

use super::{
    super::{
        AssetPreviewNavigation, AssetPreviewState, AssetPreviewWorkspace, DocumentState,
        OpenAssetPreview,
        animation::{AnimationEditorState, AnimationPreviewRuntime},
        gltf_tree,
        rigging::RiggingState,
        types::{AssetPreviewContent, AssetPreviewModelTarget, AssetPreviewPhase},
    },
    edit::{self, Operation, Tool},
    file,
    panel::Action,
    resources::*,
};
use crate::{
    asset_catalog::EditorAssetCatalog,
    resources::{SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        sidebars::{EditorNavigatorTab, EditorRightSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{
    ecs::system::SystemParam,
    gltf::Gltf,
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use feathrise_editor_ui::EditorButtonClick;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(SystemParam)]
pub(super) struct Target<'w> {
    pub preview: Res<'w, AssetPreviewState>,
    media: Res<'w, AssetPreviewWorkspace>,
    workspace: Res<'w, VegetationPresetEditorState>,
    documents: Res<'w, DocumentState>,
}

impl Target<'_> {
    pub fn path(&self) -> Option<&str> {
        gltf_tree::central_glb(&self.media, self.workspace.active_tab)
    }

    pub fn root(&self) -> &Path {
        self.documents.asset_root()
    }

    pub fn matches(&self, key: &Key) -> bool {
        matches_key(key, &self.media, &self.workspace, &self.documents)
    }

    pub fn key(&self) -> Option<Key> {
        let path = self.path()?;
        if self.preview.selected_path.as_deref() != Some(path)
            || self.preview.external_preview.is_some()
            || !matches!(
                self.preview.content,
                AssetPreviewContent::Model {
                    phase: AssetPreviewPhase::Ready,
                    ..
                }
            )
            || !self
                .preview
                .model
                .as_ref()
                .is_some_and(|model| model.target == AssetPreviewModelTarget::Scene0)
        {
            return None;
        }
        Some(Key {
            root: self.root().to_owned(),
            path: path.to_owned(),
            tab: self.media.expanded.as_ref()?.id,
        })
    }

    pub fn entry(&self, rig: Option<&RiggingState>, gltfs: Option<&Assets<Gltf>>) -> Option<&str> {
        let path = self.path()?;
        if self.preview.selected_path.as_deref() != Some(path) {
            return None;
        }
        if let Some(output) =
            rig.and_then(|rig| rig.generated_weights_target(self.root(), &self.preview))
        {
            // 由调用者读取生成结果路径，避免把暂存输入当作蒙皮模型。
            if !output.is_empty() {
                return Some(path);
            }
        }
        // 根 GLTF 可先于完整场景就绪。可点击入口必须和实际加载的目标使用同一个就绪条件。
        self.key()?;
        let model = self.preview.model.as_ref()?;
        let gltf = gltfs?.get(&model.catalog.gltf)?;
        (!gltf.skins.is_empty() && !gltf.meshes.is_empty()).then_some(path)
    }
}

pub(super) fn matches_key(
    key: &Key,
    media: &AssetPreviewWorkspace,
    workspace: &VegetationPresetEditorState,
    documents: &DocumentState,
) -> bool {
    documents.asset_root() == key.root
        && gltf_tree::central_glb(media, workspace.active_tab) == Some(key.path.as_str())
        && media.expanded.as_ref().is_some_and(|tab| tab.id == key.tab)
}

#[derive(SystemParam)]
pub(super) struct Gate<'w> {
    layout: Option<Res<'w, SceneEditorLayoutState>>,
    guard: Option<Res<'w, SceneEditorNumericScrubMouseGuard>>,
    host: Option<Res<'w, crate::workbench::WorkbenchModel>>,
    settings: Option<Res<'w, crate::resources::SceneEditorSettingsState>>,
}

impl Gate<'_> {
    pub fn open(&self, editor: &SceneEditorState) -> bool {
        editor.open
            && !editor.close_requested
            && !editor.new_scene_dialog_open
            && editor.scene_switch_confirmation.is_none()
            && editor.pending_scene_transition.is_none()
            && self
                .layout
                .as_ref()
                .is_none_or(|layout| layout.inspector_shown())
            && self
                .host
                .as_ref()
                .is_none_or(|host| host.visible && !host.modal && !host.busy)
            && self.settings.as_ref().is_none_or(|settings| !settings.open)
    }

    pub fn can_press(&self, editor: &SceneEditorState) -> bool {
        self.open(editor) && self.guard.as_ref().is_none_or(|guard| !guard.blocked())
    }
}

fn open(
    state: &mut WeightEditor,
    key: Key,
    assets: &AssetServer,
    sidebar: &mut SceneEditorSidebarState,
) {
    if let Some(session) = state.session.as_mut() {
        if session.key.root == key.root && session.key.path == key.path && !session.stale {
            session.key = key;
            sidebar.weight_available = true;
            sidebar.right = EditorRightSidebarTab::Weights;
            sidebar.navigator = EditorNavigatorTab::SceneTree;
            state.opening = None;
            state.bump();
            return;
        }
        if session.dirty() {
            state.notice = format!("请先保存或丢弃 {} 的调权草稿", session.key.path);
            state.opening = None;
            state.bump();
            return;
        }
    }
    let retired = state.session.take();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let load = key.clone();
    let gltf = assets.load::<Gltf>(key.path.clone());
    let task = AsyncComputeTaskPool::get().spawn(async move {
        drop(retired);
        let doc = file::load(load.root.clone(), load.path.clone(), token).await?;
        Ok(Outcome::Open {
            key: load,
            gltf,
            doc,
        })
    });
    state.work = Some(Work {
        key,
        saving: false,
        cancel,
        task,
    });
    state.opening = None;
    state.notice = "正在读取蒙皮并建立选点索引…".into();
    sidebar.weight_available = true;
    sidebar.right = EditorRightSidebarTab::Weights;
    sidebar.navigator = EditorNavigatorTab::SceneTree;
    state.bump();
}

pub(super) fn queue(state: &mut WeightEditor, operation: Operation, reason: Reason) {
    let Some(session) = state.session.as_ref() else {
        return;
    };
    if state.work.is_some() || session.stale {
        return;
    }
    let doc = session.doc.clone();
    let surface = session.surface;
    let joint = session.joint;
    let draft = &session.drafts[surface];
    let weights = draft.weights.clone();
    let selection = draft.selection.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let result = edit::prepare(&doc, surface, weights, selection, joint, operation, &token)?;
        Ok(Outcome::Apply { result, reason })
    });
    state.work = Some(Work {
        key: session.key.clone(),
        saving: false,
        cancel,
        task,
    });
    state.bump();
}

pub(super) fn begin_save(state: &mut WeightEditor) -> Result<(), String> {
    let session = state.session.as_ref().ok_or("没有当前蒙皮草稿")?;
    if state.busy() {
        return Err("请先结束当前笔画或权重任务".into());
    }
    if !session.dirty() || session.stale {
        return Err("蒙皮草稿已保存或源模型已变化".into());
    }
    let key = session.key.clone();
    let root = key.root.clone();
    let doc = session.doc.clone();
    let drafts = session
        .drafts
        .iter()
        .map(|draft| draft.weights.clone())
        .collect();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        file::save(root, doc, drafts, token)
            .await
            .map(Outcome::Save)
    });
    state.work = Some(Work {
        key,
        saving: true,
        cancel,
        task,
    });
    state.save_receipt = None;
    state.notice = "正在另存并校验调权 GLB…".into();
    state.bump();
    Ok(())
}

pub(super) fn actions(
    mut state: ResMut<WeightEditor>,
    target: Target,
    gate: Gate,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    mut editor: ResMut<SceneEditorState>,
    mut animation: Option<ResMut<AnimationEditorState>>,
    motion: Option<Res<AnimationPreviewRuntime>>,
    mut navigation: ResMut<AssetPreviewNavigation>,
    assets: Option<Res<AssetServer>>,
    gltfs: Option<Res<Assets<Gltf>>>,
    rigging: Option<Res<RiggingState>>,
    buttons: Query<
        (&Action, &EditorButtonClick),
        (
            Changed<EditorButtonClick>,
            Without<bevy::ui::InteractionDisabled>,
        ),
    >,
    mut tree: MessageReader<TreePick>,
    mut open_asset: MessageWriter<OpenAssetPreview>,
) {
    let pick = tree.read().last().copied();
    if !gate.can_press(&editor) || state.busy() {
        return;
    }
    let Some(assets) = assets else { return };
    if let Some((root, path)) = &state.opening
        && let Some(key) = target
            .key()
            .filter(|key| &key.root == root && &key.path == path)
    {
        open(&mut state, key, &assets, &mut sidebar);
        return;
    }
    let action = buttons
        .iter()
        .find_map(|(action, click)| click.is_clicked().then_some(*action));
    if action == Some(Action::Open) {
        let Some(path) = target.entry(rigging.as_deref(), gltfs.as_deref()) else {
            return;
        };
        if sidebar.right != EditorRightSidebarTab::Assets
            || editor.selected_asset.as_deref() != Some(path)
        {
            return;
        }
        if let Some(path) = rigging
            .as_deref()
            .and_then(|rig| rig.generated_weights_target(target.root(), &target.preview))
        {
            if state
                .session
                .as_ref()
                .is_some_and(|session| session.dirty() && session.key.path != path)
            {
                state.notice = "请先保存或丢弃当前调权草稿".into();
                state.bump();
                return;
            }
            let path = path.to_owned();
            state.opening = Some((target.root().to_owned(), path.clone()));
            editor.selected_asset = Some(path.clone());
            sidebar.reveal_asset_properties();
            open_asset.write(OpenAssetPreview { path });
            return;
        }
        if let Some(key) = target.key() {
            open(&mut state, key, &assets, &mut sidebar);
        }
        return;
    }
    if sidebar.right != EditorRightSidebarTab::Weights {
        return;
    }
    let problem = state.problem;
    let Some(session) = state
        .session
        .as_mut()
        .filter(|session| target.matches(&session.key))
    else {
        return;
    };
    if let Some(pick) = pick {
        let found = match pick {
            TreePick::Joint(node) => {
                let locate = |surface: usize| {
                    session.doc.surfaces[surface]
                        .skin
                        .joints
                        .iter()
                        .position(|joint| joint.node == node)
                        .map(|joint| (surface, joint as u16))
                };
                locate(session.surface).or_else(|| (0..session.doc.surfaces.len()).find_map(locate))
            }
            TreePick::Mesh(mesh) => session
                .doc
                .surfaces
                .iter()
                .position(|surface| surface.mesh == mesh)
                .map(|index| (index, 0)),
            TreePick::Skin(skin) => session
                .doc
                .surfaces
                .iter()
                .position(|surface| surface.skin.index == skin)
                .map(|index| (index, 0)),
        };
        if let Some((surface, joint)) = found {
            state.select_joint(surface, joint);
        }
        return;
    }
    let Some(action) = action else { return };
    match action {
        Action::Open => {}
        Action::Discard => {
            let retired = state.session.take();
            if let Some(problem) = state.problem.take()
                && let Some(animation) = animation.as_deref_mut()
            {
                animation.resume_weight_frame(problem.frame);
            }
            AsyncComputeTaskPool::get()
                .spawn(async move {
                    drop(retired);
                })
                .detach();
            sidebar.reveal_asset_properties();
            state.notice.clear();
        }
        Action::OpenSaved => {
            if let Some(path) = session.saved_path.clone() {
                editor.selected_asset = Some(path.clone());
                sidebar.reveal_asset_properties();
                open_asset.write(OpenAssetPreview { path });
            }
        }
        Action::Rest => {
            if let Some(animation) = animation.as_deref_mut() {
                if let Some(problem) =
                    problem.filter(|problem| animation.matches_weight_frame(problem.frame))
                {
                    if animation.weight_rest_active() {
                        if animation.resume_weight_frame(problem.frame) {
                            navigation.view = problem.view;
                            navigation.drag = None;
                            session.original = false;
                            state.notice = format!(
                                "已回到问题帧 {:.3} 秒，可对照原始与草稿",
                                problem.frame.time
                            );
                        }
                    } else if animation.show_weight_rest(problem.frame) {
                        session.original = false;
                        state.notice = "已返回静置姿势，保留所选顶点和骨骼".into();
                    }
                } else {
                    animation.weight_rest_pose();
                }
            }
        }
        Action::Compare => {
            if let Some(animation) = animation.as_deref_mut() {
                let published = motion
                    .as_ref()
                    .and_then(|motion| motion.published_weight_time(animation));
                animation.pause_weight_comparison(published);
            }
            session.original = !session.original;
        }
        Action::Heat => session.heat = !session.heat,
        Action::Skeleton => session.show_skeleton = !session.show_skeleton,
        Action::Visible => session.visible_only = !session.visible_only,
        Action::Mirror => session.mirror = !session.mirror,
        Action::Mask => session.mask = !session.mask,
        Action::Edit => session.set_tool(session.edit_tool),
        Action::Mode(mode) => {
            session.set_tool(mode);
            if mode == Tool::Bone {
                // 选骨工具始终从可见的骨段和起点接管画布左键。
                session.show_skeleton = true;
            }
        }
        Action::Mesh(delta) => {
            let count = session.doc.surfaces.len() as isize;
            let old_skin = session.doc.surfaces[session.surface].skin.index;
            session.surface =
                (session.surface as isize + delta as isize).rem_euclid(count) as usize;
            if session.doc.surfaces[session.surface].skin.index != old_skin {
                session.joint = 0;
            }
            state.refresh = true;
        }
        Action::Lock => {
            let skin = session.doc.surfaces[session.surface].skin.index;
            let locks = Arc::make_mut(session.locks.get_mut(&skin).expect("蒙皮已建立锁表"));
            locks[session.joint as usize] = !locks[session.joint as usize];
        }
        Action::Clear => {
            session.drafts[session.surface].selection = Arc::from([]);
            state.refresh = true;
        }
        Action::Apply | Action::Assign | Action::Remove => {
            if session.original
                || session.tool == Tool::Bone
                || session.drafts[session.surface].selection.is_empty()
                || session.current_locks()[session.joint as usize]
                || animation
                    .as_ref()
                    .is_some_and(|animation| animation.weight_motion_active())
            {
                return;
            }
            if action == Action::Apply && session.tool != Tool::Set {
                // 先显露设定值，由下一次明确点击应用修改已选顶点。
                session.set_tool(Tool::Set);
                state.bump();
                return;
            }
            let amount = match action {
                Action::Assign => 1.0,
                Action::Remove => 0.0,
                _ => session.amount,
            };
            let operation = Operation::Set {
                joint: session.joint,
                amount,
                locks: session.current_locks().clone(),
            };
            queue(&mut state, operation, Reason::Edit);
        }
    }
    state.bump();
}

pub(super) fn poll(
    mut state: ResMut<WeightEditor>,
    target: Target,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut gltf_events: MessageReader<AssetEvent<Gltf>>,
    mut catalog: Option<ResMut<EditorAssetCatalog>>,
    sidebar: Res<SceneEditorSidebarState>,
    mut animation: Option<ResMut<AnimationEditorState>>,
) {
    let mut stale = false;
    for event in gltf_events.read() {
        if let AssetEvent::Modified { id } | AssetEvent::Removed { id } = event
            && let Some(session) = state
                .session
                .as_mut()
                .filter(|session| session.gltf.id() == *id)
        {
            session.stale = true;
            stale = true;
        }
    }
    if stale {
        if let Some(work) = &state.work {
            work.cancel.store(true, Ordering::Release);
        }
        if let Some(stroke) = &mut state.stroke {
            stroke.cancelled = true;
        }
        state.notice = "源 GLB 已热更新。保留现有草稿，请丢弃后重新打开。".into();
        state.bump();
    }
    if let Some(problem) = state.problem
        && (state
            .session
            .as_ref()
            .is_none_or(|session| session.stale || !target.matches(&session.key))
            || animation
                .as_ref()
                .is_none_or(|animation| !animation.matches_weight_frame(problem.frame)))
    {
        if let Some(animation) = animation.as_deref_mut() {
            animation.resume_weight_frame(problem.frame);
        }
        state.problem = None;
    }
    let ready = state
        .work
        .as_mut()
        .and_then(|work| check_ready(&mut work.task));
    if let Some(result) = ready {
        let work = state.work.take().expect("完成的是当前单槽任务");
        match result {
            Ok(Outcome::Open { key, gltf, doc }) if target.root() == key.root => {
                state.session = Some(Session::new(key, gltf, doc));
                state.refresh = true;
                state.notice.clear();
            }
            Ok(Outcome::Apply { result, reason }) => {
                if let Reason::Problem(problem) = &reason {
                    // 迟到选区不能替换其他模型、动作或静置草稿。空框保留先前的选区。
                    let current = state.session.as_ref().is_some_and(|session| {
                        !session.stale
                            && target.matches(&session.key)
                            && session.key == work.key
                            && session.surface == result.surface
                    });
                    if result.selection.is_empty()
                        || !current
                        || sidebar.right != EditorRightSidebarTab::Weights
                        || meshes.is_none()
                        || animation
                            .as_deref_mut()
                            .is_none_or(|animation| !animation.show_weight_rest(problem.frame))
                    {
                        state.notice = if result.selection.is_empty() {
                            "问题帧框内没有命中顶点".into()
                        } else {
                            "问题帧已切换，请重新框选".into()
                        };
                        state.bump();
                        return;
                    }
                    state.problem = Some(*problem);
                }
                if let (Some(meshes), Some(session)) =
                    (meshes.as_deref_mut(), state.session.as_mut())
                    && session.key == work.key
                    && session.surface == result.surface
                    && !session.stale
                {
                    let draft = &mut session.drafts[result.surface];
                    let before = std::mem::replace(&mut draft.weights, result.weights);
                    draft.selection = result.selection;
                    if Arc::ptr_eq(
                        &draft.weights,
                        &session.doc.surfaces[result.surface].original,
                    ) {
                        draft.shaded = None;
                    } else if let Some(mesh) = result.shaded {
                        draft.shaded = Some(meshes.add(mesh));
                    }
                    session.heat_mesh = Some((result.surface, meshes.add(result.heat)));
                    let after = draft.weights.clone();
                    match reason {
                        Reason::Problem(problem) => {
                            session.original = false;
                            state.notice = format!(
                                "已在静置姿势选中 {} 个顶点 · 问题帧 {:.3} 秒",
                                draft.selection.len(),
                                problem.frame.time
                            );
                        }
                        Reason::Edit => {
                            session.record(History {
                                surface: result.surface,
                                before,
                                after,
                            });
                            state.notice = if result.changed > 0 {
                                format!("已修正 {} 个顶点", result.changed)
                            } else {
                                "选区权重已符合设定或受到锁定与唯一影响保护".into()
                            };
                        }
                        Reason::Undo(entry) => {
                            session.undo.pop_back();
                            session.redo.push(entry);
                        }
                        Reason::Redo(entry) => {
                            session.redo.pop();
                            session.undo.push_back(entry);
                        }
                        _ => {}
                    }
                }
            }
            Ok(Outcome::Save(path)) => {
                if let Some(session) = state
                    .session
                    .as_mut()
                    .filter(|session| session.key == work.key)
                {
                    for draft in &mut session.drafts {
                        draft.saved = draft.weights.clone();
                    }
                    session.saved_path = Some(path.clone());
                    state.notice = format!("已另存 {path}");
                    let folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
                    if let Some(catalog) = catalog.as_deref_mut()
                        && catalog.current_folder == folder
                        && catalog.pending_reveal_revision().is_none()
                    {
                        catalog.requested_revision = catalog.requested_revision.wrapping_add(1);
                    }
                    state.save_receipt = Some((work.key.root.join(&work.key.path), Ok(path)));
                }
            }
            Ok(_) => {}
            Err(error) => {
                if work.saving {
                    state.save_receipt =
                        Some((work.key.root.join(&work.key.path), Err(error.clone())));
                }
                state.notice = error;
            }
        }
        state.bump();
    }
    if !state.busy() && state.refresh {
        state.refresh = false;
        queue(&mut state, Operation::Refresh, Reason::Display);
    }
    if state.work.is_none()
        && let Some(stroke) = state.stroke.as_mut()
    {
        let mut finish = stroke.cancelled;
        if !finish && stroke.brush.tool.paint() {
            if stroke.released {
                stroke.advance(stroke.latest, true);
            }
            if !stroke.dabs.is_empty() {
                let dabs = stroke
                    .dabs
                    .drain(..stroke.dabs.len().min(BATCH_DABS))
                    .collect();
                let operation = Operation::Paint {
                    view: stroke.view,
                    dabs,
                    brush: stroke.brush.clone(),
                };
                queue(&mut state, operation, Reason::Stroke);
                return;
            }
            finish = stroke.released;
        }
        if finish {
            let stroke = state.stroke.take().expect("有一笔结束的手势");
            if let Some(session) = state
                .session
                .as_mut()
                .filter(|session| !session.stale && session.surface == stroke.surface)
            {
                let after = session.drafts[stroke.surface].weights.clone();
                if stroke.cancelled {
                    if !Arc::ptr_eq(&stroke.before, &after) {
                        queue(
                            &mut state,
                            Operation::History(stroke.before),
                            Reason::Rollback,
                        );
                    }
                } else {
                    session.record(History {
                        surface: stroke.surface,
                        before: stroke.before,
                        after,
                    });
                }
            }
            state.bump();
        }
    }
}

pub(super) fn sync_sidebar(
    state: Res<WeightEditor>,
    target: Target,
    mut sidebar: ResMut<SceneEditorSidebarState>,
) {
    let available = state
        .session
        .as_ref()
        .is_some_and(|session| target.matches(&session.key))
        || state
            .work
            .as_ref()
            .is_some_and(|work| target.matches(&work.key));
    if sidebar.weight_available != available {
        sidebar.weight_available = available;
        if !available && sidebar.right == EditorRightSidebarTab::Weights {
            sidebar.reveal_asset_properties();
        }
    }
}
