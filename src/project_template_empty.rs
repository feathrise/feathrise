//! 空工程生成：只复制薄入口与原生自由相机视图，公共 SDK 与补丁固定到同一来源。
use super::*;
use std::{io::Read, process::Command};
use feathrise_runtime::GameView;

pub(super) fn write(
    root: &Path,
    name: &str,
    id: &str,
    repository: &Path,
    control: &BuildControl,
    view: GameView,
) -> Result<(), String> {
    let editor = if repository.join("assets/templates/empty").is_dir() {
        repository.to_path_buf()
    } else {
        repository.join("feathrise")
    };
    // 静态资源可以来自内嵌缓存；SDK 相对路径必须仍以实际配置文件所在目录为基准。
    let configuration = crate::resource_paths::sdk_configuration_root(&editor);
    write_with_sdk_configuration(root, name, id, &editor, &configuration, control, view)
}

/// 将不可变模板来源与可配置的 SDK 来源显式分离；调用方和测试都不需要改变 cwd 或环境。
pub(super) fn write_with_sdk_configuration(
    root: &Path,
    name: &str,
    id: &str,
    editor: &Path,
    configuration: &Path,
    control: &BuildControl,
    view: GameView,
) -> Result<(), String> {
    // 先完成来源校验；显式 Git 配置不要求编辑器旁边存在源码 SDK。
    let sdk = sdk::SdkSource::load(configuration)?;
    let template = editor.join("assets/templates/empty");
    let mut budget = CopyBudget::default();
    copy_tree(
        &template.join("src"),
        &root.join("src"),
        &mut budget,
        control,
    )?;
    for file in ["build.rs", "README.md"] {
        files::copy_file(&template, root, file, &mut budget, control)?;
    }
    for folder in ["assets", ".editor", ".cargo"] {
        fs::create_dir_all(root.join(folder)).map_err(|e| e.to_string())?;
    }
    feathrise_project::seed::create_empty(&root.join("assets"))?;
    let source = fs::read_to_string(template.join("Cargo.toml")).map_err(|e| e.to_string())?;
    let mut manifest = sdk.apply_template(&personalize_targets(&source, id)?)?;
    // 新工程自身保持低优化以缩短迭代；非工作区依赖统一高优化，不再逐包维护例外。
    manifest.push_str("\n[profile.dev]\nopt-level = 0\ndebug = \"line-tables-only\"\nstrip = false\n\n[profile.dev.package.\"*\"]\nopt-level = 3\n\n[profile.render-dev]\ninherits = \"dev\"\n");
    fs::write(root.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    fs::write(root.join("editor-project.json"), serde_json::to_vec_pretty(&json!({"format":2,"id":id,"name":name,"cargo_manifest":"Cargo.toml","package":"feathrise_game_template","game_entry":{"kind":"saved-world","version":1},"game_view":view,"asset_roots":["assets"],"editor_data":".editor"})).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::write(root.join(".cargo/config.toml"), "# 不记录开发机SDK或缓存绝对路径。\n[target.x86_64-pc-windows-msvc]\nlinker = \"rust-lld\"\n").map_err(|e| e.to_string())?;
    fs::copy(editor.join(sdk.lock_seed()), root.join("Cargo.lock")).map_err(|e| e.to_string())?;
    // 各种来源都用 fetch 补齐来源变化的锁条目，保留仍兼容的种子锁版本。
    // generate-lockfile 会忽略种子重求版本，使后来被 yanked 的已锁依赖也无法新建。
    // 本地联调保留离线缓存；Git／registry 首次获取受同一个可取消 Job 和发布守卫约束。
    let cargo = std::env::var_os("EDITOR_HOST_CARGO")
        .or_else(|| std::env::var_os("CARGO"))
        .unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .args(["fetch", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .current_dir(root);
    sdk.apply_network_policy(&mut command);
    let result = crate::build_process::run_cargo(&command, control, None, |mut file| {
        // 工具 stdout 必须持续排空；创建工程只需要退出码，不随下载规模累积完整日志。
        let mut bytes = [0_u8; 8192];
        while file.read(&mut bytes).map_err(|e| e.to_string())? != 0 {}
        Ok(())
    })?;
    if result.exit_code != 0 {
        return Err(crate::environment_diagnostics::cargo_exit_failure(
            crate::environment_diagnostics::CargoStage::NewProject,
            &command, result.exit_code, "", &result.stderr,
        ));
    }
    Ok(())
}
