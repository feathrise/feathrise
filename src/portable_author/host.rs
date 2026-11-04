//! 主工作台中的原作者合成层；主线程只收完整最新帧和有限输入，不访问共享像素或等待worker。

use super::{resources::HostSurface, session::HostSession, types::*};
use bevy::{
    ecs::message::MessageUpdateSystems,
    input::{
        gestures::*,
        keyboard::{KeyboardFocusLost, KeyboardInput},
        mouse::{MouseButtonInput, MouseMotion, MouseWheel},
        touch::TouchInput,
    },
    picking::PickingSystems,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{
        CursorEntered, CursorIcon, CursorLeft, CursorMoved, CursorOptions, FileDragAndDrop, Ime,
        PrimaryWindow, WindowEvent,
    },
};

pub(crate) fn install(app: &mut App) {
    app.init_resource::<HostSurface>().add_systems(
        First,
        collect_input
            .after(MessageUpdateSystems)
            .before(PickingSystems::Input),
    );
}

/// 首先取走要转发的官方输入，避免父方背后的同一套作者外壳再次消费按键或点击。
fn collect_input(world: &mut World) {
    if world.resource::<HostSurface>().token.is_none() {
        return;
    }
    let messages = world
        .resource_mut::<Messages<WindowEvent>>()
        .drain()
        .collect::<Vec<_>>();
    let mut forwarded = Vec::new();
    for event in messages {
        if matches!(
            event,
            WindowEvent::CursorEntered(_)
                | WindowEvent::CursorLeft(_)
                | WindowEvent::CursorMoved(_)
                | WindowEvent::MouseButtonInput(_)
                | WindowEvent::MouseMotion(_)
                | WindowEvent::MouseWheel(_)
                | WindowEvent::KeyboardInput(_)
                | WindowEvent::KeyboardFocusLost(_)
                | WindowEvent::Ime(_)
                | WindowEvent::TouchInput(_)
                | WindowEvent::PinchGesture(_)
                | WindowEvent::RotationGesture(_)
                | WindowEvent::DoubleTapGesture(_)
                | WindowEvent::PanGesture(_)
                | WindowEvent::FileDragAndDrop(_)
        ) {
            forwarded.push(event);
        } else {
            world.write_message(event);
        }
    }
    let mut surface = world.resource_mut::<HostSurface>();
    if forwarded.len() > MAX_EVENTS.saturating_sub(surface.events.len()) {
        // 饱和时明确撤销输入焦点，不能把丢掉的释放边沿变成作者世界的永久按住。
        surface.input_overflow = true;
        surface.events.clear();
    } else if !surface.input_overflow {
        surface.events.extend(forwarded);
    }
    macro_rules! clear { ($($kind:ty),*) => { $(world.resource_mut::<Messages<$kind>>().clear();)* }; }
    clear!(
        KeyboardInput,
        KeyboardFocusLost,
        MouseButtonInput,
        MouseMotion,
        MouseWheel,
        CursorMoved,
        CursorEntered,
        CursorLeft,
        Ime,
        TouchInput,
        PinchGesture,
        RotationGesture,
        DoubleTapGesture,
        PanGesture,
        FileDragAndDrop
    );
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    world
        .resource_mut::<ButtonInput<bevy::input::keyboard::Key>>()
        .reset_all();
    world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
}

pub(crate) fn poll(
    session: &mut HostSession,
    world: &mut World,
    active: bool,
) -> Result<(), String> {
    session.connect()?;
    let mut windows = world.query_filtered::<(Entity, &Window), With<PrimaryWindow>>();
    let (window_entity, window) = windows.single(world).map_err(|_| "主窗口尚未建立")?;
    let ratio = (MAX_WIDTH as f32 / window.physical_width().max(1) as f32)
        .min(MAX_HEIGHT as f32 / window.physical_height().max(1) as f32)
        .min(1.);
    let width = (window.physical_width() as f32 * ratio).round().max(1.) as u32;
    let height = (window.physical_height() as f32 * ratio).round().max(1.) as u32;
    let scale = window.scale_factor() * ratio;
    let focused = window.focused;
    let cursor_position = window.cursor_position();
    if (width, height, scale)
        != (
            session.geometry.width,
            session.geometry.height,
            session.geometry.scale,
        )
    {
        session.geometry = Geometry {
            epoch: session.geometry.epoch.saturating_add(1),
            width,
            height,
            scale,
        }
        .checked()?;
        session.first_frame = false;
    }
    if let Some(input) = &session.input {
        let surface = world.resource::<HostSurface>();
        let events = if active {
            surface.events.clone()
        } else {
            Vec::new()
        };
        let overflow = active && surface.input_overflow;
        let batch = InputBatch {
            geometry: session.geometry,
            active,
            focused: focused && !overflow,
            cursor_position: if active { cursor_position } else { None },
            events,
        };
        if input.try_send(batch).is_ok() && active {
            let mut surface = world.resource_mut::<HostSurface>();
            surface.events.clear();
            surface.input_overflow = false;
        }
        for _ in 0..4 {
            match input.try_recv()? {
                Some(feedback) if active && feedback.epoch == session.geometry.epoch => {
                    if let Some(mut window) = world.get_mut::<Window>(window_entity) {
                        window.ime_enabled = feedback.ime_enabled;
                        window.ime_position = feedback.ime_position;
                    }
                    if let Some(mut cursor) = world.get_mut::<CursorOptions>(window_entity) {
                        *cursor = feedback.cursor;
                    }
                    if let Some(icon) = feedback.cursor_icon {
                        world
                            .entity_mut(window_entity)
                            .insert(CursorIcon::System(icon));
                    }
                }
                Some(_) => {}
                None => break,
            }
        }
        if overflow {
            return Err("作者输入通道拥塞，已释放按键；请稍后继续操作".into());
        }
    }
    let frame = session
        .latest
        .try_lock()
        .ok()
        .and_then(|mut latest| latest.take());
    if let Some(frame) = frame {
        if frame.geometry == session.geometry
            && frame.sequence > session.last_frame
            && frame.pixels.len() == session.geometry.pixels()
        {
            // RENDER_WORLD让Bevy提取时移交Vec，不在宿主帧克隆整张截图；图像上传每次至多一份。
            let image = Image::new(
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                frame.pixels,
                TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
            );
            if let Some(handle) = &session.image {
                world
                    .resource_mut::<Assets<Image>>()
                    .insert(handle.id(), image)
                    .map_err(|error| error.to_string())?;
            } else {
                session.image = Some(world.resource_mut::<Assets<Image>>().add(image));
            }
            session.last_frame = frame.sequence;
            session.first_frame = true;
        }
    }
    if active {
        present(session, world)?;
    }
    Ok(())
}

pub(crate) fn activate(session: &mut HostSession, world: &mut World) -> Result<(), String> {
    if !session.ready() || session.input.is_none() {
        return Err("候选作者共享画面尚未就绪".into());
    }
    clear(world);
    world.resource_mut::<HostSurface>().token = Some(session.token.clone());
    present(session, world)
}

fn present(session: &HostSession, world: &mut World) -> Result<(), String> {
    let image = session.image.clone().ok_or("作者完整帧尚未到达")?;
    if world.resource::<HostSurface>().token.as_deref() != Some(session.token.as_str()) {
        return Ok(());
    }
    let mut surface = world.remove_resource::<HostSurface>().unwrap();
    if let Some(entity) = surface.entity {
        if let Some(mut node) = world.get_mut::<ImageNode>(entity) {
            node.image = image.clone();
        }
    } else {
        surface.entity = Some(
            world
                .spawn((
                    Name::new("工程作者原画面"),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.),
                        top: Val::Px(0.),
                        width: Val::Percent(100.),
                        height: Val::Percent(100.),
                        ..default()
                    },
                    // 此层只承担合成，所有原按钮/文字/预览仍由独立作者App渲染；不重新搭一套业务界面。
                    ImageNode::new(image.clone()),
                    GlobalZIndex(i32::MAX - 16),
                    bevy::ui::FocusPolicy::Block,
                ))
                .id(),
        );
    }
    surface.image = image;
    surface.geometry = Some(session.geometry);
    world.insert_resource(surface);
    Ok(())
}

pub(crate) fn clear(world: &mut World) {
    if let Some(mut surface) = world.remove_resource::<HostSurface>() {
        if let Some(entity) = surface.entity.take() {
            let _ = world.despawn(entity);
        }
        world.insert_resource(HostSurface::default());
    }
    let mut windows =
        world.query_filtered::<(Entity, &mut Window, &mut CursorOptions), With<PrimaryWindow>>();
    if let Ok((entity, mut window, mut cursor)) = windows.single_mut(world) {
        window.ime_enabled = false;
        *cursor = CursorOptions::default();
        world.entity_mut(entity).insert(CursorIcon::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_dimensions_reject_malformed_or_unbounded_results() {
        assert!(
            Geometry {
                width: MAX_WIDTH + 1,
                ..Geometry::initial()
            }
            .checked()
            .is_err()
        );
        assert!(
            Geometry {
                scale: f32::NAN,
                ..Geometry::initial()
            }
            .checked()
            .is_err()
        );
        assert!(
            Geometry {
                epoch: 0,
                ..Geometry::initial()
            }
            .checked()
            .is_err()
        );
    }
}
