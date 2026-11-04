//! 边界真实输入门、缺页拖动和单墙撤销回归；不安装渲染或访问地图文件。
use super::*;
use crate::authoring::EditorCommand;
use crate::resources::MapEditorNumericScrubMouseGuard;
use feathrise_map_content::{MapBoundariesAsset, MapTerrainKind};
use std::sync::Arc;

fn app(clipmap: bool) -> App {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    if clipmap {
        draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
    }
    let mut doc =
        MapBoundariesAsset::rectangular_default(&draft.map_id, [0.; 2], [10.; 2], -70., 190.)
            .unwrap();
    doc.paths[0].role = MapBoundaryRole::Internal;
    doc.paths[0].bottom_y = -70.;
    doc.paths[0].top_y = 190.; // 独立世界高度故意不同于地表，验证落点不会改写墙高。
    doc.paths[0].closed = false;
    doc.paths[0].points = vec![[2., 2.], [8., 2.]];
    let mut other = doc.paths[0].clone();
    other.id = "other-wall".into();
    doc.paths.push(other);
    let map = draft.map_id.clone();
    let id = doc.paths[0].id.clone();
    draft.boundaries = Some(doc);
    let mut app = App::new();
    let camera = app.world_mut().spawn_empty().id();
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<BoundaryEditState>()
        .init_resource::<VegetationGroundCursor>()
        .init_resource::<DynamicClipmapGroundSurface>()
        .init_resource::<ViewportModeState>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            selected_boundary_path: Some((map.clone(), id)),
            ..default()
        })
        .insert_resource(MapEditorState {
            open: true,
            tool: EditorTool::Boundary,
            selected_map_id: Some(map),
            cursor_world: Some(Vec3::new(4., 400., 3.)),
            ..default()
        })
        .insert_resource(MapEditorViewportState {
            _image: default(),
            camera,
            center: Vec3::ZERO,
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::splat(5.),
            extent: 10.,
            yaw: 0.,
            pitch: 0.,
            framed_map_id: None,
            render_size: UVec2::splat(1000),
        })
        .add_systems(Update, edit_boundary_from_viewport);
    app.world_mut().spawn((
        MapEditorViewport,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app
}

fn path(app: &App) -> MapBoundaryPath {
    app.world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .boundaries
        .as_ref()
        .unwrap()
        .paths[0]
        .clone()
}

fn drag(app: &mut App) -> MapBoundaryPath {
    let before = path(app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let map = session.draft.as_ref().unwrap().map_id.clone();
    let revision = session.revision;
    let terrain_revision = session.terrain_revision;
    *app.world_mut().resource_mut::<BoundaryEditState>() = BoundaryEditState {
        map_id: Some(map),
        path_index: Some(0),
        point_index: Some(0),
        selected_point: Some((before.id.clone(), 0)),
        before: Some(before.clone()),
        expected_revision: revision,
        terrain_revision,
        clipmap_content: Some([1; 7]),
        ..default()
    };
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    before
}

fn set_point(app: &mut App, point: [f32; 2]) {
    app.world_mut()
        .resource_mut::<BoundaryEditState>()
        .preview_point = Some(point);
    app.world_mut().resource_mut::<BoundaryEditState>().moved = true;
}

#[test]
fn clipmap_boundary_click_and_insert_never_use_a_retained_cursor_without_base_pages() {
    for insert in [false, true] {
        let mut app = app(true);
        let before = path(&app);
        if insert {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ControlLeft);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert_eq!(path(&app), before); // 有粗显示点也不能追加、插点或隐式改墙高。
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        assert!(!app.world().resource::<BoundaryEditState>().dragging());
    }
}

#[test]
fn clipmap_boundary_drag_waits_without_reusing_display_and_release_drops_pending_input() {
    let mut app = app(true);
    let before = drag(&mut app);
    app.update();
    assert_eq!(path(&app), before);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert_eq!(path(&app), before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(!app.world().resource::<BoundaryEditState>().dragging());
}

#[test]
fn boundary_drag_commits_last_accepted_point_once_even_when_release_edge_is_lost() {
    let mut app = app(true);
    let before = drag(&mut app);
    set_point(&mut app, [3., 3.]); // 模拟上一帧已经受理的精确样本，当前缺页不能补入粗点。
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    app.update();
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    let EditorCommand::BoundaryPathChanged {
        index,
        before: Some(old),
        after: Some(new),
    } = session.undo[0].clone()
    else {
        panic!("拖动必须是单墙历史");
    };
    assert_eq!(old, before);
    assert_eq!(new.points[0], [3., 3.]);
    assert_eq!((new.bottom_y, new.top_y), (-70., 190.)); // 精确地表Y只供作者光标与辅助显示，不改墙体高度协议。
    let doc = session.draft.as_mut().unwrap().boundaries.as_mut().unwrap();
    super::super::path_history::apply_path_version(doc, index, Some(&new), Some(&old)).unwrap();
    assert_eq!(doc.paths[0], old);
    super::super::path_history::apply_path_version(doc, index, Some(&old), Some(&new)).unwrap();
    assert_eq!(doc.paths[0], new);
    assert_eq!(doc.paths[1].id, "other-wall");
}

#[test]
fn boundary_drag_cancels_and_restores_on_wall_tool_or_author_height_change() {
    for variant in 0..3 {
        let mut app = app(true);
        let before = drag(&mut app);
        set_point(&mut app, [3., 3.]);
        match variant {
            0 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .selected_boundary_path
                    .as_mut()
                    .unwrap()
                    .1 = "other-wall".into()
            }
            1 => app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::Inspect,
            _ => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .terrain_revision += 1
            }
        }
        app.update();
        assert_eq!(path(&app), before);
        assert!(!app.world().resource::<BoundaryEditState>().dragging());
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
}

#[test]
fn boundary_invalid_release_restores_geometry_without_history() {
    let mut app = app(true);
    let before = drag(&mut app);
    set_point(&mut app, [8., 2.]); // 与相邻点重合，正式短段规则必须继续生效。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert_eq!(path(&app), before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .status
            .contains("无效")
    );
}

#[test]
fn boundary_pending_drag_never_mutates_author_document_or_overwrites_concurrent_changes() {
    let mut app = app(true);
    let before = drag(&mut app);
    set_point(&mut app, [3., 3.]);
    assert_eq!(path(&app), before); // 受理拖点也只写输入资源，保存/后台捕获始终看到完整作者版本。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.revision += 1;
        session
            .draft
            .as_mut()
            .unwrap()
            .boundaries
            .as_mut()
            .unwrap()
            .paths[0]
            .label = "外部新名称".into();
    }
    app.update();
    let after = path(&app);
    assert_eq!(after.points, before.points);
    assert_eq!(after.label, "外部新名称");
    assert!(!app.world().resource::<BoundaryEditState>().dragging());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn boundary_overlay_scrub_and_save_do_not_leak_viewport_input() {
    for variant in 0..5 {
        let mut app = app(false); // GLB真实可用点使回归不能仅依赖Clipmap空快照碰巧拒绝。
        let before = path(&app);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        match variant {
            0 => {
                app.world_mut()
                    .resource_mut::<ViewportModeState>()
                    .pointer_over_controls = true
            }
            1 => app
                .world_mut()
                .resource_mut::<MapEditorNumericScrubMouseGuard>()
                .advance(true, true, false),
            2 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .suppress_world_pick_this_frame = true
            }
            3 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .save_requested = true
            }
            _ => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .close_requested = true
            }
        }
        app.update();
        assert_eq!(path(&app), before);
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
}

#[test]
fn glb_boundary_click_and_ctrl_insert_preserve_projected_xz_and_wall_heights() {
    let mut app = app(false);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .cursor_world = Some(Vec3::new(4., 300., 2.2));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    let after = path(&app);
    assert_eq!(after.points, vec![[2., 2.], [4., 2.], [8., 2.]]);
    assert_eq!((after.bottom_y, after.top_y), (-70., 190.));
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn boundary_cursor_freezes_wall_and_point_identity_without_falling_back_to_another_wall() {
    let mut app = app(true);
    let before = drag(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let edit = app.world().resource::<BoundaryEditState>();
    assert_eq!(
        super::super::cursor::clipmap_cursor_target(session, Some(edit), false),
        Some(GroundCursorTarget::BoundaryDrag {
            path_id: before.id,
            point_index: 0
        })
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .selected_boundary_path
        .as_mut()
        .unwrap()
        .1 = "other-wall".into();
    assert_eq!(
        super::super::cursor::clipmap_cursor_target(
            app.world().resource::<EditorAuthoringSession>(),
            Some(app.world().resource::<BoundaryEditState>()),
            false
        ),
        None
    );
}
