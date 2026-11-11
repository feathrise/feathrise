//! 真实预览中请求系统关闭，确认原保存层可见、可取消，且同一游戏与作者草稿保留。
use super::{super::EmbeddedStatus, presentation_evidence};
use crate::native_project_close::embedded_validation as close;
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
};
use feathrise_editor_scene::standalone::embedded_validation as author;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Resource)]
struct Check {
    pid: u32,
    world: u64,
    author: Value,
    prompt: Option<Value>,
    capture: Arc<Mutex<Option<Result<(), String>>>>,
    pulse: Option<Entity>,
    cancelled: bool,
}

pub(super) fn exercise(world: &mut World, directory: &Path) -> Result<Option<Value>, String> {
    if !world.contains_resource::<Check>() {
        let status = world.resource::<EmbeddedStatus>();
        if !status.playing || !status.ready {
            return Err("关闭专项必须从真实预览回执开始".into());
        }
        let (pid, id) = (status.pid, status.world_id);
        let before =
            serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
        if before["dirty"] != true {
            return Err("关闭专项必须有真实未保存作者修改".into());
        }
        world.insert_resource(Check {
            pid,
            world: id,
            author: before,
            prompt: None,
            capture: Arc::new(Mutex::new(None)),
            pulse: None,
            cancelled: false,
        });
        close::request(world)?;
        return Ok(None);
    }
    world.resource_scope(|world, mut check: Mut<Check>| {
        if let Some(pulse) = check.pulse.take() {
            author::release_pulse(world, pulse);
        }
        let status = world.resource::<EmbeddedStatus>();
        if !status.active
            || !status.ready
            || (status.pid, status.world_id) != (check.pid, check.world)
        {
            return Err("关闭确认前或取消关闭时丢失常驻工程World".into());
        }
        if serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?
            != check.author
        {
            return Err("关闭确认/取消改变了未保存草稿或历史".into());
        }
        if check.prompt.is_none() {
            if !presentation_evidence::mode_ready(world, false)? {
                return Ok(None);
            }
            let Some(prompt) = close::visible_prompt(world) else {
                return Ok(None);
            };
            check.prompt = Some(prompt);
            let capture = check.capture.clone();
            let path = directory.join("playing-close-visible-prompt.png");
            world.spawn(Screenshot::primary_window()).observe(
                move |image: On<ScreenshotCaptured>| {
                    let image = image.image.clone();
                    let path = path.clone();
                    let capture = capture.clone();
                    IoTaskPool::get()
                        .spawn(async move {
                            let result = image
                                .try_into_dynamic()
                                .map_err(|error| error.to_string())
                                .and_then(|image| {
                                    image
                                        .to_rgb8()
                                        .save(path)
                                        .map_err(|error| error.to_string())
                                });
                            if let Ok(mut slot) = capture.lock() {
                                *slot = Some(result);
                            }
                        })
                        .detach();
                },
            );
        } else if !check.cancelled {
            let captured = check
                .capture
                .try_lock()
                .ok()
                .and_then(|mut slot| slot.take());
            if let Some(result) = captured {
                result?;
                check.pulse = Some(close::cancel(world)?);
                check.cancelled = true;
            }
        } else if close::idle(world)
            && close::hidden_prompt(world)
            && presentation_evidence::mode_ready(world, false)?
        {
            return Ok(Some(
                json!({"input":"real WindowCloseRequested while Playing, original Cancel button",
                "pid":check.pid,"world_id":check.world,"author_before_and_after":check.author,
                "prompt":check.prompt,"screenshot":"playing-close-visible-prompt.png",
                "cancelled_to_original_editor":true,"same_game_remained_alive":true}),
            ));
        }
        Ok(None)
    })
}
