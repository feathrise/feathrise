//! Cursor后再检查模态与超时，不让定位等待或迟到确认锁住编辑器。
use super::{request::build_request, resources::SpatialBrushState, systems, types::*};
use crate::{
    authoring::EditorAuthoringSession,
    resources::{SceneEditorSettingsState, SceneEditorState},
    viewport::VegetationGroundCursor,
};
use bevy::{ecs::system::RunSystemOnce, prelude::*};

#[test]
fn spatial_post_cursor_modal_cancels_pending_confirmation_and_never_replays_it() {
    let (mut app, _) = super::tests::app();
    app.init_resource::<VegetationGroundCursor>();
    let request = build_request(
        app.world().resource::<SceneEditorState>(),
        app.world().resource::<EditorAuthoringSession>(),
        Some(Vec3::ZERO),
        ApplyMode::Point,
        None,
        None,
    )
    .unwrap();
    app.world_mut().resource_mut::<SpatialBrushState>().pending = Some(request);
    app.world_mut()
        .resource_mut::<SceneEditorSettingsState>()
        .open = true;
    app.world_mut().run_system_once(systems::place).unwrap();
    let state = app.world().resource::<SpatialBrushState>();
    assert!(state.pending.is_none());
    assert!(!state.authoring_available());
    app.world_mut()
        .resource_mut::<SceneEditorSettingsState>()
        .open = false;
    app.world_mut().run_system_once(systems::place).unwrap();
    let state = app.world().resource::<SpatialBrushState>();
    assert!(state.pending.is_none());
    assert!(state.authoring_available());
}

#[test]
fn spatial_placement_timeout_releases_pending_click_even_without_a_ground_snapshot() {
    let (mut app, _) = super::tests::app();
    app.init_resource::<VegetationGroundCursor>();
    let identity = Identity::capture(
        app.world().resource::<SceneEditorState>(),
        app.world().resource::<EditorAuthoringSession>(),
    )
    .unwrap();
    let started = app.world().resource::<Time<Real>>().elapsed_secs_f64();
    {
        let mut state = app.world_mut().resource_mut::<SpatialBrushState>();
        state.armed = true;
        state.click_pending = true;
        state.click_identity = Some(identity);
        state.click_started = started;
    }
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs(31));
    app.world_mut().run_system_once(systems::place).unwrap();
    let state = app.world().resource::<SpatialBrushState>();
    assert!(!state.click_pending);
    assert!(state.armed);
    assert_eq!(
        state.center,
        Some(Vec3::new(0., -2., 0.)),
        "定位超时不能把已有刷心改为假平面"
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("超时")
    );
}
