//! 按所选构建方式、工程入口及 Cargo 配置定位并探测工具，保留 Cargo 和 DX 的真实回退边界。

use super::{Probe, Report, first_line};
use crate::{
    native_play::{self, ProjectBuildBackend},
    native_process::resolve_tool,
    project::{Project, display_path},
};
use feathrise_editor_core::ProjectBuildProfile;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

pub(super) struct Tools<'a> {
    cwd: &'a Path,
    dx: bool,
    cargo: Result<OsString, String>,
    path: Option<OsString>,
}

pub(super) fn select<'a>(
    project: &'a Project,
    profile: ProjectBuildProfile,
    backend: ProjectBuildBackend,
    probe: &Probe,
    report: &mut Report,
) -> Tools<'a> {
    let dx = uses_dx(project, profile, backend, probe, report);
    let cwd = if dx {
        project.manifest.parent().unwrap_or(&project.root)
    } else {
        &project.root
    };
    report.directory("build_environment.working_directory", cwd);
    if cwd != project.root {
        report.directory("build_environment.rustup_directory", &project.root);
    }
    let mut rustup = probe.command("rustup", &project.root);
    rustup.args(["show", "active-toolchain"]);
    let active_toolchain = probe
        .output(&rustup)
        .map(|output| first_line(&output).to_owned());
    if active_toolchain.is_ok() || dx {
        // DX 直接通过 rustup 找 Cargo；独立安装的 Cargo 无需 rustup。
        report.tool("rustup", active_toolchain);
    }
    #[cfg(windows)]
    if dx {
        use crate::code_iteration::hotpatch::toolchain::toolchain_path;
        // 原 DX 先从工程根目录选工具链，再从 Cargo 清单所在目录调用 DX。
        let cargo = rustup_tool(project, "cargo", probe);
        let (cargo, path) = match cargo {
            Ok(cargo) => match toolchain_path(&cargo) {
                Ok(path) => (Ok(cargo.into_os_string()), Some(path)),
                Err(error) => (Err(error), None),
            },
            Err(error) => (Err(error), None),
        };
        return Tools {
            cwd,
            dx,
            cargo,
            path,
        };
    }
    Tools {
        cwd,
        dx,
        cargo: Ok(native_play::cargo_program()),
        path: None,
    }
}

impl Tools<'_> {
    #[cfg(windows)]
    pub(super) fn dx(&self) -> bool {
        self.dx
    }

    pub(super) fn command(&self, probe: &Probe, program: impl AsRef<OsStr>) -> Command {
        let mut command = probe.command(program, self.cwd);
        if let Some(path) = &self.path {
            command.env("PATH", path);
        }
        command
    }

    pub(super) fn inspect(&self, project: &Project, probe: &Probe, report: &mut Report) {
        #[cfg(not(windows))]
        let _ = project;
        let cargo = self.cargo.as_ref().map_err(Clone::clone).and_then(|cargo| {
            let mut command = self.command(probe, cargo);
            command.arg("-vV");
            versioned_tool(probe, &command, "cargo ")
        });
        report.tool("Cargo", describe_version(&cargo));

        // 配置加载与路径求值使用 Cargo 同一优先级；不使用会另外执行 rustc 的 cfg 解析 API。
        let options = self.cargo.as_ref().map_or_else(
            |_| cargo_config2::ResolveOptions::default(),
            |program| {
                let path = cargo.as_ref().ok().map_or_else(
                    || program.clone(),
                    |(path, _)| path.clone().into_os_string(),
                );
                cargo_config2::ResolveOptions::default().cargo(path)
            },
        );
        let Ok(config) = cargo_config2::Config::load_with_options(self.cwd, options) else {
            report.unconfirmed(
                "Cargo config",
                probe.locale.text("build_environment.config_failed"),
            );
            return;
        };
        let configured = config.rustc();
        // 包装器（例如 sccache）可能创建服务或缓存；预检只核对它们存在，再直接查询配置的编译器。
        for (label, wrapper) in [
            ("RUSTC_WRAPPER", &config.build.rustc_wrapper),
            (
                "RUSTC_WORKSPACE_WRAPPER",
                &config.build.rustc_workspace_wrapper,
            ),
        ] {
            if let Some(wrapper) = wrapper {
                report.tool(
                    label,
                    resolve_tool(&self.command(probe, wrapper)).map(|path| display_path(&path)),
                );
            }
        }
        let rustc = configured
            .args
            .last()
            .map_or(configured.path.as_os_str(), OsString::as_os_str);
        let mut command = self.command(probe, rustc);
        command.arg("-vV");
        let version = versioned_tool(probe, &command, "rustc ");
        report.tool("rustc", describe_version(&version));
        let Ok((_, version)) = &version else {
            return;
        };
        let host = host_triple(version).or_else(|| {
            cargo
                .as_ref()
                .ok()
                .and_then(|(_, version)| host_triple(version))
        });
        let Ok(mut targets) = config.build_target_for_cli(std::iter::empty::<&str>()) else {
            report.unconfirmed("Cargo target", probe.locale.text("build_environment.config_failed"));
            return;
        };
        if self.dx {
            targets = vec!["x86_64-pc-windows-msvc".into()];
        }
        if targets.is_empty() {
            targets.extend(host.map(str::to_owned));
        }
        if targets.is_empty() {
            report.unconfirmed("Cargo target", probe.locale.text("build_environment.custom_target"));
            return;
        }
        const TARGET_LIMIT: usize = 2;
        if targets.len() > TARGET_LIMIT {
            report.lines.push(probe.locale.format(
                "build_environment.target_limit",
                &[("count", TARGET_LIMIT.to_string())],
            ));
        }
        // 保持最坏输出和子进程数固定；未覆盖的自定义目标在日志中明确标注。
        for target in targets.into_iter().take(TARGET_LIMIT) {
            let mut command = self.command(probe, rustc);
            command.args(["--print", "target-libdir", "--target", &target]);
            let path = probe.output(&command).map(PathBuf::from).and_then(|path| {
                if path.is_dir() {
                    return Ok(display_path(&path));
                }
                Err(probe.locale.format(
                    "build_environment.lib_missing",
                    &[("file", display_path(&path))],
                ))
            });
            report.tool(&format!("Rust std ({target})"), path);
            #[cfg(windows)]
            super::windows::inspect_linker(self, project, probe, report, rustc, host, &target);
            #[cfg(not(windows))]
            inspect_unix_linker(self, probe, report, host, &target);
        }
    }
}

#[cfg(windows)]
pub(super) fn rustup_tool(project: &Project, tool: &str, probe: &Probe) -> Result<PathBuf, String> {
    let mut rustup = probe.command("rustup", &project.root);
    rustup.args(["which", tool]);
    PathBuf::from(probe.output(&rustup)?)
        .canonicalize()
        .map_err(|error| error.to_string())
}

fn host_triple(version: &str) -> Option<&str> {
    version.lines().find_map(|line| line.strip_prefix("host: "))
}

fn describe_version(result: &Result<(PathBuf, String), String>) -> Result<String, String> {
    match result {
        Ok((path, version)) => Ok(format!("{} · {}", first_line(version), display_path(path))),
        Err(error) => Err(error.clone()),
    }
}

fn versioned_tool(
    probe: &Probe,
    command: &Command,
    prefix: &str,
) -> Result<(PathBuf, String), String> {
    let version = probe.output(command)?;
    let path = resolve_tool(command)?;
    if !first_line(&version).starts_with(prefix) {
        return Err(probe.locale.text("build_environment.empty_output").into());
    }
    Ok((path, version))
}

#[cfg(windows)]
fn uses_dx(
    project: &Project,
    profile: ProjectBuildProfile,
    backend: ProjectBuildBackend,
    probe: &Probe,
    report: &mut Report,
) -> bool {
    use crate::code_iteration::hotpatch::toolchain::{self, DxSource};
    let eligible = backend == ProjectBuildBackend::Dx && native_play::supports_dx(profile);
    let entry = eligible.then(|| {
        if !toolchain::rustup_available() {
            return Err(probe.locale.text("build_environment.dx_rustup_missing").into());
        }
        toolchain::select_bin(project)
    });
    if let Some(Err(reason)) = &entry {
        report.lines.push(probe.locale.format(
            "build_environment.dx_fallback",
            &[("reason", reason.clone())],
        ));
    } else if !eligible {
        report.note("build_environment.dx_inactive");
    }
    let active = matches!(entry, Some(Ok(_)));
    let source = match toolchain::dx_source() {
        Ok(source) => source,
        Err(error) => {
            report.tool("DX", Err(error));
            return active;
        }
    };
    let description = match &source {
        DxSource::Explicit(path) => probe.locale.format(
            "build_environment.dx_explicit",
            &[("path", display_path(path))],
        ),
        DxSource::Development(path) => probe.locale.format(
            "build_environment.dx_development",
            &[("path", display_path(path))],
        ),
        DxSource::Bundled => {
            report.tool(
                "DX",
                Ok(probe.locale.text("build_environment.dx_embedded").into()),
            );
            return active;
        }
    };
    let result = match source.existing_file() {
        Ok(Some(path)) => {
            let mut command = probe.command(path, &project.root);
            command.arg("--version");
            probe.output(&command).and_then(|version| {
                let first = first_line(&version);
                if toolchain::supported_dx_version(&version) {
                    return Ok(format!("{description} · {first}"));
                }
                Err(probe.locale.format(
                    "build_environment.unexpected_version",
                    &[("version", first.into())],
                ))
            })
        }
        Ok(None) => {
            let DxSource::Development(path) = source else {
                unreachable!("内嵌来源已报告")
            };
            let missing = probe.locale.format(
                "build_environment.dx_missing",
                &[("path", display_path(&path))],
            );
            report.tool("DX", Err(missing.clone()));
            if active {
                report.lines.push(
                    probe
                        .locale
                        .format("build_environment.dx_fallback", &[("reason", missing)]),
                );
            }
            return false;
        }
        Err(error) => Err(error),
    };
    report.tool(
        "DX",
        result.map_err(|error| format!("{description} · {error}")),
    );
    active
}

#[cfg(not(windows))]
fn uses_dx(
    _project: &Project,
    _profile: ProjectBuildProfile,
    _backend: ProjectBuildBackend,
    _probe: &Probe,
    report: &mut Report,
) -> bool {
    report.note("build_environment.dx_platform");
    false
}

pub(super) enum Linker {
    Explicit(PathBuf),
    Conditional,
    Default,
}

pub(super) fn configured_linker(cwd: &Path, target: &str) -> Result<Linker, ()> {
    use cargo_config2::de::Definition;
    // 显式目标或环境覆盖可只读解析；条件链接器依赖 rustc cfg 与项目 flags，由实际构建确认。
    let target = if target.ends_with(".json") {
        Path::new(target)
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or(target)
    } else {
        target
    };
    let key = format!(
        "CARGO_TARGET_{}_LINKER",
        target.replace('-', "_").to_ascii_uppercase()
    );
    if let Some(linker) = std::env::var_os(key) {
        return Ok(Linker::Explicit(linker.into()));
    }
    let raw = cargo_config2::de::Config::load_with_cwd(cwd).map_err(|_| ())?;
    if let Some(linker) = raw
        .target
        .get(target)
        .and_then(|entry| entry.linker.as_ref())
    {
        let path = PathBuf::from(&linker.val);
        if path.is_relative()
            && linker.val.contains(['/', '\\'])
            && let Some(Definition::Path(file)) = &linker.definition
            && let Some(root) = file.parent().and_then(Path::parent)
        {
            return Ok(Linker::Explicit(root.join(path)));
        }
        return Ok(Linker::Explicit(path));
    }
    let conditional = raw
        .target
        .iter()
        .any(|(key, entry)| key.starts_with("cfg(") && entry.linker.is_some());
    Ok(if conditional {
        Linker::Conditional
    } else {
        Linker::Default
    })
}

#[cfg(not(windows))]
fn inspect_unix_linker(
    tools: &Tools,
    probe: &Probe,
    report: &mut Report,
    host: Option<&str>,
    target: &str,
) {
    let (name, linker) = match configured_linker(tools.cwd, target) {
        Ok(Linker::Explicit(linker)) => ("linker", linker),
        Ok(Linker::Default) if host == Some(target) => (
            probe.locale.text("build_environment.default_linker"),
            "cc".into(),
        ),
        Ok(Linker::Default) => {
            report.unconfirmed(
                "linker",
                probe.locale.text("build_environment.custom_target"),
            );
            return;
        }
        Ok(Linker::Conditional) => {
            report.unconfirmed(
                "linker",
                probe.locale.text("build_environment.conditional_linker"),
            );
            return;
        }
        Err(()) => {
            report.unconfirmed(
                "linker",
                probe.locale.text("build_environment.config_failed"),
            );
            return;
        }
    };
    report.tool(
        name,
        resolve_tool(&tools.command(probe, linker)).map(|path| display_path(&path)),
    );
}
