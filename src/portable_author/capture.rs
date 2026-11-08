//! 专用异步GPU回读：RenderApp只提交复制/映射，map回调只投递Buffer，像素始终由帧worker读取。

use super::types::{Geometry, MappedFrame};
use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{
            Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, MapMode,
            TexelCopyBufferInfo, TexelCopyBufferLayout, Texture,
        },
        renderer::{RenderDevice, RenderQueue},
        texture::GpuImage,
    },
};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
    mpsc::{SyncSender, TrySendError},
};

#[derive(Clone)]
pub(super) struct Ticket {
    pub image: Handle<Image>,
    pub geometry: Geometry,
    pub sequence: u64,
    pub frames: SyncSender<MappedFrame>,
    pub completed: Arc<AtomicU64>,
    pub error: Arc<OnceLock<String>>,
}

#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(RenderApp)]
pub(super) struct CaptureRequest(pub Option<Ticket>);

struct Prepared {
    ticket: Ticket,
    texture: Texture,
    buffer: Buffer,
    stride: u32,
}

#[derive(Resource, Default)]
struct CaptureState {
    sequence: u64,
    buffer: Option<Buffer>,
    prepared: Option<Prepared>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<CaptureRequest>()
        .add_plugins(ExtractResourcePlugin::<CaptureRequest>::default());
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .init_resource::<CaptureState>()
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            // 主CameraDriver已经提交本帧原作者画面后，在同一Queue后提交COPY，保证不读到半帧。
            .add_systems(Render, submit.in_set(RenderSystems::Cleanup));
    }
}

fn prepare(
    request: Res<CaptureRequest>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    mut state: ResMut<CaptureState>,
) {
    let Some(ticket) = &request.0 else {
        return;
    };
    if ticket.sequence <= state.sequence || state.prepared.is_some() {
        return;
    }
    let Some(image) = images.get(&ticket.image) else {
        return;
    };
    if image.size_2d() != UVec2::new(ticket.geometry.width, ticket.geometry.height) {
        return;
    }
    let stride = (ticket.geometry.width * 4).next_multiple_of(256);
    let size = stride as u64 * ticket.geometry.height as u64;
    if state
        .buffer
        .as_ref()
        .is_none_or(|buffer| buffer.size() != size)
    {
        state.buffer = Some(device.create_buffer(&BufferDescriptor {
            label: Some("Feathrise作者单槽异步回读"),
            size,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }));
    }
    state.sequence = ticket.sequence;
    state.prepared = Some(Prepared {
        ticket: ticket.clone(),
        texture: image.texture.clone(),
        buffer: state.buffer.as_ref().unwrap().clone(),
        stride,
    });
}

fn submit(mut state: ResMut<CaptureState>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let Some(prepared) = state.prepared.take() else {
        return;
    };
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("Feathrise作者帧COPY"),
    });
    encoder.copy_texture_to_buffer(
        prepared.texture.as_image_copy(),
        TexelCopyBufferInfo {
            buffer: &prepared.buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(prepared.stride),
                rows_per_image: None,
            },
        },
        Extent3d {
            width: prepared.ticket.geometry.width,
            height: prepared.ticket.geometry.height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let buffer = prepared.buffer.clone();
    let ticket = prepared.ticket;
    prepared
        .buffer
        .slice(..)
        .map_async(MapMode::Read, move |result| {
            if let Err(error) = result {
                let _ = ticket.error.set(format!("作者GPU帧映射失败：{error}"));
                ticket.completed.store(ticket.sequence, Ordering::Release);
                return;
            }
            // 不调用get_mapped_range/Vec::from；驱动可能在Render线程执行本回调，这里只交接句柄。
            let mapped = MappedFrame {
                geometry: ticket.geometry,
                sequence: ticket.sequence,
                buffer,
                completed: ticket.completed.clone(),
            };
            if let Err(error) = ticket.frames.try_send(mapped) {
                let frame = match error {
                    TrySendError::Full(frame) | TrySendError::Disconnected(frame) => frame,
                };
                frame.buffer.unmap();
                let _ = ticket
                    .error
                    .set("作者帧worker未能接收唯一在途GPU结果".into());
                ticket.completed.store(ticket.sequence, Ordering::Release);
            }
        });
}
