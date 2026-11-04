//! 精确拥有当前作者与一个候选实例；仅低频控制跨进程，原作者数据完全留在各自World。
use super::types::{AuthorEvent, HostEvent};
use crate::{project::Project, workspace_transport::Transport};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub(super) struct AuthorProcess {
    pub child: Child,
    pub project: Project,
    pub transport: Transport<AuthorEvent, HostEvent>,
    pub hwnd: Option<usize>,
    pub started: Instant,
    pub model_revision: u64,
    #[cfg(unix)]
    pub surface: crate::portable_author::HostSession,
    #[cfg(unix)]
    pub surface_ready: bool,
}

impl AuthorProcess {
    pub fn spawn(project: Project, target: &Path, parent: usize) -> Result<Self, String> {
        let token = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        #[cfg(unix)]
        let surface = crate::portable_author::HostSession::new(&token)?;
        command
            .args(["--native-editor", "--project"])
            .arg(&project.root)
            .arg("--target-dir")
            .arg(target)
            .env("NEW_EDITOR_MANAGED_INSTANCE", "1")
            .env(
                "FEATHRISE_WORKSPACE_PARENT_PID",
                std::process::id().to_string(),
            )
            .env("FEATHRISE_WORKSPACE_CANDIDATE", "1")
            .env("FEATHRISE_WORKSPACE_TOKEN", &token)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        command.env("FEATHRISE_WORKSPACE_PARENT_HWND", parent.to_string());
        #[cfg(unix)]
        surface.configure(&mut command, parent);
        if let Some(work) = std::env::var_os("FEATHRISE_WORKSPACE_SMOKE_DIR") {
            // 仅显式自有工程有限验收启用原作者/游戏夹具；每个实际实例使用不同报告目录。
            static INSTANCE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let index = INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let work = std::path::PathBuf::from(work);
            command
                .env(
                    "NEW_EDITOR_NATIVE_SMOKE_DIR",
                    work.join(format!("editor-{index}")),
                )
                .env_remove("NEW_EDITOR_NATIVE_SMOKE_SAVE")
                .env_remove("NEW_EDITOR_NATIVE_SMOKE_PLAY")
                .env_remove("NEW_EDITOR_GAME_SMOKE_DIR");
            if index == 1 {
                command
                    .env("NEW_EDITOR_NATIVE_SMOKE_SAVE", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_PLAY", "1")
                    .env("NEW_EDITOR_GAME_SMOKE_DIR", work.join("game-f12"));
            }
        }
        #[cfg(all(feature = "validation", windows))]
        crate::window_chrome::preview_validation::capture_author_stderr(&mut command, &project.root)?;
        let mut child = command
            .spawn()
            .map_err(|e| format!("无法准备工程工作台：{e}"))?;
        let reader = child.stdout.take().ok_or("没有取得作者回执通道")?;
        let writer = child.stdin.take().ok_or("没有取得作者控制通道")?;
        match Transport::new(reader, writer, token) {
            Ok(transport) => Ok(Self {
                child,
                project,
                transport,
                hwnd: None,
                started: Instant::now(),
                model_revision: 0,
                #[cfg(unix)]
                surface,
                #[cfg(unix)]
                surface_ready: false,
            }),
            Err(error) => {
                // 通道未建立、作者仍隐藏且没有用户输入，仅终止刚创建的这一候选。
                let _ = child.kill();
                let _ = child.try_wait();
                Err(error)
            }
        }
    }

    pub fn send(&self, event: HostEvent) -> Result<(), String> {
        self.transport.try_send(event)
    }

    pub fn ready(&self) -> bool {
        #[cfg(windows)]
        { self.hwnd.is_some() }
        #[cfg(unix)]
        { self.surface_ready && self.surface.ready() }
    }

    pub fn activate(&mut self, world: &mut bevy::prelude::World, parent: usize) -> Result<(), String> {
        #[cfg(windows)]
        { let _ = world; crate::workspace_window::activate_child(parent, self.child.id(), self.hwnd.ok_or("候选窗口尚未就绪")?) }
        #[cfg(unix)]
        { let _ = parent; crate::portable_author::host::activate(&mut self.surface, world) }
    }
}

pub(super) fn open_job(
    command: crate::workspace_ui::WorkspaceCommand,
) -> super::resources::OpenJob {
    use crate::workspace_ui::WorkspaceCommand;
    let control = crate::build_process::BuildControl::default();
    let check = control.clone();
    let (sender, result) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = (|| {
            check.check()?;
            let root = match command {
                WorkspaceCommand::Open(root) => root,
                WorkspaceCommand::Create {
                    parent,
                    name,
                    game_view,
                } => {
                    let destination =
                        crate::project_creation::destination(&parent.to_string_lossy(), &name)?;
                    // 默认目录可能尚不存在；只有作者确认创建时才建父目录，保存偏好本身不创建工程。
                    std::fs::create_dir_all(&parent)
                        .map_err(|e| format!("无法创建工程父目录：{e}"))?;
                    let root = crate::project_template::create_empty_with_control(
                        &destination,
                        &name,
                        &crate::project_template_root(),
                        &check,
                        game_view,
                    )?;
                    // 只有显式同窗口验收且本轮独占的刚创建工程才安装中性夹具；普通新建仍为空。
                    #[cfg(all(feature = "validation", windows))]
                    super::validation::prepare_created(&root, &name)?;
                    root
                }
                _ => return Err("不是工程打开请求".into()),
            };
            let project = crate::project::open(&root)?;
            feathrise_project::ProjectMaps::load_with_check(
                &project.context.asset_roots[0],
                || check.check(),
            )?;
            check.check()?;
            Ok(project)
        })();
        let _ = sender.send(result);
    });
    super::resources::OpenJob {
        worker,
        result,
        control,
    }
}

pub(super) fn save_preferences(
    mut preferences: crate::user_preferences::Preferences,
) -> super::resources::PreferencesJob {
    let (sender, result) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = preferences.save().map(|()| preferences);
        let _ = sender.send(result);
    });
    super::resources::PreferencesJob { worker, result }
}
