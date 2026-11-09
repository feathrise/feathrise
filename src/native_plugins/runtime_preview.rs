//! 领域预览的通用宿主：只认工程、插件、文档和修订，真实玩法及渲染由工程进程拥有。
//!
//! 作者帧只比较小身份、投递有界请求和移动一张固定尺寸像素；构建、哈希和管道均在后台。
#[path = "runtime_preview_transport.rs"]
mod transport;
#[path = "runtime_preview_worker.rs"]
mod worker;

use crate::{build_process::BuildControl, project::Project};
use bevy::{
    ecs::system::SystemParam,
    input::mouse::{AccumulatedMouseMotion, MouseWheel},
    prelude::*,
};
use feathrise_runtime::project::author_preview::wire;
use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
use feathrise_plugin_api::RuntimePreview;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    pub index: usize,
    pub document: u32,
    pub revision: u64,
    pub kind: String,
}

#[derive(Resource)]
pub(super) struct Settings {
    project: Project,
    target: PathBuf,
}

/// 只有当前修订的真实回传帧才增加frames；握手成功不能冒充技能已施放或画面已显示。
#[derive(Resource, Default, Debug)]
pub(crate) struct Status {
    pub active: bool,
    pub ready: bool,
    pub pid: u32,
    pub session: u64,
    pub revision: u64,
    pub rendered_revision: u64,
    pub frames: u64,
    pub frame_sha256: Option<String>,
    pub nonuniform_pixels: bool,
    /// 来自当前实际渲染帧的通用运行证据；协议后台限制4KiB，宿主不解释领域字段。
    pub runtime_evidence: serde_json::Value,
    pub error: Option<String>,
    pub(super) identity: Option<Identity>,
}

#[derive(Debug)]
pub(super) enum Event {
    Ready(u32),
    Frame {
        revision: u64,
        kind: String,
        pixels: Vec<u8>,
        sha256: String,
        nonuniform: bool,
        evidence: serde_json::Value,
    },
    Error {
        revision: u64,
        kind: String,
        message: String,
    },
}

pub(super) struct Request {
    revision: u64,
    message: &'static str,
    kind: String,
    data: serde_json::Value,
}

struct Active {
    control: BuildControl,
    profile: ProjectBuildProfile,
    identity: Option<Identity>,
    revision: u64,
    ready: bool,
    commands: async_channel::Sender<Request>,
    events: async_channel::Receiver<Event>,
    result: mpsc::Receiver<Result<(), String>>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
pub(super) struct Runtime {
    active: Option<Active>,
    identity: Option<Identity>,
    revision: u64,
    scene: Option<Request>,
    camera: Option<Request>,
    image: Option<Handle<Image>>,
    camera_angles: Vec3,
    exit_requested: bool,
    cleanup_failure: Option<String>,
    failed_identity: Option<Identity>,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.control.cancel();
        }
    }
}

#[derive(Component)]
pub(super) struct RuntimeCanvas;
#[derive(Component)]
pub(super) struct RuntimeCaption(pub usize);

pub(super) fn install(app: &mut App, project: &Project, target: &Path) {
    app.insert_resource(Settings {
        project: project.clone(),
        target: target.to_path_buf(),
    })
    .init_resource::<Status>()
    .init_non_send::<Runtime>()
    .add_systems(Update, camera_input.after(super::systems::preview));
}

/// 原工程关闭事务已经授权后才调用。取消信号不等同于退出，必须继续保留事件循环核对回执。
pub(super) fn ready_to_exit(world: &mut World) -> Result<bool, String> {
    let Some(mut runtime) = world.get_non_send_mut::<Runtime>() else {
        return Ok(true);
    };
    runtime.exit_requested = true;
    if let Some(active) = &runtime.active {
        active.control.cancel();
        return Ok(false);
    }
    if let Some(error) = &runtime.cleanup_failure {
        return Err(error.clone());
    }
    Ok(true)
}

/// 验收只读取真实握手、当前帧和实际画布事实；不写状态，也不能据此代替屏幕截图。
// 与读取这些事实的外部扩展同步编译，扩展测试同样可用。
#[cfg(all(feature = "validation", feathrise_external_validation))]
pub(crate) fn validation_snapshot(world: &mut World) -> serde_json::Value {
    let (Some(status), Some(runtime)) = (
        world.get_resource::<Status>(),
        world.get_non_send::<Runtime>(),
    ) else {
        return serde_json::Value::Null;
    };
    let image = runtime.image.as_ref().map(Handle::id);
    let value = serde_json::json!({
        "active":status.active,"ready":status.ready,"pid":status.pid,"session":status.session,
        "revision":status.revision,"rendered_revision":status.rendered_revision,"frames":status.frames,
        "rgba_sha256":status.frame_sha256,"nonuniform_pixels":status.nonuniform_pixels,"error":status.error,
        "runtime_evidence":status.runtime_evidence,
        "plugin":status.identity.as_ref().map(|id|id.index),"document":status.identity.as_ref().map(|id|id.document),
        "plugin_revision":status.identity.as_ref().map(|id|id.revision),"kind":status.identity.as_ref().map(|id|id.kind.as_str()),
        "cleanup_failed":runtime.cleanup_failure,
    });
    let visible = world
        .query_filtered::<(&ImageNode, &ComputedNode, &InheritedVisibility), With<RuntimeCanvas>>()
        .iter(world)
        .any(|(node, computed, visibility)| {
            Some(node.image.id()) == image && visibility.get() && computed.size().min_element() > 0.
        });
    let mut value = value;
    value["canvas_visible"] = visible.into();
    value
}

/// 验收读取当前正式画布已经采用的GPU像素；领域特定的颜色判断留给对应场景。
#[cfg(all(feature = "validation", feathrise_external_validation))]
pub(crate) fn validation_pixels(world: &World) -> Option<&[u8]> {
    let runtime = world.get_non_send::<Runtime>()?;
    world
        .get_resource::<Assets<Image>>()?
        .get(runtime.image.as_ref()?)?
        .data
        .as_deref()
}

#[derive(SystemParam)]
pub(super) struct Context<'w> {
    runtime: NonSendMut<'w, Runtime>,
    settings: Res<'w, Settings>,
    pub status: ResMut<'w, Status>,
    images: ResMut<'w, Assets<Image>>,
    launch: Res<'w, ProjectLaunchSettings>,
    logs: Option<Res<'w, crate::project_log::ProjectLogChannel>>,
    close: Option<Res<'w, crate::native_project_close::NativeProjectCloseState>>,
}

impl Context<'_> {
    pub(super) fn update(
        &mut self,
        selected: Option<(usize, u32, &RuntimePreview)>,
    ) -> Option<Handle<Image>> {
        // 切文档先撤销采用权，再取回后台消息，同帧迟到的上一页图也没有显示机会。
        self.runtime.select(selected, &mut self.status);
        if !self.close.as_ref().is_some_and(|close| close.active()) {
            self.runtime.exit_requested = false; // 用户取消原关闭事务后可以明确再次发起预览。
        }
        self.runtime.poll(&mut self.status, &mut self.images);
        if let Some(error) = &self.runtime.cleanup_failure {
            self.status.error = Some(error.clone());
        }
        if let Some(active) = &self.runtime.active {
            if active.profile != self.launch.profile {
                active.control.cancel();
            }
        }
        if self.runtime.can_start() {
            self.runtime.start(
                &self.settings,
                self.launch.profile,
                self.logs.as_deref(),
                &mut self.status,
            );
        }
        self.runtime.dispatch();
        (self.status.identity.is_some() && self.status.rendered_revision == self.status.revision)
            .then(|| self.runtime.image.clone())
            .flatten()
    }

    pub(super) fn caption(&self) -> String {
        if let Some(error) = &self.status.error {
            return format!("真实预览失败：{error}");
        }
        if self.status.rendered_revision == self.status.revision && self.status.frames > 0 {
            "工程真实运行预览 · 右键旋转 / 滚轮缩放".into()
        } else if self.status.ready {
            "正在由工程执行技能并准备真实画面…".into()
        } else {
            "正在后台构建并启动工程预览…".into()
        }
    }
}

impl Runtime {
    fn can_start(&self) -> bool {
        self.active.is_none() && self.identity.is_some() && self.cleanup_failure.is_none()
            && !self.exit_requested && self.failed_identity != self.identity
    }

    fn select(&mut self, selected: Option<(usize, u32, &RuntimePreview)>, status: &mut Status) {
        // 中性作者外观只能由本地预览解释，防止后续调用方误把普通查看送入Cargo/工程进程。
        let selected = selected.filter(|(_, _, preview)| !super::local_prefab::is_local(preview));
        let identity = selected.map(|(index, document, preview)| Identity {
            index,
            document,
            revision: preview.revision,
            kind: preview.kind.clone(),
        });
        if identity == self.identity {
            return;
        }
        self.identity = identity.clone();
        self.revision = self.revision.checked_add(1).expect("预览修订不允许回绕");
        self.failed_identity = None;
        self.camera_angles = Vec3::new(0.675, 0.245, 6.6);
        self.camera = None;
        status.identity = identity;
        status.revision = self.revision;
        status.rendered_revision = 0;
        status.frame_sha256 = None;
        status.nonuniform_pixels = false;
        status.runtime_evidence = serde_json::Value::Null;
        status.error = None;
        // 发送前只保留仍有采用权的选择；进入有界队列的preview/clear始终按原序可靠发送。
        // 深拷贝草稿仅发生于显式预览换代，稳定作者帧不会复制JSON正文。
        self.scene = Some(Request {
            revision: self.revision,
            message: if selected.is_some() {
                "preview"
            } else {
                "clear"
            },
            kind: selected.map_or(String::new(), |(_, _, preview)| preview.kind.clone()),
            data: selected.map_or(serde_json::Value::Null, |(_, _, preview)| {
                preview.payload.clone()
            }),
        });
        if selected.is_some() {
            // 同一工程进程会复用上一文档的相机，换身份必须把默认镜头实际发送给它。
            // dispatch始终先发送preview，再发送同revision相机；队列反压不会颠倒这两个边沿。
            let angles = self.camera_angles;
            self.camera = Some(Request {
                revision: self.revision,
                message: "camera",
                kind: String::new(),
                data: serde_json::json!({"yaw":angles.x,"pitch":angles.y,"distance":angles.z}),
            });
        }
    }
    fn start(
        &mut self,
        settings: &Settings,
        profile: ProjectBuildProfile,
        logs: Option<&crate::project_log::ProjectLogChannel>,
        status: &mut Status,
    ) {
        static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
        let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let control = logs
            .map(|logs| BuildControl::with_log(logs.begin_task(crate::project_log::LogTaskKind::AuthorPreview)))
            .unwrap_or_default();
        let (commands, command_rx) = async_channel::bounded(2);
        let (event_tx, events) = async_channel::bounded(2);
        let (result_tx, result) = mpsc::sync_channel(1);
        let project = settings.project.clone();
        let target = settings.target.clone();
        let job_control = control.clone();
        let spawned = thread::Builder::new()
            .name("native-runtime-preview".into())
            .spawn(move || {
                let outcome = worker::run(
                    project,
                    target,
                    profile,
                    session,
                    job_control,
                    command_rx,
                    event_tx,
                );
                let _ = result_tx.send(outcome);
            });
        match spawned {
            Ok(handle) => {
                self.active = Some(Active {
                    control,
                    profile,
                    identity: self.identity.clone(),
                    revision: self.revision,
                    ready: false,
                    commands,
                    events,
                    result,
                    handle,
                });
                status.active = true;
                status.ready = false;
                status.pid = 0;
                status.session = session;
                status.rendered_revision = 0;
                status.error = None;
            }
            Err(error) => {
                status.error = Some(format!("无法创建真实预览后台线程：{error}"));
                self.failed_identity = self.identity.clone();
            }
        }
    }

    fn dispatch(&mut self) {
        let Some(active) = &self.active else {
            return;
        };
        if active.control.is_cancelled() || !active.ready {
            // 构建尚未握手时保留唯一最新草稿，旧构建失败后新身份仍有真实请求可发。
            return;
        }
        if let Some(request) = self.scene.take() {
            if let Err(error) = active.commands.try_send(request) {
                self.scene = Some(error.into_inner());
                return;
            }
        }
        if let Some(request) = self.camera.take() {
            if let Err(error) = active.commands.try_send(request) {
                self.camera = Some(error.into_inner());
            }
        }
    }

    fn poll(&mut self, status: &mut Status, images: &mut Assets<Image>) {
        let Some(active) = &mut self.active else {
            return;
        };
        // 最多两张固定320×320图，长度已在后台协议读线程验证；采用时移动像素而不复制。
        for _ in 0..2 {
            let Ok(event) = active.events.try_recv() else {
                break;
            };
            match event {
                Event::Ready(pid) => {
                    active.ready = true;
                    status.ready = true;
                    status.pid = pid;
                }
                Event::Frame {
                    revision,
                    kind,
                    pixels,
                    sha256,
                    nonuniform,
                    evidence,
                } if accepts(status, revision, &kind) && status.error.is_none() => {
                    let handle = self.image.get_or_insert_with(|| {
                        images.add(Image::new_target_texture(
                            wire::WIDTH,
                            wire::HEIGHT,
                            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                            None,
                        ))
                    });
                    if let Some(mut image) = images.get_mut(handle.id()) {
                        image.data = Some(pixels);
                        status.frames = status.frames.saturating_add(1);
                        status.rendered_revision = revision;
                        status.frame_sha256 = Some(sha256);
                        status.nonuniform_pixels = nonuniform;
                        status.runtime_evidence = evidence;
                        status.error = None;
                    }
                }
                Event::Error {
                    revision,
                    kind,
                    message,
                } if accepts(status, revision, &kind) => {
                    status.error = Some(message);
                    status.rendered_revision = 0;
                    status.runtime_evidence = serde_json::Value::Null;
                }
                _ => (), // 旧文档/旧草稿的图和错误都不能污染当前预览。
            }
        }
        if !active.handle.is_finished() {
            return;
        }
        let active = self.active.take().expect("唯一工作槽已检查");
        let result = active
            .result
            .try_recv()
            .unwrap_or_else(|_| Err("预览线程缺少真实退出回执".into()));
        let joined = active.handle.join().is_ok();
        let stale_build = !active.ready && (active.identity != self.identity || active.revision != self.revision);
        let log = active.control.log_sink();
        status.active = false;
        status.ready = false;
        status.pid = 0;
        status.rendered_revision = 0;
        if active.control.cleanup_failed() || !joined {
            let error = "真实预览的Job或通信线程未确认收口，禁止再次启动".to_string();
            self.cleanup_failure = Some(error.clone());
            status.error = Some(error);
        } else if !active.control.is_cancelled() {
            let error = result.err().unwrap_or_else(|| "工程预览进程意外结束，请再次点击预览".into());
            if let Some(log) = &log { log.task_message(&format!("任务失败：{error}")); }
            if stale_build {
                // 迟到的旧构建仍留在原任务日志；不能把新文档标成已失败并吞掉其待发正文。
                self.failed_identity = None;
            } else {
                status.error = Some(log.map_or(error.clone(), |log| format!("{}：{error}", log.task_label())));
                self.failed_identity = self.identity.clone();
            }
        } else {
            if let Some(log) = &log { log.task_message("任务已停止"); }
            // 配置切换会重建同一份草稿；正文保留在唯一待发槽，不从磁盘伪造未保存内容。
            self.scene = None;
            self.identity = None;
        }
    }
}

fn accepts(status: &Status, revision: u64, kind: &str) -> bool {
    status
        .identity
        .as_ref()
        .is_some_and(|identity| identity.kind == kind)
        && status.revision == revision
}

fn camera_input(
    mut runtime: NonSendMut<Runtime>,
    status: Res<Status>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    canvases: Query<(&Node, &bevy::ui::RelativeCursorPosition), With<RuntimeCanvas>>,
    mut dragging: Local<Option<u64>>,
) {
    let scroll: f32 = wheel
        .read()
        .map(|event| match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => event.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => event.y / 40.,
        })
        .sum();
    let over = status.identity.is_some()
        && canvases
            .iter()
            .any(|(node, cursor)| node.display != Display::None && cursor.cursor_over());
    if dragging.is_some_and(|revision| revision != status.revision) {
        *dragging = None; // 按住右键切文档也必须重新按下，旧文档手势没有新镜头的修改权。
    }
    if mouse.just_pressed(MouseButton::Right) {
        *dragging = over.then_some(status.revision);
    }
    if !mouse.pressed(MouseButton::Right) || status.identity.is_none() {
        *dragging = None;
    }
    let turn = *dragging == Some(status.revision) && motion.delta != Vec2::ZERO;
    if !status.ready || (!turn && !(over && scroll != 0.)) {
        return;
    }
    if turn {
        runtime.camera_angles.x -= motion.delta.x * 0.008;
        runtime.camera_angles.y =
            (runtime.camera_angles.y + motion.delta.y * 0.008).clamp(-1.5, 1.5);
    }
    if over {
        runtime.camera_angles.z =
            (runtime.camera_angles.z * (-scroll * 0.12).exp()).clamp(0.2, 100.);
    }
    let angle = runtime.camera_angles;
    runtime.camera = Some(Request {
        revision: status.revision,
        message: "camera",
        kind: String::new(),
        data: serde_json::json!({"yaw":angle.x,"pitch":angle.y,"distance":angle.z}),
    });
    runtime.dispatch();
}

#[cfg(test)]
#[path = "runtime_preview_tests.rs"]
mod tests;
