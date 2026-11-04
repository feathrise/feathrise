//! 正式离屏UI渲染统一创建面板；真实来源/用途按钮驱动截图，不打开用户窗口。

use super::*;
use bevy::{ecs::system::RunSystemOnce,
    render::{gpu_readback::{Readback, ReadbackComplete}, render_resource::{TextureFormat, TextureUsages}}};
use std::time::{Duration, Instant};

/// 正式命中系统只接受窗口目标；仅在命中瞬间映射窗口，渲染始终使用独立图像。
fn activate(app: &mut App, window: Entity, camera: Entity, target: Action) {
    let button = action(app, target);
    let position = app.world().get::<UiGlobalTransform>(button).unwrap().translation;
    app.world_mut().get_mut::<Window>(window).unwrap().set_physical_cursor_position(Some(position.as_dvec2()));
    let original = app.world().get::<bevy::camera::RenderTarget>(camera).unwrap().clone();
    *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
        bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    for pressed in [true, false] {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.clear();
        if pressed { mouse.press(MouseButton::Left); } else { mouse.release(MouseButton::Left); }
        app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
        app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
        assert_eq!(app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().is_clicked(), !pressed,
            "来源与用途必须等原按钮释放后确认：{target:?}");
        app.world_mut().run_system_once(systems::input).unwrap();
    }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = original;
    for _ in 0..8 { app.update(); std::thread::sleep(Duration::from_millis(16)); }
}

#[test]
#[ignore = "显式离屏验证统一网格创建面板的立方体、Clipmap与坐标模式，不接管用户原生窗口"]
fn unified_mesh_creation_gpu_snapshots() {
    let (mut app, window) = page_with_render(true);
    crate::ui::theme::install(&mut app);
    app.world_mut().resource_mut::<MapEditorSidebarState>().right = EditorRightSidebarTab::Tools;
    let (camera, target) = {
        let world = app.world_mut();
        let (camera, target) = world.query::<(Entity, &bevy::camera::RenderTarget)>().single(world).unwrap();
        let bevy::camera::RenderTarget::Image(target) = target else { panic!("必须使用独立离屏图像") };
        (camera, target.handle.clone())
    };
    let mut image = Image::new_target_texture(768, 1024, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    app.world_mut().resource_mut::<Assets<Image>>().insert(target.id(), image).unwrap();
    app.world_mut().get_mut::<Camera>(camera).unwrap().viewport = None;
    app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(768.0, 1024.0);
    {
        let world = app.world_mut();
        for mut node in world.query_filtered::<&mut Node, With<ListHost>>().iter_mut(world) { node.display = Display::None; }
        let host = world.query_filtered::<Entity, With<PageHost>>().single(world).unwrap();
        let root = world.get::<ChildOf>(host).unwrap().parent();
        world.get_mut::<Node>(root).unwrap().height = px(1000);
        let mut node = world.get_mut::<Node>(host).unwrap();
        node.width = px(360); node.height = px(1000);
        world.entity_mut(host).insert(BackgroundColor(crate::ui::theme::PANEL_BG));
    }
    for _ in 0..24 { app.update(); std::thread::sleep(Duration::from_millis(16)); }
    activate(&mut app, window, camera, Action::ToggleCreation);
    let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/unified-mesh-creation-validation");
    std::fs::create_dir_all(&output).unwrap();
    for (source, placement, name) in [
        (Action::Cuboid, Action::PlacementViewport, "cuboid-viewport.png"),
        (Action::Clipmap, Action::PlacementViewport, "clipmap-viewport-no-height-domain.png"),
        (Action::Cuboid, Action::PlacementCoordinates, "cuboid-coordinates.png"),
    ] {
        activate(&mut app, window, camera, source);
        activate(&mut app, window, camera, placement);
        let world = app.world_mut();
        let slots: Vec<_> = world.query::<(&Slot, &Node)>().iter(world).map(|(slot, node)| (slot.0, node.display)).collect();
        assert_eq!(slots.iter().find(|(field, _)| *field == Field::Creation(0)).unwrap().1 == Display::Flex,
            placement == Action::PlacementCoordinates);
        if source == Action::Clipmap {
            assert_eq!(slots.iter().find(|(field, _)| *field == Field::Creation(4)).unwrap().1, Display::None);
            assert_eq!(slots.iter().find(|(field, _)| *field == Field::CuboidCreation(0)).unwrap().1, Display::None);
        }
        let start = action(&mut app, Action::StartCreation);
        let center = app.world().get::<UiGlobalTransform>(start).unwrap().translation;
        let size = app.world().get::<ComputedNode>(start).unwrap().size();
        assert!(size.min_element() > 0.0 && center.y + size.y * 0.5 < 1000.0,
            "提交按钮必须完整显示在当前表单中：{name}");
        let (sender, receiver) = std::sync::mpsc::channel();
        app.world_mut().spawn(Readback::texture(target.clone())).observe(move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone()); commands.entity(event.entity).try_despawn();
        });
        let started = Instant::now();
        let bytes = loop {
            app.update();
            if let Ok(bytes) = receiver.try_recv() { break bytes; }
            assert!(started.elapsed().as_secs() < 10);
            std::thread::sleep(Duration::from_millis(16));
        };
        image::RgbaImage::from_raw(768, 1024, bytes).unwrap().save(output.join(name)).unwrap();
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}
