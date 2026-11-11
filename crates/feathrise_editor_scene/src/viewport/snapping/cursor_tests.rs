//! 真实ECS输入门回归：控件和模态独占期间不捕获或重放吸附点击，取消仍回收唯一CPU槽。

use super::{MeshSnapCursor, types::MeshSnapKey, update_mesh_cursor};
use crate::{
    authoring::{
        EditorAuthoringSession, EditorPreviewAssets, TerrainExternalChangeState,
        place_from_viewport,
    },
    scene_browser::scene_browser_general_input_available,
    resources::{SceneBrowserDragState, SceneBrowserState, SceneEditorState},
    types::{EditorTool, SceneFolderDialogMode, SceneFolderDialogState},
    ui::{
        asset_preview::AssetPreviewState,
        numeric_scrub_mouse_guard::{
            scene_editor_numeric_scrub_mouse_available, update_scene_editor_numeric_scrub_mouse_guard,
        },
    },
};
use bevy::{prelude::*, tasks::AsyncComputeTaskPool};
use feathrise_editor_ui::ActiveNumericInputScrub;
use feathrise_scene::ClipmapGroundHit;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorSnapSettings;

#[derive(Clone, Copy, Debug)]
enum Blocker {
    Numeric,
    FolderDialog,
    SceneDrag,
    TerrainConflict,
}

fn app() -> App {
    let (mut app, _, _) = crate::viewport::placement_move::interaction_tests::app();
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.tool = EditorTool::PlaceAsset;
        editor.placement_transform_enabled = false;
        editor.selected_asset = Some("gltf/test.glb".into());
        editor.cursor_world = Some(Vec3::new(6.3, 2.0, 6.3));
    }
    app.init_asset::<WorldAsset>()
        .init_asset::<feathrise_scene_content::SceneManifestAsset>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<MeshSnapCursor>()
        .insert_resource(EditorSettings {
            editor_snapping: EditorSnapSettings {
                enabled: true,
                ..default()
            },
            ..default()
        })
        .insert_resource(EditorPreviewAssets {
            portal_base_mesh: Handle::default(),
            portal_arrow_head_mesh: Handle::default(),
            portal_arrow_shaft_mesh: Handle::default(),
            portal_edge_beam_mesh: Handle::default(),
            portal_edge_ring_mesh: Handle::default(),
            portal_base_material: Handle::default(),
            portal_arrow_material: Handle::default(),
            portal_edge_beam_material: Handle::default(),
            portal_edge_ring_material: Handle::default(),
            portal_label_font: Handle::default(),
        })
        .add_systems(
            Update,
            (
                update_mesh_cursor,
                place_from_viewport
                    .run_if(scene_editor_numeric_scrub_mouse_available)
                    .run_if(scene_browser_general_input_available),
            )
                .chain()
                .after(update_scene_editor_numeric_scrub_mouse_guard),
        ); // 使用正式公共门和真实点放消费者，不能只测资源取消方法。
    app
}

fn set_blocker(app: &mut App, blocker: Blocker, active: bool) {
    match blocker {
        Blocker::Numeric => {
            app.world_mut()
                .resource_mut::<ActiveNumericInputScrub>()
                .entity = active.then_some(Entity::PLACEHOLDER);
        }
        Blocker::FolderDialog => {
            app.world_mut().resource_mut::<SceneBrowserState>().dialog =
                active.then(|| SceneFolderDialogState {
                    mode: SceneFolderDialogMode::Create,
                    target_folder_id: None,
                    name: "冻结输入".into(),
                });
        }
        Blocker::SceneDrag => {
            app.world_mut()
                .resource_mut::<SceneBrowserDragState>()
                .blocked = active;
        }
        Blocker::TerrainConflict => {
            if active {
                let map = app
                    .world()
                    .resource::<EditorAuthoringSession>()
                    .draft
                    .as_ref()
                    .unwrap()
                    .scene_id
                    .clone();
                app.world_mut()
                    .resource_mut::<TerrainExternalChangeState>()
                    .register_save_conflict(map, [9; 32]);
            } else {
                app.world_mut()
                    .resource_mut::<TerrainExternalChangeState>()
                    .conflict = None;
            }
        }
    }
}

fn step(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

fn assert_click_cleared(app: &App) {
    let snap = app.world().resource::<MeshSnapCursor>();
    assert!(snap.pending.is_none());
    assert!(snap.accepted.is_none());
    assert!(snap.completed.is_none());
    assert!(snap.last_started.is_none());
    assert!(snap.preview.is_none());
    assert!(!snap.requires_query());
}

fn assert_author_unchanged(app: &App) {
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 7);
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements.len(),
        1
    );
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

fn seed_click(app: &mut App) -> MeshSnapKey {
    step(app); // 由实际光标系统构造身份，避免测试猜测生产输入键。
    let key = app
        .world()
        .resource::<MeshSnapCursor>()
        .last_started
        .clone()
        .unwrap();
    let hit = ClipmapGroundHit::Hit(Vec3::new(6.0, 2.0, 6.0));
    let mut snap = app.world_mut().resource_mut::<MeshSnapCursor>();
    snap.pending = Some(key.clone());
    snap.accepted = Some((key.clone(), hit));
    snap.completed = Some((key.clone(), hit));
    snap.preview = Some(Vec3::new(6.0, 2.0, 6.0));
    key
}

#[test]
fn blocked_mesh_snap_click_is_discarded_and_never_replayed_after_quiet_release() {
    for blocker in [
        Blocker::Numeric,
        Blocker::FolderDialog,
        Blocker::SceneDrag,
        Blocker::TerrainConflict,
    ] {
        let mut app = app();
        set_blocker(&mut app, blocker, true);
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.press(MouseButton::Right);
            mouse.press(MouseButton::Left); // 拖出控件后的和弦不得冻结成场景点击。
        }
        step(&mut app);
        assert_click_cleared(&app);
        assert!(app.world().resource::<MeshSnapCursor>().task.is_none());
        assert_author_unchanged(&app);

        set_blocker(&mut app, blocker, false);
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Left);
            mouse.release(MouseButton::Right);
        }
        for _ in 0..3 {
            step(&mut app); // release和静默帧均没有新左键边沿，允许恢复悬停但不得补放资产。
            assert_author_unchanged(&app);
            let snap = app.world().resource::<MeshSnapCursor>();
            assert!(snap.pending.is_none());
            assert!(snap.completed.is_none());
        }
    }
}

#[test]
fn ownership_loss_clears_an_existing_mesh_snap_completion_before_consumer_resumes() {
    for blocker in [
        Blocker::Numeric,
        Blocker::FolderDialog,
        Blocker::SceneDrag,
        Blocker::TerrainConflict,
    ] {
        let mut app = app();
        seed_click(&mut app);
        set_blocker(&mut app, blocker, true);
        step(&mut app);
        assert_click_cleared(&app); // 禁用帧不能只停止捕获，先前已经完成的点击也必须失效。
        set_blocker(&mut app, blocker, false);
        step(&mut app);
        assert_author_unchanged(&app);
        assert!(app.world().resource::<MeshSnapCursor>().completed.is_none());
    }
}

#[test]
fn blocked_mesh_snap_keeps_the_unfinished_cpu_slot_while_clearing_click_authority() {
    let mut app = app();
    let key = seed_click(&mut app);
    app.world_mut().resource_mut::<MeshSnapCursor>().task =
        Some(AsyncComputeTaskPool::get().spawn(async move {
            std::future::pending::<()>().await;
            (key, ClipmapGroundHit::Failed)
        })); // 未完成任务刻意保持占槽，验证取消不会把丢弃句柄当抢占。
    set_blocker(&mut app, Blocker::Numeric, true);
    for _ in 0..2 {
        step(&mut app);
        assert_click_cleared(&app);
        assert!(app.world().resource::<MeshSnapCursor>().task.is_some());
    }
    set_blocker(&mut app, Blocker::Numeric, false);
    step(&mut app);
    assert!(app.world().resource::<MeshSnapCursor>().task.is_some());
    assert!(app.world().resource::<MeshSnapCursor>().pending.is_none());
    assert_author_unchanged(&app);
}

#[test]
fn object_cursor_snaps_moved_scaled_surface_without_container_mesh_and_rejects_object_switch() {
    use crate::authoring::terrain_ground::{TerrainGroundCursor, TerrainGroundSnapshot};
    use feathrise_scene_content::{TerrainId, TerrainObjectTransform};
    use std::sync::Arc;

    let mut app = app();
    let id = TerrainId::parse("snap-island-a").unwrap();
    let pose = TerrainObjectTransform {
        translation: [20.0, 7.0, 30.0],
        yaw_degrees: 90.0,
        scale: [2.0, 3.0, 4.0],
        ..Default::default()
    };
    let collider = avian3d::prelude::Collider::trimesh(
        vec![Vec3::new(-2.0, -2.0, -2.0), Vec3::new(2.0, 2.0, -2.0),
            Vec3::new(2.0, 2.0, 2.0), Vec3::new(-2.0, -2.0, 2.0)],
        vec![[0, 2, 1], [0, 3, 2]],
    ); // 源空间Y=X斜坡，旋转与非均匀缩放可揭露把世界点直接传入源网格的错误。
    let (map, definition) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.terrain_revision = 0; // test_mesh发布的真实碰撞快照使用这一修订。
        let draft = session.draft.as_mut().unwrap();
        let map = draft.scene_id.clone();
        let mut definition = draft.manifest.terrain_objects_for_edit()[0].clone();
        definition.id = id.clone();
        definition.transform = pose;
        draft.manifest.terrains = Some(Arc::from([definition.clone()]));
        draft.manifest.terrain.resolution = [0, 0];
        draft.manifest.world_min = [0.0; 2];
        draft.manifest.world_max = [0.0; 2];
        draft.terrain_mesh = None;
        draft.terrain_editing_index = None;
        draft.heights = None;
        draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(map.clone(), id.clone(), pose, collider.clone()));
        (map, definition)
    };
    let mut cursor = TerrainGroundCursor::default();
    cursor.terrain = Some(id.clone());
    app.insert_resource(cursor);
    let point = pose.as_transform().to_matrix().transform_point3(Vec3::new(0.25, 0.25, 0.125));
    app.world_mut().resource_mut::<SceneEditorState>().cursor_world = Some(point);
    for _ in 0..1000 {
        step(&mut app);
        if app.world().resource::<MeshSnapCursor>().preview.is_some() { break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let preview = app.world().resource::<MeshSnapCursor>().preview.expect("独立对象使用真实碰撞面完成后台吸附");
    assert!(preview.distance(Vec3::new(21.0, 7.0, 30.0)) < 0.002);
    let clicked = app.world().resource::<MeshSnapCursor>().last_started.clone().unwrap();
    assert_eq!(clicked.object.as_ref().unwrap().id, id);
    {
        let mut state = app.world_mut().resource_mut::<MeshSnapCursor>();
        state.pending = Some(clicked.clone());
        state.accepted = Some((clicked.clone(), ClipmapGroundHit::Hit(preview)));
        state.completed = Some((clicked, ClipmapGroundHit::Hit(preview)));
    }
    let other = TerrainId::parse("snap-island-b").unwrap();
    app.world_mut().resource_mut::<TerrainGroundCursor>().terrain = Some(other.clone());
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut definition = definition;
        definition.id = other.clone();
        draft.manifest.terrains = Some(Arc::from([definition]));
        draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(map, other.clone(), pose, collider));
    }
    step(&mut app); // 特意保持作者/地形修订不变，只换承托对象来证明完整身份门。
    let state = app.world().resource::<MeshSnapCursor>();
    assert!(state.completed.is_none());
    assert!(state.frozen_terrain().is_none());
    assert_eq!(state.last_started.as_ref().unwrap().object.as_ref().unwrap().id, other);
}

fn object_click_app(enabled: bool) -> (App, feathrise_scene_content::TerrainId) {
    use crate::authoring::terrain_ground::{CursorGroundSnapshot, CursorIdentity, CursorRay, TerrainGroundCursor, TerrainGroundSnapshot};
    use feathrise_scene_content::{SceneTerrainDescriptor, TerrainId, TerrainObjectTransform};
    use std::sync::Arc;

    let mut app = app();
    let id = TerrainId::parse("async-click-terrain").unwrap();
    let map = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.terrain_revision = 0;
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = id.clone();
        object.transform = TerrainObjectTransform::default();
        draft.manifest.terrains = Some(Arc::from([object]));
        draft.manifest.terrain = SceneTerrainDescriptor::empty();
        draft.terrain_mesh = None;
        draft.terrain_editing_index = None;
        draft.heights = None;
        let collider = avian3d::prelude::Collider::trimesh(
            vec![Vec3::new(0.0, 2.0, 0.0), Vec3::new(10.0, 2.0, 0.0),
                Vec3::new(10.0, 2.0, 10.0), Vec3::new(0.0, 2.0, 10.0)],
            vec![[0, 2, 1], [0, 3, 2]],
        );
        draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(draft.scene_id.clone(), id.clone(), TerrainObjectTransform::default(), collider));
        draft.scene_id.clone()
    };
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.enabled = enabled;
    let identity = CursorIdentity {
        scene_id: map, revision: 7, tool: EditorTool::PlaceAsset, asset: Some("gltf/test.glb".into()),
        terrain_revision: 0, ground_snapshot: app.world().resource::<EditorAuthoringSession>().draft.as_ref()
            .unwrap().terrain_ground.as_ref().map(CursorGroundSnapshot::capture),
        asset_target: Some(default()), portal: None, camera: Mat4::IDENTITY, size: UVec2::new(800, 600),
        camera_entity: app.world().resource::<crate::resources::SceneEditorViewportState>().camera,
        projection: Mat4::IDENTITY, view: app.world().resource::<SceneEditorState>().view,
        orthographic: app.world().resource::<SceneEditorState>().orthographic,
        extent: app.world().resource::<crate::resources::SceneEditorViewportState>().extent.to_bits(),
        visibility: Arc::as_ptr(&app.world().resource::<SceneEditorState>().terrain_hidden) as usize, relocation: None,
        translation_snap: Some(app.world().resource::<EditorSettings>().editor_snapping.translation_snap(false, [0.0; 3])),
    };
    let mut cursor = TerrainGroundCursor::default();
    cursor.ray = Some(CursorRay { origin: Vec3::new(6.3, 20.0, 6.3), direction: Vec3::NEG_Y, epoch: 1 });
    cursor.identity = Some(identity);
    app.insert_resource(cursor);
    app.world_mut().resource_mut::<SceneEditorState>().cursor_world = None;
    (app, id)
}

fn start_object_click(app: &mut App, control: bool) {
    use crate::authoring::terrain_ground::{TerrainGroundCursor, cursor_for_tests};
    if control {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
    }
    let click_snap = app.world().resource::<EditorSettings>().editor_snapping.translation_snap(control, [0.0; 3]);
    {
        let mut cursor = app.world_mut().resource_mut::<TerrainGroundCursor>();
        let identity = cursor.identity.clone().unwrap();
        let ray = cursor.ray.unwrap();
        cursor.capture_click(identity, ray, Some(click_snap));
    }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    cursor_for_tests(app.world_mut()); // 正式Collider单飞查询启动，不能用旧悬停点冒充这次单击。
    step(app);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::ControlLeft);
}

fn hold_mesh_result(app: &mut App) -> std::sync::mpsc::Sender<()> {
    let original = app.world_mut().resource_mut::<MeshSnapCursor>().task.take().expect("先启动真实表面补查");
    let (release, receiver) = std::sync::mpsc::channel();
    app.world_mut().resource_mut::<MeshSnapCursor>().task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let _ = receiver.recv();
        original.await // 仅延后真实查询输出，不能替换命中或释放仍未完成的任务槽。
    }));
    release
}

fn queue_object_click_behind_hover(app: &mut App, control: bool) -> std::sync::mpsc::Sender<()> {
    use crate::authoring::terrain_ground::{cursor_for_tests, hold_cursor_result_for_tests, TerrainGroundCursor};
    cursor_for_tests(app.world_mut()); // 先启动真实hover，占用原始射线唯一CPU槽。
    let release = hold_cursor_result_for_tests(app.world_mut());
    app.world_mut().resource_mut::<TerrainGroundCursor>().ray.as_mut().unwrap().origin = Vec3::new(7.3, 20.0, 8.3);
    start_object_click(app, control); // 与hover不同的真实按下射线，此时只能排队，不能另起查询。
    assert!(app.world().resource::<TerrainGroundCursor>().pending_click.is_some());
    assert!(app.world().resource::<SceneEditorState>().ground_click_world.is_none());
    assert_author_unchanged(app);
    release
}

#[test]
fn queued_object_click_finishes_once_after_hover_without_ground_change() {
    use crate::authoring::terrain_ground::cursor_for_tests;
    for control in [false, true] {
        let (mut app, id) = object_click_app(true);
        let release = queue_object_click_behind_hover(&mut app, control);
        step(&mut app); // 已松Ctrl及左键，排队仍使用按下帧的吸附选择。
        assert_author_unchanged(&app);
        release.send(()).unwrap();
        for _ in 0..1000 {
            app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
            cursor_for_tests(app.world_mut());
            step(&mut app);
            if app.world().resource::<EditorAuthoringSession>().revision != 7 { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, 8);
        assert_eq!(session.undo.len(), 1);
        let record = session.draft.as_ref().unwrap().placements.placements.last().unwrap();
        assert_eq!(record.translation, if control { [7.3, 2.0, 8.3] } else { [7.0, 2.0, 8.0] });
        assert_eq!(record.terrain_binding.as_ref().unwrap().terrain_id, id);
        for _ in 0..3 { step(&mut app); }
        assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 8);
    }
}

#[test]
fn queued_object_click_cannot_revive_after_ground_changes_and_returns() {
    use crate::authoring::terrain_ground::{cursor_for_tests, cursor_running_for_tests, TerrainGroundCursor, TerrainGroundSnapshot};
    use feathrise_scene_content::TerrainObjectTransform;
    for change in 0..2 {
        let (mut app, id) = object_click_app(true);
        let release = queue_object_click_behind_hover(&mut app, false);
        let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_ground.clone().unwrap();
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            if change == 0 {
                session.terrain_revision += 1; // 不推进作者修订，也不更新viewport身份，raw仍必须直接废票。
            } else {
                let collider = avian3d::prelude::Collider::trimesh(
                    vec![Vec3::new(0.0, 4.0, 0.0), Vec3::new(10.0, 4.0, 0.0),
                        Vec3::new(10.0, 4.0, 10.0), Vec3::new(0.0, 4.0, 10.0)],
                    vec![[0, 2, 1], [0, 3, 2]],
                );
                let replacement = TerrainGroundSnapshot::test_mesh(before.scene_id.clone(), id, TerrainObjectTransform::default(), collider);
                assert_eq!(replacement.revision, before.revision);
                assert_eq!(replacement.generation, before.generation); // 同修订、同代次的新快照仍不能借用旧点击。
                session.draft.as_mut().unwrap().terrain_ground = Some(replacement);
            }
        }
        cursor_for_tests(app.world_mut());
        step(&mut app);
        assert!(app.world().resource::<TerrainGroundCursor>().pending_click.is_none(), "地形换代{change}必须在hover占槽期间废票");
        assert!(cursor_running_for_tests(app.world()), "取消pending不能丢掉尚未完成的hover任务槽");
        assert_author_unchanged(&app);
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.terrain_revision = 0;
            session.draft.as_mut().unwrap().terrain_ground = Some(before);
        }
        release.send(()).unwrap(); // 恢复原快照与修订，没有第二次按下时绝不能重新授权旧射线。
        for _ in 0..1000 {
            app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
            cursor_for_tests(app.world_mut());
            assert!(app.world().resource::<SceneEditorState>().ground_click_world.is_none(), "恢复地形{change}后排队点击不得复活");
            assert!(app.world().resource::<TerrainGroundCursor>().click_terrain.is_none());
            step(&mut app);
            if !cursor_running_for_tests(app.world()) { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!cursor_running_for_tests(app.world()), "必须轮询完真实迟到结果，才可验证无重放");
        assert_author_unchanged(&app);
    }
}

#[test]
fn object_cold_click_freezes_control_until_real_collider_and_surface_confirmation() {
    use crate::authoring::terrain_ground::{cursor_for_tests, hold_cursor_result_for_tests, TerrainGroundCursor};
    for (enabled, control, expected) in [
        (true, false, [6.0, 2.0, 6.0]),
        (false, true, [6.0, 2.0, 6.0]),
        (true, true, [6.3, 2.0, 6.3]),
        (false, false, [6.3, 2.0, 6.3]),
    ] {
        let (mut app, id) = object_click_app(enabled);
        assert!(app.world().resource::<TerrainGroundCursor>().terrain.is_none());
        assert!(app.world().resource::<SceneEditorState>().cursor_world.is_none());
        start_object_click(&mut app, control);
        let release = hold_cursor_result_for_tests(app.world_mut());
        step(&mut app); // 真正命中前释放Ctrl及左键，不允许读取完成帧的修饰键。
        assert_author_unchanged(&app);
        release.send(()).unwrap();
        for _ in 0..1000 {
            app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
            cursor_for_tests(app.world_mut());
            step(&mut app);
            if app.world().resource::<EditorAuthoringSession>().revision != 7 { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, 8, "常开={enabled} Ctrl={control}");
        assert_eq!(session.undo.len(), 1);
        let record = session.draft.as_ref().unwrap().placements.placements.last().unwrap();
        assert_eq!(record.translation, expected, "常开={enabled} Ctrl={control}");
        assert_eq!(record.terrain_binding.as_ref().unwrap().terrain_id, id);
        for _ in 0..3 { step(&mut app); }
        assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 8);
    }
}

#[test]
fn switching_to_legacy_terrain_drains_object_click_without_erasing_new_hit() {
    use crate::authoring::terrain_ground::{cursor_for_tests, hold_cursor_result_for_tests, cursor_running_for_tests, TerrainGroundCursor};
    let (mut app, _) = object_click_app(false);
    start_object_click(&mut app, false);
    let release = hold_cursor_result_for_tests(app.world_mut());
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(
        crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![2; 121])),
    ); // 换回旧GLB作者源，上一张多网格场景的后台任务仍在途。
    let point = Some(Vec3::new(5.0, 2.0, 5.0));
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.cursor_world = point; editor.brush_cursor_world = point;
        editor.cursor_triangle = Some(120); editor.brush_cursor_triangle = Some(120);
    }
    cursor_for_tests(app.world_mut());
    assert!(cursor_running_for_tests(app.world())); // 不能丢弃句柄后提前归还CPU槽。
    release.send(()).unwrap();
    for _ in 0..1000 {
        cursor_for_tests(app.world_mut());
        let editor = app.world().resource::<SceneEditorState>();
        assert_eq!(editor.cursor_world, point);
        assert_eq!(editor.brush_cursor_world, point);
        assert_eq!(editor.brush_cursor_triangle, Some(120));
        assert!(editor.ground_click_world.is_none());
        if !cursor_running_for_tests(app.world()) { break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!cursor_running_for_tests(app.world()));
    assert!(app.world().resource::<TerrainGroundCursor>().click_terrain.is_none());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 7);
    assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn object_raw_click_cannot_revive_after_ui_blocks_and_releases_before_completion() {
    use crate::authoring::terrain_ground::{cursor_for_tests, hold_cursor_result_for_tests, cursor_running_for_tests, TerrainGroundCursor};
    for blocker in [Blocker::Numeric, Blocker::FolderDialog, Blocker::SceneDrag, Blocker::TerrainConflict] {
        let (mut app, _) = object_click_app(true);
        start_object_click(&mut app, false);
        let release = hold_cursor_result_for_tests(app.world_mut());
        set_blocker(&mut app, blocker, true);
        step(&mut app); // 先推进正式公共鼠标门，再由原始射线系统废止本次点击票据。
        cursor_for_tests(app.world_mut());
        assert_click_cleared(&app);
        assert_author_unchanged(&app);
        set_blocker(&mut app, blocker, false);
        step(&mut app);
        release.send(()).unwrap();
        for _ in 0..1000 {
            app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
            cursor_for_tests(app.world_mut());
            assert!(app.world().resource::<SceneEditorState>().ground_click_world.is_none(), "解除{blocker:?}后旧点击不得复活");
            assert!(app.world().resource::<TerrainGroundCursor>().click_terrain.is_none());
            step(&mut app);
            if !cursor_running_for_tests(app.world()) { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!cursor_running_for_tests(app.world()), "必须实际采用迟到结果再验证没有重放");
        assert_author_unchanged(&app);
    }
}

#[test]
fn object_cold_click_does_not_revive_when_zoom_or_projection_changes_and_returns() {
    use crate::authoring::terrain_ground::{cursor_for_tests, hold_cursor_result_for_tests, cursor_running_for_tests, TerrainGroundCursor};
    use crate::resources::SceneEditorViewportState;
    for change in 0..3 {
        let (mut app, _) = object_click_app(false);
        start_object_click(&mut app, true);
        let release = hold_cursor_result_for_tests(app.world_mut());
        let before = app.world().resource::<TerrainGroundCursor>().identity.clone().unwrap();
        match change {
            0 => {
                let extent = f32::from_bits(before.extent) * 2.0;
                app.world_mut().resource_mut::<SceneEditorViewportState>().extent = extent;
                app.world_mut().resource_mut::<TerrainGroundCursor>().identity.as_mut().unwrap().extent = extent.to_bits();
            }
            1 => app.world_mut().resource_mut::<TerrainGroundCursor>().identity.as_mut().unwrap().projection = Mat4::from_scale(Vec3::new(2.0, 2.0, 1.0)),
            _ => {
                app.world_mut().resource_mut::<SceneEditorState>().orthographic = !before.orthographic;
                // 相机缓存矩阵仍旧，投影模式意图先变化的一帧也必须废止旧点击。
                app.world_mut().resource_mut::<TerrainGroundCursor>().identity.as_mut().unwrap().orthographic = !before.orthographic;
            }
        }
        cursor_for_tests(app.world_mut());
        step(&mut app);
        app.world_mut().resource_mut::<SceneEditorViewportState>().extent = f32::from_bits(before.extent);
        app.world_mut().resource_mut::<SceneEditorState>().orthographic = before.orthographic;
        app.world_mut().resource_mut::<TerrainGroundCursor>().identity = Some(before);
        release.send(()).unwrap();
        for _ in 0..1000 {
            app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
            cursor_for_tests(app.world_mut());
            assert!(app.world().resource::<SceneEditorState>().ground_click_world.is_none(), "恢复投影场景{change}后旧点击不得复活");
            assert!(app.world().resource::<TerrainGroundCursor>().click_terrain.is_none());
            step(&mut app);
            if !cursor_running_for_tests(app.world()) { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!cursor_running_for_tests(app.world()));
        assert_author_unchanged(&app);
    }
}

#[test]
fn second_object_click_with_control_disabling_snap_replaces_older_active_click() {
    use crate::authoring::terrain_ground::{cursor_for_tests, hold_cursor_result_for_tests, TerrainGroundCursor};
    let (mut app, id) = object_click_app(true);
    start_object_click(&mut app, false);
    for _ in 0..1000 {
        app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
        cursor_for_tests(app.world_mut());
        step(&mut app);
        let snap = app.world().resource::<MeshSnapCursor>();
        if snap.pending.is_some() && !snap.awaiting_object_hit { break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.world().resource::<MeshSnapCursor>().pending.as_ref().unwrap().snap.active());
    let release_mesh = hold_mesh_result(&mut app);
    assert_author_unchanged(&app);
    app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
    app.world_mut().resource_mut::<TerrainGroundCursor>().ray.as_mut().unwrap().origin = Vec3::new(7.3, 20.0, 8.3);
    start_object_click(&mut app, true); // 同目标快速第二次点击：常开吸附由Ctrl临时关闭。
    let release_raw = hold_cursor_result_for_tests(app.world_mut());
    assert!(app.world().resource::<MeshSnapCursor>().pending.is_none());
    assert!(app.world().resource::<MeshSnapCursor>().completed.is_none());
    assert!(app.world().resource::<MeshSnapCursor>().task.is_some());
    step(&mut app); // 原始结果到达前已松Ctrl，不能继承第一笔或当前常开状态。
    release_raw.send(()).unwrap();
    for _ in 0..1000 {
        app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
        cursor_for_tests(app.world_mut());
        step(&mut app);
        if app.world().resource::<EditorAuthoringSession>().revision != 7 { break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 8);
    assert_eq!(session.undo.len(), 1);
    let record = session.draft.as_ref().unwrap().placements.placements.last().unwrap();
    assert_eq!(record.translation, [7.3, 2.0, 8.3]);
    assert_eq!(record.terrain_binding.as_ref().unwrap().terrain_id, id);
    assert!(app.world().resource::<MeshSnapCursor>().task.is_some()); // 第二笔无需等待或丢弃第一笔在途CPU槽。
    release_mesh.send(()).unwrap();
    for _ in 0..40 {
        app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
        cursor_for_tests(app.world_mut());
        step(&mut app);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 8);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn object_click_finishes_real_surface_snap_after_hover_moves_into_empty_space_and_control_releases() {
    use crate::authoring::terrain_ground::{TerrainGroundCursor, cursor_for_tests};
    for (enabled, control) in [(true, false), (false, true)] {
        let (mut app, id) = object_click_app(enabled);
        start_object_click(&mut app, control);
        for _ in 0..1000 {
            app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
            cursor_for_tests(app.world_mut());
            let hit = app.world().resource::<SceneEditorState>().ground_click_world.is_some();
            step(&mut app);
            if hit { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.world().resource::<MeshSnapCursor>().pending.is_some(), "真实单击进入吸附补查");
        assert!(!app.world().resource::<MeshSnapCursor>().awaiting_object_hit);
        let release = hold_mesh_result(&mut app); // 指针离开之前延后真实补查输出，保留同一个在途任务槽。
        app.world_mut().resource_mut::<TerrainGroundCursor>().ray.as_mut().unwrap().origin.x = 50.0;
        for _ in 0..1000 {
            app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
            cursor_for_tests(app.world_mut());
            step(&mut app);
            if app.world().resource::<TerrainGroundCursor>().terrain.is_none() { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.world().resource::<TerrainGroundCursor>().terrain.is_none());
        assert!(app.world().resource::<MeshSnapCursor>().pending.is_some(), "移出承托面不能丢弃已确认点击");
        assert_author_unchanged(&app);
        release.send(()).unwrap();
        for _ in 0..1000 {
            app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
            cursor_for_tests(app.world_mut());
            step(&mut app);
            if app.world().resource::<EditorAuthoringSession>().revision != 7 { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, 8);
        assert_eq!(session.undo.len(), 1);
        let record = session.draft.as_ref().unwrap().placements.placements.last().unwrap();
        assert_eq!(record.translation, [6.0, 2.0, 6.0]);
        assert_eq!(record.terrain_binding.as_ref().unwrap().terrain_id, id);
        for _ in 0..3 { step(&mut app); }
        assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 8); // 原始点击和吸附完成只消费一次。
    }
}

#[test]
fn object_raw_click_rejects_changed_snap_preferences_before_mesh_confirmation() {
    use crate::authoring::terrain_ground::{TerrainGroundCursor, cursor_for_tests};
    let (mut app, _) = object_click_app(true);
    start_object_click(&mut app, false);
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.translation_meters = 2.0;
    let changed = app.world().resource::<EditorSettings>().editor_snapping.translation_snap(false, [0.0; 3]);
    app.world_mut().resource_mut::<TerrainGroundCursor>().identity.as_mut().unwrap().translation_snap = Some(changed);
    for _ in 0..30 {
        app.world_mut().resource_mut::<SceneEditorState>().ground_click_world = None;
        cursor_for_tests(app.world_mut());
        assert!(app.world().resource::<SceneEditorState>().ground_click_world.is_none());
        step(&mut app);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_author_unchanged(&app);
    assert!(app.world().resource::<MeshSnapCursor>().pending.is_none());
    assert!(app.world().resource::<MeshSnapCursor>().completed.is_none());
}
