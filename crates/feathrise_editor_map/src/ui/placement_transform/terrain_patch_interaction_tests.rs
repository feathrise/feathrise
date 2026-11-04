//! 静态补丁属性的真实数值/按钮输入回归；源父夹具只证明层级与事务，不冒充GPU地形加载。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use crate::authoring::EditorAuthoringSession;
use crate::resources::MapEditorState;
use crate::ui::placement_water::{register_placement_instance_root, unregister_placement_instance_root};
use feathrise_editor_ui::{NumericInputChanged, NumericInputCommitted, NumericInputCommitOutcome,
    NumericInputCommitSource, NumericInputScrubSensitivityChanged, NumericInputScrubSensitivityCommitted,
    NumericInputSetValue};
use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainCutout, MapTerrainDescriptor,
    MapTerrainKind, MapTerrainObject, TerrainId, TerrainObjectTransform};
use feathrise_scene::{MapPlacementInstance, TerrainVolumeAuthorPreviewRoot};
use std::sync::Arc;

fn source_pose() -> TerrainObjectTransform {
    TerrainObjectTransform { translation: [-130.0, 11.0, 75.0], yaw_degrees: 31.0,
        pitch_degrees: 17.0, roll_degrees: -12.0, scale: [2.0, 3.0, 5.0] }
}

fn record(app: &App) -> &MapPlacementRecord {
    &app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements[0]
}

fn pose(record: &MapPlacementRecord) -> Transform {
    Transform::from_translation(record.translation.into())
        .with_rotation(Quat::from_rotation_y(record.yaw_degrees.to_radians()))
        .with_scale(record.scale.into())
}

fn assert_matrix(actual: Mat4, expected: Mat4) {
    for point in [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(-2.0, 3.0, 5.0)] {
        assert!(actual.transform_point3(point).abs_diff_eq(expected.transform_point3(point), 0.001),
            "point={point:?}, actual={actual:?}, expected={expected:?}");
    }
}

fn fixture(field: PlacementTransformField, transform: TerrainObjectTransform, install_parent: bool,
    owned: bool) -> (App, Entity, Entity, Option<Entity>, MapTerrainObject) {
    let (mut app, root, _) = crate::viewport::placement_move::interaction_tests::app();
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .add_message::<NumericInputChanged>()
        .add_message::<NumericInputCommitted>()
        .add_message::<NumericInputScrubSensitivityChanged>()
        .add_message::<NumericInputScrubSensitivityCommitted>()
        .add_message::<NumericInputSetValue>()
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root)
        .add_systems(Update, handle_placement_transform_numeric_input);
    let (map, source, snapshot) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        if !owned { session.placement_previews.clear(); }
        let draft = session.draft.as_mut().unwrap();
        let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
        source.id = TerrainId::parse("property-patch-source").unwrap();
        source.transform = transform;
        source.terrain.kind = MapTerrainKind::SparseClipmap;
        source.world_min = [100.0; 2]; source.world_max = [200.0; 2];
        draft.manifest.world_min = [-20.0; 2]; draft.manifest.world_max = [20.0; 2];
        draft.manifest.terrains = Some(Arc::from([source.clone()]));
        draft.manifest.terrain = MapTerrainDescriptor::empty();
        draft.placement_water_surface_count = 0;
        let record = &mut draft.placements.placements[0];
        record.translation = [150.0, 3.0, 150.0]; record.yaw_degrees = 0.0; record.scale = [1.0; 3];
        record.water_surfaces.clear(); record.terrain_binding = None;
        record.terrain_patch = Some(MapPlacementTerrainPatch { terrain_id: Some(source.id.clone()),
            cutout: Some(MapTerrainCutout { min: [-2.0; 2], max: [2.0; 2] }), ..Default::default() });
        (draft.map_id.clone(), source, record.clone())
    };
    let parent = install_parent.then(|| feathrise_scene::runtime_test_support::install_active_terrain_instances(
        app.world_mut(), &map, std::slice::from_ref(&source))[0]);
    // 通用夹具先于索引观察者安装根；重新Add正式身份，才能按生产生命周期登记当前根。
    app.world_mut().entity_mut(root).remove::<MapPlacementInstance>()
        .insert((MapPlacementInstance::new(&map, &snapshot), pose(&snapshot)));
    if let Some(parent) = parent {
        app.world_mut().entity_mut(root).insert(ChildOf(parent));
        if owned { app.world_mut().entity_mut(root).insert(TerrainVolumeAuthorPreviewRoot(
            feathrise_map_content::MapInstancePath::root(map).terrain(source.id.clone()))); }
    }
    let input = app.world_mut().spawn(field).id();
    app.update();
    (app, input, root, parent, source)
}

fn commit(app: &mut App, input: Entity, value: f32, source: NumericInputCommitSource) {
    app.world_mut().write_message(NumericInputCommitted { entity: input, value: NumericValue::F32(value),
        outcome: NumericInputCommitOutcome::Accepted, source, changed: true, edited: true });
    app.update();
}

fn action(app: &mut App, action: PlacementTransformAction) {
    let button = app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(),
        MapEditorPlacementTransformActionButton(action))).id();
    app.world_mut().run_system_once(handle_placement_transform_actions).unwrap();
    app.world_mut().despawn(button);
    app.update();
}

fn preview(app: &App) -> Entity {
    app.world().resource::<EditorAuthoringSession>().placement_previews[&record(app).id]
}

#[test]
fn terrain_patch_numeric_edits_and_history_keep_real_source_parent_and_full_world_matrix() {
    for owned in [false, true] {
        for field in [PlacementTransformField::TranslationX, PlacementTransformField::TranslationY] {
            let (mut app, input, root, parent, source) = fixture(field, source_pose(), true, owned);
            if field == PlacementTransformField::TranslationY {
                let snapshot = {
                    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                    let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
                    record.yaw_degrees = 37.0; record.terrain_patch.as_mut().unwrap().cutout = None;
                    record.clone()
                };
                let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
                app.world_mut().entity_mut(root).insert((MapPlacementInstance::new(&map, &snapshot), pose(&snapshot)));
            }
            let before = record(&app).clone();
            commit(&mut app, input, if field == PlacementTransformField::TranslationX { 151.0 } else { 9.0 }, NumericInputCommitSource::Text);
            let after = record(&app).clone();
            assert_ne!(before, after);
            assert!(after.terrain_binding.is_none());
            assert!(!after.height_mode.follows_terrain());
            assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
            if !owned { assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden)); }
            crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
            for undo in [None, Some(true), Some(false)] {
                if let Some(undo) = undo {
                    let mut editor = app.world_mut().resource_mut::<MapEditorState>();
                    editor.undo_requested = undo; editor.redo_requested = !undo;
                    app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
                    app.update();
                }
                assert_eq!(record(&app), if undo == Some(true) { &before } else { &after });
                let visible = preview(&app);
                assert_eq!(app.world().get::<ChildOf>(visible).map(ChildOf::parent), parent);
                assert_eq!(app.world().get::<TerrainVolumeAuthorPreviewRoot>(visible).unwrap().0.terrain_id.as_ref(), Some(&source.id));
                assert_matrix(app.world().get::<GlobalTransform>(visible).unwrap().to_matrix(),
                    source.transform.as_transform().to_matrix() * pose(record(&app)).to_matrix());
                assert!(app.world().get::<feathrise_scene::TerrainPatchCollisionRequest>(visible).is_none());
                assert!(app.world().get::<avian3d::prelude::Collider>(visible).is_none());
            }
        }
    }
}

#[test]
fn terrain_patch_numeric_missing_parent_keeps_record_history_roots_and_visibility() {
    for owned in [false, true] {
        for scrub in [false, true] {
            let (mut app, input, root, _, _) = fixture(PlacementTransformField::TranslationY, source_pose(), false, owned);
            let before = record(&app).clone();
            let previews = app.world().resource::<EditorAuthoringSession>().placement_previews.clone();
            let transform = *app.world().get::<Transform>(root).unwrap();
            let visibility = *app.world().get::<Visibility>(root).unwrap();
            if scrub {
                app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(8.0) });
                app.update();
            } else { commit(&mut app, input, 8.0, NumericInputCommitSource::Text); }
            assert_eq!(record(&app), &before);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.revision, 7); assert!(session.undo.is_empty());
            assert_eq!(session.placement_previews, previews);
            assert_eq!(app.world().get::<Transform>(root), Some(&transform));
            assert_eq!(app.world().get::<Visibility>(root), Some(&visibility));
            assert!(app.world().resource::<MapEditorState>().status.contains("尚未就绪"));
        }
    }
}

#[test]
fn terrain_patch_scrub_source_loss_restores_only_owned_value_without_committing_history() {
    for outcome in [NumericInputCommitOutcome::Accepted, NumericInputCommitOutcome::Canceled] {
        for invalidation in 0..4 {
            for external in 0..3 {
                let (mut app, input, root, parent, _) = fixture(
                    PlacementTransformField::TranslationY, source_pose(), true, true);
                let before = record(&app).clone();
                let parent = parent.unwrap();
                let batch = app.world().get::<feathrise_scene::SubMapInstance>(parent).unwrap().batch;
                app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(8.0) });
                app.update();
                assert_eq!(record(&app).translation[1], 8.0, "先证明 Changed 已实际采用");
                assert_eq!(app.world().get::<Transform>(root).unwrap().translation.y, 8.0);
                let revision = app.world().resource::<EditorAuthoringSession>().revision;
                let previews = app.world().resource::<EditorAuthoringSession>().placement_previews.clone();
                if external != 0 {
                    let snapshot = {
                        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                        let current = &mut session.draft.as_mut().unwrap().placements.placements[0];
                        if external == 1 { current.translation[2] = 155.0; }
                        else { current.translation[1] = 9.0; }
                        current.clone()
                    };
                    app.world_mut().entity_mut(root).insert(pose(&snapshot));
                    app.world_mut().resource_mut::<MapEditorState>().dirty = true;
                }
                match invalidation {
                    0 => { app.world_mut().entity_mut(parent).remove::<feathrise_scene::TerrainObjectInstance>(); }
                    1 => { app.world_mut().entity_mut(batch).remove::<feathrise_scene::SubMapActive>(); }
                    2 => { app.world_mut().get_mut::<feathrise_scene::SubMapInstance>(parent).unwrap().generation += 1; }
                    _ => {
                        app.world_mut().entity_mut(root).remove::<ChildOf>();
                        app.world_mut().despawn(parent); // 保留会话根，证明父消失不能阻止作者责任撤销。
                    }
                }
                assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, revision);
                app.world_mut().write_message(NumericInputCommitted { entity: input,
                    value: NumericValue::F32(8.0), source: NumericInputCommitSource::Scrub,
                    outcome, changed: true, edited: true });
                app.update();
                let mut expected = before;
                if external == 1 { expected.translation[2] = 155.0; }
                if external == 2 { expected.translation[1] = 9.0; }
                assert_eq!(record(&app), &expected, "outcome={outcome:?}, source={invalidation}, external={external}");
                let session = app.world().resource::<EditorAuthoringSession>();
                assert!(session.undo.is_empty()); assert!(session.redo.is_empty());
                assert_eq!(session.placement_previews, previews, "取消不得补生或回收别的根");
                assert!(app.world().resource::<PlacementTransformInputState>().scrub.is_none());
                assert_eq!(app.world().get::<Transform>(root), Some(&pose(&expected)));
                if external != 2 {
                    assert!(app.world().get::<ChildOf>(root).is_none(), "恢复不得挂回失效源父");
                    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
                }
                if external != 0 { assert!(app.world().resource::<MapEditorState>().dirty); }
            }
        }
    }
}

#[test]
fn terrain_patch_scrub_canceled_message_reverts_live_value_even_when_changed_is_true() {
    let (mut app, input, root, parent, _) = fixture(
        PlacementTransformField::TranslationY, source_pose(), true, true);
    let before = record(&app).clone();
    app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(8.0) });
    app.update();
    assert_eq!(record(&app).translation[1], 8.0);
    app.world_mut().write_message(NumericInputCommitted { entity: input,
        value: NumericValue::F32(8.0), source: NumericInputCommitSource::Scrub,
        outcome: NumericInputCommitOutcome::Canceled, changed: true, edited: true });
    app.update();
    assert_eq!(record(&app), &before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), parent);
    assert_eq!(app.world().get::<Transform>(root), Some(&pose(&before)));
}

/// 外部写入即使未推进作者修订，也不能被下一档 Changed 收编进旧手势的完整历史。
fn assert_terrain_patch_scrub_external_write_cancels_on_next_changed(same_field: bool, late_changed: bool) {
    let (mut app, input, root, parent, source) = fixture(
        PlacementTransformField::TranslationY, source_pose(), true, true);
    let before = record(&app).clone();
    let previews = app.world().resource::<EditorAuthoringSession>().placement_previews.clone();
    app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(8.0) });
    app.update();
    assert_eq!(record(&app).translation[1], 8.0, "首档必须实际采用，不能把无效来源拒绝当作取消成功");
    assert_eq!(app.world().get::<Transform>(root).unwrap().translation.y, 8.0);
    assert_matrix(app.world().get::<GlobalTransform>(root).unwrap().to_matrix(),
        source.transform.as_transform().to_matrix() * pose(record(&app)).to_matrix());
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    assert!(app.world().resource::<PlacementTransformInputState>().scrub.is_some());
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());

    let external = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let current = &mut session.draft.as_mut().unwrap().placements.placements[0];
        if same_field { current.translation[1] = 9.0; }
        else { current.translation[2] = 155.0; }
        current.clone()
    };
    app.world_mut().entity_mut(root).insert(pose(&external));
    app.world_mut().resource_mut::<MapEditorState>().dirty = true;
    // 同修订派生/外部改值不能只靠 expected_revision 识别；来源与选择仍保持有效。
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, revision);
    assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), parent);
    assert!(app.world().get::<feathrise_scene::TerrainObjectInstance>(parent.unwrap()).is_some());

    app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(10.0) });
    app.update();
    let after_changed = record(&app).clone();
    let canceled_on_changed = app.world().resource::<PlacementTransformInputState>().scrub.is_none();
    let after_late_changed = late_changed.then(|| {
        // 同一次物理按住仍可能产生后续档；事务清空不能被当成允许重新开始的凭证。
        app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(11.0) });
        app.update();
        record(&app).clone()
    });
    // 仍送达真实松手消息，证明下一档与最后提交均不能重新接管后来作者的值。
    commit(&mut app, input, if late_changed { 11.0 } else { 10.0 }, NumericInputCommitSource::Scrub);

    let mut expected = before;
    if same_field { expected.translation[1] = 9.0; }
    else { expected.translation[2] = 155.0; }
    assert_eq!(after_changed, expected, "后续 Changed 必须取消旧笔，只撤回仍归本笔拥有的字段");
    assert!(canceled_on_changed, "不能把外部记录重新记为本事务 latest_record");
    if let Some(after_late_changed) = after_late_changed {
        assert_eq!(after_late_changed, expected, "取消后的同次拖动必须持续拒收，直到真实 Committed 结束这笔输入");
    }
    assert_eq!(record(&app), &expected, "迟到松手不得把外部改值纳入旧笔的完整撤销快照");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert!(session.redo.is_empty());
    assert_eq!(session.placement_previews, previews, "冲突取消只恢复已有预览，不补生或回收模型");
    assert!(app.world().resource::<PlacementTransformInputState>().scrub.is_none());
    assert!(app.world().resource::<MapEditorState>().dirty, "外部脏责任不能随旧手势取消而清除");
    assert_eq!(app.world().get::<Transform>(root), Some(&pose(&expected)));
    assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), parent);
    assert_matrix(app.world().get::<GlobalTransform>(root).unwrap().to_matrix(),
        source.transform.as_transform().to_matrix() * pose(&expected).to_matrix());
    if late_changed {
        app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(12.0) });
        app.update();
        assert_eq!(record(&app).translation[1], 12.0, "松手之后的新独立手势应重新允许正常编辑");
        assert!(app.world().resource::<PlacementTransformInputState>().scrub.is_some());
        commit(&mut app, input, 12.0, NumericInputCommitSource::Scrub);
        let session = app.world().resource::<EditorAuthoringSession>();
        let [crate::authoring::EditorCommand::PlacementChanged { before, after, .. }] = session.undo.as_slice()
            else { panic!("新独立手势应且仅应形成一条完整历史"); };
        let mut expected_after = expected.clone();
        expected_after.translation[1] = 12.0;
        assert_eq!(before, &expected, "新笔的撤销起点必须保留此前外部改值");
        assert_eq!(after, &expected_after);
        assert_eq!(record(&app), &expected_after);
        assert!(session.redo.is_empty());
        assert!(app.world().resource::<PlacementTransformInputState>().scrub.is_none());
    }
}

#[test]
fn terrain_patch_scrub_next_changed_preserves_external_other_field_without_history() {
    assert_terrain_patch_scrub_external_write_cancels_on_next_changed(false, false);
}

#[test]
fn terrain_patch_scrub_next_changed_preserves_external_same_field_without_history() {
    assert_terrain_patch_scrub_external_write_cancels_on_next_changed(true, false);
}

#[test]
fn terrain_patch_scrub_canceled_gesture_rejects_late_changed_with_external_other_field() {
    assert_terrain_patch_scrub_external_write_cancels_on_next_changed(false, true);
}

#[test]
fn terrain_patch_scrub_canceled_gesture_rejects_late_changed_with_external_same_field() {
    assert_terrain_patch_scrub_external_write_cancels_on_next_changed(true, true);
}

fn set_scrub_context_interruption(app: &mut App, interruption: usize, active: bool) {
    match interruption {
        0 => app.world_mut().resource_mut::<MapEditorState>().open = !active,
        1 => app.world_mut().resource_mut::<EditorAuthoringSession>().saving = active,
        _ if active => {
            let mut state = crate::authoring::mesh_edit::MeshEditState::default();
            state.activate_for_test(); // 这里只验证真实输入系统的编辑模式门，不声称驱动了网格进入流程。
            app.insert_resource(state);
        }
        _ => { app.world_mut().remove_resource::<crate::authoring::mesh_edit::MeshEditState>(); }
    }
}

#[test]
fn terrain_patch_scrub_context_interruption_waits_for_real_commit_before_a_new_gesture() {
    for interruption in 0..3 {
        let (mut app, input, root, parent, _) = fixture(
            PlacementTransformField::TranslationY, source_pose(), true, true);
        let before = record(&app).clone();
        app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(8.0) });
        app.update();
        assert_eq!(record(&app).translation[1], 8.0);
        set_scrub_context_interruption(&mut app, interruption, true);
        app.update();
        assert_eq!(record(&app), &before, "上下文中断须撤回仍拥有的字段：{interruption}");
        assert!(app.world().resource::<PlacementTransformInputState>().scrub.is_none());
        assert_eq!(app.world().resource::<PlacementTransformInputState>().rejected_scrub, Some(input));

        set_scrub_context_interruption(&mut app, interruption, false);
        app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(10.0) });
        app.update();
        assert_eq!(record(&app), &before, "上下文恢复也不是同次物理按住重新获权的证明");
        // 松手可发生在界面再次关闭、保存或网格编辑时，早退清消息也必须释放拒收标记。
        set_scrub_context_interruption(&mut app, interruption, true);
        commit(&mut app, input, 10.0, NumericInputCommitSource::Scrub);
        assert!(app.world().resource::<PlacementTransformInputState>().rejected_scrub.is_none());
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        set_scrub_context_interruption(&mut app, interruption, false);
        app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(12.0) });
        app.update();
        commit(&mut app, input, 12.0, NumericInputCommitSource::Scrub);
        assert_eq!(record(&app).translation[1], 12.0);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        assert_eq!(app.world().get::<Transform>(root), Some(&pose(record(&app))));
        assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), parent);
    }
}

#[test]
fn terrain_patch_scrub_rejected_or_despawned_input_does_not_block_another_field() {
    for despawn in [false, true] {
        let (mut app, input, root, _, _) = fixture(
            PlacementTransformField::TranslationY, source_pose(), true, true);
        app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(8.0) });
        app.update();
        let external = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let current = &mut session.draft.as_mut().unwrap().placements.placements[0];
            current.translation[1] = 9.0;
            current.clone()
        };
        app.world_mut().entity_mut(root).insert(pose(&external));
        app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(10.0) });
        app.update();
        assert_eq!(record(&app), &external);
        assert_eq!(app.world().resource::<PlacementTransformInputState>().rejected_scrub, Some(input));
        if despawn { app.world_mut().despawn(input); }
        let next_input = app.world_mut().spawn(PlacementTransformField::TranslationY).id();
        app.world_mut().write_message(NumericInputChanged { entity: next_input, value: NumericValue::F32(12.0) });
        app.update();
        assert_eq!(record(&app).translation[1], 12.0, "旧根拒收不能扩大成所有字段的永久禁用");
        commit(&mut app, next_input, 12.0, NumericInputCommitSource::Scrub);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        if !despawn {
            app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(11.0) });
            app.update();
            assert_eq!(record(&app).translation[1], 12.0, "别的字段完成新笔也不能替旧捕获结清拒收");
        }
        commit(&mut app, input, 11.0, NumericInputCommitSource::Scrub);
        assert_eq!(record(&app).translation[1], 12.0);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        assert!(app.world().resource::<PlacementTransformInputState>().rejected_scrub.is_none());
    }
}

#[test]
fn terrain_patch_scrub_interrupted_after_source_loss_restores_author_value_and_keeps_external_field() {
    let (mut app, input, root, parent, _) = fixture(
        PlacementTransformField::TranslationY, source_pose(), true, true);
    let mut expected = record(&app).clone();
    app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(8.0) });
    app.update();
    assert_eq!(record(&app).translation[1], 8.0);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements[0].translation[2] = 155.0;
        session.revision += 1; // 外部作者动作优先，恢复不能覆盖其余字段或清除dirty。
    }
    expected.translation[2] = 155.0;
    app.world_mut().entity_mut(parent.unwrap()).remove::<feathrise_scene::TerrainObjectInstance>();
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert_eq!(record(&app), &expected);
    assert_eq!(app.world().get::<Transform>(root), Some(&pose(&expected)));
    assert!(app.world().get::<ChildOf>(root).is_none());
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(app.world().resource::<MapEditorState>().dirty);
    assert!(app.world().resource::<PlacementTransformInputState>().scrub.is_none());
}

#[test]
fn ordinary_numeric_update_preserves_existing_group_parent() {
    let (mut app, input, root, _, _) = fixture(PlacementTransformField::TranslationY, source_pose(), false, true);
    let snapshot = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
        record.terrain_patch = None; record.clone()
    };
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    let group = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited, avian3d::prelude::RigidBodyDisabled)).id();
    app.world_mut().entity_mut(root).insert((MapPlacementInstance::new(&map, &snapshot), ChildOf(group)));
    commit(&mut app, input, 8.0, NumericInputCommitSource::Text);
    assert_eq!(record(&app).translation[1], 8.0);
    assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), Some(group));
    assert!(app.world().get::<avian3d::prelude::RigidBodyDisabled>(group).is_some());
}

#[test]
fn terrain_patch_panel_uses_source_local_limits_and_explains_coordinate_space() {
    let (mut app, input, _, _, _) = fixture(PlacementTransformField::TranslationX, source_pose(), true, true);
    let field = PlacementTransformField::TranslationX;
    app.world_mut().entity_mut(input).insert((
        feathrise_editor_ui::NumericInputConfig(field.spec(None)),
        feathrise_editor_ui::NumericInputValue(NumericValue::F32(0.0)),
        field.scrub_config(&feathrise_editor_core::EditorSettings::default(), None),
    ));
    let summary = app.world_mut().spawn((MapEditorPlacementTransformSummary, Text::new(""))).id();
    app.world_mut().run_system_once(sync_placement_transform_panel).unwrap();
    let bounds = Rect::from_corners(Vec2::splat(100.0), Vec2::splat(200.0));
    assert_eq!(app.world().get::<feathrise_editor_ui::NumericInputConfig>(input).unwrap().0, field.spec(Some(bounds)));
    assert_eq!(app.world().get::<feathrise_editor_ui::NumericInputScrubConfig>(input).unwrap(),
        &field.scrub_config(&feathrise_editor_core::EditorSettings::default(), Some(bounds)));
    assert!(app.world().get::<Text>(summary).unwrap().contains("源局部"));
}

#[test]
fn terrain_patch_cutout_button_uses_explicit_source_and_follow_action_cannot_change_role() {
    let (mut app, _, _, parent, _) = fixture(PlacementTransformField::TranslationX, source_pose(), true, true);
    let before = record(&app).clone();
    action(&mut app, PlacementTransformAction::ToggleTerrainCutout);
    assert!(record(&app).terrain_patch.as_ref().unwrap().cutout.is_none());
    assert_eq!(app.world().get::<ChildOf>(preview(&app)).map(ChildOf::parent), parent);
    action(&mut app, PlacementTransformAction::ToggleTerrainCutout);
    assert_eq!(record(&app), &before);
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    action(&mut app, PlacementTransformAction::FollowTerrain);
    assert_eq!(record(&app), &before);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, revision);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 2);
}

#[test]
fn terrain_role_toggle_preserves_representable_geometry_and_binding_identity() {
    let source = TerrainObjectTransform { translation: [-130.0, 11.0, 75.0], yaw_degrees: 90.0,
        scale: [2.0, 3.0, 5.0], ..Default::default() };
    let (mut app, _, root, parent, definition) = fixture(PlacementTransformField::TranslationX, source, true, true);
    let world = source.as_transform().to_matrix() * pose(record(&app)).to_matrix();
    action(&mut app, PlacementTransformAction::ToggleTerrainPatch);
    assert!(record(&app).terrain_patch.is_none());
    assert_eq!(record(&app).terrain_binding.as_ref().unwrap().terrain_id, definition.id);
    let ordinary = preview(&app);
    assert_ne!(ordinary, root);
    assert!(app.world().get::<ChildOf>(ordinary).is_none());
    assert!(app.world().get::<TerrainVolumeAuthorPreviewRoot>(ordinary).is_none());
    assert_matrix(app.world().get::<GlobalTransform>(ordinary).unwrap().to_matrix(), world);
    action(&mut app, PlacementTransformAction::ToggleTerrainPatch);
    assert!(record(&app).terrain_binding.is_none());
    assert_eq!(record(&app).terrain_patch.as_ref().unwrap().terrain_id.as_ref(), Some(&definition.id));
    assert_eq!(app.world().get::<ChildOf>(preview(&app)).map(ChildOf::parent), parent);
    assert_matrix(app.world().get::<GlobalTransform>(preview(&app)).unwrap().to_matrix(), world);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 2);
}

#[test]
fn terrain_role_toggle_rejects_unrepresentable_world_matrix_without_any_partial_changes() {
    for source in [source_pose(), TerrainObjectTransform { scale: [2.0, 1.0, 5.0], ..Default::default() }] {
        let (mut app, _, root, parent, _) = fixture(PlacementTransformField::TranslationY, source, true, true);
        let before = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
            record.yaw_degrees = 37.0; record.terrain_patch.as_mut().unwrap().cutout = None;
            record.clone()
        };
        let previews = app.world().resource::<EditorAuthoringSession>().placement_previews.clone();
        let revision = app.world().resource::<EditorAuthoringSession>().revision;
        action(&mut app, PlacementTransformAction::ToggleTerrainPatch);
        assert_eq!(record(&app), &before);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, revision); assert!(session.undo.is_empty());
        assert_eq!(session.placement_previews, previews);
        assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), parent);
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
        let status = &app.world().resource::<MapEditorState>().status;
        assert!(status.contains("剪切") || status.contains("俯仰或翻滚"), "{status}");
    }
}

#[test]
fn terrain_role_enable_missing_parent_preserves_ordinary_record_and_preview() {
    let source = TerrainObjectTransform { translation: [10.0, 4.0, -8.0], ..Default::default() };
    let (mut app, _, root, _, definition) = fixture(PlacementTransformField::TranslationX, source, false, true);
    let before = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
        record.terrain_patch = None;
        record.terrain_binding = Some(feathrise_map_content::MapTerrainBinding::capture(definition.id.clone(), source,
            TerrainObjectTransform { translation: record.translation, yaw_degrees: record.yaw_degrees,
                scale: record.scale, ..Default::default() }).unwrap());
        record.clone()
    };
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    app.world_mut().entity_mut(root).insert(MapPlacementInstance::new(&map, &before));
    let previews = app.world().resource::<EditorAuthoringSession>().placement_previews.clone();
    action(&mut app, PlacementTransformAction::ToggleTerrainPatch);
    assert_eq!(record(&app), &before);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 7); assert!(session.undo.is_empty());
    assert_eq!(session.placement_previews, previews);
    assert!(app.world().get_entity(root).is_ok());
    assert!(app.world().get::<ChildOf>(root).is_none());
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
}

#[test]
fn terrain_role_disable_rejects_missing_or_unadopted_source_before_replacing_preview() {
    for stale in [false, true] {
        let source = TerrainObjectTransform { translation: [10.0, 4.0, -8.0], ..Default::default() };
        let (mut app, _, root, parent, _) = fixture(PlacementTransformField::TranslationX, source, stale, true);
        if stale {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let source = &mut Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0];
            source.transform.translation[0] += 1.0; // 作者已换姿态，已采用父仍是上一完整代。
        }
        let before = record(&app).clone();
        let previews = app.world().resource::<EditorAuthoringSession>().placement_previews.clone();
        let transform = *app.world().get::<Transform>(root).unwrap();
        action(&mut app, PlacementTransformAction::ToggleTerrainPatch);
        assert_eq!(record(&app), &before);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, 7); assert!(session.undo.is_empty());
        assert_eq!(session.placement_previews, previews);
        assert_eq!(app.world().get::<Transform>(root), Some(&transform));
        assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), parent);
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
        assert!(app.world().resource::<MapEditorState>().status.contains("三维地形设置失败"));
    }
}
