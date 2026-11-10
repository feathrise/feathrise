//! 真实多选输入从只有正式根的状态准备、拖动及提交；未保存作者结果不能随旧候选容器退役而消失。
//!
//! 【验证边界】使用真实handle_placement_move与Transform传播；仅在提交后用公开实体/运行时代次
//! 模拟旧正式批次退休与新批次接管，不声称覆盖渲染器或共享加载门的完整候选构建。

use super::*;
use bevy::world_serialization::WorldAsset;
use feathrise_scene::{ScenePlacementRuntimePhase, ScenePlacementRuntimeStatus};

struct FormalGroupFixture {
    app: App,
    formal: [Entity; 2],
    node: Entity,
    parent: Entity,
    original: [Transform; 2],
    records: Vec<ScenePlacementRecord>,
}

impl FormalGroupFixture {
    fn new() -> Self {
        let (mut app, formal, node, parent) = group_app();
        app.init_asset::<WorldAsset>()
            .add_observer(crate::ui::placement_water::register_placement_instance_root)
            .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
        let (map, records) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.placement_previews.clear(); // 已保存的旧预览已清理，场景现在只有归正式候选拥有的两个根。
            let draft = session.draft.as_ref().unwrap();
            (draft.scene_id.clone(), draft.placements.placements.clone())
        };
        for (root, record) in formal.iter().zip(&records) {
            app.world_mut().entity_mut(*root).remove::<ScenePlacementInstance>()
                .insert(ScenePlacementInstance::new(&map, record)); // 真实observer登记正式根，不能借空索引假装不存在。
        }
        app.insert_resource(ScenePlacementRuntimeStatus {
            scene_id: Some(map), phase: ScenePlacementRuntimePhase::Building,
            revision: 17, attempt_revision: 18, placement_count: 2, ..default()
        }); // 旧正式画面保持活动；此前保存的候选仍可能在本次作者编辑后采用。
        let original = formal.map(|root| *app.world().get::<Transform>(root).unwrap());
        Self { app, formal, node, parent, original, records }
    }

    fn prepare(&mut self) {
        let start = group_point(&self.app, MoveAxis::X, 0.0);
        self.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut self.app, self.node, start);
        assert!(self.app.world().resource::<PlacementMoveState>().group.as_ref()
            .is_some_and(|group| group.phase == GroupPhase::Preparing));
        // 自补作者根需要先落地再由observer登记；准备阶段允许跨多帧，不能要求命中正式根即刻绑父。
        for _ in 0..8 {
            step(&mut self.app, self.node, start);
            if self.app.world().resource::<PlacementMoveState>().group.as_ref()
                .is_some_and(|group| group.phase == GroupPhase::Dragging) { return; }
        }
        panic!("真实Preparing应取得独立作者根并进入Dragging：{}",
            self.app.world().resource::<SceneEditorState>().status);
    }

    fn assert_formal_pose_unchanged(&self) {
        for (index, root) in self.formal.iter().enumerate() {
            assert_eq!(self.app.world().get::<Transform>(*root), Some(&self.original[index]),
                "作者整组变换不能写入旧正式候选拥有的Transform");
            assert_eq!(self.app.world().get::<ChildOf>(*root).map(ChildOf::parent), Some(self.parent),
                "Preparing不能把旧正式根绑到作者临时pivot，破坏正式容器的生命期");
        }
    }

    fn author_previews(&self) -> [Entity; 2] {
        let session = self.app.world().resource::<EditorAuthoringSession>();
        std::array::from_fn(|index| {
            let root = *session.placement_previews.get(&self.records[index].id)
                .expect("每个只有正式根的已选对象都应取得独立作者预览");
            assert!(!self.formal.contains(&root), "session作者地址不能指向正式批次拥有的根");
            root
        })
    }
}

#[test]
fn multi_formal_only_preparing_binds_independent_previews_without_mutating_formal_pose() {
    let mut fixture = FormalGroupFixture::new();
    fixture.prepare();
    fixture.assert_formal_pose_unchanged();
    let previews = fixture.author_previews();
    let drag = fixture.app.world().resource::<PlacementMoveState>().group.as_ref().unwrap();
    assert_eq!(drag.roots.len(), 2);
    assert!(drag.roots.iter().all(|root| previews.contains(&root.entity)));
    let cursor = group_point(&fixture.app, MoveAxis::X, 1.25);
    step(&mut fixture.app, fixture.node, cursor);
    fixture.assert_formal_pose_unchanged();
    for (index, root) in previews.iter().enumerate() {
        assert!((fixture.app.world().get::<GlobalTransform>(*root).unwrap().translation().x
            - fixture.records[index].translation[0] - 1.25).abs() < 0.002);
    }
    let session = fixture.app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, fixture.records);
    assert!(session.undo.is_empty());
    assert!(!fixture.app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn multi_formal_only_commit_survives_old_batch_retirement_with_one_complete_history() {
    let mut fixture = FormalGroupFixture::new();
    fixture.prepare();
    let cursor = group_point(&fixture.app, MoveAxis::X, 1.25);
    step(&mut fixture.app, fixture.node, cursor);
    fixture.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    finish(&mut fixture.app, fixture.node, cursor);
    fixture.assert_formal_pose_unchanged();
    let previews = fixture.author_previews();
    let (map, after) = {
        let session = fixture.app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.undo.len(), 1);
        assert!(matches!(&session.undo[0], EditorCommand::CourtyardBatch(_)));
        assert_eq!(session.revision, 8);
        assert!(fixture.app.world().resource::<SceneEditorState>().dirty);
        let draft = session.draft.as_ref().unwrap();
        (draft.scene_id.clone(), draft.placements.placements.clone())
    };
    for (before, record) in fixture.records.iter().zip(&after) {
        assert!((record.translation[0] - before.translation[0] - 1.25).abs() < 0.002);
        assert_eq!(record.scale, before.scale);
        assert_eq!(record.yaw_degrees, before.yaw_degrees);
        assert_eq!(record.water_surfaces, before.water_surfaces);
    }
    // 提交之后的正式采用只携带旧保存值；旧容器可以回收自己的整树，不能回收作者预览。
    fixture.app.world_mut().despawn(fixture.parent);
    let next_parent = fixture.app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id();
    for (record, original) in fixture.records.iter().zip(&fixture.original) {
        fixture.app.world_mut().spawn((*original, ChildOf(next_parent), Visibility::Inherited,
            ScenePlacementInstance::new(&map, record)));
    }
    {
        let mut status = fixture.app.world_mut().resource_mut::<ScenePlacementRuntimeStatus>();
        status.revision += 1;
        status.phase = ScenePlacementRuntimePhase::Ready;
    }
    step(&mut fixture.app, fixture.node, cursor);
    for (index, root) in previews.iter().enumerate() {
        assert!(fixture.app.world().get_entity(*root).is_ok(), "未保存多选编辑不能随旧正式根退役消失");
        assert!(fixture.app.world().get::<ScenePlacementInstance>(*root).unwrap()
            .matches_record(&map, &after[index], &after[index].water_surfaces),
            "归还临时父级时必须同步完整作者记录证据，后续保存才能冻结这次新变换");
        assert_eq!(fixture.app.world().get::<Visibility>(*root), Some(&Visibility::Inherited));
        assert!(fixture.app.world().get::<ChildOf>(*root).is_none(), "普通作者预览不能依附旧正式容器");
        assert!((fixture.app.world().get::<GlobalTransform>(*root).unwrap().translation().x
            - after[index].translation[0]).abs() < 0.002);
    }
    let session = fixture.app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, after);
    assert_eq!(session.undo.len(), 1);
    assert!(fixture.formal.iter().all(|root| fixture.app.world().get_entity(*root).is_err()));
}
