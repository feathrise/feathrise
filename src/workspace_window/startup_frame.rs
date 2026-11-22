//! Windows 隐藏作者窗口的首帧回执，按工作台相机和窗口物理尺寸确认真实呈现提交。

use bevy::{
    camera::NormalizedRenderTarget,
    core_pipeline::upscaling::ViewUpscalingPipeline,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        render_resource::PipelineCache,
        renderer::render_system,
        sync_world::MainEntity,
        view::{ViewTarget, window::ExtractedWindow},
    },
    ui::ComputedUiTargetCamera,
    window::PrimaryWindow,
};
use feathrise_editor_scene::workbench::WorkbenchHeader;
use serde::{Deserialize, Serialize};

use super::EmbeddedAuthorStatus;

/// 旧尺寸或 DPI 的画面不能用于显示新尺寸的 HWND。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StartupFrameSize {
    pub physical: (u32, u32),
    pub dpi: u32,
}

impl StartupFrameSize {
    fn from_window(window: &Window) -> Self {
        Self {
            physical: (window.physical_width(), window.physical_height()),
            dpi: (window.scale_factor() * 96.0).round() as u32,
        }
    }
}

#[derive(Resource)]
pub(super) struct StartupFrameReceiver(pub async_channel::Receiver<StartupFrameSize>);

#[derive(Clone, Copy)]
struct Request {
    window: Entity,
    camera: Entity,
    size: StartupFrameSize,
}

#[derive(Resource)]
struct FrameProbe {
    sender: async_channel::Sender<StartupFrameSize>,
    request: Option<Request>,
    acquired: bool,
}

pub(super) fn install(app: &mut App) {
    let (sender, receiver) = async_channel::bounded(1);
    app.insert_resource(StartupFrameReceiver(receiver));
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .insert_resource(FrameProbe {
            sender,
            request: None,
            acquired: false,
        })
        .add_systems(ExtractSchedule, extract_request)
        .add_systems(
            Render,
            note_acquire
                .in_set(RenderSystems::Render)
                .before(render_system),
        )
        .add_systems(Render, report_present.in_set(RenderSystems::Cleanup));
}

fn extract_request(
    status: Extract<Res<EmbeddedAuthorStatus>>,
    windows: Extract<Query<(Entity, &Window), With<PrimaryWindow>>>,
    headers: Extract<
        Query<
            (&ComputedNode, &ComputedUiTargetCamera, &InheritedVisibility),
            With<WorkbenchHeader>,
        >,
    >,
    mut proof: ResMut<FrameProbe>,
) {
    proof.acquired = false;
    proof.request = (|| {
        if !status.attached || status.activated || status.error.is_some() {
            return None;
        }
        let (window, settings) = windows.single().ok()?;
        let (node, camera, visible) = headers.single().ok()?;
        if node.is_empty()
            || !visible.get()
            || settings.resolution.scale_factor_override().is_none()
        {
            return None;
        }
        Some(Request {
            window,
            camera: camera.get()?,
            size: StartupFrameSize::from_window(settings),
        })
    })();
}

fn note_acquire(mut proof: ResMut<FrameProbe>, windows: Query<(&MainEntity, &ExtractedWindow)>) {
    if let Some(request) = proof.request {
        proof.acquired = windows.iter().any(|(entity, window)| {
            entity.id() == request.window
                && window.swap_chain_texture.is_some()
                && (window.physical_width, window.physical_height) == request.size.physical
        });
    }
}

fn report_present(
    proof: Res<FrameProbe>,
    windows: Query<(&MainEntity, &ExtractedWindow)>,
    cameras: Query<(
        &MainEntity,
        &ExtractedCamera,
        &ViewTarget,
        &ViewUpscalingPipeline,
    )>,
    pipelines: Res<PipelineCache>,
) {
    let Some(request) = proof.request.filter(|_| proof.acquired) else {
        return;
    };
    let composed = cameras.iter().any(|(entity, camera, view, pipeline)| {
        entity.id() == request.camera
            && matches!(camera.target, Some(NormalizedRenderTarget::Window(window)) if window.entity() == request.window)
            && camera.physical_target_size == Some(UVec2::new(request.size.physical.0, request.size.physical.1))
            && view.needs_present() && pipeline.is_ready(&pipelines)
    });
    // RenderApp 已将真实取得的图像交给 present。冷管线的纯清屏托底不证明工作台已绘制。
    if composed
        && windows.iter().any(|(entity, window)| {
            entity.id() == request.window && window.swap_chain_texture.is_none()
        })
    {
        let _ = proof.sender.try_send(request.size);
    }
}
