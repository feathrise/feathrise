//! 预览呈现验收读取原UI、相机和历史；独立预览期间作者工作台必须保持完整呈现。

use crate::viewport_mode::ViewportGeometry;
use bevy::prelude::*;
use feathrise_editor_map::standalone::embedded_validation as author;
use serde_json::{Value, json};

const SOURCE: &str = "author Node+Visibility+InheritedVisibility+Camera+selection+history";

fn presentation(world: &mut World) -> Result<Value, String> {
    serde_json::to_value(author::presentation_probe(world)?).map_err(|error| error.to_string())
}

fn matches_mode(probe: &Value, _playing: bool) -> bool {
    let Some(roots) = probe["roots"]
        .as_array()
        .filter(|values| !values.is_empty())
    else {
        return false;
    };
    let Some(headers) = probe["headers"]
        .as_array()
        .filter(|values| !values.is_empty())
    else {
        return false;
    };
    roots.iter().all(|root| {
        root["display"] != "None"
            && root["visibility"] != "Hidden"
            && root["inherited_visible"] == true
    }) && headers
        .iter()
        .all(|header| header["inherited_visible"] == true)
        && probe["camera"]["active"] == true
        && probe["layout"]
                .as_array()
                .is_some_and(|rect| rect.len() == 4)
}

pub(super) fn mode_ready(world: &mut World, playing: bool) -> Result<bool, String> {
    Ok(matches_mode(&presentation(world)?, playing)
        && (!playing || crate::window_chrome::preview_chrome_probe(world)
            .is_some_and(|probe| preview_chrome_ready(&probe))))
}

fn preview_chrome_ready(chrome: &Value) -> bool {
    chrome["layout"]["visible"] == false
        && chrome["active_header"].is_u64()
        && chrome["active_header"] == chrome["regular_header"]
        && chrome["preview_header"].is_null()
        && chrome["camera"].is_null()
        && chrome["captions"].as_array().is_some_and(|captions| {
            captions.len() == 3 && ["minimize", "maximize", "close"].iter().all(|action| {
                captions.iter().filter(|caption| caption["action"] == *action).count() == 1
                    && captions.iter().find(|caption| caption["action"] == *action)
                        .is_some_and(|caption| {
                            let Some(rect) = caption["rect"].as_array().filter(|rect| rect.len()==4) else {return false};
                            caption["visible"] == true && caption["entity"].is_u64()
                                && rect.iter().all(|value| value.as_f64().is_some())
                                && rect[0].as_f64().unwrap() >= 0.0 && rect[1].as_f64().unwrap() >= 0.0
                                && rect[2].as_f64().unwrap() > rect[0].as_f64().unwrap()
                                && rect[3].as_f64().unwrap() > rect[1].as_f64().unwrap()
                        })
            })
        })
}

pub(super) fn capture_baseline(world: &mut World) -> Result<Value, String> {
    let current = presentation(world)?;
    if !matches_mode(&current, false) {
        return Err("F12前作者根、顶栏、相机或实际视口尚未呈现，不能建立恢复基线".into());
    }
    let geometry = world.resource::<ViewportGeometry>();
    Ok(
        json!({"presentation":current,"author":author::probe(world)?,
        "physical_rect":geometry.rect,"excluded_rects":geometry.excluded_rects,"excluded_shapes":geometry.excluded_shapes,
        "window_chrome":crate::window_chrome::preview_chrome_probe(world)}),
    )
}

fn plays(stage: &str) -> bool {
    matches!(
        stage,
        "empty_map_f12_play_same_world"
            | "f12_play_same_world"
            | "restart_adopted_same_unsaved_author"
    )
}

fn restores(stage: &str) -> bool {
    matches!(
        stage,
        "empty_map_f12_returned"
            | "f12_edit_same_world_and_paused_ticks"
            | "recovered_world_returned_to_edit"
            | "back_to_original_editor_3d_with_same_author"
    )
}

fn verify_transition(proof: &Value, stage: &str) -> Result<(), String> {
    if proof["source"] != SOURCE || !matches_mode(&proof["actual"], plays(stage)) {
        return Err(format!("{stage}实际作者UI或相机显隐不符合F12呈现"));
    }
    if plays(stage) || restores(stage) {
        let baseline = &proof["before_f12"];
        if !matches_mode(&baseline["presentation"], false) || proof["author"] != baseline["author"]
        {
            return Err(format!(
                "{stage}缺少实际F12前基线，或切换改变了作者草稿/历史"
            ));
        }
        if plays(stage) {
            let chrome = &proof["window_chrome"];
            if !preview_chrome_ready(chrome)
                || chrome["active_header"] != baseline["window_chrome"]["active_header"]
                || chrome["captions"].as_array().is_none_or(|captions| captions.iter().any(|caption| {
                    baseline["window_chrome"]["captions"].as_array().is_none_or(|original| {
                        !original.iter().any(|item| item["action"]==caption["action"] && item["entity"]==caption["entity"])
                    })
                }))
            {
                return Err(format!("{stage}没有保留原作者标题栏和三键实体"));
            }
        }
        {
            // 本夹具期间没有作者操作，Playing和返回均必须与进入前的实际界面一致。
            for field in [
                "roots",
                "headers",
                "layout",
                "camera",
                "navigation",
                "selection",
                "panes",
            ] {
                if proof["actual"][field] != baseline["presentation"][field] {
                    return Err(format!("{stage}没有恢复F12前的作者{field}状态"));
                }
            }
            if proof["physical_rect"] != baseline["physical_rect"]
                || proof["excluded_rects"] != baseline["excluded_rects"]
                || proof["excluded_shapes"] != baseline["excluded_shapes"]
            {
                return Err(format!(
                    "{stage}没有恢复F12前的中央视口位置、尺寸或控件区域；before physical_rect={} excluded_rects={}；actual physical_rect={} excluded_rects={}",
                    baseline["physical_rect"], baseline["excluded_rects"],
                    proof["physical_rect"], proof["excluded_rects"],
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn record(
    world: &mut World,
    baseline: Option<&Value>,
    stage: &str,
) -> Result<Value, String> {
    let actual = presentation(world)?;
    let geometry = world.resource::<ViewportGeometry>();
    let proof = json!({"source":SOURCE,"actual":actual,"author":author::probe(world)?,
        "physical_rect":geometry.rect,"excluded_rects":geometry.excluded_rects,"excluded_shapes":geometry.excluded_shapes,"before_f12":baseline,
        "window_chrome":crate::window_chrome::preview_chrome_probe(world)});
    verify_transition(&proof, stage)?;
    Ok(proof)
}

/// 最终汇总重新核对实际组件证据，并把每轮基线接回进入预览前已记录的作者阶段。
pub(super) fn verify_records(steps: &[Value], empty_fixture: bool) -> Result<(), String> {
    for step in steps {
        let stage = step["stage"].as_str().ok_or("阶段缺少名称")?;
        verify_transition(&step["author_presentation"], stage)?;
        if step["author"] != step["author_presentation"]["author"] {
            return Err(format!("{stage}呈现与作者历史证据不属于同一观测"));
        }
    }
    let mut transitions = vec![
        (
            // 工具阶段也会推进作者修订，使用点击预览前最后一个实测作者阶段。
            if steps.iter().any(|step|step["stage"]=="continuous_previews_completed") { "continuous_previews_completed" } else { "semantic_tools_rendered_and_cleared" },
            "f12_play_same_world",
            "f12_edit_same_world_and_paused_ticks",
        ),
        (
            "crash_preserved_unsaved_author_and_history",
            "restart_adopted_same_unsaved_author",
            "recovered_world_returned_to_edit",
        ),
    ];
    if empty_fixture {
        transitions.push((
            "resident_game_3d_ready",
            "empty_map_f12_play_same_world",
            "empty_map_f12_returned",
        ));
    }
    for (before, playing, returned) in transitions {
        let find = |stage: &str| {
            steps
                .iter()
                .find(|step| step["stage"] == stage)
                .ok_or_else(|| format!("缺少F12呈现阶段：{stage}"))
        };
        let original = &find(before)?["author_presentation"];
        let play = &find(playing)?["author_presentation"];
        let returned = &find(returned)?["author_presentation"];
        let baseline = &play["before_f12"];
        if baseline["presentation"] != original["actual"]
            || baseline["author"] != original["author"]
            || baseline["physical_rect"] != original["physical_rect"]
            || baseline["excluded_rects"] != original["excluded_rects"]
            || baseline["excluded_shapes"] != original["excluded_shapes"]
            || returned["before_f12"] != *baseline
        {
            return Err(format!("{playing}的恢复基线没有对应实际进入前的作者观测"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Value {
        json!({"roots":[{"display":"Flex","visibility":"Visible","inherited_visible":true}],
            "headers":[{"inherited_visible":true}],"camera":{"active":true,"position":[1,2,3]},
            "layout":[800,600,500,350],"selection":{"placement":"cube"},"navigation":{"yaw":1}})
    }

    fn baseline() -> Value {
        json!({"presentation":editor(),"author":{"undo_len":1,"redo_len":0,"dirty":true},
            "physical_rect":[200,50,800,600],"excluded_rects":[[0,0,10,10]]})
    }

    #[test]
    fn native_preview_requires_visible_author_components_in_both_modes() {
        let mut actual = editor();
        assert!(matches_mode(&actual, true));
        actual["roots"][0] =
            json!({"display":"None","visibility":"Hidden","inherited_visible":false});
        actual["camera"]["active"] = json!(false);
        assert!(!matches_mode(&actual, true), "作者根或相机被收起时拒绝通过");
        actual["headers"][0]["inherited_visible"] = json!(false);
        assert!(!matches_mode(&actual, true));
    }

    #[test]
    fn native_preview_keeps_original_header_and_all_three_actual_caption_rects() {
        let chrome = json!({"layout":{"visible":false,"bottom":0},"preview_header":null,"active_header":4,"regular_header":4,
            "camera":null,"captions":[
                {"action":"minimize","entity":10,"rect":[100,0,150,54],"visible":true},
                {"action":"maximize","entity":11,"rect":[150,0,200,54],"visible":true},
                {"action":"close","entity":12,"rect":[200,0,250,54],"visible":true}]});
        assert!(preview_chrome_ready(&chrome));
        for (path, value) in [("/layout/visible", json!(true)), ("/camera", json!({"active":true})), ("/captions/0/visible", json!(false))] {
            let mut changed=chrome.clone();
            *changed.pointer_mut(path).unwrap()=value;
            assert!(!preview_chrome_ready(&changed));
        }
        let mut covered=chrome.clone();
        covered["captions"][2]["rect"][3]=json!(0);
        assert!(!preview_chrome_ready(&covered));
        covered=chrome;
        covered["captions"][0]["action"]=json!("close");
        assert!(!preview_chrome_ready(&covered));
    }

    #[test]
    fn returning_requires_original_layout_camera_selection_and_history() {
        let before = baseline();
        let original = json!({"source":SOURCE,"actual":editor(),"author":before["author"],
            "before_f12":before,"physical_rect":[200,50,800,600],"excluded_rects":[[0,0,10,10]]});
        let stage = "f12_edit_same_world_and_paused_ticks";
        assert!(verify_transition(&original, stage).is_ok());
        for path in [
            "/actual/layout/0",
            "/actual/camera/position/0",
            "/actual/selection/placement",
            "/author/undo_len",
            "/physical_rect/0",
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(path).unwrap() = json!(999);
            assert!(
                verify_transition(&changed, stage).is_err(),
                "{path}必须恢复"
            );
        }
    }

    #[test]
    fn returning_control_region_difference_is_rejected_with_both_pixel_sets() {
        let before = baseline();
        let mut proof = json!({"source":SOURCE,"actual":editor(),"author":before["author"],
            "before_f12":before,"physical_rect":[200,50,800,600],"excluded_rects":[[0,0,10,10]]});
        proof["excluded_rects"].as_array_mut().unwrap().push(json!([12,183,648,35]));
        let error = verify_transition(&proof, "f12_edit_same_world_and_paused_ticks").unwrap_err();
        assert!(error.contains("before physical_rect=[200,50,800,600] excluded_rects=[[0,0,10,10]]"));
        assert!(error.contains("actual physical_rect=[200,50,800,600] excluded_rects=[[0,0,10,10],[12,183,648,35]]"));
        proof["excluded_rects"] = json!([]);
        assert!(verify_transition(&proof, "f12_edit_same_world_and_paused_ticks").is_err());
    }
}
