//! 在后台读取 Blender 的 GLB，按原对象身份提交已加载的模型。

use super::files::{BlenderProcess, Export, MainSceneProcess};
use crate::{
    authoring::{
        EditorAuthoringSession, EditorSceneDraft, asset_target_with_mesh_collision, mesh_collision_enabled,
        mesh_edit::{EditWorld, MeshEditState},
    },
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::gltf_browser::EditorGltfBrowserState,
    ui::scene_objects::SceneObjectState,
    viewport::placement_move::PlacementMoveState,
    workbench::WorkbenchModel,
};
use bevy::{
    asset::{LoadState, RecursiveDependencyLoadState}, ecs::system::SystemParam,
    gltf::GltfAssetLabel, prelude::*, tasks::{IoTaskPool, Task, futures::check_ready},
    world_serialization::WorldAsset,
};
use feathrise_editor_core::EditorSettings;
use feathrise_scene_content::{SceneId, ScenePlacementAssetTarget, ScenePlacementRecord};
use std::{path::PathBuf, time::{Duration, Instant}};

#[derive(Clone, Message)]
pub(crate) struct BlenderMeshAction {
    pub scene_id: SceneId,
    pub placement_id: String,
}

#[derive(Message)]
pub(crate) struct BlenderMainSceneAction {
    pub scene_id: SceneId,
}

pub(crate) fn supports(record: &ScenePlacementRecord) -> bool {
    record.terrain_patch.is_none() && record.building_preset_id.is_none()
        && record.water_surfaces.is_empty()
        && (record.asset_target.is_builtin() || (record.asset_target.is_scene0()
            && std::path::Path::new(&record.asset_path).extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))))
}

#[derive(Default, Resource)]
pub(crate) struct BlenderMeshSession {
    active: Option<Session>,
    main_scene: Option<MainSceneProcess>,
    task: Option<Task<Finished>>,
    next_check: u64,
    close: CloseCheck,
}

impl BlenderMeshSession {
    pub(crate) fn prepare_close(&mut self) {
        self.next_check = self.next_check.wrapping_add(1);
        self.close = if self.active.is_some() || self.task.is_some() {
            CloseCheck::Waiting(self.next_check)
        } else {
            CloseCheck::Ready
        };
        if let Some(live) = &mut self.active {
            live.next_poll = Instant::now();
        }
    }

    pub(crate) fn cancel_close(&mut self) {
        self.close = CloseCheck::Idle;
    }

    pub(crate) fn close_pending(&self) -> bool {
        self.close.ticket().is_some()
    }

    pub(crate) fn close_failure(&self) -> Option<&str> {
        match &self.close {
            CloseCheck::Failed(error) => Some(error),
            _ => None,
        }
    }

    fn finish_check(&mut self, ticket: Option<u64>) {
        // 取消或再次确认后，需要等新发起的回执探测。旧的空闲快照不得授权关闭。
        if ticket.is_some() && self.close.ticket() == ticket {
            self.close = CloseCheck::Ready;
        }
    }

    fn report_failure(&mut self, author: &mut Author, error: String) {
        if self.close_pending() {
            self.close = CloseCheck::Failed(error.clone());
        }
        author.editor.status = error;
    }

    fn blender_open(&self) -> bool {
        self.active.is_some() || self.task.is_some() || self.main_scene.is_some()
    }

    #[cfg(any(test, feature = "validation"))]
    pub(crate) fn has_open_session(&self) -> bool {
        self.blender_open()
    }
}

#[derive(Default)]
enum CloseCheck {
    #[default]
    Idle,
    Waiting(u64),
    Ready,
    Failed(String),
}

impl CloseCheck {
    fn ticket(&self) -> Option<u64> {
        match self {
            Self::Waiting(ticket) => Some(*ticket),
            _ => None,
        }
    }
}

struct Session {
    target: BlenderMeshAction,
    project: Option<u64>,
    asset_root: PathBuf,
    folder: String,
    basis: ScenePlacementRecord,
    process: BlenderProcess,
    next_poll: Instant,
    pending: Option<(String, Handle<WorldAsset>)>,
    ended: bool,
    preserve_files: bool,
}

enum Finished {
    Opened(Result<Session, String>),
    MainSceneOpened(Result<MainSceneProcess, String>),
    Polled(Session, Result<Export, String>, Option<u64>),
    Cleaned,
}

#[derive(SystemParam)]
pub(super) struct Author<'w, 's> {
    session: ResMut<'w, EditorAuthoringSession>,
    editor: ResMut<'w, SceneEditorState>,
    browser: Res<'w, EditorGltfBrowserState>,
    mesh_edit: Res<'w, MeshEditState>,
    batch: Res<'w, crate::authoring::placement_batch::PlacementBatchState>,
    objects: Option<Res<'w, SceneObjectState>>,
    moving: Res<'w, PlacementMoveState>,
    numeric_mouse: Res<'w, SceneEditorNumericScrubMouseGuard>,
    numeric_input: Option<Res<'w, feathrise_editor_ui::ActiveNumericInput>>,
    project: Option<Res<'w, WorkbenchModel>>,
    assets: Res<'w, AssetServer>,
    world: EditWorld<'w, 's>,
}

impl Author<'_, '_> {
    fn draft(&self, target: &BlenderMeshAction) -> Option<&EditorSceneDraft> {
        self.session.draft.as_ref().filter(|draft| self.editor.open
            && draft.scene_id == target.scene_id && self.editor.selected_scene_id.as_ref() == Some(&target.scene_id))
    }

    fn current(&self, target: &BlenderMeshAction) -> Option<(usize, &ScenePlacementRecord)> {
        let draft = self.draft(target)?;
        let index = self.browser.placement_record_index(&target.placement_id, self.session.placement_structure_revision)?;
        let record = draft.placements.placements.get(index)?;
        (record.id == target.placement_id && supports(record)
            && !record.editor.locked && !record.editor.hidden).then_some((index, record))
    }

    fn asset_root(&self) -> Option<PathBuf> {
        match &self.project {
            Some(project) => project.asset_root.clone(),
            None => Some(feathrise_editor_core::workspace_asset_root()),
        }
    }

    fn busy(&self, closing: bool) -> bool {
        // 宿主关闭层会持续占用公共鼠标门。回填只等待真实手势和写事务，否则会与关闭互等。
        let input_busy = if closing {
            self.mesh_edit.moving() || self.numeric_input.as_ref().is_some_and(|input| input.entity.is_some())
        } else {
            self.mesh_edit.active() || self.numeric_mouse.blocked()
        };
        self.editor.close_requested || self.editor.scene_transition_in_progress
            || self.editor.pending_scene_transition.is_some() || self.editor.save_requested
            || self.session.saving || self.session.model_vegetation_busy || self.session.vegetation_cross_busy
            || self.objects.as_ref().is_some_and(|objects| objects.busy())
            || self.batch.busy() || self.moving.exit_pending() || input_busy
    }

    fn can_finish(&self, target: &BlenderMeshAction, closing: bool) -> bool {
        self.draft(target).is_none() || (!self.busy(closing)
            && self.browser.placement_index_ready(self.session.placement_structure_revision))
    }

    fn owns(&self, live: &Session) -> bool {
        self.project.as_ref().map(|project| project.context) == live.project
            && self.project.as_ref().is_none_or(|project| project.asset_root.as_ref() == Some(&live.asset_root))
    }

    fn adopt(&mut self, live: &mut Session, path: String) -> Result<(), String> {
        if self.draft(&live.target).is_none_or(|draft| draft.folder != live.folder) {
            return Err("原场景目录已经更换".into());
        }
        let (index, record) = self.current(&live.target).ok_or("找不到原场景对象")?;
        // 切换碰撞只改场景属性。Blender 还在编辑同一模型，回填采用属性面板里的最新值。
        let source = |record: &ScenePlacementRecord| asset_target_with_mesh_collision(&record.asset_target, false);
        if record.asset_path != live.basis.asset_path || source(record) != source(&live.basis) {
            return Err("原对象的模型已在 Feathrise 中更改".into());
        }
        if record.asset_path == path {
            return Ok(());
        }
        let before = record.clone();
        let collision = mesh_collision_enabled(&before.asset_target).unwrap_or_default();
        let after = ScenePlacementRecord {
            asset_path: path,
            asset_target: asset_target_with_mesh_collision(&ScenePlacementAssetTarget::Scene0, collision)
                .unwrap_or_default(),
            ..before.clone()
        };
        // GLB 保存自己的局部节点变换，场景根的位置、旋转和缩放沿用原摆放记录。
        self.world.commit_record(index, &before, &after, &mut self.session, &mut self.editor)?;
        live.basis = after;
        self.editor.status = "已更新 Blender 模型，请保存场景".into();
        Ok(())
    }
}

fn report_export_failure(state: &mut BlenderMeshSession, author: &mut Author, live: &mut Session, error: &str) {
    live.preserve_files = true;
    state.report_failure(author, format!("Blender 模型更新失败：{error}。工作文件位于 {}", live.process.directory().display()));
}

pub(super) fn update(
    mut state: ResMut<BlenderMeshSession>,
    mut messages: MessageReader<BlenderMeshAction>,
    mut main_scenes: MessageReader<BlenderMainSceneAction>,
    mut author: Author<'_, '_>,
    settings: Res<EditorSettings>,
) {
    if let Some(finished) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        match finished {
            Finished::Opened(Ok(live)) => {
                state.active = Some(live);
                author.editor.status = "Blender 启动后，保存即可更新模型".into();
            }
            Finished::Opened(Err(error)) => state.report_failure(&mut author, error),
            Finished::MainSceneOpened(Ok(live)) => {
                state.main_scene = Some(live);
                author.editor.status = "已启动 Blender。主场景导出后，关闭并重新打开场景标签读取改动".into();
            }
            Finished::MainSceneOpened(Err(error)) => state.report_failure(&mut author, error),
            Finished::Polled(mut live, _, _) if !author.owns(&live) => {
                report_export_failure(&mut state, &mut author, &mut live, "工程会话已更换");
            }
            Finished::Polled(mut live, result, ticket) => {
                match result {
                    Ok(Export::Ready(path)) => {
                        let asset = author.assets.load(GltfAssetLabel::Scene(0).from_asset(path.clone()));
                        live.pending = Some((path, asset));
                    }
                    Ok(Export::Idle) => state.finish_check(ticket),
                    Ok(Export::Writing) => {}
                    Err(error) => report_export_failure(&mut state, &mut author, &mut live, &error),
                }
                state.active = Some(live);
            }
            Finished::Cleaned => {}
        }
    }

    if let Some(exit) = state.main_scene.as_mut().and_then(MainSceneProcess::poll_exit) {
        let live = state.main_scene.take().unwrap();
        match exit {
            Ok(status) if status.success() => {
                state.task = Some(IoTaskPool::get().spawn(async move {
                    live.cleanup();
                    Finished::Cleaned
                }));
            }
            status => state.report_failure(&mut author, format!(
                "主场景 Blender 会话结束异常：{status:?}。日志位于 {}", live.directory().display()
            )),
        }
    }

    if let Some(target) = main_scenes.read().last()
        && matches!(state.close, CloseCheck::Idle)
    {
        if state.blender_open() {
            author.editor.status = "请先关闭当前 Blender 窗口".into();
        } else if !author.busy(false) && author.editor.open
            && author.editor.selected_scene_id.as_ref() == Some(&target.scene_id)
            && let Some(draft) = author.session.draft.as_ref().filter(|draft| draft.scene_id == target.scene_id)
            && let Some(expected) = draft.placement_glb_hash.clone()
            && let Some(root) = author.asset_root()
        {
            // 用当前场景的正式版本。只打开文件，不保存或接管尚未提交的作者草稿。
            let scene_id = draft.scene_id.clone();
            let path = draft.placements_path();
            let executable = settings.blender_executable.clone();
            state.task = Some(IoTaskPool::get().spawn(async move {
                Finished::MainSceneOpened(MainSceneProcess::start(
                    &root, scene_id.as_str(), &path, &expected, executable.as_deref(),
                ))
            }));
            author.editor.status = "正在 Blender 打开主场景".into();
        }
    }

    if let Some(target) = messages.read().last().cloned()
        && matches!(state.close, CloseCheck::Idle)
    {
        if state.blender_open() {
            author.editor.status = "请先关闭当前 Blender 窗口".into();
        } else if let Some((_, record)) = author.current(&target).filter(|_| !author.busy(false)) {
            let basis = record.clone();
            let folder = author.session.draft.as_ref().unwrap().folder.clone();
            let project = author.project.as_ref().map(|project| project.context);
            let Some(root) = author.asset_root() else { return; };
            let executable = settings.blender_executable.clone();
            state.task = Some(IoTaskPool::get().spawn(async move {
                let process = BlenderProcess::start(root.clone(), folder.clone(), &basis, executable.as_deref());
                Finished::Opened(process.map(|process| Session {
                    target, project, asset_root: root, folder, basis, process,
                    next_poll: Instant::now(), pending: None,
                    ended: false, preserve_files: false,
                }))
            }));
            author.editor.status = "正在打开 Blender".into();
        }
    }

    let Some(mut live) = state.active.take() else {
        if state.task.is_none() {
            let ticket = state.close.ticket();
            state.finish_check(ticket);
        }
        return;
    };
    if !author.owns(&live) {
        report_export_failure(&mut state, &mut author, &mut live, "工程会话已更换");
        return;
    }
    if let Some((_, handle)) = &live.pending
        && let Some((load, _, dependencies)) = author.assets.get_load_states(handle.id())
    {
        match (load, dependencies) {
            (LoadState::Failed(error), _) | (_, RecursiveDependencyLoadState::Failed(error)) => {
                report_export_failure(&mut state, &mut author, &mut live, &error.to_string());
                live.pending = None;
            }
            (LoadState::Loaded, RecursiveDependencyLoadState::Loaded)
                if author.can_finish(&live.target, state.close_pending()) =>
            {
                let (path, _handle) = live.pending.take().unwrap();
                match author.adopt(&mut live, path) {
                    Ok(()) => live.preserve_files = false,
                    Err(error) => report_export_failure(&mut state, &mut author, &mut live, &error),
                }
            }
            _ => {}
        }
    }

    // 每次只处理一个完整导出。连续保存期间，磁盘上的最新版本留给下一次轮询。
    if live.pending.is_none() && live.ended {
        if !live.preserve_files {
            state.task = Some(IoTaskPool::get().spawn(async move {
                live.process.cleanup();
                Finished::Cleaned
            }));
        }
    } else if live.pending.is_none() && Instant::now() >= live.next_poll
        && matches!(state.close, CloseCheck::Idle | CloseCheck::Waiting(_))
    {
        live.next_poll = Instant::now() + Duration::from_millis(180);
        let ticket = state.close.ticket();
        state.task = Some(IoTaskPool::get().spawn(async move {
            let (ended, result) = live.process.poll();
            live.ended = ended;
            Finished::Polled(live, result, ticket)
        }));
    } else {
        state.active = Some(live);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standalone::{cancel_project_close_exports, prepare_project_close_exports, project_close_snapshot};

    #[test]
    fn blender_close_probe_requires_the_current_receipt_after_reconfirmation() {
        let mut world = World::new();
        let pool = IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        world.insert_resource(BlenderMeshSession {
            task: Some(pool.spawn(async { Finished::Cleaned })),
            ..default()
        });
        prepare_project_close_exports(&mut world);
        let first = world.resource::<BlenderMeshSession>().close.ticket();
        assert!(first.is_some());
        assert!(project_close_snapshot(&world).pending.is_some());

        cancel_project_close_exports(&mut world);
        assert!(project_close_snapshot(&world).pending.is_none());
        prepare_project_close_exports(&mut world);
        let mut state = world.resource_mut::<BlenderMeshSession>();
        let current = state.close.ticket();
        assert_ne!(current, first);
        state.finish_check(None);
        state.finish_check(first);
        assert!(project_close_snapshot(&world).pending.is_some());

        world.resource_mut::<BlenderMeshSession>().finish_check(current);
        assert!(project_close_snapshot(&world).pending.is_none());
    }
}
