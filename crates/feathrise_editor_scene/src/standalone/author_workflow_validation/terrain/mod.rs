//! 仅自有工程安装的原地形/地表/Blade工作流夹具；父流程负责最终原Save与F12/重开。
//!
//! 【验证边界】输入为ECS鼠标源和原按钮动作，命中、笔划、后台采用及Undo/Redo全部复用正式实现。
//! 不声称覆盖物理鼠标手感；不直接改像素、Mesh或JSON，不安装于普通编辑会话。

mod capture;
mod drive;
mod pointer;
mod resources;
mod types;

use bevy::prelude::*;
use resources::TerrainWorkflow;
use types::{Clock, Phase};

pub(super) const BLADE_SOURCE: &str = "scene/验收/Blade叶片.glb";

pub(super) fn install(app: &mut App) {
    if std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_none()
        || std::env::var("NEW_EDITOR_NATIVE_SMOKE_SAVE").as_deref() != Ok("1")
        || std::env::var("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS").as_deref() != Ok("1")
    {
        return;
    }
    app.init_resource::<TerrainWorkflow>().add_systems(
        Update,
        pointer::inject
            .in_set(crate::SceneEditorSystemSet::Cursor)
            .before(crate::viewport::sync_editor_viewport_camera),
    );
}

pub(super) fn begin(world: &mut World) -> Result<(), String> {
    if !world.contains_resource::<TerrainWorkflow>() {
        return Err("原地形工具夹具尚未显式安装".into());
    }
    if world.resource::<TerrainWorkflow>().phase != Phase::Idle {
        return Err("原地形工具夹具不能重复开始".into());
    }
    let editor = world.resource::<crate::resources::SceneEditorState>();
    let view = world.resource::<crate::resources::SceneEditorViewportState>();
    let old = (
        editor.view,
        editor.orthographic,
        view.center,
        view.extent,
        view.yaw,
        view.pitch,
    );
    let session = world.resource::<crate::authoring::EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("原场景草稿未就绪")?;
    let map = draft.scene_id.clone();
    let terrain = draft
        .manifest
        .terrains
        .as_ref()
        .and_then(|objects| objects.first())
        .filter(|object| object.terrain.kind == feathrise_scene_content::SceneTerrainKind::Heightfield)
        .map(|object| object.id.clone())
        .ok_or("有限夹具缺少真实Heightfield示例地形")?;
    let mut state = world.resource_mut::<TerrainWorkflow>();
    state.view = Some(old);
    state.scene_id = Some(map);
    state.terrain = Some(terrain);
    state.clock = Clock::default();
    state.advance(Phase::Focus);
    Ok(())
}

pub(super) fn advance(world: &mut World) -> Option<Result<serde_json::Value, String>> {
    if !world.contains_resource::<TerrainWorkflow>() {
        return Some(Err("原地形工具夹具未安装".into()));
    }
    world.resource_scope(|world, mut state: Mut<TerrainWorkflow>| {
        if let Some(entity) = state.pulse.take() {
            let _ = world.despawn(entity);
        }
        if state.phase == Phase::Idle {
            return None;
        }
        if state.phase == Phase::Complete {
            return Some(Ok(serde_json::json!({
                "complete": true,
                "input": "original ECS pointer/button actions; physical mouse not exercised",
                "scene_id": state.scene_id.as_ref().map(|id| id.as_str()),
                "terrain_id": state.terrain.as_ref().map(|id| id.as_str()),
                "asset_root": feathrise_editor_core::workspace_asset_root(),
                "surface_layer_id": state.surface_layer,
                "blade_layer_id": state.blade_layer,
                "blade_output_id": state.blade_output,
                "blade_source": BLADE_SOURCE,
                "strokes": state.proofs,
                "save": "pending parent original map Save; no implicit save by fixture",
            })));
        }
        if state.phase == Phase::Failed {
            return (!state.mouse_owned).then(|| {
                Err(state
                    .failure
                    .clone()
                    .unwrap_or_else(|| "原地形工具验收失败".into()))
            });
        }
        if let Err(error) = drive::advance(world, &mut state) {
            state.failure = Some(error);
            state.advance(Phase::Failed);
            pointer::restore(world, &mut state);
        }
        None
    })
}
