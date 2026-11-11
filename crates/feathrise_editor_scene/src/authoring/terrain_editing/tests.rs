//! 三种独立作者源的真实文件版本与回读回归；测试只使用独占临时目录。
use super::*;
use crate::types::EditorTool;
use bevy::prelude::*;
use feathrise_scene_content::*;
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        // Windows时钟可能让并行测试取得相同纳秒值；独占创建保留，并用进程序号避免串用作者目录。
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rpg-object-source-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        if self.0.parent() == Some(std::env::temp_dir().as_path()) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn object(kind: SceneTerrainKind) -> (SceneId, SceneTerrainObject) {
    let owner = SceneId::parse("MAP-source-test").unwrap();
    let id = TerrainId::parse("terrain-a").unwrap();
    let folder = format!("scenes/{owner}/terrains/{}", id.as_str());
    let object = SceneTerrainObject {clipmap_stitches: Default::default(),
        mesh_lod: None,
        id,
        display_name: "独立源".into(),
        enabled: true,
        transform: Default::default(),
        world_min: [-32.; 2],
        world_max: [32.; 2],
        terrain: SceneTerrainDescriptor {resolved_stitches: Default::default(),
            kind,
            resolution: [129; 2],
            base_height: -8.,
            height_scale: 16.,
            glb_path: format!("{folder}/terrain.glb"),
            height_path: Some(format!("{folder}/height_tiles.scene_height_manifest.json")),
            ..SceneTerrainDescriptor::empty()
        },
        voxel: None,
        surface_path: None,
        vegetation_path: None,
        water_path: None,
        puddles: None,
    };
    (owner, object)
}
fn brush(tool: EditorTool) -> ObjectBrushInput {
    ObjectBrushInput {
        stroke: 1,
        tool,
        center: Vec3::ZERO,
        radius: 2.,
        strength: 1.,
        speed: 24.,
        delta_seconds: 0.05,
        plane_point: Vec3::ZERO,
        shape: Default::default(),
        half_height: 1.,
    }
}
fn write(root: &std::path::Path, path: &str, bytes: &[u8]) {
    let target = root.join(path);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(target, bytes).unwrap();
}

fn sculpt_input_app() -> App {
    use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce};
    use crate::{authoring::{EditorAuthoringSession, terrain_documents::TerrainDocumentIdentity,
        terrain_ground::TerrainGroundCursor}, resources::SceneEditorState,
        ui::terrain_objects::TerrainObjectsEditorState,
        viewport::{LayerCandidateState, modes::{ViewportMode, ViewportModeState}}};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), crate::SceneEditorPlugin))
        .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<crate::ui::text_input::EditorTextInputGuard>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>();
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("sculpt-canvas").unwrap();
    object.terrain.kind = SceneTerrainKind::Heightfield;
    object.terrain.height_path = None; // 输入回归只核对后台事务受理；故意缺源使worker不能写项目资产。
    let source = object.source_manifest(&draft.manifest);
    let map = draft.scene_id.clone();
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(std::sync::Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: std::sync::Arc::new(object.clone()), manifest: source, folder: draft.folder.clone(),
    });
    app.insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .insert_resource(SceneEditorState { open: true, selected_scene_id: Some(map.clone()),
            tool: EditorTool::Raise, terrain_canvas_target: Some(object.id.clone()),
            brush_cursor_world: Some(Vec3::ZERO), ..default() });
    {
        let mut selection = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        selection.scene_id = Some(map); selection.selected = Some(object.id.clone()); selection.selected_index = Some(0);
    }
    app.world_mut().resource_mut::<LayerCandidateState>().confirm_terrain(&object.id, 0);
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    app.world_mut().resource_mut::<LayerCandidateState>().clear_for_scene_tree(); // 几何权限独立于检查器目标。
    app.world_mut().run_system_once(crate::viewport::poll_layer_candidates).unwrap();
    assert!(app.world().resource::<LayerCandidateState>().selected.is_none(), "编辑时检查候选清理不应撤销作者画布权限");
    app.world_mut().resource_mut::<TerrainGroundCursor>().terrain = Some(object.id.clone());
    app.world_mut().resource_mut::<TerrainGroundCursor>().test_paint_hit(object.id, Vec3::ZERO, None);
    app.world_mut().spawn((crate::components::SceneEditorViewport, bevy::ui::RelativeCursorPosition {
        cursor_over: true, normalized: Some(Vec2::ZERO),
    }));
    app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_millis(50));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app
}

#[test]
fn edit_mode_sculpt_uses_selected_canvas_after_inspector_selection_is_cleared() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::ui::terrain_objects::TerrainObjectsEditorState;
    let mut app = sculpt_input_app();
    app.world_mut().run_system_once(super::input::brush).unwrap();
    assert!(app.world().resource::<TerrainObjectsEditorState>().preparing(), "已确认画布、编辑模式和真实命中应受理抬高事务");
    assert!(app.world().resource::<super::resources::BrushGesture>().active.is_some());
}

fn finish_input_candidate(app: &mut App) {
    use bevy::ecs::system::RunSystemOnce;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.world().resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>().preparing() {
        app.world_mut().run_system_once(crate::ui::terrain_objects::poll_objects).unwrap();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn continuous_sculpt_completed_points_keep_one_held_gesture_without_strict_hover() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::{authoring::{EditorAuthoringSession, terrain_ground::TerrainGroundCursor}, resources::SceneEditorState};
    let mut app = sculpt_input_app();
    let id = app.world().resource::<SceneEditorState>().terrain_canvas_target.clone().unwrap();
    let mut stroke = None;
    for index in 0..3 {
        if index > 0 { app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear(); }
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.brush_cursor_world = None; // 鼠标仍在移动，最新射线尚未完成严格hover。
            editor.brush_terrain_id = None;
        }
        let mut cursor = app.world_mut().resource_mut::<TerrainGroundCursor>();
        cursor.terrain = None;
        cursor.test_paint_hit(id.clone(), Vec3::new(index as f32, 0.0, 0.0), None);
        drop(cursor);
        app.world_mut().run_system_once(super::input::brush).unwrap();
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, index + 1,
            "每个同按压后台完成真点都应受理，不能等待鼠标停下取得最新hover");
        let current = app.world().resource::<super::resources::BrushGesture>().active.as_ref().unwrap().2;
        if let Some(stroke) = stroke { assert_eq!(current, stroke); } else { stroke = Some(current); }
        finish_input_candidate(&mut app); // 缺源夹具只验证路由，worker明确失败且不会写项目资产。
    }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.world_mut().run_system_once(super::input::brush).unwrap();
    assert!(app.world().resource::<super::resources::BrushGesture>().active.is_none());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 3);
}

#[test]
fn continuous_sculpt_release_focus_viewport_mode_or_authority_change_never_replays_a_held_press() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::{authoring::{EditorAuthoringSession, terrain_ground::TerrainGroundCursor},
        resources::SceneEditorState, viewport::modes::{ViewportMode, ViewportModeState}};
    for case in 0..6 {
        let mut app = sculpt_input_app();
        app.world_mut().run_system_once(super::input::brush).unwrap();
        finish_input_candidate(&mut app);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        let window = app.world_mut().spawn((Window::default(), bevy::window::PrimaryWindow)).id();
        let viewport = {
            let world = app.world_mut();
            world.query_filtered::<Entity, With<crate::components::SceneEditorViewport>>().single(world).unwrap()
        };
        match case {
            0 => app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left),
            1 => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            2 => app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(viewport).unwrap().cursor_over = false,
            3 => app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select,
            4 => app.world_mut().resource_mut::<TerrainGroundCursor>().cancel_click(),
            _ => app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Lower,
        }
        app.world_mut().run_system_once(super::input::brush).unwrap();
        assert!(app.world().resource::<super::resources::BrushGesture>().active.is_none(), "case {case}");
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 1);
        // 恢复视口与工具只是恢复悬停，仍按住时不得把取消前的点补写成新笔。
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(viewport).unwrap().cursor_over = true;
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
        app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Raise;
        app.world_mut().run_system_once(super::input::brush).unwrap();
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 1, "case {case} 恢复不能重放旧按住");
    }
}

#[test]
fn sculpt_canvas_permission_rejects_non_edit_hidden_locked_loading_or_other_object_hits() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::{authoring::{EditorAuthoringSession, terrain_ground::TerrainGroundCursor},
        resources::SceneEditorState, ui::terrain_objects::TerrainObjectsEditorState,
        viewport::modes::{ViewportMode, ViewportModeState}};
    for case in 0..7 {
        let mut app = sculpt_input_app();
        let id = app.world().resource::<SceneEditorState>().terrain_canvas_target.clone().unwrap();
        match case {
            0 => app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select,
            1 => { app.world_mut().resource_mut::<SceneEditorState>().terrain_locked.insert(id); }
            2 => { std::sync::Arc::make_mut(&mut app.world_mut().resource_mut::<SceneEditorState>().terrain_hidden).insert(id); }
            3 => app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().terrain_documents.loading = true,
            4 => {
                let other = TerrainId::parse("other").unwrap();
                app.world_mut().resource_mut::<TerrainGroundCursor>().terrain = Some(other.clone());
                app.world_mut().resource_mut::<TerrainGroundCursor>().test_paint_hit(other, Vec3::ZERO, None);
            }
            5 => app.world_mut().resource_mut::<SceneEditorState>().terrain_canvas_target = Some(TerrainId::parse("other").unwrap()),
            _ => {
                // 选择与真实命中都已指向另一对象，原画布尚未交换；不得借旧画布权限写新对象。
                let other = TerrainId::parse("other").unwrap();
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let draft = session.draft.as_mut().unwrap();
                let first = draft.manifest.terrains.as_ref().unwrap()[0].clone();
                let mut second = first.clone(); second.id = other.clone();
                draft.manifest.terrains = Some(std::sync::Arc::from([first, second]));
                drop(session);
                let mut selection = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
                selection.selected = Some(other.clone()); selection.selected_index = Some(1);
                drop(selection);
                app.world_mut().resource_mut::<TerrainGroundCursor>().terrain = Some(other.clone());
                app.world_mut().resource_mut::<TerrainGroundCursor>().test_paint_hit(other, Vec3::ZERO, None);
            }
        }
        app.world_mut().run_system_once(super::input::brush).unwrap();
        assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing(), "case {case} 不得提交作者事务");
        assert!(app.world().resource::<super::resources::BrushGesture>().active.is_none(), "case {case} 不得起笔");
    }
}

#[test]
fn glb_object_sculpt_keeps_original_bytes_and_reopens_the_edited_topology() {
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, mut object) = object(SceneTerrainKind::Heightfield);
    object.terrain.resolution = [33; 2];
    let original = feathrise_terrain::build_flat_terrain_glb(
        feathrise_terrain::TerrainSampleSpec {
            resolution: [33; 2],
            world_min: [-32.; 2],
            world_max: [32.; 2],
            base_height: -8.,
            height_scale: 16.,
        },
        0.,
    )
    .unwrap();
    write(&temporary.0, &object.terrain.glb_path, &original);
    let edited = bevy::tasks::block_on(edit(
        temporary.0.clone(),
        owner,
        object.clone(),
        brush(EditorTool::Raise),
        "edit-test".into(),
    ))
    .unwrap();
    assert_ne!(edited.terrain.glb_path, object.terrain.glb_path);
    assert_eq!(
        std::fs::read(temporary.0.join(&object.terrain.glb_path)).unwrap(),
        original
    );
    let mesh = feathrise_terrain::parse_dynamic_terrain_glb(
        &std::fs::read(temporary.0.join(&edited.terrain.glb_path)).unwrap(),
    )
    .unwrap();
    assert!((mesh.positions[16 * 33 + 16][1] - 1.2).abs() < 0.001);
    assert_eq!(
        mesh.indices,
        feathrise_terrain::parse_dynamic_terrain_glb(&original)
            .unwrap()
            .indices
    );
}
#[test]
fn clipmap_object_sculpt_publishes_a_reopenable_private_version_and_leaves_original_flat() {
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, object) = object(SceneTerrainKind::SparseClipmap);
    let spec = SparseHeightSpec::new(object.world_min, object.world_max, 0.5, -8., 16.).unwrap();
    let mut document = SparseHeightDocument::create_for_terrain(
        &temporary.0,
        owner.clone(),
        Some(object.id.clone()),
        spec,
    )
    .unwrap();
    document.save_dirty().unwrap();
    let path = object.terrain.height_path.as_ref().unwrap();
    let baseline = std::fs::read(temporary.0.join(path)).unwrap();
    let edited = bevy::tasks::block_on(edit(
        temporary.0.clone(),
        owner.clone(),
        object.clone(),
        brush(EditorTool::Raise),
        "edit-test".into(),
    ))
    .unwrap();
    assert_eq!(std::fs::read(temporary.0.join(path)).unwrap(), baseline);
    let mut reopened = SparseHeightDocument::load_source(
        &temporary.0,
        &owner,
        Some(&object.id),
        edited.terrain.height_path.as_ref().unwrap(),
    )
    .unwrap();
    reopened
        .load_tile(SparseHeightTileKey { x: 0, z: 0 })
        .unwrap();
    assert!((reopened.sample_grid(64, 64).unwrap().unwrap() - 1.2).abs() < 0.01);
}

#[test]
fn tilted_glb_smooth_and_flatten_use_meter_speed_along_local_y_and_preserve_source_xz() {
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, mut object) = object(SceneTerrainKind::Heightfield);
    object.transform.translation = [12., 7., -9.];
    object.transform.yaw_degrees = 37.;
    object.transform.pitch_degrees = 31.;
    object.transform.roll_degrees = -22.;
    object.transform.scale = [2., 2., 0.5];
    let matrix = object.transform.as_transform().to_matrix();
    let source = feathrise_terrain::TerrainMeshInput { positions: vec![[0., 6., 0.], [1., 0., 0.], [0., 0., 1.]], indices: vec![0, 2, 1] };
    let original = feathrise_terrain::build_white_terrain_glb(&source).unwrap();
    write(&temporary.0, &object.terrain.glb_path, &original);
    for (tool_index, tool) in [EditorTool::Smooth, EditorTool::Flatten].into_iter().enumerate() {
        for (speed_index, (speed, expected)) in [(4., 5.9), (40., 5.), (1000., 2.)].into_iter().enumerate() {
            let input = ObjectBrushInput { center: matrix.transform_point3(Vec3::new(0., 6., 0.)), radius: 0.25,
                // 首次冻结点的XZ与当前刷印不同，必须仍整平到同一个本地Y=2平面。
                plane_point: matrix.transform_point3(Vec3::new(-1., 2., -1.)), speed, ..brush(tool) };
            let edited = bevy::tasks::block_on(edit(temporary.0.clone(), owner.clone(), object.clone(), input,
                format!("tilt-{tool_index}-{speed_index}"))).unwrap();
            let mesh = feathrise_terrain::parse_dynamic_terrain_glb(&std::fs::read(temporary.0.join(&edited.terrain.glb_path)).unwrap()).unwrap();
            assert!((mesh.positions[0][1] - expected).abs() < 0.00001, "tool={tool_index}, speed={speed}");
            for (after, before) in mesh.positions.iter().zip(&source.positions) {
                assert_eq!([after[0], after[2]], [before[0], before[2]], "旋转不能反烘焙进作者XZ");
            }
            assert_eq!(mesh.indices, source.indices);
            assert_eq!(edited.transform, object.transform);
        }
    }
    assert_eq!(std::fs::read(temporary.0.join(&object.terrain.glb_path)).unwrap(), original);
}

#[test]
fn scaled_glb_sculpt_keeps_unaffected_author_heights_bit_exact() {
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, mut object) = object(SceneTerrainKind::Heightfield);
    object.transform.scale[1] = 0.3;
    let unchanged_y = 0.013698630034923553_f32;
    let source = feathrise_terrain::TerrainMeshInput { positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 0., 1.],
        [10., unchanged_y, 10.], [11., unchanged_y, 10.], [10., unchanged_y, 11.]], indices: vec![0, 2, 1, 3, 5, 4] };
    let original = feathrise_terrain::build_white_terrain_glb(&source).unwrap();
    write(&temporary.0, &object.terrain.glb_path, &original);
    let edited = bevy::tasks::block_on(edit(temporary.0.clone(), owner, object,
        ObjectBrushInput { radius: 0.25, ..brush(EditorTool::Raise) }, "precise-local-edit".into())).unwrap();
    let mesh = feathrise_terrain::parse_dynamic_terrain_glb(&std::fs::read(temporary.0.join(&edited.terrain.glb_path)).unwrap()).unwrap();
    assert!(mesh.positions[0][1] > 0.);
    assert_eq!(&mesh.positions[1..], &source.positions[1..], "刷圈外高度不能受缩放往返影响");
}
#[test]
fn voxel_object_dig_writes_hashed_density_without_mutating_the_original_source() {
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, mut object) = object(SceneTerrainKind::Voxel);
    object.world_min = [-2.; 2];
    object.world_max = [2.; 2];
    object.terrain.glb_path.clear();
    object.terrain.height_path = None;
    let source = SceneVoxelTerrainSource {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: owner.clone(),
        terrain_id: object.id.clone(),
        cell_size_meters: 0.5,
        min: [-2., -2., -2.],
        max: [2., 0., 2.],
        blocks: Default::default(),
        surface_fields_path: None,
    };
    let path = format!(
        "{}/source.scene_voxel_source.json",
        object.private_folder(&owner)
    );
    object.voxel = Some(SceneVoxelTerrainDescriptor {
        cell_size_meters: source.cell_size_meters,
        min: source.min,
        max: source.max,
        source_path: path.clone(),
        display_lods: default_collision_lods(),
        collision_lods: default_collision_lods(),
    });
    let baseline = serde_json::to_vec_pretty(&source).unwrap();
    write(&temporary.0, &path, &baseline);
    let mut input = brush(EditorTool::VolumeDig);
    input.radius = 1.;
    let edited = bevy::tasks::block_on(edit(
        temporary.0.clone(),
        owner,
        object,
        input,
        "edit-test".into(),
    ))
    .unwrap();
    assert_eq!(std::fs::read(temporary.0.join(&path)).unwrap(), baseline);
    let reopened: SceneVoxelTerrainSource = serde_json::from_slice(
        &std::fs::read(temporary.0.join(&edited.voxel.unwrap().source_path)).unwrap(),
    )
    .unwrap();
    reopened.validate().unwrap();
    assert!(!reopened.blocks.is_empty());
    let reference = reopened
        .blocks
        .iter()
        .find(|reference| reference.key == [0; 3])
        .unwrap();
    let bytes = std::fs::read(temporary.0.join(reopened.block_path(&reference.sha256))).unwrap();
    assert_eq!(authoring_io::bytes_hash(&bytes), reference.sha256);
    assert!(i16::from_le_bytes([bytes[0], bytes[1]]) > 0); // 球心已从岩土/边界变为空气。
}

#[test]
fn native_surface_fields_reopen_without_a_clipmap_and_preserve_the_previous_source() {
    use feathrise_terrain::volume::sparse::paged::*;
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, mut object) = object(SceneTerrainKind::Voxel);
    object.world_min = [-2.; 2];
    object.world_max = [2.; 2];
    object.terrain.glb_path.clear();
    object.terrain.height_path = None;
    object.transform.translation = [20., 3., -10.];
    object.transform.scale = [2., 1., 0.5];
    object.transform.yaw_degrees = 35.;
    let source = SceneVoxelTerrainSource {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: owner.clone(),
        terrain_id: object.id.clone(),
        cell_size_meters: 0.5,
        min: [-2., -2., -2.],
        max: [2., 0., 2.],
        blocks: Default::default(),
        surface_fields_path: None,
    };
    let path = format!(
        "{}/source.scene_voxel_source.json",
        object.private_folder(&owner)
    );
    object.voxel = Some(SceneVoxelTerrainDescriptor {
        cell_size_meters: source.cell_size_meters,
        min: source.min,
        max: source.max,
        source_path: path.clone(),
        display_lods: default_collision_lods(),
        collision_lods: default_collision_lods(),
    });
    let original = serde_json::to_vec_pretty(&source).unwrap();
    write(&temporary.0, &path, &original);
    let frame =
        feathrise_scene_core::residency::SceneSourceFrame::from_transform(object.transform.as_transform());
    let collider = avian3d::prelude::Collider::trimesh(
        vec![
            Vec3::new(-2., 0., -2.),
            Vec3::new(2., 0., -2.),
            Vec3::new(2., 0., 2.),
            Vec3::new(-2., 0., 2.),
        ],
        vec![[0, 2, 1], [0, 3, 2]],
    );
    let point = frame.to_world(Vec3::ZERO);
    let input = NativeFieldInput {
        stroke: 1,
        query: feathrise_scene::TerrainObjectRaySnapshot::Mesh(collider, frame),
        ray: (point + Vec3::Y * 10., Vec3::NEG_Y),
        brush: VolumeFieldBrush {
            channel: VolumeFieldChannel::Surface,
            id: "stone".into(),
            from: point.to_array(),
            to: point.to_array(),
            normal: Vec3::Y.to_array(),
            radius: 1.,
            strength: 1.,
            flow_exposure: 0.,
            erase: false,
            source_to_world: Mat3::IDENTITY,
        },
    };
    let next = bevy::tasks::block_on(edit_fields(
        temporary.0.clone(),
        owner,
        object.clone(),
        input,
        "field-test".into(),
    ))
    .unwrap();
    assert_eq!(std::fs::read(temporary.0.join(&path)).unwrap(), original);
    let next_source: SceneVoxelTerrainSource = serde_json::from_slice(
        &std::fs::read(temporary.0.join(&next.voxel.unwrap().source_path)).unwrap(),
    )
    .unwrap();
    next_source.validate().unwrap();
    assert!(next_source.blocks.is_empty() && source.blocks.is_empty());
    let field_path = temporary.0.join(next_source.surface_fields_path.unwrap());
    let field = decode_paged_volume(&std::fs::read(&field_path).unwrap())
        .unwrap()
        .unwrap();
    let fields = load_surface_fields(
        &field_path.parent().unwrap().join("objects"),
        &field,
        &[[0.; 3]],
    )
    .unwrap();
    assert_eq!(
        fields.samples[&[0, 0, 0]].surface,
        vec![("stone".into(), 255)]
    );
    assert!(fields.samples.values().all(|sample| {
        frame
            .to_world(Vec3::from_array(sample.position))
            .distance(point)
            <= 1.001
    }));
    assert_eq!(object.voxel.unwrap().source_path, path); // 历史持有旧路径即可撤销，不需要恢复Clipmap。
}
