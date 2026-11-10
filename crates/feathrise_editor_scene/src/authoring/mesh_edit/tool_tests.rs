//! 实际元素输入系统的工具切换、手柄拖动与旋转事务；始终复用正式投影和几何配方。

use super::*;
use crate::authoring::mesh_edit::transform_math::{move_arrows, rotation_rings};
use bevy::ui::RelativeCursorPosition;

fn orthographic_app() -> (App, Entity, Entity) {
    let (mut app, root, node) = app();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .orthographic = true;
    (app, root, node)
}

fn choose_tool(app: &mut App, node: Entity, tool: MeshEditTool) {
    request(app, MeshEditAction::Tool(tool));
    fixture::step(app, node, Vec2::ZERO);
    fixture::step(app, node, Vec2::ZERO); // 工具按钮完成点击后经过完整静默帧，才允许抓取手柄。
}

fn mouse(app: &mut App, pressed: bool) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    if pressed {
        input.press(MouseButton::Left);
    } else {
        input.release(MouseButton::Left);
    }
}

fn x_handle(app: &App, origin: Vec3) -> (Vec2, Vec3) {
    let camera = fixture::camera(app);
    let arrow = move_arrows(camera, origin)
        .into_iter()
        .flatten()
        .find(|arrow| arrow.constraint == MoveConstraint::X)
        .expect("正面视角应提供真实 X 箭头");
    (
        arrow.screen[0].lerp(arrow.screen[1], 0.60) / (camera.size * camera.inverse_scale),
        arrow.world[0].lerp(arrow.world[1], 0.60),
    )
}

fn z_ring_points(app: &App, origin: Vec3) -> (Vec2, Vec2) {
    let camera = fixture::camera(app);
    let ring = rotation_rings(camera, origin)
        .into_iter()
        .flatten()
        .find(|ring| ring.constraint == MoveConstraint::Z)
        .expect("正面视角应提供真实 Z 旋转圆环");
    let logical = camera.size * camera.inverse_scale;
    // 相邻两段只转5.625度，足以改变顶点又不让原闭壳跨越内部中心。
    (ring.screen[8] / logical, ring.screen[9] / logical)
}

#[test]
fn tool_choice_preserves_vertex_mode_selection_and_author_record_without_starting_gesture() {
    let (mut app, _, node) = orthographic_app();
    enter(&mut app, node);
    let before = current(&app).clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut().resource_mut::<MeshEditState>().selected = 0b00001100;
    for tool in [
        MeshEditTool::Move,
        MeshEditTool::Rotate,
        MeshEditTool::Select,
    ] {
        choose_tool(&mut app, node, tool);
        let state = app.world().resource::<MeshEditState>();
        assert!(state.active());
        assert!(!state.moving());
        assert_eq!(state.kind, MeshElementKind::Vertex);
        assert_eq!(state.tool, tool);
        assert_eq!(state.selected, 0b00001100);
        assert_eq!(current(&app), &before);
    }
    app.world_mut().resource_mut::<MeshEditState>().selected = 0;
    request(&mut app, MeshEditAction::Move);
    fixture::step(&mut app, node, Vec2::ZERO);
    assert!(!app.world().resource::<MeshEditState>().moving());
    assert_eq!(current(&app), &before);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision);
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn rebound_idle_history_keys_preserve_element_selection_and_global_dispatch_edge() {
    for (action, key) in [
        (feathrise_editor_core::EditorShortcutAction::Undo, KeyCode::KeyR),
        (feathrise_editor_core::EditorShortcutAction::Redo, KeyCode::KeyG),
    ] {
        let (mut app, _, node) = orthographic_app();
        {
            let mut settings = app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>();
            // R 默认属于对象缩放；先正常解绑，确保使用设置系统实际接受的合法重绑。
            settings
                .editor_keybindings
                .assign(feathrise_editor_core::EditorShortcutAction::ScaleSelected, None)
                .unwrap();
            settings
                .editor_keybindings
                .assign(action, Some(feathrise_editor_core::EditorKeyBinding::plain(key)))
                .unwrap();
        }
        enter(&mut app, node);
        app.world_mut().resource_mut::<MeshEditState>().selected = 0b00001100;
        let before = current(&app).clone();
        let revision = app.world().resource::<EditorAuthoringSession>().revision;
        press(&mut app, key);
        app.update(); // 保留边沿，检查同帧原历史分派仍能接收它。
        let state = app.world().resource::<MeshEditState>();
        assert!(state.active());
        assert!(!state.moving());
        assert_eq!(state.kind, MeshElementKind::Vertex);
        assert_eq!(state.tool, MeshEditTool::Select);
        assert_eq!(state.selected, 0b00001100);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(key)
        );
        assert_eq!(current(&app), &before);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, revision);
        assert!(session.undo.is_empty());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
}

#[test]
fn move_tool_drags_only_selected_vertex_and_release_commits_one_complete_history() {
    let (mut app, _, node) = orthographic_app();
    enter(&mut app, node);
    app.world_mut().resource_mut::<MeshEditState>().selected = 1 << 7;
    choose_tool(&mut app, node, MeshEditTool::Move);
    let before = current(&app).clone();
    let points = world_vertices(&before).unwrap();
    let (start, start_world) = x_handle(&app, points[7]);
    fixture::step(&mut app, node, start);
    assert_eq!(
        app.world().resource::<MeshEditState>().handle_hover,
        Some(MoveConstraint::X)
    );
    mouse(&mut app, true);
    fixture::step(&mut app, node, start);
    {
        let movement = app
            .world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap();
        assert!(movement.dragging);
        assert_eq!(movement.tool, MeshEditTool::Move);
        assert_eq!(movement.selected_vertices, 1 << 7);
        assert_eq!(movement.constraint, MoveConstraint::X);
    }
    let end = fixture::camera(&app)
        .project(start_world + Vec3::X * 0.125)
        .unwrap();
    fixture::step(&mut app, node, end);
    assert_eq!(current(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    let preview = app
        .world()
        .resource::<MeshEditState>()
        .movement
        .as_ref()
        .unwrap()
        .preview
        .clone();
    let moved = world_vertices(&preview).unwrap();
    assert!((moved[7] - points[7] - Vec3::X * 0.125).length() < 0.001);
    for index in 0..7 {
        assert_eq!(moved[index], points[index]);
    }
    mouse(&mut app, false);
    fixture::step(&mut app, node, end);
    assert!(!app.world().resource::<MeshEditState>().moving());
    assert_eq!(
        app.world().resource::<MeshEditState>().kind,
        MeshElementKind::Vertex
    );
    assert_eq!(current(&app), &preview);
    assert_eq!(current(&app).translation, before.translation);
    assert_eq!(current(&app).yaw_degrees, before.yaw_degrees);
    assert_eq!(current(&app).scale, before.scale);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(
        matches!(&session.undo[0], EditorCommand::PlacementChanged {before:old,after:new,..}
        if old == &before && new == &preview)
    );
}

#[test]
fn modal_r_rotates_four_top_vertices_about_mean_and_cancel_discards_later_preview() {
    let (mut app, _, node) = orthographic_app();
    enter(&mut app, node);
    let mask = 0b11001100;
    app.world_mut().resource_mut::<MeshEditState>().selected = mask as u128;
    let before = current(&app).clone();
    let points = world_vertices(&before).unwrap();
    let pivot = selection_center(&points, mask).unwrap();
    let (start, end) = z_ring_points(&app, pivot);
    press(&mut app, KeyCode::KeyR);
    fixture::step(&mut app, node, start);
    assert_eq!(
        app.world().resource::<MeshEditState>().tool,
        MeshEditTool::Rotate
    );
    assert!(
        !app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .dragging
    );
    press(&mut app, KeyCode::KeyZ);
    fixture::step(&mut app, node, end);
    let preview = app
        .world()
        .resource::<MeshEditState>()
        .movement
        .as_ref()
        .unwrap()
        .preview
        .clone();
    let rotated = world_vertices(&preview).unwrap();
    assert_ne!(preview.asset_target, before.asset_target);
    assert!((selection_center(&rotated, mask).unwrap() - pivot).length() < 0.001);
    for index in [0, 1, 4, 5] {
        assert_eq!(rotated[index], points[index]);
    }
    assert!((rotated[7] - points[7]).length() > 0.01);
    assert_eq!(current(&app), &before);
    press(&mut app, KeyCode::Enter);
    fixture::step(&mut app, node, end);
    assert_eq!(current(&app), &preview);
    assert_eq!(current(&app).translation, before.translation);
    assert_eq!(current(&app).yaw_degrees, before.yaw_degrees);
    assert_eq!(current(&app).scale, before.scale);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );

    let pivot = selection_center(&world_vertices(current(&app)).unwrap(), mask).unwrap();
    let (start, end) = z_ring_points(&app, pivot);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyZ);
    request(&mut app, MeshEditAction::Rotate);
    fixture::step(&mut app, node, start);
    press(&mut app, KeyCode::KeyZ);
    fixture::step(&mut app, node, end);
    assert_ne!(
        app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .preview,
        preview
    );
    request(&mut app, MeshEditAction::Cancel);
    fixture::step(&mut app, node, end);
    assert!(!app.world().resource::<MeshEditState>().moving());
    assert_eq!(current(&app), &preview);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn modal_r_from_selection_center_waits_for_valid_pointer_then_commits_one_rotation() {
    let (mut app, _, node) = orthographic_app();
    enter(&mut app, node);
    let mask = 0b11001100;
    app.world_mut().resource_mut::<MeshEditState>().selected = mask as u128;
    let before = current(&app).clone();
    let points = world_vertices(&before).unwrap();
    let pivot = selection_center(&points, mask).unwrap();
    let center = fixture::camera(&app).project(pivot).unwrap();
    let (start, end) = z_ring_points(&app, pivot);
    press(&mut app, KeyCode::KeyR);
    fixture::step(&mut app, node, center);
    fixture::step(&mut app, node, center); // 轴心没有有效旋转方向，继续等待而非永久冻结此起点。
    {
        let movement = app
            .world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap();
        assert!(!movement.dragging);
        assert!(!movement.valid);
        assert!(movement.start_cursor.is_none());
    }
    press(&mut app, KeyCode::KeyZ);
    fixture::step(&mut app, node, start);
    {
        let movement = app
            .world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap();
        assert!(movement.valid);
        assert_eq!(movement.start_cursor, Some(start));
        assert_eq!(movement.preview, before); // 第一个有效方向只建立基准，模型不能跳转。
    }
    fixture::step(&mut app, node, end);
    let preview = app
        .world()
        .resource::<MeshEditState>()
        .movement
        .as_ref()
        .unwrap()
        .preview
        .clone();
    let rotated = world_vertices(&preview).unwrap();
    assert_ne!(preview.asset_target, before.asset_target);
    assert!((selection_center(&rotated, mask).unwrap() - pivot).length() < 0.001);
    for index in [0, 1, 4, 5] {
        assert_eq!(rotated[index], points[index]);
    }
    assert_eq!(current(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    press(&mut app, KeyCode::Enter);
    fixture::step(&mut app, node, end);
    let state = app.world().resource::<MeshEditState>();
    assert!(!state.moving());
    assert_eq!(state.kind, MeshElementKind::Vertex);
    assert_eq!(state.selected, mask as u128);
    assert_eq!(current(&app), &preview);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(
        matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. }
            if old == &before && new == &preview)
    );
}

#[test]
fn rotating_one_vertex_about_itself_keeps_parameter_recipe_revision_and_redo() {
    let (mut app, _, node) = orthographic_app();
    enter(&mut app, node);
    app.world_mut().resource_mut::<MeshEditState>().selected = 1 << 7;
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
    let pivot = world_vertices(&before).unwrap()[7];
    let (start, end) = z_ring_points(&app, pivot);
    request(&mut app, MeshEditAction::Rotate);
    fixture::step(&mut app, node, start);
    press(&mut app, KeyCode::KeyZ);
    fixture::step(&mut app, node, end);
    assert!(
        app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .valid
    );
    assert_eq!(
        app.world()
            .resource::<MeshEditState>()
            .movement
            .as_ref()
            .unwrap()
            .preview,
        before
    );
    request(&mut app, MeshEditAction::Confirm);
    fixture::step(&mut app, node, end);
    assert_eq!(current(&app), &before);
    assert!(matches!(
        current(&app).asset_target,
        ScenePlacementAssetTarget::Cuboid { .. }
    ));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision);
    assert!(session.undo.is_empty());
    assert_eq!(session.redo.len(), 1);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn invalid_or_outside_handle_release_restores_authority_without_partial_history() {
    for outside in [false, true] {
        let (mut app, root, node) = orthographic_app();
        enter(&mut app, node);
        app.world_mut().resource_mut::<MeshEditState>().selected = 1 << 7;
        choose_tool(&mut app, node, MeshEditTool::Move);
        let before = current(&app).clone();
        let pivot = world_vertices(&before).unwrap()[7];
        let (start, start_world) = x_handle(&app, pivot);
        mouse(&mut app, true);
        fixture::step(&mut app, node, start);
        let end = fixture::camera(&app)
            .project(start_world + Vec3::X * if outside { 0.125 } else { -2.0 })
            .unwrap();
        fixture::step(&mut app, node, end);
        assert_eq!(
            app.world()
                .resource::<MeshEditState>()
                .movement
                .as_ref()
                .unwrap()
                .valid,
            outside
        );
        if outside {
            app.world_mut()
                .entity_mut(node)
                .get_mut::<RelativeCursorPosition>()
                .unwrap()
                .cursor_over = false;
        }
        mouse(&mut app, false);
        fixture::step(
            &mut app,
            node,
            if outside { Vec2::splat(0.75) } else { end },
        );
        assert!(!app.world().resource::<MeshEditState>().moving());
        assert_eq!(current(&app), &before);
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Inherited)
        );
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
}
