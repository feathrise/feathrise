//! 固定几何的遮挡/约束与真实 ECS 输入、草稿隔离、确认和取消回归。

use feathrise_editor_ui::UiInteraction;
use super::{geometry::*, input::handle_input, resources::MeshEditState, types::*};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand},
    resources::SceneEditorState,
    viewport::{
        modes::{ViewportMode, ViewportModeState},
        placement_move::interaction_tests as fixture,
    },
};
use bevy::prelude::*;
use feathrise_scene_content::{ScenePlacementAssetTarget, ScenePlacementRecord};

fn record() -> ScenePlacementRecord {
    let mut record = fixture::record();
    record.asset_path.clear();
    record.water_surfaces.clear();
    record.asset_target = ScenePlacementAssetTarget::DEFAULT_CUBOID;
    record.yaw_degrees = 0.0;
    record.scale = [1.0; 3];
    record
}

fn app() -> (App, Entity, Entity) {
    let (mut app, root, node) = fixture::app();
    let record = record();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements[0] = record.clone();
    }
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .placement_transform_enabled = false;
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .scene_id
        .clone();
    app.world_mut()
        .entity_mut(root)
        .insert(feathrise_scene::ScenePlacementInstance::new(&map, &record));
    app.init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<MeshEditState>()
        .add_systems(
            Update,
            handle_input.in_set(super::MeshEditInput).before(crate::viewport::placement_move::handle_placement_move),
        );
    (app, root, node)
}

fn current(app: &App) -> &ScenePlacementRecord {
    &app.world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements[0]
}

fn press(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
}

fn request(app: &mut App, action: MeshEditAction) {
    app.world_mut().resource_mut::<MeshEditState>().request = Some(action);
}

fn enter(app: &mut App, node: Entity) {
    press(app, KeyCode::Tab);
    fixture::step(app, node, Vec2::ZERO);
    assert!(app.world().resource::<MeshEditState>().active());
    fixture::step(app, node, Vec2::ZERO); // Tab 的静默帧归还旧点击释放。
}

#[test]
fn shared_vertex_masks_and_kind_conversion_preserve_real_topology() {
    assert_eq!(selected_vertices(MeshElementKind::Edge, 1), 0b00000011); // 第一条 X 向棱共用两个逻辑顶点。
    let face = element_vertices(MeshElementKind::Face, 1);
    assert_eq!(face.count_ones(), 4);
    let vertex_selection =
        selection_for_kind(MeshElementKind::Face, MeshElementKind::Vertex, 1 << 1);
    assert_eq!(vertex_selection as u64, face);
    assert_eq!(
        selection_for_kind(
            MeshElementKind::Vertex,
            MeshElementKind::Face,
            vertex_selection
        ),
        1 << 1
    );
}

#[test]
fn world_delta_uses_inverse_root_transform_and_zero_delta_keeps_parametric_form() {
    let mut before = record();
    before.yaw_degrees = 90.0;
    before.scale = [2.0, 3.0, 4.0];
    assert_eq!(
        translated_record(&before, 0xff, Vec3::ZERO).unwrap(),
        before
    );
    let vertices = world_vertices(&before).unwrap();
    let after = translated_record(&before, 1 << 7, Vec3::new(0.25, 0.5, -0.75)).unwrap();
    let moved = world_vertices(&after).unwrap();
    assert!((moved[7] - vertices[7] - Vec3::new(0.25, 0.5, -0.75)).length() < 0.002);
    for i in 0..7 {
        assert_eq!(moved[i], vertices[i]);
    }
    assert_eq!(after.translation, before.translation);
    assert_eq!(after.scale, before.scale);
    assert_eq!(after.yaw_degrees, before.yaw_degrees);
}

#[test]
fn back_vertices_and_edges_are_occluded_by_nearest_real_face() {
    let (app, _, _) = app();
    let camera = fixture::camera(&app);
    let vertices = world_vertices(&record()).unwrap();
    let center = Vec3::from_array(record().translation);
    let ray = camera.ray(camera.project(center).unwrap()).unwrap();
    let (face, distance) = nearest_face(ray, &vertices).unwrap();
    assert!(distance > 0.0);
    assert!(face == 4 || face == 5);
    let visible = (0..8)
        .filter(|&i| visible_point(camera, vertices[i], &vertices))
        .count();
    assert_eq!(visible, 4);
    assert_eq!(
        pick_element(
            MeshElementKind::Face,
            camera,
            camera.project(center).unwrap(),
            &vertices
        ),
        Some(face)
    );
}

#[test]
fn true_input_preview_keeps_draft_and_commit_writes_one_complete_history() {
    let (mut app, _, node) = app();
    enter(&mut app, node);
    let before = current(&app).clone();
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Face));
    fixture::step(&mut app, node, Vec2::ZERO);
    app.world_mut().resource_mut::<MeshEditState>().selected = 1 << 1;
    press(&mut app, KeyCode::KeyG);
    fixture::step(&mut app, node, Vec2::ZERO);
    assert!(app.world().resource::<MeshEditState>().moving());
    press(&mut app, KeyCode::KeyX);
    fixture::step(&mut app, node, Vec2::new(0.025, 0.0));
    assert_eq!(current(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    let candidate = app
        .world()
        .resource::<MeshEditState>()
        .movement
        .as_ref()
        .unwrap()
        .preview
        .clone();
    assert_ne!(candidate.asset_target, before.asset_target);
    press(&mut app, KeyCode::Enter);
    fixture::step(&mut app, node, Vec2::new(0.025, 0.0));
    assert_eq!(current(&app), &candidate);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(
        matches!(&session.undo[0],EditorCommand::PlacementChanged{before:old,after:new,..} if old==&before && new==&candidate)
    );
    assert!(app.world().resource::<SceneEditorState>().dirty);
    fixture::step(&mut app, node, Vec2::new(0.025, 0.0));
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    assert!(app.world().resource::<MeshEditState>().active());
    assert!(!app.world().resource::<MeshEditState>().moving());
}

#[test]
fn cancel_and_stale_author_identity_do_not_publish_candidate_or_history() {
    for stale in [false, true] {
        let (mut app, _, node) = app();
        enter(&mut app, node);
        let before = current(&app).clone();
        request(&mut app, MeshEditAction::Move);
        fixture::step(&mut app, node, Vec2::ZERO);
        fixture::step(&mut app, node, Vec2::new(0.04, 0.01));
        if stale {
            app.world_mut()
                .resource_mut::<EditorAuthoringSession>()
                .revision += 1;
        } else {
            press(&mut app, KeyCode::Escape);
        }
        fixture::step(&mut app, node, Vec2::new(0.04, 0.01));
        assert_eq!(current(&app), &before);
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        assert!(!app.world().resource::<MeshEditState>().moving());
        assert_eq!(app.world().resource::<MeshEditState>().active(), !stale);
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
}

#[test]
fn modal_document_change_and_f12_exit_cancel_before_existing_close_flow() {
    for close in [false, true] {
        let (mut app, _, node) = app();
        enter(&mut app, node);
        let before = current(&app).clone();
        request(&mut app, MeshEditAction::Move);
        fixture::step(&mut app, node, Vec2::ZERO);
        fixture::step(&mut app, node, Vec2::new(0.04, 0.01));
        if close {
            press(&mut app, KeyCode::F12);
        } else {
            app.world_mut()
                .resource_mut::<SceneEditorState>()
                .new_scene_dialog_open = true;
        }
        fixture::step(&mut app, node, Vec2::new(0.04, 0.01));
        assert!(!app.world().resource::<MeshEditState>().active());
        assert_eq!(current(&app), &before);
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        if close {
            assert!(app.world().resource::<SceneEditorState>().close_requested);
        }
    }
}

#[test]
fn real_asset_menu_owner_blocks_mesh_shortcuts_without_locking_its_closed_state() {
    use crate::{
        components::{AssetRowBinding, SceneEditorAssetRow, SceneEditorRoot},
        resources::EditorAssetCatalog,
        types::EditorAssetEntry,
        ui::{
            asset_context_menu::{self, AssetContextMenuState},
            external_files::{ExternalEditors, OpenExternalFile},
            sidebars::{EditorNavigatorTab, SceneEditorSidebarState},
        },
    };
    let (mut app, _, node) = app();
    app.init_resource::<AssetContextMenuState>()
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<SceneEditorSidebarState>()
        .insert_resource(ExternalEditors::for_tests(&[]))
        .add_message::<asset_context_menu::CopyAssetPath>()
        .add_message::<OpenExternalFile>();
    app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = EditorNavigatorTab::Assets;
    {
        let mut catalog = app.world_mut().resource_mut::<EditorAssetCatalog>();
        // 连续目录只接受已采用且位于可见窗口的行；仅填entries仍属于尚未显示的目录夹具。
        catalog.requested_revision = 7;
        catalog.applied_revision = 7;
        catalog.visible_count = 1;
        catalog.entries.push(EditorAssetEntry {
            asset_path: "gltf/menu-owner.glb".into(), display_name: "菜单目标".into(),
            is_directory: false, kind: feathrise_scene_content::SceneAssetKind::Scene,
        });
    }
    let mut window = Window { focused: true, ..default() };
    window.set_physical_cursor_position(Some(Vec2::new(100.0, 100.0).as_dvec2()));
    app.world_mut().spawn((window, bevy::window::PrimaryWindow));
    app.world_mut().spawn((
        SceneEditorRoot,
        ComputedNode { size: Vec2::new(800.0, 600.0), inverse_scale_factor: 1.0, ..default() },
        UiGlobalTransform::from_xy(400.0, 300.0),
    ));
    app.world_mut().spawn((
        SceneEditorAssetRow { slot: 0, entry_index: Some(0) }, UiInteraction::Hovered, Node::default(),
        AssetRowBinding { revision: 7, viewport_revision: 0, index: Some(0) },
    ));
    assert!(!app.world().resource::<AssetContextMenuState>().owns_mouse());
    enter(&mut app, node); // 已安装但关闭的外部菜单不能阻止正常 Tab 进入。
    let before = current(&app).clone();

    // 通过正式右键菜单输入冻结真实行，不依赖私有字段或伪造公共 guard。
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
    asset_context_menu::control_menu_for_tests(app.world_mut());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    asset_context_menu::control_menu_for_tests(app.world_mut());
    assert!(app.world().resource::<AssetContextMenuState>().owns_mouse());
    press(&mut app, KeyCode::KeyG);
    fixture::step(&mut app, node, Vec2::ZERO);
    assert!(!app.world().resource::<MeshEditState>().active(), "外部菜单接管时安全退出网格会话");
    assert!(!app.world().resource::<MeshEditState>().moving(), "菜单中的 G 不能开始元素移动");
    assert_eq!(current(&app), &before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);

    fixture::step(&mut app, node, Vec2::ZERO); // 清除本域退出的释放门，单独验证外部菜单门。
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    fixture::step(&mut app, node, Vec2::ZERO);
    assert!(!app.world().resource::<MeshEditState>().active(), "活动菜单中的 Tab 不能进入编辑");
}

#[test]
fn real_animation_timeline_blocks_mesh_shortcuts_on_first_press_and_release() {
    use crate::{
        resources::SceneEditorNumericScrubMouseGuard,
        ui::{
            asset_preview::AnimationEditorState,
            vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
        },
    };

    for already_editing in [false, true] {
        let (mut app, _, node) = app();
        if already_editing {
            enter(&mut app, node);
        }
        assert!(!app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked(),
            "空闲网格会话已走完真实静默帧，不应伪造公共门来制造交互路径");
        let before = current(&app).clone();
        crate::ui::asset_preview::animation::tests::install_timeline_input_for_tests(&mut app);
        assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Scene,
            "右侧动画属性与中央场景页可同时可见");
        app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(node).unwrap().cursor_over = false;

        // 正式动画系统先处理真实轨道按下；同帧Tab不能进入网格，已有会话中的G也不能开始移动。
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        press(&mut app, if already_editing { KeyCode::KeyG } else { KeyCode::Tab });
        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(app.world().resource::<AnimationEditorState>().owns_mouse());
        assert!(!app.world().resource::<MeshEditState>().active());
        assert!(!app.world().resource::<MeshEditState>().moving());
        assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());

        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(app.world().resource::<AnimationEditorState>().owns_mouse(), "释放帧仍属于时间轴");
        assert!(!app.world().resource::<MeshEditState>().active());

        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(!app.world().resource::<AnimationEditorState>().owns_mouse());
        assert!(!app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(app.world().resource::<MeshEditState>().active(), "完整静默帧后可正常重新进入编辑");
        assert_eq!(current(&app), &before);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
}

#[test]
fn real_element_click_and_shift_click_select_shared_vertices_without_object_history() {
    let (mut app, _, node) = app();
    enter(&mut app, node);
    let camera = fixture::camera(&app);
    let vertices = world_vertices(current(&app)).unwrap();
    let visible = (0..8)
        .filter(|&i| visible_point(camera, vertices[i], &vertices))
        .collect::<Vec<_>>();
    assert!(visible.len() >= 2);
    for (ordinal, &index) in visible.iter().take(2).enumerate() {
        if ordinal == 1 {
            press(&mut app, KeyCode::ShiftLeft);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        fixture::step(&mut app, node, camera.project(vertices[index]).unwrap());
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        fixture::step(&mut app, node, camera.project(vertices[index]).unwrap());
    }
    let state = app.world().resource::<MeshEditState>();
    assert_eq!(state.selected, (1 << visible[0]) | (1 << visible[1]));
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn ghost_preview_keeps_original_physics_recipe_and_cancel_restores_visibility() {
    let (mut app, root, node) = app();
    enter(&mut app, node);
    let before = current(&app).clone();
    request(&mut app, MeshEditAction::Move);
    fixture::step(&mut app, node, Vec2::ZERO);
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    let ghost = app.world().resource::<MeshEditState>().ghost.unwrap();
    assert_eq!(
        app.world().get::<Visibility>(root),
        Some(&Visibility::Hidden)
    );
    assert_eq!(
        app.world()
            .get::<feathrise_scene::ScenePlacementInstance>(root)
            .unwrap()
            .asset_target(),
        before.asset_target
    );
    assert!(
        app.world()
            .get::<avian3d::prelude::Collider>(ghost)
            .is_none()
    );
    assert!(
        app.world()
            .get::<feathrise_scene::ScenePlacementInstance>(ghost)
            .is_none()
    );
    let mesh = app.world().get::<Mesh3d>(ghost).unwrap().0.id();
    fixture::step(&mut app, node, Vec2::new(0.04, 0.02));
    assert_eq!(app.world().get::<Mesh3d>(ghost).unwrap().0.id(), mesh);
    request(&mut app, MeshEditAction::Cancel);
    fixture::step(&mut app, node, Vec2::new(0.04, 0.02));
    assert_eq!(
        app.world().get::<Visibility>(root),
        Some(&Visibility::Inherited)
    );
    assert_eq!(
        app.world().get::<Visibility>(ghost),
        Some(&Visibility::Hidden)
    );
    assert_eq!(current(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn configured_save_discards_only_unconfirmed_preview_and_uses_existing_save_request() {
    for rebound in [false, true] {
        let (mut app, _, node) = app();
        if rebound {
            app.world_mut()
                .resource_mut::<feathrise_editor_core::EditorSettings>()
                .editor_keybindings
                .assign(
                    feathrise_editor_core::EditorShortcutAction::Save,
                    Some(feathrise_editor_core::EditorKeyBinding {
                        control: true,
                        ..feathrise_editor_core::EditorKeyBinding::plain(KeyCode::KeyK)
                    }),
                )
                .unwrap();
        }
        enter(&mut app, node);
        let before = current(&app).clone();
        request(&mut app, MeshEditAction::Move);
        fixture::step(&mut app, node, Vec2::ZERO);
        fixture::step(&mut app, node, Vec2::new(0.04, 0.01));
        press(&mut app, KeyCode::ControlLeft);
        press(&mut app, KeyCode::KeyS);
        fixture::step(&mut app, node, Vec2::new(0.04, 0.01));
        if rebound {
            assert!(app.world().resource::<MeshEditState>().moving());
            assert!(!app.world().resource::<SceneEditorState>().save_requested);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::KeyS);
            press(&mut app, KeyCode::KeyK);
            fixture::step(&mut app, node, Vec2::new(0.04, 0.01));
        }
        assert!(!app.world().resource::<MeshEditState>().active());
        assert!(app.world().resource::<SceneEditorState>().save_requested);
        assert_eq!(current(&app), &before);
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
    let (mut app, _, node) = app();
    app.world_mut()
        .resource_mut::<feathrise_editor_core::EditorSettings>()
        .editor_keybindings
        .assign(
            feathrise_editor_core::EditorShortcutAction::Save,
            Some(feathrise_editor_core::EditorKeyBinding::plain(KeyCode::KeyG)),
        )
        .unwrap();
    enter(&mut app, node);
    press(&mut app, KeyCode::KeyG);
    app.update(); // 不清除边沿，验证原保存系统仍能在同帧接收与元素 G 键重叠的合法重绑。
    assert!(app.world().resource::<MeshEditState>().active());
    assert!(!app.world().resource::<MeshEditState>().moving());
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyG)
    );
}

#[test]
fn toolbar_confirm_uses_last_viewport_preview_and_toolbar_move_binds_first_real_pointer() {
    let (mut app, _, node) = app();
    enter(&mut app, node);
    let before = current(&app).clone();
    app.world_mut()
        .resource_mut::<ViewportModeState>()
        .pointer_over_controls = true;
    request(&mut app, MeshEditAction::Move);
    fixture::step(&mut app, node, Vec2::new(-0.4, -0.4));
    assert!(
        app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .start_cursor
            .is_none()
    );
    fixture::step(&mut app, node, Vec2::new(-0.3, -0.3));
    assert_eq!(
        app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .preview,
        before
    );
    app.world_mut()
        .resource_mut::<ViewportModeState>()
        .pointer_over_controls = false;
    fixture::step(&mut app, node, Vec2::ZERO);
    assert_eq!(
        app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .preview,
        before
    );
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    let preview = app
        .world()
        .resource::<MeshEditState>()
        .movement
        .as_ref()
        .unwrap()
        .preview
        .clone();
    assert_ne!(preview, before);
    app.world_mut()
        .resource_mut::<ViewportModeState>()
        .pointer_over_controls = true;
    fixture::step(&mut app, node, Vec2::new(-0.4, -0.4));
    assert_eq!(
        app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .preview,
        preview
    );
    request(&mut app, MeshEditAction::Confirm);
    fixture::step(&mut app, node, Vec2::new(-0.4, -0.4));
    assert_eq!(current(&app), &preview);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn inverted_face_cannot_publish_last_valid_preview_as_the_requested_invalid_result() {
    let (mut app, _, node) = app();
    enter(&mut app, node);
    let before = current(&app).clone();
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Face));
    fixture::step(&mut app, node, Vec2::ZERO);
    app.world_mut().resource_mut::<MeshEditState>().selected = 1 << 1;
    let camera = fixture::camera(&app);
    let origin = selection_center(
        &world_vertices(&before).unwrap(),
        element_vertices(MeshElementKind::Face, 1),
    )
    .unwrap();
    let start = camera.project(origin).unwrap();
    request(&mut app, MeshEditAction::Move);
    fixture::step(&mut app, node, start);
    let invalid = camera.project(origin - Vec3::X * 4.0).unwrap();
    fixture::step(&mut app, node, invalid);
    assert!(
        !app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .valid
    );
    request(&mut app, MeshEditAction::Confirm);
    fixture::step(&mut app, node, invalid);
    assert!(app.world().resource::<MeshEditState>().moving());
    assert_eq!(current(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    request(&mut app, MeshEditAction::Cancel);
    fixture::step(&mut app, node, invalid);
    assert!(!app.world().resource::<MeshEditState>().moving());
}

#[test]
fn zero_motion_confirm_keeps_parametric_record_revision_and_redo_chain() {
    let (mut app, _, node) = app();
    enter(&mut app, node);
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .redo
        .push(EditorCommand::PlacementChanged {
            record_index: 0,
            before: before.clone(),
            after: before.clone(),
        });
    request(&mut app, MeshEditAction::Move);
    fixture::step(&mut app, node, Vec2::ZERO);
    request(&mut app, MeshEditAction::Confirm);
    fixture::step(&mut app, node, Vec2::ZERO);
    assert_eq!(current(&app), &before);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision);
    assert!(session.undo.is_empty());
    assert_eq!(session.redo.len(), 1);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn cancelled_mouse_gesture_owns_final_release_until_one_complete_quiet_frame() {
    let (mut app, _, node) = app();
    enter(&mut app, node);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    fixture::step(&mut app, node, Vec2::new(0.01, 0.01));
    assert_eq!(app.world().resource::<MeshEditState>().selected_count(), 8);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    fixture::step(&mut app, node, Vec2::ZERO);
    request(&mut app, MeshEditAction::Move);
    fixture::step(&mut app, node, Vec2::ZERO);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    assert!(app.world().resource::<MeshEditState>().releasing_mouse());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    assert!(app.world().resource::<MeshEditState>().releasing_mouse());
    fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
    assert!(!app.world().resource::<MeshEditState>().releasing_mouse());
}

#[path = "tool_tests.rs"]
mod tools;

#[path = "workflow_tests.rs"]
mod workflows;

#[path = "interaction_safety_tests.rs"]
mod interaction_safety;

#[path = "text_focus_tests.rs"]
mod text_focus;

#[path = "numeric_release_tests.rs"]
mod numeric_release;

#[path = "face_pick_clip_tests.rs"]
mod face_pick_clip;

#[path = "preview_ownership_tests.rs"]
mod preview_ownership;
