//! 独立根静水的新建、取消、空地图与旧私有水兼容回归。
use super::*;
use feathrise_map_content::{MapId, MapTerrainKind, MapWaterRiver, MapWaterfall, TerrainId};
use bevy::ecs::system::RunSystemOnce;

fn root_fixture() -> App {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([]));
    draft.manifest.world_min = [-0.5; 2];
    draft.manifest.world_max = [0.5; 2];
    draft.terrain_mesh = None;
    draft.heights = None;
    draft.terrain_ground = None;
    draft.surface = None;
    draft.vegetation = None;
    draft.boundaries = None;
    draft.water = Some(crate::authoring::default_water_asset(&draft.map_id));
    draft.water_asset_path = format!("{}/{}", draft.folder, feathrise_map_content::MAP_WATER_FILE_NAME);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(MapEditorState { open: true, selected_map_id: Some(draft.map_id.clone()),
            water_selected_preset_id: Some("windy_lake".into()), cursor_world: Some(Vec3::splat(900.0)), ..default() })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), revision: 9, ..default() })
        .init_resource::<PendingClipmapWaterLayerCreate>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>()
        .init_resource::<crate::authoring::terrain_documents::TerrainDocumentTask>();
    app
}

fn click_new(app: &mut App) {
    let button = app.world_mut().spawn((EditorButtonClick::triggered(), MapEditorActionButton(EditorAction::WaterAddLayer))).id();
    app.world_mut().run_system_once(handle_water_layer_buttons).unwrap();
    app.world_mut().despawn(button);
}
fn finish(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.world().resource::<PendingClipmapWaterLayerCreate>().blank.is_some() {
        app.world_mut().run_system_once(handle_water_layer_buttons).unwrap();
        assert!(std::time::Instant::now() < deadline, "真实空白R8任务应完成或后台退役");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// 保存往返测试共用真正的按钮/单槽R8事务，不手工伪造已创建的水层。
pub(crate) fn empty_root_created_via_button_for_tests() -> crate::authoring::EditorMapDraft {
    let mut app = root_fixture();
    click_new(&mut app);
    finish(&mut app);
    app.world_mut().remove_resource::<EditorAuthoringSession>().unwrap().draft.unwrap()
}

#[test]
fn empty_map_creates_one_root_water_without_mesh_ground_or_old_cursor_height() {
    let mut app = root_fixture();
    click_new(&mut app);
    assert!(app.world().resource::<PendingClipmapWaterLayerCreate>().blank.is_some());
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    app.world_mut().resource_mut::<MapEditorState>().water_selected_preset_id = Some("calm_lake".into());
    finish(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let water = draft.water.as_ref().unwrap();
    assert_eq!(water.layers.len(), 1);
    assert_eq!(water.layers[0].water_level, 0.0, "侧栏新建不借900米旧鼠标位置");
    assert_eq!(water.layers[0].preset_id, "windy_lake", "预设按点击时冻结");
    assert_eq!(water.layers[0].canvas_bounds, Some([[-128.0; 2], [128.0; 2]]));
    assert!(water.layers[0].mask_source_path.starts_with(&format!("{}/water/", draft.folder)));
    assert!(draft.water_masks[&water.layers[0].id].iter().all(|pixel| *pixel == 0));
    assert!(draft.terrain_documents.active.is_none() && draft.terrain_mesh.is_none() && draft.terrain_ground.is_none());
    assert_eq!(session.undo.len(), 1);
    assert_eq!(session.revision, 10);
}

#[test]
fn empty_preset_catalog_creates_inline_water_without_a_hidden_game_reference() {
    let mut app = root_fixture();
    let mut assets = crate::ui::status_bar::feedback_tests::asset_app();
    let mut registry = assets.world_mut().remove_resource::<feathrise_map_content::WaterPresetRegistry>().unwrap();
    registry.resolved = feathrise_map_content::ResolvedWaterPresetCatalog::build(
        &feathrise_map_content::WaterPresetCatalogAsset {
            schema_version: feathrise_map_content::WATER_PRESET_SCHEMA_VERSION,
            presets: Vec::new(),
        },
    ).unwrap();
    registry.revision = 1;
    registry.last_error = None;
    app.insert_resource(registry);
    app.world_mut().resource_mut::<MapEditorState>().water_selected_preset_id = None;
    click_new(&mut app);
    finish(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let water = draft.water.as_ref().unwrap();
    assert_eq!(water.layers.len(), 1);
    assert!(water.layers[0].preset_id.is_empty(), "空目录不得自动补原游戏calm_lake引用");
    assert!(water.validate(&draft.map_id).is_ok());
    // 保存所用正式格式必须原样保留内联身份，重开不能重新猜测目录首项。
    let reopened: MapWaterAsset = serde_json::from_slice(&serde_json::to_vec(water).unwrap()).unwrap();
    assert!(reopened.layers[0].preset_id.is_empty());
    assert_eq!(reopened.layers[0].base_color_srgba, water.layers[0].base_color_srgba);
    assert_eq!(session.undo.len(), 1);
}

#[test]
fn existing_terrain_missing_source_locked_or_hidden_does_not_block_root_water_creation() {
    for mode in 0..4 {
        let mut app = root_fixture();
        let mut object = crate::authoring::test_heightfield_draft(Arc::new(vec![0;121])).manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("unneeded-ground").unwrap();
        object.terrain.kind = MapTerrainKind::SparseClipmap;
        object.terrain.glb_path.clear();
        object.terrain.height_path = Some("maps/EDGE-TEST/terrains/unneeded-ground/missing.map_height_manifest.json".into());
        object.enabled = mode != 1;
        if mode == 2 { app.world_mut().resource_mut::<MapEditorState>().terrain_locked.insert(object.id.clone()); }
        if mode == 3 { Arc::make_mut(&mut app.world_mut().resource_mut::<MapEditorState>().terrain_hidden).insert(object.id.clone()); }
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().manifest.terrains = Some(Arc::from([object]));
        click_new(&mut app);
        finish(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert!(draft.terrain_documents.active.is_none() && !draft.terrain_documents.loading);
        assert_eq!(draft.water.as_ref().unwrap().layers.len(), 1, "场景{mode}不应触发地形读取");
        assert!(app.world().resource::<crate::authoring::terrain_documents::TerrainDocumentTask>().requested.is_none());
        assert!(app.world().resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>().selected.is_none());
    }
}

/// 活动私有水与停放根水共享真实作者槽，便于检查按钮拒绝前是否破坏原来源。
fn private_canvas_fixture() -> (App, TerrainId, String, Arc<Vec<u8>>) {
    let mut app = root_fixture();
    let private = TerrainId::legacy();
    let pixels = Arc::new(vec![41; 512 * 512]);
    let private_layer;
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let original = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.manifest = original.manifest;
        crate::authoring::terrain_documents::ensure_primary_canvas(draft);
        let object = draft.terrain_documents.active.as_ref().unwrap().definition.clone();
        draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
        draft.manifest.terrains = Some(Arc::from([(*object).clone()]));
        draft.terrain_documents.root_water = Some(crate::authoring::terrain_documents::RootWaterCanvas {
            water: draft.water.take(), water_asset_path: draft.water_asset_path.clone(), ..default()
        });
        draft.water = Some(crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &draft.folder));
        private_layer = draft.water.as_ref().unwrap().layers[0].id.clone();
        draft.water_asset_path = format!("{}/private.map_water.json", draft.folder);
        draft.water_masks.insert(private_layer.clone(), pixels.clone());
        draft.water_dirty_layers.insert(private_layer.clone());
    }
    app.world_mut().resource_mut::<MapEditorState>().terrain_canvas_target = Some(private.clone());
    app.world_mut().resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>().selected = Some(private.clone());
    app.world_mut().resource_mut::<crate::authoring::terrain_documents::TerrainDocumentTask>().requested = Some(private.clone());
    (app, private, private_layer, pixels)
}

#[test]
fn new_root_water_keeps_unfinished_or_unsaved_private_authors_before_any_source_change() {
    for reason in ["sparse-dirty", "terrain-stroke", "numeric", "documents-loading", "transforming"] {
        let (mut app, private, layer, pixels) = private_canvas_fixture();
        app.world_mut().resource_mut::<EditorAuthoringSession>().undo.push(EditorCommand::WaterLevelChanged {
            layer_id: layer.clone(), before: 0.0, after: 1.0,
        });
        match reason {
            "sparse-dirty" => app.world_mut().resource_mut::<EditorAuthoringSession>().sparse_terrain_dirty = true,
            "terrain-stroke" => { app.insert_resource(crate::authoring::TerrainBrushStroke { active: true, ..default() }); }
            "numeric" => {
                let entity = app.world_mut().spawn_empty().id();
                app.insert_resource(feathrise_editor_ui::ActiveNumericInput { entity: Some(entity) });
            }
            "documents-loading" => app.world_mut().resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>().documents_loading = true,
            "transforming" => app.world_mut().resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>().transforming = true,
            _ => unreachable!(),
        }
        click_new(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.terrain_documents.active.as_ref().unwrap().definition.id, private, "{reason}");
        assert!(draft.terrain_documents.values.is_empty() && draft.terrain_documents.root_water.is_some(), "{reason}");
        assert_eq!(draft.terrain_documents.scoped_through, 0, "拒绝前不能改动撤销归属：{reason}");
        assert_eq!(session.revision, 9);
        assert_eq!(session.sparse_terrain_dirty, reason == "sparse-dirty");
        assert!(Arc::ptr_eq(&draft.water_masks[&layer], &pixels));
        assert!(app.world().resource::<PendingClipmapWaterLayerCreate>().blank.is_none());
        assert_eq!(app.world().resource::<MapEditorState>().terrain_canvas_target.as_ref(), Some(&private));
        assert_eq!(app.world().resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>().selected.as_ref(), Some(&private));
        assert_eq!(app.world().resource::<crate::authoring::terrain_documents::TerrainDocumentTask>().requested.as_ref(), Some(&private));
    }
}

#[test]
fn new_root_water_waits_for_all_private_history_before_parking_the_canvas() {
    let (mut app, private, private_layer, pixels) = private_canvas_fixture();
    // 刻意超过两帧预算，确保最后一条不会在切源后才被包装成根水历史。
    for index in 0..129 {
        app.world_mut().resource_mut::<EditorAuthoringSession>().undo.push(EditorCommand::WaterLevelChanged {
            layer_id: private_layer.clone(), before: index as f32, after: index as f32 + 1.0,
        });
    }
    for expected in [64, 128] {
        click_new(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.terrain_documents.scoped_through, expected);
        assert_eq!(draft.terrain_documents.active.as_ref().unwrap().definition.id, private);
        assert!(draft.terrain_documents.values.is_empty());
        assert_eq!(session.revision, 9);
        assert!(Arc::ptr_eq(&draft.water_masks[&private_layer], &pixels));
        assert!(app.world().resource::<PendingClipmapWaterLayerCreate>().blank.is_none());
        assert_eq!(app.world().resource::<MapEditorState>().terrain_canvas_target.as_ref(), Some(&private));
        assert_eq!(app.world().resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>().selected.as_ref(), Some(&private));
        assert_eq!(app.world().resource::<crate::authoring::terrain_documents::TerrainDocumentTask>().requested.as_ref(), Some(&private));
    }
    click_new(&mut app);
    finish(&mut app);
    crate::authoring::terrain_documents::scope_history(&mut app.world_mut().resource_mut::<EditorAuthoringSession>());
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert!(draft.terrain_documents.active.is_none());
    assert_eq!(draft.water.as_ref().unwrap().layers.len(), 1);
    assert!(session.undo[..129].iter().all(|command| matches!(command,
        EditorCommand::TerrainCanvas { id, command } if id == &private && matches!(command.as_ref(), EditorCommand::WaterLevelChanged { .. }))));
    assert!(matches!(session.undo.last(), Some(EditorCommand::RootWater(_))));
    let parked = &draft.terrain_documents.values[&private];
    assert!(Arc::ptr_eq(&parked.water_masks[&private_layer], &pixels));
    assert!(parked.water_dirty_layers.contains(&private_layer));
}

#[test]
fn root_canvas_uses_real_manifest_bounds_and_default_only_for_placeholder() {
    let mut app = root_fixture();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.world_min = [-40.0, -20.0];
        draft.manifest.world_max = [60.0, 35.0];
    }
    click_new(&mut app);
    finish(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().water.as_ref().unwrap().layers[0].canvas_bounds,
        Some([[-40.0, -20.0], [60.0, 35.0]]));
}

#[test]
fn legacy_single_map_still_creates_in_its_existing_document_without_a_ground_hit() {
    let mut app = root_fixture();
    let original = crate::authoring::test_heightfield_draft(Arc::new(vec![0;121]));
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest = original.manifest;
        draft.water_asset_path = "maps/EDGE-TEST/original-water.json".into();
    }
    click_new(&mut app);
    finish(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.water_asset_path, "maps/EDGE-TEST/original-water.json");
    assert!(draft.manifest.terrains.is_none() && !draft.manifest.terrain.is_empty());
    assert_eq!(draft.water.as_ref().unwrap().layers.len(), 1);
    assert!(draft.terrain_ground.is_none());
}

#[test]
fn blank_root_task_rejects_source_author_and_selection_changes_without_reviving() {
    for case in 0..7 {
        let mut app = root_fixture();
        click_new(&mut app);
        let task = create::pause_for_tests(&mut app.world_mut().resource_mut::<PendingClipmapWaterLayerCreate>());
        let original_path = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().water_asset_path.clone();
        match case {
            0 => app.world_mut().resource_mut::<MapEditorState>().save_requested = true,
            1 => app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::Inspect,
            2 => app.world_mut().resource_mut::<MapEditorState>().selected_water_layer = 1,
            3 => app.world_mut().resource_mut::<MapEditorState>().map_transition_in_progress = true,
            4 => app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1,
            5 => app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().water_asset_path = "maps/EDGE-TEST/changed.water.json".into(),
            6 => app.world_mut().resource_mut::<MapEditorState>().terrain_canvas_target = Some(TerrainId::parse("sibling").unwrap()),
            _ => unreachable!(),
        }
        app.world_mut().run_system_once(observe_water_creation_cancellation).unwrap();
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.save_requested = false; editor.tool = EditorTool::Water;
            editor.selected_water_layer = 0; editor.map_transition_in_progress = false;
            editor.terrain_canvas_target = None;
        }
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.revision = 9; session.draft.as_mut().unwrap().water_asset_path = original_path;
        }
        create::resume_for_tests(&mut app.world_mut().resource_mut::<PendingClipmapWaterLayerCreate>(), task);
        finish(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert!(draft.water.as_ref().unwrap().layers.is_empty() && draft.water_masks.is_empty(), "已取消任务不能复活，场景{case}");
        assert!(session.undo.is_empty());
    }
}

#[derive(Resource)]
struct WaterGeneralInputForTests(bool);

#[test]
fn input_and_numeric_guards_record_cancellation_without_polling_or_freeing_the_single_slot() {
    for numeric in [true, false] {
        let mut app = root_fixture();
        click_new(&mut app);
        let task = create::pause_for_tests(&mut app.world_mut().resource_mut::<PendingClipmapWaterLayerCreate>());
        app.insert_resource(WaterGeneralInputForTests(numeric))
            .add_systems(Update, handle_water_layer_buttons.run_if(|gate: Res<WaterGeneralInputForTests>| gate.0)
                .in_set(crate::MapEditorSystemSet::Input))
            .add_systems(Update, observe_water_creation_cancellation.after(crate::MapEditorSystemSet::Input));
        if numeric { app.world_mut().resource_mut::<MapEditorNumericScrubMouseGuard>().advance(true, true, false); }
        app.world_mut().resource_mut::<MapEditorState>().save_requested = true;
        app.update();
        assert!(app.world().resource::<PendingClipmapWaterLayerCreate>().blank.is_some());
        app.world_mut().resource_mut::<MapEditorState>().save_requested = false;
        app.world_mut().resource_mut::<MapEditorNumericScrubMouseGuard>().advance(false, false, false);
        app.world_mut().resource_mut::<WaterGeneralInputForTests>().0 = true;
        create::resume_for_tests(&mut app.world_mut().resource_mut::<PendingClipmapWaterLayerCreate>(), task);
        finish(&mut app);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().water.as_ref().unwrap().layers.is_empty());
    }
}

#[test]
fn delete_selected_enters_the_layer_transaction_only_for_still_water() {
    assert!(water_structure_action(EditorAction::WaterDeleteSelected, EditorTool::Water));
    assert!(!water_structure_action(EditorAction::WaterDeleteSelected, EditorTool::RiverSpline));
    assert!(!water_structure_action(EditorAction::WaterDeleteSelected, EditorTool::Waterfall));
}

#[test]
fn duplicate_water_layer_shares_pixels_and_all_layers_can_be_deleted() {
    let map_id = MapId::parse("MAP-water-test").expect("测试地图ID有效");
    let mut water = crate::authoring::default_water_asset(&map_id);
    let mut masks = BTreeMap::new();
    add_water_layer(0, None, None, "maps/MAP-water-test", &mut water, &mut masks)
        .expect("零层水域应能显式新建首层");
    let base = Arc::new(vec![64; 512 * 512]);
    masks.insert(water.layers[0].id.clone(), base.clone());
    let (index, _) = duplicate_water_layer(0, "maps/MAP-water-test", &mut water, &mut masks)
        .expect("复制水层应成功");
    assert!(Arc::ptr_eq(&base, &masks[&water.layers[index].id]));
    delete_water_layer(index, &mut water, &mut masks).expect("两层时可删除副本");
    let (selected, _) =
        delete_water_layer(0, &mut water, &mut masks).expect("最后一层也应允许删除");
    assert_eq!(selected, 0); // 空列表沿用无对象的零选择哨兵，不发生usize下溢。
    assert!(water.layers.is_empty());
    assert!(masks.is_empty());
    assert!(delete_water_layer(0, &mut water, &mut masks).is_err()); // 已经为空时才拒绝重复删除。
}

#[test]
fn deleting_the_last_layer_disconnects_rivers_and_waterfalls() {
    let map_id = MapId::parse("MAP-water-links").expect("测试地图ID有效");
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&map_id, "maps/MAP-water-links");
    let layer_id = water.layers[0].id.clone();
    water.rivers.push(MapWaterRiver {
        id: "river".to_owned(),
        label: "引用静水的河流".to_owned(),
        source_layer_id: Some(layer_id.clone()),
        target_layer_id: Some(layer_id.clone()),
        points: Vec::new(),
        base_color_srgba: [0.055, 0.24, 0.29, 0.72],
        perceptual_roughness: 0.22,
    });
    water.waterfalls.push(MapWaterfall {
        id: "waterfall".to_owned(),
        label: "引用静水的瀑布".to_owned(),
        source: Some(MapWaterEndpoint::StillWater {
            layer_id: layer_id.clone(),
        }),
        target: Some(MapWaterEndpoint::StillWater {
            layer_id: layer_id.clone(),
        }),
        points: Vec::new(),
        foam_strength: 0.65,
        base_color_srgba: [0.055, 0.24, 0.29, 0.72],
        perceptual_roughness: 0.22,
    });
    let mut masks = BTreeMap::from([(layer_id, Arc::new(vec![0; 512 * 512]))]);

    delete_water_layer(0, &mut water, &mut masks).expect("带连接的末层应原子删除");

    assert!(water.layers.is_empty());
    assert_eq!(water.rivers[0].source_layer_id, None);
    assert_eq!(water.rivers[0].target_layer_id, None);
    assert_eq!(water.waterfalls[0].source, None);
    assert_eq!(water.waterfalls[0].target, None);
}
