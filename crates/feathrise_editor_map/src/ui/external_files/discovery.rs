//! 后台检测三种编辑器的真实启动入口；只查PATH、标准目录与有界安装记录，不扫描磁盘。
use super::{constants::MAX_PATH_BYTES, types::{DetectedEditors, EditorLauncher, ExternalEditor}};
use std::path::{Path, PathBuf};

fn bounded(path: &Path) -> bool { path.as_os_str().len() <= MAX_PATH_BYTES }

#[cfg(windows)]
mod windows;

pub(super) fn discover() -> DetectedEditors {
    let paths: Vec<_> = std::env::var_os("PATH").map(|value| std::env::split_paths(&value).take(128).collect()).unwrap_or_default();
    #[cfg(windows)]
    {
        let mut homes = Vec::new();
        for (variable, suffix) in [("LOCALAPPDATA", "Programs"), ("ProgramFiles", ""), ("ProgramFiles(x86)", "")] {
            if let Some(path) = std::env::var_os(variable) { homes.push(PathBuf::from(path).join(suffix)); }
        }
        let mut installed = windows::registered_installations();
        for home in homes {
            installed.extend([(ExternalEditor::VsCode, home.join("Microsoft VS Code")),
                (ExternalEditor::Zed, home.join("Zed")), (ExternalEditor::RustRover, home.join("RustRover"))]);
            collect_rustrover(&home.join("JetBrains"), &mut installed);
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            collect_rustrover(&local.join("JetBrains/Toolbox/apps"), &mut installed);
            collect_rustrover(&local.join("JetBrains"), &mut installed);
        }
        discover_in(&paths, &installed)
    }
    #[cfg(not(windows))]
    { discover_in(&paths, &[]) }
}

/// 固定最多128个PATH入口；测试可注入目录，避免修改进程环境或启动任何IDE。
pub(super) fn discover_in(paths: &[PathBuf], installed: &[(ExternalEditor, PathBuf)]) -> DetectedEditors {
    let mut result = DetectedEditors::default();
    for editor in ExternalEditor::ALL {
        let candidates = installed.iter().filter(|(kind, path)| *kind == editor && bounded(path)).map(|(_, path)| path.clone())
            .chain(paths.iter().take(128).filter(|path| bounded(path)).cloned());
        result.launchers[editor as usize] = candidates.filter_map(|path| find(editor, &path)).next();
    }
    result
}

fn find(editor: ExternalEditor, directory: &Path) -> Option<EditorLauncher> {
    if !bounded(directory) { return None; }
    #[cfg(windows)]
    {
        let names: &[&str] = match editor {
            ExternalEditor::VsCode => &["Code.exe", "../Code.exe"],
            ExternalEditor::Zed => &["bin/zed.exe", "zed.exe"],
            ExternalEditor::RustRover => &["bin/rustrover64.exe", "bin/rustrover.exe", "rustrover64.exe", "rustrover.exe"],
        };
        for name in names {
            // Windows安装根的zed.exe是常驻GUI；只接独立bin CLI，不能等待GUI退出占住唯一IO槽。
            if editor == ExternalEditor::Zed && *name == "zed.exe"
                && !directory.file_name().is_some_and(|name| name.as_encoded_bytes().eq_ignore_ascii_case(b"bin")) { continue; }
            let executable = directory.join(name);
            if !bounded(&executable) || !executable.is_file() { continue; }
            let prefix_args = if editor == ExternalEditor::VsCode {
                let home = executable.parent()?;
                let fixed = home.join("resources/app/out/cli.js");
                let script = if bounded(&fixed) && fixed.is_file() { Some(fixed) } else {
                    std::fs::read_dir(home).ok().into_iter().flatten().take(32).filter_map(Result::ok)
                        .map(|entry| entry.path().join("resources/app/out/cli.js")).find(|path| bounded(path) && path.is_file())
                };
                let Some(script) = script else { continue; };
                vec![script.into_os_string()]
            } else { Vec::new() };
            return Some(EditorLauncher { executable, prefix_args });
        }
        None
    }
    #[cfg(not(windows))]
    {
        let name = match editor { ExternalEditor::VsCode => "code", ExternalEditor::Zed => "zed", ExternalEditor::RustRover => "rustrover" };
        let executable = directory.join(name);
        use std::os::unix::fs::PermissionsExt;
        if !bounded(&executable) { return None; }
        executable.metadata().ok().filter(|info| info.is_file() && info.permissions().mode() & 0o111 != 0)
            .map(|_| EditorLauncher { executable, prefix_args: Vec::new() })
    }
}

#[cfg(windows)]
fn collect_rustrover(home: &Path, installed: &mut Vec<(ExternalEditor, PathBuf)>) {
    if !bounded(home) { return; }
    let Ok(entries) = std::fs::read_dir(home) else { return; };
    let roots: Vec<_> = entries.take(64).filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase().starts_with("rustrover"))
        .map(|entry| entry.path()).filter(|path| bounded(path)).collect();
    // Toolbox版本层级最多三层、总计128个目录；不递归其他产品或跟随目录链接。
    let mut pending: std::collections::VecDeque<_> = roots.into_iter().map(|path| (path, 0)).collect();
    let mut count = 0;
    while let Some((path, depth)) = pending.pop_front() {
        count += 1;
        if count > 128 { break; }
        if !path.symlink_metadata().is_ok_and(|info| {
            use std::os::windows::fs::MetadataExt;
            info.is_dir() && info.file_attributes() & 0x400 == 0
        }) { continue; }
        installed.push((ExternalEditor::RustRover, path.clone()));
        if depth < 3 && let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.take(32).filter_map(Result::ok) {
                if bounded(&entry.path()) && entry.file_type().is_ok_and(|kind| kind.is_dir()) && pending.len() < 128 {
                    pending.push_back((entry.path(), depth + 1));
                }
            }
        }
    }
}
