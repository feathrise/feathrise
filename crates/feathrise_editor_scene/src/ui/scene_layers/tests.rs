//! 使用自建多作者层草稿核对正式树行、末尾滚动与切换场景撤销身份，不读取游戏场景。

use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    ui::scene_context_menu::{SceneObjectKey, SceneObjectRow},
};
use bevy::prelude::*;
use crate::game_authoring::scene_interaction::{fixtures, DomainItemKind, SceneInteractionState};

fn test_object(session: &EditorAuthoringSession, list: types::LayerList, index: usize) -> Option<(crate::ui::scene_context_menu::SceneObjectTarget, String)> {
    let hash = session.draft.as_ref()?.adopted_domain_hash();
    rows::object(session, list, index, Some(&fixtures::state(hash, 60)))
}


pub(crate) fn install_snapshot_sync(app: &mut App) {
    app.add_systems(Update, systems::sync);
}

/// 保存回归通过正式树行解析器取当前快照，避免只检查底层水槽而漏掉UI再次丢项。
pub(crate) fn water_snapshot(session: &EditorAuthoringSession) -> Vec<crate::ui::scene_context_menu::SceneObjectTarget> {
    let count = session.draft.as_ref().map_or(0, |draft| types::LayerList::Water.count(draft, None));
    (0..count).filter_map(|index| test_object(session, types::LayerList::Water, index).map(|item| item.0)).collect()
}

pub(crate) fn assert_snapshot_glyphs(app: &mut App) {
    let world = app.world_mut();
    let mut kinds = [false; 4];
    for (row, node, children) in world
        .query::<(&components::Row, &ComputedNode, &Children)>()
        .iter(world)
    {
        if node.size() == Vec2::ZERO {
            continue;
        }
        kinds[row.list as usize] = true;
        assert!(children.iter().any(|child| {
            world
                .get::<bevy::text::TextLayoutInfo>(child)
                .is_some_and(|layout| !layout.glyphs.is_empty())
        }));
    }
    assert!(kinds.into_iter().all(|present| present));
}

pub(crate) fn load_author_layers_fixture(app: &mut App) {
    use feathrise_scene_content::*;
    use std::sync::Arc;
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.scene_id.clone();
    draft.manifest.display_name = "中性作者树夹具".into();
    draft.manifest.terrains = Some(draft.manifest.terrain_objects_for_edit());
    // 三张私有静水画布属于同一主网格；明确绑定来源，不能把网格水误算成场景根水。
    let primary = draft.manifest.terrains.as_ref().unwrap()[0].clone();
    draft.terrain_documents.active = Some(crate::authoring::terrain_documents::TerrainDocumentIdentity {
        manifest: primary.source_manifest(&draft.manifest),
        folder: draft.folder.clone(),
        definition: Arc::new(primary),
    });
    let mut water = crate::authoring::default_water_asset(&map);
    water.layers = (0..3).map(|index| {
        let mut layer = crate::authoring::default_water_layer(&draft.folder);
        layer.id = format!("fixture-water-{index}");
        layer.label = format!("测试静水{index}");
        layer.mask_source_path = format!("{}/water/{}.r8", draft.folder, layer.id);
        layer.mask_texture_path = format!("{}/water/{}.ktx2", draft.folder, layer.id);
        layer.water_level = index as f32;
        layer.resolution = [2, 2];
        layer
    }).collect();
    draft.water = Some(water);

    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    ))).unwrap();
    let presets = ResolvedVegetationPresetCatalog::build(&catalog).unwrap();
    let layers = presets.presets().iter().take(3).map(|preset| {
        let mut output = preset.create_scene_layer(&draft.folder);
        output.resolution = [2, 2];
        output
    }).collect::<Vec<_>>();
    let authoring = SceneVegetationAuthoring {
        groups: vec![SceneVegetationAuthorGroup {
            id: "fixture-group".into(), label: "测试植被组".into(),
            mode: SceneVegetationAuthorGroupMode::Additive,
            output_merge: SceneVegetationOutputMergeMode::Maximum,
            capacity: 1.0, claim_threshold: 0.0, selection_seed: 1,
            selection_cell_meters: 1.0, enabled: true,
        }],
        layers: layers.iter().enumerate().map(|(index, output)| SceneVegetationAuthorLayer {
            id: format!("fixture-author-{index}"), label: format!("测试植被{index}"),
            group_id: "fixture-group".into(), output_layer_id: output.id.clone(),
            density_source_path: format!("{}/vegetation/author-{index}.r8", draft.folder),
            resolution: [2, 2], enabled: true, locked: false, opacity: 1.0,
            priority: 0, weight: 1.0,
        }).collect(),
    };
    draft.vegetation = Some(SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: SCENE_SCHEMA_VERSION, scene_id: map.clone(), layers,
        authoring: Some(authoring),
    });

    // 六十个独立来源超过固定四十行槽，末尾滚动仍须按真实文档索引找到最后一个。
    draft.domain = Some(fixtures::document("fixture-domain-v1"));
    app.insert_resource(fixtures::state("fixture-domain-v1", 60));
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(draft);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(map);
}

#[test]
fn scene_layers_domain_bounds_use_the_external_snapshot_and_reject_old_hash() {
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let (target, _) = test_object(session, types::LayerList::DomainLayers, 0).unwrap();
    let snapshot = app.world().resource::<SceneInteractionState>().snapshot.as_ref().unwrap();
    let bounds = (snapshot.bounds)(DomainItemKind::Layer, "fixture-domain-layer").unwrap();
    assert_eq!(bounds, (Vec3::new(-2., 0., -2.), Vec3::new(11., 8., 7.)));
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().domain = Some(fixtures::document("fixture-domain-v2"));
    assert!(!target.current(app.world().resource::<SceneEditorState>(), app.world().resource::<EditorAuthoringSession>()));
}

#[test]
fn scene_layers_water_and_vegetation_menus_use_the_frozen_row_and_original_history() {
    use crate::ui::scene_context_menu::{
        SceneContextMenuState, SceneMenuAction, SceneObjectRequest,
    };
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    app.init_resource::<SceneContextMenuState>()
        .init_resource::<crate::authoring::WaterBrushStroke>()
        .init_resource::<crate::authoring::WaterNetworkEditState>()
        .init_resource::<crate::authoring::VegetationBrushStroke>()
        .add_systems(
            Update,
            (
                crate::ui::water_layers::handle_scene_menu,
                crate::ui::vegetation_layers::handle_scene_menu,
            ),
        );
    let terrain = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        // 此夹具已直接载入primary的作者文件，补齐正式加载器采用后的画布身份，不能绕过来源门。
        let object = draft.manifest.terrains.as_ref().unwrap()[0].clone();
        let identity = crate::authoring::terrain_documents::TerrainDocumentIdentity {
            manifest: object.source_manifest(&draft.manifest),
            folder: draft.folder.clone(),
            definition: std::sync::Arc::new(object.clone()),
        };
        draft.terrain_documents.active = Some(identity);
        for layer in &mut draft.water.as_mut().unwrap().layers {
            layer.resolution = [2, 2];
            draft
                .water_masks
                .insert(layer.id.clone(), vec![1; 4].into());
        }
        for layer in &mut draft
            .vegetation
            .as_mut()
            .unwrap()
            .authoring
            .as_mut()
            .unwrap()
            .layers
        {
            draft.vegetation_author_densities.insert(
                layer.id.clone(),
                vec![1; layer.resolution[0] as usize * layer.resolution[1] as usize].into(),
            );
        }
        object.id
    };
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .terrain_canvas_target = Some(terrain);
    let (water_target, _) = test_object(
        app.world().resource::<EditorAuthoringSession>(),
        types::LayerList::Water,
        1,
    )
    .unwrap();
    let original_level = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .water
        .as_ref()
        .unwrap()
        .layers[1]
        .water_level;
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = Some(SceneObjectRequest {
        target: water_target,
        action: SceneMenuAction::Copy,
        name: String::new(),
    });
    app.update();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = None;
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.undo.len(),
        1,
        "{}",
        app.world().resource::<SceneEditorState>().status
    );
    assert!(matches!(
        session.undo.last(),
        Some(crate::authoring::EditorCommand::WaterLayersChanged(_))
    ));
    let water = session.draft.as_ref().unwrap().water.as_ref().unwrap();
    assert_eq!(water.layers.len(), 4);
    assert_eq!(water.layers[2].water_level, original_level); // 不能复制原先选择的第一个水体。
    let (target, _) = test_object(session, types::LayerList::Vegetation, 1).unwrap();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = Some(SceneObjectRequest {
        target,
        action: SceneMenuAction::Lock,
        name: String::new(),
    });
    app.update();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = None;
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.undo.len(),
        2,
        "{}",
        app.world().resource::<SceneEditorState>().status
    );
    assert!(matches!(
        session.undo.last(),
        Some(crate::authoring::EditorCommand::VegetationAuthoringChanged(
            _
        ))
    ));
    let layers = &session
        .draft
        .as_ref()
        .unwrap()
        .vegetation
        .as_ref()
        .unwrap()
        .authoring
        .as_ref()
        .unwrap()
        .layers;
    assert!(!layers[0].locked && layers[1].locked && !layers[2].locked);
    let (target, _) = test_object(session, types::LayerList::Vegetation, 1).unwrap();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = Some(SceneObjectRequest {
        target,
        action: SceneMenuAction::Visibility,
        name: String::new(),
    });
    app.update();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = None;
    let session = app.world().resource::<EditorAuthoringSession>();
    let layer = &session
        .draft
        .as_ref()
        .unwrap()
        .vegetation
        .as_ref()
        .unwrap()
        .authoring
        .as_ref()
        .unwrap()
        .layers[1];
    assert!(!layer.enabled && layer.locked);
    assert_eq!(session.undo.len(), 3);
}

#[test]
fn scene_layers_author_revision_keeps_the_writer_selected_water() {
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let old = session
        .draft
        .as_ref()
        .unwrap()
        .water
        .as_ref()
        .unwrap()
        .layers[0]
        .id
        .clone();
    let expected = session
        .draft
        .as_ref()
        .unwrap()
        .water
        .as_ref()
        .unwrap()
        .layers[1]
        .id
        .clone();
    app.world_mut().resource_scope(
        |world, mut candidates: Mut<crate::viewport::LayerCandidateState>| {
            candidates.confirm_tree_author(
                crate::viewport::LayerTarget::Water(
                    crate::types::WaterAuthoringFeatureKind::StillWater,
                    old,
                ),
                Some(0),
                world.resource::<EditorAuthoringSession>(),
                0,
                Default::default(),
            );
        },
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_water_layer = 1;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    app.add_systems(Update, systems::refresh_selection);
    app.update();
    let candidates = app
        .world()
        .resource::<crate::viewport::LayerCandidateState>();
    assert_eq!(
        candidates.selected,
        Some(crate::viewport::LayerTarget::Water(
            crate::types::WaterAuthoringFeatureKind::StillWater,
            expected
        ))
    );
    assert_eq!(candidates.selected_author_index, Some(1));
}

#[test]
fn water_selection_refresh_keeps_same_source_edits_and_rejects_restored_other_sources() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::types::{EditorTool, WaterAuthoringFeatureKind};
    use crate::viewport::{LayerCandidateState, LayerTarget};
    for completed_canvas in [false, true] {
        let (mut session, private, id) = crate::ui::water_feature_list::source_selection_tests::same_id_sources();
        let map = session.draft.as_ref().unwrap().scene_id.clone();
        let mut editor = SceneEditorState { open: true, selected_scene_id: Some(map), tool: EditorTool::Inspect, ..default() };
        if !completed_canvas {
            crate::ui::water_feature_list::activate_listed_water_feature(&mut editor, &mut session,
                Some(&private), false, WaterAuthoringFeatureKind::StillWater, &id, None, None).unwrap();
        }
        editor.tool = EditorTool::Inspect;
        let selected = LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, id.clone());
        let mut state = LayerCandidateState::default();
        state.confirm_tree_author(selected.clone(), Some(0), &session, 0, default());
        let mut app = App::new();
        app.insert_resource(editor).insert_resource(session).insert_resource(state);
        // 同一画布的普通参数修订仍走生产刷新，必须保留正式水选择和可编辑的当前作者身份。
        app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
        app.world_mut().run_system_once(systems::refresh_selection).unwrap();
        assert_eq!(app.world().resource::<LayerCandidateState>().selected.as_ref(), Some(&selected));
        assert!(app.world().resource::<LayerCandidateState>().selected_matches_session(app.world().resource::<EditorAuthoringSession>()));
        let before = app.world().resource::<EditorAuthoringSession>().revision;
        app.world_mut().resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
            let mut editor = world.resource_mut::<SceneEditorState>();
            crate::ui::water_feature_list::activate_listed_water_feature(&mut editor, &mut session,
                completed_canvas.then_some(&private), false, WaterAuthoringFeatureKind::StillWater, &id, None, None).unwrap();
            editor.tool = EditorTool::Inspect;
        }); // 通过真实来源恢复桥换槽；同ID的另一个文档没有继承旧选择的权限。
        if completed_canvas {
            let revision = app.world().resource::<EditorAuthoringSession>().revision;
            app.world_mut().resource_mut::<LayerCandidateState>().complete_terrain_canvas(&private, 0, before, revision);
            assert!(app.world().resource::<LayerCandidateState>().selected.is_none(), "画布完成不能保留另一来源的旧水选择");
        }
        app.world_mut().run_system_once(systems::refresh_selection).unwrap();
        assert!(app.world().resource::<LayerCandidateState>().selected.is_none(), "修订刷新不能把旧水索引套到恢复后的画布");
    }
}

#[test]
fn scene_layers_domain_focus_adopts_complete_bounds_and_rejects_late_camera_changes() {
    use crate::resources::SceneEditorViewportState;
    use crate::ui::scene_context_menu::{
        SceneContextMenuState, SceneMenuAction, SceneObjectRequest,
    };
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let (target, _) = test_object(session, types::LayerList::DomainLayers, 0).unwrap();
    let snapshot = app.world().resource::<SceneInteractionState>().snapshot.as_ref().unwrap();
    let bounds = (snapshot.bounds)(DomainItemKind::Layer, "fixture-domain-layer").unwrap();
    let id = "fixture-domain-layer".to_owned();
    let map = target.scene_id.clone();
    app.init_resource::<SceneContextMenuState>()
        .init_resource::<resources::LayerFocusState>()
        .init_resource::<crate::game_authoring::scene_interaction::SceneInteractionState>()
        .insert_resource(SceneEditorViewportState {
            _image: Handle::default(),
            camera: Entity::PLACEHOLDER,
            framed_scene_id: Some(map),
            center: Vec3::splat(999.0),
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::ONE,
            extent: 100.0,
            yaw: 0.8,
            pitch: 0.4,
            render_size: UVec2::splat(900),
        })
        .add_systems(Update, focus::update);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    app.world_mut()
        .resource_mut::<crate::game_authoring::scene_interaction::SceneInteractionState>()
        .selected_layer = Some(id.clone());
    app.world_mut()
        .resource_mut::<crate::viewport::LayerCandidateState>()
        .selected = Some(crate::viewport::LayerTarget::Domain(id));
    let request = SceneObjectRequest {
        target,
        action: SceneMenuAction::Focus,
        name: String::new(),
    };
    for late in [false, true] {
        app.world_mut()
            .resource_mut::<SceneContextMenuState>()
            .ready = Some(request.clone());
        app.update();
        app.world_mut()
            .resource_mut::<SceneContextMenuState>()
            .ready = None;
        let moved = Vec3::new(123.0, 456.0, 789.0);
        if late {
            app.world_mut()
                .resource_mut::<SceneEditorViewportState>()
                .center = moved;
        }
        for _ in 0..100 {
            app.update();
            if app
                .world()
                .resource::<resources::LayerFocusState>()
                .pending
                .is_none()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            app.world()
                .resource::<resources::LayerFocusState>()
                .pending
                .is_none()
        );
        let center = app.world().resource::<SceneEditorViewportState>().center;
        assert_eq!(
            center,
            if late {
                moved
            } else {
                bounds.0 * 0.5 + bounds.1 * 0.5
            }
        );
    }
}

#[test]
fn scene_a_scene_tree_contains_water_vegetation_and_domain_and_scrolls_to_last_source() {
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    app.add_systems(Update, systems::sync);
    crate::ui::scene_inspector::tests::settle(&mut app);
    let world = app.world_mut();
    let bindings: Vec<_> = world
        .query_filtered::<&SceneObjectRow, With<components::Row>>()
        .iter(world)
        .filter_map(|row| row.0.as_ref())
        .collect();
    assert!(
        bindings
            .iter()
            .any(|target| matches!(target.key, SceneObjectKey::Water { .. }))
    );
    assert!(
        bindings
            .iter()
            .any(|target| matches!(target.key, SceneObjectKey::Vegetation { output: false, .. }))
    );
    assert!(
        bindings
            .iter()
            .any(|target| matches!(target.key, SceneObjectKey::Domain { kind: DomainItemKind::Layer, .. }))
    );
    assert!(
        bindings
            .iter()
            .any(|target| matches!(target.key, SceneObjectKey::Domain { kind: DomainItemKind::Object, .. }))
    );
    let last_id = "fixture-source-59".to_owned();
    let list = world
        .query::<(Entity, &components::List)>()
        .iter(world)
        .find(|(_, list)| list.0 == types::LayerList::DomainObjects)
        .unwrap()
        .0;
    world.get_mut::<ScrollPosition>(list).unwrap().y = 1_000_000.0;
    crate::ui::scene_inspector::tests::settle(&mut app);
    let world = app.world_mut();
    assert!(world.query_filtered::<&SceneObjectRow, With<components::Row>>().iter(world)
        .filter_map(|row| row.0.as_ref()).any(|target| matches!(&target.key, SceneObjectKey::Domain { kind: DomainItemKind::Object, id, .. } if id == &last_id)));
    let saved = world
        .query_filtered::<&SceneObjectRow, With<components::Row>>()
        .iter(world)
        .find_map(|row| row.0.clone())
        .unwrap();
    world.resource_mut::<SceneEditorState>().selected_scene_id =
        Some(feathrise_scene_content::SceneId::parse("OTHER").unwrap());
    crate::ui::scene_inspector::tests::settle(&mut app);
    let world = app.world_mut();
    assert!(
        world
            .query_filtered::<&SceneObjectRow, With<components::Row>>()
            .iter(world)
            .all(|row| row.0.is_none())
    );
    assert!(!saved.current(
        world.resource::<SceneEditorState>(),
        world.resource::<EditorAuthoringSession>()
    ));
}
