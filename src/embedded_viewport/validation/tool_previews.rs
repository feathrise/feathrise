//! 原工具栏驱动与真实工程Gizmo/GPU取证；不写作者内容，不以协议回显代替画面。

use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_editor_scene::standalone::embedded_validation::{
    self as author, tool_previews as tools,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

const SOURCE: &str = "actual-project-GizmoAssets-after-GizmoMeshSystems";
const STAGES: [&str; 7] = [
    "clear_before",
    "select",
    "move",
    "rotate",
    "scale",
    "clear_after",
    "restored",
];

#[derive(Resource)]
struct Driver {
    author: Value,
    original: Value,
    cube: String,
    step: usize,
    started: Instant,
    pulse: Option<Entity>,
    expected: Option<Value>,
    minimum_frame: u64,
    task: Option<Task<Result<Option<Value>, String>>>,
    retry: Instant,
    proofs: Vec<Value>,
}

pub(super) fn exercise(world: &mut World, directory: &Path) -> Result<Option<Value>, String> {
    if !world.contains_resource::<Driver>() {
        let baseline = author::probe(world)?;
        let cube = baseline
            .last_added_id
            .clone()
            .ok_or("工具验收缺少原创建链新增的Cuboid身份")?;
        world.insert_resource(Driver {
            author: serde_json::to_value(baseline).map_err(|error| error.to_string())?,
            original: tools::probe(world)?,
            cube,
            step: 0,
            started: Instant::now(),
            pulse: None,
            expected: None,
            minimum_frame: 0,
            task: None,
            retry: Instant::now(),
            proofs: Vec::new(),
        });
    }
    world.resource_scope(|world, mut driver: Mut<Driver>| {
        if driver.started.elapsed() > Duration::from_secs(120) {
            return Err(format!("原工具真实几何/像素取证超时：{}", STAGES[driver.step.min(6)]));
        }
        if let Some(entity) = driver.pulse.take() { author::release_pulse(world, entity); }
        if serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())? != driver.author {
            return Err("切换工具或选择改变了作者草稿、保存身份或撤销历史".into());
        }
        let mode = match driver.step {
            0 | 1 | 5 => "Select",
            2 => "Move", 3 => "Rotate", 4 => "Scale",
            _ => driver.original["mode"].as_str().ok_or("缺少原操作模式")?,
        }.to_owned();
        let selected = match driver.step {
            0 | 5 => None,
            1..=4 => Some(driver.cube.clone()),
            _ => driver.original["selection"].as_str().map(str::to_owned),
        };
        let actual = tools::probe(world)?;
        if actual["mode"] != mode {
            driver.pulse = Some(tools::press_mode(world, &mode)?);
            return Ok(None);
        }
        if tools::selection(world) != selected {
            tools::select(world, selected.as_deref())?;
            return Ok(None); // 等正式绘制与语义投影系统，不能当帧写出期望几何。
        }
        if driver.step < 6 && !expected_tools(&actual, &mode, selected.is_some()) { return Ok(None); }
        if driver.step == 6 && actual["semantic_sha256"] != driver.original["semantic_sha256"] { return Ok(None); }
        if driver.expected.as_ref().is_none_or(|expected| expected["semantic_sha256"] != actual["semantic_sha256"]
            || expected["legacy_lines"] != actual["legacy_lines"] || expected["revision"] != actual["revision"]
            || expected["generation"] != actual["generation"]) {
            if driver.task.is_some() {
                if let Some(result) = driver.task.as_mut().and_then(check_ready) { driver.task = None; result?; }
                return Ok(None); // 旧后台读真正结束才释放槽，鼠标悬停换色不能无限堆积任务。
            }
            driver.expected = Some(actual.clone());
            // 首个空场景可能在进入Tools前已完成真实回读，且相同画面不会重复截图。
            // read仍核对本次进程、World、修订、语义及作者实体；后续内容变化才必须取得新帧。
            driver.minimum_frame = if driver.proofs.is_empty() { 0 } else {
                world.resource::<super::EmbeddedStatus>().frames.saturating_add(1)
            };
        }
        let capture = if let Some(task) = driver.task.as_mut() {
            if let Some(result) = check_ready(task) {
                driver.task = None;
                driver.retry = Instant::now() + Duration::from_millis(100);
                result?
            } else { None }
        } else {
            if Instant::now() >= driver.retry {
                let status = world.resource::<super::EmbeddedStatus>();
                let (pid, world_id, revision) = (status.pid,status.world_id,status.applied_revision);
                let directory = directory.join("game");
                let expected = driver.expected.clone().unwrap();
                let author = driver.author.clone();
                let minimum_frame = driver.minimum_frame;
                driver.task = Some(IoTaskPool::get().spawn(async move {
                    read(&directory,pid,world_id,revision,minimum_frame,&expected,&author)
                }));
            }
            None
        };
        if let Some(capture) = capture {
            let stage = STAGES[driver.step];
            driver.proofs.push(json!({"stage":stage,"requested":actual,"capture":capture}));
            println!("FEATHRISE_EMBEDDED_LIVE_STEP {}",json!({"stage":format!("author_tools_{stage}"),"source":"original toolbar and actual project GizmoAssets/GPU pixels"}));
            driver.step += 1;
            // 清除后恢复到同一空工具集合不会触发工程重复截图；保留已核验请求及其帧门，
            // 下一阶段仍由read重读原实际PNG。只有显示内容/会话变化才提高minimum_frame。
            // 非空旧线仅有数量证据，无法证明线内容相同，不能据此跨阶段复用截图。
            if actual["legacy_lines"] != 0 { driver.expected = None; }
            if driver.step == STAGES.len() {
                let evidence = json!({"source":"original toolbar -> real GizmoAssets -> projected PNG pixels",
                    "author_before":driver.author,"author_after":author::probe(world)?,
                    "original":driver.original,"restored":actual,"proofs":driver.proofs});
                return Ok(Some(evidence));
            }
        }
        Ok(None)
    })
}

fn expected_tools(probe: &Value, mode: &str, selected: bool) -> bool {
    let Some(items) = probe["preview"]["items"].as_array() else {
        return false;
    };
    if !selected {
        return items.is_empty() && probe["legacy_lines"] == 0;
    }
    let selections = items
        .iter()
        .filter(|item| item["kind"] == "selection")
        .count();
    let transforms = items
        .iter()
        .filter(|item| item["kind"] == "transform")
        .collect::<Vec<_>>();
    selections > 0
        && match mode {
            "Select" => transforms.is_empty(),
            "Move" => transforms
                .iter()
                .any(|item| item["preview"]["kind"] == "move"),
            "Rotate" => transforms
                .iter()
                .any(|item| item["preview"]["kind"] == "rotate"),
            "Scale" => transforms
                .iter()
                .any(|item| item["preview"]["kind"] == "scale"),
            _ => true,
        }
}

pub(super) fn capture_empty(capture: &Value) -> bool {
    let tools = &capture["author_tools"];
    capture["author_overlay"]["visible_lines"] == 0
        && tools["source"] == SOURCE
        && tools["error"].is_null()
        && tools["visible_segments"] == 0
        && tools["semantic_items"] == 0
        && tools["clipmap_input"].is_null()
        && clipmap_cleared(tools)
}

fn clipmap_cleared(tools: &Value) -> bool {
    let actual = &tools["clipmap_render"];
    actual.is_null()
        || (actual["visible"] == false
            && actual["brush"][2].as_f64() == Some(0.0)
            && actual["color"][3].as_f64() == Some(0.0))
}

pub(super) fn brush_cleared(capture: &Value) -> bool {
    let tools = &capture["author_tools"];
    tools["source"] == SOURCE
        && tools["error"].is_null()
        && tools["brushes"] == 0
        && tools["clipmap_input"].is_null()
        && clipmap_cleared(tools)
}

/// F12须撤回游戏World中的真实几何和材质刷圈；仅本地UI隐藏不构成通过。
pub(super) fn verify_visibility(capture: &Value, playing: bool) -> Result<(), String> {
    let tools = &capture["author_tools"];
    if tools["source"] != SOURCE
        || !tools["error"].is_null()
        || tools["pending"] != false
        || tools["mode"] != if playing { "Playing" } else { "Editing" }
        || tools["visible"] != !playing
    {
        return Err("F12缺少当前工程World中的实际工具显隐证据".into());
    }
    if playing && !capture_empty(capture) {
        return Err("预览仍残留实际作者Gizmo或Clipmap材质刷圈".into());
    }
    if !playing
        && tools["semantic_items"].as_u64().unwrap_or(0) > 0
        && tools["visible_segments"].as_u64().unwrap_or(0) == 0
        && tools["clipmap_render"]["visible"] != true
    {
        return Err("返回编辑未恢复实际Gizmo或Clipmap刷圈".into());
    }
    Ok(())
}

pub(super) fn matches_brush(capture: &Value, expected: &Value) -> bool {
    let tools = &capture["author_tools"];
    let Some(brush) = expected["preview"]["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["kind"] == "brush"))
    else {
        return false;
    };
    tools["source"] == SOURCE
        && tools["error"].is_null()
        && tools["mode"] == "Editing"
        && tools["pending"] == false
        && tools["visible"] == true
        && tools["brushes"].as_u64().unwrap_or(0) > 0
        && tools["semantic_sha256"] == expected["semantic_sha256"]
        && if brush["preview"]["clipmap"].is_null() {
            tools["visible_segments"].as_u64().unwrap_or(0) >= 64
        } else {
            tools["clipmap_matches_preview"] == true
                && tools["clipmap_render"]["bound"] == true
                && tools["clipmap_render"]["visible"] == true
                && tools["clipmap_matched_samples"].as_u64().unwrap_or(0) >= 6
        }
}

pub(super) fn matches_request(capture: &Value, expected: &Value) -> bool {
    let tools = &capture["author_tools"];
    tools["source"] == SOURCE
        && tools["error"].is_null()
        && tools["visible"] == true
        && tools["pending"] == false
        && tools["viewport_visible"] == true
        && tools["revision"] == capture["revision"]
        && tools["mode"] == "Editing"
        && tools["semantic_sha256"] == expected["semantic_sha256"]
        && expected["preview"]["items"]
            .as_array()
            .is_some_and(|items| tools["semantic_items"] == items.len())
        && if expected["preview"]["items"]
            .as_array()
            .is_some_and(|items| items.is_empty())
        {
            capture_empty(capture)
        } else {
            tools["visible_segments"].as_u64().unwrap_or(0) >= minimum_segments(expected).max(1)
                && (expected["selection"].is_null()
                    || tools["selections"].as_u64().unwrap_or(0) > 0)
                && tools["geometry_sha256"]
                    .as_str()
                    .is_some_and(|hash| hash.len() == 64)
        }
}

/// 最小夹具仍按每项固定几何校验下界，不能只有选择框却把未绘制的旋转/缩放手柄算通过。
fn minimum_segments(expected: &Value) -> u64 {
    expected["preview"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            if item["kind"] == "selection" {
                return 12;
            }
            if item["kind"] != "transform" {
                return 0;
            }
            let preview = &item["preview"];
            match preview["kind"].as_str() {
                Some("rotate") => 64,
                Some("scale") => {
                    if preview["cube"] == true {
                        12
                    } else {
                        6
                    }
                }
                Some("move") => {
                    u64::from((preview["axes"].as_u64().unwrap_or(0) as u8).count_ones())
                        + if preview["plane"] == true { 4 } else { 0 }
                }
                _ => 0,
            }
        })
        .sum()
}

pub(super) fn read(
    directory: &Path,
    pid: u32,
    world_id: u64,
    revision: u64,
    minimum_frame: u64,
    expected: &Value,
    author: &Value,
) -> Result<Option<Value>, String> {
    let prefix = format!("{pid}-{revision}-editing-author-view-");
    for (index, entry) in fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .enumerate()
    {
        if index >= 2048 {
            return Err("工具取证目录超出固定预算".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(&prefix) || !name.ends_with(".json") {
            continue;
        }
        let mut bytes = Vec::new();
        fs::File::open(entry.path())
            .map_err(|error| error.to_string())?
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 256 * 1024 {
            return Err("工具GPU回执超过固定预算".into());
        }
        let Ok(mut capture) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if capture["pid"] != pid
            || capture["world_id"] != world_id
            || capture["revision"] != revision
            || capture["mode"] != "editing"
            || capture["source"] != "real-project-world"
        {
            return Err("工具截图的会话/World/修订不符".into());
        }
        if capture["frame"].as_u64().unwrap_or(0) < minimum_frame
            || !matches_request(&capture, expected)
        {
            continue;
        }
        super::placement_evidence::verify(&capture, author)?;
        verify_pixels(&capture, &entry.path().with_extension("png"))?;
        capture["immutable_evidence"] = json!(name.as_ref());
        return Ok(Some(capture));
    }
    Ok(None)
}

pub(super) fn verify_pixels(capture: &Value, path: &Path) -> Result<(), String> {
    if capture["capture"]["source"] != "Bevy ScreenshotCaptured" {
        return Err("工具缺少真实GPU回读来源".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() < 24 || bytes.len() > 64 * 1024 * 1024 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("工具PNG缺失或超过预算".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16 * 1024 * 1024 {
        return Err("工具PNG尺寸超过固定预算".into());
    }
    let image = Image::from_buffer(
        &bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::nearest(),
        RenderAssetUsages::default(),
    )
    .map_err(|error| error.to_string())?
    .try_into_dynamic()
    .map_err(|error| error.to_string())?
    .to_rgba8();
    let tools = &capture["author_tools"];
    let digest = Sha256::digest(image.as_raw())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if capture["capture"]["width"] != width
        || capture["capture"]["height"] != height
        || tools["rgba_sha256"] != digest
    {
        return Err("工具PNG与本次回读的像素身份不符".into());
    }
    for (field, count, required) in [
        (
            "samples",
            "matched_samples",
            tools["visible_segments"].as_u64().unwrap_or(0) > 0,
        ),
        (
            "clipmap_samples",
            "clipmap_matched_samples",
            tools["clipmap_render"]["visible"] == true,
        ),
    ] {
        let samples = tools[field]
            .as_array()
            .filter(|samples| samples.len() <= 96)
            .ok_or("工具真实几何投影采样缺失或超限")?;
        let mut matched = 0;
        for sample in samples {
            let position = sample["position"]
                .as_array()
                .filter(|values| values.len() == 2)
                .ok_or("工具投影坐标不完整")?;
            let x = position[0].as_f64().ok_or("工具投影X无效")?;
            let y = position[1].as_f64().ok_or("工具投影Y无效")?;
            let color = sample["srgb"]
                .as_array()
                .filter(|values| values.len() == 3)
                .ok_or("工具实际线色缺失")?;
            let expected = [
                color[0].as_f64().ok_or("工具红色无效")?,
                color[1].as_f64().ok_or("工具绿色无效")?,
                color[2].as_f64().ok_or("工具蓝色无效")?,
            ];
            if !x.is_finite()
                || !y.is_finite()
                || x < 0.0
                || y < 0.0
                || x >= f64::from(width)
                || y >= f64::from(height)
                || expected.iter().any(|value| !value.is_finite())
            {
                return Err("工具采样位置或颜色非法".into());
            }
            let mut hit = false;
            for dy in -3..=3 {
                for dx in -3..=3 {
                    let (px, py) = (x.round() as i32 + dx, y.round() as i32 + dy);
                    if px >= 0 && py >= 0 && px < width as i32 && py < height as i32 {
                        hit |= matches_color(expected, image.get_pixel(px as u32, py as u32).0);
                    }
                }
            }
            if sample["matched"] != hit {
                return Err("工具回执与独立PNG像素不符".into());
            }
            matched += usize::from(hit);
        }
        if tools[count] != matched || tools["sample_radius"] != 3 {
            return Err("工具像素命中计数不符".into());
        }
        if required && (samples.len() < 6 || matched < 6) {
            return Err(format!(
                "工具实际投影仅命中{matched}/{}处颜色像素，不能以消息或灰背景通过",
                samples.len()
            ));
        }
    }
    Ok(())
}

fn matches_color(expected: [f64; 3], pixel: [u8; 4]) -> bool {
    // 与工程侧同用f32，避免恰好位于阈值的颜色因JSON转f64而产生不同结论。
    let expected = expected.map(|value| value as f32);
    let actual = [
        f32::from(pixel[0]) / 255.0,
        f32::from(pixel[1]) / 255.0,
        f32::from(pixel[2]) / 255.0,
    ];
    let chroma = |values: &[f32]| {
        values.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            - values.iter().copied().fold(f32::INFINITY, f32::min)
    };
    expected
        .iter()
        .zip(actual)
        .all(|(expected, actual)| (expected - actual).abs() <= 0.42)
        && (chroma(&expected) < 0.15 || chroma(&actual) > 0.12)
}

/// 再清选择还要在原真实线位置看见像素消失，CPU缓冲为空不能掩盖GPU残影。
fn verify_cleared_pixels(
    directory: &Path,
    selected: &Value,
    before: &Value,
    after: &Value,
) -> Result<(), String> {
    let samples = selected["author_tools"]["samples"]
        .as_array()
        .ok_or("缺少选择框实际投影")?;
    let count = |capture: &Value| -> Result<usize, String> {
        let name = capture["immutable_evidence"]
            .as_str()
            .ok_or("清选择缺少独立截图")?;
        let mut bytes = Vec::new();
        fs::File::open(directory.join(name).with_extension("png"))
            .map_err(|error| error.to_string())?
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("清选择PNG超出预算".into());
        }
        let image = Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::nearest(),
            RenderAssetUsages::default(),
        )
        .map_err(|error| error.to_string())?
        .try_into_dynamic()
        .map_err(|error| error.to_string())?
        .to_rgba8();
        if selected["capture"]["width"] != image.width()
            || selected["capture"]["height"] != image.height()
        {
            return Err("清选择前后像素尺寸改变，无法核对原线位置".into());
        }
        Ok(samples
            .iter()
            .filter(|sample| {
                let Some(position) = sample["position"].as_array() else {
                    return false;
                };
                let Some(color) = sample["srgb"].as_array() else {
                    return false;
                };
                if position.len() != 2 || color.len() != 3 {
                    return false;
                }
                let (Some(x), Some(y), Some(r), Some(g), Some(b)) = (
                    position[0].as_f64(),
                    position[1].as_f64(),
                    color[0].as_f64(),
                    color[1].as_f64(),
                    color[2].as_f64(),
                ) else {
                    return false;
                };
                (-3..=3).any(|dy| {
                    (-3..=3).any(|dx| {
                        let (x, y) = (x.round() as i32 + dx, y.round() as i32 + dy);
                        x >= 0
                            && y >= 0
                            && x < image.width() as i32
                            && y < image.height() as i32
                            && matches_color([r, g, b], image.get_pixel(x as u32, y as u32).0)
                    })
                })
            })
            .count())
    };
    let before_count = count(before)?;
    let selected_count = selected["author_tools"]["matched_samples"]
        .as_u64()
        .ok_or("缺少选择框实际命中数")? as usize;
    let after_count = count(after)?;
    if selected_count < after_count.saturating_add(3)
        || after_count > before_count.saturating_add(1)
    {
        return Err(format!(
            "清选择后的GPU线色仍残留：原空图{before_count}、选中{selected_count}、清除{after_count}"
        ));
    }
    Ok(())
}

pub(super) fn verify(directory: &Path, evidence: &Value) -> Result<(), String> {
    if evidence["author_before"] != evidence["author_after"] {
        return Err("工具取证改变了作者内容或历史".into());
    }
    if evidence["original"]["mode"] != evidence["restored"]["mode"]
        || evidence["original"]["selection"] != evidence["restored"]["selection"]
    {
        return Err("工具取证没有恢复原模式与选择".into());
    }
    let proofs = evidence["proofs"]
        .as_array()
        .ok_or("缺少工具逐项真实取证")?;
    for name in STAGES {
        let proof = proofs
            .iter()
            .find(|proof| proof["stage"] == name)
            .ok_or_else(|| format!("缺少工具阶段{name}"))?;
        if !matches_request(&proof["capture"], &proof["requested"]) {
            return Err(format!("{name}没有匹配实际工具几何"));
        }
        let file = proof["capture"]["immutable_evidence"]
            .as_str()
            .ok_or("工具缺少独立截图路径")?;
        if Path::new(file).components().count() != 1 || !file.ends_with(".json") {
            return Err("工具取证路径越界".into());
        }
        let stored: Value = serde_json::from_slice(
            &fs::read(directory.join(file)).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let mut recorded = proof["capture"].clone();
        recorded
            .as_object_mut()
            .unwrap()
            .remove("immutable_evidence");
        if stored != recorded {
            return Err(format!("{name}实际落盘回执与汇总不一致"));
        }
        verify_pixels(&stored, &directory.join(file).with_extension("png"))?;
    }
    let find = |name: &str| &proofs.iter().find(|proof| proof["stage"] == name).unwrap()["capture"];
    for pair in [["select", "move"], ["move", "rotate"], ["rotate", "scale"]] {
        let (a, b) = (find(pair[0]), find(pair[1]));
        if a["author_tools"]["geometry_sha256"] == b["author_tools"]["geometry_sha256"]
            || a["capture"]["sha256"] == b["capture"]["sha256"]
        {
            return Err(format!("{}到{}的真实几何或GPU像素未改变", pair[0], pair[1]));
        }
    }
    if !capture_empty(find("clear_before")) || !capture_empty(find("clear_after")) {
        return Err("清选择仍残留作者工具".into());
    }
    verify_cleared_pixels(
        directory,
        find("select"),
        find("clear_before"),
        find("clear_after"),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_legacy_overlay_does_not_hide_stale_semantic_geometry_or_clipmap() {
        let clear = json!({"author_overlay":{"visible_lines":0},"author_tools":{
            "source":SOURCE,"error":null,"visible_segments":0,"semantic_items":0,"clipmap_input":null}});
        assert!(capture_empty(&clear));
        for (field, value) in [
            ("visible_segments", json!(12)),
            ("semantic_items", json!(1)),
            ("clipmap_input", json!({"radius":3})),
            (
                "clipmap_render",
                json!({"visible":true,"brush":[0.,0.,3.,1.],"color":[1.,1.,0.,1.]}),
            ),
        ] {
            let mut stale = clear.clone();
            stale["author_tools"][field] = value;
            assert!(!capture_empty(&stale), "{field}残留不能当作清选择完成");
        }
        assert!(
            !capture_empty(&json!({"author_overlay":{"visible_lines":0}})),
            "旧回执缺少实际工具证据不能通过"
        );
    }

    #[test]
    fn actual_material_and_play_mode_release_are_required_for_clipmap() {
        let expected = json!({"semantic_sha256":"brush","preview":{"items":[
            {"kind":"brush","preview":{"clipmap":{"terrain_id":"ground"}}}]}});
        let mut capture = json!({"author_overlay":{"visible_lines":0},"author_tools":{
            "source":SOURCE,"error":null,"pending":false,"mode":"Editing","visible":true,
            "semantic_sha256":"brush","semantic_items":1,"brushes":1,"visible_segments":0,
            "clipmap_matches_preview":true,"clipmap_matched_samples":12,
            "clipmap_render":{"visible":true,"bound":true,"brush":[0.,0.,3.,1.],"color":[1.,1.,0.,1.]}}});
        assert!(matches_brush(&capture, &expected));
        capture["author_tools"]["clipmap_render"]["bound"] = json!(false);
        assert!(
            !matches_brush(&capture, &expected),
            "像素/消息相同也不能冒领真实材质绑定"
        );
        capture["author_tools"]["mode"] = json!("Playing");
        capture["author_tools"]["visible"] = json!(false);
        capture["author_tools"]["semantic_items"] = json!(0);
        assert!(
            verify_visibility(&capture, true).is_err(),
            "隐藏相机不能代替清除材质旧刷圈"
        );
        capture["author_tools"]["clipmap_render"] = Value::Null;
        assert!(verify_visibility(&capture, true).is_ok());
    }

    #[test]
    fn matching_message_without_real_geometry_is_rejected() {
        let expected = json!({"selection":"cube","semantic_sha256":"same",
            "preview":{"items":[{"kind":"selection"}]}});
        let capture = json!({"revision":7,"author_tools":{"source":SOURCE,"error":null,
            "visible":true,"pending":false,"viewport_visible":true,"revision":7,"mode":"Editing",
            "semantic_sha256":"same","semantic_items":1,"visible_segments":0,"selections":1}});
        assert!(
            !matches_request(&capture, &expected),
            "回显语义计数不能代替真实Gizmo资产"
        );
        assert!(!matches_color([1.0, 0.7, 0.1], [100, 100, 100, 255]));
        let rotate = json!({"preview":{"items":[{"kind":"selection"},{"kind":"transform","preview":{"kind":"rotate"}}]}});
        assert_eq!(
            minimum_segments(&rotate),
            76,
            "仅有12边选择框不能冒充64段旋转环"
        );
    }
}
