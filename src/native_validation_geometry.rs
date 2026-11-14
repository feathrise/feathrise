//! 几何专项的真实保存和重开闭环；复用编辑器只读源回验，不复制高度与三维字段算法。
use feathrise_editor_scene::standalone::validation_fixture::{self, GeometryUnificationProof};
use serde_json::Value;
use std::path::Path;

fn enabled() -> bool {
    std::env::var("FEATHRISE_GEOMETRY_UNIFICATION_VALIDATION").as_deref() == Ok("1")
}

pub(super) fn verify_saved(root: &Path, output: &Path, game: &Value, proof: &Value) -> Result<(),String> {
    if !enabled() { return Ok(()); }
    let typed: GeometryUnificationProof = serde_json::from_value(proof.clone()).map_err(|error|error.to_string())?;
    if !typed.complete || typed.error.is_some() || !typed.overbudget_source_unchanged
        || typed.overbudget_error.as_deref().is_none_or(str::is_empty) {
        return Err(format!("几何专项没有完成原业务与预算失败：{proof}"));
    }
    typed.verify_volume_sides()?;
    typed.verify_height_vegetation()?;
    for image in ["geometry-small.png","geometry-clipmap-vegetation.png","geometry-voxel.png","geometry-voxel-underside.png"] {
        if !output.join(image).is_file() { return Err(format!("几何专项缺少真实GPU截图：{image}")); }
    }
    validation_fixture::verify_geometry_unification_saved(&root.join("assets"),&typed)?;
    let targets = [typed.tiny.as_ref().ok_or("缺少0.01米地形")?,
        &typed.small.as_ref().ok_or("缺少单层小地形")?.after,
        &typed.voxel_underside.as_ref().ok_or("缺少体积双侧种植")?.after];
    for target in targets {
        let runtime = runtime_target(game, &typed.scene_id, &target.terrain_id)?;
        let path_key = if target.kind == "volume_voxel" { "voxel_source_path" } else { "height_path" };
        if runtime[path_key] != target.source_path || runtime["collision_ready"] != true {
            return Err(format!("正式游戏未采用已保存的地形或碰撞：{runtime}"));
        }
        if target.kind == "volume_voxel" {
            verify_volume_runtime(runtime, &target.source_path,
                typed.vegetation_output_layer.as_deref().ok_or("缺少上侧保存层身份")?,
                typed.underside_vegetation_output_layer.as_deref().ok_or("缺少下侧保存层身份")?)?;
        } else if typed.height_vegetation.as_ref().is_some_and(|stroke| stroke.after.terrain_id == target.terrain_id)
            && runtime["vegetation"]["blade_windows"].as_u64().unwrap_or(0) == 0
            && runtime["vegetation"]["mesh_batches"].as_u64().unwrap_or(0) == 0 {
            return Err("正式游戏没有显示已保存的Clipmap植被".into());
        }
    }
    Ok(())
}

/// 稳定地形ID之外还须核对来源场景及完整实例路径；兄弟或候选实例不能替父图取得信用。
fn runtime_target<'a>(game: &'a Value, scene: &str, terrain: &str) -> Result<&'a Value, String> {
    let path = feathrise_scene_content::SceneInstancePath::root(feathrise_scene_content::SceneId::parse(scene)?)
        .terrain(feathrise_scene_content::TerrainId::parse(terrain)?).to_string();
    let matches = game["terrain_objects"].as_array().ok_or("正式游戏缺少专项地形集合")?.iter()
        .filter(|object| object["terrain_id"] == terrain).take(2).collect::<Vec<_>>();
    if game["scene_id"] != scene || matches.len() != 1 || matches[0]["source_scene_id"] != scene
        || matches[0]["instance_path"] != path || matches[0]["instance"].as_u64().is_none() {
        return Err("正式游戏专项地形的场景、完整实例路径或唯一身份不符".into());
    }
    Ok(matches[0])
}

/// 隐藏模板也有blade_windows；体积目标只接受当前三维字段和保存后的两层实际草窗。
fn verify_volume_runtime(runtime: &Value, source_path: &str, top: &str, bottom: &str) -> Result<(), String> {
    let vegetation = &runtime["vegetation"];
    let volume = &vegetation["volume"];
    let saved = volume["saved_layers"].as_array().ok_or("F12缺少保存的三维层目录")?;
    let layers = volume["layers"].as_array().ok_or("F12缺少正式三维层采用证据")?;
    if top == bottom || vegetation["phase"] != "Ready" || vegetation["scene_id"] != runtime["source_scene_id"]
        || volume["ready"] != true || volume["generation"].as_u64().unwrap_or(0) == 0
        || volume["instance"] != runtime["instance"] || volume["instance_path"] != runtime["instance_path"]
        || volume["saved_source_path"] != source_path || volume["root_owned"] != true
        || volume["fields_current"] != true || volume["ground_current"] != true
        || !volume["fields"].as_array().is_some_and(|fields| !fields.is_empty() && fields.len() <= 32)
        || !volume["ground_revision"].as_array().is_some_and(|words| words.len() == 7) {
        return Err(format!("F12没有采用当前目标的完整三维字段：{runtime}"));
    }
    for id in [top, bottom] {
        let matching = layers.iter().filter(|layer| layer["id"] == id).collect::<Vec<_>>();
        if saved.iter().filter(|layer| layer.as_str() == Some(id)).count() != 1 || matching.len() != 1 {
            return Err(format!("F12缺少保存后的独立三维作者层：{id}"));
        }
        let layer = matching[0];
        let windows = layer["expected_windows"].as_u64().unwrap_or(0);
        let total = layer["source_samples"].as_u64().unwrap_or(0);
        let positive = layer["positive_source_samples"].as_u64().unwrap_or(0);
        if layer["source_present"] != true || positive == 0 || positive > total
            || windows == 0 || layer["actual_windows"].as_u64() != Some(windows)
            || layer["unique_lods"].as_u64() != Some(windows) || layer["bindings_current"] != true {
            return Err(format!("F12保存层没有当前三维来源和实际窗口：{layer}"));
        }
    }
    Ok(())
}

pub(super) fn verify_reopen(root: &Path, document: &Value, proof: &Value, same: bool) -> Result<(),String> {
    if !enabled() { return Ok(()); }
    let typed: GeometryUnificationProof = serde_json::from_value(proof.clone()).map_err(|error|error.to_string())?;
    typed.verify_volume_sides()?;
    typed.verify_height_vegetation()?;
    let objects = document["terrains"].as_array().ok_or("重开缺少原地形对象集合")?;
    for target in [typed.tiny.as_ref().ok_or("缺少微型地形证据")?,
        &typed.small.as_ref().ok_or("缺少小地形证据")?.after,
        &typed.voxel_underside.as_ref().ok_or("缺少体积双侧种植证据")?.after] {
        let actual = objects.iter().find(|object|object["id"] == target.terrain_id);
        if same {
            let actual = actual.ok_or("重开没有专项新增地形")?;
            let path = if target.kind == "volume_voxel" { &actual["voxel"]["source_path"] }
                else { &actual["terrain"]["height_path"] };
            if path != &target.source_path { return Err("重开没有采用专项已保存来源".into()); }
        } else if actual.is_some() { return Err("B工程串用了A的专项地形".into()); }
    }
    // 全新作者进程已读入同一稳定目标；退出后再从该工程的真实持久根核对两侧完整样本。
    if same { validation_fixture::verify_geometry_unification_saved(&root.join("assets"), &typed)?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn game() -> Value {
        json!({"scene_id":"fixture","terrain_objects":[{
            "terrain_id":"thin-plate","source_scene_id":"fixture","instance":99,
            "instance_path":"fixture/terrain:thin-plate",
            "vegetation":{"phase":"Ready","scene_id":"fixture","blade_windows":8,"volume":{
                "ready":true,"generation":7,"instance":99,"instance_path":"fixture/terrain:thin-plate",
                "saved_source_path":"source.json","root_owned":true,"fields_current":true,"ground_current":true,
                "fields":[{"entity":100,"snapshot":101}],"ground_revision":[1,1,1,1,1,1,1],
                "saved_layers":["top","bottom"],"layers":[
                    {"id":"top","source_present":true,"source_samples":3,"positive_source_samples":1,"expected_windows":4,"actual_windows":4,"unique_lods":4,"bindings_current":true},
                    {"id":"bottom","source_present":true,"source_samples":4,"positive_source_samples":1,"expected_windows":4,"actual_windows":4,"unique_lods":4,"bindings_current":true}
                ]
            }}
        }]})
    }

    #[test]
    fn f12_volume_accepts_only_current_saved_double_sided_sources() {
        let mut game = game();
        // 完整地址以生产Display为准，避免测试自行发明路径格式。
        let path = feathrise_scene_content::SceneInstancePath::root(feathrise_scene_content::SceneId::parse("fixture").unwrap())
            .terrain(feathrise_scene_content::TerrainId::parse("thin-plate").unwrap()).to_string();
        game["terrain_objects"][0]["instance_path"] = path.clone().into();
        game["terrain_objects"][0]["vegetation"]["volume"]["instance_path"] = path.into();
        let runtime = runtime_target(&game, "fixture", "thin-plate").unwrap();
        verify_volume_runtime(runtime, "source.json", "top", "bottom").unwrap();
        for (field, value) in [("ready", json!(false)), ("generation", json!(0)),
            ("fields_current", json!(false)), ("ground_current", json!(false)), ("root_owned", json!(false)), ("instance", json!(98))] {
            let mut bad = runtime.clone(); bad["vegetation"]["volume"][field] = value;
            assert!(verify_volume_runtime(&bad, "source.json", "top", "bottom").is_err(), "必须拒绝{field}");
        }
    }

    #[test]
    fn f12_hidden_templates_cannot_cover_empty_missing_or_stale_volume_layer() {
        let game = game();
        let runtime = &game["terrain_objects"][0];
        for (field, value) in [("source_present", json!(false)), ("source_samples", json!(0)),
            ("positive_source_samples", json!(0)), ("positive_source_samples", json!(5)),
            ("actual_windows", json!(0)), ("bindings_current", json!(false))] {
            let mut bad = runtime.clone(); bad["vegetation"]["volume"]["layers"][1][field] = value;
            assert!(verify_volume_runtime(&bad, "source.json", "top", "bottom").is_err());
            assert_eq!(bad["vegetation"]["blade_windows"], 8);
        }
        let mut missing = runtime.clone(); missing["vegetation"]["volume"]["layers"].as_array_mut().unwrap().pop();
        assert!(verify_volume_runtime(&missing, "source.json", "top", "bottom").is_err());
    }

    #[test]
    fn f12_runtime_target_rejects_sibling_paths_and_duplicate_stable_ids() {
        let mut game = game();
        game["terrain_objects"][0]["instance_path"] = "fixture/sibling/terrain:thin-plate".into();
        assert!(runtime_target(&game, "fixture", "thin-plate").is_err());
        game["terrain_objects"][0]["instance_path"] = "fixture/terrain:thin-plate".into();
        assert!(runtime_target(&game, "fixture", "thin-plate").is_ok());
        let duplicate = game["terrain_objects"][0].clone();
        game["terrain_objects"].as_array_mut().unwrap().push(duplicate);
        assert!(runtime_target(&game, "fixture", "thin-plate").is_err());
    }
}
