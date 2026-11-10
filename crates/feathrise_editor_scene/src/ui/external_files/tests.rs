//! 外部打开的路径边界、原样参数与单槽合并回归；普通测试不启动用户的应用。

use super::{launcher, messages::OpenExternalFile, resources::ExternalFileState, systems, types::ExternalFileAction};
use bevy::{prelude::*, tasks::IoTaskPool};
use std::{path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
use super::{discovery, resources::ExternalEditors, types::{DetectedEditors, EditorLauncher, ExternalEditor, ExternalTaskReply}};

static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Resource, Default)]
pub(crate) struct ExternalRequestProbe(pub Vec<OpenExternalFile>);
pub(crate) fn install_probe(app: &mut App) {
    app.init_resource::<ExternalRequestProbe>().add_systems(Update, collect.after(crate::SceneEditorSystemSet::Input));
}
fn collect(mut requests: MessageReader<OpenExternalFile>, mut probe: ResMut<ExternalRequestProbe>) {
    probe.0.extend(requests.read().cloned()); // GPU验证确认真实菜单意图，避免启动用户正在使用的外部应用。
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("rpg-external-open-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(root.join("中文 空格")).unwrap();
        std::fs::write(root.join("中文 空格/配置.json"), "{}").unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

#[test]
fn external_open_rejects_absolute_parent_missing_and_accepts_real_file_and_folder() {
    let fixture = Fixture::new();
    assert!(launcher::checked_path(&fixture.0, "中文 空格/配置.json").unwrap().is_file());
    assert!(launcher::checked_path(&fixture.0, "中文 空格").unwrap().is_dir());
    for path in ["", "../outside.json", "missing.json"] {
        assert!(launcher::checked_path(&fixture.0, path).is_err());
    }
    assert!(launcher::checked_path(&fixture.0, fixture.0.to_str().unwrap()).is_err());
}

#[test]
fn external_vscode_arguments_keep_unicode_spaces_and_command_characters_in_one_argument() {
    let path = PathBuf::from("D:/assets/中文 空格/$name&(word)'配置.json");
    let args = launcher::code_arguments(&path, Some((17, 3)));
    assert_eq!(args.len(), 3);
    assert_eq!(args[0], "--reuse-window");
    assert_eq!(args[1], "--goto");
    let mut expected = path.into_os_string(); expected.push(":17:3");
    assert_eq!(args[2], expected); // 无Shell引号、替换或字符串命令解析。
    assert_eq!(launcher::code_arguments(std::path::Path::new("plain.json"), None).len(), 2);
}

#[test]
fn external_vscode_project_arguments_keep_the_workspace_and_file_in_one_request() {
    let project = PathBuf::from("D:/worktrees/当前 分支&$项目");
    let path = project.join("assets/中文 空格/配置.json");
    let args = launcher::code_project_arguments(&project, &path, Some((17, 3)));
    let mut expected = path.as_os_str().to_os_string(); expected.push(":17:3");
    assert_eq!(args, vec!["--reuse-window".into(), project.into_os_string(), "--goto".into(), expected]);
    let args = launcher::code_project_arguments(path.parent().unwrap(), &path, None);
    assert!(args[3].to_str().unwrap().ends_with(":1:1")); // 菜单没有行位时仍明确打开文件首行。
}

#[test]
fn external_vscode_project_uses_the_asset_roots_workspace_and_rejects_nonprojects() {
    let fixture = Fixture::new();
    let project = fixture.0.join("当前 分支");
    let assets = project.join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[workspace]\n").unwrap();
    assert_eq!(launcher::checked_project(&assets).unwrap(), project.canonicalize().unwrap());
    assert!(launcher::checked_project(&fixture.0.join("中文 空格")).is_err());
    assert!(launcher::checked_project(std::path::Path::new("")).is_err()); // 不能猜测main或当前进程目录。
}

#[test]
fn external_source_reference_uses_project_scope_and_rejects_changed_files() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.0.join("assets")).unwrap();
    std::fs::create_dir_all(fixture.0.join("crates/中文/src")).unwrap();
    std::fs::write(fixture.0.join("Cargo.toml"), "[workspace]\n").unwrap();
    let path = "crates/中文/src/player.rs";
    std::fs::write(fixture.0.join(path), "\u{feff}// 主角\r\nconst BODY: &str = \"gltf/蓝衣.glb\";\r\n").unwrap();
    let (digest, source) = launcher::read_project_source(&fixture.0, path).unwrap();
    assert!(source.starts_with("// 主角\n"));
    for (ide, in_project) in ExternalEditor::ALL.into_iter().flat_map(|ide| [false, true].map(|in_project| (ide, in_project))) {
        let request = OpenExternalFile { path: path.into(), has_draft: false,
            action: ExternalFileAction::ProjectSource { editor: ide, line: 2, column: 20, digest, in_project } };
        assert_eq!(request.action.in_project(), in_project);
        assert_eq!(request.action.location(), Some((2, 20)));
        assert!(launcher::open(&fixture.0.join("assets"), &request, &mut DetectedEditors::default()).unwrap_err().contains("未检测到"));
    }
    std::fs::write(fixture.0.join(path), "const BODY: &str = \"other.glb\";\n").unwrap();
    for in_project in [false, true] {
        let request = OpenExternalFile { path: path.into(), has_draft: false,
            action: ExternalFileAction::ProjectSource { editor: ExternalEditor::VsCode, line: 2, column: 20, digest, in_project } };
        assert!(launcher::open(&fixture.0.join("assets"), &request, &mut DetectedEditors::default()).unwrap_err().contains("已变化"));
    }
    for invalid in ["../crates/中文/src/player.rs", "crates/../target/player.rs", "assets/player.rs", "Cargo.toml", "/src/player.rs", "C:/src/player.rs"] {
        assert!(launcher::read_project_source(&fixture.0, invalid).is_err());
    }
    assert!(launcher::checked_path(&fixture.0.join("assets"), "../crates/中文/src/player.rs").is_err());
}

#[cfg(windows)]
#[test]
fn external_vscode_removes_only_windows_extended_prefix_preserving_disk_and_unc() {
    assert_eq!(launcher::code_arguments(std::path::Path::new(r"\\?\D:\assets\说明.json"), Some((0, 0)))[2], r"D:\assets\说明.json:1:1");
    assert_eq!(launcher::code_arguments(std::path::Path::new(r"\\?\UNC\server\share\说明.json"), None)[1], r"\\server\share\说明.json");
    let args = launcher::code_project_arguments(std::path::Path::new(r"\\?\D:\当前 项目"), std::path::Path::new(r"\\?\D:\当前 项目\assets\说明.json"), None);
    assert_eq!(args[1], r"D:\当前 项目");
    assert_eq!(args[3], r"D:\当前 项目\assets\说明.json:1:1");
}

#[test]
fn external_open_coalesces_requests_without_replacing_a_running_task() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).init_resource::<crate::resources::SceneEditorState>()
        .init_resource::<ExternalFileState>().init_resource::<super::resources::ExternalEditors>().add_message::<OpenExternalFile>()
        .add_message::<super::messages::OpenExternalProjectFile>()
        .add_systems(Update, systems::drive);
    app.world_mut().resource_mut::<ExternalFileState>().task = Some(IoTaskPool::get().spawn(std::future::pending()));
    for path in ["first.json", "latest.json"] {
        app.world_mut().write_message(OpenExternalFile { path: path.into(), action: ExternalFileAction::VsCode(None), has_draft: true });
    }
    app.update();
    let state = app.world().resource::<ExternalFileState>();
    assert!(state.task.is_some());
    assert_eq!(state.pending.as_ref().unwrap().path, "latest.json");
    assert!(state.pending.as_ref().unwrap().has_draft);
}

#[test]
fn external_zed_and_rustrover_keep_workspace_unicode_and_line_arguments_separate() {
    let project = PathBuf::from("D:/worktrees/中文 项目&$分支");
    let file = project.join("assets/配置.json");
    let mut positioned = file.as_os_str().to_os_string(); positioned.push(":17:3");
    assert_eq!(launcher::editor_arguments(ExternalEditor::Zed, Some(&project), &file, Some((17, 3)), false),
        vec!["--existing".into(), project.as_os_str().to_os_string(), positioned]);
    assert_eq!(launcher::editor_arguments(ExternalEditor::RustRover, Some(&project), &file, Some((17, 3)), false),
        vec![project.as_os_str().to_os_string(), "--line".into(), "17".into(), "--column".into(), "2".into(), file.as_os_str().to_os_string()]);
    assert_eq!(launcher::editor_arguments(ExternalEditor::RustRover, None, &file, None, false), vec![file.as_os_str().to_os_string()]);
    assert_eq!(launcher::editor_arguments(ExternalEditor::Zed, None, &file, None, false), vec!["--existing".into(), file.as_os_str().to_os_string()]);
    assert_eq!(launcher::editor_arguments(ExternalEditor::RustRover, Some(&project), &file, None, false)[2], "1");
    assert_eq!(launcher::editor_arguments(ExternalEditor::RustRover, Some(&project), &file, None, false)[4], "0");
    assert_eq!(launcher::editor_arguments(ExternalEditor::RustRover, None, &file, Some((0, 0)), false)[1], "1");
    assert_eq!(launcher::editor_arguments(ExternalEditor::RustRover, None, &file, Some((0, 0)), false)[3], "0");
}

#[test]
fn external_directory_project_arguments_open_only_the_current_workspace_without_file_locations() {
    let project = PathBuf::from("D:/worktrees/当前 中文项目&$分支");
    let folder = project.join("assets/中文 目录");
    for (editor, prefix) in [(ExternalEditor::VsCode, Some("--reuse-window")),
        (ExternalEditor::Zed, Some("--existing")), (ExternalEditor::RustRover, None)] {
        let mut expected: Vec<std::ffi::OsString> = prefix.into_iter().map(Into::into).collect();
        expected.push(project.as_os_str().to_os_string());
        assert_eq!(launcher::editor_arguments(editor, Some(&project), &folder, None, true), expected);
        *expected.last_mut().unwrap() = folder.as_os_str().to_os_string();
        assert_eq!(launcher::editor_arguments(editor, None, &folder, None, true), expected); // 原目录直接打开仍打开所选目录。
    }
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("Cargo.toml"), "[workspace]\n").unwrap();
    std::fs::create_dir_all(fixture.0.join("assets/中文 空格")).unwrap();
    for action in [ExternalFileAction::VsCodeProject(None), ExternalFileAction::ZedProject(None), ExternalFileAction::RustRoverProject(None)] {
        let request = OpenExternalFile { path: "中文 空格".into(), action, has_draft: false };
        let error = launcher::open(&fixture.0.join("assets"), &request, &mut DetectedEditors::default()).unwrap_err();
        assert!(error.contains("未检测到")); // 目录项目请求通过受检路径和项目校验，停在安装校验而非误报文件行定位。
    }
    for action in [ExternalFileAction::VsCodeProject(Some((3, 1))), ExternalFileAction::ZedProject(Some((3, 1))), ExternalFileAction::RustRoverProject(Some((3, 1)))] {
        let request = OpenExternalFile { path: "中文 空格".into(), action, has_draft: false };
        assert_eq!(launcher::open(&fixture.0.join("assets"), &request, &mut DetectedEditors::default()).unwrap_err(), "只有文件可以定位到指定行");
    } // 显式文件行号仍不能用于目录；整个测试不启动用户的IDE。
}

#[cfg(windows)]
#[test]
fn external_discovery_requires_real_launchers_and_accepts_custom_install_directories() {
    let fixture = Fixture::new();
    let code = fixture.0.join("自选 目录/Code");
    let zed = fixture.0.join("自选 目录/Zed");
    let rover = fixture.0.join("自选 目录/RustRover");
    for path in [&code, &zed, &rover] { std::fs::create_dir_all(path.join("bin")).unwrap(); }
    std::fs::write(code.join("Code.exe"), []).unwrap();
    std::fs::write(zed.join("zed.exe"), []).unwrap(); // 仅有GUI残留不能当作独立CLI等待退出。
    let installs = vec![(ExternalEditor::VsCode, code.clone()), (ExternalEditor::Zed, zed.clone()), (ExternalEditor::RustRover, rover.clone())];
    assert!(discovery::discover_in(&[], &installs).launchers.iter().all(Option::is_none)); // 单独残留的Code.exe不能伪装成可用CLI。
    std::fs::create_dir_all(code.join("版本/resources/app/out")).unwrap();
    std::fs::write(code.join("版本/resources/app/out/cli.js"), []).unwrap();
    std::fs::write(zed.join("bin/zed.exe"), []).unwrap();
    std::fs::write(rover.join("bin/rustrover64.exe"), []).unwrap();
    let detected = discovery::discover_in(&[], &installs);
    assert!(detected.launchers.iter().all(Option::is_some));
    assert_eq!(detected.launchers[ExternalEditor::Zed as usize].as_ref().unwrap().executable, zed.join("bin/zed.exe"));
    let from_path = discovery::discover_in(&[zed.join("bin"), rover.join("bin"), code.join("bin")], &[]);
    assert!(from_path.launchers.iter().all(Option::is_some));
    std::fs::remove_file(rover.join("bin/rustrover64.exe")).unwrap();
    assert!(discovery::discover_in(&[], &installs).launchers[ExternalEditor::RustRover as usize].is_none());
}

#[test]
fn external_installed_menu_only_includes_cached_editors_without_empty_rows() {
    let empty = ExternalEditors::default();
    assert_eq!(empty.menu_size(true, 2, 144.0), Vec2::new(144.0, 86.0)); // 无IDE仍有两种复制和文件夹。
    assert_eq!(empty.menu_size(true, 5, 168.0), Vec2::new(168.0, 164.0));
    let zed = ExternalEditors::for_tests(&[ExternalEditor::Zed]);
    assert!(zed.visible(ExternalFileAction::Zed(None), true));
    assert!(zed.visible(ExternalFileAction::ZedProject(None), true));
    assert!(!zed.visible(ExternalFileAction::VsCode(None), true));
    assert!(!zed.visible(ExternalFileAction::RustRover(None), true));
    assert!(zed.visible(ExternalFileAction::ZedProject(None), false));
    assert!(!zed.visible(ExternalFileAction::ZedProject(Some((3, 1))), false));
    assert_eq!(zed.menu_size(true, 2, 144.0), Vec2::new(240.0, 138.0));
    assert_eq!(zed.menu_size(false, 2, 144.0), Vec2::new(240.0, 138.0)); // 目录同样有当前项目入口。
    let all = ExternalEditors::for_tests(&ExternalEditor::ALL);
    assert_eq!(all.menu_size(true, 2, 144.0), Vec2::new(240.0, 242.0));
    assert_eq!(all.menu_size(false, 2, 144.0), Vec2::new(240.0, 242.0));
    assert_eq!(all.menu_size(true, 5, 168.0), Vec2::new(240.0, 320.0));
}

#[test]
fn external_missing_editor_rejects_a_stale_request_without_opening_an_application() {
    let fixture = Fixture::new();
    for action in [ExternalFileAction::VsCode(None), ExternalFileAction::Zed(None), ExternalFileAction::RustRover(None)] {
        let request = OpenExternalFile { path: "中文 空格/配置.json".into(), action, has_draft: true };
        assert!(launcher::open(&fixture.0, &request, &mut DetectedEditors::default()).unwrap_err().contains("未检测到"));
    }
}

#[test]
fn external_detection_is_cached_and_reopening_does_not_replace_an_inflight_task() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).init_resource::<crate::resources::SceneEditorState>()
        .init_resource::<ExternalFileState>().init_resource::<ExternalEditors>().add_message::<OpenExternalFile>()
        .add_message::<super::messages::OpenExternalProjectFile>()
        .add_systems(Update, systems::drive);
    {
        let mut state = app.world_mut().resource_mut::<ExternalFileState>();
        state.scan_requested = false;
        let detected = ExternalEditors::for_tests(&[ExternalEditor::Zed]).detected;
        state.task = Some(IoTaskPool::get().spawn(async move { ExternalTaskReply::Detected(detected) }));
    }
    for _ in 0..100 {
        app.update();
        if app.world().resource::<ExternalEditors>().installed(ExternalEditor::Zed) { break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.world().resource::<ExternalEditors>().installed(ExternalEditor::Zed));
    for _ in 0..3 { app.update(); }
    assert!(app.world().resource::<ExternalFileState>().task.is_none());
    app.world_mut().resource_mut::<ExternalFileState>().task = Some(IoTaskPool::get().spawn(std::future::pending()));
    app.world_mut().resource_mut::<crate::resources::SceneEditorState>().open = true;
    app.update();
    let state = app.world().resource::<ExternalFileState>();
    assert!(state.task.is_some() && state.scan_requested); // 旧任务保留，重开只合并下一次检测意图。
}

#[cfg(windows)]
#[test]
fn external_vscode_repeated_open_refreshes_a_replaced_version_script_once() {
    let fixture = Fixture::new();
    let home = fixture.0.join("VS Code 中文安装");
    let old = home.join("旧版/resources/app/out/cli.js");
    let current = home.join("新版/resources/app/out/cli.js");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(home.join("Code.exe"), []).unwrap();
    std::fs::write(&old, []).unwrap();
    let installs = [(ExternalEditor::VsCode, home.clone())];
    let mut available = discovery::discover_in(&[], &installs);
    let args = launcher::code_project_arguments(&fixture.0, &fixture.0.join("中文 空格/配置.json"), Some((28, 34)));
    let refreshes = std::cell::Cell::new(0);
    let mut executed = 0;
    for turn in 0..3 {
        let expected_script = if turn == 0 { &old } else { &current };
        launcher::launch_editor_with(ExternalEditor::VsCode, &mut available, &args, || {
            refreshes.set(refreshes.get() + 1);
            discovery::discover_in(&[], &installs)
        }, |command| {
            assert_eq!(command.get_program(), home.join("Code.exe"));
            let passed: Vec<_> = command.get_args().collect();
            assert_eq!(std::path::Path::new(passed[0]), expected_script); // Windows目录枚举可能规范化斜杠，核对真实脚本身份。
            assert_eq!(&passed[1..], args.iter().map(|arg| arg.as_os_str()).collect::<Vec<_>>()); // 恢复仍为原项目、原文件及原行列。
            executed += 1;
            Ok(true) // 捕获正式Command，不执行夹具中的空EXE或启动用户应用。
        }).unwrap();
        if turn == 0 {
            std::fs::remove_file(&old).unwrap();
            std::fs::create_dir_all(current.parent().unwrap()).unwrap();
            std::fs::write(&current, []).unwrap();
        }
    }
    assert_eq!(executed, 3);
    assert_eq!(refreshes.get(), 1);
    assert_eq!(available, discovery::discover_in(&[], &installs));
}

#[test]
fn external_disappeared_launcher_refreshes_cache_even_when_no_editor_remains() {
    let fixture = Fixture::new();
    for editor in ExternalEditor::ALL {
        let mut available = DetectedEditors::default();
        available.launchers[editor as usize] = Some(EditorLauncher { executable: fixture.0.join("removed.exe"), prefix_args: Vec::new() });
        let mut refreshes = 0;
        let error = launcher::launch_editor_with(editor, &mut available, &[], || {
            refreshes += 1; DetectedEditors::default()
        }, |_| panic!("失效入口不能启动任何进程")).unwrap_err();
        assert_eq!(refreshes, 1);
        assert!(error.contains("重新检测后未找到"));
        assert!(!error.contains("重新打开F12"));
        assert!(available.launchers.iter().all(Option::is_none));
    }
}

#[test]
fn external_process_not_found_retries_once_but_nonzero_exit_and_permissions_do_not() {
    let fixture = Fixture::new();
    let before = fixture.0.join("before.exe");
    let after = fixture.0.join("after.exe");
    for path in [&before, &after] { std::fs::write(path, []).unwrap(); }
    let detected = |path: &std::path::Path| {
        let mut result = DetectedEditors::default();
        result.launchers[ExternalEditor::Zed as usize] = Some(EditorLauncher { executable: path.into(), prefix_args: Vec::new() });
        result
    };
    let args = launcher::editor_arguments(ExternalEditor::Zed, Some(&fixture.0), &fixture.0.join("中文 空格/配置.json"), Some((28, 34)), false);
    let mut available = detected(&before);
    let mut attempts = 0;
    let mut refreshes = 0;
    launcher::launch_editor_with(ExternalEditor::Zed, &mut available, &args, || {
        refreshes += 1; detected(&after)
    }, |command| {
        attempts += 1;
        assert_eq!(command.get_args().collect::<Vec<_>>(), args.iter().map(|arg| arg.as_os_str()).collect::<Vec<_>>());
        assert_eq!(command.get_program(), if attempts == 1 { &before } else { &after });
        if attempts == 1 { Err(std::io::ErrorKind::NotFound.into()) } else { Ok(true) }
    }).unwrap();
    assert_eq!((attempts, refreshes), (2, 1));
    assert_eq!(available, detected(&after));
    for outcome in [Ok(false), Err(std::io::ErrorKind::PermissionDenied), Err(std::io::ErrorKind::NotFound)] {
        let mut available = detected(&before);
        let mut attempts = 0;
        let error = launcher::launch_editor_with(ExternalEditor::Zed, &mut available, &args, || {
            assert!(outcome.is_err_and(|kind| kind == std::io::ErrorKind::NotFound));
            detected(&after)
        }, |_| { attempts += 1; outcome.map_err(Into::into) }).unwrap_err();
        assert_eq!(attempts, if outcome.is_err_and(|kind| kind == std::io::ErrorKind::NotFound) { 2 } else { 1 });
        assert!(!error.contains("重新打开F12"));
    }
}

#[test]
fn external_refreshed_entry_is_rechecked_without_an_unbounded_retry() {
    let fixture = Fixture::new();
    let mut available = DetectedEditors::default();
    available.launchers[ExternalEditor::VsCode as usize] = Some(EditorLauncher { executable: fixture.0.join("removed.exe"), prefix_args: Vec::new() });
    let refreshed = available.clone();
    let mut refreshes = 0;
    assert!(launcher::launch_editor_with(ExternalEditor::VsCode, &mut available, &[], || {
        refreshes += 1; refreshed
    }, |_| panic!("重新检测后的缺失入口仍不能启动")).is_err());
    assert_eq!(refreshes, 1);
    assert!(available.launchers[ExternalEditor::VsCode as usize].is_none()); // 探测结束后又消失的入口也不继续显示。
}

#[test]
fn external_open_reply_adopts_the_refreshed_menu_cache_without_reopening_f12() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).init_resource::<crate::resources::SceneEditorState>()
        .init_resource::<ExternalFileState>().init_resource::<ExternalEditors>().add_message::<OpenExternalFile>()
        .add_message::<super::messages::OpenExternalProjectFile>()
        .add_systems(Update, systems::drive);
    app.world_mut().resource_mut::<ExternalEditors>().detected = ExternalEditors::for_tests(&[ExternalEditor::VsCode]).detected;
    let mut state = app.world_mut().resource_mut::<ExternalFileState>();
    state.scan_requested = false;
    let detected = ExternalEditors::for_tests(&[ExternalEditor::Zed]).detected;
    state.task = Some(IoTaskPool::get().spawn(async move {
        ExternalTaskReply::Opened { result: Err("检测后VS Code已不可用".into()), detected }
    }));
    for _ in 0..100 {
        app.update();
        if app.world().resource::<ExternalFileState>().task.is_none() { break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let available = app.world().resource::<ExternalEditors>();
    assert!(!available.installed(ExternalEditor::VsCode));
    assert!(available.installed(ExternalEditor::Zed));
    assert_eq!(app.world().resource::<crate::resources::SceneEditorState>().status, "检测后VS Code已不可用");
    assert!(!app.world().resource::<ExternalFileState>().scan_requested);
}

#[cfg(windows)]
#[test]
#[ignore = "开发机安装检测与CLI版本探针；不打开IDE窗口"]
fn external_installed_editor_discovery_and_cli_versions_are_real() {
    let detected = discovery::discover();
    for editor in ExternalEditor::ALL {
        let Some(found) = &detected.launchers[editor as usize] else { println!("{}: 未检测到", editor.name()); continue; };
        println!("{}: {}", editor.name(), found.executable.display());
        if editor != ExternalEditor::RustRover {
            let output = launcher::editor_command(editor, found).unwrap().arg("--version").output().unwrap();
            assert!(output.status.success(), "{}: {}", editor.name(), String::from_utf8_lossy(&output.stderr));
            assert!(!output.stdout.is_empty());
            println!("{} CLI: {}", editor.name(), String::from_utf8_lossy(&output.stdout).trim());
        }
    }
}

#[cfg(windows)]
#[test]
#[ignore = "仅在装有VS Code的开发机验证官方CLI；不打开窗口"]
fn external_installed_vscode_cli_version_succeeds_without_a_shell() {
    let output = launcher::code_command().unwrap().arg("--version").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!output.stdout.is_empty());
    println!("VS Code CLI: {}", String::from_utf8_lossy(&output.stdout).trim());
}

#[cfg(windows)]
#[test]
#[ignore = "实机连续打开当前项目源码并模拟缓存脚本失效；会打开VS Code窗口"]
fn external_installed_vscode_repeated_project_source_open_succeeds() {
    let fixture = Fixture::new();
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let project = launcher::checked_project(&assets).unwrap();
    let path = "crates/feathrise_editor_scene/src/ui/external_files/tests.rs";
    let (digest, _) = launcher::read_project_source(&project, path).unwrap();
    let request = OpenExternalFile { path: path.into(), has_draft: false,
        action: ExternalFileAction::ProjectSource { editor: ExternalEditor::VsCode, line: 1, column: 1, digest, in_project: true } };
    let mut available = discovery::discover();
    assert!(available.launchers[ExternalEditor::VsCode as usize].is_some(), "未安装VS Code");
    for turn in 0..3 {
        if turn == 1 {
            // 仅改变测试内缓存，不改本机安装；第二次恢复真实CLI，第三次复用新缓存。
            available.launchers[ExternalEditor::VsCode as usize].as_mut().unwrap().prefix_args = vec![fixture.0.join("removed-cli.js").into_os_string()];
        }
        let result = launcher::open(&assets, &request, &mut available).unwrap();
        assert!(result.contains("已请求在 VS Code 项目中打开"));
        println!("第{}次：{result}", turn + 1);
    }
}
