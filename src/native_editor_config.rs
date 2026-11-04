//! 原入口的设备、窗口和任务池装配参数；独立编辑器保持生产渲染能力及现有配置。

use bevy::{
    app::{PluginGroupBuilder, TaskPoolThreadAssignmentPolicy},
    asset::AssetPlugin,
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::WgpuFeatures,
        settings::{RenderCreation, WgpuSettings},
    },
    tasks::available_parallelism,
    window::{MonitorSelection, PresentMode, WindowMode, WindowPosition, WindowResolution},
};
use feathrise_runtime::game_plugins::{StartupDisplayPreferences, StartupWindowPolicy};

fn pool_policy(threads: usize) -> TaskPoolThreadAssignmentPolicy {
    TaskPoolThreadAssignmentPolicy {
        min_threads: threads,
        max_threads: threads,
        percent: 1.0,
        on_thread_spawn: None,
        on_thread_destroy: None,
    }
}

fn task_pool_options() -> TaskPoolOptions {
    let threads = available_parallelism().max(1);
    // 与原入口相同：IO最多四核，剩余算力在异步准备与ECS执行间对半分配。
    let io = (threads.saturating_add(2) / 4).clamp(1, 4);
    let asynchronous = (threads.saturating_sub(io) / 2).max(1);
    let compute = threads
        .saturating_sub(io)
        .saturating_sub(asynchronous)
        .max(1);
    TaskPoolOptions {
        min_total_threads: threads,
        max_total_threads: threads,
        io: pool_policy(io),
        async_compute: pool_policy(asynchronous),
        compute: pool_policy(compute),
    }
}

fn render_settings() -> WgpuSettings {
    let mut settings = WgpuSettings::default();
    settings.backends = Some(crate::portable_author::backend());
    #[cfg(all(
        debug_assertions,
        target_os = "windows",
        any(
            feature = "raytracing",
            feature = "rtx_rendering",
            feature = "fsr3_upscaling",
            feature = "fsr3_frame_generation"
        )
    ))]
    if std::env::var_os("WGPU_VALIDATION").is_none() {
        // 与原生产入口保持一致：Bevy0.19 Windows Vulkan首Present问题的已有驱动规避。
        settings
            .instance_flags
            .remove(bevy::render::settings::InstanceFlags::VALIDATION);
    }
    let wireframe = WgpuFeatures::POLYGON_MODE_LINE | WgpuFeatures::IMMEDIATES;
    settings.features |= wireframe;
    if let Some(disabled) = settings.disabled_features.as_mut() {
        disabled.remove(wireframe);
    }
    settings
}

pub(super) fn default_plugins(
    name: &str,
    policy: StartupWindowPolicy,
    display: StartupDisplayPreferences,
    assets: AssetPlugin,
) -> PluginGroupBuilder {
    let mode = if !policy.force_windowed && display.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
    } else {
        WindowMode::Windowed
    };
    let plugins = DefaultPlugins
        // 首版不启用原生代码热补丁；验证原型仍保留自己的hotpatch路径。
        .build()
        .disable::<bevy::app::hotpatch::HotPatchPlugin>()
        .set(TaskPoolPlugin {
            task_pool_options: task_pool_options(),
        })
        .set(assets)
        .set(WindowPlugin {
            // 原作者草稿和后台工程运行由NativePlayPlugin统一收口，系统关闭请求不能直接销毁World。
            close_when_requested: false,
            primary_window: Some(Window {
                title: format!("{name} · Feathrise（羽立）"),
                // 候选不显示独立顶层窗口、不抢焦点；父窗口提交后才显示内部作者子窗口。
                visible: !embedded_author(),
                focused: !embedded_author(),
                decorations: !cfg!(target_os = "windows") && !embedded_author(),
                resolution: WindowResolution::new(1270, 720),
                position: WindowPosition::Centered(MonitorSelection::Primary),
                mode,
                present_mode: if display.vsync {
                    PresentMode::AutoVsync
                } else {
                    PresentMode::AutoNoVsync
                },
                ..default()
            }),
            ..default()
        })
        .set(RenderPlugin {
            render_creation: RenderCreation::Automatic(Box::new(render_settings())),
            ..default()
        });
    #[cfg(unix)]
    if embedded_author() {
        // Unix不创建第二个作者原生窗口：原Window组件继续供布局/拾取使用，专用帧通道负责呈现。
        // 输入与唤醒均来自主宿主，固定16ms调度保证等待GPU/关闭事务时事件循环仍能推进。
        return plugins.disable::<bevy::winit::WinitPlugin>()
            .add(bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(16)));
    }
    plugins
}

fn embedded_author() -> bool {
    std::env::var_os("FEATHRISE_WORKSPACE_PARENT_HWND").is_some()
        || std::env::var_os("FEATHRISE_WORKSPACE_SESSION").is_some()
}
