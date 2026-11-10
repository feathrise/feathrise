//! 自有夹具的完整采用证据：回执修订相同还不够，静态实例必须仍处于作者最终位置。
use serde_json::Value;

pub(super) fn verify(capture: &Value, author: &Value) -> Result<(), String> {
    let expected = author["placements"].as_array().ok_or("作者证据缺少实例")?;
    let actual = capture["placements"].as_array().ok_or("运行截图未记录真实实例")?;
    if expected.len() > 64 || actual.len() > 64 || expected.len() != actual.len() {
        return Err(format!("完整采用后的实例数量不符：作者{}，运行实例{}", expected.len(), actual.len()));
    }
    for placement in expected {
        let id = placement["id"].as_str().ok_or("作者实例缺少稳定ID")?;
        let mut matches = actual.iter().filter(|actual| actual["id"] == id);
        let instance = matches.next().ok_or_else(|| format!("运行截图缺少作者实例{id}"))?;
        if matches.next().is_some() || instance["scene_id"] != "M1-SAMPLE"
            || instance["position"] != placement["translation"]
        {
            return Err(format!("完整采用后的实例{id}位置/归属不符：作者{}，运行实例{}，场景{}",
                placement["translation"], instance["position"], instance["scene_id"]));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn full_snapshot_cannot_restore_a_cancelled_drag_value() {
        let author = json!({"placements":[{"id":"cube","translation":[9.0,1.0,-2.0]}]});
        let mut capture = json!({"revision":48,"placements":[{"id":"cube","scene_id":"M1-SAMPLE","position":[10.0,1.0,-2.0]}]});
        assert!(verify(&capture, &author).is_err());
        capture["placements"][0]["position"] = json!([9.0,1.0,-2.0]);
        assert!(verify(&capture, &author).is_ok());
    }

    #[test]
    fn removed_or_duplicate_instances_do_not_pass_snapshot_evidence() {
        let empty = json!({"placements":[]});
        let cube = json!({"id":"cube","scene_id":"M1-SAMPLE","position":[9.0,1.0,-2.0]});
        assert!(verify(&json!({"placements":[cube.clone()]}), &empty).is_err());
        let author = json!({"placements":[{"id":"cube","translation":[9.0,1.0,-2.0]},
            {"id":"second","translation":[0.0,0.0,0.0]}]});
        assert!(verify(&json!({"placements":[cube.clone(),cube]}), &author).is_err());
    }
}
