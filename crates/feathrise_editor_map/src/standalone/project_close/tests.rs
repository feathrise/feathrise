//! 普通地图保存调度回归；领域目录/采用回归已随实际RPG适配归原游戏。
use super::*;
use crate::{authoring::EditorAuthoringSession, resources::MapEditorState};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

/// 使用原原子写盘事务；只注入正式目录采用失败，不能靠直接填写调度器失败状态证明恢复。
#[derive(Resource)]
struct TextFileAuthor {
    root: PathBuf,
    path: PathBuf,
    dirty: bool,
    adopted: bool,
    requests: usize,
    polls: AtomicUsize,
}

impl TextFileAuthor {
    const DOMAIN: &'static str = "close-test-text";
    const ID: u64 = 7;
    const SAVED_BYTES: &'static [u8] = b"latest saved document\n";

    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "feathrise-project-close-adoption-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        // 独占创建，只清理由本夹具创建的单文件目录，绝不复用用户路径或旧测试输出。
        std::fs::create_dir(&root).unwrap();
        let path = root.join("document.txt");
        std::fs::write(&path, b"original document\n").unwrap();
        Self {
            root,
            path,
            dirty: true,
            adopted: false,
            requests: 0,
            polls: AtomicUsize::new(0),
        }
    }
}

impl Drop for TextFileAuthor {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.root);
    }
}

fn text_file_snapshot(world: &World) -> ProjectAuthorSnapshot {
    let author = world.resource::<TextFileAuthor>();
    ProjectAuthorSnapshot {
        unsaved: if author.dirty {
            vec![ProjectAuthorDocument {
                id: TextFileAuthor::ID,
                label: "普通文本作者文档".into(),
            }]
        } else {
            Vec::new()
        },
        ..default()
    }
}

fn save_text_file(world: &mut World, id: u64) -> Result<(), String> {
    use feathrise_map_content::authoring_transaction::{
        PreparedWrite, commit_prepared_writes,
    };
    assert_eq!(id, TextFileAuthor::ID);
    let mut author = world.resource_mut::<TextFileAuthor>();
    author.requests += 1;
    commit_prepared_writes(vec![PreparedWrite::new(
        author.path.clone(),
        TextFileAuthor::SAVED_BYTES.to_vec(),
    )])
    .map_err(|failure| failure.message().to_owned())?;
    // 原作者在真实写盘成功后清脏；正式采用由原poll另行确认。
    author.dirty = false;
    Ok(())
}

fn poll_text_file(world: &World, id: u64) -> Result<bool, String> {
    assert_eq!(id, TextFileAuthor::ID);
    let author = world.resource::<TextFileAuthor>();
    author.polls.fetch_add(1, Ordering::Relaxed);
    assert_eq!(
        std::fs::read(&author.path).unwrap(),
        TextFileAuthor::SAVED_BYTES
    );
    if !author.adopted {
        return Err("真实文本已写盘，正式目录尚未采用".into());
    }
    Ok(true)
}

#[test]
fn cancelling_after_real_write_and_failed_adoption_preserves_failure_until_original_poll_recovers()
{
    let mut world = World::new();
    world.insert_resource(TextFileAuthor::new());
    let mut adapters = ProjectAuthorAdapters::default();
    adapters
        .register(ProjectAuthorAdapter {
            domain: TextFileAuthor::DOMAIN,
            order: 0,
            collect: text_file_snapshot,
            saved_play_blocker: |_| None,
            request: save_text_file,
            poll: poll_text_file,
            catalog_hash: |_, _| None,
            rebase_own_write: |_, _, _| {},
        })
        .unwrap();
    world.insert_resource(adapters);
    request_save_all(&mut world).unwrap();
    advance_project_save(&mut world);
    assert!(!world.resource::<TextFileAuthor>().dirty);
    assert!(project_close_snapshot(&world).unsaved.is_empty());
    advance_project_save(&mut world);
    let failed = project_save_progress(&world);
    assert!(matches!(
        &failed,
        ProjectSaveProgress::Failed { detail } if detail.contains("正式目录尚未采用")
    ));
    assert!(world.resource::<ProjectSaveState>().failed_active.is_some());

    cancel_project_save(&mut world);
    assert_eq!(project_save_progress(&world), failed);
    assert!(world.resource::<ProjectSaveState>().failed_active.is_some());
    advance_project_save(&mut world);
    assert_eq!(
        world
            .resource::<TextFileAuthor>()
            .polls
            .load(Ordering::Relaxed),
        1
    );

    // 没有脏文档的新队列也必须重验旧poll；取消与重试都不能将采用失败变成空队列成功。
    request_save_all(&mut world).unwrap();
    advance_project_save(&mut world);
    assert_eq!(project_save_progress(&world), failed);
    assert_eq!(world.resource::<TextFileAuthor>().requests, 1);
    assert_eq!(
        world
            .resource::<TextFileAuthor>()
            .polls
            .load(Ordering::Relaxed),
        2
    );
    cancel_project_save(&mut world);
    world.resource_mut::<TextFileAuthor>().adopted = true;
    request_save_all(&mut world).unwrap();
    advance_project_save(&mut world);
    assert_eq!(
        project_save_progress(&world),
        ProjectSaveProgress::Succeeded
    );
    let author = world.resource::<TextFileAuthor>();
    assert_eq!(author.requests, 1, "复验采用不得重复写盘");
    assert_eq!(author.polls.load(Ordering::Relaxed), 3);
    assert_eq!(
        std::fs::read(&author.path).unwrap(),
        TextFileAuthor::SAVED_BYTES
    );
    assert!(world.resource::<ProjectSaveState>().failed_active.is_none());
}

#[test]
fn no_map_success_sequence_is_a_failure_even_if_task_slot_is_empty() {
    let mut world = World::new();
    world.init_resource::<MapEditorState>();
    world.init_resource::<EditorAuthoringSession>();
    let result = maps::poll(&world, None, Some(0));
    assert!(result.unwrap_err().contains("没有成功回执"));
}

#[test]
fn cancelling_while_restoring_a_parked_map_never_starts_its_save_request() {
    let mut world = World::new();
    let map = feathrise_map_content::MapId::parse("MAP-cancel-selecting").unwrap();
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.map_id = map.clone();
    draft.manifest.id = map.clone();
    world.insert_resource(MapEditorState {
        open: true,
        dirty: true,
        selected_map_id: Some(map.clone()),
        ..default()
    });
    world.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        revision: 7,
        ..default()
    });
    world.insert_resource(ProjectSaveState {
        progress: ProjectSaveProgress::Running {
            current: "恢复后台地图".into(),
        },
        active: Some(ActiveSave {
            item: SaveItem {
                target: SaveTarget::Map(map),
                label: "后台地图".into(),
            },
            stage: SaveStage::SelectingMap,
            catalog_before: None,
        }),
        ..default()
    });
    cancel_project_save(&mut world);
    advance_project_save(&mut world);
    assert_eq!(project_save_progress(&world), ProjectSaveProgress::Idle);
    assert!(!world.resource::<MapEditorState>().save_requested);
    assert!(world.resource::<MapEditorState>().dirty);
    assert_eq!(world.resource::<EditorAuthoringSession>().revision, 7);
}

#[test]
fn embedded_play_allows_current_map_without_saving_but_rejects_original_external_dirty_owner() {
    let mut world = World::new();
    let draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    world.insert_resource(MapEditorState {
        open: true,
        dirty: true,
        ..default()
    });
    world.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        revision: 7,
        ..default()
    });
    assert!(embedded_play_blocker(&world).is_none());
    world.insert_resource(TextFileAuthor::new());
    let mut adapters = ProjectAuthorAdapters::default();
    adapters
        .register(ProjectAuthorAdapter {
            domain: TextFileAuthor::DOMAIN,
            order: 0,
            collect: text_file_snapshot,
            saved_play_blocker: |_| None,
            request: save_text_file,
            poll: poll_text_file,
            catalog_hash: |_, _| None,
            rebase_own_write: |_, _, _| {},
        })
        .unwrap();
    world.insert_resource(adapters);
    assert!(
        embedded_play_blocker(&world)
            .unwrap()
            .contains("普通文本作者文档")
    );
    assert_eq!(
        world.resource::<TextFileAuthor>().requests,
        0,
        "预览预检不能代替用户保存"
    );
    assert_eq!(world.resource::<EditorAuthoringSession>().revision, 7);
    assert!(world.resource::<MapEditorState>().dirty);
    world.resource_mut::<TextFileAuthor>().dirty = false;
    assert!(embedded_play_blocker(&world).is_none());
    world.resource_mut::<MapEditorState>().save_requested = true;
    assert!(embedded_play_blocker(&world).unwrap().contains("事务"));
}

#[test]
fn embedded_play_preserves_isolated_document_drafts_without_claiming_their_preview_is_gameplay() {
    let map = feathrise_map_content::MapId::parse("MAP-current").unwrap();
    let other = feathrise_map_content::MapId::parse("MAP-parked").unwrap();
    assert!(!requires_external_projection(&SaveTarget::Map(map)));
    assert!(!requires_external_projection(&SaveTarget::GlobalEnvironment));
    for target in [
        SaveTarget::Map(other),
        SaveTarget::Documents,
        SaveTarget::Animation("clip.glb".into()),
        SaveTarget::Surface(1),
        SaveTarget::Vegetation(1),
        SaveTarget::Water(1),
        SaveTarget::Building(1),
    ] {
        assert!(
            !requires_external_projection(&target),
            "原隔离文档及停放地图不应新增强制保存门"
        );
    }
    assert!(requires_external_projection(&SaveTarget::Domain("unknown-gameplay", 1)));
}

#[test]
fn embedded_play_dirty_surface_form_keeps_draft_and_still_obeys_original_input_gate() {
    use feathrise_editor_surface_model::{SurfacePresetEditorState, types::{SurfaceDraft, SurfaceDraftMode}};
    let mut world = World::new();
    let descriptor = serde_json::from_value(serde_json::json!({
        "id":"unsaved-surface", "label":"未保存样品", "base_color_srgba":[1.,1.,1.,1.],
        "tile_meters":2., "normal_strength":1., "height_blend_strength":0.2,
        "perceptual_roughness":0.7, "metallic":0.
    })).unwrap();
    let mut state = SurfacePresetEditorState::default();
    let id = state.open_draft(SurfaceDraft::new(SurfaceDraftMode::Create, descriptor));
    world.insert_resource(state);
    assert!(project_close_snapshot(&world).unsaved.iter().any(|label| label.contains("未保存样品")));
    assert!(embedded_play_blocker(&world).is_none(), "隔离预设草稿不应强制保存后才可预览");
    let state = world.resource::<SurfacePresetEditorState>();
    assert_eq!(state.tabs[0].id, id);
    assert!(matches!(state.tabs[0].draft.mode, SurfaceDraftMode::Create));
    assert!(state.write_task.is_none());
    let mut numeric = feathrise_editor_ui::ActiveNumericInput::default();
    numeric.entity = Some(Entity::PLACEHOLDER);
    world.insert_resource(numeric);
    assert!(embedded_play_blocker(&world).unwrap().contains("数值"));
}
