//! 自有夹具的原工具按钮端口；只改变操作模式和选择，不写作者内容、变换或历史。

use bevy::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    standalone::embedded_tools::EmbeddedAuthorTools,
    viewport::modes::{ViewportMode, ViewportModeState},
};

fn mode(name: &str) -> Result<ViewportMode, String> {
    match name {
        "Select" => Ok(ViewportMode::Select),
        "Move" => Ok(ViewportMode::Move),
        "Rotate" => Ok(ViewportMode::Rotate),
        "Scale" => Ok(ViewportMode::Scale),
        "Edit" => Ok(ViewportMode::Edit),
        _ => Err("未知原视口模式".into()),
    }
}

pub fn press_mode(world: &mut World, name: &str) -> Result<Entity, String> {
    let _ = super::probe(world)?;
    crate::viewport::modes::validation_press_mode(world, mode(name)?)
}

pub fn selection(world: &World) -> Option<String> {
    super::live::render_selection(world)
}

pub fn select(world: &mut World, id: Option<&str>) -> Result<(), String> {
    super::live::set_render_selection(world, id)
}

/// 读取生产发布后的语义；该值只作为跨进程预期，游戏端还必须提供真实几何与GPU像素。
pub fn probe(world: &World) -> Result<Value, String> {
    let _ = super::probe(world)?;
    let tools = world
        .get_resource::<EmbeddedAuthorTools>()
        .ok_or("原工具语义尚未发布")?;
    if let Some(error) = &tools.error {
        return Err(error.clone());
    }
    let bytes = serde_json::to_vec(&tools.preview).map_err(|error| error.to_string())?;
    Ok(json!({
        "mode": format!("{:?}", world.resource::<ViewportModeState>().mode),
        "selection": selection(world), "revision": tools.revision, "generation": tools.generation,
        "preview": tools.preview,
        "semantic_sha256": Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "legacy_lines": world.resource::<crate::standalone::embedded_overlay::EmbeddedOverlay>().lines.len(),
    }))
}

/// 清选择取证同时等待新语义和旧辅助线撤回，不能把旧通道为空当作全部工具已消失。
pub fn empty(world: &World) -> bool {
    world
        .get_resource::<EmbeddedAuthorTools>()
        .is_some_and(|tools| tools.error.is_none() && tools.preview.items.is_empty())
        && world
            .get_resource::<crate::standalone::embedded_overlay::EmbeddedOverlay>()
            .is_some_and(|overlay| overlay.lines.is_empty())
}
