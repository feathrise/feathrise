//! 根画布水面走真实视口输入、后台候选和属性应用，重复点击必须保留完整重叠列表并继续轮选。
use super::*;
use bevy::camera::primitives::Aabb;
use feathrise_scene::{PaintedWaterPurpose, PaintedWaterSurface, WaterSurfaceKind};
#[path = "candidate_source_tests.rs"]
mod candidate_source_tests;

/// 通过真正生成的UI行读取文字和挂载状态，不能只靠候选资源数量宣称卡片还在。
fn assert_card(app: &mut App, expected: &[String]) {
    let world = app.world_mut();
    assert_eq!(world.query_filtered::<&Node, With<LayerCandidateSlot>>().single(world).unwrap().display, Display::Flex);
    let mut rows = world.query::<(&CandidateAction, &Children)>().iter(world)
        .filter_map(|(action, children)| match action {
            CandidateAction::Select(index) => Some((*index, children.iter().find_map(|child| world.get::<Text>(child).map(|text| text.0.clone())).unwrap())),
            _ => None,
        }).collect::<Vec<_>>();
    rows.sort_by_key(|row| row.0);
    assert_eq!(rows.into_iter().map(|row| row.1).collect::<Vec<_>>(), expected);
}

/// 原私有Clipmap只保留一层真实地表；根水、地表、网格应组成截图中的完整三项。
fn root_over_private_fixture() -> (App, Vec<String>) {
    let (mut app, _) = fixture();
    let (map, layer) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let id = draft.manifest.terrains.as_ref().unwrap()[0].id.clone();
        let canvas = Arc::make_mut(draft.terrain_documents.values.get_mut(&id).unwrap());
        canvas.vegetation = None;
        canvas.vegetation_author_densities.clear();
        canvas.surface.as_mut().unwrap().layers.truncate(1);
        canvas.surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(4)));
        canvas.surface_indices = Some(Arc::new(vec![0; 64]));
        assert_eq!(canvas.identity.as_ref().unwrap().definition.terrain.kind, SceneTerrainKind::SparseClipmap);
        assert!(canvas.water.is_none(), "地表恢复不能依赖私有water文档存在");
        let mut water = crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
        water.layers[0].id = "root-over-private".into();
        water.layers[0].label = "场景湖".into();
        water.layers[0].resolution = [3; 2];
        water.layers[0].canvas_bounds = Some([[-32.0; 2], [32.0; 2]]);
        water.layers[0].water_level = 7.0;
        draft.water_masks = [(water.layers[0].id.clone(), Arc::new(vec![255; 9]))].into();
        let layer = water.layers[0].clone();
        draft.water = Some(water);
        (draft.scene_id.clone(), layer)
    };
    app.world_mut().spawn((
        PaintedWaterSurface { scene_id: map, layer_id: layer.id,
            kind: WaterSurfaceKind::Still, purpose: PaintedWaterPurpose::Runtime },
        Aabb::from_min_max(Vec3::new(-32.0, 0.0, -32.0), Vec3::new(32.0, 0.0, 32.0)),
        GlobalTransform::from(Transform::from_xyz(0.0, 7.0, 0.0)), Visibility::Visible,
    )); // 真实Input按正式水面Aabb与R8采样，不直接伪造候选或正式选择。
    let grid_label = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
        .manifest.terrains.as_ref().unwrap()[0].display_name.clone();
    (app, vec!["静水体 · 场景湖".into(), "地表 · soil-a".into(), format!("网格 · {grid_label}")])
}

fn finish_confirmation(app: &mut App, before: u64, expected: &LayerTarget) {
    finish_confirmation_count(app, before, expected, 3);
}

fn finish_confirmation_count(app: &mut App, before: u64, expected: &LayerTarget, count: usize) {
    let started = Instant::now();
    loop {
        app.update();
        let state = app.world().resource::<LayerCandidateState>();
        if state.selection_revision > before {
            assert_eq!(state.selected.as_ref(), Some(expected));
            assert_eq!(state.selection_revision, before + 1);
            assert_eq!(state.candidates.len(), count);
            assert_eq!(state.pixel, Some(Vec2::splat(64.0)));
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "真实点击未完成：selected={:?},pending={},wait={}",
            state.selected, state.task.is_some(), state.waiting_canvas.is_some());
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn private_clipmap_root_water_keeps_real_three_row_card_and_restores_surface_and_grid_sources() {
    let (mut app, labels) = root_over_private_fixture();
    let root = LayerTarget::Water(crate::types::WaterAuthoringFeatureKind::StillWater, "root-over-private".into());
    let grid = LayerTarget::TerrainObject("terrain-a".into());
    let surface = LayerTarget::Surface("soil-a-a".into());
    let before = app.world().resource::<LayerCandidateState>().selection_revision;
    click_grid(&mut app, 0);
    finish_confirmation(&mut app, before, &root);
    assert_card(&mut app, &labels);
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.is_none());
    for expected in [surface.clone(), root.clone(), grid.clone(), root.clone()] {
        let before = app.world().resource::<LayerCandidateState>().selection_revision;
        choose(&mut app, expected.clone());
        assert_eq!(app.world().resource::<LayerCandidateState>().selection_revision, before + 1);
        assert_card(&mut app, &labels);
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert!(app.world().resource::<LayerCandidateState>().author_canvas_matches(draft));
        assert_eq!(draft.terrain_documents.active.as_ref().map(|active| active.definition.id.as_str()),
            if expected == root { None } else { Some("terrain-a") });
        assert!(session.undo.is_empty() && session.redo.is_empty());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
    // 根水当前活动也必须继续同点轮选；重采样为原地表恢复画布，不能清掉上次列表或重命名根水来源。
    for expected in [surface, grid, root] {
        let before = app.world().resource::<LayerCandidateState>().selection_revision;
        click_grid(&mut app, 0);
        finish_confirmation(&mut app, before, &expected);
        assert_card(&mut app, &labels);
    }
}

#[test]
fn active_root_water_keeps_overlap_candidates_and_cycles_through_real_viewport_clicks() {
    let (mut app, _) = fixture();
    let (map, layers) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrains = Some(Arc::from([]));
        draft.manifest.world_min = [-32.0; 2];
        draft.manifest.world_max = [32.0; 2];
        draft.terrain_documents = default();
        draft.terrain_ground = None;
        draft.surface = None;
        draft.vegetation = None;
        let mut water = crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
        water.layers[0].id = "upper-root-water".into();
        water.layers[0].resolution = [3; 2];
        water.layers[0].canvas_bounds = Some([[-32.0; 2], [32.0; 2]]);
        water.layers[0].water_level = 4.0;
        let mut lower = water.layers[0].clone();
        lower.id = "lower-root-water".into();
        lower.water_level = 2.0;
        water.layers.push(lower);
        draft.water_masks = water.layers.iter().map(|layer| (layer.id.clone(), Arc::new(vec![255; 9]))).collect();
        let layers = water.layers.clone();
        draft.water = Some(water);
        (draft.scene_id.clone(), layers)
    };
    for layer in &layers {
        app.world_mut().spawn((
            PaintedWaterSurface { scene_id: map.clone(), layer_id: layer.id.clone(),
                kind: WaterSurfaceKind::Still, purpose: PaintedWaterPurpose::Runtime },
            Aabb::from_min_max(Vec3::new(-32.0, 0.0, -32.0), Vec3::new(32.0, 0.0, 32.0)),
            GlobalTransform::from(Transform::from_xyz(0.0, layer.water_level, 0.0)),
            Visibility::Visible,
        )); // 正式上传后只留完成Aabb的水面，不依赖CPU Mesh或直接写入候选结果。
    }
    let camera = app.world().resource::<SceneEditorViewportState>().camera;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_translation(Vec3::Y * 20.0).looking_at(Vec3::ZERO, Vec3::Z);
    for index in [0, 1, 0] {
        let confirmations = app.world().resource::<LayerCandidateState>().selection_revision;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        let expected = LayerTarget::Water(crate::types::WaterAuthoringFeatureKind::StillWater, layers[index].id.clone());
        let started = Instant::now();
        loop {
            app.update();
            let state = app.world().resource::<LayerCandidateState>();
            if state.selection_revision > confirmations {
                assert_eq!(state.selected.as_ref(), Some(&expected));
                assert_eq!(state.candidates.len(), 2, "根画布直选不能清掉重叠候选卡片");
                assert_eq!(state.pixel, Some(Vec2::splat(64.0)), "同点轮选保留真实点击位置");
                assert_eq!(state.selection_revision, confirmations + 1);
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "根水的真实视口点击应完成后台选择");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    choose(&mut app, LayerTarget::Water(crate::types::WaterAuthoringFeatureKind::StillWater, layers[1].id.clone()));
    assert_eq!(app.world().resource::<SceneEditorState>().selected_water_layer, 1);
    assert_eq!(app.world().resource::<LayerCandidateState>().candidates.len(), 2);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().terrain_documents.active.is_none());
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}
