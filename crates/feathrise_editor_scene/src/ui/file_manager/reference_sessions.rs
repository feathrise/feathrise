//! 干净的场景标签在引用发布期间暂留原草稿。文件回执与受影响句柄的热采用收口后，从原加载入口换基线。

use super::file_identity::scene_write_matches;
use crate::{authoring::{AuthoringRetirement, EditorAuthoringSession}, resources::SceneEditorState, ui::scene_documents::SceneDocuments};
use bevy::{asset::{LoadState, UntypedAssetId, UntypedHandle}, prelude::*};
use feathrise_editor_project_browser_model::file_operations::FileSources;
use feathrise_scene_content::*;
use std::{collections::HashSet, path::{Path, PathBuf}};

#[derive(Clone)]
pub(super) struct Reader {
    id: SceneId,
    folder: PathBuf,
    assets: Vec<(PathBuf, UntypedHandle)>,
}

pub(super) fn blocker(session: &EditorAuthoringSession, dirty: bool, root: &Path, files: &FileSources) -> Option<String> {
    let draft = session.draft.as_ref()?;
    if !scene_write_matches(root, draft, true, files) { return None; }
    if session.saving || (dirty || session.has_scene_data_changes()) && !session.is_unedited_load_migration_for(&draft.scene_id) {
        return Some(format!("场景「{}」有未保存的修改或正在写入，请先保存再确认引用更新", draft.manifest.display_name));
    }
    None
}

pub(super) fn hold(session: &mut EditorAuthoringSession, root: &Path, files: &FileSources, registry: Option<&SceneContentRegistry>) -> Option<Reader> {
    let draft = session.draft.as_ref()?;
    if !scene_write_matches(root, draft, true, files) { return None; }
    let mut assets = Vec::new();
    let mut add = |handle: UntypedHandle| {
        if let Some(relative) = handle.path() {
            let path = root.join(relative.path());
            if files.contains(&path) { assets.push((path, handle)); }
        }
    };
    if let Some(registry) = registry {
        add(registry.catalog.clone().untyped());
        if let Some(handles) = registry.handles(&draft.scene_id) {
            add(handles.manifest.clone().untyped());
            add(handles.portals.clone().untyped());
            add(handles.placements.clone().untyped());
            if let Some(h) = &handles.surface { add(h.clone().untyped()); }
            if let Some(h) = &handles.vegetation { add(h.clone().untyped()); }
            if let Some(h) = &handles.water { add(h.clone().untyped()); }
            if let Some(h) = &handles.boundaries { add(h.clone().untyped()); }
        }
    }
    session.saving = true; // 沿用所有作者命令已识别的在途写入门，确认之后不能生成新的脏稿。
    Some(Reader { id: draft.scene_id.clone(), folder: root.join(&draft.folder), assets })
}

struct Refresh { id: SceneId, reload: bool, waiting: HashSet<UntypedAssetId>, fresh: bool }
#[derive(Resource, Default)]
pub(super) struct Pending(Vec<Refresh>);

impl Pending {
    pub(super) fn published(&mut self, readers: Vec<Reader>, paths: &FileSources, server: Option<&AssetServer>) {
        for reader in readers {
            let mut waiting = HashSet::new();
            let mut reload = paths.overlaps_tree(&reader.folder);
            for (path, handle) in reader.assets {
                if !paths.contains(&path) { continue; }
                reload = true;
                if let (Some(server), Some(relative)) = (server, handle.path()) {
                    server.reload(relative.clone());
                    // 摆放主文件与旧 JSON 均由正常草稿加载器在后台回读，其余作者数据先等待对应正式资产换代。
                    if handle.id().type_id() != std::any::TypeId::of::<ScenePlacementsAsset>() { waiting.insert(handle.id()); }
                }
            }
            self.0.push(Refresh { id: reader.id, reload, waiting, fresh: true });
        }
    }
}

type Events<'w, 's, A> = Option<MessageReader<'w, 's, AssetEvent<A>>>;
fn observe<A: Asset>(events: &mut Events<A>, pending: &mut [Refresh]) {
    let Some(events) = events else { return; };
    if pending.is_empty() { events.clear(); return; }
    for event in events.read() {
        let (AssetEvent::Modified { id } | AssetEvent::Added { id } | AssetEvent::LoadedWithDependencies { id }) = event else { continue; };
        for job in pending.iter_mut().filter(|job| !job.fresh) { job.waiting.remove(&id.untyped()); }
    }
}

pub(super) fn adopt(
    mut pending: ResMut<Pending>, server: Option<Res<AssetServer>>,
    mut events: (Events<SceneCatalogAsset>, Events<SceneManifestAsset>, Events<ScenePortalsAsset>, Events<SceneSurfaceAsset>,
        Events<SceneVegetationAsset>, Events<SceneWaterAsset>, Events<SceneBoundariesAsset>),
    mut active: Option<ResMut<EditorAuthoringSession>>, mut scenes: Option<ResMut<SceneDocuments>>,
    mut editor: Option<ResMut<SceneEditorState>>, mut retirement: Option<ResMut<AuthoringRetirement>>,
) {
    // 连续推进事件游标，建立引用确认前后清晰的采用边界。只读有事件的句柄与正在等待的标签。
    observe(&mut events.0, &mut pending.0); observe(&mut events.1, &mut pending.0); observe(&mut events.2, &mut pending.0);
    observe(&mut events.3, &mut pending.0); observe(&mut events.4, &mut pending.0); observe(&mut events.5, &mut pending.0);
    observe(&mut events.6, &mut pending.0);
    pending.0.retain_mut(|job| {
        if std::mem::take(&mut job.fresh) && !job.waiting.is_empty() { return true; }
        let failed = server.as_ref().is_some_and(|server| job.waiting.iter().any(|&id| matches!(server.get_load_state(id), Some(LoadState::Failed(_)))));
        if !failed && !job.waiting.is_empty() { return true; }
        if let Some(session) = active.as_deref_mut().filter(|s| s.draft.as_ref().is_some_and(|d| d.scene_id == job.id)) {
            session.saving = false;
            if job.reload {
                let draft = session.draft.take();
                if let Some(retire) = retirement.as_deref_mut() { retire.push(draft); }
                if let Some(editor) = editor.as_deref_mut() {
                    editor.status = if failed { format!("场景 {} 的引用已更新，资产重新加载失败，请检查场景文件", job.id) }
                        else { format!("引用已更新，正在重新加载场景 {}", job.id) };
                }
            }
        } else if let Some(scenes) = scenes.as_deref_mut() && let Some(parked) = scenes.parked.get_mut(&job.id) {
            parked.session.saving = false;
            if job.reload {
                // 保留标签。下一次激活走正式磁盘加载，并以新文件重建撤销基线，避免恢复旧路径。
                let old = scenes.parked.remove(&job.id);
                if let Some(retire) = retirement.as_deref_mut() { retire.push(old); }
            }
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn reference_move_reloads_clean_active_and_parked_scenes_after_publication_and_keeps_drafts_on_failure() {
        let root = Path::new("C:/fixture/assets");
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let active_id = draft.scene_id.clone();
        let mut other = draft.clone();
        other.scene_id = SceneId("SCENE-other".into());
        other.folder = "scenes/other".into();
        let parked_id = other.scene_id.clone();
        let files = FileSources::from(vec![root.join(draft.placements_path()), root.join(other.placements_path())]);
        let mut active = EditorAuthoringSession { draft: Some(draft), ..default() };
        assert!(blocker(&active, true, root, &files).is_some());
        let reader = hold(&mut active, root, &files, None).unwrap();
        let mut pending = Pending::default();
        pending.published(vec![reader], &FileSources::default(), None);
        let mut app = App::new();
        app.insert_resource(active).insert_resource(pending).add_systems(Update, adopt);
        app.update();
        let mut active = app.world_mut().resource_mut::<EditorAuthoringSession>();
        assert!(!active.saving && active.draft.as_ref().unwrap().scene_id == active_id);
        let reader = hold(&mut active, root, &files, None).unwrap();
        let mut parked = crate::ui::scene_documents::SceneDocumentSnapshot::with_test_session(
            EditorAuthoringSession { draft: Some(other), ..default() });
        parked.dirty = false;
        let parked_reader = hold(&mut parked.session, root, &files, None).unwrap();
        let mut docs = SceneDocuments::default();
        docs.upsert(parked_id.clone(), "另一场景".into(), false);
        docs.parked.insert(parked_id.clone(), parked);
        app.insert_resource(docs);
        app.world_mut().resource_mut::<Pending>().published(vec![reader, parked_reader], &files, None);
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.draft.is_none() && !session.saving);
        let docs = app.world().resource::<SceneDocuments>();
        assert!(docs.contains(&parked_id) && !docs.parked.contains_key(&parked_id));
    }
}
