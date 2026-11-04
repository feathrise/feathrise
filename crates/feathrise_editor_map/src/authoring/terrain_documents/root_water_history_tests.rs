//! 根水与同ID私有水的真实撤销回归：切换画布先完整停放，再只恢复历史所属的根文档。
use super::{activate_history_canvas, history_fixture};
use crate::authoring::*;
use crate::authoring::terrain_documents::{TerrainDocumentTask, root_water, scope_history};
use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce, prelude::*};
use std::sync::Arc;

fn undo_app(session: EditorAuthoringSession, private: &feathrise_map_content::TerrainId) -> App {
    let map = session.draft.as_ref().unwrap().map_id.clone();
    let mut selection = crate::ui::terrain_objects::TerrainObjectsEditorState::default();
    selection.map = Some(map.clone());
    selection.selected = Some(private.clone());
    selection.selected_index = Some(1);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(session)
        .insert_resource(crate::resources::MapEditorState {
            open: true, selected_map_id: Some(map), terrain_canvas_target: Some(private.clone()),
            undo_requested: true, ..default()
        })
        .insert_resource(selection)
        .insert_resource(EditorPreviewAssets {
            portal_base_mesh: default(), portal_arrow_head_mesh: default(),
            portal_arrow_shaft_mesh: default(), portal_edge_beam_mesh: default(),
            portal_edge_ring_mesh: default(), portal_base_material: default(),
            portal_arrow_material: default(), portal_edge_beam_material: default(),
            portal_edge_ring_material: default(), portal_label_font: default(),
        })
        .init_resource::<TerrainDocumentTask>()
        .init_resource::<courtyards::CourtyardAuthoringState>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<WaterMaskMaterializeTask>()
        .init_resource::<crate::authoring::surface_layer_edit::SurfaceLayerEditTask>()
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<WaterFileHandoff>();
    app
}

#[test]
fn root_water_undo_and_redo_restore_their_canvas_without_changing_same_id_private_water() {
    // 参数和整笔R8历史都经过真实统一撤销入口；不能只验证命令包装的静态形状。
    for snapshot in [false, true] {
        let (mut session, _, private) = history_fixture();
        let before_pixels = Arc::new(vec![17; 16]);
        let after_pixels = Arc::new(vec![29; 16]);
        let private_pixels = Arc::new(vec![211; 16]);
        let layer = {
            let draft = session.draft.as_mut().unwrap();
            root_water::focus(draft);
            let mut water = water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
            water.layers[0].resolution = [4; 2];
            water.layers[0].water_level = 7.0;
            let layer = water.layers[0].id.clone();
            draft.water = Some(water);
            draft.water_asset_path = format!("{}/root.map_water.json", draft.folder);
            draft.water_masks.insert(layer.clone(), after_pixels.clone());
            layer
        };
        let root_path = session.draft.as_ref().unwrap().water_asset_path.clone();
        session.undo.push(if snapshot {
            EditorCommand::WaterStrokeSnapshot(Box::new(commands::WaterStrokeSnapshot {
                layer_id: layer.clone(), before_mask: before_pixels.clone(),
                after_mask: after_pixels.clone(), terrain: None,
            }))
        } else {
            EditorCommand::WaterLevelChanged { layer_id: layer.clone(), before: 3.0, after: 7.0 }
        });
        scope_history(&mut session);
        assert!(matches!(session.undo.last(), Some(EditorCommand::RootWater(_))));
        {
            let draft = session.draft.as_mut().unwrap();
            activate_history_canvas(draft, &private);
            assert_eq!(draft.water.as_ref().unwrap().layers[0].id, layer,
                "私有画布故意使用相同层ID，排除仅靠层名碰巧隔离的实现");
            draft.water_asset_path = format!("{}/terrains/{}/private.map_water.json", draft.folder, private.as_str());
            draft.water_masks.insert(layer.clone(), private_pixels.clone());
        }
        let mut app = undo_app(session, &private);
        for undoing in [true, false] {
            if !undoing {
                activate_history_canvas(app.world_mut().resource_mut::<EditorAuthoringSession>()
                    .draft.as_mut().unwrap(), &private);
                let mut editor = app.world_mut().resource_mut::<crate::resources::MapEditorState>();
                editor.terrain_canvas_target = Some(private.clone());
                editor.redo_requested = true;
            }
            app.world_mut().run_system_once(apply_undo_redo).unwrap();
            {
                let editor = app.world().resource::<crate::resources::MapEditorState>();
                let session = app.world().resource::<EditorAuthoringSession>();
                let draft = session.draft.as_ref().unwrap();
                assert!(draft.terrain_documents.active.is_none());
                assert_eq!(draft.water_asset_path, root_path);
                assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level,
                    if !snapshot && !undoing { 3.0 } else { 7.0 });
                assert!(if undoing { editor.undo_requested } else { editor.redo_requested },
                    "恢复画布这一帧保留请求，不能误消费私有层或丢失历史");
                assert!(editor.terrain_canvas_target.is_none());
            }
            app.world_mut().run_system_once(apply_undo_redo).unwrap();
            let session = app.world().resource::<EditorAuthoringSession>();
            let draft = session.draft.as_ref().unwrap();
            assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level,
                if !snapshot && undoing { 3.0 } else { 7.0 });
            let expected = if snapshot && undoing { &before_pixels } else { &after_pixels };
            assert!(Arc::ptr_eq(&draft.water_masks[&layer], expected));
            let parked = &draft.terrain_documents.values[&private];
            assert_eq!(parked.water.as_ref().unwrap().layers[0].water_level, 900.0);
            assert!(Arc::ptr_eq(&parked.water_masks[&layer], &private_pixels),
                "切回根水后，私有水的完整像素版本仍由自己的停放槽持有");
            assert_eq!((session.undo.len(), session.redo.len()), if undoing { (0, 1) } else { (1, 0) });
            let command = if undoing { session.redo.last() } else { session.undo.last() };
            assert!(matches!(command, Some(EditorCommand::RootWater(_))));
        }
    }
}

#[test]
fn root_water_history_waits_for_private_dirty_pages_and_strokes_before_switching_sources() {
    // 保存后撤销雕刻仍会产生待保存稀疏页。根水历史必须等待原作者提交，不能先换槽退役它。
    for undoing in [true, false] {
        for blocker in ["sparse_dirty", "terrain_stroke", "vegetation_stroke"] {
            let (mut session, _, private) = history_fixture();
            let layer = {
                let draft = session.draft.as_mut().unwrap();
                root_water::focus(draft);
                let mut water = water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
                water.layers[0].water_level = 7.0;
                let layer = water.layers[0].id.clone();
                draft.water = Some(water);
                layer
            };
            session.undo.push(EditorCommand::WaterLevelChanged {
                layer_id: layer.clone(), before: 3.0, after: 7.0,
            });
            scope_history(&mut session);
            activate_history_canvas(session.draft.as_mut().unwrap(), &private);
            let private_pixels = Arc::new(vec![173; 16]);
            session.draft.as_mut().unwrap().water_masks.insert(layer.clone(), private_pixels.clone());
            if !undoing {
                session.redo.push(session.undo.pop().unwrap());
            }
            let mut app = undo_app(session, &private);
            app.world_mut().resource_mut::<TerrainDocumentTask>().requested = Some(private.clone());
            {
                let mut editor = app.world_mut().resource_mut::<crate::resources::MapEditorState>();
                editor.undo_requested = undoing;
                editor.redo_requested = !undoing;
            }
            app.init_resource::<TerrainBrushStroke>().init_resource::<VegetationBrushStroke>();
            match blocker {
                "sparse_dirty" => {
                    let world = app.world_mut();
                    world.resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
                        // 复用真实雕刻/稀疏撤销完成时的脏责任入口，而非另造一种切源标记。
                        session.mark_sparse_terrain_changed(&mut world.resource_mut::<crate::resources::MapEditorState>());
                    });
                }
                "terrain_stroke" => app.world_mut().resource_mut::<TerrainBrushStroke>().active = true,
                "vegetation_stroke" => app.world_mut().resource_mut::<VegetationBrushStroke>().active = true,
                _ => unreachable!(),
            }
            let revision = app.world().resource::<EditorAuthoringSession>().revision;
            let water_revision = app.world().resource::<EditorAuthoringSession>().water_revision;
            app.world_mut().run_system_once(apply_undo_redo).unwrap();
            {
                let session = app.world().resource::<EditorAuthoringSession>();
                let editor = app.world().resource::<crate::resources::MapEditorState>();
                let draft = session.draft.as_ref().unwrap();
                assert_eq!(draft.terrain_documents.active.as_ref().unwrap().definition.id, private, "{blocker}");
                assert!(Arc::ptr_eq(&draft.water_masks[&layer], &private_pixels));
                assert_eq!((session.revision, session.water_revision), (revision, water_revision));
                assert_eq!((session.undo.len(), session.redo.len()), if undoing { (1, 0) } else { (0, 1) });
                assert_eq!((editor.undo_requested, editor.redo_requested), (undoing, !undoing));
                assert_eq!(editor.terrain_canvas_target.as_ref(), Some(&private));
                assert_eq!(app.world().resource::<TerrainDocumentTask>().requested.as_ref(), Some(&private));
                assert_eq!(app.world().resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>().selected.as_ref(), Some(&private));
            }
            // 模拟原事务完整结束/保存清责后，同一个请求自动恢复，不要求用户再次点击。
            app.world_mut().resource_mut::<EditorAuthoringSession>().sparse_terrain_dirty = false;
            app.world_mut().resource_mut::<TerrainBrushStroke>().active = false;
            app.world_mut().resource_mut::<VegetationBrushStroke>().active = false;
            app.world_mut().run_system_once(apply_undo_redo).unwrap();
            app.world_mut().run_system_once(apply_undo_redo).unwrap();
            let session = app.world().resource::<EditorAuthoringSession>();
            let draft = session.draft.as_ref().unwrap();
            assert!(draft.terrain_documents.active.is_none());
            assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, if undoing { 3.0 } else { 7.0 });
            assert!(Arc::ptr_eq(&draft.terrain_documents.values[&private].water_masks[&layer], &private_pixels));
            assert_eq!((session.undo.len(), session.redo.len()), if undoing { (0, 1) } else { (1, 0) });
        }
    }
}

#[test]
fn root_water_redo_scopes_all_private_history_before_parking_its_canvas() {
    let (mut session, _, private) = history_fixture();
    let layer = {
        let draft = session.draft.as_mut().unwrap();
        root_water::focus(draft);
        let mut water = water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
        water.layers[0].water_level = 3.0;
        let layer = water.layers[0].id.clone();
        draft.water = Some(water);
        layer
    };
    activate_history_canvas(session.draft.as_mut().unwrap(), &private);
    for _ in 0..129 {
        session.undo.push(EditorCommand::TerrainChanged(Vec::new()));
    }
    session.redo.push(EditorCommand::RootWater(Box::new(EditorCommand::WaterLevelChanged {
        layer_id: layer, before: 3.0, after: 7.0,
    })));
    let mut app = undo_app(session, &private);
    {
        let mut editor = app.world_mut().resource_mut::<crate::resources::MapEditorState>();
        editor.undo_requested = false;
        editor.redo_requested = true;
    }
    for scoped in [64, 128] {
        app.world_mut().run_system_once(apply_undo_redo).unwrap();
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.terrain_documents.scoped_through, scoped);
        assert_eq!(draft.terrain_documents.active.as_ref().unwrap().definition.id, private);
        assert_eq!((session.undo.len(), session.redo.len()), (129, 1));
        assert!(app.world().resource::<crate::resources::MapEditorState>().redo_requested);
    }
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    {
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.draft.as_ref().unwrap().terrain_documents.active.is_none());
        assert!(session.undo.iter().all(|command|
            matches!(command, EditorCommand::TerrainCanvas { id, .. } if id == &private)),
            "根水重做恢复来源前，129条旧私有历史必须全部取得原对象身份");
        assert_eq!((session.undo.len(), session.redo.len()), (129, 1));
    }
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!((session.undo.len(), session.redo.len()), (130, 0));
    assert_eq!(session.draft.as_ref().unwrap().water.as_ref().unwrap().layers[0].water_level, 7.0);
}
