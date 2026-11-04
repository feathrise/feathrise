//! 无窗口验证候选悬停只改临时覆盖、确认取消旧点击和同图请求换代。
use crate::game_authoring::map_interaction::MapInteractionState;
use super::{
    layer_ui::{CandidateAction, layer_candidate_interactions},
    layers::*,
}; // 直接运行生产交互系统。
use crate::viewport::modes::{SelectionFilter, ViewportModeState}; // 默认选择模式。
use bevy::prelude::*; // 最小ECS测试无游戏窗口。
use feathrise_map_content::MapId; // 合法地图身份。

#[test]
fn transform_modes_lock_selection_and_return_to_select_restores_candidate_input() {
    use crate::authoring::{EditorAuthoringSession, test_heightfield_draft};
    use crate::resources::{MapEditorState, MapEditorNumericScrubMouseGuard};
    use crate::viewport::modes::ViewportMode;
    let draft = test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    let map = draft.map_id.clone();
    let hash = draft.adopted_domain_hash().to_owned();
    let a = LayerTarget::Placement("asset-a".into());
    let b = LayerTarget::Placement("asset-b".into());
    let mut modes = ViewportModeState::default();
    modes.filter = SelectionFilter::Water;
    let mut app = App::new();
    app.insert_resource(MapEditorState { open: true, placement_transform_enabled: true, selected_map_id: Some(map.clone()), ..default() })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), revision: 7, ..default() })
        .init_resource::<MapInteractionState>().init_resource::<MapEditorNumericScrubMouseGuard>()
        .insert_resource(modes)
        .insert_resource(LayerCandidateState { selected: Some(a.clone()),
            identity: Some(Identity { map, revision: 7, placement_derived_pose_revision: 0, domain_hash: hash, generation: 0, ticket: 0, filter: SelectionFilter::Water }),
            candidates: vec![LayerCandidate { author_index: Some(0), target: a.clone(), label: "资产 A".into(), rank: 0, distance: 1.0 },
                LayerCandidate { author_index: Some(1), target: b.clone(), label: "资产 B".into(), rank: 0, distance: 2.0 }], ..default() })
        .add_systems(Update, (super::layer_query::poll_layer_candidates, layer_candidate_interactions).chain());
    let row = app.world_mut().spawn((Interaction::None, feathrise_editor_ui::EditorButtonClick::default(), CandidateAction::Select(1), BackgroundColor::default())).id();
    app.update();
    for mode in [ViewportMode::Move, ViewportMode::Rotate, ViewportMode::Scale] {
        app.world_mut().resource_mut::<ViewportModeState>().mode = mode;
        app.world_mut().entity_mut(row).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update();
        let state = app.world().resource::<LayerCandidateState>();
        assert_eq!(state.selected.as_ref(), Some(&a));
        assert!(state.apply.is_none() && state.hover.is_none());
        assert_eq!(state.candidates.len(), 2);
        *app.world_mut().get_mut::<Interaction>(row).unwrap() = Interaction::None;
        app.update();
    }
    assert_eq!(app.world().resource::<ViewportModeState>().filter, SelectionFilter::Water);
    assert_eq!(app.world().resource::<ViewportModeState>().selection_filter(app.world().resource::<MapEditorState>()), SelectionFilter::Water);
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.world_mut().entity_mut(row).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert_eq!(app.world().resource::<LayerCandidateState>().apply.as_ref(), Some(&b));
    // 拖动公共门拥有鼠标时，同一菜单按下不能继续请求换目标。
    app.world_mut().resource_mut::<LayerCandidateState>().apply = None;
    app.world_mut().entity_mut(row).get_mut::<Interaction>().unwrap().set_if_neq(Interaction::None);
    app.world_mut().resource_mut::<MapEditorNumericScrubMouseGuard>().advance(true, true, false);
    app.update();
    app.world_mut().entity_mut(row).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(app.world().resource::<LayerCandidateState>().apply.is_none());
}

#[path = "selection_lock_tests.rs"]
mod selection_lock_tests;

#[path = "terrain_canvas_selection_tests.rs"]
mod terrain_canvas_selection_tests;

#[test]
fn sub_map_document_selection_restores_in_move_mode_and_is_scoped_to_parent() {
    use crate::authoring::{EditorAuthoringSession, test_heightfield_draft};
    use crate::resources::MapEditorState;
    use crate::viewport::modes::ViewportMode;
    use feathrise_map_content::{MapInstanceId, MapInstancePlacement};
    let id = MapInstanceId::parse("house").unwrap();
    let mut draft = test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.manifest.sub_maps = std::sync::Arc::from([MapInstancePlacement { id: id.clone(),
        source_map_id: MapId::parse("MAP-house").unwrap(), label: "房屋".into(), transform: default() }]);
    let map = draft.map_id.clone();
    let session = EditorAuthoringSession { draft: Some(draft), revision: 7, ..default() };
    let mut state = LayerCandidateState::default();
    let old_ticket = state.ticket;
    state.confirm_sub_map(&id, &session, 0, SelectionFilter::All);
    assert!(state.ticket > old_ticket); // 原视口查询失去采用资格。
    assert_eq!(state.identity.as_ref().unwrap().map, map);
    state.restore_sub_map(&id, &session, 0, SelectionFilter::All);
    let mut app = App::new();
    let mut modes = ViewportModeState::default(); modes.mode = ViewportMode::Move;
    app.insert_resource(session).insert_resource(state).init_resource::<MapInteractionState>()
        .insert_resource(MapEditorState { open: true, selected_map_id: Some(map), ..default() })
        .insert_resource(modes)
        .add_systems(Update, super::layer_query::poll_layer_candidates);
    app.update();
    assert_eq!(app.world().resource::<LayerCandidateState>().selected,
        Some(LayerTarget::SubMap(id.to_string()))); // 恢复无需重新进入选择模式。
    assert!(app.world().resource::<LayerCandidateState>().apply.is_none());
    app.world_mut().resource_mut::<MapEditorState>().selected_map_id = Some(MapId::parse("MAP-other").unwrap());
    app.update();
    assert!(app.world().resource::<LayerCandidateState>().selected.is_none()); // 同名引用不能跨父图遗留。
}

#[test]
fn candidate_hover_preserves_selection_and_confirm_cancels_old_query() {
    // 防止hover误选和旧后台抢回。
    let selected = LayerTarget::Surface("soil-a".into()); // 正式地表选择。
    let target = LayerTarget::Vegetation("grass-b".into()); // 同点第二个类型。
    let mut app = App::new(); // 只测试作者交互。
    app.init_resource::<ViewportModeState>()
        .insert_resource(LayerCandidateState {
            selected: Some(selected.clone()),
            selected_author_index: Some(3),
            candidates: vec![LayerCandidate {
                target: target.clone(),
                author_index: Some(7),
                label: "植被 · 草地".into(),
                rank: 2,
                distance: 0.0,
            }],
            ticket: 5,
            ..default()
        })
        .add_systems(Update, layer_candidate_interactions); // 无渲染依赖。
    let row = app
        .world_mut()
        .spawn((
            Interaction::Hovered,
            feathrise_editor_ui::EditorButtonClick::default(),
            CandidateAction::Select(0),
            BackgroundColor::default(),
        ))
        .id(); // 指针移动到候选。
    app.update(); // 生产系统预高亮。
    let state = app.world().resource::<LayerCandidateState>(); // 检查正式/临时两条身份。
    assert_eq!(state.selected.as_ref(), Some(&selected)); // hover不改正式选择。
    assert_eq!(state.hover.as_ref(), Some(&target)); // 只增加临时视觉目标。
    assert_eq!(state.vegetation_index("grass-b"), Some(7)); // 覆盖读取后台缓存。
    assert_eq!(state.surface_index("soil-a"), Some(3)); // 旧正式目标仍可恢复。
    *app.world_mut().get_mut::<Interaction>(row).unwrap() = Interaction::None; // 离开候选。
    app.update(); // 恢复正式覆盖。
    assert!(
        app.world()
            .resource::<LayerCandidateState>()
            .hover
            .is_none()
    ); // 不残留hover。
    app.world_mut().entity_mut(row).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered())); // 明确确认。
    app.update(); // 发布正式选择请求。
    let state = app.world().resource::<LayerCandidateState>(); // 系统只排队应用。
    assert_eq!(state.apply.as_ref(), Some(&target)); // 选中对应作者层。
    assert_eq!(state.ticket, 6); // 旧后台票据失效。
    assert!(state.pointer_over_ui); // 行点击不能穿透地图。
}

#[test]
fn latest_request_replaces_pending_and_freezes_mesh_sample_identity() {
    // 单次在途只合并最后一次输入。
    let mut state = LayerCandidateState::default(); // 空选择会话。
    let key = Identity {
        map: MapId::parse("MAP-select-test").unwrap(),
        revision: 7,
        placement_derived_pose_revision: 0,
        domain_hash: "hash".into(),
        generation: 4,
        ticket: 0,
        filter: SelectionFilter::All,
    }; // 冻结作者身份。
    let ray = Ray3d::new(Vec3::Y * 100.0, Dir3::NEG_Y); // 无窗口世界射线。
    state.request(
        key.clone(),
        ray,
        Some(Vec3::ZERO),
        Vec2::ONE,
        Vec2::splat(100.0),
        1.0,
        None,
    ); // 第一击。
    state.request(
        key.clone(),
        ray,
        Some(Vec3::ZERO),
        Vec2::splat(20.0),
        Vec2::splat(100.0),
        1.0,
        Some((23, Vec3::new(3.0, 8.0, 9.0), 92.0)),
    ); // 第二击替换未启动输入。
    let request = state.request.as_ref().unwrap(); // 只剩一个完整快照。
    assert_eq!(request.ticket, 2); // 后一击具有新票据。
    assert_eq!(request.identity.generation, 4); // 不能等启动时才捕获新图代次。
    assert_eq!(request.vegetation_world, Some(Vec2::new(3.0, 9.0))); // 冠层XZ不误用树后地面。
    assert_eq!(request.world, Some(Vec3::ZERO)); // 地表仍使用真实地面落点。
    assert_eq!(request.preferred_distance, Some(92.0)); // 强命中排序使用Mesh距离。
    state.clear(); // 模式退出或换代。
    assert!(state.request.is_none()); // 未开始旧输入完整取消。
    assert_eq!(state.ticket, 3); // 迟到旧结果没有采用资格。
}

#[test]
fn placement_candidate_hover_and_click_use_the_unified_ui() {
    for target in [
        LayerTarget::Placement("tree-instance".into()), // 摆放实例。
        LayerTarget::Portal("gate".into()),             // 传送点使用同一确认出口。
        LayerTarget::Boundary("outer".into()),          // 空气墙路径同样不能穿透点击。
    ] {
        let selected = LayerTarget::Surface("soil".into()); // 重叠地表保持正式选择。
        let mut app = App::new(); // 使用真实交互系统而非模拟按钮逻辑。
        app.init_resource::<ViewportModeState>()
            .insert_resource(LayerCandidateState {
                selected: Some(selected.clone()), // 悬停不能覆盖正式选择。
                candidates: vec![LayerCandidate {
                    target: target.clone(),               // 行绑定稳定实例ID。
                    author_index: None,                   // 摆放物不冒充作者图层数组索引。
                    label: "资产 · tree-instance".into(), // 类型可辨识。
                    rank: 0,                              // 实际网格命中参与前景排序。
                    distance: 1.0,                        // 测试固定射线距离。
                }], // 与其他类别复用同一候选池。
                ..default()
            })
            .add_systems(Update, layer_candidate_interactions); // 行输入沿用统一防穿透逻辑。
        let row = app
            .world_mut()
            .spawn((
                Interaction::Hovered,
            feathrise_editor_ui::EditorButtonClick::default(),       // 模拟资产候选悬停。
                CandidateAction::Select(0), // 第一项绑定实例。
                BackgroundColor::default(), // 生产样式查询必需。
            ))
            .id(); // 真实ECS行实体。
        app.update(); // 发布临时目标。
        let state = app.world().resource::<LayerCandidateState>(); // 只读取已完成帧。
        assert_eq!(state.selected.as_ref(), Some(&selected)); // 地表仍是正式目标。
        assert_eq!(state.hover.as_ref(), Some(&target)); // 实例只成为预览目标。
        *app.world_mut().get_mut::<Interaction>(row).unwrap() = Interaction::None; // 离开资产行。
        app.update(); // 恢复正式目标视觉。
        assert!(
            app.world()
                .resource::<LayerCandidateState>()
                .hover
                .is_none()
        ); // 临时目标被清除。
        app.world_mut().entity_mut(row).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered())); // 点击确认实例。
        app.update(); // 统一出口收到确认请求。
        let state = app.world().resource::<LayerCandidateState>(); // 检查候选绑定未丢失。
        assert_eq!(state.apply.as_ref(), Some(&target)); // 下游同步实例与场景树。
        assert!(state.pointer_over_ui); // 点击不能穿透成为另一张地图射线。
    }
}
