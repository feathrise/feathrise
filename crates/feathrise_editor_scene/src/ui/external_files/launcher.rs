//! 平台启动适配：受检资产路径、三种IDE的官方参数与无控制台启动。

use super::{messages::OpenExternalFile, types::{DetectedEditors, EditorLauncher, ExternalEditor, ExternalFileAction}};
use std::{ffi::OsString, path::{Component, Path, PathBuf}, process::{Command, Stdio}};

pub(super) fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty() || Path::new(relative).components().any(|part| !matches!(part, Component::Normal(_))) {
        return Err("只能打开 assets 内的资产相对路径".into());
    }
    let root = root.canonicalize().map_err(|error| format!("无法打开 assets 目录：{error}"))?;
    let path = root.join(relative).canonicalize().map_err(|error| format!("无法打开资产：{error}"))?;
    if !path.starts_with(&root) {
        return Err("资产路径超出 assets 目录".into());
    }
    Ok(path)
}

pub(super) fn code_arguments(path: &Path, location: Option<(usize, usize)>) -> Vec<OsString> {
    let path = platform_path(path);
    let mut args = vec![OsString::from("--reuse-window")];
    if let Some((line, column)) = location {
        args.push(OsString::from("--goto"));
        let mut target = path.as_os_str().to_os_string();
        target.push(format!(":{}:{}", line.max(1), column.max(1)));
        args.push(target);
    } else {
        args.push(path.as_os_str().to_os_string());
    }
    args
}

/// 项目与文件作为同一次CLI请求的独立参数，避免先打开目录再打开文件的窗口竞争。
pub(super) fn code_project_arguments(project: &Path, path: &Path, location: Option<(usize, usize)>) -> Vec<OsString> {
    let mut args = code_arguments(path, Some(location.unwrap_or((1, 1))));
    args.insert(1, platform_path(project).into_os_string());
    args
}

pub(crate) fn checked_project(root: &Path) -> Result<PathBuf, String> {
    let project = if let Some(paths) = feathrise_editor_core::editor_project_paths() {
        // 独立工程允许资产目录有多级路径；IDE、源码引用和个人导航仍使用显式工程根。
        let expected = paths.asset_root().canonicalize().map_err(|error| format!("无法定位工程资产目录：{error}"))?;
        if root.canonicalize().map_err(|error| format!("无法定位作者资产目录：{error}"))? != expected {
            return Err("作者资产来源不属于当前编辑工程".into());
        }
        paths.project_root().canonicalize().map_err(|error| format!("无法打开当前工程目录：{error}"))?
    } else {
        // 一体入口仍从原始assets路径取父目录，assets链接不能重定向工作区根。
        root.parent().ok_or_else(|| "无法确定当前工程目录".to_owned())?
            .canonicalize().map_err(|error| format!("无法打开当前工程目录：{error}"))?
    };
    if !project.join("Cargo.toml").is_file() {
        return Err("当前资产目录旁未找到工程的 Cargo.toml，请使用单文件打开".into());
    }
    Ok(project)
}

/// 源码结果的独立边界；不能把../或其他任意项目文件混入既有资产打开入口。
pub(crate) fn is_project_source(path: &str) -> bool {
    !path.is_empty() && path.len() <= 1024 && !path.contains(['\\', ':'])
        && Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
        && (path == "build.rs" || ["src/", "crates/", "apps/"].iter().any(|prefix| path.starts_with(prefix)))
        && Path::new(path).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// 搜索与实际外部打开共用相同的受检读取；只在IO池调用，保留完整磁盘摘要。
pub(crate) fn read_project_source(project: &Path, relative: &str) -> Result<([u8; 32], std::sync::Arc<str>), String> {
    use std::io::Read;
    use sha2::{Digest, Sha256};
    if !is_project_source(relative) { return Err("源码引用必须位于当前工程src、crates或apps目录".into()); }
    let path = checked_path(project, relative)?;
    let file = std::fs::File::open(path).map_err(|error| format!("无法读取源码：{error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 { return Err("源码超过256 KiB引用查询预算".into()); }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(256 * 1024 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > 256 * 1024 { return Err("源码超过256 KiB引用查询预算".into()); }
    let text = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes))
        .map_err(|_| "源码不是UTF-8编码".to_owned())?;
    if text.contains('\0') { return Err("源码包含二进制字符".into()); }
    Ok((Sha256::digest(&bytes).into(), std::sync::Arc::from(text.replace("\r\n", "\n"))))
}

/// Zed使用path:line:column；RustRover把行列作为独立参数，只作用于其后的目标文件。
pub(super) fn editor_arguments(editor: ExternalEditor, project: Option<&Path>, path: &Path, location: Option<(usize, usize)>, directory: bool) -> Vec<OsString> {
    // 目录的项目动作只传当前工作树项目根；不能把目录当文件附加行号，也不把它另加为第二个工作区根。
    let (project, path, location) = if directory { (None, project.unwrap_or(path), None) } else { (project, path, location) };
    if editor == ExternalEditor::VsCode {
        return project.map_or_else(|| code_arguments(path, location), |project| code_project_arguments(project, path, location));
    }
    let location = location.or_else(|| project.map(|_| (1, 1)));
    let mut args = Vec::new();
    if editor == ExternalEditor::Zed { args.push("--existing".into()); } // 兼容本机CLI；不使用新版才有的--reuse。
    if let Some(project) = project { args.push(platform_path(project).into_os_string()); }
    if editor == ExternalEditor::RustRover && let Some((line, column)) = location {
        // JetBrains的行号从1起、字符列从0起；内部和Zed/VS Code保持1起坐标，仅在此转换。
        args.extend(["--line".into(), line.max(1).to_string().into(), "--column".into(), column.saturating_sub(1).to_string().into()]);
    }
    let mut file = platform_path(path).into_os_string();
    if editor == ExternalEditor::Zed && let Some((line, column)) = location {
        file.push(format!(":{}:{}", line.max(1), column.max(1)));
    }
    args.push(file);
    args
}

pub(super) fn editor_command(editor: ExternalEditor, launcher: &EditorLauncher) -> Result<Command, String> {
    if !launcher.executable.is_file() || launcher.prefix_args.iter().any(|path| !Path::new(path).is_file()) {
        return Err(format!("{} 启动文件不存在或不可访问", editor.name()));
    }
    let mut command = Command::new(&launcher.executable);
    command.args(&launcher.prefix_args);
    #[cfg(windows)]
    if editor == ExternalEditor::VsCode {
        command.env("ELECTRON_RUN_AS_NODE", "1").env_remove("VSCODE_DEV");
    }
    Ok(command)
}

/// 入口被IDE更新替换或启动前消失时，在原IO槽内重新检测一次并恢复原请求。
/// 只有尚未创建进程的NotFound可重试；CLI非零退出可能已受理请求，不能重复打开窗口。
pub(super) fn launch_editor_with(
    editor: ExternalEditor, available: &mut DetectedEditors, args: &[OsString],
    refresh: impl FnOnce() -> DetectedEditors, mut execute: impl FnMut(Command) -> std::io::Result<bool>,
) -> Result<(), String> {
    if available.launchers[editor as usize].is_none() {
        return Err(format!("未检测到 {}，请安装后重新打开编辑器；也可以打开所在文件夹", editor.name()));
    }
    let mut attempt = |detected: &DetectedEditors| {
        let launcher = detected.launchers[editor as usize].as_ref().unwrap();
        let mut command = editor_command(editor, launcher)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::NotFound, error))?;
        command.args(args);
        quiet(&mut command);
        execute(command)
    };
    let mut result = attempt(available);
    if result.as_ref().is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
        *available = refresh(); // 完整三项缓存随打开回执采用；卸载后的入口也及时隐藏。
        if available.launchers[editor as usize].is_none() {
            return Err(format!("重新检测后未找到可用的 {} 启动入口，请检查安装；也可以打开所在文件夹", editor.name()));
        }
        result = attempt(available); // 一次请求最多重新检测和重试一次，不增加队列或任务。
    }
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("{} 启动失败，请确认安装正常；也可以打开所在文件夹", editor.name())),
        Err(error) => {
            if error.kind() == std::io::ErrorKind::NotFound { available.launchers[editor as usize] = None; }
            Err(format!("无法启动 {}：{error}", editor.name()))
        }
    }
}

pub(super) fn open(root: &Path, request: &OpenExternalFile, available: &mut DetectedEditors) -> Result<String, String> {
    let path = if let ExternalFileAction::ProjectSource { digest, .. } = request.action {
        let project = checked_project(root)?;
        if read_project_source(&project, &request.path)?.0 != digest {
            return Err("源码在查询后已变化，请重新查看引用再定位".into());
        }
        checked_path(&project, &request.path)?
    } else { checked_path(root, &request.path)? }; // 资产打开仍严格局限于assets；源码使用显式动作。
    match request.action {
        action if action.editor().is_some() => {
            let editor = action.editor().unwrap();
            let location = action.location();
            let in_project = action.in_project();
            if location.is_some() && !path.is_file() {
                return Err("只有文件可以定位到指定行".into());
            }
            let project = in_project.then(|| checked_project(root)).transpose()?;
            let directory = path.is_dir();
            let args = editor_arguments(editor, project.as_deref(), &path, location, directory);
            launch_editor_with(editor, available, &args, super::discovery::discover, |mut command| {
                // 重新检测可能跨越源码更新；实际发给IDE前再次确认原查询摘要，不能定位旧行。
                if let ExternalFileAction::ProjectSource { digest, .. } = action {
                    let project = checked_project(root).map_err(std::io::Error::other)?;
                    if read_project_source(&project, &request.path).map_err(std::io::Error::other)?.0 != digest {
                        return Err(std::io::Error::other("源码在查询后已变化，请重新查看引用再定位"));
                    }
                }
                if editor == ExternalEditor::RustRover {
                    // 原生JetBrains启动器可能承载完整IDE进程；只确认创建，不占槽等待IDE关闭。
                    command.spawn().map(|_| true)
                } else { command.status().map(|status| status.success()) }
            })?;
            if directory && let Some(project) = project {
                return Ok(format!("已请求用 {} 打开当前工程：{}", editor.name(), platform_path(&project).display()));
            }
            Ok(format!("已请求{} {}{}", if in_project { format!("用 {} 打开工程", editor.name()) } else { format!("用 {} 打开", editor.name()) }, request.path,
                if request.has_draft { " · 外部打开的是磁盘版本，内置未保存草稿仍保留" } else { "" }))
        }
        ExternalFileAction::Folder => {
            let folder = if path.is_dir() { path.as_path() } else {
                path.parent().ok_or_else(|| "资产没有所在文件夹".to_owned())?
            };
            let mut command = folder_command(folder);
            quiet(&mut command);
            command.spawn().map_err(|error| format!("无法打开所在文件夹：{error}"))?;
            Ok(format!("已请求打开 {} 的所在文件夹", request.path))
        }
        _ => unreachable!("编辑器动作已按结构化身份处理"),
    }
}

/// 工程文件仅在明确IDE动作下打开；不按系统关联执行源码、脚本或二进制文件。
pub(super) fn open_project_file(root: &Path, request: &OpenExternalFile, available: &mut DetectedEditors) -> Result<String, String> {
    let root = root.canonicalize().map_err(|error| format!("无法核验工程根：{error}"))?;
    if !root.join("Cargo.toml").is_file() { return Err("工程根缺少Cargo.toml，外部打开已取消".into()); }
    let path = checked_path(&root, &request.path)?;
    if matches!(request.action, ExternalFileAction::Folder) {
        let folder = if path.is_dir() { path.as_path() } else { path.parent().ok_or("文件没有父目录")? };
        let mut command = folder_command(folder);
        quiet(&mut command);
        command.spawn().map_err(|error| format!("无法打开所在文件夹：{error}"))?;
        return Ok(format!("已请求打开 {} 的所在文件夹", request.path));
    }
    let editor = request.action.editor().ok_or("请选择明确的外部编辑器")?;
    let directory = path.is_dir();
    if !path.is_file() && !directory { return Err("工程路径不是可打开的文件或目录".into()); }
    if directory && request.action.location().is_some() { return Err("只有文件可以定位到指定行".into()); }
    let project = request.action.in_project().then_some(root.as_path());
    let args = editor_arguments(editor, project, &path, request.action.location(), directory);
    launch_editor_with(editor, available, &args, super::discovery::discover, |mut command| {
        if editor == ExternalEditor::RustRover { command.spawn().map(|_| true) }
        else { command.status().map(|status| status.success()) }
    })?;
    Ok(format!("已请求用 {} 打开 {}{}", editor.name(), if directory && project.is_some() { "当前工程" } else { &request.path },
        if request.has_draft { " · 外部读取磁盘版本，内置草稿保留" } else { "" }))
}

fn quiet(command: &mut Command) {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW：CLI 不闪出黑色控制台。
    }
}

#[cfg(all(windows, test))]
pub(super) fn code_command() -> Result<Command, String> {
    let available = super::discovery::discover();
    let launcher = available.launchers[ExternalEditor::VsCode as usize].as_ref().ok_or_else(|| "未找到 VS Code".to_owned())?;
    editor_command(ExternalEditor::VsCode, launcher)
}

fn folder_command(folder: &Path) -> Command {
    #[cfg(windows)]
    let mut command = Command::new(std::env::var_os("WINDIR")
        .map_or_else(|| PathBuf::from("C:/Windows"), PathBuf::from).join("explorer.exe"));
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    command.arg(platform_path(folder));
    command
}

/// Windows canonicalize 返回扩展路径前缀；外部应用使用普通盘符/UNC写法，Unicode仍原样保留。
pub(crate) fn platform_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(Component::Prefix(prefix)) = path.components().next() {
        use std::path::Prefix;
        let mut result = match prefix.kind() {
            Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", drive as char)),
            Prefix::VerbatimUNC(server, share) => {
                let mut root = OsString::from("\\\\");
                root.push(server); root.push("\\"); root.push(share);
                PathBuf::from(root)
            }
            _ => return path.to_path_buf(),
        };
        for component in path.components().skip(1) {
            if matches!(component, Component::Normal(_)) { result.push(component.as_os_str()); }
        }
        return result;
    }
    path.to_path_buf()
}
