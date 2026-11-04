//! 对象事务真实单飞采用、保存请求收口和元数据历史回归，不创建项目资产。
#[path="stitch_tests.rs"]
mod stitch_tests;
use super::*;
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand},
    resources::MapEditorState,
};
use bevy::prelude::*;
use feathrise_map_content::*;
fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("terrain-a").unwrap();
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(vec![object].into());
    draft.terrain_mesh = None;
    draft.heights = None;
    draft.terrain_editing_index = None;
    let map = draft.map_id.clone();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(map),
        ..default()
    })
    .init_resource::<TerrainObjectsEditorState>()
    .init_resource::<crate::viewport::LayerCandidateState>()
    .init_resource::<feathrise_scene::TerrainObjectTransformPreview>()
    .add_systems(Update, systems::poll);
    app
}
fn select_first(app: &mut App) {
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .map_id
        .clone();
    let id = TerrainId::parse("terrain-a").unwrap();
    let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
    state.map = Some(map);
    state.selected = Some(id);
    state.selected_index = Some(0);
    app.world_mut().resource_mut::<crate::viewport::LayerCandidateState>()
        .confirm_terrain(&TerrainId::parse("terrain-a").unwrap(), 0);
}

#[test]
fn scene_context_menu_terrain_uses_original_name_delete_and_session_protection_paths() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::ui::scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectRequest, SceneObjectTarget};
    for action in [SceneMenuAction::Rename, SceneMenuAction::Delete, SceneMenuAction::Visibility, SceneMenuAction::Lock] {
        let mut app = app(); select_first(&mut app);
        app.init_resource::<feathrise_scene::TerrainObjectEditorVisibility>();
        let session = app.world().resource::<EditorAuthoringSession>();
        let object = &session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[0];
        let id = object.id.clone();
        let mut menu = SceneContextMenuState::default();
        menu.ready = Some(SceneObjectRequest { target: SceneObjectTarget { map: session.draft.as_ref().unwrap().map_id.clone(),
            revision: session.revision, key: SceneObjectKey::Terrain { index: 0, id: id.clone() },
            name: object.display_name.clone(), hidden: false, locked: false, copyable: true }, action, name: "山门地面".into() });
        app.insert_resource(menu);
        app.world_mut().run_system_once(super::scene_menu::input).unwrap();
        app.world_mut().resource_mut::<SceneContextMenuState>().ready = None;
        complete(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        match action {
            SceneMenuAction::Rename => {
                assert_eq!(session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[0].display_name, "山门地面");
                assert_eq!(session.undo.len(), 1);
            }
            SceneMenuAction::Delete => {
                assert!(session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap().is_empty());
                assert_eq!(session.undo.len(), 1);
            }
            SceneMenuAction::Visibility => {
                assert!(app.world().resource::<TerrainObjectsEditorState>().hidden.contains(&id));
                assert!(app.world().resource::<MapEditorState>().terrain_hidden.contains(&id));
                assert!(app.world().resource::<feathrise_scene::TerrainObjectEditorVisibility>().hidden.contains(&id));
                assert!(session.undo.is_empty());
            }
            SceneMenuAction::Lock => {
                assert!(app.world().resource::<TerrainObjectsEditorState>().locked.contains(&id));
                assert!(app.world().resource::<MapEditorState>().terrain_locked.contains(&id));
                assert!(session.undo.is_empty());
            }
            _ => unreachable!(),
        }
    }
}
fn begin(app: &mut App, mutation: Mutation) {
    app.world_mut()
        .resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
            world.resource_scope(|world, mut state: Mut<TerrainObjectsEditorState>| {
                let mut editor = world.resource_mut::<MapEditorState>();
                systems::start(&mut session, &mut editor, &mut state, mutation);
            });
        });
}
fn complete(app: &mut App) {
    let started = std::time::Instant::now();
    while app
        .world()
        .resource::<TerrainObjectsEditorState>()
        .preparing()
    {
        app.update();
        assert!(started.elapsed().as_secs() < 10, "对象后台事务没有完成");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn non_divisible_clipmap_creation_in_empty_container_adopts_the_full_background_transaction() {
    let temp = std::env::temp_dir().canonicalize().unwrap();
    let root = temp.join(format!("bevy-clipmap-creation-test-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    assert_eq!(root.canonicalize().unwrap().parent(), Some(temp.as_path()));
    let mut app = app();
    let creation = [0.0, 0.0, 0.0, 97.1, 92.69, 95.4, 0.0, 0.125];
    let (owner, before, revision) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrains = Some(Vec::new().into());
        draft.manifest.world_min = [-0.5; 2];
        draft.manifest.world_max = [0.5; 2];
        draft.terrain_documents = Default::default();
        (draft.manifest.clone(), MapTerrainCollection::capture(&draft.manifest), session.revision)
    };
    let map = owner.id.clone();
    let objects = owner.terrain_objects_for_edit();
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.map = Some(map.clone());
        state.creation = creation;
        state.creation_open = true;
        // 入口预检由creation_tests覆盖；这里执行实际生产准备器，只有源/画布全部成功才交给原poll采用。
        state.pending = Some(types::Pending {
            preview_stitch: false, stitching: false, stroke: None, target: None,
            affects_terrain: true, lod_navigation: None, map: map.clone(), revision, before,
            task: bevy::tasks::IoTaskPool::get().spawn(jobs::prepare_at(
                root.clone(), owner, objects, Mutation::Add(MapTerrainKind::SparseClipmap), creation, 1,
            )),
        });
    }
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
        .manifest.terrains.as_ref().unwrap().is_empty(), "后台准备前不得提前采用半成品对象");
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let objects = draft.manifest.terrains.as_ref().unwrap();
    assert_eq!(objects.len(), 1, "{}", app.world().resource::<MapEditorState>().status);
    let object = &objects[0];
    assert_eq!(object.terrain.kind, MapTerrainKind::SparseClipmap);
    assert_eq!(object.terrain.resolution, [778, 765]);
    assert_eq!(object.world_min, [-creation[3] * 0.5, -creation[5] * 0.5]);
    assert_eq!(object.world_max, [creation[3] * 0.5, creation[5] * 0.5]);
    assert_eq!(object.terrain.height_scale, creation[4]);
    validate_terrain_objects(&draft.manifest).unwrap();
    let document = SparseHeightDocument::load_for_terrain(&root, &map, Some(&object.id)).unwrap();
    let height = document.manifest();
    assert_eq!(height.spec.requested_spacing_meters, 0.125, "作者文件必须保留原请求，不能写回均分实际格距");
    assert_eq!(height.spec.cells, [777, 764]);
    assert!(height.spec.actual_spacing_meters[0] < 0.125);
    assert_eq!(height.spec.world_min, object.world_min);
    assert_eq!(height.spec.world_max, object.world_max);
    assert!(height.tiles.is_empty() && height.mip_tiles.is_empty(), "新网格保持隐式零高，不生成整图作者块");
    compile_display_lod_plan(&height.spec, object.terrain.display_lods.as_ref().unwrap()).unwrap();
    feathrise_scene::validate_layered_collision_settings(&height.spec, &object.terrain.collision_lods).unwrap();
    for path in [object.terrain.height_path.as_ref().unwrap(), object.surface_path.as_ref().unwrap(),
        object.vegetation_path.as_ref().unwrap(), object.water_path.as_ref().unwrap()] {
        let target = root.join(path);
        assert!(target.is_file());
        assert!(target.canonicalize().unwrap().starts_with(&root));
    }
    let surface: MapSurfaceAsset = serde_json::from_slice(&std::fs::read(root.join(
        object.surface_path.as_ref().unwrap())).unwrap()).unwrap();
    for path in [&surface.index_source_path, &surface.weight_source_path,
        &surface.index_texture_path, &surface.weight_texture_path] {
        assert!(root.join(path).is_file(), "对象采用前必须完成配套地表作者源与纹理：{path}");
    }
    let vegetation: MapVegetationAsset = serde_json::from_slice(&std::fs::read(root.join(
        object.vegetation_path.as_ref().unwrap())).unwrap()).unwrap();
    assert_eq!(vegetation.map_id, map);
    assert!(vegetation.layers.is_empty());
    assert!(vegetation.authoring.as_ref().unwrap().layers.is_empty());
    assert_eq!(session.undo.len(), 1);
    assert!(session.redo.is_empty());
    assert_eq!(session.revision, revision + 1);
    assert_eq!(session.terrain_revision, 1);
    let EditorCommand::TerrainObjectsChanged { before, after } = &session.undo[0] else {
        panic!("后台创建必须生成一次完整集合历史");
    };
    assert!(before.objects.as_ref().unwrap().is_empty());
    assert_eq!(after.objects.as_deref(), Some(objects.as_ref()));
    let state = app.world().resource::<TerrainObjectsEditorState>();
    assert_eq!(state.selected.as_ref(), Some(&object.id));
    assert_eq!(state.selected_index, Some(0));
    assert!(!state.creation_open && !state.preparing());
    assert!(app.world().resource::<MapEditorState>().status.contains("网格已采用"));
    drop(document);
    drop(app);
    assert_eq!(root.canonicalize().unwrap().parent(), Some(temp.as_path()));
    std::fs::remove_dir_all(root).unwrap(); // 只清理已核对位于临时目录内的本测试独占资产根。
}

fn native_lod_app(voxel: bool) -> App {
    let mut app = app();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let object = &mut std::sync::Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0];
        let folder = object.private_folder(&draft.map_id);
        object.world_min = [-128.0; 2]; object.world_max = [128.0; 2];
        object.terrain.kind = if voxel { MapTerrainKind::Voxel } else { MapTerrainKind::SparseClipmap };
        object.terrain.glb_path.clear(); object.terrain.resolution = [1025; 2];
        object.terrain.height_path = (!voxel).then(|| format!("{folder}/height_tiles.map_height_manifest.json"));
        object.terrain.display_lods = Some((0..6).map(|index| MapTerrainDisplayLod {
            outer_half_extent_meters: (index < 5).then(|| 4.0 * 2.0_f32.powi(index)),
            spacing_multiplier: 1 << index,
        }).collect());
        object.terrain.collision_lods = (0..6).map(|index| MapCollisionLod {
            max_distance_meters: 3.0 * 2.0_f32.powi(index),
            // 1025格高度源的已存父Mip最高4倍；后续累计距离可继续增大，但不能伪造不存在的高度层。
            spacing_multiplier: 1 << if voxel { index } else { index.min(2) },
        }).collect();
        object.voxel = voxel.then(|| MapVoxelTerrainDescriptor {
            cell_size_meters: 0.25, min: [-128.0, -8.0, -128.0], max: [128.0, 8.0, 128.0],
            source_path: format!("{folder}/source.map_voxel_source.json"),
            display_lods: (0..6).map(|index| MapCollisionLod { max_distance_meters: 4.0 * 2.0_f32.powi(index), spacing_multiplier: 1 << index }).collect(),
            collision_lods: object.terrain.collision_lods.clone(),
        });
        if voxel {
            // 体素两个用途只存在独立volume协议中，不能夹带Clipmap显示/碰撞配置。
            object.terrain.display_lods = None;
            object.terrain.collision_lods.clear();
        }
        validate_terrain_objects(&draft.manifest).expect("原生LOD夹具必须满足正式作者校验");
        if !voxel {
            let object = &draft.manifest.terrains.as_ref().unwrap()[0];
            let spec = SparseHeightSpec::new(object.world_min, object.world_max, 0.25, object.terrain.base_height, object.terrain.height_scale).unwrap();
            compile_display_lod_plan(&spec, object.terrain.display_lods.as_ref().unwrap()).unwrap();
            feathrise_scene::validate_layered_collision_settings(&spec, &object.terrain.collision_lods).unwrap();
        }
    }
    select_first(&mut app);
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(components::MeshPropertySection::DisplayLods, true);
    app
}

fn native_object(app: &App) -> &MapTerrainObject {
    &app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[0]
}

fn native_rows(object: &MapTerrainObject, display: bool) -> Vec<(Option<f32>, u32)> {
    if let Some(voxel) = &object.voxel {
        let rows = if display { &voxel.display_lods } else { &voxel.collision_lods };
        rows.iter().map(|row| (Some(row.max_distance_meters), row.spacing_multiplier)).collect()
    } else if display {
        object.terrain.display_lods.as_ref().unwrap().iter().map(|row| (row.outer_half_extent_meters, row.spacing_multiplier)).collect()
    } else {
        object.terrain.collision_lods.iter().map(|row| (Some(row.max_distance_meters), row.spacing_multiplier)).collect()
    }
}

#[test]
fn native_lod_transactions_remove_the_frozen_first_middle_or_last_layer_and_preserve_the_other_usage() {
    for voxel in [false, true] { for display in [false, true] { for removed in [0, 2, 5] {
        let mut app = native_lod_app(voxel);
        let before = native_object(&app).clone();
        let original_rows = native_rows(&before, display);
        {
            let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
            state.display_lods = display; state.lod_mut(display).selected = removed;
        }
        begin(&mut app, Mutation::RemoveLod(before.id.clone(), display, removed));
        // 目标下标已经冻结，外部焦点变化也不能把已确认删除改解释为当前行或末行。
        app.world_mut().resource_mut::<TerrainObjectsEditorState>().lod_mut(display).selected = 4;
        complete(&mut app);
        let after = native_object(&app);
        let rows = native_rows(after, display);
        assert_eq!(rows.len(), 5);
        for (local, original) in (0..6).filter(|index| *index != removed).enumerate() {
            assert_eq!(rows[local].1, original_rows[original].1);
            if !voxel && display && local == 4 { assert_eq!(rows[local].0, None); }
            else { assert_eq!(rows[local].0, original_rows[original].0); }
        }
        assert_eq!(native_rows(after, !display), native_rows(&before, !display));
        assert_eq!(after.transform, before.transform); assert_eq!(after.world_min, before.world_min);
        assert_eq!(after.world_max, before.world_max); assert_eq!(after.terrain.height_path, before.terrain.height_path);
        assert_eq!(after.voxel.as_ref().map(|voxel| &voxel.source_path), before.voxel.as_ref().map(|voxel| &voxel.source_path));
        let state = app.world().resource::<TerrainObjectsEditorState>();
        assert_eq!(state.lod(display).selected, removed.min(4));
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        assert!(app.world().resource::<MapEditorState>().status.contains(&format!("已删除{}LOD{}", if display { "显示" } else { "碰撞" }, removed)));
    } } }
}

#[test]
fn successful_native_lod_add_selects_the_new_layer_without_changing_the_other_usage() {
    for voxel in [false, true] { for display in [false, true] {
        let mut app = native_lod_app(voxel);
        let before = native_object(&app).clone();
        {
            let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
            state.display_lods = display; state.lod_mut(display).selected = 1;
            state.lod_mut(!display).selected = 3;
            state.lod_mut(!display).start = 1;
        }
        begin(&mut app, Mutation::AddLod(before.id.clone(), display)); complete(&mut app);
        assert_eq!(native_rows(native_object(&app), display).len(), 7);
        assert_eq!(native_rows(native_object(&app), !display), native_rows(&before, !display));
        let state = app.world().resource::<TerrainObjectsEditorState>();
        assert_eq!(state.lod(display).selected, 6); assert_eq!(state.display_lods, display);
        assert_eq!(state.lod(!display).selected, 3);
        assert_eq!(state.lod(!display).start, 1);
        assert!(app.world().resource::<MapEditorState>().status.contains(&format!("已添加{}LOD6", if display { "显示" } else { "碰撞" })));
    } }
}

#[test]
fn invalid_or_superseded_native_lod_transactions_keep_the_original_selection_and_list() {
    for superseded in [false, true] {
        let mut app = native_lod_app(false); let before = native_object(&app).clone();
        {
            let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
            state.display_lods = true; state.lod_mut(true).selected = 2;
        }
        let mutation = if superseded { Mutation::AddLod(before.id.clone(), true) } else { Mutation::RemoveLod(before.id.clone(), true, 99) };
        begin(&mut app, mutation);
        if superseded { app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1; }
        complete(&mut app);
        assert_eq!(native_object(&app), &before);
        let state = app.world().resource::<TerrainObjectsEditorState>();
        assert_eq!(state.lod(true).selected, 2); assert!(state.display_lods);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(app.world().resource::<MapEditorState>().status.contains(if superseded { "过期结果未采用" } else { "操作失败" }));
    }
}

#[test]
fn deleting_the_only_native_lod_preserves_the_channel_and_does_not_navigate_or_write_history() {
    for voxel in [false, true] { for display in [false, true] {
        let mut app = native_lod_app(voxel);
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let object = &mut std::sync::Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0];
            if let Some(voxel) = &mut object.voxel {
                let rows = if display { &mut voxel.display_lods } else { &mut voxel.collision_lods }; rows.truncate(1);
            } else if display {
                object.terrain.display_lods = Some(vec![object.terrain.display_lods.as_ref().unwrap().last().unwrap().clone()]);
            } else { object.terrain.collision_lods.truncate(1); }
        }
        let before = native_object(&app).clone();
        app.world_mut().resource_mut::<TerrainObjectsEditorState>().display_lods = display;
        begin(&mut app, Mutation::RemoveLod(before.id.clone(), display, 0)); complete(&mut app);
        assert_eq!(native_object(&app), &before);
        let state = app.world().resource::<TerrainObjectsEditorState>();
        assert_eq!(state.lod(display).selected, 0);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(app.world().resource::<MapEditorState>().status.contains("至少保留一层"));
    } }
}

#[test]
fn legacy_source_rebuild_waits_for_saved_geometry_but_metadata_edits_remain_available() {
    for sparse_dirty in [false, true] {
        let mut app = app();
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        draft.terrain_dirty = !sparse_dirty;
        if sparse_dirty {
            draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
            draft.manifest.terrain.glb_path.clear();
            draft.manifest.terrain.height_path =
                Some(sparse_height_manifest_asset_path(&draft.map_id));
        }
        let legacy = draft.manifest.terrain_objects_for_edit()[0].clone();
        let map = draft.map_id.clone();
        app.world_mut().insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            sparse_terrain_dirty: sparse_dirty,
            ..default()
        });
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .selected_map_id = Some(map.clone());
        {
            let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
            state.map = Some(map);
            state.selected = Some(legacy.id.clone());
            state.selected_index = Some(0);
            state.legacy = Some(std::sync::Arc::new(legacy));
        }
        begin(
            &mut app,
            Mutation::Value(TerrainId::legacy(), 7, 0.25, true),
        );
        assert!(
            !app.world()
                .resource::<TerrainObjectsEditorState>()
                .preparing(),
            "源格距重采样必须在任何IO任务和票据产生前拒绝WIP"
        );
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().next_serial,
            0
        );
        assert!(
            app.world()
                .resource::<MapEditorState>()
                .status
                .contains("先保存")
        );
        for mutation in [
            Mutation::Rename(TerrainId::legacy(), "可改名的未保存地形".into()),
            Mutation::Transform(
                TerrainId::legacy(),
                TerrainObjectTransform {
                    translation: [3., 0., 0.],
                    ..default()
                },
            ),
            Mutation::Value(TerrainId::legacy(), 0, 5., true),
            Mutation::Value(TerrainId::legacy(), 8, 32., false),
        ] {
            begin(&mut app, mutation);
            assert!(
                app.world()
                    .resource::<TerrainObjectsEditorState>()
                    .preparing(),
                "主源WIP不能阻止名称、姿态或LOD的元数据准备"
            );
            complete(&mut app);
        }
    }
}

#[test]
fn first_legacy_collection_adoption_preserves_the_dirty_primary_canvas_before_emptying_container() {
    use std::sync::Arc;
    let mut app = app();
    let pixels = Arc::new(vec![149u8; 16]);
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
    draft.manifest.terrain.glb_path.clear();
    draft.manifest.terrain.height_path = Some(sparse_height_manifest_asset_path(&draft.map_id));
    draft.puddle_mask = Some(pixels.clone());
    draft.puddles_dirty = true;
    let old_manifest = draft.manifest.clone();
    let mut primary = old_manifest.terrain_objects_for_edit()[0].clone();
    primary.display_name = "未保存的原主地形".into();
    let map = draft.map_id.clone();
    let before = MapTerrainCollection::capture(&old_manifest);
    app.world_mut().insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        sparse_terrain_dirty: true,
        ..default()
    });
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = Some(map.clone());
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .pending = Some(types::Pending {
        stitching:false,
        preview_stitch:false,
        lod_navigation: None,
        stroke: None,
        target: Some(TerrainId::legacy()),
        affects_terrain: false,
        map,
        revision: 0,
        before,
        task: bevy::tasks::IoTaskPool::get().spawn(async move {
            Ok(types::Prepared {
                stitch_bytes:0,
                changed: true,
                objects: Arc::from([primary]),
                selected: Some(TerrainId::legacy()),
                selected_index: Some(0),
            })
        }),
    });
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert!(draft.manifest.terrain.is_empty());
    assert!(session.sparse_terrain_dirty);
    let active = draft
        .terrain_documents
        .active
        .as_ref()
        .expect("必须在旧清单清空前建立原主槽");
    assert_eq!(active.definition.id, TerrainId::legacy());
    assert_eq!(active.manifest.terrain, old_manifest.terrain);
    assert!(Arc::ptr_eq(draft.puddle_mask.as_ref().unwrap(), &pixels));
    assert!(draft.puddles_dirty);
    assert_eq!(draft.terrain_documents.scoped_through, 0);
}
#[test]
fn transform_adopts_before_a_pending_save_and_history_restores_the_complete_collection() {
    let mut app = app();
    let id = TerrainId::parse("terrain-a").unwrap();
    let pose = TerrainObjectTransform {
        translation: [8., 4., -2.],
        yaw_degrees: 90.,
        scale: [2., 3., 1.],
        ..Default::default()
    };
    begin(&mut app, Mutation::Transform(id.clone(), pose));
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .save_requested = true;
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert_eq!(session.terrain_revision, 1);
    let draft = session.draft.as_ref().unwrap();
    assert!(!draft.terrain_dirty);
    assert_eq!(draft.manifest.terrains.as_ref().unwrap()[0].transform, pose);
    let EditorCommand::TerrainObjectsChanged { before, after } = &session.undo[0] else {
        panic!("应生成对象集合历史")
    };
    let mut restored = draft.manifest.clone();
    before.apply(&mut restored);
    assert_eq!(
        restored.terrains.as_ref().unwrap()[0].transform,
        Default::default()
    );
    after.apply(&mut restored);
    assert_eq!(restored.terrains.as_ref().unwrap()[0].transform, pose);
}
#[test]
fn identical_parameters_make_no_history_and_a_late_result_cannot_change_the_new_revision() {
    let mut app = app();
    let id = TerrainId::parse("terrain-a").unwrap();
    begin(&mut app, Mutation::Value(id.clone(), 0, 0., true));
    complete(&mut app);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    begin(
        &mut app,
        Mutation::Transform(
            id,
            TerrainObjectTransform {
                translation: [8., 0., 0.],
                ..default()
            },
        ),
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    complete(&mut app);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .terrains
            .as_ref()
            .unwrap()[0]
            .transform
            .translation,
        [0.; 3]
    );
}

#[test]
fn chinese_rename_preserves_stable_id_source_and_geometry_revision_and_is_undoable() {
    let mut app = app();
    select_first(&mut app);
    let id = TerrainId::parse("terrain-a").unwrap();
    let old = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .terrains
        .as_ref()
        .unwrap()[0]
        .clone();
    begin(
        &mut app,
        Mutation::Rename(id.clone(), "  临水高台  ".into()),
    );
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let renamed = &draft.manifest.terrains.as_ref().unwrap()[0];
    assert_eq!(renamed.id, id);
    assert_eq!(renamed.display_name, "临水高台");
    assert_eq!(renamed.transform, old.transform);
    assert_eq!(renamed.terrain, old.terrain);
    assert_eq!(session.terrain_revision, 0);
    let EditorCommand::TerrainObjectsChanged { before, after } = &session.undo[0] else {
        panic!("改名应进入对象集合历史");
    };
    let mut restored = draft.manifest.clone();
    before.apply(&mut restored);
    assert_eq!(
        restored.terrains.as_ref().unwrap()[0].display_name,
        old.display_name
    );
    after.apply(&mut restored);
    assert_eq!(
        restored.terrains.as_ref().unwrap()[0].display_name,
        "临水高台"
    );
}

#[test]
fn locked_object_rejects_all_property_transactions_and_late_results() {
    let mut app = app();
    select_first(&mut app);
    let id = TerrainId::parse("terrain-a").unwrap();
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .locked
        .insert(id.clone());
    for mutation in [
        Mutation::Rename(id.clone(), "锁定对象".into()),
        Mutation::Enabled(id.clone(), false),
        Mutation::Value(id.clone(), 0, 18.0, true),
        Mutation::Transform(
            id.clone(),
            TerrainObjectTransform {
                translation: [8.0, 0.0, 0.0],
                ..default()
            },
        ),
        Mutation::Remove(id.clone()),
        Mutation::AddLod(id.clone(), true),
        Mutation::RemoveLod(id.clone(), true, 0),
    ] {
        begin(&mut app, mutation);
        assert!(
            !app.world()
                .resource::<TerrainObjectsEditorState>()
                .preparing()
        );
    }
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .locked
        .clear();
    begin(
        &mut app,
        Mutation::Transform(
            id.clone(),
            TerrainObjectTransform {
                translation: [8.0, 0.0, 0.0],
                ..default()
            },
        ),
    );
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .locked
        .insert(id);
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert_eq!(session.terrain_revision, 0);
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .terrains
            .as_ref()
            .unwrap()[0]
            .transform
            .translation,
        [0.0; 3]
    );
}

#[test]
fn editor_visibility_changes_only_session_state_and_keeps_persistent_participation() {
    let mut app = app();
    select_first(&mut app);
    app.init_resource::<feathrise_scene::TerrainObjectEditorVisibility>()
        .add_systems(Update, systems::input);
    let id = TerrainId::parse("terrain-a").unwrap();
    app.world_mut().spawn((
        feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed,
        components::ActionButton(components::Action::Visible),
    ));
    app.update();
    assert!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .hidden
            .contains(&id)
    );
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .terrain_hidden
            .contains(&id)
    );
    assert!(
        app.world()
            .resource::<feathrise_scene::TerrainObjectEditorVisibility>()
            .hidden
            .contains(&id)
    );
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert_eq!(session.terrain_revision, 0);
    assert!(
        session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .terrains
            .as_ref()
            .unwrap()[0]
            .enabled
    );
}

#[test]
fn closing_f12_restores_game_visibility_and_reopening_restores_the_editor_hidden_set() {
    use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub, NumericInputSetValue};
    let mut app = app();
    select_first(&mut app);
    app.init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<feathrise_scene::TerrainObjectEditorVisibility>()
        .add_message::<NumericInputSetValue>()
        .add_systems(Update, systems::sync);
    let id = TerrainId::parse("terrain-a").unwrap();
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .hidden
        .insert(id.clone());
    app.world_mut()
        .resource_mut::<feathrise_scene::TerrainObjectEditorVisibility>()
        .hidden
        .insert(id.clone());
    app.update();
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .map_id
        .clone();
    assert_eq!(
        app.world()
            .resource::<feathrise_scene::TerrainObjectEditorVisibility>()
            .map
            .as_ref(),
        Some(&map)
    );
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    let visibility = app
        .world()
        .resource::<feathrise_scene::TerrainObjectEditorVisibility>();
    assert!(visibility.map.is_none());
    assert!(visibility.hidden.contains(&id));
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    app.update();
    let visibility = app
        .world()
        .resource::<feathrise_scene::TerrainObjectEditorVisibility>();
    assert_eq!(visibility.map.as_ref(), Some(&map));
    assert!(visibility.hidden.contains(&id));
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn locked_numeric_fields_are_disabled_and_name_binding_cannot_commit_to_another_map() {
    use feathrise_editor_ui::{
        ActiveNumericInput, ActiveNumericInputScrub, NumericInputDisabled, NumericInputSetValue,
        NumericInputValue, NumericValue,
    };
    let mut app = app();
    select_first(&mut app);
    app.init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<feathrise_scene::TerrainObjectEditorVisibility>()
        .add_message::<NumericInputSetValue>()
        .init_resource::<resources::NameInputBinding>()
        .add_systems(Update, (systems::sync, names::input).chain());
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .locked
        .insert(TerrainId::parse("terrain-a").unwrap());
    let field = app
        .world_mut()
        .spawn((
            components::Field::Selected(0),
            NumericInputValue(NumericValue::F32(0.0)),
        ))
        .id();
    app.update();
    assert!(app.world().get::<NumericInputDisabled>(field).is_some());
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .locked
        .clear();
    app.world_mut()
        .resource_mut::<resources::NameInputBinding>()
        .target = Some((
        MapId::parse("OTHER-MAP").unwrap(),
        TerrainId::parse("terrain-a").unwrap(),
    ));
    app.world_mut().spawn((
        components::NameInput,
        bevy::text::EditableText::new("不得串写"),
    ));
    app.world_mut().spawn((
        feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed,
        components::ActionButton(components::Action::Rename),
    ));
    app.update();
    assert!(
        !app.world()
            .resource::<TerrainObjectsEditorState>()
            .preparing()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn name_focus_releases_the_numeric_editor_and_resumes_only_for_its_bound_object() {
    use bevy::{
        ecs::system::RunSystemOnce,
        input_focus::{FocusCause, InputFocus},
    };
    use feathrise_editor_ui::{
        ActiveNumericInput, NumericInputSetValue, NumericInputValue, NumericValue,
    };
    let mut app = app();
    select_first(&mut app);
    app.init_resource::<InputFocus>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<resources::NameInputBinding>()
        .add_message::<NumericInputSetValue>();
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .map_id
        .clone();
    app.world_mut()
        .resource_mut::<resources::NameInputBinding>()
        .target = Some((map, TerrainId::parse("terrain-a").unwrap()));
    let source_name = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .terrains
        .as_ref()
        .unwrap()[0]
        .display_name
        .clone();
    app.world_mut()
        .resource_mut::<resources::NameInputBinding>()
        .observed_name = source_name;
    let name = app
        .world_mut()
        .spawn((
            components::NameInput,
            bevy::text::EditableText::new("已确认但未应用的名称"),
        ))
        .id();
    let number = app
        .world_mut()
        .spawn(NumericInputValue(NumericValue::F32(8.0)))
        .id();
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(number);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(name, FocusCause::Navigated);
    app.world_mut().run_system_once(names::focus).unwrap();
    let request = app
        .world_mut()
        .resource_mut::<Messages<NumericInputSetValue>>()
        .drain()
        .next()
        .unwrap();
    assert_eq!(request.entity, number);
    assert!(request.cancel_edit);
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = None;
    app.world_mut().resource_mut::<InputFocus>().clear();
    app.world_mut().run_system_once(names::focus).unwrap();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(name));
    app.init_resource::<crate::ui::sidebars::MapEditorSidebarState>();
    app.world_mut().run_system_once(names::sync).unwrap();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(name)); // 左侧切页不再关闭右侧名称。
    app.world_mut().resource_mut::<crate::ui::sidebars::MapEditorSidebarState>().right = crate::ui::sidebars::EditorRightSidebarTab::Assets;
    app.world_mut().run_system_once(names::sync).unwrap();
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
    assert_eq!(
        app.world()
            .get::<bevy::text::EditableText>(name)
            .unwrap()
            .value()
            .to_string(),
        "已确认但未应用的名称"
    );
}
