//! 真实元素输入链的预览所有权：旧地址不能隐藏外部根，也不能阻断正式根回退或在取消时串图恢复。

use super::*;
use crate::ui::placement_water::{
    PlacementInstanceIndex, register_placement_instance_root, unregister_placement_instance_root,
};
use feathrise_scene_content::SceneId;
use feathrise_scene::ScenePlacementInstance;

#[derive(Clone, Copy, Debug)]
enum StaleAddress {
    Dead,
    OtherScene,
    OtherId,
    MissingIdentity,
}

/// 父子和姿态由外部拥有；显隐隔离不能顺便改写它们或回收整棵树。
struct RootSnapshot {
    transform: Transform,
    parent: Entity,
    child: Entity,
}

fn attach_hierarchy(app: &mut App, root: Entity) -> RootSnapshot {
    let transform = *app.world().get::<Transform>(root).unwrap();
    let parent = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id();
    app.world_mut().entity_mut(root).insert(ChildOf(parent));
    let child = app.world_mut().spawn((
        Transform::from_xyz(0.25, 0.5, 0.75), Visibility::Inherited, ChildOf(root),
    )).id();
    RootSnapshot { transform, parent, child }
}

fn assert_root_untouched(app: &App, root: Entity, snapshot: &RootSnapshot, visibility: Visibility) {
    assert_eq!(app.world().get::<Transform>(root), Some(&snapshot.transform));
    assert_eq!(app.world().get::<Visibility>(root), Some(&visibility));
    assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), snapshot.parent);
    assert_eq!(app.world().get::<ChildOf>(snapshot.child).unwrap().parent(), root);
    assert_eq!(app.world().get::<Visibility>(snapshot.child), Some(&Visibility::Inherited));
}

fn assert_no_author_change(app: &App, before: &ScenePlacementRecord, revision: u64) {
    assert_eq!(current(app), before);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision);
    assert!(session.undo.is_empty());
    assert!(session.redo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

fn tracked_app() -> (App, Entity, Entity) {
    let (mut app, root, node) = app();
    crate::ui::scene_objects::install_visibility_observers_for_tests(&mut app);
    let instance = app.world().get::<ScenePlacementInstance>(root).unwrap().clone();
    app.world_mut().entity_mut(root).remove::<ScenePlacementInstance>();
    app.world_mut().entity_mut(root).insert(instance); // 真实Remove/Add登记票据，不向资源私有表塞测试值。
    (app, root, node)
}

fn begin_preview(app: &mut App, node: Entity) -> Entity {
    enter(app, node);
    request(app, MeshEditAction::Move);
    fixture::step(app, node, Vec2::ZERO);
    fixture::step(app, node, Vec2::new(0.03, 0.01));
    let state = app.world().resource::<MeshEditState>();
    let movement = state.movement.as_ref().expect("实际Move输入必须持有元素手势");
    assert!(movement.valid);
    assert_ne!(movement.preview, movement.before, "必须实际生成变形候选，不能只测空预览");
    let ghost = state.ghost.expect("正式预览入口必须建立无碰撞幽灵");
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Inherited));
    ghost
}

fn cancel_preview(app: &mut App, node: Entity, ghost: Entity) {
    request(app, MeshEditAction::Cancel);
    fixture::step(app, node, Vec2::new(0.03, 0.01));
    assert!(!app.world().resource::<MeshEditState>().moving());
    assert!(app.world().resource::<MeshEditState>().hidden_roots.is_empty());
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden));
}

fn replace_address_owner(app: &mut App, root: Entity, stale: StaleAddress, map: &SceneId, before: &ScenePlacementRecord) {
    match stale {
        StaleAddress::Dead => { app.world_mut().despawn(root); }
        StaleAddress::OtherScene => {
            let other_scene = SceneId::parse("MAP-mesh-preview-other-owner").unwrap();
            app.world_mut().entity_mut(root).insert(ScenePlacementInstance::new(&other_scene, before));
        }
        StaleAddress::OtherId => {
            let mut other = before.clone();
            other.id = "mesh-preview-other-owner".into();
            app.world_mut().entity_mut(root).insert(ScenePlacementInstance::new(map, &other));
        }
        StaleAddress::MissingIdentity => {
            app.world_mut().entity_mut(root).remove::<ScenePlacementInstance>();
        }
    }
}

fn stale_preview_falls_back_to_formal_root(stale: StaleAddress) {
    let (mut app, old_address, node) = app();
    let before = current(&app).clone();
    let (map, revision) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.draft.as_ref().unwrap().scene_id.clone(), session.revision)
    };
    let external = attach_hierarchy(&mut app, old_address);
    replace_address_owner(&mut app, old_address, stale, &map, &before);
    // 映射故意保留旧地址；正式根必须通过真实Add观察登记，不能靠测试伪造索引内容。
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews[&before.id], old_address);
    app.add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    let formal = app.world_mut().spawn((
        external.transform, Visibility::Inherited, ScenePlacementInstance::new(&map, &before),
    )).id();
    let formal_snapshot = attach_hierarchy(&mut app, formal);
    assert!(app.world().resource::<PlacementInstanceIndex>().membership_revision() > 0);

    let ghost = begin_preview(&mut app, node);
    assert_root_untouched(&app, formal, &formal_snapshot, Visibility::Hidden);
    if matches!(stale, StaleAddress::Dead) {
        assert!(app.world().get_entity(old_address).is_err());
    } else {
        assert_root_untouched(&app, old_address, &external, Visibility::Inherited);
    }
    assert_no_author_change(&app, &before, revision);

    cancel_preview(&mut app, node, ghost);
    assert_root_untouched(&app, formal, &formal_snapshot, Visibility::Inherited);
    if !matches!(stale, StaleAddress::Dead) {
        assert_root_untouched(&app, old_address, &external, Visibility::Inherited);
    }
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn dead_preview_address_does_not_block_the_real_formal_root_fallback() {
    stale_preview_falls_back_to_formal_root(StaleAddress::Dead);
}

#[test]
fn other_scene_preview_address_is_untouched_and_falls_back_to_the_real_formal_root() {
    stale_preview_falls_back_to_formal_root(StaleAddress::OtherScene);
}

#[test]
fn other_id_preview_address_is_untouched_and_falls_back_to_the_real_formal_root() {
    stale_preview_falls_back_to_formal_root(StaleAddress::OtherId);
}

#[test]
fn preview_address_without_instance_identity_is_untouched_and_uses_the_real_formal_root() {
    stale_preview_falls_back_to_formal_root(StaleAddress::MissingIdentity);
}

fn cancel_preserves_reassigned_root(stale: StaleAddress) {
    let (mut app, root, node) = app();
    let before = current(&app).clone();
    let (map, revision) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.draft.as_ref().unwrap().scene_id.clone(), session.revision)
    };
    let mut external = attach_hierarchy(&mut app, root);
    let ghost = begin_preview(&mut app, node);
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    // 隐藏后发生外部接管；旧hidden_roots地址仍在，恢复时必须重新复核真实map和ID。
    replace_address_owner(&mut app, root, stale, &map, &before);
    if !matches!(stale, StaleAddress::Dead) {
        external.transform = Transform::from_xyz(91.0, -8.0, 53.0)
            .with_rotation(Quat::from_rotation_z(0.3)).with_scale(Vec3::new(3.0, 2.0, 1.0));
        app.world_mut().entity_mut(root).insert((external.transform, Visibility::Hidden));
    }
    cancel_preview(&mut app, node, ghost);
    if matches!(stale, StaleAddress::Dead) {
        assert!(app.world().get_entity(root).is_err());
    } else {
        assert_root_untouched(&app, root, &external, Visibility::Hidden);
    }
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn cancel_does_not_restore_a_root_reassigned_to_another_scene_with_the_same_id() {
    cancel_preserves_reassigned_root(StaleAddress::OtherScene);
}

#[test]
fn cancel_does_not_restore_a_root_reassigned_to_another_id() {
    cancel_preserves_reassigned_root(StaleAddress::OtherId);
}

#[test]
fn cancel_does_not_restore_a_root_whose_instance_identity_was_removed() {
    cancel_preserves_reassigned_root(StaleAddress::MissingIdentity);
}

#[test]
fn cancel_does_not_recreate_a_root_deleted_during_the_element_preview() {
    cancel_preserves_reassigned_root(StaleAddress::Dead);
}

#[test]
fn cancel_restores_the_current_owned_preview_without_changing_its_hierarchy_or_record() {
    let (mut app, root, node) = app();
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    let snapshot = attach_hierarchy(&mut app, root);
    let ghost = begin_preview(&mut app, node);
    assert_root_untouched(&app, root, &snapshot, Visibility::Hidden);
    cancel_preview(&mut app, node, ghost);
    assert_root_untouched(&app, root, &snapshot, Visibility::Inherited);
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn scene_switch_discards_the_preview_without_reshowing_the_old_scenes_hidden_root() {
    let (mut app, root, node) = app();
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    let snapshot = attach_hierarchy(&mut app, root);
    let ghost = begin_preview(&mut app, node);
    let next_scene = SceneId::parse("MAP-mesh-preview-next-map").unwrap();
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().scene_id = next_scene.clone();
    app.world_mut().resource_mut::<SceneEditorState>().selected_scene_id = Some(next_scene.clone());
    app.world_mut().resource_mut::<crate::resources::SceneEditorViewportState>().framed_scene_id = Some(next_scene);
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    assert!(!app.world().resource::<MeshEditState>().active());
    assert_root_untouched(&app, root, &snapshot, Visibility::Hidden);
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden));
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn preview_address_without_visibility_does_not_block_the_formal_root_fallback() {
    let (mut app, address, node) = app();
    let before = current(&app).clone();
    let (map, revision) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.draft.as_ref().unwrap().scene_id.clone(), session.revision)
    };
    let transform = *app.world().get::<Transform>(address).unwrap();
    app.world_mut().entity_mut(address).remove::<Visibility>();
    app.add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    let formal = app.world_mut().spawn((transform, Visibility::Inherited,
        ScenePlacementInstance::new(&map, &before))).id();
    let snapshot = attach_hierarchy(&mut app, formal);
    let ghost = begin_preview(&mut app, node);
    assert_root_untouched(&app, formal, &snapshot, Visibility::Hidden);
    assert!(app.world().get::<Visibility>(address).is_none());
    cancel_preview(&mut app, node, ghost);
    assert_root_untouched(&app, formal, &snapshot, Visibility::Inherited);
    assert!(app.world().get::<Visibility>(address).is_none());
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn cancel_does_not_recreate_visibility_removed_after_the_preview_hid_the_owned_root() {
    let (mut app, root, node) = app();
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    let ghost = begin_preview(&mut app, node);
    app.world_mut().entity_mut(root).remove::<Visibility>();
    cancel_preview(&mut app, node, ghost);
    assert!(app.world().get::<Visibility>(root).is_none(),
        "显隐组件移除后，旧手势不能重新为实体补回该组件");
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn cancel_preserves_an_external_visibility_override_after_the_preview_hid_the_owned_root() {
    let (mut app, root, node) = app();
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut().entity_mut(root).insert(Visibility::Visible);
    let ghost = begin_preview(&mut app, node);
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    app.world_mut().entity_mut(root).insert(Visibility::Inherited);
    cancel_preview(&mut app, node, ghost);
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited),
        "外部已接管显隐时，不能以冻结的Visible覆盖新值");
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn formal_only_commit_then_restore_keeps_the_old_root_hidden_by_the_new_author_preview() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::authoring::placement_preview_ownership::components::PlacementPreviewSuppression;

    let (mut app, formal, node) = tracked_app();
    let before = current(&app).clone();
    let (map, revision) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.placement_previews.clear(); // 首次编辑只有正式根，必须在确认帧补生独立作者预览。
        (session.draft.as_ref().unwrap().scene_id.clone(), session.revision)
    };
    app.add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    app.world_mut().entity_mut(formal).remove::<ScenePlacementInstance>();
    app.world_mut().entity_mut(formal).insert(ScenePlacementInstance::new(&map, &before));
    let original = attach_hierarchy(&mut app, formal);
    let ghost = begin_preview(&mut app, node);
    let after = app.world().resource::<MeshEditState>().movement.as_ref().unwrap().preview.clone();
    assert_root_untouched(&app, formal, &original, Visibility::Hidden);

    // 正式Enter输入在同一次handle_input内先commit，再restore；新root和claim此时都仍在Commands队列。
    press(&mut app, KeyCode::Enter);
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    let preview = app.world().resource::<EditorAuthoringSession>().placement_previews[&before.id];
    assert_ne!(preview, formal, "新作者记录不能继续归旧正式树拥有");
    assert_eq!(current(&app), &after);
    assert_root_untouched(&app, formal, &original, Visibility::Hidden);
    assert_eq!(app.world().get::<PlacementPreviewSuppression>(formal).unwrap().owner, preview);
    assert!(app.world().get::<ScenePlacementInstance>(formal).unwrap()
        .matches_record(&map, &before, &before.water_surfaces));
    assert!(app.world().get::<ScenePlacementInstance>(preview).unwrap()
        .matches_record(&map, &after, &after.water_surfaces));
    assert_eq!(app.world().get::<Visibility>(preview), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden));
    assert!(!app.world().resource::<MeshEditState>().moving());
    assert!(app.world().resource::<MeshEditState>().hidden_roots.is_empty());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision + 1);
    assert_eq!(session.undo.len(), 1);
    assert!(matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. }
        if old == &before && new == &after));
    assert!(app.world().resource::<SceneEditorState>().dirty);

    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    assert_root_untouched(&app, formal, &original, Visibility::Hidden);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);

    // 冷撤回不发布新正式批次：幽灵的临时Hidden必须已转交回真正的原显隐基线。
    app.world_mut().run_system_once(move |mut commands: Commands| {
        crate::authoring::placement_preview_ownership::withdraw_cold_preview(&mut commands, preview);
        commands.entity(preview).despawn();
    }).unwrap();
    assert_root_untouched(&app, formal, &original, Visibility::Inherited);
    assert!(app.world().get::<PlacementPreviewSuppression>(formal).is_none());
    assert_eq!(current(&app), &after);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn queued_restore_rechecks_external_identity_record_and_visibility_changes_before_writing() {
    use bevy::ecs::system::RunSystemOnce;

    // 外部命令先排队、但尚未执行时，EditWorld的普通Query仍看见旧身份和本手势的Hidden。
    for change in 0..7 {
        let (mut app, root, node) = app();
        let before = current(&app).clone();
        let ghost = begin_preview(&mut app, node);
        app.world_mut().run_system_once(move |
            mut edit: super::super::preview::EditWorld,
            mut state: ResMut<MeshEditState>,
            session: Res<EditorAuthoringSession>,
        | {
            let before = before.clone();
            edit.commands.queue(move |world: &mut World| match change {
                0 => {
                    let map = SceneId::parse("MAP-mesh-restore-late-owner").unwrap();
                    world.entity_mut(root).insert(ScenePlacementInstance::new(&map, &before));
                }
                1 => {
                    let map = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
                    let mut record = before;
                    record.id = "late-reassigned-mesh".into();
                    world.entity_mut(root).insert(ScenePlacementInstance::new(&map, &record));
                }
                2 => { world.resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placements.placements.clear(); }
                3 => {
                    let mut session = world.resource_mut::<EditorAuthoringSession>();
                    std::sync::Arc::make_mut(&mut session.draft.as_mut().unwrap().placements.placements[0].editor).hidden = true;
                }
                4 => { world.entity_mut(root).remove::<Visibility>(); }
                5 => { world.entity_mut(root).insert(Visibility::Visible); }
                6 => {
                    world.resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().scene_id =
                        SceneId::parse("MAP-mesh-restore-late-switch").unwrap();
                }
                _ => unreachable!(),
            });
            edit.restore(&mut state, &session);
        }).unwrap();
        assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden));
        let expected = match change { 4 => None, 5 => Some(&Visibility::Visible), _ => Some(&Visibility::Hidden) };
        assert_eq!(app.world().get::<Visibility>(root), expected,
            "排队恢复不能覆盖命令执行前发生的外部接管，case={change}");
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
}

#[test]
fn commit_restore_keeps_newer_external_visibility_as_the_cold_withdraw_base() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::authoring::placement_preview_ownership::components::PlacementPreviewSuppression;

    let (mut app, formal, node) = tracked_app();
    let before = current(&app).clone();
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews.clear();
    app.add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    app.world_mut().entity_mut(formal).remove::<ScenePlacementInstance>();
    app.world_mut().entity_mut(formal).insert(ScenePlacementInstance::new(&map, &before));
    let original = attach_hierarchy(&mut app, formal);
    begin_preview(&mut app, node);

    app.world_mut().run_system_once(move |
        mut edit: super::super::preview::EditWorld,
        mut state: ResMut<MeshEditState>,
        mut session: ResMut<EditorAuthoringSession>,
        mut editor: ResMut<SceneEditorState>,
    | {
        // Query仍看见幽灵Hidden，外部显隐先落地、再由本次commit的claim重新遮蔽。
        edit.commands.entity(formal).insert(Visibility::Visible);
        let target = state.target.as_ref().unwrap().clone();
        let after = state.movement.as_ref().unwrap().preview.clone();
        assert!(edit.commit(&target, &after, &mut session, &mut editor).unwrap());
        edit.restore(&mut state, &session);
    }).unwrap();
    let preview = app.world().resource::<EditorAuthoringSession>().placement_previews[&before.id];
    assert_root_untouched(&app, formal, &original, Visibility::Hidden);
    assert_eq!(app.world().get::<PlacementPreviewSuppression>(formal).unwrap().original_visibility, Visibility::Visible);
    app.world_mut().run_system_once(move |mut commands: Commands| {
        crate::authoring::placement_preview_ownership::withdraw_cold_preview(&mut commands, preview);
        commands.entity(preview).despawn();
    }).unwrap();
    assert_root_untouched(&app, formal, &original, Visibility::Visible);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn same_hidden_external_insert_during_mesh_preview_is_preserved_on_real_cancel() {
    let (mut app, root, node) = tracked_app();
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    let original = attach_hierarchy(&mut app, root);
    let ghost = begin_preview(&mut app, node);
    app.world_mut().entity_mut(root).insert(Visibility::Hidden); // 值相同，但是真实外部Insert意图。
    cancel_preview(&mut app, node, ghost);
    assert_root_untouched(&app, root, &original, Visibility::Hidden);
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn same_hidden_external_insert_survives_real_commit_claim_and_cold_withdraw() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::authoring::placement_preview_ownership::components::PlacementPreviewSuppression;

    let (mut app, formal, node) = tracked_app();
    let before = current(&app).clone();
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews.clear();
    app.add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    app.world_mut().entity_mut(formal).remove::<ScenePlacementInstance>();
    app.world_mut().entity_mut(formal).insert(ScenePlacementInstance::new(&map, &before));
    let original = attach_hierarchy(&mut app, formal);
    begin_preview(&mut app, node);
    let after = app.world().resource::<MeshEditState>().movement.as_ref().unwrap().preview.clone();
    app.world_mut().entity_mut(formal).insert(Visibility::Hidden);

    press(&mut app, KeyCode::Enter);
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    let preview = app.world().resource::<EditorAuthoringSession>().placement_previews[&before.id];
    assert_ne!(preview, formal);
    assert_root_untouched(&app, formal, &original, Visibility::Hidden);
    assert_eq!(app.world().get::<PlacementPreviewSuppression>(formal).unwrap().original_visibility, Visibility::Hidden);
    assert_eq!(current(&app), &after);
    app.world_mut().run_system_once(move |mut commands: Commands| {
        crate::authoring::placement_preview_ownership::withdraw_cold_preview(&mut commands, preview);
        commands.entity(preview).despawn();
    }).unwrap();
    assert_root_untouched(&app, formal, &original, Visibility::Hidden);
    assert!(app.world().get::<PlacementPreviewSuppression>(formal).is_none());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn queued_same_hidden_external_insert_revokes_mesh_restore_before_the_command_writes() {
    use bevy::ecs::system::RunSystemOnce;

    let (mut app, root, node) = tracked_app();
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    let original = attach_hierarchy(&mut app, root);
    let ghost = begin_preview(&mut app, node);
    app.world_mut().run_system_once(move |
        mut edit: super::super::preview::EditWorld,
        mut state: ResMut<MeshEditState>,
        session: Res<EditorAuthoringSession>,
    | {
        edit.commands.entity(root).insert(Visibility::Hidden);
        edit.restore(&mut state, &session); // 普通Query仍见旧Hidden；票据必须等实际队列执行时再复核。
    }).unwrap();
    assert_root_untouched(&app, root, &original, Visibility::Hidden);
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden));
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn queued_external_visible_before_own_hide_is_not_overwritten_by_the_mesh_ghost() {
    use bevy::ecs::system::RunSystemOnce;

    let (mut app, root, node) = tracked_app();
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    let original = attach_hierarchy(&mut app, root);
    enter(&mut app, node);
    request(&mut app, MeshEditAction::Move);
    fixture::step(&mut app, node, Vec2::ZERO);
    app.world_mut().run_system_once(move |
        mut edit: super::super::preview::EditWorld,
        mut state: ResMut<MeshEditState>,
        session: Res<EditorAuthoringSession>,
    | {
        edit.commands.entity(root).insert(Visibility::Visible);
        let record = state.target.as_ref().unwrap().record.clone();
        edit.show(&mut state, &session, &record).unwrap();
    }).unwrap();
    let ghost = app.world().resource::<MeshEditState>().ghost.unwrap();
    assert_root_untouched(&app, root, &original, Visibility::Visible);
    cancel_preview(&mut app, node, ghost);
    assert_root_untouched(&app, root, &original, Visibility::Visible);
    assert_no_author_change(&app, &before, revision);
}

#[test]
fn production_visibility_resource_without_a_registered_intent_cannot_hide_the_root() {
    let (mut app, root, node) = app();
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.init_resource::<crate::ui::scene_objects::ObjectVisibilityState>();
    enter(&mut app, node);
    request(&mut app, MeshEditAction::Move);
    fixture::step(&mut app, node, Vec2::ZERO);
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    let state = app.world().resource::<MeshEditState>();
    assert!(!state.movement.as_ref().unwrap().valid);
    assert!(state.hidden_roots.is_empty());
    assert!(state.ghost.is_none(), "缺生产票据必须在生成幽灵之前失败关闭");
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
    assert_no_author_change(&app, &before, revision);
}

fn delete_during_mesh_preview(external_hidden: bool) {
    use bevy::ecs::system::RunSystemOnce;
    use crate::authoring::placement_deletion::{self, PlacementDeletionSuppression};

    let (mut app, formal, node) = tracked_app();
    let before = current(&app).clone();
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews.clear();
    app.add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    app.world_mut().entity_mut(formal).remove::<ScenePlacementInstance>();
    app.world_mut().entity_mut(formal).insert(ScenePlacementInstance::new(&map, &before));
    let original = attach_hierarchy(&mut app, formal);
    let ghost = begin_preview(&mut app, node);
    assert_root_untouched(&app, formal, &original, Visibility::Hidden);
    let (delete_scene, id) = (map.clone(), before.id.clone());
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        session.draft.as_mut().unwrap().placements.placements.clear();
        placement_deletion::mark_deleted(&mut commands, &mut session, &delete_scene, &id);
    }).unwrap();
    assert_eq!(app.world().get::<PlacementDeletionSuppression>(formal).unwrap().original_visibility, Visibility::Hidden,
        "删除取得责任时只看见幽灵的临时Hidden，恢复入口必须交接仍有信用的基线");
    if external_hidden {
        app.world_mut().entity_mut(formal).insert(Visibility::Hidden); // 同值真实Insert撤销旧手势信用。
    }
    cancel_preview(&mut app, node, ghost);
    let expected = if external_hidden { Visibility::Hidden } else { Visibility::Inherited };
    assert_root_untouched(&app, formal, &original, Visibility::Hidden);
    assert_eq!(app.world().get::<PlacementDeletionSuppression>(formal).unwrap().original_visibility, expected);
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.is_empty());
    // 模拟Undo已恢复作者记录，再由真实删除API撤回自身mask；不重建或替换正式根。
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let id = before.id.clone();
        session.draft.as_mut().unwrap().placements.placements.push(before.clone());
        placement_deletion::restore_deleted(&mut commands, &mut session, &map, &id);
    }).unwrap();
    assert!(app.world().get::<PlacementDeletionSuppression>(formal).is_none());
    assert_root_untouched(&app, formal, &original, expected);
}

#[test]
fn mesh_cancel_transfers_the_original_base_to_deletion_after_the_record_was_removed() {
    delete_during_mesh_preview(false);
}

#[test]
fn mesh_cancel_does_not_transfer_an_obsolete_base_over_same_hidden_external_intent() {
    delete_during_mesh_preview(true);
}
