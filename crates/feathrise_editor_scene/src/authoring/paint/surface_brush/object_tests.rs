//! 显式Clipmap地表起笔的真实相机→单飞Collider射线→笔刷/历史回归，不改磁盘场景。

use super::*;
use crate::{
    authoring::{
        SparseTerrainAuthoringState, poll_surface_layer_edit,
        terrain_documents::TerrainDocumentIdentity,
        terrain_ground::{
            TerrainGroundSnapshot, cursor_for_tests, cursor_running_for_tests,
            hold_cursor_result_for_tests,
        },
    },
    components::SceneEditorCamera,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorViewportState},
    ui::{asset_preview::AssetPreviewState, placement_transform::PlacementTransformInputState},
    viewport::sync_editor_viewport_camera,
};
use bevy::{
    asset::AssetPlugin,
    camera::{ComputedCameraValues, RenderTargetInfo},
};
use feathrise_scene_content::{
    SCENE_SURFACE_SCHEMA_VERSION, SceneCollisionLod, SceneSurfaceAsset, SceneSurfaceLayer,
    SceneTerrainDescriptor, SceneTerrainKind, ResolvedSurfacePresetCatalog,
    SURFACE_PRESET_SCHEMA_VERSION, SurfacePresetCatalogAsset, SurfacePresetDescriptor,
    SurfaceProjection, TerrainId, TerrainObjectTransform,
};
use std::time::{Duration, Instant};

fn resolve_cursor(world: &mut World) {
    cursor_for_tests(world);
}

fn registry() -> SurfacePresetRegistry {
    let catalog = SurfacePresetCatalogAsset {
        schema_version: SURFACE_PRESET_SCHEMA_VERSION,
        texture_resolution: [4; 2],
        base_color_array_path: "config/surface/base.ktx2".into(),
        normal_array_path: "config/surface/normal.ktx2".into(),
        arm_height_array_path: "config/surface/arm.ktx2".into(),
        presets: [("preset-wet-mud", "湿泥"), ("preset-rammed-earth", "夯土")]
            .into_iter()
            .map(|(id, label)| SurfacePresetDescriptor {
                id: id.into(),
                label: label.into(),
                aliases: Vec::new(),
                base_color_srgba: [1.0; 4],
                base_color_texture_path: None,
                normal_texture_path: None,
                arm_height_texture_path: None,
                tile_meters: 4.0,
                normal_strength: 1.0,
                normal_y_flip: false,
                height_blend_strength: 0.0,
                perceptual_roughness: 1.0,
                metallic: 0.0,
                crater_response: 1.0,
                projection: SurfaceProjection::WorldXz,
            })
            .collect(),
    };
    SurfacePresetRegistry {
        catalog: Handle::default(),
        revision: 1,
        last_error: None,
        resolved: ResolvedSurfacePresetCatalog::build(&catalog).unwrap(),
    }
}

fn fixture() -> App {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("surface-real-clipmap").unwrap();
    object.world_min = [-32.0; 2];
    object.world_max = [32.0; 2];
    object.transform = TerrainObjectTransform::default();
    let folder = object.private_folder(&draft.scene_id);
    object.terrain = SceneTerrainDescriptor {resolved_stitches: Default::default(),
        kind: SceneTerrainKind::SparseClipmap,
        height_path: Some(format!("{folder}/height_tiles.scene_height_manifest.json")),
        resolution: [257; 2],
        base_height: -16.0,
        height_scale: 32.0,
        collision_lods: vec![SceneCollisionLod {
            max_distance_meters: 64.0,
            spacing_multiplier: 1,
        }],
        ..SceneTerrainDescriptor::empty()
    };
    let mut canvas_manifest = draft.manifest.clone();
    canvas_manifest.world_min = object.world_min;
    canvas_manifest.world_max = object.world_max;
    canvas_manifest.terrain = object.terrain.clone();
    canvas_manifest.terrains = None;
    let definition = Arc::new(object);
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([(*definition).clone()]));
    feathrise_scene_content::validate_terrain_objects(&draft.manifest).unwrap();
    draft.heights = None;
    draft.terrain_mesh = None;
    draft.terrain_editing_index = None;
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: definition.clone(),
        manifest: canvas_manifest,
        folder: folder.clone(),
    });
    // 查询真实上向三角平面，绝不使用基准高度或人工命中点替代后台结果。
    draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(
        draft.scene_id.clone(),
        definition.id.clone(),
        definition.transform,
        avian3d::prelude::Collider::trimesh(
            vec![
                Vec3::new(-32.0, 2.0, -32.0),
                Vec3::new(32.0, 2.0, -32.0),
                Vec3::new(32.0, 2.0, 32.0),
                Vec3::new(-32.0, 2.0, 32.0),
            ],
            vec![[0, 2, 1], [0, 3, 2]],
        ),
    ));
    draft.surface = Some(SceneSurfaceAsset {
        schema_version: SCENE_SURFACE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        weight_source_path: format!("{folder}/surface/weights.rgba8"),
        weight_texture_path: format!("{folder}/surface/weights.ktx2"),
        index_source_path: format!("{folder}/surface/indices.rgba32"),
        index_texture_path: format!("{folder}/surface/indices.ktx2"),
        resolution: [3; 2],
        layers: vec![
            SceneSurfaceLayer {
                id: "wet-mud".into(),
                label: "湿泥".into(),
                preset_id: "preset-wet-mud".into(),
            },
            SceneSurfaceLayer {
                id: "rammed-earth".into(),
                label: "夯土".into(),
                preset_id: "preset-rammed-earth".into(),
            },
        ],
    });
    draft.surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(9)));
    draft.surface_indices = Some(Arc::new(vec![0; 9 * 16]));
    let map = draft.scene_id.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(registry())
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(map),
            tool: EditorTool::Surface,
            selected_surface_layer: 1,
            surface_selected_preset_id: Some("preset-rammed-earth".into()),
            terrain_canvas_target: Some(definition.id.clone()),
            view: EditorView::Top,
            brush_radius: 2.0,
            brush_strength: 0.5,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        })
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<SurfaceLayerEditTask>()
        .init_resource::<SurfaceStrokeTrail>()
        .init_resource::<VegetationGroundCursor>()
        .init_resource::<TerrainGroundCursor>()
        .init_resource::<SparseTerrainAuthoringState>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .add_systems(
            Update,
            (
                sync_editor_viewport_camera,
                resolve_cursor,
                poll_surface_layer_edit,
                apply_surface_brush.run_if(
                    crate::ui::numeric_scrub_mouse_guard::scene_editor_numeric_scrub_mouse_available,
                ),
            )
                .chain(),
        );
    let size = UVec2::splat(128);
    let camera = app
        .world_mut()
        .spawn((
            SceneEditorCamera,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: size,
                        scale_factor: 1.0,
                    }),
                    clip_from_view: bevy::math::proj::perspective_infinite_reverse(
                        50_f32.to_radians(),
                        1.0,
                        0.1,
                    ),
                    ..default()
                },
                ..default()
            },
            Projection::default(),
            Transform::default(),
            feathrise_scene::DynamicClipmapAuthoringView {
                focus_xz: Vec2::ZERO,
                focus_y: 2.0,
            },
        ))
        .id();
    let (yaw, pitch) = (0.0, std::f32::consts::FRAC_PI_2);
    app.insert_resource(SceneEditorViewportState {
        _image: Handle::default(),
        camera,
        center: Vec3::new(0.0, 2.0, 0.0),
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::splat(32.0),
        extent: 16.0,
        yaw,
        pitch,
        framed_scene_id: None,
        render_size: size,
    });
    app.world_mut().spawn((
        SceneEditorViewport,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app
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

fn until(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        assert!(Instant::now() < deadline, "真实单飞射线及笔刷必须有限完成");
        step(app);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn warm_hover(app: &mut App) {
    until(app, |app| {
        app.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
            && !cursor_running_for_tests(app.world())
    });
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .unwrap()
            .distance(Vec3::new(0.0, 2.0, 0.0))
            < 0.001
    );
}

fn begin_waiting(app: &mut App) -> std::sync::mpsc::Sender<()> {
    warm_hover(app);
    let before_epoch = app
        .world()
        .resource::<TerrainGroundCursor>()
        .ray
        .unwrap()
        .epoch;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(app);
    assert_ne!(
        app.world()
            .resource::<TerrainGroundCursor>()
            .ray
            .unwrap()
            .epoch,
        before_epoch
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_none(),
        "真实按下换代必须撤销旧hover命中"
    );
    assert!(!app.world().resource::<SurfaceBrushStroke>().active);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    hold_cursor_result_for_tests(app.world_mut())
}

fn pixels(app: &App) -> (Vec<u8>, Vec<u8>) {
    let draft = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap();
    (
        (**draft.surface_weights.as_ref().unwrap()).clone(),
        (**draft.surface_indices.as_ref().unwrap()).clone(),
    )
}

#[test]
fn explicit_clipmap_surface_press_survives_real_cursor_epoch_delay_and_creates_one_stroke() {
    for blank in [true, false] {
        let mut app = fixture();
        if blank {
            app.world_mut()
                .resource_mut::<EditorAuthoringSession>()
                .draft
                .as_mut()
                .unwrap()
                .surface_weights = Some(Arc::new(vec![0; 9 * 4]));
        }
        let before = pixels(&app);
        let release = begin_waiting(&mut app);
        assert_eq!(pixels(&app), before);
        release.send(()).unwrap();
        until(&mut app, |app| pixels(app) != before);
        assert!(
            app.world().resource::<SurfaceBrushStroke>().active,
            "仍按住时真实命中到达才能起笔"
        );
        let (weights, indices) = pixels(&app);
        assert!(
            (0..4).any(|slot| {
                let offset = 4 * 16 + slot * 4;
                u32::from_le_bytes(indices[offset..offset + 4].try_into().unwrap()) == 1
                    && weights[4 * 4 + slot] > 0
            }),
            "中心纹素必须写入选中的夯土层实际索引和权重"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        until(&mut app, |app| {
            !app.world().resource::<SurfaceBrushStroke>().active
        });
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().undo.len(),
            1
        );
        for _ in 0..3 {
            step(&mut app);
        }
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().undo.len(),
            1
        );
    }
}

#[test]
fn explicit_surface_waiting_press_cancels_after_release_or_any_author_target_change() {
    for variant in 0..11 {
        let mut app = fixture();
        let before = pixels(&app);
        let release = begin_waiting(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let revision = session.revision;
        let surface_revision = session.surface_revision;
        let snapshot = session.draft.as_ref().unwrap().terrain_ground.clone();
        let map = app
            .world()
            .resource::<SceneEditorState>()
            .selected_scene_id
            .clone();
        let target = app
            .world()
            .resource::<SceneEditorState>()
            .terrain_canvas_target
            .clone();
        let hidden = app
            .world()
            .resource::<SceneEditorState>()
            .terrain_hidden
            .clone();
        match variant {
            0 => app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left),
            1 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .terrain_canvas_target = Some(TerrainId::parse("other-canvas").unwrap())
            }
            2 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .selected_surface_layer = 0
            }
            3 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .surface_selected_preset_id = None
            }
            4 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .selected_scene_id = Some(feathrise_scene_content::SceneId::parse("MAP-other").unwrap())
            }
            5 => {
                app.world_mut()
                    .resource_mut::<SceneEditorViewportState>()
                    .extent = 20.0
            }
            6 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .terrain_hidden = Arc::new(Default::default())
            }
            7 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .revision += 1
            }
            8 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .surface_revision += 1
            }
            9 => {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let old = snapshot.as_ref().unwrap();
                let pose = TerrainObjectTransform::default();
                let replacement = TerrainGroundSnapshot::test_queries(
                    old.scene_id.clone(),
                    old.generation + 1,
                    [(target.clone().unwrap(), pose,
                        feathrise_scene::TerrainObjectRaySnapshot::Mesh(
                            avian3d::prelude::Collider::cuboid(64.0, 1.0, 64.0),
                            feathrise_scene_core::residency::SceneSourceFrame::from_transform(pose.as_transform())),
                        [1; 32])],
                );
                session.draft.as_mut().unwrap().terrain_ground = Some(replacement);
            }
            _ => app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ShiftLeft),
        }
        step(&mut app); // 来源变化必须立即废票；恢复原选择时仍不能自动起笔。
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.selected_scene_id = map;
            editor.terrain_canvas_target = target;
            editor.selected_surface_layer = 1;
            editor.surface_selected_preset_id = Some("preset-rammed-earth".into());
            editor.terrain_hidden = hidden;
        }
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.revision = revision;
            session.surface_revision = surface_revision;
            session.draft.as_mut().unwrap().terrain_ground = snapshot;
        }
        app.world_mut()
            .resource_mut::<SceneEditorViewportState>()
            .extent = 16.0;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ShiftLeft);
        release.send(()).unwrap();
        until(&mut app, |app| !cursor_running_for_tests(app.world()));
        for _ in 0..4 {
            step(&mut app);
        }
        assert_eq!(pixels(&app), before, "case {variant}");
        assert!(
            !app.world().resource::<SurfaceBrushStroke>().active,
            "case {variant}"
        );
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty(),
            "case {variant}"
        );
    }
}

#[test]
fn explicit_surface_skipped_input_system_cannot_revive_old_press_after_authority_returns() {
    let mut app = fixture();
    let before = pixels(&app);
    let release = begin_waiting(&mut app);
    let authority = app
        .world()
        .resource::<TerrainGroundCursor>()
        .input_authority()
        .unwrap();
    app.world_mut()
        .resource_mut::<SceneEditorNumericScrubMouseGuard>()
        .advance(true, true, false);
    step(&mut app); // 与正式调度相同，笔刷被run_if跳过，cursor仍运行并取消权限代次。
    assert!(
        app.world()
            .resource::<TerrainGroundCursor>()
            .input_authority()
            .is_none()
    );
    app.world_mut()
        .resource_mut::<SceneEditorNumericScrubMouseGuard>()
        .advance(false, false, false);
    release.send(()).unwrap();
    until(&mut app, |app| !cursor_running_for_tests(app.world()));
    for _ in 0..4 {
        step(&mut app);
    }
    assert_ne!(
        app.world()
            .resource::<TerrainGroundCursor>()
            .input_authority()
            .unwrap(),
        authority
    );
    assert_eq!(pixels(&app), before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

fn dense_surface(app: &mut App) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    draft.surface.as_mut().unwrap().resolution = [129; 2];
    draft.surface_weights = Some(Arc::new(vec![0; 129 * 129 * 4]));
    draft.surface_indices = Some(Arc::new(vec![0; 129 * 129 * 16]));
    drop(session);
    app.world_mut().resource_mut::<SceneEditorState>().brush_radius = 0.6;
}

fn move_pointer(app: &mut App, x: f32) {
    let world = app.world_mut();
    let mut viewport = world.query_filtered::<&mut RelativeCursorPosition, With<SceneEditorViewport>>();
    viewport.single_mut(world).unwrap().normalized = Some(Vec2::new(x, 0.0));
}

fn begin_drag(app: &mut App) {
    warm_hover(app);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(app);
    until(app, |app| app.world().resource::<SurfaceBrushStroke>().active
        && app.world().resource::<SurfaceBrushStroke>().pending_sample.is_none());
}

#[test]
fn explicit_surface_continuous_drag_consumes_delayed_real_points_and_fills_the_entire_segment() {
    let mut app = fixture();
    dense_surface(&mut app);
    begin_drag(&mut app);
    let start = app.world().resource::<SurfaceBrushStroke>().last_world.unwrap();
    move_pointer(&mut app, 0.25);
    step(&mut app);
    let release = hold_cursor_result_for_tests(app.world_mut());
    move_pointer(&mut app, 0.45);
    for _ in 0..3 { step(&mut app); }
    assert_eq!(app.world().resource::<SurfaceBrushStroke>().last_world, Some(start),
        "同源后台等待只能暂停，不能每次移动都把连续刷段清成孤立点");
    assert!(app.world().resource::<TerrainGroundCursor>().paint_pending());
    release.send(()).unwrap();
    until(&mut app, |app| app.world().resource::<SurfaceBrushStroke>().last_world
        .is_some_and(|point| point.x > 1.0));
    let endpoint = app.world().resource::<SurfaceBrushStroke>().last_world.unwrap();
    assert!(endpoint.x < 5.0, "先采用0.25处的真实完成点，不能冒用0.45处的新射线");
    assert!(app.world().resource::<SceneEditorState>().brush_cursor_world.is_none(),
        "连续画笔不能把旧射线完成点冒充当前严格悬停或单击");
    let (weights, _) = pixels(&app);
    for x in 65..=70 {
        let texel = 64 * 129 + x;
        assert!(weights[texel * 4..texel * 4 + 4].iter().any(|weight| *weight > 0),
            "真实端点之间的第{x}列不得留白");
    }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<SurfaceBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1,
        "延迟和中间补刷仍是同一笔撤销");
}

#[test]
fn explicit_surface_delayed_query_keeps_hold_exposure_without_extra_dabs_or_history() {
    let mut contributions = Vec::new();
    for frames in [1, 6] {
        let mut app = fixture();
        dense_surface(&mut app);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(20)));
        app.world_mut().resource_mut::<SceneEditorState>().brush_strength = 0.25;
        // 保留已有背景，空白像素归一化为255会掩盖持续曝光的实际差异。
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
            .surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(129 * 129)));
        begin_drag(&mut app);
        move_pointer(&mut app, 0.25);
        step(&mut app);
        let release = hold_cursor_result_for_tests(app.world_mut());
        move_pointer(&mut app, 0.45);
        for _ in 0..frames { step(&mut app); }
        assert!(app.world().resource::<TerrainGroundCursor>().paint_pending());
        assert!(app.world().resource::<SurfaceBrushStroke>().waiting_amount
            .is_some_and(|(erase, amount)| !erase && amount > 0.0));
        // 查询恢复后的非确定轮询帧不增加时间，让最终像素只比较真实按住的等待时间。
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        release.send(()).unwrap();
        until(&mut app, |app| app.world().resource::<SurfaceBrushStroke>().last_world
            .is_some_and(|point| point.x > 1.0));
        assert!(app.world().resource::<SurfaceBrushStroke>().waiting_amount.is_none());
        let (weights, indices) = pixels(&app);
        let texel = 64 * 129 + 67; // 原起笔圈外、已验证连续段内的真实作者像素。
        let contribution: u16 = (0..4).filter_map(|slot| {
            let offset = texel * 16 + slot * 4;
            (u32::from_le_bytes(indices[offset..offset + 4].try_into().unwrap()) == 1)
                .then_some(u16::from(weights[texel * 4 + slot]))
        }).sum();
        assert!(contribution > 0 && contribution < 255);
        contributions.push(contribution);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        until(&mut app, |app| !app.world().resource::<SurfaceBrushStroke>().active);
        assert!(app.world().resource::<SurfaceBrushStroke>().waiting_amount.is_none());
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    }
    assert!(contributions[1] > contributions[0],
        "等待六帧的真实曝光不能退化为仅恢复帧的一次盖印：{contributions:?}");
}

#[test]
fn explicit_surface_continuous_drag_rejects_late_points_after_release_or_camera_change() {
    for camera_changed in [false, true] {
        let mut app = fixture();
        dense_surface(&mut app);
        begin_drag(&mut app);
        let before = pixels(&app);
        move_pointer(&mut app, 0.25);
        step(&mut app);
        let release = hold_cursor_result_for_tests(app.world_mut());
        if camera_changed {
            app.world_mut().resource_mut::<SceneEditorViewportState>().extent = 20.0;
        } else {
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        }
        step(&mut app);
        release.send(()).unwrap();
        until(&mut app, |app| !cursor_running_for_tests(app.world()));
        for _ in 0..3 { step(&mut app); }
        assert_eq!(pixels(&app), before, "相机/按压换代之后不能补写迟到位置");
        assert!(!app.world().resource::<SurfaceBrushStroke>().active);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    }
}

#[test]
fn object_cursor_moving_hover_keeps_a_display_point_without_granting_brush_or_click_permission() {
    let mut app = fixture();
    warm_hover(&mut app);
    assert!(app.world().resource::<TerrainGroundCursor>().display_hit().is_some());
    move_pointer(&mut app, 0.25);
    step(&mut app);
    let release = hold_cursor_result_for_tests(app.world_mut());
    move_pointer(&mut app, 0.45);
    step(&mut app);
    assert!(app.world().resource::<TerrainGroundCursor>().display_hit().is_some(),
        "移动期间保留上一同来源真实点，不能不停切回普通鼠标");
    assert!(app.world().resource::<TerrainGroundCursor>().paint_hit().is_none());
    release.send(()).unwrap();
    until(&mut app, |app| app.world().resource::<TerrainGroundCursor>().display_hit()
        .is_some_and(|hit| hit.point.x > 1.0));
    assert!(app.world().resource::<SceneEditorState>().ground_click_world.is_none());
    assert!(app.world().resource::<TerrainGroundCursor>().paint_hit().is_none());
    assert!(!app.world().resource::<SurfaceBrushStroke>().active);
}

#[test]
fn explicit_surface_continuous_drag_survives_unrelated_snapshot_adoption_as_one_history() {
    use feathrise_scene::TerrainObjectRaySnapshot;
    use feathrise_scene_core::residency::SceneSourceFrame;
    let mut app = fixture();
    dense_surface(&mut app);
    begin_drag(&mut app);
    let start = app.world().resource::<SurfaceBrushStroke>().last_world.unwrap();
    let (map, id, pose, query) = {
        let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
        let active = &draft.terrain_documents.active.as_ref().unwrap().definition;
        (draft.scene_id.clone(), active.id.clone(), active.transform,
            draft.terrain_ground.as_ref().unwrap().object_query(&active.id).unwrap())
    };
    let sibling_pose = TerrainObjectTransform { translation: [70., 10., 40.], ..default() };
    let adopted = TerrainGroundSnapshot::test_queries(map, 10, [
        (id, pose, query, [0; 32]),
        (TerrainId::parse("unrelated-surface").unwrap(), sibling_pose,
            TerrainObjectRaySnapshot::Mesh(avian3d::prelude::Collider::cuboid(2., 1., 2.),
                SceneSourceFrame::from_transform(sibling_pose.as_transform())), [8; 32]),
    ]);
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().terrain_ground = Some(adopted);
    move_pointer(&mut app, 0.25);
    until(&mut app, |app| app.world().resource::<SurfaceBrushStroke>().last_world
        .is_some_and(|point| point.xz().distance(start.xz()) > 1.));
    assert!(app.world().resource::<SurfaceBrushStroke>().active);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty(),
        "整场Arc换代不能提前收口原按住笔划");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<SurfaceBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

fn surface_slope_fixture() -> (App, feathrise_scene_content::SceneId, TerrainId, Vec3) {
    let mut app = fixture();
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    let ground = feathrise_scene::runtime_test_support::clipmap_height_query_fixture(map.clone(), 0);
    let feathrise_scene::ClipmapGroundHit::Hit(center) = ground.sample_authoritative_surface(Vec2::new(22., 12.))
        else { panic!("自有真实坡面必须有同代作者页"); };
    assert!(center.y > 5.);
    let id = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = (*draft.terrain_documents.active.as_ref().unwrap().definition).clone();
        object.world_min = [0.; 2]; object.world_max = [96., 64.];
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        let manifest = object.source_manifest(&draft.manifest);
        let active = draft.terrain_documents.active.as_mut().unwrap();
        active.definition = Arc::new(object.clone()); active.manifest = manifest;
        draft.surface.as_mut().unwrap().resolution = [97, 65];
        draft.surface_weights = Some(Arc::new(vec![0; 97 * 65 * 4]));
        draft.surface_indices = Some(Arc::new(vec![0; 97 * 65 * 16]));
        object.id
    };
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = center;
    (app, map, id, center)
}

fn install_surface_slope(app: &mut App, ground: feathrise_scene::ClipmapGroundSnapshot,
    generation: u64, geometry: u8, blocker: Option<feathrise_scene::TerrainObjectRaySnapshot>) {
    use feathrise_scene::TerrainObjectRaySnapshot;
    use feathrise_scene_core::residency::SceneSourceFrame;
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    let object = &draft.terrain_documents.active.as_ref().unwrap().definition;
    let mut sources = vec![(object.id.clone(), object.transform,
        TerrainObjectRaySnapshot::Clipmap(ground, SceneSourceFrame::from_transform(object.transform.as_transform())), [geometry; 32])];
    if let Some(query) = blocker {
        let frame = match &query { TerrainObjectRaySnapshot::Mesh(_, frame) | TerrainObjectRaySnapshot::Clipmap(_, frame) => frame };
        let pose = TerrainObjectTransform { translation: frame.source_to_world.w_axis.truncate().to_array(), ..default() };
        sources.push((TerrainId::parse("surface-blocker").unwrap(), pose, query, [9; 32]));
    }
    draft.terrain_ground = Some(TerrainGroundSnapshot::test_queries(draft.scene_id.clone(), generation, sources));
}

fn begin_cold_surface(app: &mut App, map: feathrise_scene_content::SceneId) -> feathrise_scene::ClipmapGroundSnapshot {
    let (cold, adopt) = feathrise_scene::runtime_test_support::cold_clipmap_height_query_fixture(map, 0);
    let old_revision = cold.authoritative_page_revision();
    install_surface_slope(app, cold, 1, 1, None);
    until(app, |app| app.world().resource::<SceneEditorState>().brush_cursor_world.is_some()
        && !cursor_running_for_tests(app.world()));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(app);
    until(app, |app| !cursor_running_for_tests(app.world()));
    assert!(!app.world().resource::<SurfaceBrushStroke>().active);
    assert!(app.world().resource::<TerrainGroundCursor>().paint_hit().is_none());
    assert!(pixels(app).0.iter().all(|value| *value == 0));
    let adopted = adopt(); // 页需求必须来自原光标的真实作者精度查询。
    assert!(adopted.authoritative_page_revision() > old_revision);
    adopted
}

#[test]
fn explicit_surface_cold_press_survives_author_page_adoption_and_continuous_drag() {
    let (mut app, map, _, center) = surface_slope_fixture();
    let adopted = begin_cold_surface(&mut app, map);
    let epoch = app.world().resource::<TerrainGroundCursor>().ray.unwrap().epoch;
    install_surface_slope(&mut app, adopted, 2, 1, None);
    until(&mut app, |app| app.world().resource::<SurfaceBrushStroke>().active
        && !app.world().resource::<SurfaceBrushStroke>().before_weights.is_empty());
    assert!(app.world().resource::<SurfaceBrushStroke>().last_world
        .is_some_and(|point| point.distance(center) < 0.01));
    assert_eq!(app.world().resource::<TerrainGroundCursor>().ray.unwrap().epoch, epoch);
    move_pointer(&mut app, 0.12);
    until(&mut app, |app| app.world().resource::<SurfaceBrushStroke>().last_world
        .is_some_and(|point| point.xz().distance(center.xz()) > 1.));
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<SurfaceBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn explicit_surface_cold_press_rejects_changed_target_new_occluder_and_unknown_occluder_pages() {
    use feathrise_scene::{ClipmapGroundHit, TerrainObjectRaySnapshot};
    use feathrise_scene_core::residency::SceneSourceFrame;
    for variant in 0..3 {
        let (mut app, map, _, _) = surface_slope_fixture();
        let adopted = begin_cold_surface(&mut app, map.clone());
        let ray = app.world().resource::<TerrainGroundCursor>().ray.unwrap();
        let blocker = match variant {
            0 => None,
            1 => {
                let visible = adopted.ray_surface(ray.origin, ray.direction, 1_000_000.).unwrap().0;
                Some(TerrainObjectRaySnapshot::Mesh(avian3d::prelude::Collider::cuboid(4., 1., 4.),
                    SceneSourceFrame::from_transform(Transform::from_translation(ray.origin.lerp(visible, 0.5)))))
            }
            _ => {
                let mut unknown = feathrise_scene::runtime_test_support::clipmap_height_query_fixture(map, 0);
                unknown.ready = false; // 故障夹具保留真实范围但尚无可查询显示页，不能视作没有遮挡。
                Some(TerrainObjectRaySnapshot::Clipmap(unknown,
                    SceneSourceFrame::from_transform(Transform::from_xyz(0., 20., 0.))))
            }
        };
        install_surface_slope(&mut app, adopted, 2, if variant == 0 { 2 } else { 1 }, blocker);
        if variant == 2 {
            let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
            assert!(matches!(draft.terrain_ground.as_ref().unwrap()
                .ray_visible_checked(ray.origin, ray.direction, &Default::default()), Err(ClipmapGroundHit::WaitingForHeight)));
        }
        step(&mut app);
        until(&mut app, |app| !cursor_running_for_tests(app.world()));
        for _ in 0..4 { step(&mut app); }
        assert!(!app.world().resource::<SurfaceBrushStroke>().active, "case {variant}");
        assert!(pixels(&app).0.iter().all(|value| *value == 0), "case {variant}");
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty(), "case {variant}");
    }
}
