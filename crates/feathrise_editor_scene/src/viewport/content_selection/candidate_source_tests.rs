//! 跨源候选失败保留正式目标与行，迟到查询继续占单槽并在真正完成后安全拒收。
use feathrise_editor_ui::UiInteraction;
use super::*;
use bevy::{ecs::system::RunSystemOnce, tasks::AsyncComputeTaskPool};
use crate::viewport::content_selection::layers::{Identity, LayerCandidate, Request};

fn press_card(app: &mut App, target: &LayerTarget) {
    let index = app.world().resource::<LayerCandidateState>().candidates.iter()
        .position(|candidate| &candidate.target == target).unwrap();
    let world = app.world_mut();
    let row = world.query::<(Entity, &CandidateAction)>().iter(world)
        .find(|(_, action)| matches!(action, CandidateAction::Select(at) if *at == index)).unwrap().0;
    app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
}

fn selected_root() -> (App, Vec<String>) {
    let (mut app, labels) = root_over_private_fixture();
    let before = app.world().resource::<LayerCandidateState>().selection_revision;
    click_grid(&mut app, 0);
    finish_confirmation(&mut app, before, &LayerTarget::Water(
        crate::types::WaterAuthoringFeatureKind::StillWater, "root-over-private".into()));
    (app, labels)
}

#[test]
fn rejected_cross_source_rows_keep_the_confirmed_target_index_canvas_and_card() {
    for private_active in [false, true] {
        for reason in ["busy", "numeric", "sparse-dirty", "saving", "loading", "selection-busy",
            "definition-changed", "frozen-definition-changed", "missing-author", "history"] {
            let (mut app, labels) = selected_root();
            if private_active { choose(&mut app, LayerTarget::Surface("soil-a-a".into())); }
            let (selected, author_index, row_index, identity, canvas, confirmations) = {
                let state = app.world().resource::<LayerCandidateState>();
                (state.selected.clone(), state.selected_author_index, state.index, state.identity.clone(),
                    state.canvas_terrain.clone(), state.selection_revision)
            };
            let revision = app.world().resource::<EditorAuthoringSession>().revision;
            match reason {
                "busy" => app.world_mut().resource_mut::<WaterBrushStroke>().active = true,
                "numeric" => {
                    let entity = app.world_mut().spawn_empty().id();
                    app.init_resource::<feathrise_editor_ui::ActiveNumericInput>();
                    app.world_mut().resource_mut::<feathrise_editor_ui::ActiveNumericInput>().entity = Some(entity);
                }
                "sparse-dirty" => app.world_mut().resource_mut::<EditorAuthoringSession>().sparse_terrain_dirty = true,
                "saving" => app.world_mut().resource_mut::<EditorAuthoringSession>().saving = true,
                "loading" => app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().terrain_documents.loading = true,
                "selection-busy" => app.world_mut().resource_mut::<TerrainObjectsEditorState>().transforming = true,
                "definition-changed" | "frozen-definition-changed" => {
                    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                    let draft = session.draft.as_mut().unwrap();
                    let object = &mut Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0];
                    object.transform.translation[0] += 1.0;
                    let changed = object.clone();
                    if reason == "frozen-definition-changed" {
                        if let Some(active) = draft.terrain_documents.active.as_mut() {
                            active.definition = Arc::new(changed);
                        } else {
                            let stored = draft.terrain_documents.values.get_mut(&changed.id).unwrap();
                            Arc::make_mut(stored).identity.as_mut().unwrap().definition = Arc::new(changed);
                        }
                    } // 即使缓存和当前清单已经共同换代，旧卡片仍须拒绝，不能只验证二者相等。
                }
                "missing-author" => {
                    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                    let draft = session.draft.as_mut().unwrap();
                    if private_active { draft.terrain_documents.root_water.as_mut().unwrap().water.as_mut().unwrap().layers.clear(); }
                    else { Arc::make_mut(draft.terrain_documents.values.get_mut(&TerrainId::parse("terrain-a").unwrap()).unwrap())
                        .surface.as_mut().unwrap().layers.clear(); }
                }
                "history" => {
                    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                    for _ in 0..257 { session.undo.push(crate::authoring::EditorCommand::WaterLevelChanged {
                        layer_id: "root-over-private".into(), before: 6.0, after: 7.0 }); }
                }
                _ => unreachable!(),
            }
            let target = if private_active { LayerTarget::RootWater(
                crate::types::WaterAuthoringFeatureKind::StillWater, "root-over-private".into()) }
                else { LayerTarget::Surface("soil-a-a".into()) };
            press_card(&mut app, &target);
            let state = app.world().resource::<LayerCandidateState>();
            assert_eq!(state.selected, selected, "{private_active}/{reason}");
            assert_eq!(state.selected_author_index, author_index, "{private_active}/{reason}");
            assert_eq!(state.index, row_index, "拒绝不能让原正式目标配上待应用行：{private_active}/{reason}");
            assert_eq!(state.identity, identity, "失败不得提升旧列表修订：{private_active}/{reason}");
            assert_eq!(state.canvas_terrain, canvas);
            assert_eq!(state.selection_revision, confirmations);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.revision, revision);
            assert_eq!(session.draft.as_ref().unwrap().terrain_documents.active.is_some(), private_active);
            assert_card(&mut app, &labels);
        }
    }
}

#[test]
fn stale_candidate_apply_cannot_promote_an_old_scene_revision_generation_or_filter() {
    for field in ["scene_id", "revision", "hash", "generation", "filter"] {
        let (mut app, _) = selected_root();
        let before = app.world().resource::<LayerCandidateState>().selected.clone();
        {
            let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
            state.apply = Some(LayerTarget::Surface("soil-a-a".into()));
            state.apply_candidate_index = Some(1);
            let identity = state.identity.as_mut().unwrap();
            match field {
                "scene_id" => identity.scene_id = SceneId::parse("MAP-old-candidate").unwrap(),
                "revision" => identity.revision = identity.revision.wrapping_sub(1),
                "hash" => identity.domain_hash.push_str("-old"),
                "generation" => identity.generation = identity.generation.wrapping_add(1),
                "filter" => identity.filter = crate::viewport::modes::SelectionFilter::Water,
                _ => unreachable!(),
            }
        }
        app.world_mut().run_system_once(crate::viewport::content_selection::apply_layer_candidate).unwrap();
        let state = app.world().resource::<LayerCandidateState>();
        assert_eq!(state.selected, before, "{field}");
        assert_eq!(state.selected_author_index, Some(0));
        assert_eq!(state.index, 0);
        assert_eq!(state.candidates.len(), 3);
        assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.is_none());
    } // 单独执行真实采用出口，验证其自身权限门；正常全调度还会按旧身份完整淘汰候选。
}

#[test]
fn source_switch_keeps_the_actual_inflight_slot_and_rejects_its_late_empty_result() {
    type Result = (Identity, Request, Vec<LayerCandidate>);
    let (mut app, labels) = selected_root();
    let slot = Arc::new(std::sync::Mutex::new((None::<Result>, None::<std::task::Waker>)));
    let late = {
        let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
        let key = state.identity.clone().unwrap();
        state.request(key, Ray3d::new(Vec3::Y * 20.0, Dir3::NEG_Y), None,
            Vec2::splat(64.0), Vec2::splat(128.0), 1.0, None);
        let request = state.request.take().unwrap();
        state.task_identity = Some(request.identity.clone());
        let work = slot.clone();
        state.task = Some(AsyncComputeTaskPool::get().spawn(std::future::poll_fn(move |cx| {
            let mut work = work.lock().unwrap();
            if let Some(result) = work.0.take() { std::task::Poll::Ready(result) }
            else { work.1 = Some(cx.waker().clone()); std::task::Poll::Pending }
        })));
        // 结果已冻结旧来源；只控制何时完成，不drop句柄假装取消真正运行中的任务。
        (request.identity.clone(), request, Vec::new())
    };
    // 结果尚未交付时worker保持Pending；成功换槽时原Task仍占唯一槽位。
    press_card(&mut app, &LayerTarget::Surface("soil-a-a".into()));
    assert!(app.world().resource::<LayerCandidateState>().task.is_some());
    assert!(app.world().resource::<LayerCandidateState>().task_identity.is_none());
    let wake = { let mut work = slot.lock().unwrap(); work.0 = Some(late); work.1.take() };
    if let Some(wake) = wake { wake.wake(); }
    let started = Instant::now();
    while app.world().resource::<LayerCandidateState>().task.is_some() {
        app.update();
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
    let state = app.world().resource::<LayerCandidateState>();
    assert_eq!(state.selected, Some(LayerTarget::Surface("soil-a-a".into())));
    assert_eq!(state.selected_author_index, Some(0));
    assert_eq!(state.index, 1);
    assert_card(&mut app, &labels);
}

#[test]
fn real_repeated_clicks_keep_same_id_root_and_private_water_in_the_complete_candidate_order() {
    let (mut app, _) = selected_root();
    let (map, private, masks) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let private = draft.manifest.terrains.as_ref().unwrap()[0].id.clone();
        let mut water = draft.water.as_ref().unwrap().clone();
        water.layers[0].water_level = 5.0;
        water.layers[0].label = "网格湖".into();
        let masks = Arc::new(vec![211; 9]);
        let stored = Arc::make_mut(draft.terrain_documents.values.get_mut(&private).unwrap());
        stored.water_masks.insert(water.layers[0].id.clone(), masks.clone());
        stored.water = Some(water);
        (draft.scene_id.clone(), private, masks)
    };
    let batch = app.world_mut().spawn(feathrise_scene::SubsceneActive).id();
    let owner = app.world_mut().spawn(feathrise_scene::SubsceneInstance {
        path: SceneInstancePath::root(map.clone()).terrain(private.clone()),
        source_scene_id: map.clone(), generation: 1, batch,
    }).id();
    app.world_mut().spawn((
        PaintedWaterSurface { scene_id: map, layer_id: "root-over-private".into(),
            kind: WaterSurfaceKind::Still, purpose: PaintedWaterPurpose::Runtime },
        Aabb::from_min_max(Vec3::new(-32.0, 0.0, -32.0), Vec3::new(32.0, 0.0, 32.0)),
        GlobalTransform::from(Transform::from_xyz(0.0, 5.0, 0.0)), Visibility::Visible,
        feathrise_scene_core::residency::SceneInstanceOwner(owner),
    )); // 两个正式水面故意共享作者ID，只能按owner与冻结来源区分。
    let root = LayerTarget::RootWater(crate::types::WaterAuthoringFeatureKind::StillWater, "root-over-private".into());
    let water = LayerTarget::Water(crate::types::WaterAuthoringFeatureKind::StillWater, "root-over-private".into());
    choose(&mut app, LayerTarget::Surface("soil-a-a".into()));
    let before = app.world().resource::<LayerCandidateState>().selection_revision;
    click_grid(&mut app, 0); // 活动私有水首次进入这批完整候选，最前方仍是根水。
    finish_confirmation_count(&mut app, before, &water, 4);
    let labels = vec!["静水体 · 场景湖".into(), "静水体 · 网格湖".into(), "地表 · soil-a".into(),
        app.world().resource::<LayerCandidateState>().candidates[3].label.clone()];
    assert_card(&mut app, &labels);
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.is_none());
    for (expected, private_active) in [(water.clone(), true), (LayerTarget::Surface("soil-a-a".into()), true),
        (LayerTarget::TerrainObject("terrain-a".into()), true), (water.clone(), false), (water.clone(), true)] {
        let before = app.world().resource::<LayerCandidateState>().selection_revision;
        click_grid(&mut app, 0);
        finish_confirmation_count(&mut app, before, &expected, 4);
        assert_card(&mut app, &labels);
        let state = app.world().resource::<LayerCandidateState>();
        assert_eq!(state.candidates[0].target, root);
        assert_eq!(state.candidates[1].target, water);
        let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
        assert_eq!(draft.terrain_documents.active.is_some(), private_active);
        if private_active {
            assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, 5.0);
            assert!(Arc::ptr_eq(&draft.water_masks["root-over-private"], &masks));
        } else { assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, 7.0); }
    }
}
