//! Windows 预检定位配置链接器与常见 MSVC／SDK 工具，沿用 DX 的只读库发现规则。

use super::{
    Probe, Report,
    toolchains::{Linker, Tools, configured_linker, rustup_tool},
};
use crate::{
    code_iteration::hotpatch::toolchain::discover_msvc_libs,
    native_process::resolve_tool,
    project::{Project, display_path},
};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

const LIBRARY_DIRECTORIES: usize = 64;

pub(super) fn inspect_linker(
    tools: &Tools,
    project: &Project,
    probe: &Probe,
    report: &mut Report,
    rustc: &OsStr,
    host: Option<&str>,
    target: &str,
) {
    let target_directory = project.manifest.parent().unwrap_or(&project.root);
    let cwd = if tools.dx() {
        target_directory
    } else {
        &project.root
    };
    let linker = match configured_linker(cwd, target) {
        Ok(linker) => linker,
        Err(()) => {
            report.unconfirmed(
                "linker",
                probe.locale.text("build_environment.config_failed"),
            );
            return;
        }
    };
    let uses_lld = matches!(&linker, Linker::Explicit(path) if bundled_lld(path));
    match linker {
        Linker::Conditional => report.unconfirmed(
            "linker",
            probe.locale.text("build_environment.conditional_linker"),
        ),
        Linker::Explicit(path) if bundled_lld(&path) => {
            let command = if tools.dx() {
                // DX 适配器从工程根独立解析 rustup 的 rustc，不使用 Cargo 配置覆盖的编译器。
                rustup_tool(project, "rustc", probe)
                    .map(|rustc| probe.command(rustc, &project.root))
            } else {
                Ok(tools.command(probe, rustc))
            };
            let sysroot = command.and_then(|mut command| {
                command.args(["--print", "sysroot"]);
                probe.output(&command).map(PathBuf::from)
            });
            // rustc 自带的 lld 与编译器宿主架构配套；DX 的适用组合也是 x64 MSVC。
            let lld = sysroot.and_then(|sysroot| {
                let path = sysroot
                    .join("lib/rustlib")
                    .join(if tools.dx() {
                        target
                    } else {
                        host.unwrap_or(target)
                    })
                    .join("bin/rust-lld.exe");
                if path.is_file() {
                    return Ok(display_path(&path));
                }
                if !tools.dx()
                    && let Ok(found) = resolve_tool(&tools.command(probe, "rust-lld.exe"))
                {
                    return Ok(display_path(&found));
                }
                Err(probe.locale.format(
                    "build_environment.lib_missing",
                    &[("file", display_path(&path))],
                ))
            });
            report.tool("rust-lld", lld);
        }
        Linker::Explicit(path) => {
            report.tool(
                "linker",
                resolve_tool(&tools.command(probe, path)).map(|path| display_path(&path)),
            );
        }
        Linker::Default if target == "x86_64-pc-windows-msvc" => {
            let libraries = discover_msvc_libs(project, |command| probe.output(command));
            let search = libraries.as_deref().map(|paths| {
                paths
                    .split(';')
                    .take(LIBRARY_DIRECTORIES)
                    .map(PathBuf::from)
                    .collect::<Vec<_>>()
            });
            for tool in ["link.exe", "cl.exe"] {
                let path = resolve_tool(&tools.command(probe, tool)).ok().or_else(|| {
                    search
                        .as_deref()
                        .ok()
                        .and_then(|paths| installed_tool(paths, tool))
                });
                if let Some(path) = path {
                    report.tool(tool, Ok(display_path(&path)));
                } else {
                    // Cargo 的 MSVC 发现还可能使用其他安装根，常用路径没有命中不能替代真实链接。
                    report
                        .unconfirmed(tool, probe.locale.text("build_environment.implicit_linker"));
                }
            }
            inspect_libraries(libraries, probe, report, false);
        }
        Linker::Default => report.unconfirmed(
            "linker",
            probe.locale.text("build_environment.custom_target"),
        ),
    }
    if uses_lld && target == "x86_64-pc-windows-msvc" {
        let libraries = discover_msvc_libs(project, |command| probe.output(command));
        inspect_libraries(libraries, probe, report, tools.dx());
    }
}

fn bundled_lld(path: &Path) -> bool {
    path == Path::new("rust-lld") || path == Path::new("rust-lld.exe")
}

fn inspect_libraries(
    libraries: Result<String, String>,
    probe: &Probe,
    report: &mut Report,
    required: bool,
) {
    let label = "MSVC / Windows SDK";
    let paths = match libraries {
        Ok(paths) => paths,
        Err(error) if required => {
            report.tool(label, Err(error));
            return;
        }
        Err(error) => {
            report.unconfirmed(label, error);
            return;
        }
    };
    for library in ["libcmt.lib", "ucrt.lib", "kernel32.lib"] {
        let found = paths
            .split(';')
            .take(LIBRARY_DIRECTORIES)
            .map(|path| Path::new(path.trim_matches('"')).join(library))
            .find(|path| path.is_file());
        match found {
            Some(path) => report.tool(library, Ok(display_path(&path))),
            None if required => report.tool(
                library,
                Err(probe.locale.text("build_environment.lib_search").into()),
            ),
            None => report.unconfirmed(library, probe.locale.text("build_environment.lib_search")),
        }
    }
}

fn installed_tool(libraries: &[PathBuf], tool: &str) -> Option<PathBuf> {
    let x64_libs = libraries
        .iter()
        .filter(|path| path.join("libcmt.lib").is_file());
    for path in x64_libs {
        let Some(msvc) = path.parent().and_then(Path::parent) else {
            continue;
        };
        for host in ["Hostx64", "Hostx86", "Hostarm64"] {
            let candidate = msvc.join("bin").join(host).join("x64").join(tool);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
