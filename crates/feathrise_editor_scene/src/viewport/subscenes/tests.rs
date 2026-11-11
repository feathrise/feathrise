//! 子场景手柄悬停沿原输入和共享语义显示；纯悬停不得打开作者事务或改变文档。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use crate::viewport::placement_move::{MoveAxis, interaction_tests::{camera, step, batch_top_camera}};

fn fixture() -> (App, Entity, Vec3) {
    let (mut app, _, node) = crate::viewport::placement_move::interaction_tests::app();
    let origin = Vec3::new(5.0, 4.0, 5.0);
    let id = SceneInstanceId::parse("hover-submap").unwrap();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().manifest.subscenes = Arc::from([SceneInstancePlacement {
            id: id.clone(), source_scene_id: SceneId::parse("hover-source").unwrap(), label: "子场景".into(),
            transform: SceneInstanceTransform { translation: origin.to_array(), ..default() },
        }]);
    }
    app.init_resource::<SubsceneTransformState>()
        .init_resource::<SubsceneEditorState>()
        .init_resource::<AuthorToolFrame>()
        .init_resource::<Assets<feathrise_scene_content::SceneManifestAsset>>();
    app.world_mut().resource_mut::<SubsceneEditorState>().selected = Some(id.clone());
    app.world_mut().resource_mut::<LayerCandidateState>().selected = Some(LayerTarget::Subscene(id.as_str().into()));
    app.edit_schedule(Update, |schedule| { *schedule = Schedule::new(Update); schedule.add_systems(input); });
    (app, node, origin)
}

fn move_feedback(app: &mut App) -> (u8, bool) {
    *app.world_mut().resource_mut::<AuthorToolFrame>() = default();
    app.world_mut().run_system_once(draw).unwrap();
    let frame = app.world().resource::<AuthorToolFrame>();
    frame.preview.items.iter().find_map(|item| match item {
        AuthorTool::Transform(TransformPreview::Move { axes, active_axes, plane_active, color_active, .. }) if *axes != 0 => {
            assert_eq!(*color_active, Color::srgb(0.92, 0.72, 0.40).to_linear().to_f32_array());
            Some((*active_axes, *plane_active))
        }
        _ => None,
    }).expect("当前选择必须投影子场景移动手柄")
}

#[test]
fn subscene_hover_projects_axis_and_plane_without_history_and_clears_for_ui() {
    let (mut app, node, origin) = fixture();
    let viewport = camera(&app);
    let cursor = viewport.project(origin + Vec3::X * viewport.handle_length(origin).unwrap() * 0.65).unwrap();
    step(&mut app, node, cursor);
    assert_eq!(app.world().resource::<SubsceneTransformState>().hover, Some(MoveHandle::Axis(MoveAxis::X)));
    assert_eq!(move_feedback(&mut app), (0b001, false));
    assert!(!app.world().resource::<SubsceneTransformState>().owns_mouse());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);

    batch_top_camera(&mut app);
    let viewport = camera(&app);
    let length = viewport.handle_length(origin).unwrap();
    let middle = (feathrise_author_tools::AUTHOR_PLANE_INNER_FRACTION
        + feathrise_author_tools::AUTHOR_PLANE_OUTER_FRACTION) * 0.5;
    let plane = viewport.project(origin + Vec3::new(length * middle, 0.0, length * middle)).unwrap();
    step(&mut app, node, plane);
    assert_eq!(app.world().resource::<SubsceneTransformState>().hover, Some(MoveHandle::XzPlane));
    assert_eq!(move_feedback(&mut app), (0, true));
    app.world_mut().resource_mut::<LayerCandidateState>().pointer_over_ui = true;
    step(&mut app, node, plane);
    assert_eq!(app.world().resource::<SubsceneTransformState>().hover, None);
    assert_eq!(move_feedback(&mut app), (0, false));
    app.world_mut().resource_mut::<LayerCandidateState>().pointer_over_ui = false;
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, plane);
    assert!(app.world().resource::<SubsceneTransformState>().owns_mouse());
    step(&mut app, node, Vec2::ZERO);
    assert_eq!(app.world().resource::<SubsceneTransformState>().hover, None);
    assert_eq!(move_feedback(&mut app), (0, true), "抓取后离开方框仍由当前手势决定激活显示");
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}
