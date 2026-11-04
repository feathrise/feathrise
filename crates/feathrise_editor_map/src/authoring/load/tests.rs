//! F12作者草稿加载门的纯数据回归测试。

use feathrise_map_content::{MAP_SURFACE_SCHEMA_VERSION, MapId, MapSurfaceAsset}; // 使用正式公开版本与文档类型构造最小夹具。

use super::validate_surface_authoring_identity; // 直接锁定启动后台任务前的同步身份门。

use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::IoTaskPool;
use feathrise_map_content::{MapCatalogAsset, MapCatalogEntry};

use super::{
    DraftLoadTask, EditorAuthoringSession, EditorMapDraft, MapEditorState, SurfacePreviewState,
    VegetationPreviewState, WaterBrushStroke, WaterPreviewState, poll_draft_load,
};
use crate::authoring::{GlobalEnvironmentAuthoringState, test_heightfield_draft};

/// 使用正式完成值采用系统建立作者基线，不绕过“载入即脏”的原始触发路径。
fn load_completed_draft(migrated: bool) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<SurfacePreviewState>()
        .init_resource::<VegetationPreviewState>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<feathrise_scene::DynamicWaterMaterial>>()
        .add_systems(Update, poll_draft_load);
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    if migrated {
        draft.vegetation_registration_source_hash = Some("missing".to_owned());
        draft.water_catalog_registration_required = true;
        draft.boundary_catalog_registration_required = true;
        draft
            .vegetation_author_dirty_layers
            .insert("旧层作者源".to_owned()); // 一对一迁移同样属于可重建的加载结果。
    }
    let map_id = draft.map_id.clone();
    app.insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(map_id.clone()),
        ..default()
    });
    adopt_completed_draft(&mut app, draft);
    app
}

/// 连续换图复用同一会话和正式采用系统，不能用每次重新初始化资源掩盖基线换代问题。
fn adopt_completed_draft(app: &mut App, draft: EditorMapDraft) {
    let map_id = draft.map_id.clone();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = Some(map_id.clone());
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .pending_map_transition = None;
    app.init_resource::<DraftLoadTask>();
    let mut load = app.world_mut().resource_mut::<DraftLoadTask>();
    load.ticket = load.ticket.wrapping_add(1);
    let ticket = load.ticket;
    load.desired_map_id = Some(map_id.clone());
    load.task =
        Some(IoTaskPool::get().spawn(async move { (ticket, Ok::<EditorMapDraft, String>(draft)) }));
    drop(load);
    for _ in 0..100 {
        app.update();
        if app
            .world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .is_some_and(|draft| draft.map_id == map_id)
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1)); // 仅测试等待真实任务完成，不占用产品帧路径。
    }
    panic!("最小作者完成值必须在测试等待预算内被采用");
}

#[test]
fn loaded_migration_keeps_save_responsibility_and_allows_the_next_map_entry() {
    let mut app = load_completed_draft(true);
    let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    let global = app.world().resource::<GlobalEnvironmentAuthoringState>();
    assert!(editor.dirty && session.has_map_changes()); // 保留显式保存及目录补登记责任，不直接清脏丢数据。
    assert!(!session.needs_save_before_map_switch(&editor, global));
    let current = editor.selected_map_id.clone();
    let target = MapId::parse("MAP-next").unwrap();
    editor.map_list_selection_id = Some(target.clone());
    let catalog = MapCatalogAsset {
        schema_version: feathrise_map_content::MAP_SCHEMA_VERSION,
        maps: vec![MapCatalogEntry {
            id: target.clone(),
            display_name: "下一张地图".to_owned(),
            folder: format!("maps/{target}"),
            published: true,
            surface_path: None,
            vegetation_path: None,
            water_path: None,
            characters_path: None,
            boundary_path: None,
        }],
    };
    crate::ui::rows::request_selected_map_entry(
        &mut editor,
        &crate::resources::MapDeletionTask::default(),
        Some(&catalog),
        session,
        global,
        false,
    );
    assert_eq!(editor.pending_map_transition.as_ref(), Some(&target)); // 正式按钮路径不再报未编辑地图的未保存错误。
    assert_eq!(editor.selected_map_id, current); // 仍等待清单验证，不能在按钮帧提前发布目标。
}

#[test]
fn repeated_loaded_migrations_allow_roundtrip_and_still_protect_a_real_edit() {
    let mut app = load_completed_draft(true);
    let first = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .clone();
    let mut second = first.clone();
    second.map_id = MapId::parse("MAP-roundtrip-second").unwrap();
    second.folder = format!("maps/{}", second.map_id);
    second.manifest.id = second.map_id.clone();
    second.placements.map_id = second.map_id.clone();
    second.portals.map_id = second.map_id.clone();
    second.domain = None;
    second.water_asset_path = format!("{}/water.map_water.json", second.folder);
    let catalog = MapCatalogAsset {
        schema_version: feathrise_map_content::MAP_SCHEMA_VERSION,
        maps: [&first, &second]
            .into_iter()
            .map(|draft| MapCatalogEntry {
                id: draft.map_id.clone(),
                display_name: draft.map_id.to_string(),
                folder: draft.folder.clone(),
                published: true,
                surface_path: None,
                vegetation_path: None,
                water_path: None,
                characters_path: None,
                boundary_path: None,
            })
            .collect(),
    };
    let mut expected_revision = app.world().resource::<EditorAuthoringSession>().revision;
    for next in [second.clone(), first.clone(), second] {
        let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
        editor.map_list_selection_id = Some(next.map_id.clone());
        crate::ui::rows::request_selected_map_entry(
            &mut editor,
            &crate::resources::MapDeletionTask::default(),
            Some(&catalog),
            app.world().resource::<EditorAuthoringSession>(),
            app.world().resource::<GlobalEnvironmentAuthoringState>(),
            false,
        );
        assert_eq!(editor.pending_map_transition.as_ref(), Some(&next.map_id));
        app.insert_resource(editor);
        adopt_completed_draft(&mut app, next);
        expected_revision = expected_revision.wrapping_add(1);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.loaded_migration_revision, Some(expected_revision)); // 每次采用都使用新基线，不能只让第一次切图通过。
        assert!(app.world().resource::<MapEditorState>().dirty && session.has_map_changes()); // 浏览不清除显式保存责任。
    }
    let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .mark_changed(&mut editor);
    editor.map_list_selection_id = Some(first.map_id);
    crate::ui::rows::request_selected_map_entry(
        &mut editor,
        &crate::resources::MapDeletionTask::default(),
        Some(&catalog),
        app.world().resource::<EditorAuthoringSession>(),
        app.world().resource::<GlobalEnvironmentAuthoringState>(),
        false,
    );
    assert!(editor.pending_map_transition.is_none()); // 多次换图之后真实作者变化仍然不能被放行。
    assert!(editor.status.contains("未保存修改"));
}

#[test]
fn user_edit_after_loaded_migration_still_blocks_map_switch() {
    let mut app = load_completed_draft(true);
    let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .mark_changed(&mut editor); // 即使尚未压入撤销命令，也必须保护真实作者变化。
    let session = app.world().resource::<EditorAuthoringSession>();
    let global = app.world().resource::<GlobalEnvironmentAuthoringState>();
    assert!(session.needs_save_before_map_switch(&editor, global));
}

#[test]
fn global_edits_saving_and_stale_map_identity_cannot_use_load_migration_exemption() {
    let mut app = load_completed_draft(true);
    let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
    let mut global = GlobalEnvironmentAuthoringState::default();
    global.dirty = true;
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .needs_save_before_map_switch(&editor, &global)
    );
    global.dirty = false;
    editor.selected_map_id = Some(MapId::parse("MAP-other").unwrap());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .needs_save_before_map_switch(&editor, &global)
    );
    editor.selected_map_id = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .map(|draft| draft.map_id.clone());
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .needs_save_before_map_switch(&editor, &global)
    );
}

#[test]
fn ordinary_loaded_map_starts_clean_without_a_migration_exemption() {
    let app = load_completed_draft(false);
    assert!(!app.world().resource::<MapEditorState>().dirty);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .loaded_migration_revision
            .is_none()
    );
}

#[test]
fn ordinary_loader_reopens_custom_game_document_path_without_loading_or_creating_its_body() {
    use feathrise_map_content::{
        MapBoundariesAsset, MapContentHandles, MapContentRegistry, MapManifestAsset,
        MapPlacementsAsset, MapPortalsAsset, MapTerrainDescriptor, MapVegetationAsset,
        MapWaterAsset,
    };
    let mut app = load_completed_draft(false);
    app.init_resource::<Assets<MapCatalogAsset>>()
        .init_resource::<Assets<MapManifestAsset>>()
        .init_resource::<Assets<MapPlacementsAsset>>()
        .init_resource::<Assets<MapPortalsAsset>>()
        .init_resource::<Assets<MapSurfaceAsset>>()
        .init_resource::<Assets<MapVegetationAsset>>()
        .init_resource::<Assets<MapWaterAsset>>()
        .init_resource::<Assets<MapBoundariesAsset>>()
        .add_message::<AssetEvent<MapBoundariesAsset>>()
        .add_systems(Update, super::start_draft_load.before(poll_draft_load));
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = MapId::parse("MAP-uninterpreted-body").unwrap();
    let private_path = format!("maps/{map}/private-actors.map_characters.json");
    draft.manifest.id = map.clone();
    draft.placements.map_id = map.clone();
    draft.portals.map_id = map.clone();
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.manifest.runtime_scene = feathrise_map_content::MapRuntimeKind::DataDriven.as_str().into();
    draft.manifest.terrains = Some(Default::default()); // 合法空容器隔离可选作者文件，不靠虚构场景类型绕过加载约束。
    let manifest = app
        .world_mut()
        .resource_mut::<Assets<MapManifestAsset>>()
        .add(draft.manifest);
    let placements = app
        .world_mut()
        .resource_mut::<Assets<MapPlacementsAsset>>()
        .add(draft.placements);
    let portals = app
        .world_mut()
        .resource_mut::<Assets<MapPortalsAsset>>()
        .add(draft.portals);
    let water = app.world_mut().resource_mut::<Assets<MapWaterAsset>>()
        .add(crate::authoring::water_defaults::default_water_asset(&map)); // 使用合法空水正文，避免测试去读取任意工程磁盘源。
    let catalog = app
        .world_mut()
        .resource_mut::<Assets<MapCatalogAsset>>()
        .add(MapCatalogAsset {
            schema_version: 1,
            maps: vec![MapCatalogEntry {
                id: map.clone(),
                display_name: "保留旧游戏正文".into(),
                folder: format!("maps/{map}"),
                published: true,
                surface_path: None,
                vegetation_path: None,
                water_path: Some(format!("maps/{map}/water.map_water.json")),
                characters_path: Some(private_path.clone()),
                boundary_path: None,
            }],
        });
    app.insert_resource(MapContentRegistry {
        catalog,
        maps: [(
            map.clone(),
            MapContentHandles {
                manifest,
                placements,
                portals,
                characters_path: Some(private_path.clone()),
                surface: None,
                vegetation: None,
                water: Some(water),
                boundaries: None,
            },
        )]
        .into(),
        ..default()
    }); // 缺失角色Asset也不能阻塞普通地图；真正解释和错误恢复属于已安装游戏适配。
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = Some(map.clone());
    for _ in 0..2 {
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft = None;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.update();
            let session = app.world().resource::<EditorAuthoringSession>();
            if let Some(loaded) = session.draft.as_ref().filter(|loaded| loaded.map_id == map) {
                assert!(loaded.domain.is_none());
                assert_eq!(
                    loaded.preserved_asset_paths.as_ref(),
                    &[private_path.clone()]
                );
                assert!(!loaded.domain_dirty());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "普通地图不能等待缺失的原游戏文档：{}",
                app.world().resource::<MapEditorState>().status
            );
            std::thread::yield_now();
        }
    }
}

/// 构造只供身份门使用的最小地表文档；像素路径不会在本组纯测试中读取。
fn surface_fixture(map_id: MapId, schema_version: u32) -> MapSurfaceAsset {
    MapSurfaceAsset {
        schema_version,                     // 由测试分别传入当前或过期地表专用版本。
        map_id,                             // 文档所属地图必须与作者目标逐值一致。
        weight_source_path: String::new(),  // 身份门不读取权重作者源。
        weight_texture_path: String::new(), // 身份门不加载权重派生纹理。
        index_source_path: String::new(),   // 身份门不读取局部索引作者源。
        index_texture_path: String::new(),  // 身份门不加载局部索引派生纹理。
        resolution: [1, 1],                 // 最小合法形状足以表达夹具用途。
        layers: Vec::new(),                 // 层内容由后续正式加载与保存校验负责。
    }
}

#[test]
fn current_surface_schema_is_accepted_by_authoring_loader() {
    let map_id = MapId::parse("MAP-surface-v2").expect("测试MapId必须合法"); // 模拟已经真正进入的动态地图。
    let surface = surface_fixture(map_id.clone(), MAP_SURFACE_SCHEMA_VERSION); // 使用运行时当前正式地表版本。
    assert!(
        validate_surface_authoring_identity(&surface, &map_id).is_ok(), // F12必须与运行时接受同一份地表文档。
        "当前地表专用版本不得再被地图通用版本门静默拦截",
    );
}

#[test]
fn stale_surface_schema_is_rejected_with_a_version_diagnostic() {
    let map_id = MapId::parse("MAP-surface-stale").expect("测试MapId必须合法"); // 保持地图身份正确以隔离版本分支。
    let stale_version = MAP_SURFACE_SCHEMA_VERSION.saturating_sub(1); // 构造确定落后于当前正式地表协议的版本。
    let surface = surface_fixture(map_id.clone(), stale_version); // 过期文档不得进入作者IO任务。
    let error = validate_surface_authoring_identity(&surface, &map_id)
        .expect_err("过期地表版本必须被明确拒绝"); // 失败需携带可展示诊断而非无声等待。
    assert!(
        error.contains(&MAP_SURFACE_SCHEMA_VERSION.to_string()), // 诊断必须告诉作者当前要求的正式版本。
        "版本诊断应包含当前地表专用版本：{error}",
    );
}
