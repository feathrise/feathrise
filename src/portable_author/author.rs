//! 原作者App的离屏承载；输入阶段保留Window目标，只在渲染提取前改为Image，原控件/拾取不复制。

use super::{capture, resources::*, session::AuthorSession, types::*};
use bevy::{
    app::MainScheduleOrder,
    camera::RenderTarget,
    ecs::schedule::ScheduleLabel,
    input::keyboard::KeyboardFocusLost,
    picking::PickingSystems,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    window::{CursorIcon, CursorOptions, PrimaryWindow, WindowEvent, WindowFocused, WindowResized},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct NativeWindowPrepare;
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct RenderSurface;

pub(crate) struct EmbeddedAuthorPlugin;

impl Plugin for EmbeddedAuthorPlugin {
    fn build(&self, app: &mut App) {
        let target = match EmbedTarget::from_env() {
            Ok(Some(target)) => target,
            Ok(None) => return,
            Err(error) => {
                app.insert_resource(EmbeddedAuthorStatus {
                    error: Some(error),
                    ..default()
                });
                return;
            }
        };
        let session = match AuthorSession::open(&target) {
            Ok(session) => session,
            Err(error) => {
                app.insert_resource(EmbeddedAuthorStatus {
                    error: Some(error),
                    ..default()
                });
                return;
            }
        };
        install_session(app, session, target.pid);
    }
}

pub(super) fn install_session(app: &mut App, session: AuthorSession, parent_pid: u32) {
    capture::install(app);
    let geometry = Geometry::initial();
    let image = target_image(geometry);
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.insert_resource(EmbeddedAuthorStatus {
        attached: true,
        ..default()
    })
    .insert_non_send(AuthorSurface {
        session,
        geometry,
        target: image,
        stable_frames: 0,
        sequence: 0,
        readback: None,
        last_capture: Instant::now(),
        last_feedback: Vec::new(),
        desired_cursor: CursorOptions::default(),
        focused: false,
        parent_pid,
        desired_ime: (false, Vec2::ZERO),
        readback_completed: Arc::new(AtomicU64::new(0)),
    })
    // 无原生作者窗口的专用循环仍需持续处理父方输入与GPU结果，不能等待实体键鼠才推进。
    .insert_resource(bevy::winit::WinitSettings {
        focused_mode: bevy::winit::UpdateMode::reactive(Duration::from_millis(16)),
        unfocused_mode: bevy::winit::UpdateMode::reactive(Duration::from_millis(16)),
    })
    .add_systems(
        First,
        receive_and_restore
            .after(bevy::ecs::message::MessageUpdateSystems)
            .before(PickingSystems::Input),
    )
    .init_schedule(NativeWindowPrepare)
    .add_systems(NativeWindowPrepare, prepare_native_window)
    .init_schedule(RenderSurface)
    .add_systems(RenderSurface, render_surface);
    let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
    order.insert_before(Last, NativeWindowPrepare);
    order.insert_after(Last, RenderSurface);
}

fn target_image(geometry: Geometry) -> Image {
    // 不在作者帧中分配/清零整幅CPU像素；GPU负责初始化，后台只消费异步回读的实际完整结果。
    let mut image = Image::new_uninit(
        Extent3d {
            width: geometry.width,
            height: geometry.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    image
}

fn receive_and_restore(world: &mut World) {
    let Some(mut surface) = world.remove_non_send::<AuthorSurface>() else {
        return;
    };
    #[cfg(unix)]
    if unsafe { libc::getppid() } as u32 != surface.parent_pid {
        world.resource_mut::<EmbeddedAuthorStatus>().error =
            Some("工作台父进程已经退出或身份已变化，停止作者实例".into());
    }
    // 原作者所有业务系统照常看到原Window RenderTarget，兼容Bevy Interaction与现有地图拾取。
    let mut targets = world.query::<(Entity, &OriginalWindowTarget, &mut RenderTarget)>();
    let mut restored = Vec::new();
    for (entity, original, mut target) in targets.iter_mut(world) {
        *target = original.0.clone();
        restored.push(entity);
    }
    for entity in restored {
        // 标记只跨越一次渲染提取；业务随后若把相机改成自己的预览Image，下次不能强行改回主窗。
        world.entity_mut(entity).remove::<OriginalWindowTarget>();
    }
    if surface.readback.is_some() {
        if surface.readback_completed.load(Ordering::Acquire) == surface.sequence {
            surface.readback = None;
            world.resource_mut::<capture::CaptureRequest>().0 = None;
        }
    }
    // 原作者组件保留窗口语义，焦点以输入通道的真实主窗状态为准。
    world.resource_mut::<Messages<KeyboardFocusLost>>().clear();
    let mut batches = Vec::new();
    for _ in 0..4 {
        match surface.session.input.try_recv() {
            Ok(Some(batch)) => batches.push(batch),
            Ok(None) => break,
            Err(error) => {
                world.resource_mut::<EmbeddedAuthorStatus>().error =
                    Some(format!("作者输入已断开：{error}"));
                break;
            }
        }
    }
    let mut windows = world.query_filtered::<Entity, With<PrimaryWindow>>();
    let Ok(window_entity) = windows.single(world) else {
        world.insert_non_send(surface);
        return;
    };
    if let Some(mut cursor) = world.get_mut::<CursorOptions>(window_entity) {
        *cursor = surface.desired_cursor.clone();
    }
    if let Some(mut window) = world.get_mut::<Window>(window_entity) {
        window.ime_enabled = surface.desired_ime.0;
        window.ime_position = surface.desired_ime.1;
        if (
            window.physical_width(),
            window.physical_height(),
            window.scale_factor(),
        ) != (
            surface.geometry.width,
            surface.geometry.height,
            surface.geometry.scale,
        ) {
            window
                .resolution
                .set_scale_factor_override(Some(surface.geometry.scale));
            window
                .resolution
                .set_physical_resolution(surface.geometry.width, surface.geometry.height);
        }
    }
    for batch in batches {
        if batch.events.len() > MAX_EVENTS {
            world.resource_mut::<EmbeddedAuthorStatus>().error =
                Some("作者输入批次超过预算".into());
            continue;
        }
        let Ok(geometry) = batch.geometry.checked() else {
            continue;
        };
        if geometry.epoch < surface.geometry.epoch {
            continue;
        } // 迟到输入不能按新尺寸命中旧坐标。
        if geometry != surface.geometry {
            surface.geometry = geometry;
            surface.target = world
                .resource_mut::<Assets<Image>>()
                .add(target_image(geometry));
            surface.stable_frames = 0;
            if let Some(mut window) = world.get_mut::<Window>(window_entity) {
                window
                    .resolution
                    .set_scale_factor_override(Some(geometry.scale));
                window
                    .resolution
                    .set_physical_resolution(geometry.width, geometry.height);
            }
            let resized = WindowResized {
                window: window_entity,
                width: geometry.width as f32 / geometry.scale,
                height: geometry.height as f32 / geometry.scale,
            };
            world.write_message(resized.clone());
            world.write_message(WindowEvent::WindowResized(resized));
        }
        if world.resource::<EmbeddedAuthorStatus>().activated != batch.active {
            // 候选期反馈只用于预热；正式采用时必须重发当前IME/光标，即使它们的值没有变化。
            surface.last_feedback.clear();
        }
        world.resource_mut::<EmbeddedAuthorStatus>().activated = batch.active;
        let focused = batch.active && batch.focused;
        if surface.focused != focused {
            surface.focused = focused;
            surface.last_feedback.clear();
            let event = WindowFocused {
                window: window_entity,
                focused,
            };
            world.write_message(event.clone());
            world.write_message(WindowEvent::WindowFocused(event));
            if !focused {
                world.write_message(KeyboardFocusLost);
                world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
                if let Some(mut window) = world.get_mut::<Window>(window_entity) {
                    window.set_cursor_position(None);
                }
            }
        }
        // 候选可预热画面却不能接收用户输入；激活仍由原工程生命周期提交。
        if batch.active && focused {
            // 接管时鼠标可能已停在原位置且没有新Move；位置快照保证第一次点击也走原命中。
            if !batch
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::CursorMoved(_)))
            {
                let previous = world
                    .get::<Window>(window_entity)
                    .and_then(Window::cursor_position);
                if previous != batch.cursor_position {
                    if let Some(position) = batch.cursor_position {
                        inject(
                            world,
                            window_entity,
                            WindowEvent::CursorMoved(bevy::window::CursorMoved {
                                window: window_entity,
                                position,
                                delta: None,
                            }),
                        );
                    } else {
                        inject(
                            world,
                            window_entity,
                            WindowEvent::CursorLeft(bevy::window::CursorLeft {
                                window: window_entity,
                            }),
                        );
                    }
                }
            }
            for event in batch.events {
                inject(world, window_entity, event);
            }
        }
    }
    if let Some(mut window) = world.get_mut::<Window>(window_entity) {
        window.focused = surface.focused;
        window.visible = false;
    }
    if let Some(error) = surface.session.error.get() {
        world.resource_mut::<EmbeddedAuthorStatus>().error = Some(error.clone());
    }
    if surface
        .readback
        .as_ref()
        .is_some_and(|(_, since)| since.elapsed() > Duration::from_secs(15))
    {
        world.resource_mut::<EmbeddedAuthorStatus>().error = Some("作者GPU完整帧回读超时".into());
    }
    let font_ready = world
        .get_resource::<Assets<Font>>()
        .is_some_and(|fonts| fonts.contains(bevy::asset::AssetId::<Font>::default()));
    if surface.session.published.load(Ordering::Acquire) != 0 && font_ready {
        world.resource_mut::<EmbeddedAuthorStatus>().ready = true;
    }
    surface.stable_frames = surface.stable_frames.saturating_add(1);
    if world.resource::<EmbeddedAuthorStatus>().error.is_some() {
        // 与Windows嵌入身份丢失相同：不留下无法呈现/操作的孤儿工程进程；已落盘作者数据保留。
        world.write_message(AppExit::error());
    }
    world.insert_non_send(surface);
}

/// 仅转发官方输入类型；每条中的父Entity在作者World内重绑定，不能直接采用跨World实体编号。
fn inject(world: &mut World, window: Entity, mut event: WindowEvent) {
    macro_rules! emit {
        ($value:ident) => {{
            $value.window = window;
            world.write_message($value.clone());
        }};
    }
    match &mut event {
        WindowEvent::CursorMoved(value) => {
            value.window = window;
            if let Some(mut target) = world.get_mut::<Window>(window) {
                target.set_cursor_position(Some(value.position));
            }
            world.write_message(value.clone());
        }
        WindowEvent::CursorEntered(value) => emit!(value),
        WindowEvent::CursorLeft(value) => {
            value.window = window;
            if let Some(mut target) = world.get_mut::<Window>(window) {
                target.set_cursor_position(None);
            }
            world.write_message(value.clone());
        }
        WindowEvent::KeyboardInput(value) => emit!(value),
        WindowEvent::MouseButtonInput(value) => emit!(value),
        WindowEvent::MouseWheel(value) => emit!(value),
        WindowEvent::MouseMotion(value) => {
            world.write_message(*value);
        }
        WindowEvent::TouchInput(value) => emit!(value),
        WindowEvent::FileDragAndDrop(value) => {
            use bevy::window::FileDragAndDrop;
            match value {
                FileDragAndDrop::DroppedFile { window: entity, .. }
                | FileDragAndDrop::HoveredFile { window: entity, .. }
                | FileDragAndDrop::HoveredFileCanceled { window: entity } => *entity = window,
            }
            world.write_message(value.clone());
        }
        WindowEvent::Ime(value) => {
            use bevy::window::Ime;
            match value {
                Ime::Preedit { window: entity, .. }
                | Ime::Commit { window: entity, .. }
                | Ime::Enabled { window: entity }
                | Ime::Disabled { window: entity } => *entity = window,
            }
            world.write_message(value.clone());
        }
        WindowEvent::KeyboardFocusLost(value) => {
            world.write_message(value.clone());
        }
        WindowEvent::PinchGesture(value) => {
            world.write_message(*value);
        }
        WindowEvent::RotationGesture(value) => {
            world.write_message(*value);
        }
        WindowEvent::DoubleTapGesture(value) => {
            world.write_message(*value);
        }
        WindowEvent::PanGesture(value) => {
            world.write_message(*value);
        }
        _ => return,
    }
    world.write_message(event);
}

fn prepare_native_window(world: &mut World) {
    let Some(mut surface) = world.remove_non_send::<AuthorSurface>() else {
        return;
    };
    let mut windows = world.query_filtered::<(Entity, &Window, &CursorOptions, Option<&CursorIcon>), With<PrimaryWindow>>();
    if let Ok((entity, window, cursor, icon)) = windows.single(world) {
        surface.desired_cursor = cursor.clone();
        surface.desired_ime = (window.ime_enabled, window.ime_position);
        let feedback = Feedback {
            epoch: surface.geometry.epoch,
            ime_enabled: window.ime_enabled,
            ime_position: window.ime_position,
            cursor: cursor.clone(),
            cursor_icon: icon.and_then(|icon| match icon {
                CursorIcon::System(icon) => Some(*icon),
                _ => None,
            }),
        };
        if let Ok(encoded) = serde_json::to_vec(&feedback) {
            if encoded != surface.last_feedback && surface.session.input.try_send(feedback).is_ok()
            {
                surface.last_feedback = encoded;
            }
        }
        // Winit只同步隐藏原生窗口；真实IME、鼠标捕获及光标由父主窗口执行同一请求。
        if let Some(mut window) = world.get_mut::<Window>(entity) {
            window.focused = false;
            window.visible = false;
            window.ime_enabled = false;
        }
        if let Some(mut cursor) = world.get_mut::<CursorOptions>(entity) {
            cursor.visible = false;
            cursor.grab_mode = bevy::window::CursorGrabMode::None;
        }
    }
    world.insert_non_send(surface);
}

fn render_surface(world: &mut World) {
    let Some(mut surface) = world.remove_non_send::<AuthorSurface>() else {
        return;
    };
    let mut query = world
        .query_filtered::<(Entity, &RenderTarget), (With<Camera>, Without<OriginalWindowTarget>)>();
    let targets = query
        .iter(world)
        .filter(|(_, target)| matches!(target, RenderTarget::Window(_)))
        .map(|(entity, target)| (entity, target.clone()))
        .collect::<Vec<_>>();
    for (entity, target) in targets {
        world
            .entity_mut(entity)
            .insert(OriginalWindowTarget(target));
    }
    let mut cameras = world.query_filtered::<&mut RenderTarget, With<OriginalWindowTarget>>();
    for mut target in cameras.iter_mut(world) {
        *target = RenderTarget::Image(surface.target.clone().into());
    }
    // 30Hz上限 + single-flight；GPU慢时丢过时帧，保持作者UI继续更新，绝不同步等待map_async。
    if surface.stable_frames >= 4
        && surface.readback.is_none()
        && surface.last_capture.elapsed() >= Duration::from_millis(33)
    {
        surface.sequence = surface.sequence.saturating_add(1);
        world.resource_mut::<capture::CaptureRequest>().0 = Some(capture::Ticket {
            image: surface.target.clone(),
            geometry: surface.geometry,
            sequence: surface.sequence,
            frames: surface.session.frames.clone(),
            completed: surface.readback_completed.clone(),
            error: surface.session.error.clone(),
        });
        surface.readback = Some((surface.sequence, Instant::now()));
        surface.last_capture = Instant::now();
    }
    world.insert_non_send(surface);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_entity_is_rebound_and_mouse_position_uses_original_window_contract() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::input::InputPlugin)
            .add_plugins(WindowPlugin::default());
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let source = Entity::from_raw_u32(731).unwrap();
        inject(
            app.world_mut(),
            window,
            WindowEvent::CursorMoved(bevy::window::CursorMoved {
                window: source,
                position: Vec2::new(18., 32.),
                delta: None,
            }),
        );
        assert_eq!(
            app.world().get::<Window>(window).unwrap().cursor_position(),
            Some(Vec2::new(18., 32.))
        );
        let event = app
            .world_mut()
            .resource_mut::<Messages<bevy::window::CursorMoved>>()
            .drain()
            .next()
            .unwrap();
        assert_eq!(event.window, window);
    }
}
