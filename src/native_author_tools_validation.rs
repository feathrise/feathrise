//! 仅本轮独占验收工程需要的专业工具素材与采用核对；不扩展普通新建模板。

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

/// 调用者刚通过create_dir独占创建工程；仅复制编辑器自有小模型来验证Blade网格采用。
pub(super) fn prepare(root: &Path) -> Result<(), String> {
    let project = crate::project::open(root)?;
    let assets = project
        .context
        .asset_roots
        .first()
        .ok_or("验收工程缺少资产根")?;
    let directory = assets.join("scene/验收");
    fs::create_dir(&directory).map_err(|error| format!("验收专用素材目录无法独占创建：{error}"))?;
    fs::write(
        directory.join("Blade叶片.glb"),
        super::fixture::scene_fixture("probe.glb")?,
    )
    .map_err(|error| format!("验收Blade源复制失败：{error}"))?;
    // probe引用同目录贴图；两份均为编辑器自有夹具，不把原游戏植被与图集带回公开边界。
    fs::write(directory.join("albedo.png"), super::fixture::scene_fixture("albedo.png")?)
        .map_err(|error| format!("验收Blade贴图复制失败：{error}"))?;
    // 普通文本关闭事务与游戏场景/地表源分离，关闭重试不会改写此前F12的内容证据。
    use std::io::Write;
    fs::OpenOptions::new().write(true).create_new(true)
        .open(assets.join(feathrise_editor_scene::standalone::validation_fixture::CLOSE_DOCUMENT_PATH))
        .and_then(|mut file| file.write_all("自有工程的关闭验收文本\n".as_bytes()))
        .map_err(|error| format!("关闭验收文本不能独占创建：{error}"))?;
    Ok(())
}

/// 同时要求原工具历史、原保存后的磁盘源、正式游戏实例；任何一端缺失都不能交付通过。
pub(super) fn verify(root: &Path, game: &Value, proof: &Value) -> Result<(), String> {
    if proof["complete"] != true || !proof["error"].is_null() {
        return Err(format!("专业作者域未全部完成：{proof}"));
    }
    let terrain = &proof["terrain"];
    let strokes = terrain["strokes"]
        .as_array()
        .ok_or("缺少原地形/地表/Blade笔划")?;
    if terrain["complete"] != true || strokes.len() != 3 {
        return Err("三个原地形作者笔划未全部完成".into());
    }
    for (stroke, tool) in strokes.iter().zip(["Raise", "Surface", "Blade"]) {
        if stroke["tool"] != tool
            || stroke["history_entries"] != 1
            || stroke["undo_matches_before"] != true
            || stroke["redo_matches_after"] != true
            || stroke["live_preview_ready"] != true
            || stroke["before"]["sha256"] == stroke["after"]["sha256"]
            || stroke["after"]["nonzero_samples"].as_u64().unwrap_or(0) == 0
        {
            return Err(format!(
                "{tool}缺少非空原笔划/唯一历史/精确UndoRedo/实时采用证据：{stroke}"
            ));
        }
        for file in stroke["after"]["files"]
            .as_array()
            .ok_or("笔划缺少源指纹")?
        {
            let path = file[0].as_str().ok_or("笔划源路径缺失")?;
            if tool == "Raise" {
                let scene = feathrise_scene_content::SceneId::parse(terrain["scene_id"].as_str().ok_or("作者验收场景身份缺失")?)?;
                let target = feathrise_scene_content::TerrainId::parse(terrain["terrain_id"].as_str().ok_or("作者验收地形身份缺失")?)?;
                let actual = feathrise_runtime::project::validation_height_fingerprint(&root.join("assets"), &scene, &target, path)?;
                if file[1] != actual { return Err(format!("分页高度作者源已变化或未保存：{path}")); }
            } else if !path.contains('#') {
                verify_hash(root, path, &file[1])?;
            }
        }
    }
    let actual = &game["author_tools"]["Ok"];
    let height = &strokes[0]["after"]["files"][0];
    if actual["terrain_id"] != terrain["terrain_id"]
        || actual["height_source_sha256"] != height[1]
        || actual["height_source_path"] != height[0]
        || actual["collision_height_at_sculpt_center"]
            .as_f64()
            .is_none_or(|height| height <= 0.)
    {
        return Err(format!("运行实例实际分页高度/碰撞没有采用雕刻结果：{actual}"));
    }
    let object = game["terrain_objects"]
        .as_array()
        .and_then(|objects| {
            objects
                .iter()
                .find(|object| object["terrain_id"] == terrain["terrain_id"])
        })
        .ok_or("运行实例缺少笔划地形实例")?;
    if object["vegetation"]["blade_layers"].as_u64().unwrap_or(0) == 0
        || object["vegetation"]["blade_windows"].as_u64().unwrap_or(0) < 4
    {
        return Err(format!("运行实例未建立正式Blade四级窗口：{object}"));
    }
    verify_river_building(root, actual, &proof["river_building"])?;
    verify_animation_assets(root, game, actual, &proof["animation_assets"])?;
    Ok(())
}

fn verify_river_building(root: &Path, actual: &Value, proof: &Value) -> Result<(), String> {
    if proof["complete"] != true
        || proof["river_undo_redo"] != true
        || proof["building_undo_redo"] != true
    {
        return Err(format!("原曲线河流/建筑历史不完整：{proof}"));
    }
    let water = read_json(
        root,
        proof["water_source_path"]
            .as_str()
            .ok_or("水网正式源缺失")?,
    )?;
    if !water["rivers"].as_array().is_some_and(|rivers| {
        rivers
            .iter()
            .any(|river| river_matches(river, &proof["river"]))
    }) {
        return Err("原场景Save没有保存同一河流的全部点与水参数".into());
    }
    if actual["water"]["phase"] != "Ready"
        || actual["water"]["desired_generation"] != actual["water"]["applied_generation"]
        || !actual["river_surfaces"].as_array().is_some_and(|surfaces| {
            surfaces.iter().any(|surface| {
                surface["id"] == proof["river"]["id"]
                    && surface["vertices"].as_u64().unwrap_or(0) > 0
                    && surface["triangles"].as_u64().unwrap_or(0) > 0
            })
        })
    {
        return Err(format!("运行实例没有采用正式曲线水面：{actual}"));
    }
    let building = actual["rendered_objects"]
        .as_array()
        .and_then(|objects| {
            objects
                .iter()
                .find(|object| object["id"] == proof["building"]["id"])
        })
        .ok_or("运行实例缺少原建筑稳定ID的可视根")?;
    let entry = placement_entry(root, &proof["scene_id"])?;
    if building["building"]["matches_saved_record"] != true
        || building["building"]["pose_matches_resolved_record"] != true
        || !building_matches(&building["building"]["record"], &proof["building"], &entry)
        || !building["meshes"]
            .as_array()
            .is_some_and(|meshes| !meshes.is_empty())
    {
        return Err(format!("运行实例建筑没有采用完整保存记录：{building}"));
    }
    let source: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(building["building"]["record"].clone())
        .map_err(|error| format!("运行实例保存来源记录无效：{error}"))?;
    let manifest: feathrise_scene_content::SceneManifestAsset = serde_json::from_value(read_json(root,
        &entry.manifest_path())?).map_err(|error| error.to_string())?;
    let mut resolved = source;
    // 以落盘清单独立重建关联期望；运行端不能借用另一实例的姿态证明当前保存来源。
    feathrise_scene_content::TerrainBindingIndex::from_manifest(&manifest).resolve_placement(&mut resolved)?;
    if serde_json::from_value::<feathrise_scene_content::ScenePlacementRecord>(building["building"]["resolved_record"].clone())
        .ok().as_ref() != Some(&resolved) {
        return Err("运行实例建筑关联解析与保存清单不一致".into());
    }
    Ok(())
}

fn verify_animation_assets(
    root: &Path,
    game: &Value,
    actual: &Value,
    proof: &Value,
) -> Result<(), String> {
    let animation = &proof["animation"];
    let assets = &proof["asset_hot_reload"];
    if proof["passed"] != true
        || proof["scene_preview_restored"] != true
        || animation["saved"]["dirty"] != false
        || animation["saved"]["save_receipt"] != serde_json::json!({"Ok":null})
        || animation["edited"]["key"] != animation["redone"]["key"]
        || animation["before"]["key"] != animation["undone"]["key"]
        || animation["sha256_before"] == animation["sha256_after"]
    {
        return Err(format!("原动画编辑/UndoRedo/保存回执不完整：{animation}"));
    }
    let model_hash = assets["model_sha256"]
        .as_array()
        .and_then(|hashes| hashes.last())
        .ok_or("缺少最终模型SHA")?;
    for (path, expected) in [
        (proof["animation_path"].as_str(), &animation["sha256_after"]),
        (assets["model_path"].as_str(), model_hash),
        (assets["texture_path"].as_str(), &assets["texture_sha256"]),
    ] {
        let path = path.ok_or("热更资产相对路径缺失")?;
        verify_hash(root, path, expected)?;
        if !game["source"]["files"].as_array().is_some_and(|files| {
            files
                .iter()
                .any(|file| file["path"] == path && &file["sha256"] == expected)
        }) {
            return Err(format!("正式工程实例没有读取最新热更源：{path}"));
        }
    }
    let expected = &assets["texture_reloaded"];
    let matches = actual["rendered_objects"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|object| object["meshes"].as_array().into_iter().flatten())
        .any(|mesh| {
            near(&mesh["positions"], &expected["positions"])
                && near(&mesh["base_color_linear"], &expected["base_color_linear"])
                && near(&mesh["roughness"], &expected["roughness"])
                && mesh["image_size"] == expected["image_size"]
                && mesh["first_pixel"] == expected["first_pixel"]
        });
    if !matches {
        return Err(format!("运行实例实际Mesh/材质/Image没有采用热更结果：{actual}"));
    }
    Ok(())
}

/// 重开后的两个作者World各自回读当前文档与本工程实际分页源。
pub(super) fn verify_reopen(root: &Path, document: &Value, proof: &Value, same: bool) -> Result<(), String> {
    let terrain = &proof["terrain"];
    let objects = document["terrains"]
        .as_array()
        .ok_or("重开缺少原地形作者文档")?;
    let object = objects
        .iter()
        .find(|object| object["id"] == terrain["terrain_id"])
        .ok_or("重开没有同稳定ID地形")?;
    let height = &terrain["strokes"][0];
    let expected = &height[if same { "after" } else { "before" }]["files"][0];
    let height_path = expected[0].as_str().ok_or("缺少地形分页源证据")?;
    let scene = feathrise_scene_content::SceneId::parse(document["scene_id"].as_str().ok_or("重开缺少场景身份")?)?;
    let target = feathrise_scene_content::TerrainId::parse(object["id"].as_str().ok_or("重开缺少地形身份")?)?;
    let source_path = object["terrain"]["height_path"].as_str().ok_or("重开缺少Clipmap作者清单")?;
    if object["terrain"]["kind"] != "sparse_clipmap" || source_path != height_path {
        return Err(format!("重开未采用当前地形的分页清单：{source_path}"));
    }
    // 两个工程的同模板相对清单路径相同。各自回读实际作者页，确认 A 保存值与 B 原始值。
    let actual = feathrise_runtime::project::validation_height_fingerprint(&root.join("assets"), &scene, &target, source_path)?;
    if expected[1] != actual {
        return Err(format!("重开的工程分页高度与{}证据不符：{source_path}", if same { "保存后" } else { "编辑前" }));
    }
    let river = &proof["river_building"]["river"];
    let building = &proof["river_building"]["building"];
    let entry = placement_entry(root, &document["scene_id"])?;
    let manifest: feathrise_scene_content::SceneManifestAsset = serde_json::from_value(read_json(root,
        &entry.manifest_path())?).map_err(|error| error.to_string())?;
    let bindings = feathrise_scene_content::TerrainBindingIndex::from_manifest(&manifest);
    let river_found = document["water"]["rivers"]
        .as_array()
        .is_some_and(|rivers| rivers.iter().any(|item| river_matches(item, river)));
    let building_found = document["placements"]
        .as_array()
        .is_some_and(|placements| placements.iter().any(|item| reopened_building_matches(item, building, &entry, &bindings)));
    let blade_found = document["vegetation"]["layers"]
        .as_array()
        .is_some_and(|layers| {
            layers
                .iter()
                .any(|layer| layer["id"] == terrain["blade_output_id"])
        });
    if same {
        if !river_found
            || !building_found
            || !blade_found
            || !document["surface"]["layers"]
                .as_array()
                .is_some_and(|layers| {
                    layers
                        .iter()
                        .any(|layer| layer["id"] == terrain["surface_layer_id"])
                })
        {
            return Err(format!(
                "A重开未恢复地形/地表/Blade/河流/建筑文档：{document}"
            ));
        }
    } else if river_found || building_found || blade_found {
        return Err("B串用了A的作者修改".into());
    }
    Ok(())
}

/// 同一份已编辑工程再走普通cargo run，防止F12开发标记掩盖正式游戏对作者文件的读取差异。
pub(super) fn independent(
    root: &Path,
    target: &Path,
    output: &Path,
    control: &crate::build_process::BuildControl,
) -> Result<Value, String> {
    let project = crate::project::open(root)?;
    fs::create_dir(output).map_err(|error| error.to_string())?;
    let mut command = std::process::Command::new(
        std::env::var_os("EDITOR_HOST_CARGO")
            .or_else(|| std::env::var_os("CARGO"))
            .unwrap_or_else(|| "cargo".into()),
    );
    command
        .args(["run", "--offline", "--locked", "--manifest-path"])
        .arg(&project.manifest)
        .current_dir(&project.root)
        .env("CARGO_TARGET_DIR", target)
        .env("NEW_EDITOR_GAME_SMOKE_DIR", output)
        .env("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS", "1");
    for name in [
        "EDITOR_SAVED_PLAY",
        "EDITOR_SAVED_PLAY_SCENE",
        "NEW_EDITOR_NATIVE_SMOKE_DIR",
        "NEW_EDITOR_NATIVE_SMOKE_PLAY",
        "NEW_EDITOR_NATIVE_SMOKE_SAVE",
        "NEW_EDITOR_NATIVE_SMOKE_AUTHORING",
        "NEW_EDITOR_NATIVE_SMOKE_RECOVERY",
        "NEW_EDITOR_NATIVE_SMOKE_CLOSE",
        "NEW_EDITOR_NATIVE_SMOKE_SCENE_TOOLS",
    ] {
        command.env_remove(name);
    }
    let process = crate::build_process::run(
        &command,
        control,
        Some(std::time::Duration::from_secs(1900)),
        super::tail,
    )?;
    fs::write(
        output.join("process.log"),
        format!("{}\n{}", process.stdout, process.stderr),
    )
    .map_err(|error| error.to_string())?;
    if process.exit_code != 0 {
        return Err(format!(
            "已编辑工程普通cargo run失败{}：{}\n{}",
            process.exit_code, process.stdout, process.stderr
        ));
    }
    super::verify_game(root, output)
}

fn near(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| near(left, right))
        }
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .is_some_and(|(left, right)| (left - right).abs() < 0.00001),
        _ => left == right,
    }
}

/// JSON内存Value会把f32扩展为f64，文件则使用最短往返小数；按正式f32合同逐字段精确比较，不用容差放过真实修改。
fn river_matches(left: &Value, right: &Value) -> bool {
    match (
        serde_json::from_value::<feathrise_scene_content::SceneWaterRiver>(left.clone()),
        serde_json::from_value::<feathrise_scene_content::SceneWaterRiver>(right.clone()),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn placement_entry(root: &Path, scene: &Value) -> Result<feathrise_scene_content::SceneCatalogEntry, String> {
    let scene = feathrise_scene_content::SceneId::parse(scene.as_str().ok_or("建筑场景身份缺失")?)?;
    let catalog: feathrise_scene_content::SceneCatalogAsset = serde_json::from_value(read_json(root,
        feathrise_scene_content::SCENE_CATALOG_ASSET_PATH)?).map_err(|error| error.to_string())?;
    catalog.scenes.into_iter().find(|entry| entry.id == scene).ok_or("保存目录缺少建筑所属场景".into())
}

/// 只按实际目录的保存格式比较：JSON保留原f32；GLB接受同一生产TRS往返的精确结果，不设容差。
fn building_matches(actual: &Value, author: &Value, entry: &feathrise_scene_content::SceneCatalogEntry) -> bool {
    let result = (|| -> Result<bool, String> {
        let actual: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(actual.clone()).map_err(|error| error.to_string())?;
        Ok(actual == saved_building_record(author, entry)?)
    })();
    result == Ok(true)
}

fn saved_building_record(author: &Value, entry: &feathrise_scene_content::SceneCatalogEntry) -> Result<feathrise_scene_content::ScenePlacementRecord, String> {
    let author = serde_json::from_value(author.clone()).map_err(|error| error.to_string())?;
    if !entry.placements_glb { return Ok(author); }
    let path = entry.placements_path();
    let document = feathrise_scene_content::ScenePlacementsAsset { scene_id: entry.id.clone(), placements: vec![author], ..Default::default() };
    let bytes = feathrise_scene_content::write_scene_placements_glb(&document, &path, None)?;
    let mut saved = feathrise_scene_content::read_scene_placements(&bytes, &path)?;
    if saved.placements.len() != 1 { return Err("建筑保存往返未返回唯一原记录".into()); }
    Ok(saved.placements.pop().unwrap())
}

/// 重开作者草稿会按当前地形关联覆盖TRS；先走唯一保存格式再正式解析，不能同时接受原始和解析两套值。
fn reopened_building_matches(actual: &Value, author: &Value, entry: &feathrise_scene_content::SceneCatalogEntry,
    bindings: &feathrise_scene_content::TerrainBindingIndex) -> bool {
    let result = (|| -> Result<bool, String> {
        let actual: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(actual.clone()).map_err(|error| error.to_string())?;
        let mut expected = saved_building_record(author, entry)?;
        bindings.resolve_placement(&mut expected)?;
        Ok(actual == expected)
    })();
    result == Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn building_roundtrip() -> (Value, Value, feathrise_scene_content::SceneCatalogEntry) {
        let scene = feathrise_scene_content::SceneId::parse("M1-building").unwrap();
        let record = serde_json::from_value(serde_json::json!({
            "id":"building", "asset_path":"scene/probe.glb", "building_preset_id":"sample-building",
            "translation":[-16.,0.,-16.], "yaw_degrees":90., "scale":[1.,1.,1.],
            "terrain_binding":{"terrain_id":"starter-ground", "local_pose":{
                "translation":[-16.,0.,-16.], "yaw_degrees":90., "scale":[1.,1.,1.]
            }}
        })).unwrap();
        let document = feathrise_scene_content::ScenePlacementsAsset { scene_id: scene.clone(), placements: vec![record], ..Default::default() };
        let path = "scenes/M1-building/scene.glb";
        let bytes = feathrise_scene_content::write_scene_placements_glb(&document, path, None).unwrap();
        let saved = feathrise_scene_content::read_scene_placements(&bytes, path).unwrap();
        let entry = serde_json::from_value(serde_json::json!({"id":scene, "display_name":"自有建筑夹具",
            "folder":"scenes/M1-building", "placements_glb":true})).unwrap();
        (serde_json::to_value(&saved.placements[0]).unwrap(), serde_json::to_value(&document.placements[0]).unwrap(), entry)
    }

    fn building_bindings(pose: feathrise_scene_content::TerrainObjectTransform) -> feathrise_scene_content::TerrainBindingIndex {
        let object = serde_json::from_value(serde_json::json!({
            "id":"starter-ground", "display_name":"自有承托夹具", "transform":pose,
            "world_min":[-32.,-32.], "world_max":[32.,32.],
            "terrain":feathrise_scene_content::SceneTerrainDescriptor::empty()
        })).unwrap();
        feathrise_scene_content::TerrainBindingIndex::from_objects(&[object])
    }

    #[test]
    fn reopened_building_comparison_resolves_saved_binding_without_accepting_raw_glb_pose() {
        let (saved, author, entry) = building_roundtrip();
        for pose in [Default::default(), feathrise_scene_content::TerrainObjectTransform {
            translation: [10., 2., 6.], scale: [2., 3., 4.], ..Default::default()
        }] {
            let bindings = building_bindings(pose);
            let mut reopened: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(saved.clone()).unwrap();
            bindings.resolve_placement(&mut reopened).unwrap();
            let reopened = serde_json::to_value(reopened).unwrap();
            assert_ne!(reopened["scale"], saved["scale"], "真实关联解析覆盖GLB原始解码TRS");
            assert!(reopened_building_matches(&reopened, &author, &entry, &bindings));
            assert!(!reopened_building_matches(&saved, &author, &entry, &bindings));
            assert!(building_matches(&saved, &author, &entry), "磁盘来源仍只接受原保存往返");
            assert!(!building_matches(&reopened, &author, &entry));
        }
        assert!(!reopened_building_matches(&author, &author, &entry, &Default::default()), "缺失承托地形不能沿用作者姿态");
    }

    #[test]
    fn reopened_building_comparison_rejects_metadata_binding_and_single_f32_changes() {
        let (saved, author, entry) = building_roundtrip();
        let bindings = building_bindings(Default::default());
        let mut reopened: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(saved).unwrap();
        bindings.resolve_placement(&mut reopened).unwrap();
        for variant in 0..8 {
            let mut changed = reopened.clone();
            match variant {
                0 => changed.id.push_str("-other"),
                1 => changed.asset_path = "scene/other.glb".into(),
                2 => changed.building_preset_id = Some("other-building".into()),
                3 => changed.terrain_binding.as_mut().unwrap().terrain_id = feathrise_scene_content::TerrainId::parse("other-ground").unwrap(),
                4 => changed.translation[0] = f32::from_bits(changed.translation[0].to_bits() + 1),
                5 => changed.yaw_degrees = f32::from_bits(changed.yaw_degrees.to_bits() + 1),
                6 => changed.scale[0] = f32::from_bits(changed.scale[0].to_bits() + 1),
                _ => {
                    let local = &mut changed.terrain_binding.as_mut().unwrap().local_pose;
                    local.translation[0] = f32::from_bits(local.translation[0].to_bits() + 1);
                }
            }
            assert!(!reopened_building_matches(&serde_json::to_value(changed).unwrap(), &author, &entry, &bindings), "case {variant}");
        }
    }

    #[test]
    fn reopened_building_comparison_keeps_unbound_glb_and_json_format_contracts() {
        let (_, mut author, mut entry) = building_roundtrip();
        author.as_object_mut().unwrap().remove("terrain_binding");
        let saved = serde_json::to_value(saved_building_record(&author, &entry).unwrap()).unwrap();
        let bindings = building_bindings(Default::default());
        assert!(reopened_building_matches(&saved, &author, &entry, &bindings));
        assert!(!reopened_building_matches(&author, &author, &entry, &bindings), "无关联GLB不能按原作者TRS蒙混通过");
        entry.placements_glb = false;
        assert!(reopened_building_matches(&author, &author, &entry, &bindings));
        assert!(!reopened_building_matches(&saved, &author, &entry, &bindings), "JSON不能接受GLB解码值");
    }

    #[test]
    fn building_comparison_uses_exact_production_glb_roundtrip() {
        let (saved, author, scene) = building_roundtrip();
        assert_ne!(saved["scale"], author["scale"], "复现90度GLB四元数解码的一个f32位规范化");
        assert!(building_matches(&saved, &author, &scene));
        let disk: Value = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert!(building_matches(&disk, &author, &scene));
    }

    #[test]
    fn building_comparison_json_preserves_exact_author_pose_without_glb_normalization() {
        let (glb, author, mut entry) = building_roundtrip();
        entry.placements_glb = false;
        let json: Value = serde_json::from_str(&serde_json::to_string(&author).unwrap()).unwrap();
        assert!(building_matches(&json, &author, &entry));
        assert!(!building_matches(&glb, &author, &entry), "JSON目录不能同时接受GLB规范化值");
        let mut changed: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(json).unwrap();
        changed.scale[0] = f32::from_bits(changed.scale[0].to_bits() + 1);
        assert!(!building_matches(&serde_json::to_value(changed).unwrap(), &author, &entry));
    }

    #[test]
    fn building_comparison_rejects_changed_metadata_binding_and_single_float_step() {
        let (saved, author, scene) = building_roundtrip();
        let record: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(saved).unwrap();
        for variant in 0..7 {
            let mut changed = record.clone();
            match variant {
                0 => changed.id.push_str("-other"),
                1 => changed.asset_path = "scene/other.glb".into(),
                2 => changed.building_preset_id = Some("other-building".into()),
                3 => changed.terrain_binding.as_mut().unwrap().terrain_id = feathrise_scene_content::TerrainId::parse("other-ground").unwrap(),
                4 => changed.translation[0] = f32::from_bits(changed.translation[0].to_bits() + 1),
                5 => changed.yaw_degrees = f32::from_bits(changed.yaw_degrees.to_bits() + 1),
                _ => changed.scale[0] = f32::from_bits(changed.scale[0].to_bits() + 1),
            }
            assert!(!building_matches(&serde_json::to_value(changed).unwrap(), &author, &scene), "case {variant}");
        }
    }

    #[test]
    fn river_comparison_keeps_exact_f32_identity_across_json_roundtrip() {
        let river: feathrise_scene_content::SceneWaterRiver = serde_json::from_value(serde_json::json!({
            "id":"river", "label":"河流", "points":[{
                "id":"point", "position":[-16.650642,0.05,5.314064],
                "width":6.0, "depth":1.5, "flow_speed":2.0, "outgoing":"river"
            }], "base_color_srgba":[0.055,0.24,0.29,0.72], "perceptual_roughness":0.06
        }))
        .unwrap();
        let proof = serde_json::to_value(&river).unwrap();
        let saved: Value = serde_json::from_str(&serde_json::to_string(&river).unwrap()).unwrap();
        assert_ne!(proof, saved, "重现内存与磁盘JSON的数字表示差异");
        assert!(river_matches(&proof, &saved));
        let mut changed = river;
        let original = changed.points[0].position[1];
        changed.points[0].position[1] = f32::from_bits(original.to_bits() + 1);
        assert!(
            !river_matches(&proof, &serde_json::to_value(changed).unwrap()),
            "真实变化一个f32步长也不能被当成格式差异忽略"
        );
    }
}

fn asset_file(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    let assets = root
        .join("assets")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let path = assets
        .join(relative)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.starts_with(&assets) || !path.is_file() {
        return Err("验收源越出本工程资产根".into());
    }
    Ok(path)
}

fn verify_hash(root: &Path, relative: &str, expected: &Value) -> Result<(), String> {
    let bytes = fs::read(asset_file(root, relative)?).map_err(|error| error.to_string())?;
    let actual = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if expected != &actual {
        return Err(format!("专业作者正式源已变化或未保存：{relative}"));
    }
    Ok(())
}

fn read_json(root: &Path, relative: &str) -> Result<Value, String> {
    serde_json::from_slice(
        &fs::read(asset_file(root, relative)?).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}
