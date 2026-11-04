//! 新旧作者工具的共同输入取证；调用旧编辑器正式笔刷核心，独立输出R8，不复制新算法。
//! 仅显式EDITOR_BRUSH_PARITY_REPORT写新报告；本测试不启动窗口，也不代表完整作者事务/GPU验收。
use super::paint::*;
use bevy::prelude::Vec3;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn integrated_brush_records_shared_parity_fixture() {
    // 中性轨迹随作者库保留，独立仓库测试不依赖原私有验证目录。
    let bytes = include_bytes!("../../tests/fixtures/editor-brush-parity-v1.json");
    let spec: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(spec["resolution"], json!([32,32]), "本轮生产原型只比较共同32×32画布");
    let pair = |key: &str| -> [f32; 2] { [0, 1].map(|axis| spec[key][axis].as_f64().unwrap() as f32) };
    let radius = spec["radius"].as_f64().unwrap() as f32;
    let strength = spec["strength"].as_f64().unwrap() as f32;
    let delta = spec["delta_seconds"].as_f64().unwrap() as f32;
    let mut rows = Vec::new();
    for case in spec["cases"].as_array().unwrap() {
        let points: Vec<Vec3> = case["points"].as_array().unwrap().iter()
            .map(|p| Vec3::new(p[0].as_f64().unwrap() as f32, 0.0, p[1].as_f64().unwrap() as f32)).collect();
        let mut density = vec![case["initial"].as_u64().unwrap() as u8; 1024];
        let mut revisions = Vec::new();
        for _ in 0..case["strokes"].as_u64().unwrap() {
            let mut before = BTreeMap::new();
            let mut previous = points[0];
            for (index, point) in points.iter().copied().chain(
                std::iter::repeat_n(*points.last().unwrap(), case["hold_frames"].as_u64().unwrap() as usize)).enumerate() {
                let (amount, exposure) = vegetation_paint_exposure(strength, delta, index == 0);
                let count = interpolated_dab_count(previous, point, radius);
                assert_eq!(count as usize, brush_path_dab_count(previous, point, radius),
                    "共同轨迹须落在旧生产每帧32枚刷印预算内，不能截断后冒充完整结果");
                for dab in 1..=count {
                    paint_density_neighborhood_with_flow(&mut density, [32, 32], pair("world_min"), pair("world_max"),
                        previous.lerp(point, dab as f32 / count as f32), radius, case["erase"].as_bool().unwrap(),
                        amount, exposure, &mut before);
                }
                previous = point;
            }
            revisions.push(json!({"density": density, "changed_cells": before.len(),
                "sha256": digest(&density)}));
        }
        rows.push(json!({"name": case["name"], "strokes": revisions}));
    }
    let density_for = |name: &str| rows.iter().find(|r| r["name"] == name).unwrap()["strokes"][0]["density"].clone();
    assert_ne!(density_for("tap"), density_for("hold"), "旧工具的真实停留曝光必须可观察");
    let repeat = rows.iter().find(|r| r["name"] == "repeat").unwrap();
    assert_ne!(repeat["strokes"][0]["density"], repeat["strokes"][1]["density"], "旧工具重复起笔继续累积");
    let report = json!({"schema": 1, "candidate": "integrated-production-brush-core", "fixture_sha256": digest(bytes),
        "scope": "纯数据语义；无窗口、无GPU、无F12计时", "cases": rows});
    if let Some(path) = std::env::var_os("EDITOR_BRUSH_PARITY_REPORT") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
        file.write_all(serde_json::to_string_pretty(&report).unwrap().as_bytes()).unwrap();
    }
}
