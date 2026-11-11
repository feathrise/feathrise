//! 连续预览专用的有限资产，只在独占夹具启动前准备；窗口运行期间不保存预设或密度。

use feathrise_scene_content::{VegetationPresetCatalogAsset, VegetationPresetRuntime, ResolvedVegetationPresetCatalog, VEGETATION_PRESET_CATALOG_ASSET_PATH};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub(super) fn prepare(root: &Path) -> Result<(), String> {
    let project = super::orchestration::owner(root)?;
    if super::empty_fixture::selected(root)? {
        return Err("连续植被预览夹具需要真实地形，不能与空场景专项同时启用".into());
    }
    let assets = project.root.join("assets");
    let directory = assets.join("scene/embedded-live");
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    // 与作者现成生产回归共用中性GLTF和正式Blade目录规格，不复制游戏资产或创建第二套草后端。
    fs::write(directory.join("blade.gltf"), include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/crates/feathrise_editor_scene/tests/fixtures/gltf/fixture_vegetation.gltf")))
        .map_err(|error| error.to_string())?;
    let mut catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/crates/feathrise_editor_scene/tests/fixtures/vegetation_presets.vegetation_presets.json")))
        .map_err(|error| error.to_string())?;
    let mut preset = catalog.presets.remove(0);
    preset.id = "embedded-live-blade".into();
    preset.output_id = "embedded-live-blade".into();
    preset.label = "连续预览验收叶片".into();
    preset.author_asset_path = "scene/embedded-live/blade.gltf".into();
    preset.resolution = [128, 128];
    if let VegetationPresetRuntime::Blade { lods, .. } = &mut preset.runtime {
        for lod in lods { lod.mesh_path = "scene/embedded-live/blade.gltf#Mesh0/Primitive0".into(); }
    } else { return Err("中性夹具首项必须为正式Blade预设".into()); }
    catalog.presets = vec![preset];
    ResolvedVegetationPresetCatalog::build(&catalog)?;
    fs::write(assets.join(VEGETATION_PRESET_CATALOG_ASSET_PATH), serde_json::to_vec_pretty(&catalog).map_err(|error|error.to_string())?)
        .map_err(|error| error.to_string())?;
    let path = project.root.join(".native-fixture-provenance.json");
    let mut provenance: Value = serde_json::from_slice(&fs::read(&path).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
    provenance["embedded_live_preview"] = json!(true);
    fs::write(path, serde_json::to_vec_pretty(&provenance).map_err(|error|error.to_string())?).map_err(|error|error.to_string())
}

pub(super) fn selected(root: &Path) -> Result<bool, String> {
    let provenance: Value = serde_json::from_slice(&fs::read(root.join(".native-fixture-provenance.json")).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
    Ok(provenance["embedded_live_preview"] == true)
}
