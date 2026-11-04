//! 串行投递原水网/建筑意图并核对真实历史；有限完成后把完整记录交回父保存与游戏验收。

use super::{pointer, resources::Fixture, types::Phase};
use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand, buildings,
        terrain_documents::{self, TerrainDocumentTask},
    },
    resources::{MapEditorState, MapEditorViewportState},
    types::{EditorTool, WaterNetworkAction},
    ui::{building_presets::BuildingPlacementControls, terrain_objects::TerrainObjectsEditorState},
};
use bevy::prelude::*;
use feathrise_map_content::{BuildingPresetRegistry, MapWaterRiver};
use serde_json::{Value, json};
use std::time::Duration;

pub(super) fn begin(world: &mut World) -> Result<(), String> {
    if !world.contains_resource::<Fixture>() {
        return Err("河流建筑验收未显式启用".into());
    }
    if world.resource::<Fixture>().phase != Phase::Idle {
        return Err("河流建筑验收拒绝重复开始".into());
    }
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session
        .draft
        .as_ref()
        .ok_or("河流建筑验收缺少当前原地图草稿")?;
    let terrain = draft
        .manifest
        .terrains
        .as_ref()
        .and_then(|objects| {
            objects
                .iter()
                .find(|object| object.id.as_str() == "starter-ground")
        })
        .ok_or("河流建筑验收缺少工程原starter-ground网格")?
        .id
        .clone();
    let map = draft.map_id.clone();
    if !world
        .get_resource::<BuildingPresetRegistry>()
        .is_some_and(|registry| registry.resolve("sample-building").is_some())
    {
        return Err("河流建筑验收缺少本工程sample-building预设".into());
    }
    let editor = world.resource::<MapEditorState>();
    let viewport = world.resource::<MapEditorViewportState>();
    let view = (
        editor.view,
        editor.orthographic,
        viewport.center,
        viewport.extent,
        viewport.yaw,
        viewport.pitch,
    );
    let controls = world.resource::<BuildingPlacementControls>().clone();
    let mut fixture = world.resource_mut::<Fixture>();
    fixture.map = Some(map);
    fixture.terrain = Some(terrain);
    fixture.view = Some(view);
    fixture.controls = Some(controls);
    fixture.next(Phase::Focus);
    Ok(())
}

pub(super) fn advance(world: &mut World) -> Option<Result<Value, String>> {
    if !world.contains_resource::<Fixture>() {
        return None;
    }
    world.resource_scope(|world, mut fixture: Mut<Fixture>| {
        if let Some(result) = &fixture.result {
            return Some(result.clone());
        }
        if fixture.phase == Phase::Idle {
            return None;
        }
        fixture.frames += 1;
        let result = step(world, &mut fixture);
        if let Err(error) = result {
            fixture.next(Phase::Failed);
            // 不清历史、不删草稿；失败后的按键释放仍由Cursor阶段唯一输入系统执行。
            pointer::restore(world, &mut fixture);
            fixture.result = Some(Err(error));
        }
        fixture.result.clone()
    })
}

fn step(world: &mut World, fixture: &mut Fixture) -> Result<(), String> {
    if fixture.since.elapsed() > Duration::from_secs(90) {
        let editor = world.resource::<MapEditorState>();
        let session = world.resource::<EditorAuthoringSession>();
        let river = current_river(session, fixture).map(|river| (&river.id, river.points.len()));
        return Err(format!(
            "河流建筑原操作超时：{:?}；点{}；river={river:?}；undo={} redo={}；点击={:?}；{}",
            fixture.phase,
            fixture.point,
            session.undo.len(),
            session.redo.len(),
            editor.ground_click_world,
            editor.status
        ));
    }
    if world
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .map(|draft| &draft.map_id)
        != fixture.map.as_ref()
    {
        return Err("河流建筑验收期间原活动地图发生变化".into());
    }
    match fixture.phase {
        Phase::Focus => {
            world
                .resource_scope(|world, mut task: Mut<TerrainDocumentTask>| {
                    world.resource_scope(|world, mut selection: Mut<TerrainObjectsEditorState>| {
                        let session = world.resource::<EditorAuthoringSession>();
                        terrain_documents::request_focus(
                            &mut task,
                            &mut selection,
                            session.draft.as_ref().unwrap(),
                            fixture.terrain.as_ref().unwrap(),
                        )
                    })
                })
                .then_some(())
                .ok_or("原河流画布焦点请求未受理")?;
            fixture.next(Phase::WaitFocus);
        }
        Phase::WaitFocus => {
            let session = world.resource::<EditorAuthoringSession>();
            let editor = world.resource::<MapEditorState>();
            let draft = session.draft.as_ref().unwrap();
            if terrain_documents::canvas_writable(session, editor)
                && draft
                    .terrain_documents
                    .active
                    .as_ref()
                    .map(|active| &active.definition.id)
                    == fixture.terrain.as_ref()
                && !session.saving
            {
                if draft.water.is_none() {
                    return Err("所选原画布没有水域作者文档".into());
                }
                fixture.river_ids = draft
                    .water
                    .as_ref()
                    .unwrap()
                    .rivers
                    .iter()
                    .map(|river| river.id.clone())
                    .collect();
                fixture.next(Phase::RequestRiver);
            }
        }
        Phase::RequestRiver => {
            fixture.undo = world.resource::<EditorAuthoringSession>().undo.len();
            world
                .resource_mut::<MapEditorState>()
                .water_network_action_requested = Some(WaterNetworkAction::RiverAdd);
            fixture.next(Phase::WaitRiver);
        }
        Phase::WaitRiver => {
            let session = world.resource::<EditorAuthoringSession>();
            let water = session.draft.as_ref().unwrap().water.as_ref().unwrap();
            let new: Vec<_> = water
                .rivers
                .iter()
                .filter(|river| !fixture.river_ids.contains(&river.id))
                .collect();
            if new.is_empty() {
                return Ok(());
            }
            if new.len() != 1 || !new[0].points.is_empty() || session.undo.len() != fixture.undo + 1
            {
                return Err("原新建河流没有产生唯一空路径及一笔历史".into());
            }
            fixture.river = Some(new[0].clone());
            fixture.undo = session.undo.len();
            fixture.point = 0;
            fixture.next(Phase::HoverRiver);
        }
        Phase::HoverRiver => {
            if fixture.frames >= 4 && ground_ready(world, fixture, EditorTool::RiverSpline) {
                fixture.next(Phase::PressRiver);
            }
        }
        Phase::PressRiver => fixture.next(Phase::WaitPoint),
        Phase::WaitPoint => {
            if world
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
                || fixture.frames < 2
            {
                return Ok(());
            }
            let session = world.resource::<EditorAuthoringSession>();
            let river = current_river(session, fixture).ok_or("原河流稳定身份已丢失")?;
            if river.points.len() <= fixture.point {
                return Ok(());
            }
            if river.points.len() != fixture.point + 1 || session.undo.len() != fixture.undo + 1 {
                return Err("一次原地面单击没有产生唯一控制点/历史".into());
            }
            fixture.undo = session.undo.len();
            fixture.river = Some(river.clone());
            if fixture.point < 2 {
                fixture.point += 1;
                fixture.next(Phase::HoverRiver);
            } else {
                validate_curve(river)?;
                if !matches!(
                    core_command(session.undo.last().ok_or("河流绘制没有原历史")?),
                    EditorCommand::WaterDefinitionsChanged { .. }
                ) {
                    return Err("河流控制点没有进入原完整水网撤销命令".into());
                }
                world.resource_mut::<MapEditorState>().undo_requested = true;
                fixture.next(Phase::WaitRiverUndo);
            }
        }
        Phase::WaitRiverUndo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() == fixture.undo {
                return Ok(());
            }
            let river = current_river(session, fixture).ok_or("撤销最后控制点丢失河流稳定ID")?;
            let saved = fixture.river.as_ref().unwrap();
            if session.undo.len() + 1 != fixture.undo || river.points != saved.points[..2] {
                return Err("原河流撤销没有精确还原前两控制点".into());
            }
            world.resource_mut::<MapEditorState>().redo_requested = true;
            fixture.next(Phase::WaitRiverRedo);
        }
        Phase::WaitRiverRedo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() < fixture.undo {
                return Ok(());
            }
            if current_river(session, fixture) != fixture.river.as_ref()
                || session.undo.len() != fixture.undo
            {
                return Err("原河流重做改变了稳定ID、控制点或水参数".into());
            }
            fixture.placement_ids = session
                .draft
                .as_ref()
                .unwrap()
                .placements
                .placements
                .iter()
                .map(|record| record.id.clone())
                .collect();
            let mut controls = world.resource_mut::<BuildingPlacementControls>();
            controls.yaw_degrees = 90.;
            controls.grid_step_meters = 1.;
            controls.connection_snap = false;
            controls.level_terrain = false;
            world
                .resource_mut::<MapEditorState>()
                .building_selected_preset_id = Some("sample-building".into());
            fixture.next(Phase::HoverBuilding);
        }
        Phase::HoverBuilding => {
            // 原空间索引随水网修订重建；等待多帧和真实Collider悬停，不伪造受检落印结果。
            if fixture.frames >= 12
                && ground_ready(world, fixture, EditorTool::Building)
                && !buildings::exit_pending(world)
            {
                fixture.undo = world.resource::<EditorAuthoringSession>().undo.len();
                fixture.next(Phase::PressBuilding);
            }
        }
        Phase::PressBuilding => fixture.next(Phase::WaitBuilding),
        Phase::WaitBuilding => {
            if world
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
                || buildings::exit_pending(world)
            {
                return Ok(());
            }
            let session = world.resource::<EditorAuthoringSession>();
            let new: Vec<_> = session
                .draft
                .as_ref()
                .unwrap()
                .placements
                .placements
                .iter()
                .filter(|record| !fixture.placement_ids.contains(&record.id))
                .collect();
            if new.is_empty() {
                return Ok(());
            }
            if new.len() != 1
                || new[0].building_preset_id.as_deref() != Some("sample-building")
                || session.undo.len() != fixture.undo + 1
            {
                return Err("建筑原校验没有形成唯一规范建筑及单笔历史".into());
            }
            let record = new[0];
            if (record.translation[0] + 16.).abs() > 0.1
                || (record.translation[2] + 16.).abs() > 0.1
                || record.yaw_degrees != 90.
                || record.scale != [1.; 3]
            {
                return Err(format!("建筑受检姿态不符：{:?}", record));
            }
            fixture.building = Some(record.clone());
            fixture.undo = session.undo.len();
            world.resource_mut::<MapEditorState>().undo_requested = true;
            fixture.next(Phase::WaitBuildingUndo);
        }
        Phase::WaitBuildingUndo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() == fixture.undo {
                return Ok(());
            }
            let records = &session.draft.as_ref().unwrap().placements.placements;
            if session.undo.len() + 1 != fixture.undo
                || records.len() != fixture.placement_ids.len()
                || records
                    .iter()
                    .any(|record| record.id == fixture.building.as_ref().unwrap().id)
            {
                return Err("建筑原撤销没有仅移除新稳定实例".into());
            }
            world.resource_mut::<MapEditorState>().redo_requested = true;
            fixture.next(Phase::WaitBuildingRedo);
        }
        Phase::WaitBuildingRedo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() < fixture.undo || buildings::exit_pending(world) {
                return Ok(());
            }
            let saved = fixture.building.as_ref().unwrap();
            let draft = session.draft.as_ref().unwrap();
            if session.undo.len() != fixture.undo
                || draft
                    .placements
                    .placements
                    .iter()
                    .find(|record| record.id == saved.id)
                    != Some(saved)
                || current_river(session, fixture) != fixture.river.as_ref()
            {
                return Err("建筑重做未精确恢复原稳定ID/变换，或改变了先前河流".into());
            }
            let report = json!({
                "scope":"original_ground_click_river_and_validated_building_history",
                "input_scope":"ecs_pointer_to_original_camera_collider_not_physical_mouse",
                "map_id":fixture.map.as_ref().unwrap().as_str(),
                "terrain_id":fixture.terrain.as_ref().unwrap().as_str(),
                "water_source_path":draft.water_asset_path,
                "river":fixture.river,
                "river_undo_redo":true,
                "building":fixture.building,
                "building_undo_redo":true,
                "awaits_parent_save":true,
                "complete":true,
            });
            pointer::restore(world, fixture);
            fixture.next(Phase::Complete);
            fixture.result = Some(Ok(report));
        }
        Phase::Idle | Phase::Complete | Phase::Failed => {}
    }
    Ok(())
}

fn ground_ready(world: &World, fixture: &Fixture, tool: EditorTool) -> bool {
    let editor = world.resource::<MapEditorState>();
    let session = world.resource::<EditorAuthoringSession>();
    world
        .get_resource::<crate::authoring::terrain_ground::TerrainGroundCursor>()
        .is_some_and(|ground| {
            ground.input_authority().is_some()
                && ground.identity.as_ref().is_some_and(|identity| {
                    identity.tool == tool
                        && identity.revision == session.revision
                        && identity.terrain_revision == session.terrain_revision
                })
                && ground.terrain.as_ref() == fixture.terrain.as_ref()
        })
        && editor.cursor_world.is_some()
        && !world
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
}

fn current_river<'a>(
    session: &'a EditorAuthoringSession,
    fixture: &Fixture,
) -> Option<&'a MapWaterRiver> {
    let id = &fixture.river.as_ref()?.id;
    session
        .draft
        .as_ref()?
        .water
        .as_ref()?
        .rivers
        .iter()
        .find(|river| &river.id == id)
}

fn core_command(mut command: &EditorCommand) -> &EditorCommand {
    for _ in 0..8 {
        match command {
            EditorCommand::TerrainCanvas { command: inner, .. }
            | EditorCommand::RootWater(inner) => command = inner,
            _ => return command,
        }
    }
    command
}

fn validate_curve(river: &MapWaterRiver) -> Result<(), String> {
    if river.points.len() != 3 {
        return Err("河流必须有三枚原稳定控制点".into());
    }
    let a = Vec3::from_array(river.points[0].position);
    let b = Vec3::from_array(river.points[1].position);
    let c = Vec3::from_array(river.points[2].position);
    if (b - a).xz().perp_dot((c - a).xz()).abs() < 0.1 {
        return Err("三枚河流控制点共线，不能证明曲线样条".into());
    }
    if river.points.windows(2).any(|points| {
        points[0].id == points[1].id || points[1].position[1] > points[0].position[1] + 0.0001
    }) {
        return Err("河流原控制点身份或非上升流向无效".into());
    }
    Ok(())
}
