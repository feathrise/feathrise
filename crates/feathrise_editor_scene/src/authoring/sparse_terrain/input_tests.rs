//! Clipmap输入经正式局部页任务及历史收口；CPU夹具不声称完成GPU或窗口验收。
use super::*;
use bevy::ecs::system::RunSystemOnce;
use feathrise_scene_content::{SceneTerrainDescriptor, SparseHeightDocument, SparseHeightSpec, TerrainId};
use crate::{
    authoring::{terrain_documents::TerrainDocumentIdentity, terrain_ground::{CursorIdentity, TerrainGroundCursor}},
    ui::terrain_objects::TerrainObjectsEditorState,
    viewport::{LayerCandidateState, modes::{ViewportMode, ViewportModeState}},
};
use std::{path::PathBuf, time::{Duration, Instant}};

struct Fixture { app: App, root: PathBuf, map: SceneId, id: TerrainId, path: String }

impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("feathrise-sparse-input-{}-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        let mut app = crate::authoring::vegetation_paint::object_tests::fixture();
        app.init_resource::<TerrainObjectsEditorState>().init_resource::<LayerCandidateState>()
            .init_resource::<ViewportModeState>().init_resource::<GlobalEnvironmentAuthoringState>()
            .init_resource::<SparseHeightLiveOverride>();
        let (map, mut object) = {
            let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
            (draft.scene_id.clone(), draft.manifest.terrains.as_ref().unwrap()[0].clone())
        };
        let id = object.id.clone();
        let spec = SparseHeightSpec::new([-4.; 2], [4.; 2], 0.5, -8., 16.).unwrap();
        let mut document = SparseHeightDocument::create_for_terrain(&root, map.clone(), Some(id.clone()), spec.clone()).unwrap();
        document.save_dirty().unwrap();
        let path = format!("{}/height_tiles.scene_height_manifest.json", object.private_folder(&map));
        let author = SparseTerrainAuthorSession::load_source(&root, &map, Some(&id), &path).unwrap();
        object.world_min = spec.world_min; object.world_max = spec.world_max;
        object.terrain.kind = SceneTerrainKind::SparseClipmap;
        object.terrain.glb_path.clear(); object.terrain.height_path = Some(path.clone());
        object.terrain.resolution = [17; 2]; object.terrain.base_height = -8.; object.terrain.height_scale = 16.;
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.terrain_documents.active = Some(TerrainDocumentIdentity {
                definition: Arc::new(object.clone()), manifest: object.source_manifest(&draft.manifest), folder: object.private_folder(&map),
            });
            draft.manifest.terrain = SceneTerrainDescriptor::empty();
            draft.manifest.terrains = Some(Arc::from([object.clone()]));
            draft.terrain_mesh = None; draft.terrain_editing_index = None; draft.heights = None;
        }
        feathrise_scene::runtime_test_support::install_active_terrain_instances(app.world_mut(), &map, &[object]);
        app.world_mut().resource_mut::<SparseHeightLiveOverride>().retarget(Some(map.clone()), 0);
        app.insert_resource(SparseTerrainAuthoringState { desired_scene_id: Some(map.clone()),
            desired_source: Some((Some(id.clone()), path.clone())), author: Some(author), ..default() });
        feathrise_scene::runtime_test_support::publish_cpu_authoring_height(app.world_mut(), &map, &id, spec);
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.tool = EditorTool::Raise; editor.terrain_canvas_target = Some(id.clone());
            editor.brush_strength = 1.; editor.terrain_sculpt_speed = 24.;
        }
        let mut selection = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        selection.scene_id = Some(map.clone()); selection.selected = Some(id.clone()); selection.selected_index = Some(0);
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
        app.world_mut().resource_mut::<LayerCandidateState>().clear_for_scene_tree();
        assert!(app.world().resource::<LayerCandidateState>().selected.is_none());
        app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_millis(50));
        let mut fixture = Self { app, root, map, id, path };
        fixture.point(0.);
        fixture.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        fixture
    }

    /// 路由测试注入已经完成的公共命中；几何射线/取消本身由terrain_ground专项真实任务覆盖。
    fn point(&mut self, x: f32) {
        let session = self.app.world().resource::<EditorAuthoringSession>();
        let editor = self.app.world().resource::<SceneEditorState>();
        let identity = CursorIdentity { scene_id: self.map.clone(), revision: session.revision, terrain_revision: session.terrain_revision,
            tool: editor.tool, ground_snapshot: None, asset: None, asset_target: None, portal: None,
            camera: Mat4::IDENTITY, size: UVec2::splat(128), camera_entity: Entity::PLACEHOLDER, projection: Mat4::IDENTITY,
            view: editor.view, orthographic: false, extent: 16_f32.to_bits(), visibility: 0, relocation: None, translation_snap: None };
        let mut cursor = self.app.world_mut().resource_mut::<TerrainGroundCursor>();
        cursor.identity = Some(identity); cursor.terrain = None;
        cursor.test_paint_hit(self.id.clone(), Vec3::new(x, 0., 0.), None);
        let mut editor = self.app.world_mut().resource_mut::<SceneEditorState>();
        editor.brush_cursor_world = None; editor.brush_terrain_id = None; // 连续完成点不依赖严格hover。
    }

    fn apply(&mut self) { self.app.world_mut().run_system_once(apply_sparse_terrain_brush).unwrap(); }

    fn finish_task(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.app.world().resource::<SparseTerrainAuthoringState>().task.is_some() {
            self.app.world_mut().run_system_once(poll_sparse_terrain_task).unwrap();
            assert!(Instant::now() < deadline, "局部高度刷印必须有界完成");
            std::thread::sleep(Duration::from_millis(1));
        }
        let state = self.app.world().resource::<SparseTerrainAuthoringState>();
        assert!(!state.live_publish_failed);
        let spec = state.author.as_ref().unwrap().spec().clone();
        feathrise_scene::runtime_test_support::publish_cpu_authoring_height(self.app.world_mut(), &self.map, &self.id, spec);
    }

    fn finish(self) { let Self { app, root, .. } = self; drop(app); std::fs::remove_dir_all(root).unwrap(); }
}

#[test]
fn edit_mode_sculpt_uses_selected_canvas_after_inspector_selection_is_cleared() {
    let mut fixture = Fixture::new();
    fixture.apply();
    assert!(fixture.app.world().resource::<SparseTerrainAuthoringState>().task.is_some(), "Clipmap应受理正式局部页任务");
    assert!(fixture.app.world().resource::<TerrainObjectsEditorState>().brushing);
    fixture.finish_task();
    assert!(fixture.app.world().resource::<EditorAuthoringSession>().sparse_terrain_dirty);
    assert!(fixture.app.world().resource::<SparseTerrainAuthoringState>().author.as_ref().unwrap().sample_world_height(0., 0.).unwrap().unwrap() > 1.);
    fixture.finish();
}

#[test]
fn continuous_sculpt_completed_points_keep_one_held_gesture_without_strict_hover() {
    let mut fixture = Fixture::new();
    let baseline = std::fs::read(fixture.root.join(&fixture.path)).unwrap();
    let mut authority = None;
    for index in 0..3 {
        if index > 0 { fixture.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear(); }
        fixture.point(index as f32);
        fixture.apply();
        let state = fixture.app.world().resource::<SparseTerrainAuthoringState>();
        assert!(state.stroke.active && state.task.is_some());
        let previous = (index > 0).then(|| Vec3::new((index - 1) as f32, 0., 0.));
        assert_eq!(state.validation_applied_sculpt_point(), previous, "在途刷印不得冒充已采用落点");
        if let Some(previous) = authority { assert_eq!(state.stroke.input_authority, Some(previous)); }
        else { authority = state.stroke.input_authority; }
        fixture.finish_task();
        assert_eq!(fixture.app.world().resource::<SparseTerrainAuthoringState>().validation_applied_sculpt_point(),
            Some(Vec3::new(index as f32, 0., 0.)), "同一次按压应记录每次实际采用的世界落点");
        let session = fixture.app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.terrain_revision, index + 1, "每个完成点应发布一次局部页修订");
        assert!(session.undo.is_empty(), "持续按压不能把刷印拆成多条历史");
    }
    fixture.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    fixture.apply();
    assert!(!fixture.app.world().resource::<SparseTerrainAuthoringState>().stroke.active);
    assert!(fixture.app.world().resource::<SparseTerrainAuthoringState>().validation_applied_sculpt_point().is_none());
    assert_eq!(fixture.app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    assert_eq!(std::fs::read(fixture.root.join(&fixture.path)).unwrap(), baseline, "保存前局部刷印不应改写源清单");
    fixture.finish();
}

#[test]
fn continuous_sculpt_release_focus_viewport_mode_or_authority_change_never_replays_a_held_press() {
    for case in 0..6 {
        let mut fixture = Fixture::new();
        fixture.apply(); fixture.finish_task();
        fixture.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        let window = fixture.app.world_mut().spawn((Window::default(), bevy::window::PrimaryWindow)).id();
        let viewport = { let world = fixture.app.world_mut(); world.query_filtered::<Entity, With<SceneEditorViewport>>().single(world).unwrap() };
        match case {
            0 => fixture.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left),
            1 => fixture.app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            2 => fixture.app.world_mut().get_mut::<RelativeCursorPosition>(viewport).unwrap().cursor_over = false,
            3 => fixture.app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select,
            4 => fixture.app.world_mut().resource_mut::<TerrainGroundCursor>().cancel_click(),
            _ => fixture.app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Lower,
        }
        if case == 1 { crate::authoring::terrain_ground::cursor_for_tests(fixture.app.world_mut()); }
        fixture.apply();
        let state = fixture.app.world().resource::<SparseTerrainAuthoringState>();
        assert!(!state.stroke.active && state.task.is_none(), "case {case} 必须收口而非保留旧按压");
        fixture.app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        fixture.app.world_mut().get_mut::<RelativeCursorPosition>(viewport).unwrap().cursor_over = true;
        fixture.app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
        fixture.app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Raise;
        fixture.point(1.);
        fixture.apply();
        assert!(fixture.app.world().resource::<SparseTerrainAuthoringState>().task.is_none(), "case {case} 恢复不能重放旧按压");
        assert_eq!(fixture.app.world().resource::<EditorAuthoringSession>().terrain_revision, 1);
        fixture.finish();
    }
}

#[test]
fn sculpt_canvas_permission_rejects_non_edit_hidden_locked_loading_or_other_object_hits() {
    for case in 0..7 {
        let mut fixture = Fixture::new();
        match case {
            0 => fixture.app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select,
            1 => { fixture.app.world_mut().resource_mut::<SceneEditorState>().terrain_locked.insert(fixture.id.clone()); }
            2 => { Arc::make_mut(&mut fixture.app.world_mut().resource_mut::<SceneEditorState>().terrain_hidden).insert(fixture.id.clone()); }
            3 => fixture.app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().terrain_documents.loading = true,
            4 => fixture.app.world_mut().resource_mut::<TerrainGroundCursor>().test_paint_hit(TerrainId::parse("other").unwrap(), Vec3::ZERO, None),
            5 => fixture.app.world_mut().resource_mut::<SceneEditorState>().terrain_canvas_target = Some(TerrainId::parse("other").unwrap()),
            _ => {
                let mut target = fixture.app.world_mut().resource_mut::<feathrise_scene::TerrainObjectAuthoringTarget>();
                target.terrain = Some(TerrainId::parse("other").unwrap()); // requested已变、正式作者根未交换，不能借上一对象的页。
            }
        }
        fixture.apply();
        let state = fixture.app.world().resource::<SparseTerrainAuthoringState>();
        assert!(state.task.is_none(), "case {case} 不得提交局部页任务");
        assert!(!state.author.as_ref().unwrap().is_dirty());
        assert_eq!(fixture.app.world().resource::<EditorAuthoringSession>().terrain_revision, 0);
        fixture.finish();
    }
}
