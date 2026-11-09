//! 着色选项的完成点击、相机隔离和隐藏事件回归。
use super::*;
use crate::{
    components::MapEditorCamera,
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState},
};
use bevy::prelude::*;
use components::*;
use resources::ShadingState;
use feathrise_editor_ui::EditorButtonClick;
use types::*;

#[test]
fn completed_shading_clicks_only_update_the_editor_camera_and_hidden_events_do_not_replay() {
    let mut app = App::new();
    app.init_resource::<ShadingState>()
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .add_systems(Update, (reset_capture, ui::input, camera::sync).chain());
    let editor = app.world_mut().spawn(MapEditorCamera).id();
    let game = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            AmbientLight {
                brightness: 13.0,
                ..default()
            },
        ))
        .id();
    let button = app
        .world_mut()
        .spawn((
            Node::default(),
            ShadingButton(ShadingAction::Mode(ShadingMode::MaterialPreview)),
            EditorButtonClick::default(),
        ))
        .id();
    app.update();
    assert_eq!(
        app.world().resource::<ShadingState>().mode,
        ShadingMode::Rendered
    );
    assert!(
        app.world()
            .get::<bevy::core_pipeline::prepass::DepthPrepass>(editor)
            .is_none(),
        "默认渲染不额外开启预通道"
    );
    app.world_mut()
        .entity_mut(button)
        .insert(EditorButtonClick::triggered());
    app.update();
    assert_eq!(
        app.world().resource::<ShadingState>().mode,
        ShadingMode::MaterialPreview
    );
    assert_eq!(
        app.world().get::<AmbientLight>(editor).unwrap().brightness,
        800.0
    );
    assert_eq!(
        app.world().get::<AmbientLight>(game).unwrap().brightness,
        13.0
    );
    assert!(app.world().get::<ShadingView>(game).is_none());
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .suppress_world_pick_this_frame
    );
    app.world_mut().get_mut::<Node>(button).unwrap().display = Display::None;
    app.world_mut().entity_mut(button).insert((
        ShadingButton(ShadingAction::Mode(ShadingMode::Solid)),
        EditorButtonClick::triggered(),
    ));
    app.update();
    assert_eq!(
        app.world().resource::<ShadingState>().mode,
        ShadingMode::MaterialPreview
    );
    app.world_mut().get_mut::<Node>(button).unwrap().display = Display::Flex;
    app.update();
    assert_eq!(
        app.world().resource::<ShadingState>().mode,
        ShadingMode::MaterialPreview
    );
    app.world_mut()
        .entity_mut(button)
        .insert(EditorButtonClick::triggered());
    app.update();
    assert_eq!(
        app.world().resource::<ShadingState>().mode,
        ShadingMode::Solid
    );
    assert!(
        app.world().get::<AmbientLight>(editor).is_none(),
        "离开预览恢复相机原始补光附件"
    );
}

fn reset_capture(mut editor: ResMut<MapEditorState>) {
    editor.suppress_world_pick_this_frame = false;
}

#[test]
fn temporary_solid_and_wire_prepasses_restore_the_original_camera_attachments() {
    use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
    let mut app = App::new();
    app.init_resource::<ShadingState>()
        .add_systems(Update, camera::sync);
    let camera = app
        .world_mut()
        .spawn((MapEditorCamera, Msaa::Sample4, DepthPrepass))
        .id();
    app.update();
    assert_eq!(app.world().get::<Msaa>(camera), Some(&Msaa::Sample4));
    assert!(app.world().get::<NormalPrepass>(camera).is_none());
    for mode in [ShadingMode::Solid, ShadingMode::Wireframe] {
        app.world_mut().resource_mut::<ShadingState>().mode = mode;
        app.update();
        assert_eq!(app.world().get::<Msaa>(camera), Some(&Msaa::Off));
        assert!(app.world().get::<DepthPrepass>(camera).is_some());
        assert!(app.world().get::<NormalPrepass>(camera).is_some());
        app.world_mut().resource_mut::<ShadingState>().mode = ShadingMode::Rendered;
        app.update();
        assert_eq!(app.world().get::<Msaa>(camera), Some(&Msaa::Sample4));
        assert!(app.world().get::<DepthPrepass>(camera).is_some());
        assert!(app.world().get::<NormalPrepass>(camera).is_none());
        assert!(app.world().get::<PrepassLease>(camera).is_none());
    }
}

#[test]
fn wire_prepass_lease_can_move_to_solid_and_restores_a_camera_without_original_prepasses() {
    use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
    let mut app = App::new();
    app.init_resource::<ShadingState>()
        .add_systems(Update, camera::sync);
    let camera = app.world_mut().spawn((MapEditorCamera, Msaa::Sample4)).id();
    // 在线框与实体之间切换不能重新记录临时附件，否则退出时会把租来的预通道当成原附件。
    for mode in [ShadingMode::Wireframe, ShadingMode::Solid, ShadingMode::Wireframe] {
        app.world_mut().resource_mut::<ShadingState>().mode = mode;
        app.update();
        assert_eq!(app.world().get::<Msaa>(camera), Some(&Msaa::Off));
        assert!(app.world().get::<DepthPrepass>(camera).is_some());
        assert!(app.world().get::<NormalPrepass>(camera).is_some());
    }
    app.world_mut().resource_mut::<ShadingState>().mode = ShadingMode::Rendered;
    app.update();
    assert_eq!(app.world().get::<Msaa>(camera), Some(&Msaa::Sample4));
    assert!(app.world().get::<DepthPrepass>(camera).is_none());
    assert!(app.world().get::<NormalPrepass>(camera).is_none());
    assert!(app.world().get::<PrepassLease>(camera).is_none());
}

#[test]
fn overlay_controls_stay_top_right_after_narrow_resize_and_ui_scaling() {
    use crate::components::{MapEditorOrientationAxisRoot, MapEditorViewport};
    let mut app = App::new();
    app.add_systems(Update, ui::place_overlays);
    let viewport = app
        .world_mut()
        .spawn((
            MapEditorViewport,
            ComputedNode {
                size: Vec2::new(1100.0, 600.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
        ))
        .id();
    let axis = app
        .world_mut()
        .spawn((MapEditorOrientationAxisRoot, Node::default()))
        .id();
    let shading = app
        .world_mut()
        .spawn((ViewShadingRoot, Node::default()))
        .id();
    let menu = app
        .world_mut()
        .spawn((ViewShadingRoot, ShadingMenu, Node::default()))
        .id();
    for (size, inverse_scale) in [
        (Vec2::new(1100.0, 600.0), 1.0),
        (Vec2::new(528.0, 360.0), 1.0 / 1.7),
        (Vec2::new(1100.0, 600.0), 1.0),
        (Vec2::new(610.0, 400.0), 1.0),
        (Vec2::new(609.0, 400.0), 1.0),
    ] {
        {
            let mut node = app.world_mut().get_mut::<ComputedNode>(viewport).unwrap();
            node.size = size;
            node.inverse_scale_factor = inverse_scale;
        }
        app.update();
        let axis = app.world().get::<Node>(axis).unwrap();
        let shading = app.world().get::<Node>(shading).unwrap();
        let menu = app.world().get::<Node>(menu).unwrap();
        assert_eq!((axis.top, axis.bottom, axis.right), (px(42), Val::Auto, px(8)));
        assert_eq!((shading.top, shading.bottom, shading.right), (px(8), Val::Auto, px(8)));
        assert_eq!((menu.top, menu.bottom, menu.right), (px(30), Val::Auto, px(0)));
        for node in [axis, shading, menu] {
            assert_eq!(node.position_type, PositionType::Absolute);
        }
    }
}
