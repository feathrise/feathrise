//! 菜单发起的单飞预检只查询当前工程的工具，完整回执由作者线程采用并写入工程日志。

use crate::{
    build_process::{self, BuildControl},
    native_play::ProjectBuildBackend,
    project::{self, Project},
    project_log::{ProjectLogChannel, ProjectLogState},
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
use feathrise_editor_ui::localization::EditorLocale;
use std::{
    io::Read,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;
mod toolchains;
#[cfg(windows)]
mod windows;

#[derive(Message)]
pub(crate) struct RequestBuildEnvironment;

#[derive(Resource)]
struct ActiveProject(Project);

#[derive(Resource, Default)]
pub(crate) struct BuildEnvironmentState {
    pub(crate) enabled: bool,
    flight: Option<Flight>,
    cleanup_failed: bool,
}

impl BuildEnvironmentState {
    pub(crate) fn checking(&self) -> bool {
        self.flight.is_some()
    }
}

struct Flight {
    project: Project,
    profile: ProjectBuildProfile,
    backend: ProjectBuildBackend,
    control: BuildControl,
    task: Task<Vec<String>>,
}

impl Drop for Flight {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

pub(crate) fn install(app: &mut App, project: Option<&Project>) {
    app.add_message::<RequestBuildEnvironment>()
        .insert_resource(BuildEnvironmentState {
            enabled: project.is_some(),
            ..default()
        });
    if let Some(project) = project {
        app.insert_resource(ActiveProject(project.clone()))
            .add_systems(
                Update,
                update.after(crate::viewport_mode::ViewportModeInput),
            );
    }
}

fn update(
    mut requests: MessageReader<RequestBuildEnvironment>,
    project: Res<ActiveProject>,
    launch: Res<ProjectLaunchSettings>,
    backend: Res<ProjectBuildBackend>,
    mut state: ResMut<BuildEnvironmentState>,
    channel: Option<Res<ProjectLogChannel>>,
    logs: Option<ResMut<ProjectLogState>>,
    locale: Option<Res<EditorLocale>>,
) {
    let requested = requests.read().count() != 0;
    let (Some(channel), Some(mut logs)) = (channel, logs) else {
        return;
    };
    let locale = locale.as_deref().copied().unwrap_or_default();
    if let Some(flight) = state.flight.as_mut() {
        let current = flight.project == project.0
            && flight.profile == launch.profile
            && flight.backend == *backend;
        if !current {
            flight.control.cancel();
        }
        let Some(report) = check_ready(&mut flight.task) else {
            return;
        };
        let cleanup_failed = flight.control.cleanup_failed();
        let cancelled = flight.control.is_cancelled();
        state.flight = None;
        state.cleanup_failed = cleanup_failed;
        if !current || cancelled {
            logs.append_report(
                &channel,
                vec![locale.text("build_environment.stale").into()],
            );
        } else {
            logs.append_report(&channel, report);
        }
        // 检查期间的连点已消费，完成后需要一次新点击才能再次启动。
        return;
    }
    if !requested {
        return;
    }
    logs.show();
    if state.cleanup_failed {
        logs.append_report(
            &channel,
            vec![locale.text("build_environment.cleanup_failed").into()],
        );
        return;
    }
    logs.append_report(
        &channel,
        vec![locale.text("build_environment.checking").into()],
    );
    let control = BuildControl::default();
    let (project, profile, backend) = (project.0.clone(), launch.profile, *backend);
    let task = IoTaskPool::get().spawn({
        let project = project.clone();
        let control = control.clone();
        async move { collect_report(&project, profile, backend, locale, &control) }
    });
    state.flight = Some(Flight {
        project,
        profile,
        backend,
        control,
        task,
    });
}

fn collect_report(
    project: &Project,
    profile: ProjectBuildProfile,
    backend: ProjectBuildBackend,
    locale: EditorLocale,
    control: &BuildControl,
) -> Vec<String> {
    let mut report = Report {
        locale,
        lines: vec![format!("── {} ──", locale.text("build_environment.title"))],
    };
    report.lines.push(locale.format(
        "build_environment.selection",
        &[
            ("profile", profile.label().into()),
            ("backend", locale.editor_label(backend.label()).into()),
        ],
    ));
    match project::open(&project.root) {
        Ok(opened) if opened == *project => {
            let probe = Probe {
                control,
                deadline: Instant::now() + Duration::from_secs(15),
                locale,
            };
            let tools = toolchains::select(project, profile, backend, &probe, &mut report);
            tools.inspect(project, &probe, &mut report);
        }
        _ => report.note("build_environment.project_changed"),
    }
    report.note("build_environment.scope");
    report.lines
}

struct Report {
    locale: EditorLocale,
    lines: Vec<String>,
}

impl Report {
    fn note(&mut self, key: &str) {
        self.lines.push(self.locale.text(key).into());
    }

    fn tool(&mut self, tool: &str, result: Result<String, String>) {
        let (key, details) = match result {
            Ok(details) => ("build_environment.available", details),
            Err(details) => ("build_environment.unavailable", details),
        };
        self.lines.push(
            self.locale
                .format(key, &[("tool", tool.into()), ("details", details)]),
        );
    }

    fn unconfirmed(&mut self, tool: &str, details: impl Into<String>) {
        self.lines.push(self.locale.format(
            "build_environment.unconfirmed",
            &[("tool", tool.into()), ("details", details.into())],
        ));
    }

    fn directory(&mut self, key: &str, directory: &Path) {
        self.lines.push(
            self.locale
                .format(key, &[("path", project::display_path(directory))]),
        );
    }
}

struct Probe<'a> {
    control: &'a BuildControl,
    deadline: Instant,
    locale: EditorLocale,
}

impl Probe<'_> {
    fn command(&self, program: impl AsRef<std::ffi::OsStr>, cwd: &Path) -> Command {
        let mut command = Command::new(program);
        // rustup shim 也继承禁用自动安装的环境。版本／路径查询不进入原构建活动或构建缓存。
        command
            .current_dir(cwd)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env("CARGO_NET_OFFLINE", "true");
        command
    }

    fn output(&self, command: &Command) -> Result<String, String> {
        self.control.check()?;
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(self.locale.text("build_environment.time_budget").into());
        }
        let result = build_process::run(
            command,
            self.control,
            Some(remaining.min(Duration::from_secs(3))),
            |mut file| {
                // 只存前 4 KiB，继续排空本次专属管道，防止工具因输出过量阻塞。
                let mut bytes = Vec::new();
                let mut chunk = [0; 4096];
                loop {
                    let count = file.read(&mut chunk).map_err(|error| error.to_string())?;
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(
                        &chunk[..count.min(4096usize.saturating_sub(bytes.len()))],
                    );
                }
                Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
            },
        )?;
        if result.exit_code == 0 && !result.stdout.is_empty() {
            return Ok(result.stdout);
        }
        if result.exit_code == 0 {
            return Err(self.locale.text("build_environment.empty_output").into());
        }
        Err(self.locale.format(
            "build_environment.exit",
            &[
                ("code", result.exit_code.to_string()),
                ("details", first_line(&result.stderr).into()),
            ],
        ))
    }
}

fn first_line(output: &str) -> &str {
    let line = output
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    let mut end = line.len().min(512);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}
