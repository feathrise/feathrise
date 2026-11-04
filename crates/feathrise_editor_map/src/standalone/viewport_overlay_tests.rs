//! 非模态浮层只按实际布局留孔；通过正式投影核对同一游戏表面的其余区域保持可见。

use super::*;

#[test]
fn tooltip_only_reserves_its_own_pixels_without_hiding_the_child_surface() {
    let mut app = App::new();
    app.insert_resource(MapEditorState { open: true, ..default() });
    install(&mut app);
    app.world_mut().spawn((MapEditorViewport, Node::default(),
        ComputedNode { size: Vec2::new(800., 600.), ..default() },
        UiGlobalTransform::from_xy(500., 350.), InheritedVisibility::VISIBLE));
    let tooltip = app.world_mut().spawn((crate::ui::tooltips::TooltipPopup,
        Node { display: Display::None, ..default() },
        ComputedNode { size: Vec2::new(280., 80.), ..default() },
        UiGlobalTransform::from_xy(300., 120.), InheritedVisibility::VISIBLE)).id();
    app.update();
    let layout = app.world().resource::<ViewportHostProjection>().layout;
    for shown in [true, false, true] {
        app.world_mut().get_mut::<Node>(tooltip).unwrap().display = if shown { Display::Flex } else { Display::None };
        // 单帧拾取门属于原输入流程，不能成为切换到父编辑器World的视觉开关。
        app.world_mut().resource_mut::<MapEditorState>().suppress_world_pick_this_frame = true;
        app.update();
        let projection = app.world().resource::<ViewportHostProjection>();
        assert!(!projection.obscured && projection.error.is_none(), "提示显隐不应隐藏整个游戏child");
        assert_eq!(projection.layout, layout, "不缩小中央视口或改变相机投影");
        assert_eq!(projection.excluded_rects, if shown { vec![Rect::new(160., 80., 440., 160.)] } else { vec![] });
        assert!(!projection.excluded_rects.iter().any(|rect| rect.contains(Vec2::new(500., 400.))),
            "提示外的天空和场景仍来自原游戏画面");
        assert!(app.world().resource::<MapEditorState>().suppress_world_pick_this_frame,
            "视觉投影不得解除原输入门");
    }
}
