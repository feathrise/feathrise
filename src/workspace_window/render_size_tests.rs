//! 用自有隐藏原生窗口与真实相机更新证明：late原生还原不改变已计算的渲染尺寸。

use super::*;
use bevy::{
    camera::Viewport,
    ecs::system::SystemState,
    render::{camera::camera_system, texture::ManualTextureViews},
    window::WindowCreated,
};

struct HiddenWindow(HWND);

impl HiddenWindow {
    fn new(parent: Option<HWND>, width: u32, height: u32) -> Self {
        let class: Vec<_> = "STATIC".encode_utf16().chain(Some(0)).collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                if parent.is_some() { WS_CHILD } else { WS_POPUP },
                0,
                0,
                width as i32,
                height as i32,
                parent.unwrap_or(std::ptr::null_mut()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "仅创建本测试拥有的隐藏窗口");
        Self(hwnd)
    }

    fn resize(&self, width: u32, height: u32) {
        assert_ne!(
            unsafe {
                SetWindowPos(
                    self.0,
                    std::ptr::null_mut(),
                    0,
                    0,
                    width as i32,
                    height as i32,
                    SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOZORDER,
                )
            },
            0
        );
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

fn state(target: EmbedTarget) -> EmbedState {
    EmbedState {
        target: Some(target),
        candidate: true,
        started: Instant::now(),
        frames: 0,
        last_size: None,
        focus_settled: false,
        root_was_foreground: false,
        native_focused: false,
        play_completed: 0,
        play_return_requested: false,
        failed: false,
    }
}

fn publish(world: &mut World, entity: Entity, size: RenderSizeSnapshot) {
    let mut params = SystemState::<(
        Query<&mut Window>,
        MessageWriter<WindowResized>,
        MessageWriter<WindowScaleFactorChanged>,
    )>::new(world);
    let (mut windows, mut resized, mut scale_changed) = params.get_mut(world).unwrap();
    publish_render_size(
        entity,
        &mut windows.get_mut(entity).unwrap(),
        size,
        &mut resized,
        &mut scale_changed,
    );
}

fn app() -> App {
    let mut app = App::new();
    app.add_message::<WindowResized>()
        .add_message::<WindowCreated>()
        .add_message::<WindowScaleFactorChanged>()
        .add_message::<AssetEvent<Image>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<ManualTextureViews>()
        .add_systems(PostUpdate, camera_system);
    app
}

#[test]
fn late_native_restore_keeps_render_snapshot_until_next_first_and_camera_update() {
    // 隐藏同进程夹具直接调用生产采样/发布/校正函数，不绕过生产system的跨进程身份门。
    let parent = HiddenWindow::new(None, 3840, 2034);
    let child = HiddenWindow::new(Some(parent.0), 3840, 2034);
    let target = EmbedTarget {
        parent: parent.0 as usize,
        pid: std::process::id(),
    };
    let status = EmbeddedAuthorStatus {
        attached: true,
        hwnd: Some(child.0 as usize),
        ..default()
    };
    let mut state = state(target);
    let mut app = app();
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                viewport: Some(Viewport {
                    physical_size: UVec2::new(3840, 54),
                    ..default()
                }),
                ..default()
            },
        ))
        .id();
    let default_camera = app.world_mut().spawn(Camera2d).id();
    let size = render_size_snapshot(target, &status, child.0 as usize).unwrap();
    assert_eq!(size.physical, (3840, 2034));
    publish(app.world_mut(), window, size);
    app.world_mut().run_schedule(PostUpdate);
    assert_eq!(
        app.world()
            .get::<Camera>(camera)
            .unwrap()
            .physical_target_size(),
        Some(UVec2::new(3840, 2034))
    );

    // 对应真实run06：PostUpdate已经算出3840裁剪后，root在late前还原到1905。
    parent.resize(1905, 1200);
    sync_native_size(&mut state, parent.0, child.0, false).unwrap();
    assert!(unsafe { fills_client(parent.0, child.0, (1905, 1200)) });
    assert_eq!(
        app.world().get::<Window>(window).unwrap().physical_size(),
        UVec2::new(3840, 2034),
        "late不能单独缩小提取交换链，留下旧UI裁剪"
    );
    assert_eq!(
        app.world()
            .get::<Camera>(camera)
            .unwrap()
            .viewport
            .as_ref()
            .unwrap()
            .physical_size,
        UVec2::new(3840, 54)
    );

    // 下一First采用新快照；真实camera_system消费生产消息，手工/默认两种视口一起更新。
    let size = render_size_snapshot(target, &status, child.0 as usize).unwrap();
    publish(app.world_mut(), window, size);
    app.world_mut().run_schedule(PostUpdate);
    for entity in [camera, default_camera] {
        assert_eq!(
            app.world()
                .get::<Camera>(entity)
                .unwrap()
                .physical_target_size(),
            Some(UVec2::new(1905, 1200))
        );
    }
    assert_eq!(
        app.world()
            .get::<Camera>(camera)
            .unwrap()
            .viewport
            .as_ref()
            .unwrap()
            .physical_size,
        UVec2::new(1905, 54)
    );
}

#[test]
fn render_snapshot_rejects_unattached_replaced_failed_or_reparented_source() {
    let parent = HiddenWindow::new(None, 800, 600);
    let other = HiddenWindow::new(None, 800, 600);
    let child = HiddenWindow::new(Some(parent.0), 800, 600);
    let target = EmbedTarget {
        parent: parent.0 as usize,
        pid: std::process::id(),
    };
    let mut status = EmbeddedAuthorStatus {
        hwnd: Some(child.0 as usize),
        ..default()
    };
    assert!(render_size_snapshot(target, &status, child.0 as usize).is_none());
    status.attached = true;
    assert!(render_size_snapshot(target, &status, child.0 as usize).is_some());
    assert!(render_size_snapshot(target, &status, other.0 as usize).is_none());
    assert!(
        render_size_snapshot(
            EmbedTarget {
                pid: target.pid.wrapping_add(1),
                ..target
            },
            &status,
            child.0 as usize
        )
        .is_none()
    );
    status.error = Some("已失效".into());
    assert!(render_size_snapshot(target, &status, child.0 as usize).is_none());
    status.error = None;
    unsafe { SetParent(child.0, other.0) };
    assert!(render_size_snapshot(target, &status, child.0 as usize).is_none());
}

#[test]
fn render_size_and_dpi_publish_one_snapshot_without_repeating_messages() {
    let mut app = app();
    let entity = app.world_mut().spawn(Window::default()).id();
    let size = RenderSizeSnapshot {
        physical: (1905, 1200),
        scale: 1.5,
    };
    publish(app.world_mut(), entity, size);
    let window = app.world().get::<Window>(entity).unwrap();
    assert_eq!(window.physical_size(), UVec2::new(1905, 1200));
    assert_eq!(window.scale_factor(), 1.5);
    assert_eq!(window.size(), Vec2::new(1270., 800.));
    let messages = app.world().resource::<Messages<WindowResized>>();
    let mut cursor = messages.get_cursor();
    let events: Vec<_> = cursor.read(messages).collect();
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].width, events[0].height), (1270., 800.));
    assert_eq!(
        app.world()
            .resource::<Messages<WindowScaleFactorChanged>>()
            .len(),
        1
    );
    publish(app.world_mut(), entity, size);
    assert_eq!(app.world().resource::<Messages<WindowResized>>().len(), 1);
    assert_eq!(
        app.world()
            .resource::<Messages<WindowScaleFactorChanged>>()
            .len(),
        1
    );
}
