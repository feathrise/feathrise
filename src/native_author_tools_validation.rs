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
    // 普通文本关闭事务与游戏地图/地表源分离，关闭重试不会改写此前F12的内容证据。
    use std::io::Write;
    fs::OpenOptions::new().write(true).create_new(true)
        .open(assets.join(feathrise_editor_map::standalone::validation_fixture::CLOSE_DOCUMENT_PATH))
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
            if !path.contains('#') {
                verify_hash(root, path, &file[1])?;
            }
        }
    }
    let actual = &game["author_tools"]["Ok"];
    let height = &strokes[0]["after"]["files"][0];
    if actual["terrain_id"] != terrain["terrain_id"]
        || actual["mesh_sha256"] != height[1]
        || actual["collision_height_at_sculpt_center"]
            .as_f64()
            .is_none_or(|height| height <= 0.)
    {
        return Err(format!("运行实例实际网格/碰撞没有采用雕刻结果：{actual}"));
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
        return Err("原地图Save没有保存同一河流的全部点与水参数".into());
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
    if building["building"]["matches_saved_record"] != true
        || building["building"]["record"] != proof["building"]
        || !building["meshes"]
            .as_array()
            .is_some_and(|meshes| !meshes.is_empty())
    {
        return Err(format!("运行实例建筑没有采用完整保存记录：{building}"));
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
        || proof["map_preview_restored"] != true
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

/// 重开的原作者World应读取同一文档；B不得串用A的新增河流/建筑/Blade层。
pub(super) fn verify_reopen(document: &Value, proof: &Value, same: bool) -> Result<(), String> {
    let terrain = &proof["terrain"];
    let objects = document["terrains"]
        .as_array()
        .ok_or("重开缺少原地形作者文档")?;
    let object = objects
        .iter()
        .find(|object| object["id"] == terrain["terrain_id"])
        .ok_or("重开没有同稳定ID地形")?;
    let height_path = terrain["strokes"][0]["after"]["files"][0][0]
        .as_str()
        .and_then(|path| path.split('#').next())
        .ok_or("缺少最终地形版本源")?;
    let river = &proof["river_building"]["river"];
    let building = &proof["river_building"]["building"];
    let river_found = document["water"]["rivers"]
        .as_array()
        .is_some_and(|rivers| rivers.iter().any(|item| river_matches(item, river)));
    let building_found = document["placements"]
        .as_array()
        .is_some_and(|placements| placements.iter().any(|item| item == building));
    let blade_found = document["vegetation"]["layers"]
        .as_array()
        .is_some_and(|layers| {
            layers
                .iter()
                .any(|layer| layer["id"] == terrain["blade_output_id"])
        });
    if same {
        if object["terrain"]["glb_path"] != height_path
            || !river_found
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
    } else if river_found
        || building_found
        || blade_found
        || object["terrain"]["glb_path"] == height_path
    {
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
        "EDITOR_SAVED_PLAY_MAP",
        "NEW_EDITOR_NATIVE_SMOKE_DIR",
        "NEW_EDITOR_NATIVE_SMOKE_PLAY",
        "NEW_EDITOR_NATIVE_SMOKE_SAVE",
        "NEW_EDITOR_NATIVE_SMOKE_AUTHORING",
        "NEW_EDITOR_NATIVE_SMOKE_RECOVERY",
        "NEW_EDITOR_NATIVE_SMOKE_CLOSE",
        "NEW_EDITOR_NATIVE_SMOKE_MAP_TOOLS",
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
        serde_json::from_value::<feathrise_map_content::MapWaterRiver>(left.clone()),
        serde_json::from_value::<feathrise_map_content::MapWaterRiver>(right.clone()),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn river_comparison_keeps_exact_f32_identity_across_json_roundtrip() {
        let river: feathrise_map_content::MapWaterRiver = serde_json::from_value(serde_json::json!({
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
