//! 自建正式工程的依赖边界验收；只在显式有限验收中运行Cargo，不介入UI或作者工程。
use crate::{
    build_process::{self, BuildControl},
    project,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
    process::Command,
    time::Duration,
};

pub(super) fn verify(root: &Path, control: &BuildControl) -> Result<Value, String> {
    let project = project::open(root)?;
    let mut command = Command::new(
        std::env::var_os("EDITOR_HOST_CARGO")
            .or_else(|| std::env::var_os("CARGO"))
            .unwrap_or_else(|| "cargo".into()),
    );
    command
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&project.manifest)
        .args(["--filter-platform", "x86_64-pc-windows-msvc"])
        .current_dir(root);
    crate::project_template::apply_project_network_policy(&project.manifest, &mut command)?;
    let output = build_process::run(&command, control, Some(Duration::from_secs(180)), metadata)?;
    if output.exit_code != 0 {
        return Err(format!("正式工程依赖解析失败：{}", output.stderr));
    }
    let names = runtime_names(&output.stdout)?;
    reject_original_game_packages(&names)?;
    reject_author_packages(&names)?;
    for required in [
        "feathrise_runtime",
        "feathrise_scene_core",
        "feathrise_scene",
        "feathrise_scene_content",
    ] {
        if !names.contains(required) {
            return Err(format!("正式工程实例缺少公共SDK能力{required}"));
        }
    }
    let root_id = output.stdout["resolve"]["root"]
        .as_str()
        .ok_or("缺少工程依赖根")?;
    let package = output.stdout["packages"]
        .as_array()
        .ok_or("缺少Cargo包表")?
        .iter()
        .find(|package| package["id"] == root_id)
        .ok_or("缺少工程包")?;
    let targets = package["targets"].as_array().ok_or("缺少工程目标表")?;
    if targets
        .iter()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .count()
        != 1
        || targets.iter().any(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "cdylib" || kind == "lib"))
        })
    {
        return Err("正式模板必须只有一个工程bin，不能同时复制原型动态库入口".into());
    }
    if root.join("sdk/contract").exists()
        || root.join("src/lib.rs").exists()
        || root.join("src/live_draft.rs").exists()
        || root.join("src/viewport.rs").exists()
    {
        return Err("正式模板仍包含旧IPC/DLL/作者视口源码".into());
    }
    Ok(
        json!({"passed":true, "root":project.root,"package":project.package,
        "normal_and_build_dependencies":names,"bin_only":true,"author_and_ipc_dependencies":[]}),
    )
}

/// 新命名和旧命名都属于作者边界；改名不能使依赖审计漏过同一份编辑器代码。
fn reject_author_packages(names: &BTreeSet<String>) -> Result<(), String> {
    for name in names {
        // SDK 的无 Bevy 进程协议是正式运行边界；仅豁免这个已发布包，不放开其余作者前缀。
        if (name.starts_with("feathrise_editor_") && name != "feathrise_editor_protocol")
            || name.starts_with("rpg_editor_")
            || matches!(name.as_str(), "feathrise_host" | "feathrise_project"
                | "new_editor_host" | "new_editor_project"
                | "new_editor_contract" | "new_editor_runtime" | "new_editor_document"
                | "editor_host_contract" | "editor_guest" | "editor_scene_document")
        {
            return Err(format!("正式工程实例的正常依赖错误地包含作者/原型包{name}"));
        }
    }
    Ok(())
}

/// 正式工程可以依赖SDK能力，不能随运行装配带入剑与侠客的设置、主题、PAK和内容策略。
fn reject_original_game_packages(names: &BTreeSet<String>) -> Result<(), String> {
    for name in [
        "rpg_game", "rpg_settings", "rpg_i18n", "rpg_save", "rpg_save_data", "rpg_asset_pack",
        "rpg_ui_core", "rpg_menu", "rpg_gameplay_ui", "rpg_character_panel", "rpg_minimap",
        "rpg_devtools", "rpg_narrative", "rpg_economy", "rpg_pickup",
        "rpg_camera", "rpg_camera_core", "rpg_input_gate", "rpg_nameplate", "rpg_editor_game",
        "rpg_character", "rpg_character_model", "rpg_character_state",
        "rpg_combat_core", "rpg_combat_model", "rpg_combat_runtime",
        "rpg_world_content", "rpg_item_core", "rpg_production_core", "rpg_faction",
        "rpg_economy_model", "rpg_narrative_model", "rpg_content_id",
        "rpg_constants", "rpg_map_content", "rpg_state",
    ] {
        if names.contains(name) {
            return Err(format!("正式工程依赖错误地包含原项目私有包{name}"));
        }
    }
    Ok(())
}

fn metadata(file: File) -> Result<Value, String> {
    // metadata只由有限验收读取；限制完整JSON预算，不用日志尾部截断后再误判依赖图。
    const LIMIT: u64 = 32 * 1024 * 1024;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("依赖验收metadata超出32MiB输出预算".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("依赖metadata不完整：{error}"))
}

fn runtime_names(metadata: &Value) -> Result<BTreeSet<String>, String> {
    let root = metadata["resolve"]["root"]
        .as_str()
        .ok_or("依赖图缺少根包")?;
    let packages = metadata["packages"].as_array().ok_or("依赖图缺少包表")?;
    let mut package_names = BTreeMap::new();
    for package in packages {
        package_names.insert(
            package["id"].as_str().ok_or("依赖包缺少身份")?,
            package["name"].as_str().ok_or("依赖包缺少名称")?,
        );
    }
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("依赖图缺少解析节点")?;
    let mut graph = BTreeMap::new();
    for node in nodes {
        graph.insert(node["id"].as_str().ok_or("依赖节点缺少身份")?, node);
    }
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        names.insert(
            package_names
                .get(id)
                .ok_or("解析到未登记的依赖包")?
                .to_string(),
        );
        let node = graph.get(id).ok_or("依赖图存在不完整节点")?;
        for dependency in node["deps"].as_array().ok_or("依赖节点缺少边表")? {
            let kinds = dependency["dep_kinds"].as_array().ok_or("依赖边缺少种类")?;
            // 从真实生成工程的根遍历正常/构建边；SDK库自己的dev依赖不进入游戏发布图。
            if kinds
                .iter()
                .any(|kind| kind["kind"].is_null() || kind["kind"] == "build")
            {
                pending.push(dependency["pkg"].as_str().ok_or("依赖边缺少目标包")?);
            }
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renamed_and_legacy_author_packages_cannot_enter_the_game_runtime_graph() {
        let clean = BTreeSet::from([
            "feathrise_game_template".into(), "feathrise_runtime".into(),
            "feathrise_scene_core".into(), "feathrise_editor_protocol".into(),
        ]);
        assert!(reject_author_packages(&clean).is_ok());
        for forbidden in [
            "feathrise_editor_scene", "feathrise_editor_documents_model", "feathrise_editor_protocol_extra", "feathrise_host",
            "feathrise_project", "rpg_editor_map", "new_editor_host", "new_editor_project",
            "new_editor_runtime", "editor_guest",
        ] {
            let mut names = clean.clone();
            names.insert(forbidden.into());
            assert!(reject_author_packages(&names).unwrap_err().contains(forbidden));
        }
    }

    #[test]
    fn dependency_walk_follows_build_edges_but_not_sdk_dev_tools_or_unreachable_packages() {
        let value = json!({"packages":[{"id":"game","name":"game"},{"id":"core","name":"feathrise_ui_widgets"},
            {"id":"dev","name":"feathrise_editor_scene"},{"id":"build","name":"build-helper"}],
            "resolve":{"root":"game","nodes":[
                {"id":"game","deps":[{"pkg":"core","dep_kinds":[{"kind":null}]}]},
                {"id":"core","deps":[{"pkg":"dev","dep_kinds":[{"kind":"dev"}]},{"pkg":"build","dep_kinds":[{"kind":"build"}]}]},
                {"id":"build","deps":[]}]}});
        assert_eq!(
            runtime_names(&value).unwrap(),
            BTreeSet::from([
                "game".into(),
                "feathrise_ui_widgets".into(),
                "build-helper".into()
            ])
        );
        let mut broken = value;
        broken["resolve"]["nodes"][0]["deps"][0]["pkg"] = "unknown".into();
        assert!(runtime_names(&broken).is_err()); // 不以跳过未知边制造“没有作者依赖”的假证据。
    }

    #[test]
    fn generated_project_rejects_original_game_settings_theme_and_pak() {
        let clean = BTreeSet::from(["feathrise_runtime".into(), "feathrise_ui_widgets".into()]);
        assert!(reject_original_game_packages(&clean).is_ok());
        for private in ["rpg_settings", "rpg_ui_core", "rpg_asset_pack", "rpg_i18n", "rpg_menu", "rpg_camera", "rpg_camera_core", "rpg_input_gate",
            "rpg_character", "rpg_character_model", "rpg_character_state",
            "rpg_combat_core", "rpg_combat_model", "rpg_combat_runtime",
            "rpg_world_content", "rpg_item_core", "rpg_production_core", "rpg_faction",
            "rpg_economy_model", "rpg_narrative_model", "rpg_content_id",
            "rpg_constants", "rpg_map_content", "rpg_state"] {
            let mut names = clean.clone();
            names.insert(private.into());
            assert!(reject_original_game_packages(&names).unwrap_err().contains(private));
        }
    }
}
