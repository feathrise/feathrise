//! 空工程 Cargo 清单的生成规则，由编辑器与打包工具共用。

use toml::{Value, map::Map};

pub(crate) const BEVY_PIN: &str = "=0.20.1";
pub(crate) const PATCHES: [(&str, &str); 8] = [
    ("avian3d", "avian3d"),
    ("bevy_anti_alias", "bevy_anti_alias"),
    ("bevy_render", "bevy_render"),
    ("bevy_core_pipeline", "bevy_core_pipeline"),
    ("bevy_extract", "bevy_extract"),
    ("wgpu-hal", "wgpu-hal"),
    ("dlss_wgpu", "dlss_wgpu"),
    ("wgpu-ffx", "wgpu-ffx"),
];

pub(crate) fn scene_registry_sdk_version(value: &str) -> Result<semver::Version, String> {
    let version = semver::Version::parse(value)
        .map_err(|error| format!("registry SDK 须指定完整版本（{error}）"))?;
    if (version.major, version.minor) != (0, 3) {
        return Err(format!("场景工程的 registry 来源需要 SDK 0.3.x，实际配置为 {version}"));
    }
    Ok(version)
}

pub(crate) fn apply_template(
    source: &str,
    runtime: Map<String, Value>,
    patches: Map<String, Value>,
) -> Result<String, String> {
    let mut manifest: Value = toml::from_str(source)
        .map_err(|error| format!("空工程 Cargo 模板无效：{error}"))?;
    let dependencies = manifest.get_mut("dependencies").and_then(Value::as_table_mut)
        .ok_or("空工程模板缺少依赖表")?;
    let dependency = dependencies.get_mut("feathrise_runtime")
        .and_then(Value::as_table_mut)
        .ok_or("空工程模板缺少显式 feathrise_runtime 依赖表")?;
    // 只替换来源，模板声明的 features 和 default-features 继续决定工程能力。
    for key in ["path", "git", "rev", "branch", "tag", "registry", "version", "workspace"] {
        dependency.remove(key);
    }
    dependency.extend(runtime);
    if let Some(dependency) = dependencies.get_mut("bevy") {
        // 自由相机组件须与所选 SDK 的 App 来自同一套 Bevy。
        let table = dependency.as_table_mut().ok_or("空工程模板的 Bevy 依赖必须声明能力表")?;
        table.insert("version".into(), Value::String(BEVY_PIN.into()));
    }
    let root = manifest.as_table_mut().ok_or("空工程 Cargo 模板必须是表")?;
    root.entry("workspace").or_insert_with(|| Value::Table(Map::from_iter([
        ("resolver".into(), Value::String("3".into())),
    ])));
    let patch = root.entry("patch").or_insert_with(|| Value::Table(Map::new()))
        .as_table_mut().ok_or("空工程模板的 patch 必须是表")?;
    let crates_io = patch.entry("crates-io").or_insert_with(|| Value::Table(Map::new()))
        .as_table_mut().ok_or("空工程模板的 patch.crates-io 必须是表")?;
    crates_io.extend(patches);
    toml::to_string_pretty(&manifest).map_err(|error| error.to_string())
}
