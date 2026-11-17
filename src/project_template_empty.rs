//! 空工程生成：只复制薄入口与原生自由相机视图，公共 SDK 与补丁固定到同一来源。
use super::*;
use std::process::Command;
use feathrise_runtime::GameView;

pub(super) fn write(
    root: &Path,
    name: &str,
    id: &str,
    repository: &Path,
    control: &BuildControl,
    view: GameView,
) -> Result<(), String> {
    let nested = repository.join("feathrise");
    let editor = if nested.join("templates/empty").is_dir() { nested } else { repository.to_owned() };
    // 发行模板来自程序资源，外置 SDK 仍按配置文件所在目录解析。
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
    let (source, lock) = copy_template(editor, configuration, root, control)?;
    let (selected, selected_manifest, resolve) = if let Some(sdk) = sdk {
        let selected = sdk.apply_template(&source)?;
        let original: toml::Value = toml::from_str(&source).map_err(|e| e.to_string())?;
        let updated: toml::Value = toml::from_str(&selected).map_err(|e| e.to_string())?;
        // 等价的外置配置沿用模板锁，首次构建时再取得缺少的依赖。
        let resolve = (original != updated).then_some(sdk);
        (selected, updated, resolve)
    } else {
        let manifest = toml::from_str(&source).map_err(|e| e.to_string())?;
        (source, manifest, None)
    };
    for folder in ["assets", crate::project::DEFAULT_EDITOR_DATA, ".cargo"] {
        fs::create_dir_all(root.join(folder)).map_err(|e| e.to_string())?;
    }
    feathrise_project::seed::create_empty(&root.join("assets"))?;
    let manifest = personalize_targets(&selected, id)?;
    fs::write(root.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    let mut descriptor = json!({
        "format": 2,
        "id": id,
        "name": name,
        "cargo_manifest": "Cargo.toml",
        "package": "feathrise_game_template",
        "game_entry": {"kind": "saved-world", "version": 1},
        "game_view": view,
        "asset_roots": ["assets"],
    });
    if sdk::requires_editor_data(&selected_manifest) {
        descriptor["editor_data"] = json!(crate::project::DEFAULT_EDITOR_DATA);
    }
    fs::write(root.join("editor-project.json"), serde_json::to_vec_pretty(&descriptor).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::write(root.join(".cargo/config.toml"), "# 不记录开发机SDK或缓存绝对路径。\n[target.x86_64-pc-windows-msvc]\nlinker = \"rust-lld\"\n").map_err(|e| e.to_string())?;
    fs::write(root.join("Cargo.lock"), lock).map_err(|e| format!("无法写入模板锁文件：{e}"))?;
    let Some(sdk) = resolve else { return Ok(()); };
    // 显式自定义来源用 fetch 补齐来源变化的锁条目，保留仍兼容的种子锁版本。
    // generate-lockfile 会忽略种子重求版本，使后来被 yanked 的已锁依赖也无法新建。
    // 本地联调保留离线缓存；Git／registry 首次获取受同一个可取消 Job 和发布守卫约束。
    let mut command = Command::new(crate::native_play::cargo_program());
    command
        .args(["fetch", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .env("CARGO_RESOLVER_LOCKFILE_PATH", root.join("Cargo.lock"))
        .current_dir(root);
    sdk.apply_network_policy(&mut command);
    // Rustup 等工具也可能把失败原因写到 stdout；两路都持续排空并只保留固定长度尾部。
    let result = crate::build_process::run_cargo(
        &command, control, None, crate::build_process::read_diagnostics,
    )?;
    if result.exit_code != 0 {
        return Err(crate::environment_diagnostics::cargo_exit_failure(
            crate::environment_diagnostics::CargoStage::NewProject,
            &command, result.exit_code, &result.stdout, &result.stderr,
        ));
    }
    Ok(())
}

fn copy_template(
    editor: &Path,
    configuration: &Path,
    root: &Path,
    control: &BuildControl,
) -> Result<(String, Vec<u8>), String> {
    #[cfg(windows)]
    if let Some(mut source) = super::resources::read(0)
        .map_err(|error| format!("无法读取发行模板：{error}"))?
    {
        let manifest = source.remove("Cargo.toml").ok_or("发行模板缺少 Cargo.toml")?;
        let lock = source.remove("Cargo.lock").ok_or("发行模板缺少 Cargo.lock")?;
        for (relative, bytes) in source {
            control.check()?;
            let target = root.join(relative);
            fs::create_dir_all(target.parent().ok_or("模板文件缺少父目录")?).map_err(|e| e.to_string())?;
            fs::write(target, bytes).map_err(|e| e.to_string())?;
        }
        return Ok((String::from_utf8(manifest).map_err(|e| e.to_string())?, lock));
    }

    let relative = "templates/empty";
    let mut template = editor.join(relative);
    // 目录包把模板放在 EXE 旁，图标等内嵌资源仍从各自的缓存读取。
    if editor != configuration && !template.try_exists().map_err(|error| error.to_string())? {
        template = configuration.join(relative);
    }
    let read_error = |error| format!("无法读取空模板 {}：{error}", template.display());
    let manifest = fs::read_to_string(template.join("Cargo.toml")).map_err(read_error)?;
    let lock = fs::read(template.join("Cargo.lock")).map_err(read_error)?;
    let mut budget = CopyBudget::default();
    copy_tree(&template.join("src"), &root.join("src"), &mut budget, control)?;
    for file in ["build.rs", "README.md"] {
        files::copy_file(&template, root, file, &mut budget, control)?;
    }
    Ok((manifest, lock))
}
