//! 本轮独占夹具的空地图变体；复现正式新建的空容器与空植被作者结构，不读取或修改用户工程。

use feathrise_map_content::{
    MapCatalogAsset, MapManifestAsset, MapTerrainDescriptor, MapVegetationAsset, MapWaterAsset,
};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};

pub(super) fn selected(root: &Path) -> Result<bool, String> {
    let source: Value = serde_json::from_slice(
        &fs::read(root.join(".native-fixture-provenance.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(source["empty_map_regression"] == true)
}

/// 只能在prepare已完成、本轮窗口尚未打开时调用；owner和provenance由原协调器严格核验。
pub(super) fn prepare(root: &Path) -> Result<(), String> {
    let project = super::orchestration::owner(root)?;
    let assets = project.root.join("assets");
    let folder = assets.join("maps/M1-SAMPLE");
    let mut manifest: MapManifestAsset = read(&folder.join("map.map_manifest.json"))?;
    manifest.terrains = Some(Arc::from([]));
    manifest.terrain = MapTerrainDescriptor::empty();
    manifest.world_min = [-0.5, -0.5];
    manifest.world_max = [0.5, 0.5];
    manifest.default_spawn = [0.0, 2.0, 0.0];
    write(&folder.join("map.map_manifest.json"), &manifest)?;
    write(
        &folder.join("placements.map_placements.json"),
        &json!({"schema_version":1,"map_id":"M1-SAMPLE","placements":[]}),
    )?;
    // 与正式新建相同：基础组存在但没有输出层/画布，不能等待不存在的地形编译任务。
    let vegetation: MapVegetationAsset = serde_json::from_value(json!({
        "schema_version":1,"map_id":"M1-SAMPLE","layers":[],
        "authoring":{"groups":[{"id":"base","label":"基础植被","mode":"additive",
            "output_merge":"maximum","capacity":1.0,"claim_threshold":0.0,
            "selection_seed":0x4D41_505F_4241_5345_u64,"selection_cell_meters":8.0,"enabled":true}],"layers":[]}
    })).map_err(|e|e.to_string())?;
    write(&folder.join("vegetation.map_vegetation.json"), &vegetation)?;
    let water: MapWaterAsset =
        serde_json::from_value(json!({"schema_version":1,"map_id":"M1-SAMPLE","layers":[]}))
            .map_err(|e| e.to_string())?;
    write(&folder.join("water.map_water.json"), &water)?;
    let mut catalog: MapCatalogAsset = read(&assets.join("maps/catalog.map_catalog.json"))?;
    catalog
        .maps
        .retain(|entry| entry.id.as_str() == "M1-SAMPLE");
    let entry = catalog.maps.first_mut().ok_or("本轮夹具缺少M1-SAMPLE")?;
    entry.surface_path = None;
    entry.boundary_path = None;
    entry.vegetation_path = Some("maps/M1-SAMPLE/vegetation.map_vegetation.json".into());
    entry.water_path = Some("maps/M1-SAMPLE/water.map_water.json".into());
    write(&assets.join("maps/catalog.map_catalog.json"), &catalog)?;
    let provenance = project.root.join(".native-fixture-provenance.json");
    let mut source: Value = read(&provenance)?;
    source["empty_map_regression"] = json!(true);
    source["empty_map_contract"] =
        json!("terrains=[]; root vegetation has base group and zero layers; no placements");
    write(&provenance, &source)
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn write(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
