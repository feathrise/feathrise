//! 正式射线任务的单飞、迟到完成和取消采用回归；导入失败前置保证测试不生成项目作者资产。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState};
use std::{future::poll_fn, sync::{atomic::{AtomicBool, Ordering}, Mutex}, task::{Poll, Waker}, time::{Duration, Instant}};

/// 屏障只延迟真实查询的交付，不替换命中结果，也不阻塞后台工作线程。
struct Release {
    ready: Arc<AtomicBool>,
    waker: Arc<Mutex<Option<Waker>>>,
}
impl Release {
    fn finish(self) {
        self.ready.store(true, Ordering::Release);
        let wake = self.waker.lock().unwrap().take();
        if let Some(waker) = wake { waker.wake(); }
    }
}

fn fixture() -> (App, Entity) {
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let map = draft.scene_id.clone();
    draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(map.clone(), TerrainId::legacy(),
        feathrise_scene_content::TerrainObjectTransform { translation: [0.0, 3.0, 0.0], ..default() },
        avian3d::prelude::Collider::cuboid(100.0, 2.0, 100.0)));
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Tools;
    let mut state = TerrainObjectsEditorState::default();
    state.scene_id = Some(map.clone());
    state.creation_open = true;
    state.creation_source = CreationSource::ImportedGlb;
    state.placement_mode = CreationPlacementMode::Viewport;
    state.creation = [91.0, 92.0, 93.0, 8.0, 6.0, 10.0, 37.0, 0.25];
    let mut mode = ViewportModeState::default();
    mode.mode = ViewportMode::Select;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(state)
        .insert_resource(sidebar)
        .insert_resource(mode)
        .insert_resource(EditorAuthoringSession { draft: Some(draft), revision: 7, ..default() })
        .insert_resource(SceneEditorState { open: true, orthographic: true,
            selected_scene_id: Some(map.clone()),
            // 【隔离设计】正式start仍接管创建事务；GLB读前路径校验必败，不能生成源或配套画布。
            selected_asset: Some("../target/__point_query_read_only_fixture__.glb".into()), ..default() })
        .insert_resource(SceneEditorViewportState { _image: Handle::default(), camera: Entity::PLACEHOLDER,
            center: Vec3::new(3.0, 0.0, 5.0), scene_center: Vec3::ZERO, scene_half_extents: Vec3::splat(60.0),
            extent: 50.0, yaw: 0.0, pitch: std::f32::consts::FRAC_PI_2,
            framed_scene_id: Some(map), render_size: UVec2::new(800, 600) })
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::resources::SceneEditorSettingsState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>();
    app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Scene;
    let window = app.world_mut().spawn((Window { focused: true, ..default() }, bevy::window::PrimaryWindow)).id();
    app.world_mut().spawn((SceneEditorViewport, ComputedNode::default(),
        RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::ZERO) }));
    (app, window)
}

fn arm(app: &mut App) {
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().start_requested = true;
    app.world_mut().run_system_once(control).unwrap();
    assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_some());
}

fn click(app: &mut App) {
    app.world_mut().resource_mut::<SceneEditorState>().suppress_world_pick_this_frame = false;
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.world_mut().run_system_once(point_place).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
    assert!(app.world().resource::<TerrainObjectsEditorState>().point_query.is_some(),
        "正式视口点击必须启动真实AsyncCompute地形任务");
}

fn hold(app: &mut App) -> Release {
    let ready = Arc::new(AtomicBool::new(false));
    let waker = Arc::new(Mutex::new(None::<Waker>));
    let signal = Release { ready: ready.clone(), waker: waker.clone() };
    let (completed, completion) = std::sync::mpsc::channel();
    let query = app.world_mut().resource_mut::<TerrainObjectsEditorState>().point_query.take().unwrap();
    let original = query.task;
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let result = original.await;
        let _ = completed.send(()); // 屏障返回前真实worker已完成；测试不靠sleep猜测强引用何时释放。
        poll_fn(|context| {
            if ready.load(Ordering::Acquire) { return Poll::Ready(()); }
            *waker.lock().unwrap() = Some(context.waker().clone());
            if ready.load(Ordering::Acquire) { Poll::Ready(()) } else { Poll::Pending }
        }).await;
        result
    });
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().point_query = Some(PointQuery { task, ..query });
    completion.recv_timeout(Duration::from_secs(5)).expect("实际地形查询应完成，再由屏障控制采用时机");
    signal
}

fn drain(app: &mut App) {
    let start = Instant::now();
    while app.world().resource::<TerrainObjectsEditorState>().point_query.is_some() {
        app.world_mut().run_system_once(point_place).unwrap();
        assert!(start.elapsed() < Duration::from_secs(5), "已释放的后台点查询必须非阻塞结清");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn unchanged(app: &App, serial: u64) {
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.next_serial, serial);
    assert_eq!(session.draft.as_ref().unwrap().manifest.terrain_objects_for_edit().len(), 1);
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(app.world().resource::<TerrainObjectsEditorState>().pending.is_none());
}

#[test]
fn actual_async_point_hit_enters_the_original_creation_transaction_once_without_publishing_an_object() {
    let (mut app, _) = fixture();
    let frozen = app.world().resource::<TerrainObjectsEditorState>().creation;
    let serial = app.world().resource::<EditorAuthoringSession>().next_serial;
    arm(&mut app);
    click(&mut app);
    let release = hold(&mut app);
    for _ in 0..3 {
        app.world_mut().run_system_once(point_place).unwrap();
        unchanged(&app, serial);
    }
    release.finish();
    drain(&mut app);
    let state = app.world().resource::<TerrainObjectsEditorState>();
    assert!(state.point_placement.is_none());
    assert_eq!(state.creation[3..], frozen[3..], "接受落点只替换冻结规格中的世界坐标");
    assert!(Vec3::from_array(state.creation[..3].try_into().unwrap()).distance(Vec3::new(3.0, 4.0, 5.0)) < 0.002,
        "完整命中应来自真实盒顶Y=4，不能使用旧坐标或Y=0工作平面");
    assert!(state.pending.is_some(), "point_place必须调用原start准备事务，不能直接发布对象或历史");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.next_serial, serial + 1);
    assert_eq!(session.revision, 7);
    assert_eq!(session.draft.as_ref().unwrap().manifest.terrain_objects_for_edit().len(), 1);
    assert!(session.undo.is_empty());
    assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Select);
    // 终结该真实创建任务，确认隔离用导入在读前拒绝，而不是留下可能写项目文件的后台任务。
    let pending = app.world_mut().resource_mut::<TerrainObjectsEditorState>().pending.take().unwrap();
    assert!(bevy::tasks::block_on(pending.task).is_err());
    app.world_mut().run_system_once(point_place).unwrap();
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, serial + 1);
}

#[test]
fn delayed_point_hits_cannot_be_adopted_after_frozen_identity_or_input_context_changes() {
    for case in 0..14 {
        let (mut app, window) = fixture();
        let serial = app.world().resource::<EditorAuthoringSession>().next_serial;
        arm(&mut app);
        click(&mut app);
        let release = hold(&mut app);
        match case {
            0 => app.world_mut().resource_mut::<SceneEditorState>().selected_scene_id = Some(SceneId::parse("MAP-other-query").unwrap()),
            1 => app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1,
            2 => app.world_mut().resource_mut::<EditorAuthoringSession>().terrain_revision += 1,
            3 => {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let draft = session.draft.as_mut().unwrap();
                draft.terrain_ground = Some(TerrainGroundSnapshot::empty(draft.scene_id.clone(), 0));
            }
            4 => {
                let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
                editor.terrain_hidden = Arc::new((*editor.terrain_hidden).clone());
            }
            5 => app.world_mut().resource_mut::<crate::resources::SceneEditorSettingsState>().open = true,
            6 => app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document,
            7 => app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Move,
            8 => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            9 => app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Scene,
            10 => app.world_mut().resource_mut::<feathrise_editor_ui::ActiveNumericInput>().entity = Some(Entity::PLACEHOLDER),
            11 => app.world_mut().resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>().entity = Some(Entity::PLACEHOLDER),
            12 => app.world_mut().resource_mut::<SceneEditorState>().close_requested = true,
            _ => app.world_mut().resource_mut::<SceneEditorState>().save_requested = true,
        }
        app.world_mut().run_system_once(point_place).unwrap();
        assert!(app.world().resource::<TerrainObjectsEditorState>().point_query.is_some(),
            "上下文失效先撤销采用权，未完成任务仍占用唯一槽：{case}");
        unchanged(&app, serial);
        release.finish();
        drain(&mut app);
        unchanged(&app, serial);
    }
}

#[test]
fn actual_query_without_a_visible_support_keeps_the_intent_and_never_falls_back_to_old_coordinates() {
    let (mut app, _) = fixture();
    let serial = app.world().resource::<EditorAuthoringSession>().next_serial;
    arm(&mut app);
    Arc::make_mut(&mut app.world_mut().resource_mut::<SceneEditorState>().terrain_hidden).insert(TerrainId::legacy());
    let frozen = app.world().resource::<TerrainObjectsEditorState>().creation;
    click(&mut app);
    drain(&mut app);
    unchanged(&app, serial);
    let state = app.world().resource::<TerrainObjectsEditorState>();
    assert!(state.point_placement.is_some(), "没有完整可见承托时保留点放，等待用户重新点击");
    assert_eq!(state.creation, frozen, "不能拿保留的XYZ或空场景工作平面伪造地形命中");
    assert!(app.world().resource::<SceneEditorState>().status.contains("没有取得完整真实地形命中"));
}

#[test]
fn a_completed_query_retains_only_weak_snapshot_identity_until_its_result_is_consumed() {
    let (mut app, _) = fixture();
    let serial = app.world().resource::<EditorAuthoringSession>().next_serial;
    arm(&mut app);
    click(&mut app);
    let release = hold(&mut app);
    let (ground, hidden) = {
        let state = app.world().resource::<TerrainObjectsEditorState>();
        let query = state.point_query.as_ref().unwrap();
        (query.snapshot.clone(), query.hidden.clone())
    };
    // hold使用完成消息确认worker已交付真实结果；唯一强引用应属于当前草稿/显隐资源。
    assert_eq!(ground.strong_count(), 1, "主线程查询槽不得额外强持有可随地形规模增长的快照");
    assert_eq!(hidden.strong_count(), 1, "可见性身份同样只保留Weak，不把旧集合析构留给完成采用帧");
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_ground = Some(TerrainGroundSnapshot::empty(draft.scene_id.clone(), 0));
    }
    app.world_mut().resource_mut::<SceneEditorState>().terrain_hidden = Arc::new(BTreeSet::new());
    assert!(ground.upgrade().is_none() && hidden.upgrade().is_none(),
        "当前来源更换后，即使查询结果还在单飞槽等待消费，旧大对象也已不受该槽强持有");
    assert!(app.world().resource::<TerrainObjectsEditorState>().point_query.is_some());
    release.finish();
    drain(&mut app);
    unchanged(&app, serial);
}

#[test]
fn cancelling_retains_the_query_slot_until_completion_and_repeated_start_cannot_add_a_second_worker() {
    let (mut app, _) = fixture();
    let serial = app.world().resource::<EditorAuthoringSession>().next_serial;
    arm(&mut app);
    click(&mut app);
    let release = hold(&mut app);
    let ticket = app.world().resource::<TerrainObjectsEditorState>().placement_ticket;
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    app.world_mut().run_system_once(control).unwrap();
    assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_none());
    assert!(app.world().resource::<TerrainObjectsEditorState>().point_query.is_some());
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape));
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    for _ in 0..3 {
        app.world_mut().resource_mut::<TerrainObjectsEditorState>().start_requested = true;
        app.world_mut().run_system_once(control).unwrap();
        app.world_mut().run_system_once(point_place).unwrap();
        let state = app.world().resource::<TerrainObjectsEditorState>();
        assert!(state.point_query.is_some() && state.point_placement.is_none());
        assert_eq!(state.placement_ticket, ticket, "取消中的单飞槽未结清时不能武装第二次查询");
        assert!(!state.start_requested, "被阻止的开始请求当帧消费，不能结清后自动重放");
        unchanged(&app, serial);
    }
    release.finish();
    drain(&mut app);
    unchanged(&app, serial);
    arm(&mut app);
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().placement_ticket, ticket + 1);
    click(&mut app);
    unchanged(&app, serial);
}
