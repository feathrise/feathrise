//! 建模整合的实际输入回归：布局身份、框选确认、真实面标记及工具栏即时拓扑候选。

use super::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_scene_content::{EditableMeshGeometry, MeshFace};

#[test]
fn malformed_mesh_never_enters_the_dynamic_selection_or_render_index_path() {
    let mut before = record();
    let mut geometry = before.asset_target.editable_geometry().unwrap();
    geometry.faces[0].vertices[0] = 63;
    before.asset_target = ScenePlacementAssetTarget::EditableMesh {
        geometry,
        color_srgb: [31, 41, 59],
        roughness_milli: 700,
        collision: true,
    };
    assert!(world_mesh(&before).is_none(), "坏索引不能进入绘制或框选");
    let (mut app, _, node) = app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements[0] = before.clone();
    press(&mut app, KeyCode::Tab);
    fixture::step(&mut app, node, Vec2::ZERO);
    assert!(!app.world().resource::<MeshEditState>().active());
    assert_eq!(current(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("无法进入网格编辑")
    );
}

fn ortho_app() -> (App, Entity, Entity) {
    let (mut app, root, node) = app();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .orthographic = true;
    (app, root, node)
}

fn mouse(app: &mut App, down: bool) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    if down {
        input.press(MouseButton::Left);
    } else {
        input.release(MouseButton::Left);
    }
}

#[test]
fn box_selection_rejects_layout_resize_even_before_render_camera_changes() {
    let (mut app, _, node) = ortho_app();
    enter(&mut app, node);
    app.world_mut().resource_mut::<MeshEditState>().selected = 1 << 7;
    let before = current(&app).clone();
    press(&mut app, KeyCode::KeyB);
    fixture::step(&mut app, node, Vec2::splat(-0.3));
    mouse(&mut app, true);
    fixture::step(&mut app, node, Vec2::splat(-0.3));
    fixture::step(&mut app, node, Vec2::splat(0.3));
    let camera = app
        .world()
        .resource::<MeshEditState>()
        .box_selection
        .as_ref()
        .unwrap()
        .camera;
    app.world_mut()
        .entity_mut(node)
        .get_mut::<ComputedNode>()
        .unwrap()
        .size += Vec2::X * 100.0;
    assert_eq!(
        fixture::camera(&app),
        camera,
        "渲染尺寸未更新时仍须检测节点布局变化"
    );
    mouse(&mut app, false);
    fixture::step(&mut app, node, Vec2::splat(0.3));
    let state = app.world().resource::<MeshEditState>();
    assert!(!state.boxing());
    assert!(state.releasing_mouse());
    assert_eq!(state.selected, 1 << 7);
    assert_eq!(current(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn boxing_confirm_uses_last_rectangle_and_without_rectangle_exits_without_changes() {
    for has_rectangle in [false, true] {
        let (mut app, _, node) = ortho_app();
        enter(&mut app, node);
        app.world_mut().resource_mut::<MeshEditState>().selected = 1 << 7;
        let before = current(&app).clone();
        press(&mut app, KeyCode::KeyB);
        fixture::step(&mut app, node, Vec2::splat(-0.3));
        if has_rectangle {
            mouse(&mut app, true);
            fixture::step(&mut app, node, Vec2::splat(-0.3));
            fixture::step(&mut app, node, Vec2::splat(0.3));
        }
        app.world_mut()
            .entity_mut(node)
            .get_mut::<RelativeCursorPosition>()
            .unwrap()
            .cursor_over = false;
        if has_rectangle {
            request(&mut app, MeshEditAction::Confirm);
        } else {
            press(&mut app, KeyCode::Enter);
        }
        fixture::step(&mut app, node, Vec2::splat(0.45));
        let state = app.world().resource::<MeshEditState>();
        assert!(!state.boxing(), "确认入口必须结束当前框选");
        assert!(state.releasing_mouse());
        if has_rectangle {
            let mesh = world_mesh(&before).unwrap();
            assert_eq!(
                state.selected,
                mesh.in_box(
                    MeshElementKind::Vertex,
                    fixture::camera(&app),
                    Vec2::splat(-0.3),
                    Vec2::splat(0.3)
                )
            );
            assert!(state.selected_count() > 1);
        } else {
            assert_eq!(state.selected, 1 << 7);
        }
        assert_eq!(current(&app), &before);
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        mouse(&mut app, false);
        fixture::step(&mut app, node, Vec2::splat(0.45));
        assert!(!app.world().resource::<MeshEditState>().boxing());
    }
}

fn concave_prism() -> EditableMeshGeometry {
    let boundary = [
        [-2000, -2000],
        [-2000, 2000],
        [2000, 2000],
        [2000, 0],
        [0, 0],
        [0, -2000],
    ];
    let mut vertices = Vec::new();
    for y in [-2000, 2000] {
        vertices.extend(boundary.iter().map(|&[x, z]| [x, y, z]));
    }
    let mut faces = vec![
        MeshFace::new(&[5, 4, 3, 2, 1, 0]).unwrap(),
        MeshFace::new(&[6, 7, 8, 9, 10, 11]).unwrap(),
    ];
    for side in 0..6 {
        let next = (side + 1) % 6;
        faces.push(MeshFace::new(&[side, next, next + 6, side + 6]).unwrap());
    }
    EditableMeshGeometry::from_parts(&vertices, &faces).unwrap()
}

#[test]
fn face_markers_and_small_box_selection_share_real_visible_surface_points() {
    let (app, _, _) = ortho_app();
    let camera = fixture::camera(&app);
    let mut geometries = vec![record().asset_target.editable_geometry().unwrap()];
    let mut warped = geometries[0];
    warped.vertices_half_mm[7][1] += 600;
    geometries.extend([warped, concave_prism()]);
    for geometry in geometries {
        let mut record = record();
        record.asset_target = ScenePlacementAssetTarget::EditableMesh {
            geometry,
            color_srgb: [31, 41, 59],
            roughness_milli: 700,
            collision: true,
        };
        record.asset_target.validate().unwrap();
        let mesh = world_mesh(&record).unwrap();
        let mut visible = 0;
        for face in 0..mesh.faces.len() {
            let Some(marker) = mesh.visible_representative(MeshElementKind::Face, face, camera)
            else {
                continue;
            };
            visible += 1;
            let screen = camera.project(marker).unwrap();
            assert_eq!(
                mesh.nearest_face(camera.ray(screen).unwrap()).unwrap().0,
                face,
                "标记应位于该面真实可见三角内"
            );
            let chosen = mesh.in_box(
                MeshElementKind::Face,
                camera,
                screen - Vec2::splat(0.0001),
                screen + Vec2::splat(0.0001),
            );
            assert_ne!(chosen & (1 << face), 0, "围住实际标记的小框必须选中同一面");
        }
        assert!(visible > 0);
    }
}

#[test]
fn topology_preview_draws_candidate_contour_without_reinterpreting_old_edge_indices() {
    let (mut app, _, node) = ortho_app();
    enter(&mut app, node);
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Edge));
    fixture::step(&mut app, node, Vec2::ZERO);
    app.world_mut().resource_mut::<MeshEditState>().selected = 1;
    request(&mut app, MeshEditAction::LoopCut);
    fixture::step(&mut app, node, Vec2::ZERO);
    fixture::step(&mut app, node, Vec2::ZERO);
    let state = app.world().resource::<MeshEditState>();
    let movement = state.movement.as_ref().unwrap();
    assert!(movement.valid);
    let mesh = world_mesh(&movement.preview).unwrap();
    assert_eq!(mesh.faces.len(), 10);
    assert_eq!(
        super::super::render::display_selection(state, &mesh),
        (MeshElementKind::Face, (1 << 10) - 1, None)
    );
    assert_eq!(
        (state.kind, state.selected),
        (MeshElementKind::Edge, 1),
        "预览绘制不能提前改写作者选择"
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn toolbar_subdivide_previews_immediately_and_confirms_without_entering_viewport() {
    let (mut app, _, node) = ortho_app();
    enter(&mut app, node);
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Face));
    fixture::step(&mut app, node, Vec2::ZERO);
    let before = current(&app).clone();
    app.world_mut()
        .entity_mut(node)
        .get_mut::<RelativeCursorPosition>()
        .unwrap()
        .cursor_over = false;
    request(&mut app, MeshEditAction::Subdivide);
    fixture::step(&mut app, node, Vec2::ZERO);
    let state = app.world().resource::<MeshEditState>();
    let movement = state.movement.as_ref().unwrap();
    assert!(movement.valid, "无参数细分在按钮请求当帧就应有可确认候选");
    let candidate = movement.preview.clone();
    assert_eq!(
        candidate
            .asset_target
            .editable_geometry()
            .unwrap()
            .face_count,
        24
    );
    assert_eq!(current(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert_eq!(
        *app.world().get::<Visibility>(state.ghost.unwrap()).unwrap(),
        Visibility::Inherited
    );
    request(&mut app, MeshEditAction::Confirm);
    fixture::step(&mut app, node, Vec2::ZERO);
    assert_eq!(current(&app), &candidate);
    assert!(!app.world().resource::<MeshEditState>().moving());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(
        matches!(&session.undo[0], EditorCommand::PlacementChanged {before: old, after: new, ..} if old == &before && new == &candidate)
    );
}

#[test]
fn extrusion_new_element_selection_move_and_box_continue_one_authoring_session() {
    let (mut app, root, node) = ortho_app();
    let mut before = current(&app).clone();
    let metadata = std::sync::Arc::make_mut(&mut before.editor);
    metadata.name = "连续建模灰模".into();
    metadata.group = "建筑草模".into();
    before.asset_target = ScenePlacementAssetTarget::Cuboid {
        size_mm: [2000; 3],
        color_srgb: [23, 47, 89],
        roughness_milli: 421,
        collision: false,
    };
    let map = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placements.placements[0] = before.clone();
        draft.scene_id.clone()
    };
    app.world_mut()
        .entity_mut(root)
        .insert(feathrise_scene::ScenePlacementInstance::new(&map, &before));
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    enter(&mut app, node);
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Face));
    fixture::step(&mut app, node, Vec2::ZERO);
    app.world_mut().resource_mut::<MeshEditState>().selected = 1 << 1;
    let camera = fixture::camera(&app);
    let mesh = world_mesh(&before).unwrap();
    let cap_center =
        selection_center(&mesh.vertices, mesh.element_mask(MeshElementKind::Face, 1)).unwrap();
    let start = camera.project(cap_center).unwrap();
    let end = camera.project(cap_center + Vec3::X * 0.4).unwrap();
    press(&mut app, KeyCode::KeyE);
    fixture::step(&mut app, node, start);
    fixture::step(&mut app, node, end);
    assert_eq!(current(&app), &before);
    let extruded = app
        .world()
        .resource::<MeshEditState>()
        .movement
        .as_ref()
        .unwrap()
        .preview
        .clone();
    request(&mut app, MeshEditAction::Confirm);
    fixture::step(&mut app, node, end);
    assert_eq!(current(&app), &extruded);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    let mesh = world_mesh(&extruded).unwrap();
    assert_eq!(
        (mesh.vertices.len(), mesh.edges.len(), mesh.faces.len()),
        (12, 20, 10)
    );

    // 正式元素类型请求在新增拓扑上换算选择，不依赖旧8点/12边/6面编号。
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Vertex));
    fixture::step(&mut app, node, end);
    let cap_mask = mesh.element_mask(MeshElementKind::Face, 1);
    assert_eq!(cap_mask, 0b1111 << 8);
    assert_eq!(
        app.world().resource::<MeshEditState>().selected,
        u128::from(cap_mask)
    );
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Edge));
    fixture::step(&mut app, node, end);
    let edges = app.world().resource::<MeshEditState>().selected;
    assert_eq!(edges.count_ones(), 4);
    assert_ne!(edges >> 12, 0, "新增顶帽边必须在动态真实边表中可选");
    assert_eq!(
        mesh.selected_vertices(MeshElementKind::Edge, edges),
        cap_mask
    );
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Face));
    fixture::step(&mut app, node, end);
    assert_eq!(app.world().resource::<MeshEditState>().selected, 1 << 1);
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Vertex));
    fixture::step(&mut app, node, end);

    let origin = selection_center(&mesh.vertices, cap_mask).unwrap();
    let start = camera.project(origin).unwrap();
    let end = camera.project(origin + Vec3::X * 0.25).unwrap();
    press(&mut app, KeyCode::KeyG);
    fixture::step(&mut app, node, start);
    press(&mut app, KeyCode::KeyX);
    fixture::step(&mut app, node, end);
    let movement = app
        .world()
        .resource::<MeshEditState>()
        .movement
        .as_ref()
        .unwrap();
    assert!(movement.valid);
    let moved = movement.preview.clone();
    assert_eq!(current(&app), &extruded);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    let new_points = world_vertices(&moved).unwrap();
    for (index, point) in new_points.iter().enumerate() {
        let expected = mesh.vertices[index]
            + if cap_mask & (1 << index) != 0 {
                Vec3::X * 0.25
            } else {
                Vec3::ZERO
            };
        assert!(point.distance(expected) < 0.001);
    }
    request(&mut app, MeshEditAction::Confirm);
    fixture::step(&mut app, node, end);
    assert_eq!(current(&app), &moved);

    let mesh = world_mesh(&moved).unwrap();
    let projected = mesh
        .vertices
        .iter()
        .map(|&point| camera.project(point).unwrap())
        .collect::<Vec<_>>();
    let start = projected
        .iter()
        .copied()
        .fold(Vec2::splat(f32::INFINITY), Vec2::min)
        - Vec2::splat(0.01);
    let end = projected
        .iter()
        .copied()
        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max)
        + Vec2::splat(0.01);
    press(&mut app, KeyCode::KeyB);
    fixture::step(&mut app, node, start);
    mouse(&mut app, true);
    fixture::step(&mut app, node, start);
    fixture::step(&mut app, node, end);
    mouse(&mut app, false);
    fixture::step(&mut app, node, end);
    let expected = mesh
        .vertices
        .iter()
        .enumerate()
        .filter(|(_, point)| mesh.visible(camera, **point))
        .fold(0u128, |mask, (index, _)| mask | (1 << index));
    let state = app.world().resource::<MeshEditState>();
    assert!(state.active() && !state.moving());
    assert_eq!(state.selected, expected);
    assert_ne!(expected >> 8, 0, "连续框选必须包含新建并可见的共享点");
    assert_eq!(current(&app), &moved);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision + 2);
    assert_eq!(session.undo.len(), 2, "只有挤出和移动各贡献一条完整历史");
    for (command, old, new) in [
        (&session.undo[0], &before, &extruded),
        (&session.undo[1], &extruded, &moved),
    ] {
        assert!(
            matches!(command, EditorCommand::PlacementChanged { before, after, .. } if before == old && after == new)
        );
        let mut original_metadata = new.clone();
        original_metadata.asset_target = old.asset_target;
        assert_eq!(&original_metadata, old);
    }
    assert!(matches!(
        moved.asset_target,
        ScenePlacementAssetTarget::EditableMesh {
            color_srgb: [23, 47, 89],
            roughness_milli: 421,
            collision: false,
            ..
        }
    ));
    let decoded: ScenePlacementRecord =
        serde_json::from_str(&serde_json::to_string(&moved).unwrap()).unwrap();
    assert_eq!(decoded, moved);
    decoded.asset_target.validate().unwrap();
    assert_eq!(
        app.world()
            .get::<feathrise_scene::ScenePlacementInstance>(root)
            .unwrap()
            .asset_target(),
        moved.asset_target
    );
}
