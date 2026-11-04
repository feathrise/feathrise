//! 对本次后台新建／工程运行的真实失败追加可操作诊断；不预探测工具、不改环境或作者工程。
//!
//! 分类只影响错误文案，不作为运行资格判断。保留退出码和有界原始日志；未知错误交还原诊断。

#[cfg(test)]
mod tests;

use std::{io, path::Path, process::Command};

const LOG_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum CargoStage {
    NewProject,
    Build,
    Play,
}

impl CargoStage {
    fn label(self) -> &'static str {
        match self {
            Self::NewProject => "空工程依赖准备",
            Self::Build => "工程构建",
            Self::Play => "工程运行",
        }
    }
}

/// 仅工具解析／创建失败具有 Cargo 启动提示；取消、收口、超时、日志解析失败原样保留。
pub(crate) fn cargo_start_failure(command: &Command, error: String) -> String {
    if !["找不到原生工具EXE", "工具环境缺少PATH", "子进程可执行文件不存在或不可访问", "CreateProcessW"].iter().any(|marker| error.starts_with(marker)) {
        return error;
    }
    format!(
        "Cargo 未能启动（{}）。请在新终端检查 cargo --version 和 rustup show；未安装时使用 Rust 官方 rustup 安装与工程要求匹配的 MSVC 工具链。已安装时检查 PATH，或修正 EDITOR_HOST_CARGO／CARGO 指向的原生 cargo.exe，然后重启编辑器以继承环境。若原始错误为访问拒绝，请检查该程序权限。\n原始诊断：{error}",
        command.get_program().to_string_lossy(),
    )
}

pub(crate) fn cargo_exit_failure(
    stage: CargoStage,
    command: &Command,
    exit_code: u32,
    stdout: &str,
    stderr: &str,
) -> String {
    let diagnostic = diagnostic_tail(stdout, stderr);
    let offline = command.get_args().any(|arg| arg == "--offline");
    let suggestion = cargo_suggestion(stage, &diagnostic, offline)
        .map(|advice| format!("\n处理建议：{advice}"))
        .unwrap_or_default();
    format!("{}失败（退出码{exit_code}）{suggestion}\n原始诊断：\n{diagnostic}", stage.label())
}

fn cargo_suggestion(stage: CargoStage, diagnostic: &str, offline: bool) -> Option<&'static str> {
    // cargo run 已在前一步构建完成；游戏自己的 stderr 不能作为修复 Rust 环境的依据。
    // 普通 rustc 源码错误也不能因引用行内刚好出现工具错误字符串就建议重装工具链。
    if matches!(stage, CargoStage::Play)
        || ((diagnostic.contains("error[E") || diagnostic.lines().any(|line| line.trim_start().starts_with("--> ")))
            && !diagnostic.contains("can't find crate for `std`"))
    {
        return None;
    }
    let message = diagnostic.to_ascii_lowercase();
    let contains = |patterns: &[&str]| patterns.iter().any(|value| message.contains(value));
    if (contains(&["no default is configured", "no active toolchain", "toolchain is not installed", "toolchain '" ])
        && contains(&["not installed", "no default", "no active", "not found"])
        ) || message.contains("rustup could not choose a version")
    {
        return Some("运行 rustup show 检查当前工具链；按工程的 rust-toolchain.toml 或 Cargo 原始要求安装对应工具链，再重试。没有工程指定版本时，可通过 rustup default <已安装的MSVC工具链名> 明确选择默认工具链。");
    }
    if contains(&["requires rustc ", "rustc is not supported", "rustc version is not supported", "is not supported by the following packages"])
        || (message.contains("could not execute process") && message.contains("rustc") && contains(&["never executed", "not found", "os error 2"]))
        || message.contains("can't find crate for `std`")
    {
        return Some("运行 rustc --version、rustup show 和 rustup target list --installed，核对工程要求的 Rust 版本及 x86_64-pc-windows-msvc 目标；按原始错误补装或更新相应工具链/目标，然后重启编辑器。编辑器不会自动切换工具链或修改工程。");
    }
    if contains(&["linker `rust-lld` not found", "linker `rust-lld.exe` not found"])
        || (message.contains("rust-lld") && message.contains("program not found"))
    {
        return Some("当前工程配置使用 rust-lld。运行 rustc --print sysroot，确认该工具链的 lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe 存在；缺失时修复对应 Rust 工具链。若文件存在，从正确工具链环境启动编辑器或修正工程自己的 linker 配置；不会自动改写 .cargo/config.toml。");
    }
    if contains(&["linker `link.exe` not found", "linker `link` not found", "failed to find tool \"cl.exe\"", "failed to find tool 'cl.exe'"])
        || (contains(&["kernel32.lib", "ucrt.lib", "vcruntime.lib", "windows.h"])
            && contains(&["could not open", "cannot open", "unable to find", "not found", "no such file"]))
    {
        return Some("Windows MSVC 工程构建缺少编译/链接组件或其搜索路径。请在 Visual Studio Installer 中安装“使用 C++ 的桌面开发”，包含 MSVC x64/x86 工具和 Windows SDK；安装后从新的 x64 Native Tools 终端启动编辑器再试。这是 Rust/MSVC 工程的通用构建前提。");
    }
    // Cargo 新版把“需要更新”改写为“cannot update ... because --locked”；
    // 同时匹配锁文件与 locked 原因，不能把普通锁文件 IO 失败误报成依赖不一致。
    if contains(&["lock file needs to be updated", "lockfile needs to be updated", "but --locked was passed"])
        || (message.contains("cannot update the lock file") && message.contains("because --locked was passed"))
    {
        return Some("工程 Cargo.toml 与 Cargo.lock 当前不一致。确认依赖修改符合预期后，在该工程目录显式运行 cargo fetch 更新锁文件并检查差异，再重试预览或启动；编辑器继续使用 --locked，不会替旧工程重写依赖或删除锁文件。");
    }
    if (contains(&["failed to acquire", "failed to lock", "timed out waiting for"])
        && contains(&["lock", "locked"]))
        || (contains(&["os error 32", "os error 33", "being used by another process"])
            && contains(&["failed to", "could not", "unable to"]))
    {
        return Some("构建文件或缓存正被其他进程占用。请等占用该缓存的 Cargo/构建结束，或关闭仍运行的同一工程实例后重试；普通“Blocking waiting for file lock”只是等待信息。不要删除锁文件或强制终止不属于本次任务的进程，编辑器取消仍只收口本次进程树。");
    }
    // 下载、解压也可能因空间或权限失败；先给出实际 IO 原因，避免误导为重新获取依赖。
    if contains(&["no space left on device", "not enough space on the disk", "os error 112", "disk full"])
    {
        return Some("磁盘空间不足。请检查原始错误指向的磁盘，以及 Cargo 下载缓存和构建缓存所在磁盘；释放由你确认可删除的内容，或显式选择已有可写缓存后重试。编辑器不会自动清理缓存。");
    }
    if contains(&["permission denied", "access is denied", "os error 5"])
        && contains(&["failed to create", "failed to write", "could not create", "could not open", "unable to open", "failed to open"])
    {
        return Some("请检查原始错误所指文件的写入权限、只读属性及占用程序；若失败位置属于构建缓存，可用 --target-dir <可写目录> 或 CARGO_TARGET_DIR 显式指定。安装目录不需要开放写权限；工程源码和保存目录仍需作者自己的写权限。");
    }
    // 新旧工程的 SDK 标识均只用于诊断；不据此改写作者的 Cargo 清单。
    if contains(&["feathrise_runtime", "bevy_feathrise", "new_editor_game_runtime"])
        && contains(&["failed to load manifest", "failed to read", "unable to update"])
        && contains(&["no such file", "os error 2", "os error 3", "path not found", "system cannot find"])
    {
        return Some(match stage {
            CargoStage::NewProject => "本地 SDK 源码不完整或原路径已移动。请检查编辑器 sdk-source.toml 的 path 和 patch_path，以及 SDK 源码和补丁仓是否齐全；修正后重试新建。编辑器不会自动换用其他 SDK。",
            _ => "该工程的本地 SDK 路径失效或源码不完整。请检查工程自己的 Cargo.toml 中 SDK 与 [patch.crates-io] 路径，并恢复对应版本的完整源码；路径相对所属 Cargo.toml 解析。编辑器的新建来源配置不会修复旧工程，也不会自动改写其依赖。",
        });
    }
    if offline && contains(&["no matching package named", "attempting to make an http request", "failed to download", "unable to update", "offline mode"])
    {
        return Some(match stage {
            CargoStage::NewProject => "本地 SDK 新建模式使用离线缓存。请由维护者先在对应 SDK 源码目录运行 cargo fetch --locked 补齐锁定依赖，再重试；也可按实际仓库配置允许首次获取的 Git 来源。不要删除种子 Cargo.lock 或用全量升级绕过缺缓存。",
            _ => "当前工程按自己的本地 SDK 依赖采用离线模式。请在该工程目录显式运行 cargo fetch --locked 补齐依赖缓存，再重试预览或启动；编辑器不会擅自联网或升级工程依赖。",
        });
    }
    if contains(&["could not resolve host", "failed to fetch", "failed to download", "failed to authenticate", "authentication required", "certificate verify failed", "network failure"])
        || (message.contains("revision ") && message.contains("not found"))
    {
        return Some("按原始 Cargo 错误检查网络、代理、仓库访问权限及固定提交是否存在；需要凭据时在本机 Cargo/Git 的凭据配置中处理。新建工程检查编辑器的 sdk-source.toml，已有工程检查自己的 Cargo.toml/Cargo.lock；编辑器不会更换来源或回退到其他版本。");
    }
    None
}

pub(crate) fn cache_failure(path: &Path, operation: &str, error: &io::Error) -> String {
    format!(
        "{operation}失败：{}：{error}\n处理建议：确认该路径是可写目录且磁盘有足够空间，检查同名文件和占用；可用 --target-dir <可写目录> 或 CARGO_TARGET_DIR 选择缓存。命令行优先于环境变量，编辑器不会自动搬迁或删除旧缓存。",
        path.display(),
    )
}

pub(crate) fn sdk_configuration_failure(file: &Path, error: String) -> String {
    format!(
        "{error}\nSDK 配置位置：{}。请核对 format=1 和 [source]：kind=\"path\" 时 path 和 patch_path 分别指向 SDK 和补丁仓（相对配置文件目录）；kind=\"git\" 时 git 使用仓库 URL、rev 使用完整 40 位提交 ID，独立补丁仓用 patch_git 和 patch_rev；kind=\"registry\" 时 git 和 rev 指向补丁仓。修正后重试新建；不会改写已创建工程的 Cargo 文件。",
        file.display(),
    )
}

fn tail(text: &str, limit: usize) -> &str {
    let mut start = text.len().saturating_sub(limit);
    while !text.is_char_boundary(start) { start += 1; }
    &text[start..]
}

fn diagnostic_tail(stdout: &str, stderr: &str) -> String {
    // 两条管道共享 16 KiB 预算；两者都有大量输出时各保留尾部，避免下载日志挤掉 rustc 诊断。
    let stdout_reserved = tail(stdout, LOG_BYTES / 2);
    let stderr = tail(stderr, LOG_BYTES - stdout_reserved.len() - 1);
    let stdout = tail(stdout, LOG_BYTES - stderr.len() - 1);
    format!("{stdout}\n{stderr}")
}
