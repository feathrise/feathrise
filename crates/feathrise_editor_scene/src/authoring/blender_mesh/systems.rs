//! 在后台读取 Blender 的 GLB，按原对象身份提交已加载的模型。

use super::files::BlenderProcess;
use crate::{
    authoring::{
        EditorAuthoringSession, EditorSceneDraft, asset_target_with_mesh_collision, mesh_collision_enabled,
        mesh_edit::{EditWorld, MeshEditState},
    },
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::gltf_browser::EditorGltfBrowserState,
    viewport::placement_move::PlacementMoveState,
};
use bevy::{
    asset::{LoadState, RecursiveDependencyLoadState}, ecs::system::SystemParam,
    gltf::GltfAssetLabel, prelude::*, tasks::{IoTaskPool, Task, futures::check_ready},
    world_serialization::WorldAsset,
};
use feathrise_scene_content::{SceneId, ScenePlacementAssetTarget, ScenePlacementRecord};
use std::time::{Duration, Instant};

#[derive(Clone, Message)]
pub(crate) struct BlenderMeshAction {
    pub scene_id: SceneId,
    pub placement_id: String,
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
    task: Option<Task<Finished>>,
}

impl BlenderMeshSession {
    #[cfg(any(test, feature = "validation"))]
    pub(crate) fn has_open_session(&self) -> bool {
        self.active.is_some() || self.task.is_some()
    }
}

struct Session {
    target: BlenderMeshAction,
    basis: ScenePlacementRecord,
    process: BlenderProcess,
    next_poll: Instant,
    pending: Option<(String, Handle<WorldAsset>)>,
    ended: bool,
    preserve_files: bool,
}

enum Finished {
    Opened(Result<Session, String>),
    Polled(Session, Result<Option<String>, String>),
    Cleaned,
}

#[derive(SystemParam)]
pub(super) struct Author<'w, 's> {
    session: ResMut<'w, EditorAuthoringSession>,
    editor: ResMut<'w, SceneEditorState>,
    browser: Res<'w, EditorGltfBrowserState>,
    mesh_edit: Res<'w, MeshEditState>,
    batch: Res<'w, crate::authoring::placement_batch::PlacementBatchState>,
    moving: Res<'w, PlacementMoveState>,
    numeric_mouse: Res<'w, SceneEditorNumericScrubMouseGuard>,
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

    fn busy(&self) -> bool {
        self.editor.close_requested || self.editor.scene_transition_in_progress
            || self.editor.pending_scene_transition.is_some() || self.editor.save_requested
            || self.session.saving || self.session.model_vegetation_busy || self.session.vegetation_cross_busy
            || self.batch.busy() || self.mesh_edit.active()
            || self.moving.exit_pending() || self.numeric_mouse.blocked()
    }

    fn can_finish(&self, target: &BlenderMeshAction) -> bool {
        self.draft(target).is_none() || (!self.busy()
            && self.browser.placement_index_ready(self.session.placement_structure_revision))
    }

    fn adopt(&mut self, live: &mut Session, path: String) -> Result<(), String> {
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

    fn error(&mut self, live: &mut Session, error: &str) {
        live.preserve_files = true;
        self.editor.status = format!("Blender 模型更新失败：{error}。工作文件位于 {}", live.process.directory().display());
    }
}

pub(super) fn update(
    mut state: ResMut<BlenderMeshSession>,
    mut messages: MessageReader<BlenderMeshAction>,
    mut author: Author<'_, '_>,
) {
    if let Some(finished) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        match finished {
            Finished::Opened(Ok(live)) => {
                state.active = Some(live);
                author.editor.status = "Blender 启动后，保存即可更新模型".into();
            }
            Finished::Opened(Err(error)) => author.editor.status = error,
            Finished::Polled(mut live, result) => {
                match result {
                    Ok(Some(path)) => {
                        let asset = author.assets.load(GltfAssetLabel::Scene(0).from_asset(path.clone()));
                        live.pending = Some((path, asset));
                    }
                    Ok(None) => {}
                    Err(error) => author.error(&mut live, &error),
                }
                state.active = Some(live);
            }
            Finished::Cleaned => {}
        }
    }

    if let Some(target) = messages.read().last().cloned() {
        if state.active.is_some() || state.task.is_some() {
            author.editor.status = "请先关闭当前 Blender 窗口".into();
        } else if let Some((_, record)) = author.current(&target).filter(|_| !author.busy()) {
            let basis = record.clone();
            let folder = author.session.draft.as_ref().unwrap().folder.clone();
            let root = feathrise_editor_core::workspace_asset_root();
            state.task = Some(IoTaskPool::get().spawn(async move {
                Finished::Opened(BlenderProcess::start(root, folder, &basis).map(|process| Session {
                    target, basis, process, next_poll: Instant::now(), pending: None,
                    ended: false, preserve_files: false,
                }))
            }));
            author.editor.status = "正在打开 Blender".into();
        }
    }

    let Some(mut live) = state.active.take() else { return; };
    if let Some((_, handle)) = &live.pending
        && let Some((load, _, dependencies)) = author.assets.get_load_states(handle.id())
    {
        match (load, dependencies) {
            (LoadState::Failed(error), _) | (_, RecursiveDependencyLoadState::Failed(error)) => {
                author.error(&mut live, &error.to_string());
                live.pending = None;
            }
            (LoadState::Loaded, RecursiveDependencyLoadState::Loaded) if author.can_finish(&live.target) => {
                let (path, _handle) = live.pending.take().unwrap();
                match author.adopt(&mut live, path) {
                    Ok(()) => live.preserve_files = false,
                    Err(error) => author.error(&mut live, &error),
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
    } else if live.pending.is_none() && Instant::now() >= live.next_poll {
        live.next_poll = Instant::now() + Duration::from_millis(180);
        state.task = Some(IoTaskPool::get().spawn(async move {
            let (ended, result) = live.process.poll();
            live.ended = ended;
            Finished::Polled(live, result)
        }));
    } else {
        state.active = Some(live);
    }
}
