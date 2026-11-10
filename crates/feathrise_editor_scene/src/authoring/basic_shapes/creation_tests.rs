//! 直接执行正式视口创建入口，验证参数记录、唯一可视/碰撞预览和单条历史的完整采用。

use super::{resources::BasicShapeState, *};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand, EditorPreviewAssets},
    components::{EditorPlacedAsset, SceneEditorViewport},
    resources::SceneEditorState,
    ui::{
        asset_preview::AssetPreviewState, gltf_browser::EditorGltfBrowserState,
        placement_transform::PlacementTransformInputState, sidebars::SceneEditorSidebarState,
    },
    viewport::modes::{ViewportMode, ViewportModeState},
};
use avian3d::prelude::{Collider, RigidBody};
use bevy::ecs::system::RunSystemOnce;
use bevy::{asset::AssetPlugin, prelude::*, ui::RelativeCursorPosition};
use feathrise_scene_content::{SceneId, SceneManifestAsset, ScenePlacementAssetTarget, ScenePlacementHeightMode};

fn app(empty: bool) -> App {
    fixture(empty, false)
}

#[test]
fn author_identity_reopened_legacy_cuboid_keeps_original_through_create_undo_redo() {
    use feathrise_scene_content::{ScenePlacementRecord, ScenePlacementsAsset};
    use feathrise_scene::ScenePlacementInstance;

    let mut app = fixture(true, false);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
    app.world_mut().resource_mut::<SceneEditorState>().tool = crate::types::EditorTool::Inspect;
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    app.init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .add_observer(crate::ui::placement_water::register_placement_instance_root)
        .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
    let original: ScenePlacementRecord = serde_json::from_value(serde_json::json!({
        "id": "placement-2-1", "asset_path": "", "translation": [4.0, 1.0, 0.0],
        "asset_target": ScenePlacementAssetTarget::DEFAULT_CUBOID,
        "height_mode": "fixed_height", "yaw_degrees": 0.0, "scale": [1.0, 1.0, 1.0]
    })).unwrap();
    let map = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placements.placements = vec![original.clone()];
        // 正式JSON往返重建保存后的文档；冷开会话的修订为1、显示计数为0。
        draft.placements = serde_json::from_slice(&serde_json::to_vec(&draft.placements).unwrap()).unwrap();
        let map = draft.scene_id.clone();
        session.revision = 1;
        session.next_serial = 0;
        map
    };
    let original_root = app.world_mut().spawn((
        ScenePlacementInstance::new(&map, &original),
        Transform::from_translation(original.translation.into()), Visibility::Inherited,
    )).id();
    let create = |mut commands: Commands, assets: Res<AssetServer>,
                  mut editor: ResMut<SceneEditorState>, mut session: ResMut<EditorAuthoringSession>| {
        crate::authoring::edit::create_cuboid_at(&mut commands, &assets, &mut editor, &mut session,
            ScenePlacementAssetTarget::DEFAULT_CUBOID, Vec3::new(7.0, 1.0, 2.0)).unwrap()
    };
    let id = app.world_mut().run_system_once(create).unwrap();
    assert_ne!(id, original.id, "重开后的首次创建不能覆盖已保存对象的稳定身份");
    let added = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let records = &session.draft.as_ref().unwrap().placements.placements;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0], original);
        assert_eq!(records[1].id, id);
        assert_eq!(records[1].translation, [7.0, 1.0, 2.0]);
        assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
        records[1].clone()
    };
    app.world_mut().resource_mut::<SceneEditorState>().undo_requested = true;
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    {
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, vec![original.clone()]);
        assert_eq!((session.undo.len(), session.redo.len()), (0, 1));
        assert!(!session.placement_previews.contains_key(&id));
    }
    assert_eq!(app.world().get::<Visibility>(original_root), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<Transform>(original_root).unwrap().translation, Vec3::new(4.0, 1.0, 0.0));
    app.world_mut().resource_mut::<SceneEditorState>().redo_requested = true;
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    {
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, vec![original.clone(), added]);
        assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
        assert!(app.world().get::<EditorPlacedAsset>(session.placement_previews[&id]).is_some());
    }
    // 同一进程再次打开也会复位场景修订/显示序号；身份计数必须继续前进。
    let persisted = serde_json::to_vec(&app.world().resource::<EditorAuthoringSession>()
        .draft.as_ref().unwrap().placements).unwrap();
    let reopened: ScenePlacementsAsset = serde_json::from_slice(&persisted).unwrap();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements = reopened;
        session.revision = 1;
        session.next_serial = 0;
        session.undo.clear();
        session.redo.clear();
    }
    let next = app.world_mut().run_system_once(create).unwrap();
    assert_ne!(next, id);
    assert_ne!(next, original.id);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements.len(), 3);
    assert_eq!(session.draft.as_ref().unwrap().placements.placements[0], original);
}

fn fixture(empty: bool, with_preview: bool) -> App {
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    if empty {
        draft.manifest.terrains = Some(Vec::new().into());
    } else {
        draft.manifest.terrains = None;
    }
    let map = draft.scene_id.clone();
    let mut mode = ViewportModeState::default();
    mode.mode = ViewportMode::Edit;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<SceneManifestAsset>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<SceneEditorSidebarState>()
        .insert_resource(mode)
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            ..default()
        })
        .insert_resource(SceneEditorState {
            open: true,
            tool: crate::types::EditorTool::PlaceAsset,
            selected_asset: Some(CUBOID_SELECTION.into()),
            selected_scene_id: Some(map.clone()),
            cursor_world: Some(Vec3::new(3.0, 6.0, 4.0)),
            ..default()
        })
        .insert_resource(BasicShapeState {
            work_plane: empty.then_some((map.clone(), 7, Vec3::new(3.0, 0.0, 4.0))),
            placement_target: Some(ScenePlacementAssetTarget::DEFAULT_CUBOID),
            placement_identity: Some((map.clone(), 7)),
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
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>();
    if with_preview {
        app.insert_resource(crate::resources::SceneEditorViewportState {
            _image: Handle::default(),
            camera: Entity::PLACEHOLDER,
            center: Vec3::ZERO,
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::ONE,
            extent: 50.0,
            yaw: 0.0,
            pitch: 0.6,
            framed_scene_id: Some(map),
            render_size: UVec2::new(800, 600),
        })
        .add_systems(
            Update,
            (
                super::placement::update_preview,
                crate::authoring::place_from_viewport,
            )
                .chain(),
        );
    } else {
        app.add_systems(Update, crate::authoring::place_from_viewport);
    }
    app.world_mut().spawn((
        SceneEditorViewport,
        ComputedNode::default(),
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app
}

#[test]
fn empty_scene_creation_uses_the_current_frame_camera_and_cursor_instead_of_old_plane_cache() {
    for changed_camera in [false, true] {
        let mut app = fixture(true, true);
        app.world_mut()
            .resource_mut::<feathrise_editor_core::EditorSettings>()
            .editor_snapping
            .enabled = false;
        // 先无点击生成旧相机工作平面，再仅在点击帧改变输入，覆盖正式Cursor→Presentation顺序。
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        app.update();
        let old = app
            .world()
            .resource::<BasicShapeState>()
            .work_plane_point(app.world().resource::<EditorAuthoringSession>())
            .unwrap();
        let normalized = if changed_camera {
            Vec2::ZERO
        } else {
            Vec2::new(0.12, 0.10)
        };
        if changed_camera {
            app.world_mut()
                .resource_mut::<crate::resources::SceneEditorViewportState>()
                .center
                .x += 8.0;
        }
        let world = app.world_mut();
        for mut cursor in world
            .query_filtered::<&mut RelativeCursorPosition, With<SceneEditorViewport>>()
            .iter_mut(world)
        {
            cursor.normalized = Some(normalized);
        }
        let expected = super::placement::plane_hit(
            crate::viewport::placement_move::MoveCamera::new(
                app.world().resource::<SceneEditorState>(),
                app.world()
                    .resource::<crate::resources::SceneEditorViewportState>(),
                1.0,
            )
            .unwrap()
            .ray(normalized)
            .unwrap(),
        )
        .unwrap();
        assert!(expected.distance(old) > 1.0, "测试落点必须与旧缓存不同");
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        let records = &session.draft.as_ref().unwrap().placements.placements;
        assert_eq!(records.len(), 1);
        assert!(Vec3::from_array(records[0].translation).abs_diff_eq(expected + Vec3::Y, 0.001));
        assert_eq!(session.undo.len(), 1);
    }
}

fn relocation_app() -> (App, Entity) {
    let mut app = app(true);
    app.update();
    let (id, structure, map, root) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
        record.scale[1] = 1.5;
        record.translation[1] = 12.0;
        let id = record.id.clone();
        let map = session.draft.as_ref().unwrap().scene_id.clone();
        let root = session.placement_previews[&id];
        session.undo.clear();
        (id, session.placement_structure_revision, map, root)
    };
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement(&id, 0, structure);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .cursor_world = None;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    app.init_resource::<crate::ui::building_presets::BuildingPlacementControls>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<crate::viewport::VegetationGroundCursor>()
        .add_message::<crate::authoring::buildings::BuildingEditRequest>()
        .insert_resource(crate::resources::SceneEditorViewportState {
            _image: Handle::default(),
            camera: Entity::PLACEHOLDER,
            center: Vec3::new(8.0, 0.0, 6.0),
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::ONE,
            extent: 50.0,
            yaw: 0.0,
            pitch: 0.6,
            framed_scene_id: Some(map),
            render_size: UVec2::new(800, 600),
        });
    let button = app
        .world_mut()
        .spawn((
            feathrise_editor_ui::EditorButtonClick::triggered(),
            crate::ui::placement_transform::SceneEditorPlacementTransformActionButton(
                crate::ui::placement_transform::PlacementTransformAction::Relocate,
            ),
        ))
        .id();
    app.world_mut()
        .run_system_once(crate::ui::placement_transform::handle_placement_transform_actions)
        .unwrap();
    app.world_mut().despawn(button);
    assert!(
        app.world()
            .resource::<PlacementTransformInputState>()
            .relocation_armed()
    );
    app.add_systems(
        Update,
        (
            super::placement::update_preview,
            crate::ui::placement_transform::apply_selected_placement_relocation,
        )
            .chain()
            .after(crate::authoring::place_from_viewport),
    );
    (app, root)
}

#[test]
fn empty_scene_cube_relocation_consumes_current_plane_once_and_places_scaled_bottom_on_it() {
    let (mut app, root) = relocation_app();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    let session = app.world().resource::<EditorAuthoringSession>();
    let record = &session.draft.as_ref().unwrap().placements.placements[0];
    assert!(Vec3::from_array(record.translation).abs_diff_eq(Vec3::new(8.0, 1.5, 6.0), 0.001));
    assert_eq!(record.height_mode, ScenePlacementHeightMode::FixedHeight);
    assert!(record.terrain_binding.is_none());
    assert_eq!(session.undo.len(), 1);
    assert!(
        app.world()
            .get::<Transform>(root)
            .unwrap()
            .translation
            .abs_diff_eq(Vec3::new(8.0, 1.5, 6.0), 0.001)
    );
    assert!(
        !app.world()
            .resource::<PlacementTransformInputState>()
            .relocation_armed()
    );
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn empty_scene_cube_relocation_rejects_stale_scene_revision_modal_or_legacy_terrain() {
    for case in 0..4 {
        let (mut app, _) = relocation_app();
        let before = app
            .world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements[0]
            .clone();
        match case {
            0 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .selected_scene_id = Some(SceneId::parse("MAP-other-relocation").unwrap())
            }
            1 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .revision += 1
            }
            2 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
            }
            _ => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .draft
                    .as_mut()
                    .unwrap()
                    .manifest
                    .terrains = None
            }
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(
            session.draft.as_ref().unwrap().placements.placements[0],
            before,
            "拒绝场景{case}"
        );
        assert!(session.undo.is_empty());
    }
}

#[test]
fn custom_cuboid_point_placement_uses_frozen_dimensions_material_and_bottom_offset() {
    let mut app = fixture(true, true);
    let target = ScenePlacementAssetTarget::Cuboid {
        size_mm: [4000, 6000, 8000], color_srgb: [80, 120, 180], roughness_milli: 250, collision: false,
    };
    app.world_mut().resource_mut::<BasicShapeState>().placement_target = Some(target);
    app.world_mut().run_system_once(super::placement::setup_preview).unwrap();
    app.update();
    let session = app.world().resource::<EditorAuthoringSession>();
    let record = &session.draft.as_ref().unwrap().placements.placements[0];
    assert_eq!(record.asset_target, target);
    assert!((record.translation[1] - 3.0).abs() < 1e-6); // 毫米转米的f32乘法可能有一个末位误差。
    assert_eq!(record.terrain_offset, 0.0);
    assert_eq!(session.undo.len(), 1);
    let world = app.world_mut();
    let (transform, visible, handle) = world.query_filtered::<(&Transform, &Visibility, &MeshMaterial3d<StandardMaterial>), With<super::components::CuboidGhost>>().single(world).unwrap();
    assert!(transform.scale.abs_diff_eq(Vec3::new(2.0, 3.0, 4.0), 1e-6));
    assert!((transform.translation.y - 3.0).abs() < 1e-6);
    assert_eq!(transform.translation.y, transform.scale.y); // 共享2米盒体的实际半高必须恰好等于中心偏移。
    assert_eq!(*visible, Visibility::Inherited);
    let material = world.resource::<Assets<StandardMaterial>>().get(&handle.0).unwrap();
    assert_eq!(material.base_color, Color::srgba(80.0 / 255.0, 120.0 / 255.0, 180.0 / 255.0, 0.4));
    assert!((material.perceptual_roughness - 0.25).abs() < 0.0001);
}

#[test]
fn viewport_cube_creation_atomically_installs_one_record_mesh_collider_history_and_selection() {
    for empty in [true, false] {
        let mut app = app(empty);
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        let records = &session.draft.as_ref().unwrap().placements.placements;
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.asset_path, "");
        assert_eq!(record.asset_target, ScenePlacementAssetTarget::DEFAULT_CUBOID);
        assert_eq!(
            record.translation,
            [3.0, if empty { 1.0 } else { 7.0 }, 4.0]
        );
        assert_eq!(record.scale, [1.0; 3]);
        assert_eq!(record.editor.name, "立方体");
        assert_eq!(
            record.height_mode,
            if empty {
                ScenePlacementHeightMode::FixedHeight
            } else {
                ScenePlacementHeightMode::FollowTerrain
            }
        );
        assert_eq!(record.terrain_offset, if empty { 0.0 } else { 1.0 });
        assert!(record.terrain_binding.is_none());
        assert_eq!(session.undo.len(), 1);
        assert!(
            matches!(&session.undo[0], EditorCommand::PlacementAdded(added) if added == record)
        );
        assert!(session.redo.is_empty());
        let root = session.placement_previews[&record.id];
        assert_eq!(
            app.world()
                .resource::<EditorGltfBrowserState>()
                .selected_placement_id
                .as_deref(),
            Some(record.id.as_str())
        );
        assert_eq!(app.world().get::<RigidBody>(root), Some(&RigidBody::Static));
        let children = app.world().get::<Children>(root).unwrap();
        assert_eq!(children.len(), 1);
        let visual = children[0];
        assert!(app.world().get::<Mesh3d>(visual).is_some());
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(visual)
                .is_some()
        );
        assert!(app.world().get::<Collider>(visual).is_some());
        assert!(
            app.world()
                .get::<feathrise_scene::ScenePlacementPrimitiveVisual>(visual)
                .is_some()
        );
        assert!(
            app.world().get::<WorldAssetRoot>(root).is_none(),
            "内置几何不能把空路径交给外部Scene加载"
        );
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            crate::types::EditorTool::Inspect
        );
        assert_eq!(
            app.world().resource::<ViewportModeState>().mode,
            ViewportMode::Select
        );
        assert!(
            app.world()
                .resource::<SceneEditorState>()
                .selected_asset
                .is_none()
        );
        for _ in 0..3 {
            app.update();
        }
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<EditorPlacedAsset>>()
                .iter(world)
                .count(),
            1
        );
        assert_eq!(world.resource::<EditorAuthoringSession>().undo.len(), 1);
    }
}

#[test]
fn cancelled_foreign_or_stale_empty_scene_cube_click_never_creates_data_or_preview() {
    for case in 0..5 {
        let mut app = app(true);
        match case {
            0 => {
                app.world_mut().resource_mut::<SceneEditorState>().tool =
                    crate::types::EditorTool::Inspect
            }
            1 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .selected_scene_id = Some(SceneId::parse("MAP-other-cube").unwrap())
            }
            2 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .revision += 1
            }
            3 => app.world_mut().resource_mut::<BasicShapeState>().work_plane = None,
            _ => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
            }
        }
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<EditorPlacedAsset>>()
                .iter(world)
                .count(),
            0,
            "拒绝场景{case}"
        );
        let session = world.resource::<EditorAuthoringSession>();
        assert!(
            session
                .draft
                .as_ref()
                .unwrap()
                .placements
                .placements
                .is_empty()
        );
        assert!(session.undo.is_empty() && session.placement_previews.is_empty());
    }
}
