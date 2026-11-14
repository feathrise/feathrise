//! 自有模型种植夹具的落盘、正式游戏和重开核对；每项证据均来自原作者事务。
use feathrise_scene_content::{ModelSurfaceVegetation, ScenePlacementRecord, SceneVegetationAsset, SurfaceTarget};
use serde_json::Value;
use std::{fs, path::Path};

use super::fixture::{MODEL_SURFACE_ID, MODEL_SURFACE_SOURCE, verify_private_model_surface};

fn enabled() -> bool {
    std::env::var("FEATHRISE_MODEL_SURFACE_VALIDATION").as_deref() == Ok("1")
}

/// 主文档必须只保留块引用，正式读取再还原同一命中层；不以作者内存代替保存结果。
pub(super) fn verify_saved(root: &Path, game: &Value, proof: &Value) -> Result<SceneVegetationAsset, String> {
    let assets = root.join("assets");
    let folder = "scenes/M1-SAMPLE";
    let mut document: SceneVegetationAsset = serde_json::from_slice(
        &fs::read(assets.join(folder).join("vegetation.scene_vegetation.json"))
            .map_err(|error|format!("模型植被主文档未保存：{error}"))?
    ).map_err(|error|error.to_string())?;
    let target = document.model_surfaces.iter().find(|surface| matches!(&surface.target,
        SurfaceTarget::Model { object_id } if object_id == MODEL_SURFACE_ID))
        .ok_or("主文档没有模型种植目标")?;
    if !target.samples.is_empty() || target.sample_blocks.is_empty() {
        return Err("模型样本没有通过原事务写入私有分块".into());
    }
    feathrise_scene_content::load_model_vegetation_blocks(&assets,folder,&mut document)?;
    let target = document.model_surfaces.iter().find(|surface| matches!(&surface.target,
        SurfaceTarget::Model { object_id } if object_id == MODEL_SURFACE_ID)).unwrap();
    verify_target(target, proof)?;
    let placement = verify_private_model_surface(&assets, target)?;
    let mut active = game["placement_instances"].as_array().ok_or("F12 缺少实际模型实例目录")?
        .iter().filter(|entry| entry["id"] == placement.id);
    if active.next().is_none_or(|entry| entry["asset_target"] != serde_json::json!(placement.asset_target)
        || entry["position"] != serde_json::json!(placement.translation)) || active.next().is_some()
    {
        return Err("F12 没有采用保存的私有 GLB 模型摆放".into());
    }
    verify_runtime(game)?;
    Ok(document)
}

/// 保存后按正式块身份完整读取，再与截图时后台冻结的目标比较；不能以“仍有非零值”代替无损保存。
fn verify_target(target: &ModelSurfaceVegetation, proof: &Value) -> Result<(), String> {
    let expected_samples = proof["samples"].as_u64().filter(|count| (1..=4096).contains(count))
        .ok_or("保存前模型样本数量证据缺失或超出夹具预算")?;
    if target.samples.len() as u64 != expected_samples {
        return Err(format!("模型样本数量保存前后不一致：保存前{expected_samples}，落盘{}", target.samples.len()));
    }
    if target.samples.iter().any(|sample| sample.anchor.primitive != 0
        || !(2..4).contains(&sample.anchor.triangle)) {
        return Err("落盘的模型样本串入桥下表面".into());
    }
    let layers = proof["layers"].as_array().ok_or("缺少模型Blade与Mesh原操作证据")?;
    for kind in ["blade","mesh"] {
        let layer = layers.iter().find(|layer|layer["kind"] == kind).ok_or("模型后端验收不完整")?;
        let id = layer["layer"].as_str().ok_or("模型作者层ID缺失")?;
        let expected_sum = layer["planted_density_sum"].as_u64().filter(|sum| *sum > 0)
            .ok_or_else(|| format!("模型{kind}层缺少非零原笔划密度证据"))?;
        let actual_sum: u64 = target.samples.iter().flat_map(|sample| &sample.layers)
            .filter(|(candidate, _)| candidate == id).map(|(_, value)| u64::from(*value)).sum();
        if layer["undo_redo"] != true || actual_sum != expected_sum {
            return Err(format!("模型{kind}层密度总和保存前后不一致或缺少撤销重做：保存前{expected_sum}，落盘{actual_sum}"));
        }
    }
    // 完整目标摘要同时覆盖稳定ID、拓扑、格距、每个三角锚点/重心坐标与全部密度，
    // 因此同数量、同总和但锚点移动或密度被重新分配也必须失败。块路径由上游正式读取单独核验。
    let expected_digest = proof["author_target_sha256"].as_str().filter(|digest|
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("保存前完整模型作者目标摘要缺失")?;
    let bytes = serde_json::to_vec(target).map_err(|error| error.to_string())?;
    if feathrise_scene_content::authoring_io::bytes_hash(&bytes) != expected_digest {
        return Err("模型作者目标的锚点、拓扑、格距或逐样本密度保存前后不一致".into());
    }
    Ok(())
}

pub(super) fn verify_initial(root: &Path, output: &Path, game: &Value, proof: &Value) -> Result<(), String> {
    if !enabled() { return Ok(()); }
    if proof["complete"] != true || proof["runtime"]["ready"] != true || proof["source"] != MODEL_SURFACE_SOURCE
        || proof["runtime"]["private_placement_source"] != MODEL_SURFACE_SOURCE
        || proof["runtime"]["private_placement_revision"].as_u64().is_none_or(|revision| revision == 0)
        || !output.join("model-surface.png").is_file() {
        return Err(format!("模型作者窗口缺少完整实际生成/截图回执：{proof}"));
    }
    verify_saved(root,game,proof)?;
    Ok(())
}

fn verify_runtime(game: &Value) -> Result<(), String> {
    let status = &game["model_vegetation"];
    if status["ready"] != true || status["failed"] != false
        || status["scene_id"] != "M1-SAMPLE"
        || status["blade_windows"].as_u64().unwrap_or(0) == 0
        || status["mesh_batches"].as_u64().unwrap_or(0) == 0 {
        return Err(format!("正式游戏没有采用模型草与树灌木实例：{status}"));
    }
    verify_gpu_report(game)?;
    Ok(())
}

/// 父进程独立核对目标四窗的真实GPU样本；其他地形数量和子进程ready不能授予绘制信用。
fn verify_gpu_report(game: &Value) -> Result<(), String> {
    use std::collections::BTreeSet;
    let proof = &game["model_surface_gpu"];
    let selection = &proof["selection"];
    let gpu = &proof["gpu"];
    let render = &proof["render"];
    let ids = |value: &Value| -> Option<BTreeSet<u64>> {
        let values = value.as_array().filter(|values| values.len() == 4)?;
        let ids = values.iter().map(Value::as_u64).collect::<Option<BTreeSet<_>>>()?;
        (ids.len() == 4).then_some(ids)
    };
    let expected = ids(&selection["window_entities"]);
    let sampled = ids(&gpu["sampled_main_entities"]);
    let ticket = selection["ticket"].as_u64().filter(|ticket| *ticket > 0);
    let baseline = selection["sample_baseline"].as_u64();
    let current = game["model_vegetation"]["runtime_revision"].as_u64().filter(|revision| *revision > 0);
    let stages_ready = proof["compute_stages"].as_array().is_some_and(|stages|
        stages.len() == 4 && stages.iter().all(|stage| stage["code"] == 3));
    let rendered = gpu["rendered_instances"].as_u64().unwrap_or(0);
    if proof["ready"] != true || proof["selection_active"] != true
        || selection["object_id"] != MODEL_SURFACE_ID
        || selection["scene_id"] != "M1-SAMPLE" || selection["instance_path"] != "M1-SAMPLE"
        || current.is_none() || selection["runtime_revision"].as_u64() != current
        || expected.is_none() || expected != sampled
        || ticket.is_none() || gpu["selection_ticket"].as_u64() != ticket
        || baseline.is_none() || gpu["sample_sequence"].as_u64().zip(baseline).is_none_or(|(sample, base)| sample <= base)
        || gpu["sampled_windows"] != 4 || gpu["sampled_output_valid_mask"] != 15
        || gpu["truncated_windows"] != 0 || rendered == 0
        || gpu["surface_survivors"].as_u64().unwrap_or(0) < rendered
        || gpu["overflow_instances"] != 0 || gpu["scene_errors"] != 0
        || !stages_ready || render["drawn_draws"].as_u64().unwrap_or(0) == 0
        || render["compute_pipeline_error_stages"] != 0 || render["pipeline_error_draws"] != 0
        || render["specialization_errors"] != 0
    {
        return Err(format!("正式游戏缺少当前模型四窗的完整GPU绘制证据：{proof}"));
    }
    Ok(())
}

/// 比较正式强类型文档，保留f32往返语义；B独立工程不得读到A的模型样本。
pub(super) fn verify_reopen(root: &Path, document: &Value, same: bool) -> Result<(), String> {
    if !enabled() { return Ok(()); }
    let reopened = &document["root_vegetation"];
    if same {
        let assets = root.join("assets");
        let folder = "scenes/M1-SAMPLE";
        let mut saved: SceneVegetationAsset = serde_json::from_slice(
            &fs::read(assets.join(folder).join("vegetation.scene_vegetation.json")).map_err(|error|error.to_string())?
        ).map_err(|error|error.to_string())?;
        feathrise_scene_content::load_model_vegetation_blocks(&assets,folder,&mut saved)?;
        let actual: SceneVegetationAsset = serde_json::from_value(reopened.clone()).map_err(|error|format!("重开根植被缺失：{error}"))?;
        if actual.model_surfaces != saved.model_surfaces || actual.authoring != saved.authoring {
            return Err("重开原作者窗口的模型样本/图层没有匹配实际保存数据".into());
        }
        let density = saved.model_surfaces.iter().find(|surface| matches!(&surface.target,
            SurfaceTarget::Model { object_id } if object_id == MODEL_SURFACE_ID))
            .ok_or("重开缺少已保存的私有 GLB 种植目标")?;
        let placement = verify_private_model_surface(&assets, density)?;
        let actual: Vec<ScenePlacementRecord> = serde_json::from_value(document["placements"].clone())
            .map_err(|error| format!("重开缺少实际模型摆放目录：{error}"))?;
        let mut matches = actual.iter().filter(|entry| entry.id == placement.id);
        if matches.next() != Some(&placement) || matches.next().is_some() {
            return Err("重开原作者窗口没有采用保存的私有 GLB 模型摆放".into());
        }
    } else if reopened["model_surfaces"].as_array().is_some_and(|surfaces|!surfaces.is_empty()) {
        return Err("独立B工程串用了A的模型种植数据".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::{ModelSurfaceAnchor, ModelSurfaceVegetationSample};
    use serde_json::json;

    fn fixture() -> (ModelSurfaceVegetation, Value) {
        let mut target = ModelSurfaceVegetation::empty("model-surface-fixture", [7; 32], 0.25);
        target.samples = vec![
            ModelSurfaceVegetationSample { anchor: ModelSurfaceAnchor {
                primitive: 0, triangle: 2, barycentric: [0.2, 0.3],
            }, layers: vec![("blade-layer".into(), 100), ("mesh-layer".into(), 32)] },
            ModelSurfaceVegetationSample { anchor: ModelSurfaceAnchor {
                primitive: 0, triangle: 3, barycentric: [0.4, 0.1],
            }, layers: vec![("blade-layer".into(), 155), ("mesh-layer".into(), 64)] },
        ];
        let proof = json!({"samples": 2,
            "author_target_sha256": feathrise_scene_content::authoring_io::bytes_hash(&serde_json::to_vec(&target).unwrap()),
            "layers": [
                {"kind": "blade", "layer": "blade-layer", "planted_density_sum": 255, "undo_redo": true},
                {"kind": "mesh", "layer": "mesh-layer", "planted_density_sum": 96, "undo_redo": true},
            ]});
        (target, proof)
    }

    #[test]
    fn saved_target_requires_exact_sample_count_and_layer_density_totals() {
        let (target, proof) = fixture();
        let loaded = serde_json::from_slice::<ModelSurfaceVegetation>(&serde_json::to_vec(&target).unwrap()).unwrap();
        verify_target(&loaded, &proof).unwrap();
        let mut missing = loaded.clone(); missing.samples.pop();
        assert!(verify_target(&missing, &proof).unwrap_err().contains("样本数量保存前后不一致"));
        let mut changed = loaded; changed.samples[0].layers[0].1 += 1;
        assert!(verify_target(&changed, &proof).unwrap_err().contains("模型blade层密度总和保存前后不一致"));
    }

    #[test]
    fn saved_target_digest_refuses_moved_anchors_and_same_total_density_redistribution() {
        let (target, proof) = fixture();
        let mut moved = target.clone(); moved.samples[0].anchor.barycentric[0] += 0.01;
        assert!(verify_target(&moved, &proof).unwrap_err().contains("锚点、拓扑、格距或逐样本密度"));
        let mut redistributed = target;
        redistributed.samples[0].layers[0].1 += 1; redistributed.samples[1].layers[0].1 -= 1;
        assert!(verify_target(&redistributed, &proof).unwrap_err().contains("锚点、拓扑、格距或逐样本密度"));
    }

    #[test]
    fn saved_target_requires_the_presave_background_digest() {
        let (target, mut proof) = fixture();
        proof.as_object_mut().unwrap().remove("author_target_sha256");
        assert!(verify_target(&target, &proof).unwrap_err().contains("完整模型作者目标摘要缺失"));
    }

    #[test]
    fn model_gpu_report_refuses_stale_sibling_duplicate_and_empty_outputs() {
        let game = json!({"model_vegetation":{"runtime_revision":7}, "model_surface_gpu":{
            "ready":true,"selection_active":true,
            "selection":{"object_id":MODEL_SURFACE_ID,"scene_id":"M1-SAMPLE","instance_path":"M1-SAMPLE",
                "runtime_revision":7,"ticket":11,"sample_baseline":19,"window_entities":[10,20,30,40]},
            "gpu":{"sample_sequence":20,"selection_ticket":11,"sampled_main_entities":[40,30,20,10],
                "sampled_windows":4,"sampled_output_valid_mask":15,"truncated_windows":0,
                "surface_survivors":53,"rendered_instances":53,"overflow_instances":0,"scene_errors":0},
            "render":{"drawn_draws":4,"compute_pipeline_error_stages":0,"pipeline_error_draws":0,"specialization_errors":0},
            "compute_stages":[{"code":3},{"code":3},{"code":3},{"code":3}]
        }});
        verify_gpu_report(&game).unwrap();
        for (field, wrong) in [
            ("sample_sequence", json!(19)), ("selection_ticket", json!(10)),
            ("sampled_main_entities", json!([10,20,30,41])),
            ("sampled_main_entities", json!([10,20,30,30])),
            ("sampled_output_valid_mask", json!(7)), ("rendered_instances", json!(0)),
            ("overflow_instances", json!(1)), ("scene_errors", json!(1)),
        ] {
            let mut bad = game.clone();
            bad["model_surface_gpu"]["gpu"][field] = wrong;
            assert!(verify_gpu_report(&bad).is_err(), "必须拒绝错误{field}");
        }
        for (field, wrong) in [("runtime_revision",json!(8)), ("object_id",json!("sibling")),
            ("instance_path",json!("M1-SAMPLE/child"))] {
            let mut bad = game.clone();
            bad["model_surface_gpu"]["selection"][field] = wrong;
            assert!(verify_gpu_report(&bad).is_err(), "不能串用另一采用或目标{field}");
        }
        let mut waiting = game.clone();
        waiting["model_surface_gpu"]["compute_stages"][2]["code"] = json!(2);
        assert!(verify_gpu_report(&waiting).is_err());
        let mut absent = game;
        absent.as_object_mut().unwrap().remove("model_surface_gpu");
        assert!(verify_gpu_report(&absent).is_err(), "没有目标证据不能靠旧ECS回执通过");
    }
}
