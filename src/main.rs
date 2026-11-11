//! 正式独立编辑器入口；无参数在本目录 cargo run 进入工作台。
// 普通 Windows Release 作为桌面应用启动；开发与显式验收仍保留控制台诊断。
#![cfg_attr(all(target_os = "windows", not(debug_assertions), not(feature = "validation")), windows_subsystem = "windows")]

mod branding;
mod build_process;
#[cfg(windows)]
mod editor_frame_pacing;
mod build_artifacts; // 所有开发启动共用独立版本副本，不占用下一轮Cargo输出。
mod compiler_diagnostics;
mod code_iteration;
mod project_export;
mod environment_diagnostics;
mod folder_picker;
#[cfg(any(test, feature = "validation"))]
mod native_authoring_validation;
mod native_editor;
mod native_plugins; // 工程安装的DLL/SO工具，不把任何RPG作者实现链接到宿主。
mod native_play;
mod embedded_viewport;
mod viewport_mode;
#[cfg(feature = "validation")]
mod native_play_recovery_validation;
mod native_process;
mod portable_author;
mod project_log;
mod native_project_close;
mod native_project_lease;
mod native_project_menu;
#[cfg(feature = "validation")]
mod native_validation;
#[cfg(feature = "validation")]
mod locale_validation;
#[cfg(all(feature = "validation", windows))]
mod scene_format_validation;
#[cfg(feature = "validation")]
mod project_browser_validation;
mod project;
mod project_template;
#[cfg(test)]
mod template_camera_tests;
mod resource_paths;
mod user_preferences;
mod writable_paths;
mod workspace;
mod workspace_transport;
mod workspace_ui;
mod workspace_window;
mod window_chrome;
mod project_creation;
mod project_identity;
mod startup_document;
mod startup_store;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    // Unix系统选择器在独占短生命周期进程中运行；取消只结束本次对话框，不阻塞作者帧。
    #[cfg(unix)]
    if folder_picker::run_helper()? { return Ok(()); }
    let raw: Vec<_> = std::env::args_os().skip(1).collect();
    let mut args = raw.into_iter();
    let mut root = None;
    let mut target = None;
    let mut native_report = None;
    let mut embedded_report = None;
    let mut embedded_build_cancel_report = None;
    let mut plugin_report = None;
    let mut code_iteration_report = None;
    let mut export_report = None;
    let mut scene_format_report = None;
    let mut project_browser_report = None;
    let mut plugin_package = None;
    let mut native = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--project") => {
                root = Some(PathBuf::from(args.next().ok_or("--project缺少目录")?))
            }
            Some("--target-dir") => {
                target = Some(PathBuf::from(args.next().ok_or("--target-dir缺少目录")?))
            }
            Some("--native-report") => {
                native_report = Some(PathBuf::from(
                    args.next().ok_or("--native-report缺少报告路径")?,
                ))
            }
            Some("--native-editor") => native = true,
            Some("--embedded-report") => {
                embedded_report = Some(PathBuf::from(args.next().ok_or("--embedded-report缺少报告路径")?));
            }
            Some("--embedded-build-cancel-report") => embedded_build_cancel_report = Some(PathBuf::from(args.next().ok_or("--embedded-build-cancel-report缺少报告路径")?)),
            Some("--plugin-report") => plugin_report = Some(PathBuf::from(args.next().ok_or("--plugin-report缺少路径")?)),
            Some("--code-iteration-report") => code_iteration_report = Some(PathBuf::from(args.next().ok_or("--code-iteration-report缺少路径")?)),
            Some("--export-report") => export_report = Some(PathBuf::from(args.next().ok_or("--export-report缺少路径")?)),
            Some("--scene-format-report") => scene_format_report = Some(PathBuf::from(args.next().ok_or("--scene-format-report缺少路径")?)),
            Some("--project-browser-report") => project_browser_report = Some(PathBuf::from(args.next().ok_or("--project-browser-report缺少路径")?)),
            Some("--plugin-package") => plugin_package = Some(PathBuf::from(args.next().ok_or("--plugin-package缺少插件文件或发布目录")?)),
            _ => {
                return Err(
                    "用法：cargo run [-- --project <工程目录>]；无参数进入工作台开始页。验收：--native-report、--embedded-report、--embedded-build-cancel-report、--code-iteration-report、--export-report、--scene-format-report、--project-browser-report 或 --plugin-report <报告路径>（插件另需 --plugin-package <插件文件或发布目录>）；详见 docs/开发与验收.md"
                        .into(),
                );
            }
        }
    }
    // 窗口、AssetServer 和后台工程任务共用启动时确定的唯一资源根；不依赖工程 cwd。
    let resource_mode = resource_paths::initialize()?;
    // 仅显式覆盖可跨实例传递；工程默认使用自己的target，不能继承宿主的默认缓存。
    let target_override = writable_paths::build_target_override(target)?;
    // 默认启动也保留原缓存路径校验；返回值只由显式validation夹具使用。
    let _validation_target = writable_paths::build_target(resource_mode, target_override.clone())?;
    // 每类验收各自创建独占夹具，不能同时选择而静默忽略另一个入口。
    let reports = [native_report.is_some(), embedded_report.is_some(), embedded_build_cancel_report.is_some(), plugin_report.is_some(), code_iteration_report.is_some(), export_report.is_some(), scene_format_report.is_some(), project_browser_report.is_some()];
    if reports.into_iter().filter(|selected| *selected).count() > 1 {
        return Err("一轮只选择一种窗口验收入口".into());
    }
    if let Some(report) = embedded_build_cancel_report {
        #[cfg(all(feature = "validation", windows))]
        return embedded_viewport::validation::build_cancel::run(_validation_target, report);
        #[cfg(not(all(feature = "validation", windows)))]
        { let _ = report; return Err("工程3D停止编译验收需要Windows及显式validation".into()); }
    }
    if let Some(report) = export_report {
        #[cfg(all(feature = "validation", windows))]
        return project_export::validation::run(_validation_target, report);
        #[cfg(not(all(feature = "validation", windows)))]
        { let _ = report; return Err("工程导出窗口验收需要Windows及显式validation".into()); }
    }
    if let Some(report) = scene_format_report {
        #[cfg(all(feature = "validation", windows))]
        return scene_format_validation::run(_validation_target, report);
        #[cfg(not(all(feature = "validation", windows)))]
        { let _ = report; return Err("场景格式窗口验收需要Windows及显式validation".into()); }
    }
    if let Some(report) = project_browser_report {
        #[cfg(feature = "validation")]
        return project_browser_validation::run(_validation_target, report);
        #[cfg(not(feature = "validation"))]
        { let _ = report; return Err("工程目录窗口验收需要显式启用validation".into()); }
    }
    if let Some(report) = code_iteration_report {
        #[cfg(all(feature = "validation", windows))]
        return code_iteration::validation::run(_validation_target, report);
        #[cfg(not(all(feature = "validation", windows)))]
        { let _ = report; return Err("此代码迭代窗口专项需要Windows及显式validation".into()); }
    }
    if let Some(report) = embedded_report {
        #[cfg(all(feature = "validation", windows))]
        return embedded_viewport::validation::run(_validation_target, report);
        #[cfg(not(all(feature = "validation", windows)))]
        {
            let _ = report;
            return Err("此原生工程视口专项需要Windows及显式validation".into());
        }
    }
    if let Some(report) = plugin_report {
        #[cfg(feature = "validation")]
        return native_plugins::validation::run(_validation_target, report, plugin_package.ok_or("插件验收需要--plugin-package")?);
        #[cfg(not(feature = "validation"))]
        { let _ = (report, plugin_package); return Err("插件验收需要显式启用validation".into()); }
    }
    if let Some(report) = native_report {
        #[cfg(all(feature = "validation", windows))]
        return native_validation::run(_validation_target, report);
        #[cfg(not(all(feature = "validation", windows)))]
        {
            let _ = report;
            return Err("窗口验收需要 cargo run --features validation -- --native-report <路径>".into());
        }
    }
    if native {
        let project = project::open(&root.ok_or("--native-editor需要--project")?)?;
        let target = writable_paths::project_target(&project.root, target_override.as_deref())?;
        return native_editor::run(project, target);
    }
    workspace::run(root, target_override)
}
pub(crate) fn project_template_root() -> PathBuf {
    feathrise_editor_core::editor_resource_root()
}
