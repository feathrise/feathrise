//! 文件任务按实际源文件占用作者写入权，真实回执采用后才归还。

use super::resources::FileManagerState;
use crate::{
    authoring::{DraftLoadTask, EditorAuthoringSession, EditorSceneDraft},
    resources::SceneEditorState,
    ui::{
        asset_preview::{AnimationEditorState, DocumentState, weights::WeightEditor},
        scene_documents::SceneDocuments,
    },
};
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_editor_project_browser_model::file_operations::{FileOperationReceipt, FileSources};
use feathrise_scene_content::{
    SCENE_BOUNDARIES_FILE_NAME, SCENE_CATALOG_ASSET_PATH, SCENE_MANIFEST_FILE_NAME,
    SCENE_PORTALS_FILE_NAME, SCENE_SURFACE_FILE_NAME, SCENE_VEGETATION_FILE_NAME,
    SceneContentHandles, SceneContentRegistry, SceneId, SceneTerrainKind, sparse_height_manifest_asset_path,
};
use std::path::Path;

#[derive(Clone)]
struct Reservation {
    context: u64,
    id: u64,
    sources: FileSources,
}

#[derive(Clone, Default, Resource)]
pub(crate) struct FileWriteReservations(Vec<Reservation>);

impl FileWriteReservations {
    pub(crate) fn is_empty(&self) -> bool { self.0.is_empty() }

    pub(crate) fn blocks(&self, path: &Path) -> bool {
        self.0.iter().any(|job| job.sources.overlaps(path))
    }

    pub(crate) fn blocks_scene_load(&self, root: &Path, handles: &SceneContentHandles) -> bool {
        !self.is_empty() && scene_handles_match(handles, &|path| self.blocks(&root.join(path)))
    }

    pub(crate) fn blocks_scene_save(&self, root: &Path, draft: &EditorSceneDraft) -> bool {
        self.0.iter().any(|job| scene_write_matches(root, draft, true, &job.sources))
    }

    pub(crate) fn add(&mut self, context: u64, id: u64, sources: FileSources) {
        self.0.push(Reservation { context, id, sources });
    }
}

#[derive(SystemParam)]
pub(super) struct ProtectedAuthors<'w> {
    pub(super) documents: Option<ResMut<'w, DocumentState>>,
    animation: Option<Res<'w, AnimationEditorState>>,
    weights: Option<Res<'w, WeightEditor>>,
    scene: Option<Res<'w, SceneEditorState>>,
    session: Option<Res<'w, EditorAuthoringSession>>,
    scenes: Option<Res<'w, SceneDocuments>>,
    registry: Option<Res<'w, SceneContentRegistry>>,
    load: Option<Res<'w, DraftLoadTask>>,
}

impl ProtectedAuthors<'_> {
    pub(super) fn blocker(&self, sources: &FileSources) -> Option<String> {
        if self.documents.as_ref().is_some_and(|state| state.protects_file_changes(sources)) {
            return Some("选中文件包含未保存文档或正在写入的文档，请先保存或等待任务完成".into());
        }
        if self.animation.as_ref().is_some_and(|state| {
            state.sessions.iter().any(|(path, session)| session.dirty() && sources.overlaps(path))
                || state.work.as_ref().is_some_and(|work| sources.overlaps(&work.path))
                || (state.pending.is_some() || state.drag.is_some() || state.gesture.is_some() || state.pose_mouse_owned)
                    && state.active.as_ref().is_some_and(|path| sources.overlaps(path))
        }) {
            return Some("选中文件包含未保存动画或动画任务的源文件，请先保存或等待任务完成".into());
        }
        if self.weights.as_ref().is_some_and(|state| state.protects_file_changes(sources)) {
            return Some("选中文件包含未保存蒙皮权重或调权任务的文件，请先保存或等待任务完成".into());
        }
        let root = self.documents.as_ref().map(|state| state.asset_root().to_owned())
            .unwrap_or_else(feathrise_editor_core::workspace_asset_root);
        let matches_handle = |id: &SceneId| {
            self.registry.as_ref().and_then(|registry| registry.handles(id))
                .is_some_and(|handles| scene_handles_match(handles, &|path| sources.overlaps(&root.join(path))))
        };
        // 场景主文件由目录和打开的文档共同拥有。移动它们需要先归还文档，避免保存重建旧路径。
        let protected = |draft: &EditorSceneDraft, dirty: bool| {
            scene_write_matches(&root, draft, dirty, sources) || matches_handle(&draft.scene_id)
        };
        let active = self.session.as_ref().and_then(|session| session.draft.as_ref().filter(|draft| {
            let dirty = self.scene.as_ref().is_some_and(|scene| scene.dirty) || session.saving || session.has_scene_data_changes();
            protected(draft, dirty)
        }));
        let parked = self.scenes.as_ref().and_then(|scenes| scenes.parked.values().find_map(|scene| {
            let dirty = scene.dirty || scene.session.has_scene_data_changes();
            scene.session.draft.as_ref().filter(|draft| protected(draft, dirty))
        }));
        if let Some(draft) = active.or(parked) {
            return Some(format!("场景「{}」正在使用选中的作者文件，请先保存并关闭该场景", draft.manifest.display_name));
        }
        if let Some(id) = self.load.as_ref().filter(|load| load.task.is_some()).and_then(|load| load.desired_scene_id.as_ref())
            && matches_handle(id) {
            return Some(format!("场景「{id}」正在读取选中的作者文件，请等待场景加载后再关闭该场景"));
        }
        None
    }
}

fn scene_handles_match(handles: &SceneContentHandles, matches: &impl Fn(&Path) -> bool) -> bool {
    [handles.manifest.path(), handles.portals.path(), handles.placements.path(),
        handles.surface.as_ref().and_then(Handle::path), handles.vegetation.as_ref().and_then(Handle::path),
        handles.water.as_ref().and_then(Handle::path), handles.boundaries.as_ref().and_then(Handle::path)]
        .into_iter().flatten().any(|path| matches(path.path()))
        || handles.characters_path.as_deref().is_some_and(|path| matches(Path::new(path)))
}

fn scene_draft_matches(root: &Path, draft: &EditorSceneDraft, matches: &impl Fn(&Path) -> bool) -> bool {
    let folder = root.join(&draft.folder);
    [SCENE_MANIFEST_FILE_NAME, SCENE_PORTALS_FILE_NAME, SCENE_SURFACE_FILE_NAME, SCENE_VEGETATION_FILE_NAME, SCENE_BOUNDARIES_FILE_NAME]
        .into_iter().any(|file| matches(&folder.join(file)))
        || [draft.water_asset_path.as_str(), draft.manifest.terrain.glb_path.as_str(), SCENE_CATALOG_ASSET_PATH]
            .into_iter().chain(draft.manifest.terrain.height_path.as_deref())
            .chain(draft.manifest.terrain.height_texture_path.as_deref())
            .chain(draft.manifest.puddles.iter().flat_map(|puddles| [puddles.source_path.as_str(), puddles.texture_path.as_str()]))
            .chain(draft.surface.iter().flat_map(|surface| [surface.weight_source_path.as_str(), surface.weight_texture_path.as_str(),
                surface.index_source_path.as_str(), surface.index_texture_path.as_str()]))
            .chain(draft.preserved_asset_paths.iter().map(String::as_str))
            .chain(draft.domain.as_ref().map(|domain| domain.asset_path()))
            .filter(|path| !path.is_empty()).any(|path| matches(&root.join(path)))
        || matches(&root.join(draft.placements_path()))
}

fn scene_write_matches(root: &Path, draft: &EditorSceneDraft, dirty: bool, sources: &FileSources) -> bool {
    let matches = |path: &str| sources.overlaps(&root.join(path));
    if scene_draft_matches(root, draft, &|path| sources.overlaps(path)) { return true; }
    if !dirty { return false; }
    let folder = root.join(&draft.folder);
    // 体积生成器、模型植被分块和独立地形保存器各自拥有生产路径，限制在这些输出目录。
    if ["volume_sculpt", "vegetation/models"].into_iter().any(|subdir| sources.overlaps_tree(&folder.join(subdir)))
        || sources.overlaps_tree(&root.join("scenes").join(draft.scene_id.as_str()).join("terrains")) {
        return true;
    }
    if draft.manifest.terrain.kind == SceneTerrainKind::SparseClipmap {
        let manifest = root.join(sparse_height_manifest_asset_path(&draft.scene_id));
        if manifest.parent().is_some_and(|folder| ["height_tiles", "height_mips", "versions"]
            .into_iter().any(|subdir| sources.overlaps_tree(&folder.join(subdir)))) { return true; }
    }
    if !sources.overlaps_tree(&folder) { return false; }
    // 兼容旧作者文件的自定义名称。仅检查受影响场景的水/植被层路径，不查看实体或像素数据。
    draft.water.iter().chain(draft.terrain_documents.root_water.iter().filter_map(|canvas| canvas.water.as_ref()))
        .flat_map(|water| &water.layers)
        .any(|layer| matches(&layer.mask_source_path) || matches(&layer.mask_texture_path))
        || draft.vegetation.iter().chain(draft.terrain_documents.root_vegetation.iter().filter_map(|canvas| canvas.vegetation.as_ref()))
            .any(|vegetation| vegetation.layers.iter().any(|layer| matches(&layer.density_source_path) || matches(&layer.density_texture_path))
                || vegetation.authoring.as_ref().is_some_and(|authoring| authoring.layers.iter().any(|layer| matches(&layer.density_source_path))))
}

pub(super) fn settle(
    state: Res<FileManagerState>,
    mut reservations: ResMut<FileWriteReservations>,
    mut documents: Option<ResMut<DocumentState>>,
    mut editor: Option<ResMut<SceneEditorState>>,
    mut animation: Option<ResMut<AnimationEditorState>>,
    mut weights: Option<ResMut<WeightEditor>>,
    mut retirement: Option<ResMut<crate::authoring::AuthoringRetirement>>,
) {
    if reservations.0.is_empty() { return; }
    let Some(snapshot) = state.snapshot.as_ref() else { return; };
    let mut released = false;
    // 回执按执行顺序采用，连续改名 A→B→C 时标签也按同一顺序跟随。
    for receipt in &snapshot.completed {
        let Some(index) = reservations.0.iter().position(|job| job.context == receipt.context && job.id == receipt.id) else { continue; };
        if receipt.context == state.context {
            if let Some(documents) = documents.as_mut() { documents.apply_file_receipt(receipt, editor.as_deref_mut()); }
            if let Some(animation) = animation.as_deref_mut() {
                retire_animation_sources(animation, receipt, retirement.as_deref_mut());
            }
            if let Some(weights) = weights.as_mut() { weights.apply_file_receipt(receipt); }
        }
        reservations.0.remove(index);
        released = true;
    }
    if released && let Some(documents) = documents.as_mut() {
        documents.file_writes = reservations.clone();
    }
}

fn retire_animation_sources(
    animation: &mut AnimationEditorState,
    receipt: &FileOperationReceipt,
    retirement: Option<&mut crate::authoring::AuthoringRetirement>,
) {
    let removed = |path: &Path| receipt.change_for(path).is_some_and(|(change, _)| !change.source_retained);
    let retired: Vec<_> = animation.sessions.extract_if(|path, session| !session.dirty() && removed(path)).collect();
    if retired.is_empty() { return; }
    if animation.active.as_deref().is_some_and(removed) {
        // 旧模型预览可能还在显示。立即撤销旧源的编辑资格，由新路径重新进入原动画加载流程。
        animation.failed.clone_from(&animation.active);
        animation.playing = false;
        animation.status = "源 GLB 文件已移动或删除，请在新位置打开模型".into();
        animation.changed();
    }
    if let Some(retirement) = retirement {
        retirement.push(retired);
    } else if let Some(pool) = bevy::tasks::AsyncComputeTaskPool::try_get() {
        pool.spawn(async move { drop(retired); }).detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn live_scene_owns_its_author_files_without_owning_unrelated_files_in_its_folder() {
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let root = Path::new("C:/fixture/assets");
        let folder = root.join(&draft.folder);
        let overlaps = |path| {
            let selection = FileSources::from(vec![path]);
            scene_write_matches(root, &draft, true, &selection)
        };
        assert!(overlaps(folder.join(SCENE_MANIFEST_FILE_NAME)));
        assert!(overlaps(folder.clone()));
        assert!(overlaps(root.join(draft.placements_path())));
        assert!(overlaps(folder.join("vegetation/models/sample.json")));
        assert!(!overlaps(folder.join("notes.txt")));
        assert!(!overlaps(root.join("unrelated/scene.glb")));
    }
}
