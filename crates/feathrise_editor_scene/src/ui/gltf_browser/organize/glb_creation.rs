//! 场景树新建或选择独立 GLB。文件工作完成后，向冻结的场景与文件夹加入摆放。

use super::{EditorGltfBrowserState, folder};
use crate::{
    authoring::{
        AuthoringRetirement, EditorAuthoringSession, EditorCommand, EditorSceneDraft,
        next_author_id, safe_asset_path, spawn_placement_preview,
        placement_deletion::PlacementDeletionIdentity,
    },
    resources::SceneEditorState,
    ui::{
        file_manager::{FileManagerState, PickedSceneGlb},
        scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectRequest, SceneObjectTarget},
    },
    workbench::WorkbenchModel,
};
use bevy::{asset::{LoadState, RecursiveDependencyLoadState}, gltf::GltfAssetLabel, prelude::*, world_serialization::WorldAsset};
use feathrise_editor_project_browser_model::file_operations::{FileOperation, FileOperationRequest, FileOperationService};
use feathrise_scene_content::{SceneId, ScenePlacementAssetTarget, ScenePlacementEditorMetadata, ScenePlacementHeightMode, ScenePlacementRecord};
use std::{collections::VecDeque, path::Path};

#[derive(Clone)]
struct Destination {
    context: u64,
    document: PlacementDeletionIdentity,
    scene: SceneId,
    group: Option<String>,
    folder_id: Option<usize>,
}

impl Destination {
    fn capture(context: u64, session: &EditorAuthoringSession, draft: &EditorSceneDraft, target: &SceneObjectTarget) -> Self {
        let group = folder(&target.key).then(|| target.name.clone());
        let folder_id = group.as_deref().and_then(|name| draft.object_folders.id_for(name));
        Self { context, document: session.placement_deletions.identity(), scene: draft.scene_id.clone(), group, folder_id }
    }

    fn current(&self, context: u64, session: &EditorAuthoringSession, draft: Option<&EditorSceneDraft>) -> bool {
        draft.is_some_and(|draft| draft.scene_id == self.scene) && self.context == context
            && self.document.matches(&session.placement_deletions)
    }

    fn folder_current(&self, draft: &EditorSceneDraft, browser: &EditorGltfBrowserState, revision: u64) -> Option<bool> {
        let Some(group) = self.group.as_deref() else { return Some(true); };
        if let Some(id) = self.folder_id { return Some(draft.object_folders.id_for(group) == Some(id)); }
        // 旧场景的隐式文件夹由已发布摆放索引判断，索引换代时等待完整结果。
        (browser.document_selection_ready(&self.scene) && browser.placement_index_ready(revision))
            .then(|| browser.placement_group_names.contains(group))
    }
}

struct Placement {
    destination: Destination,
    name: String,
    asset_path: String,
}

pub(super) struct Pending {
    id: u64,
    placement: Placement,
    outcome: Option<Result<(), String>>,
}

pub(super) struct Picker {
    window: u64,
    destination: Destination,
    selection: Option<(String, Handle<WorldAsset>)>,
}

fn submit(
    service: &FileOperationService,
    context: u64,
    session: &EditorAuthoringSession,
    draft: &EditorSceneDraft,
    request: &SceneObjectRequest,
) -> Result<Pending, String> {
    let input = request.name.trim();
    let name = input.rsplit_once('.').filter(|(_, extension)| extension.eq_ignore_ascii_case("glb"))
        .map_or(input, |(name, _)| name).trim();
    if name.is_empty() || name.contains('#') {
        return Err("请输入 GLB 名称，名称不能包含 #".into());
    }
    let filename = format!("{name}.glb");
    let root = feathrise_editor_core::workspace_asset_root();
    let scene_folder = draft.folder.trim_end_matches('/');
    let parent = safe_asset_path(&root, scene_folder)?;
    let asset_path = format!("{scene_folder}/models/{filename}");
    let id = service.enqueue(FileOperationRequest {
        context,
        allowed_roots: vec![root],
        references: None,
        operation: FileOperation::CreateSceneGlb { parent, name: filename },
    })?;
    Ok(Pending {
        id, outcome: None,
        placement: Placement {
            destination: Destination::capture(context, session, draft, &request.target),
            name: name.into(), asset_path,
        },
    })
}

pub(super) fn advance(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut files: ResMut<FileManagerState>,
    service: Res<FileOperationService>,
    menu: Res<SceneContextMenuState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut browser: ResMut<EditorGltfBrowserState>,
    mut retirement: ResMut<AuthoringRetirement>,
    mut pending: Local<VecDeque<Pending>>,
    mut picker: Local<Option<Picker>>,
    mut picked: MessageReader<PickedSceneGlb>,
    host: Option<Res<WorkbenchModel>>,
) {
    let context = files.service_context();
    let draft = session.draft.as_ref().filter(|draft| {
        editor.open && !editor.close_requested && !editor.scene_transition_in_progress
            && editor.pending_scene_transition.is_none() && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
    });
    pending.retain_mut(|job| {
        if !job.placement.destination.current(context, &session, draft) {
            // 切换草稿时取消原任务，迟到回执不能加入其他草稿。
            service.cancel(job.id);
            return false;
        }
        // 保存、菜单和树索引可能暂缓采用。先记录回执，避免它滑出文件服务的历史窗口。
        if job.outcome.is_none() && let Some(receipt) = files.receipts().rev().find(|receipt| receipt.id == job.id) {
            job.outcome = Some(match &receipt.error {
                Some(error) => Err(error.clone()),
                None if receipt.completed.is_empty() => Err(format!("空 GLB「{}」创建已取消", job.placement.name)),
                None => Ok(()),
            });
        }
        true
    });
    if picker.as_ref().is_some_and(|current| !current.destination.current(context, &session, draft)
        || !files.glb_picker_open(current.window))
    {
        files.close_glb_picker(picker.take().unwrap().window);
    }
    let choice = picked.read().filter(|choice|
        picker.as_ref().is_some_and(|current| current.window == choice.window)).last();
    let Some(draft) = draft else { return; };
    // 菜单请求只发布一帧。同场景继续使用共享队列，不等待前一份回执。
    if let Some(request) = menu.ready.as_ref().filter(|request| {
        matches!(request.action, SceneMenuAction::NewEmptyGlb | SceneMenuAction::ChooseExistingGlb)
            && request.target.allows(request.action)
            && request.target.current(&editor, &session)
    }) {
        if request.action == SceneMenuAction::ChooseExistingGlb {
            let root = host.as_ref().map_or_else(|| Some(feathrise_editor_core::workspace_asset_root()), |host| host.asset_root.clone());
            if let Some(window) = root.and_then(|root| files.open_glb_picker(root)) {
                *picker = Some(Picker { window, destination: Destination::capture(context, &session, draft, &request.target), selection: None });
            }
            return;
        }
        editor.status = match submit(&service, context, &session, draft, request) {
            Ok(job) => {
                let notice = format!("空 GLB「{}」已排队", job.placement.name);
                pending.push_back(job);
                notice
            }
            Err(error) => error,
        };
        return;
    }
    if let (Some(picker), Some(choice)) = (picker.as_mut(), choice) {
        let own_glb = draft.placement_glb_hash.is_some() && if cfg!(windows) {
            draft.placements_path().eq_ignore_ascii_case(&choice.path)
        } else { draft.placements_path() == choice.path };
        // 文件窗只交回已确认的工程资产相对路径；当前主 GLB 不得递归引用自身。
        if own_glb || choice.path.contains('#') {
            picker.selection = None;
            editor.status = "请选择当前主场景之外、名称不含 # 的 GLB 模型".into();
        } else {
            let handle = assets.load(GltfAssetLabel::Scene(0).from_asset(choice.path.clone()));
            picker.selection = Some((choice.path.clone(), handle));
            editor.status = "正在读取所选 GLB".into();
        }
    }
    if session.saving || editor.save_requested || editor.scene_switch_confirmation.is_some() || menu.has_pending_interaction() { return; }
    if let Some(picker) = picker.as_mut() {
        if let Some((path, handle)) = &picker.selection {
            match assets.get_load_states(handle.id()) {
                Some((LoadState::Failed(error), _, _)) | Some((_, _, RecursiveDependencyLoadState::Failed(error))) => {
                    editor.status = format!("所选 GLB 无法加载：{error}");
                    picker.selection = None;
                }
                Some((LoadState::Loaded, _, RecursiveDependencyLoadState::Loaded)) => {
                    match picker.destination.folder_current(draft, &browser, session.placement_structure_revision) {
                        Some(true) => {
                            let name = Path::new(path).file_stem().unwrap_or_default().to_string_lossy().into_owned();
                            let placement = Placement { destination: picker.destination.clone(), name, asset_path: path.clone() };
                            picker.selection = None;
                            files.close_glb_picker(picker.window);
                            editor.status = format!("已添加 GLB「{}」，请保存场景以保留摆放", placement.name);
                            adopt(&mut commands, &assets, &mut session, &mut editor, &mut browser, &mut retirement, placement);
                            return;
                        }
                        Some(false) => {
                            picker.selection = None;
                            files.close_glb_picker(picker.window);
                            editor.status = "目标文件夹已变化，请重新选择".into();
                        }
                        None => {}
                    }
                }
                _ => {}
            }
        }
    }
    let Some(job) = pending.front() else { return; };
    match job.outcome.as_ref() {
        None => return,
        Some(Err(error)) => {
            editor.status = error.clone();
            pending.pop_front();
            return;
        }
        Some(Ok(())) => {}
    }
    match job.placement.destination.folder_current(draft, &browser, session.placement_structure_revision) {
        None => return,
        Some(false) => {
            editor.status = format!("空 GLB 已创建为「{}」，目标文件夹已变化", job.placement.asset_path);
            pending.pop_front();
            return;
        }
        Some(true) => {}
    }
    let job = pending.pop_front().expect("已核对的文件任务");
    editor.status = format!("已新建空 GLB「{}」，请保存场景以保留摆放", job.placement.name);
    adopt(&mut commands, &assets, &mut session, &mut editor, &mut browser, &mut retirement, job.placement);
}

fn adopt(
    commands: &mut Commands, assets: &AssetServer, session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState, browser: &mut EditorGltfBrowserState,
    retirement: &mut AuthoringRetirement, placement: Placement,
) {
    let id = next_author_id(session, "placement");
    let record = ScenePlacementRecord {
        id: id.clone(),
        asset_path: placement.asset_path,
        asset_target: ScenePlacementAssetTarget::Scene0,
        editor: ScenePlacementEditorMetadata {
            name: placement.name,
            group: placement.destination.group.clone().unwrap_or_default(),
            ..default()
        }.into(),
        mesh_lod: None, terrain_binding: None, terrain_patch: None, building_preset_id: None, courtyard: None,
        translation: [0.0; 3], height_mode: ScenePlacementHeightMode::FixedHeight, terrain_offset: 0.0,
        yaw_degrees: 0.0, scale: [1.0; 3], water_surfaces: Vec::new(),
    };
    // 新文件发布或已有文件加载完成后，沿用普通摆放的预览、保存和撤销。
    let preview = spawn_placement_preview(commands, assets, &placement.destination.scene, &record);
    session.draft.as_mut().expect("已核对的场景草稿").placements.placements.push(record.clone());
    session.placement_previews.insert(id, preview);
    let mut retired = std::mem::take(&mut session.redo);
    session.push_bounded_history_retiring(EditorCommand::PlacementAdded(record), &mut retired);
    retirement.push(retired);
    session.mark_placement_structure_changed(editor);
    browser.collapsed_main_glb = None; // 在折叠后的树空白处建模时，也显示新摆放。
    if let Some(group) = placement.destination.group { browser.expanded_groups.insert(group); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::scene_context_menu::{SceneObjectKey, SceneObjectTarget};
    use feathrise_editor_project_browser_model::file_operations::{FileOperationBackend, FileOperationSnapshot};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct HeldCreations {
        names: Mutex<Vec<String>>,
        cancelled: Mutex<Vec<u64>>,
    }

    impl FileOperationBackend for HeldCreations {
        fn enqueue(&self, request: FileOperationRequest) -> Result<u64, String> {
            let FileOperation::CreateSceneGlb { name, .. } = request.operation else { panic!("预期新建场景 GLB"); };
            let mut names = self.names.lock().unwrap();
            names.push(name);
            Ok(names.len() as u64)
        }

        fn snapshot(&self) -> Option<FileOperationSnapshot> { None }
        fn cancel_context(&self, _: u64) {}
        fn is_busy(&self) -> bool { true }

        fn cancel(&self, id: u64) -> bool {
            self.cancelled.lock().unwrap().push(id);
            true
        }
    }

    #[test]
    fn accepts_another_creation_while_the_first_file_is_still_pending() {
        let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
        Arc::make_mut(&mut draft.object_folders).insert("文件夹".into());
        let scene_id = draft.scene_id.clone();
        let backend = Arc::new(HeldCreations::default());
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .insert_resource(FileOperationService(backend.clone()))
            .init_resource::<FileManagerState>()
            .init_resource::<SceneContextMenuState>()
            .add_message::<PickedSceneGlb>()
            .init_resource::<EditorGltfBrowserState>()
            .init_resource::<AuthoringRetirement>()
            .insert_resource(SceneEditorState { open: true, selected_scene_id: Some(scene_id.clone()), ..default() })
            .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
            .add_systems(Update, advance);

        let revision = app.world().resource::<EditorAuthoringSession>().revision;
        // 每次确认只发布一帧；第一次仍在等待回执时，第二次也须交给文件服务。
        for (name, key, target_name) in [
            ("根空模型", SceneObjectKey::PlacementTreeRoot, ""),
            ("夹内空模型", SceneObjectKey::PlacementFolder { empty: true }, "文件夹"),
        ] {
            app.world_mut().resource_mut::<SceneContextMenuState>().ready = Some(SceneObjectRequest {
                target: SceneObjectTarget {
                    scene_id: scene_id.clone(), revision, key, name: target_name.into(),
                    hidden: false, locked: false, copyable: false,
                },
                action: SceneMenuAction::NewEmptyGlb,
                name: name.into(),
            });
            app.update();
        }
        assert_eq!(backend.names.lock().unwrap().as_slice(), ["根空模型.glb", "夹内空模型.glb"]);
        assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.is_empty());
        app.world_mut().resource_mut::<SceneContextMenuState>().ready = None;
        app.world_mut().resource_mut::<SceneEditorState>().close_requested = true;
        app.update();
        assert_eq!(backend.cancelled.lock().unwrap().as_slice(), [1, 2]);
    }
}
