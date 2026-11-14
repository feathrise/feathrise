//! 真实CameraUpdate与按压身份回归；使用正式离屏Image目标，不手填computed投影。
use super::*;
use bevy::{
    asset::{AssetEvent, RenderAssetUsages},
    camera::RenderTarget,
    render::{camera::camera_system, render_resource::{Extent3d, TextureDimension, TextureFormat}, texture::ManualTextureViews},
    window::{WindowCreated, WindowResized, WindowScaleFactorChanged},
};

fn real_camera_fixture() -> App {
    let mut app = fixture();
    app.add_message::<WindowCreated>()
        .add_message::<WindowResized>()
        .add_message::<WindowScaleFactorChanged>()
        .add_message::<AssetEvent<Image>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<ManualTextureViews>()
        .add_systems(PostUpdate, camera_system);
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(Image::new_fill(
        Extent3d { width: 128, height: 128, depth_or_array_layers: 1 }, TextureDimension::D2,
        &[0; 4], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::MAIN_WORLD,
    ));
    let camera = app.world().resource::<SceneEditorViewportState>().camera;
    app.world_mut().entity_mut(camera).insert((Camera::default(), RenderTarget::from(image)));
    app.world_mut().resource_mut::<SceneEditorState>().orthographic = false;
    move_pointer(&mut app, -0.12);
    app
}

#[test]
fn real_camera_update_keeps_the_settled_hover_press_valid() {
    let mut app = real_camera_fixture();
    until(&mut app, |app| app.world().resource::<SceneEditorState>().brush_cursor_world.is_some()
        && !cursor_running_for_tests(app.world()));
    app.world_mut().resource_mut::<SceneEditorState>().orthographic = true;
    step(&mut app); // 等同原夹具首次Hover：先正式相机发布，再由PostUpdate采用投影。
    until(&mut app, |app| app.world().resource::<SceneEditorState>().brush_cursor_world.is_some());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app);
    until(&mut app, |app| app.world().resource::<VegetationBrushStroke>().active);
    assert!(pixels(&app, "grass-a").iter().any(|value|*value>0));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<VegetationBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}
