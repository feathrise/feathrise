use std::time::Duration;

use feathrise_scene_content::SceneId;

use super::*;
use crate::components::EditorPlacedAsset;

fn test_scene_id(name: &str) -> SceneId {
    SceneId::parse(name).expect("测试场景ID必须合法")
}

fn schedule_cleanup(app: &mut App, scene_id: &SceneId, previews: Vec<(String, Entity)>) {
    let previews = previews.into_iter().map(|(id, root)|
        PlacementPreviewHandoffSnapshot::capture(app.world(), scene_id, &id, root)
            .expect("现存作者预览可以冻结真实版本证据")).collect();
    schedule_placement_preview_cleanup(
        &mut app.world_mut().resource_mut::<WaterFileHandoff>(),
        scene_id,
        previews,
    )
    .expect("空闲清理槽必须接受测试批次");
}

#[test]
fn volume_stream_save_cleanup_transfers_index_without_destroying_resident_mesh() {
    let mut app = App::new();
    let map = test_scene_id("MAP-volume-stream-handoff");
    let root = app
        .world_mut()
        .spawn((
            EditorPlacedAsset,
            feathrise_scene::StreamedVolumeResident::default(),
            Visibility::Inherited,
        ))
        .with_child(Visibility::Inherited)
        .id();
    let child = app.world().get::<Children>(root).unwrap()[0];
    let mut session = EditorAuthoringSession::default();
    session.placement_previews.insert("volume".into(), root);
    app.insert_resource(session)
        .insert_resource(WaterFileHandoff::default())
        .add_systems(Update, cleanup_placement_handoff_previews);
    schedule_cleanup(&mut app, &map, vec![("volume".into(), root)]);
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(root),
        Some(&Visibility::Inherited)
    );
    assert!(app.world().get_entity(child).is_ok());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .placement_previews
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<WaterFileHandoff>()
            .preview_cleanup
            .is_none()
    );
}

#[test]
fn sixty_five_previews_keep_the_scene_frozen_until_the_third_bounded_frame() {
    let scene_id = test_scene_id("MAP-placement-preview-cleanup");
    let other_scene_id = test_scene_id("MAP-placement-preview-other");
    let mut app = App::new();
    let old_same_id = app.world_mut().spawn(EditorPlacedAsset).id();
    let later_same_id = app.world_mut().spawn(EditorPlacedAsset).id();
    let mut session = EditorAuthoringSession::default();
    session
        .placement_previews
        .insert("same-id".to_owned(), later_same_id); // 后续同ID实体不属于待清理保存批次。
    let mut batch = vec![("same-id".to_owned(), old_same_id)];
    let mut other_entities = Vec::new();
    for index in 0..64 {
        let id = format!("preview-{index}");
        let entity = app.world_mut().spawn(EditorPlacedAsset).id();
        session.placement_previews.insert(id.clone(), entity);
        batch.push((id, entity));
        other_entities.push(entity);
    }
    app.insert_resource(session)
        .insert_resource(WaterFileHandoff::default())
        .add_systems(Update, cleanup_placement_handoff_previews);
    schedule_cleanup(&mut app, &scene_id, batch);

    assert!(
        app.world()
            .resource::<WaterFileHandoff>()
            .placement_handoff_in_flight_for(&scene_id)
    );
    assert!(
        !app.world()
            .resource::<WaterFileHandoff>()
            .placement_handoff_in_flight_for(&other_scene_id)
    ); // 清理冻结仍只属于本批场景。

    app.update();

    assert!(app.world().get_entity(old_same_id).is_ok());
    assert!(
        other_entities
            .iter()
            .all(|entity| app.world().get_entity(*entity).is_ok())
    ); // 首帧64个操作全用于静默化，不递归销毁任何根。
    assert_eq!(
        app.world()
            .resource::<WaterFileHandoff>()
            .preview_cleanup
            .as_ref()
            .map(|cleanup| cleanup.quiesce_cursor),
        Some(64),
    );

    app.update();

    assert!(
        app.world()
            .resource::<WaterFileHandoff>()
            .placement_handoff_in_flight_for(&scene_id)
    ); // 第二帧仍有后批根时，网格水作者输入必须继续冻结。
    assert!(app.world().get_entity(old_same_id).is_err());
    assert_eq!(
        other_entities
            .iter()
            .filter(|entity| app.world().get_entity(**entity).is_ok())
            .count(),
        2,
    );

    app.update();

    assert!(
        !app.world()
            .resource::<WaterFileHandoff>()
            .placement_handoff_in_flight_for(&scene_id)
    ); // 最后一根真正回收后正常解冻，不会永久阻塞保存/关闭。
    assert!(
        !app.world()
            .resource::<WaterFileHandoff>()
            .blocks_new_save_for(Some(&scene_id))
    );
    assert!(app.world().get_entity(later_same_id).is_ok());
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .placement_previews
            .get("same-id"),
        Some(&later_same_id),
    ); // 精确实体不相等时只销毁旧批根，不移除新索引。
}

#[test]
fn wide_world_asset_tree_requires_nine_frames_without_recursive_despawn() {
    let scene_id = test_scene_id("MAP-placement-preview-wide-tree");
    let mut app = App::new();
    let root = app
        .world_mut()
        .spawn((
            EditorPlacedAsset,
            Visibility::Inherited,
            WorldAssetRoot(Handle::default()),
        ))
        .id();
    let children = (0..256)
        .map(|_| {
            app.world_mut()
                .spawn((EditorPlacedAsset, Visibility::Inherited, ChildOf(root)))
                .id()
        })
        .collect::<Vec<_>>();
    let mut session = EditorAuthoringSession::default();
    session.placement_previews.insert("wide".to_owned(), root);
    app.insert_resource(session)
        .insert_resource(WaterFileHandoff::default())
        .add_systems(Update, cleanup_placement_handoff_previews);
    schedule_cleanup(&mut app, &scene_id, vec![("wide".to_owned(), root)]);

    for _ in 0..8 {
        app.update();
    }

    assert!(app.world().get_entity(root).is_ok());
    assert!(app.world().get::<WorldAssetRoot>(root).is_none()); // 第一个操作已先停止这棵Scene0热更。
    assert!(
        children
            .iter()
            .any(|child| app.world().get_entity(*child).is_ok())
    ); // 514个明确操作不得被8帧的512额度递归绕过。
    assert!(
        app.world()
            .resource::<WaterFileHandoff>()
            .placement_handoff_in_flight_for(&scene_id)
    );

    app.update();

    assert!(app.world().get_entity(root).is_err());
    assert!(
        children
            .iter()
            .all(|child| app.world().get_entity(*child).is_err())
    );
    assert!(
        app.world()
            .resource::<WaterFileHandoff>()
            .preview_cleanup
            .is_none()
    );
}

#[test]
fn later_placement_gate_remains_frozen_after_an_earlier_cleanup_finishes() {
    let scene_id = test_scene_id("MAP-placement-preview-later-gate");
    let mut app = App::new();
    let old_preview = app.world_mut().spawn(EditorPlacedAsset).id();
    app.insert_resource(EditorAuthoringSession::default())
        .insert_resource(WaterFileHandoff::default())
        .add_systems(Update, cleanup_placement_handoff_previews);
    let old_snapshot = PlacementPreviewHandoffSnapshot::capture(app.world(), &scene_id, "old", old_preview)
        .expect("旧预览现存且可捕获");
    {
        let mut handoff = app.world_mut().resource_mut::<WaterFileHandoff>();
        let mut later_previews = Vec::new();
        handoff
            .begin_placement(
                scene_id.clone(),
                8,
                3,
                5,
                0,
                &mut later_previews,
                &mut Vec::new(),
                Duration::ZERO,
            )
            .expect("后批正式placement门必须可建立");
        schedule_placement_preview_cleanup(
            &mut handoff,
            &scene_id,
            vec![old_snapshot],
        )
        .expect("旧批精确清理可以与已经持有所有权的后批门防御性重叠");
    }

    app.update();

    let handoff = app.world().resource::<WaterFileHandoff>();
    assert!(handoff.preview_cleanup.is_none());
    assert!(handoff.placement_handoff_in_flight_for(&scene_id)); // 旧批清完不得误解冻尚未Ready的后批正式候选。
    assert!(handoff.blocks_new_save_for(Some(&scene_id)));
}
