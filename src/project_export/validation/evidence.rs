//! 实际GPU截图的单飞回读与后台编码；验收仅保存真实像素，不生成替代成功图片。

use super::Plan;
use bevy::{prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured}, tasks::IoTaskPool};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) fn screenshot(world: &mut World, plan: &mut Plan, name: &str) -> Result<Option<Value>, String> {
    if let Some(receiver) = &plan.screenshot {
        match receiver.try_recv() {
            Ok(result) => { plan.screenshot = None; return result.map(Some); }
            Err(async_channel::TryRecvError::Closed) => return Err("导出GPU截图任务未返回回执".into()),
            Err(async_channel::TryRecvError::Empty) => return Ok(None),
        }
    }
    let path = plan.report.parent().ok_or("导出报告缺少目录")?.join(name);
    let (sender, receiver) = async_channel::bounded(1);
    plan.screenshot = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |captured: On<ScreenshotCaptured>| {
        let image = captured.image.clone();
        let path = path.clone();
        let sender = sender.clone();
        IoTaskPool::get().spawn(async move {
            let result = (|| -> Result<Value, String> {
                let pixels = image.data.as_ref().ok_or("导出截图缺少CPU回读像素")?;
                if image.width() < 320 || image.height() < 240 || pixels.len() < 320 * 240 * 3 {
                    return Err("导出面板GPU截图尺寸无效".into());
                }
                let proof = json!({"source":"Bevy ScreenshotCaptured","width":image.width(),"height":image.height(),
                    "bytes":pixels.len(),"sha256":Sha256::digest(pixels).iter().map(|byte|format!("{byte:02x}")).collect::<String>(),"path":path});
                image.try_into_dynamic().map_err(|error| error.to_string())?.to_rgb8()
                    .save(&path).map_err(|error| error.to_string())?;
                Ok(proof)
            })();
            let _ = sender.try_send(result);
        }).detach();
    });
    Ok(None)
}
