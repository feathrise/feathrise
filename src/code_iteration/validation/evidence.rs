//! 真实子进程组件、GPU与作者快照的有界取证；文件检查和PNG写入全部使用唯一后台槽。

use super::resources::Plan;
use crate::embedded_viewport::{EmbeddedStatus, validation as embedded};
use bevy::{prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured}, tasks::{IoTaskPool, futures::check_ready}};
use feathrise_editor_map::standalone::{self, embedded_validation as author};
use serde_json::{Value, json};
use std::{fs, io::Read, time::{Duration, Instant}};

pub(super) fn author_state(world: &mut World) -> Result<Value, String> {
    let presentation = serde_json::to_value(author::presentation_probe(world)?).map_err(|error| error.to_string())?;
    let documents = standalone::source_location_probe(world).map(|probe| probe["documents"].clone()).unwrap_or(json!([]));
    let geometry = world.resource::<crate::viewport_mode::ViewportGeometry>();
    Ok(json!({"author":author::probe(world)?,"presentation":presentation,"documents":documents,
        "physical_rect":geometry.rect,"excluded_rects":geometry.excluded_rects,"excluded_shapes":geometry.excluded_shapes}))
}

pub(super) fn current_snapshot(world: &World) -> bool {
    use standalone::embedded_snapshot::{self, EmbeddedSnapshotState};
    embedded_snapshot::ready_is_current(world) && world.resource::<EmbeddedSnapshotState>().ready.as_ref()
        .is_some_and(|snapshot| world.resource::<EmbeddedStatus>().applied_revision >= snapshot.author_revision)
}

pub(super) fn preserved(world: &mut World, baseline: &Value) -> Result<(), String> {
    let actual = author_state(world)?;
    for field in ["author", "documents", "physical_rect", "excluded_rects", "excluded_shapes"] {
        if actual[field] != baseline[field] { return Err(format!("应用新代码改变了作者{field}：before={} actual={}", baseline[field], actual[field])); }
    }
    for field in ["roots", "headers", "layout", "camera", "navigation", "selection", "panes"] {
        if actual["presentation"][field] != baseline["presentation"][field] {
            return Err(format!("应用新代码没有恢复作者{field}：before={} actual={}", baseline["presentation"][field], actual["presentation"][field]));
        }
    }
    Ok(())
}

pub(super) fn capture(world: &mut World, plan: &mut Plan, speed: f64) -> Result<Option<Value>, String> {
    let status = world.resource::<EmbeddedStatus>();
    if !status.ready || status.playing || !current_snapshot(world) { return Ok(None); }
    let key = (status.pid, status.world_id, status.applied_revision);
    if plan.evidence_key != Some(key) && plan.evidence_task.is_none() {
        plan.evidence_key = Some(key);
        plan.evidence = None;
    }
    if let Some(result) = plan.evidence_task.as_mut().and_then(check_ready) {
        plan.evidence_task = None;
        plan.evidence = result?;
        plan.retry = Instant::now() + Duration::from_millis(150);
    }
    if let Some(value) = &plan.evidence {
        if value["runtime"]["pid"] == key.0 && value["runtime"]["world_id"] == key.1
            && value["runtime"]["revision"] == key.2 && value["runtime"]["walk_speed"].as_f64() == Some(speed)
        { return Ok(Some(value.clone())); }
        return Err(format!("真实FreeCamera组件不是本次采用的速度{speed}：{}", value["runtime"]));
    }
    if plan.evidence_task.is_some() || Instant::now() < plan.retry { return Ok(None); }
    let geometry = world.resource::<crate::viewport_mode::ViewportGeometry>().clone();
    let expected = serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
    let directory = plan.directory.clone();
    plan.evidence_task = Some(IoTaskPool::get().spawn(async move {
        let Some(mut evidence) = embedded::code_iteration_capture(&directory.join("game"), key.0, key.2, 0, &expected, &geometry)? else { return Ok(None); };
        if evidence["game"]["world_id"] != key.1 { return Err("运行实例GPU证据来自不同World".into()); }
        let runtime = directory.join("runtime").join(format!("{}-{}-{}.json", key.0, key.1, key.2));
        let file = match fs::File::open(&runtime) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(8193).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        if bytes.len() > 8192 { return Err("实际组件取证超过8KiB预算".into()); }
        let Ok(proof) = serde_json::from_slice::<Value>(&bytes) else { return Ok(None); };
        if proof["source"] != "actual-primary-scene-FreeCamera-component"
            || proof["pid"] != key.0 || proof["world_id"] != key.1 || proof["revision"] != key.2
            || proof["entity"].as_u64().is_none()
        { return Err("实际相机组件取证与运行实例/World/修订不一致".into()); }
        evidence["runtime"] = proof;
        evidence["runtime_file"] = json!(runtime);
        Ok(Some(evidence))
    }));
    Ok(None)
}

pub(super) fn screenshot(world: &mut World, plan: &mut Plan, name: &str) -> Result<bool, String> {
    if plan.screenshot_name.as_deref() == Some(name) && plan.screenshot.is_none() { return Ok(true); }
    if let Some(receiver) = &plan.screenshot {
        match receiver.try_recv() {
            Ok(result) => { result?; plan.screenshot = None; return Ok(true); }
            Err(async_channel::TryRecvError::Closed) => return Err("作者GPU截图任务未返回结果".into()),
            Err(async_channel::TryRecvError::Empty) => return Ok(false),
        }
    }
    let path = plan.directory.join(name);
    let (sender, receiver) = async_channel::bounded(1);
    plan.screenshot = Some(receiver);
    plan.screenshot_name = Some(name.into());
    world.spawn(Screenshot::primary_window()).observe(move |captured: On<ScreenshotCaptured>| {
        let image = captured.image.clone();
        let path = path.clone();
        let sender = sender.clone();
        IoTaskPool::get().spawn(async move {
            let result = image.try_into_dynamic().map_err(|error| error.to_string())
                .and_then(|image| image.to_rgb8().save(path).map_err(|error| error.to_string()));
            let _ = sender.try_send(result);
        }).detach();
    });
    Ok(false)
}
