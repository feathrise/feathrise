//! 显式独立编辑器验收夹具：原坐标创建→原保存按钮→原正式交接，不维护另一套保存逻辑。

use feathrise_editor_ui::UiInteraction;
#[path = "validation_fixture_map_tools.rs"]
mod map_tools;
#[cfg(test)]
#[path = "validation_fixture_tests.rs"]
mod tests;
pub use super::author_workflow_validation::AuthorWorkflowProof;
pub use map_tools::{MapToolProof, PaintSourceProof};

use crate::{
    authoring::{
        DraftSaveTask, EditorAuthoringSession, EditorCommand, TerrainLiveApplyState,
        WaterFileHandoff,
        basic_shapes::{BasicShapeState, CuboidCreationRequest},
    },
    components::MapEditorActionButton,
    resources::MapEditorState,
    types::EditorAction,
};
use bevy::prelude::*;
use feathrise_map_content::{MapId, MapPlacementAssetTarget};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const POSITION: [f32; 3] = [4.0, 1.0, 0.0];

/// 安装权限独立于可变阶段资源；resource_scope暂时借出Fixture时，嵌套原动作仍属同一验收。
#[derive(Resource)]
struct FixtureInstalled;

/// 固定文本只属于本轮自有验收工程，不参与游戏加载的地图/预设源。
pub const CLOSE_DOCUMENT_PATH: &str = crate::ui::asset_preview::documents::validation::PATH;

/// 实窗关闭回归先用原环境作者入口建立真实草稿，再模拟界面脏标记提前清零。
pub const CLOSE_MAP_SKYBOX_YAW: f32 = 37.0;

pub fn edit_close_map_with_stale_dirty_marker(world: &mut World) -> Result<u64, String> {
    if !world.contains_resource::<FixtureInstalled>() {
        return Err("地图保存验收未安装".into());
    }
    world.resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
        let mut editor = world.resource_mut::<MapEditorState>();
        let draft = session.draft.as_ref().filter(|draft| {
            draft.map_id.as_str() == "M1-SAMPLE" && editor.selected_map_id.as_ref() == Some(&draft.map_id)
        }).ok_or("地图关闭验收必须持有本轮样例草稿")?;
        if editor.dirty || editor.save_requested || session.saving || session.has_map_data_changes() {
            return Err("地图关闭验收要求先完成原地图保存".into());
        }
        let before = draft.manifest.environment.clone();
        let mut after = before.clone();
        after.skybox_yaw_degrees = CLOSE_MAP_SKYBOX_YAW;
        after.validate()?;
        crate::authoring::commit_environment_change(&mut session, &mut editor, before, after, "关闭验收环境草稿");
        if !editor.dirty || !session.has_map_data_changes() {
            return Err("原环境编辑没有生成待保存地图数据".into());
        }
        editor.dirty = false; // 仅注入截图中的脏标记分歧，仍由原保存系统写盘与产生成功回执。
        Ok(session.successful_save_sequence)
    })
}

pub fn close_map_save_sequence(world: &World, pending_data: bool) -> Option<u64> {
    let editor = world.get_resource::<MapEditorState>()?;
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    (draft.map_id.as_str() == "M1-SAMPLE" && editor.selected_map_id.as_ref() == Some(&draft.map_id)
        && draft.manifest.environment.skybox_yaw_degrees == CLOSE_MAP_SKYBOX_YAW
        && !editor.dirty && !editor.save_requested && !session.saving && session.has_map_data_changes() == pending_data)
        .then_some(session.successful_save_sequence)
}

pub fn request_close_document(world: &mut World) -> Result<(), String> {
    if !world.contains_resource::<FixtureInstalled>() { return Err("普通文档验收未安装".into()); }
    crate::ui::asset_preview::documents::validation::open(world)
}

pub fn close_document_ready(world: &World) -> Result<bool, String> {
    if !world.contains_resource::<FixtureInstalled>() { return Err("普通文档验收未安装".into()); }
    crate::ui::asset_preview::documents::validation::ready(world)
}

pub fn edit_close_document(world: &mut World) -> Result<serde_json::Value, String> {
    if !world.contains_resource::<FixtureInstalled>() { return Err("普通文档验收未安装".into()); }
    crate::ui::asset_preview::documents::validation::edit(world)
}

pub fn close_document_snapshot(world: &World) -> Option<serde_json::Value> {
    if !world.contains_resource::<FixtureInstalled>() { return None; }
    crate::ui::asset_preview::documents::validation::snapshot(world)
}

/// 语言专项已在宿主确认独占工程，仅开启普通文档探针，不启动地图验收驱动。
#[cfg(feature = "validation")]
pub fn install_locale_document_probe(app: &mut App) {
    app.insert_resource(FixtureInstalled);
}

#[cfg(feature = "validation")]
pub fn redo_locale_document(world: &mut World) -> Result<(), String> {
    if !world.contains_resource::<FixtureInstalled>() {
        return Err("普通文档验收未安装".into());
    }
    crate::ui::asset_preview::documents::validation::redo(world)
}

/// 只有正式作者事务的回执才能完成；宿主可写入自己的验收报告，不持有私有作者资源。
#[derive(Clone, Debug, serde::Serialize)]
pub struct SavedEditProof {
    pub map_id: String,
    pub source_project_root: PathBuf,
    pub source_asset_root: PathBuf,
    pub new_placement_id: Option<String>,
    pub translation: [f32; 3],
    pub original_save_sequence: u64,
    pub successful_save_sequence: u64,
    pub author_revision_before: u64,
    pub author_revision_after: u64,
    pub draft_clean: bool,
    pub handoff_complete: bool,
    pub complete: bool,
    pub error: Option<String>,
    pub map_tools: Option<MapToolProof>,
    pub author_tools: Option<AuthorWorkflowProof>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Creating,
    MapTools,
    AuthorTools,
    SaveButton,
    SavePulse,
    Saving,
    Complete,
    Failed,
}

#[derive(Resource)]
struct Fixture {
    phase: Phase,
    proof: Option<SavedEditProof>,
    map: Option<MapId>,
    count_before: usize,
    save_button: Option<Entity>,
    click_pulse: Option<Entity>,
    started_at: Instant,
}

/// 普通会话不安装资源或帧系统；必须同时显式提供隔离验收目录和SAVE=1。
pub fn install(app: &mut App) -> bool {
    if std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_none()
        || std::env::var("NEW_EDITOR_NATIVE_SMOKE_SAVE").as_deref() != Ok("1")
    {
        return false;
    }
    if !app.world().contains_resource::<Fixture>() {
        map_tools::install(app);
        super::author_workflow_validation::install(app);
        app.insert_resource(FixtureInstalled).insert_resource(Fixture {
            phase: Phase::Idle,
            proof: None,
            map: None,
            count_before: 0,
            save_button: None,
            click_pulse: None,
            started_at: Instant::now(),
        })
        .add_systems(Last, drive);
    }
    true
}

/// 在原地图3D画布就绪后请求一次；位置为2m立方体的世界中心，底面正好落在样例Y=0地面。
pub fn request_edit_and_save(world: &mut World, map_id: &str) -> Result<(), String> {
    if !world.contains_resource::<Fixture>() {
        return Err("保存验收夹具未显式启用".into());
    }
    if world.resource::<Fixture>().phase != Phase::Idle {
        return Err("本实例的保存验收已经请求，不能重复添加对象".into());
    }
    if !super::preview_ready_for(world, map_id) {
        return Err("原地图作者3D尚未就绪".into());
    }
    if let Some(reason) = super::saved_play_blocker(world) {
        return Err(reason);
    }
    let map = MapId::parse(map_id)?;
    let paths = feathrise_editor_core::editor_project_paths().ok_or("验收实例未绑定独立工程来源")?;
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("原地图草稿未就绪")?;
    let count_before = draft.placements.placements.len();
    let revision = session.revision;
    let sequence = session.successful_save_sequence;
    let save_button = world
        .query::<(Entity, &MapEditorActionButton)>()
        .iter(world)
        .find_map(|(entity, action)| (action.0 == EditorAction::Save).then_some(entity))
        .ok_or("原保存地图按钮尚未生成")?;
    let mut shapes = world
        .get_resource_mut::<BasicShapeState>()
        .ok_or("原基础网格作者系统未安装")?;
    if shapes.creation_request.is_some() {
        return Err("原网格创建槽已经占用".into());
    }
    shapes.creation_identity = Some((map.clone(), revision));
    shapes.creation_request = Some(CuboidCreationRequest {
        target: MapPlacementAssetTarget::DEFAULT_CUBOID,
        position: Some(Vec3::from_array(POSITION)),
    });
    let mut fixture = world.resource_mut::<Fixture>();
    fixture.phase = Phase::Creating;
    fixture.map = Some(map);
    fixture.count_before = count_before;
    fixture.save_button = Some(save_button);
    fixture.started_at = Instant::now();
    fixture.proof = Some(SavedEditProof {
        map_id: map_id.into(),
        source_project_root: paths.project_root().to_owned(),
        source_asset_root: paths.asset_root().to_owned(),
        new_placement_id: None,
        translation: POSITION,
        original_save_sequence: sequence,
        successful_save_sequence: sequence,
        author_revision_before: revision,
        author_revision_after: revision,
        draft_clean: false,
        handoff_complete: false,
        complete: false,
        error: None,
        map_tools: None,
        author_tools: None,
    });
    Ok(())
}

pub fn result(world: &World) -> Option<SavedEditProof> {
    world.get_resource::<Fixture>()?.proof.clone()
}

pub fn author_tools_enabled() -> bool {
    super::author_workflow_validation::enabled()
}

/// 只读当前小验收图真正打开的作者文档，用于退出后A→B→A核对；不从磁盘伪造World内容。
pub fn current_author_snapshot(world: &World) -> Option<serde_json::Value> {
    if std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_none() {
        return None;
    }
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    if draft.map_id.as_str() != "M1-SAMPLE"
        || draft.placements.placements.len() > 16
        || draft
            .manifest
            .terrains
            .as_ref()
            .is_some_and(|objects| objects.len() > 4)
        || draft
            .water
            .as_ref()
            .is_some_and(|water| water.rivers.len() > 4)
    {
        return None;
    }
    Some(
        serde_json::json!({"map_id":draft.map_id.as_str(),"terrains":draft.manifest.terrains,
        "skybox_yaw_degrees":draft.manifest.environment.skybox_yaw_degrees,
        "placements":draft.placements.placements,"surface":draft.surface,"vegetation":draft.vegetation,
        "water":draft.water,"focused_terrain":draft.terrain_documents.active.as_ref().map(|active|active.definition.id.as_str())}),
    )
}

/// 同一有限验收World的内容与真实历史元数据；不复制命令正文、整条历史或像素来证明恢复。
pub fn recovery_author_snapshot(world: &World) -> Option<serde_json::Value> {
    if !world.contains_resource::<FixtureInstalled>() { return None; }
    let author = current_author_snapshot(world)?;
    let session = world.get_resource::<EditorAuthoringSession>()?;
    Some(serde_json::json!({
        "author": author,
        "revision": session.revision,
        "undo_len": session.undo.len(),
        "redo_len": session.redo.len(),
        "undo_tail": session.undo.last().map(|command| history_identity(command, 0)),
        "redo_tail": session.redo.last().map(|command| history_identity(command, 0)),
        "scope": "same_world_history_metadata_and_bounded_current_document"
    }))
}

fn history_identity(command: &EditorCommand, depth: u8) -> serde_json::Value {
    // 只读末条记录的种类和固定对象身份；相同App内比较，不把该内部种类编号当文件协议。
    let kind = format!("{:?}", std::mem::discriminant(command));
    match command {
        EditorCommand::TerrainCanvas {id, command} if depth < 4 => serde_json::json!({
            "kind": kind, "terrain": id.as_str(), "inner": history_identity(command, depth + 1)
        }),
        EditorCommand::RootWater(inner) if depth < 4 => serde_json::json!({
            "kind": kind, "inner": history_identity(inner, depth + 1)
        }),
        EditorCommand::PlacementAdded(record) | EditorCommand::PlacementRemoved {record, ..} => {
            serde_json::json!({"kind":kind,"placement":record.id})
        }
        EditorCommand::PlacementChanged {before, ..} => serde_json::json!({"kind":kind,"placement":before.id}),
        _ => serde_json::json!({"kind":kind}),
    }
}

/// 重开验收通过原画布选择入口读取已保存私有文档；不直接填充草稿或把文件值冒充World采用。
pub fn focus_validation_document(world: &mut World) -> Result<bool, String> {
    if std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_none() {
        return Err("未显式启用原生验收".into());
    }
    use crate::authoring::terrain_documents::{self, TerrainDocumentTask};
    use crate::ui::terrain_objects::TerrainObjectsEditorState;
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("原地图草稿尚未载入")?;
    if draft.map_id.as_str() != "M1-SAMPLE" {
        return Err("重开验收不能选择其他地图画布".into());
    }
    let target = draft
        .manifest
        .terrains
        .as_ref()
        .and_then(|objects| objects.first())
        .map(|object| object.id.clone())
        .ok_or("示例地图缺少地形画布")?;
    if draft
        .terrain_documents
        .active
        .as_ref()
        .is_some_and(|active| active.definition.id == target)
        && terrain_documents::canvas_writable(session, world.resource::<MapEditorState>())
    {
        return Ok(true);
    }
    if draft.terrain_documents.loading {
        return Ok(false);
    }
    let accepted = world.resource_scope(|world, session: Mut<EditorAuthoringSession>| {
        world.resource_scope(|world, mut task: Mut<TerrainDocumentTask>| {
            terrain_documents::request_focus(
                &mut task,
                &mut world.resource_mut::<TerrainObjectsEditorState>(),
                session.draft.as_ref().unwrap(),
                &target,
            )
        })
    });
    if accepted {
        world.resource_mut::<MapEditorState>().terrain_canvas_target = Some(target);
    }
    Ok(false)
}

/// 仅已显式启用的有限夹具可返回原地图页；调用者负责在原Input消费后清理脉冲实体。
pub fn request_map_tab(world: &mut World) -> Result<Entity, String> {
    if !world.contains_resource::<FixtureInstalled>() {
        return Err("保存验收夹具未显式启用，不能代发地图标签动作".into());
    }
    use crate::ui::vegetation_preset_editor::{PresetEditorAction, PresetEditorButton};
    // 与原地图标签同一入口，不改活动页、相机或技能标签；真实Presentation决定地图预览何时恢复。
    Ok(world
        .spawn((
            PresetEditorButton(PresetEditorAction::ActivateMapTab),
            feathrise_editor_ui::EditorButtonClick::triggered(),
            UiInteraction::Pressed,
        ))
        .id())
}

fn fail(fixture: &mut Fixture, reason: impl Into<String>) {
    fixture.phase = Phase::Failed;
    if let Some(proof) = fixture.proof.as_mut() {
        proof.error = Some(reason.into());
    }
}

/// 只读固定资源、一个末尾作者记录和一个保存按钮；真正写盘、派生及交接仍由原后台任务完成。
fn drive(world: &mut World) {
    world.resource_scope(|world, mut fixture: Mut<Fixture>| {
        if let Some(entity) = fixture.click_pulse.take()
            && let Ok(mut button) = world.get_entity_mut(entity)
        {
            button.insert((UiInteraction::None, feathrise_editor_ui::EditorButtonClick::default()));
        } // 原Input已消费上一帧单次释放脉冲，不能在后续Changed中重复保存。
        if matches!(fixture.phase, Phase::Idle | Phase::Complete | Phase::Failed) { return; }
        let budget = if super::author_workflow_validation::enabled() { 600 } else { 180 };
        if fixture.started_at.elapsed() > Duration::from_secs(budget) {
            fail(&mut fixture, format!("原编辑保存验收超时：{}", super::author_status(world).unwrap_or("未知状态")));
            return;
        }
        let map = fixture.map.clone().expect("活跃夹具必须冻结地图");
        let session = world.resource::<EditorAuthoringSession>();
        let editor = world.resource::<MapEditorState>();
        if editor.selected_map_id.as_ref() != Some(&map)
            || session.draft.as_ref().is_none_or(|draft| draft.map_id != map)
        {
            fail(&mut fixture, "保存验收期间作者地图身份发生变化"); return;
        }
        match fixture.phase {
            Phase::Creating => {
                let proof = fixture.proof.as_ref().expect("活跃夹具必须有证据");
                let record = session.draft.as_ref().and_then(|draft| draft.placements.placements.last());
                let accepted = record.filter(|record| {
                    session.draft.as_ref().is_some_and(|draft| draft.placements.placements.len() == fixture.count_before + 1)
                        && session.revision != proof.author_revision_before && editor.dirty
                        && record.asset_target == MapPlacementAssetTarget::DEFAULT_CUBOID
                        && record.translation == POSITION
                        && matches!(session.undo.last(), Some(EditorCommand::PlacementAdded(added)) if added.id == record.id)
                });
                if let Some(record) = accepted {
                    let id = record.id.clone();
                    let revision = session.revision;
                    let proof = fixture.proof.as_mut().unwrap();
                    proof.new_placement_id = Some(id);
                    proof.author_revision_after = revision;
                    match map_tools::begin(world) {
                        Ok(true) => fixture.phase = Phase::MapTools,
                        Ok(false) => begin_author_tools(world, &mut fixture),
                        Err(error) => fail(&mut fixture, error),
                    }
                } else if world.resource::<BasicShapeState>().creation_request.is_none() {
                    fail(&mut fixture, format!("原坐标创建未受理：{}", editor.status));
                }
            }
            Phase::MapTools => {
                map_tools::advance(world);
                if let Some(proof) = map_tools::report(world) {
                    let error = proof.error.clone();
                    let complete = proof.complete;
                    fixture.proof.as_mut().unwrap().map_tools = Some(proof);
                    if let Some(error) = error { fail(&mut fixture, error); }
                    else if complete {
                        fixture.proof.as_mut().unwrap().author_revision_after = world.resource::<EditorAuthoringSession>().revision;
                        begin_author_tools(world, &mut fixture);
                    }
                }
            }
            Phase::AuthorTools => {
                if let Some(proof) = super::author_workflow_validation::advance(world) {
                    let error = proof.error.clone();
                    let complete = proof.complete;
                    fixture.proof.as_mut().unwrap().author_tools = Some(proof);
                    if let Some(error) = error { fail(&mut fixture, error); }
                    else if complete { fixture.phase = Phase::SaveButton; }
                }
            }
            Phase::SaveButton => {
                // 专业页可重建地图工具栏；只采用当前原按钮，等待Presentation恢复，不重用旧Entity。
                let Some(entity) = world.query::<(Entity, &MapEditorActionButton)>().iter(world)
                    .find_map(|(entity, action)| (action.0 == EditorAction::Save).then_some(entity)) else { return; };
                fixture.save_button = Some(entity);
                fixture.proof.as_mut().unwrap().author_revision_after = world.resource::<EditorAuthoringSession>().revision;
                if world.get::<bevy::ui::InteractionDisabled>(entity).is_some()
                    || world.get_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
                        .is_some_and(|guard| guard.blocked())
                { return; }
                if world.get::<MapEditorActionButton>(entity).is_none() {
                    fail(&mut fixture, "原保存按钮已被重建，未发送陈旧按钮动作"); return;
                }
                world.entity_mut(entity).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
                fixture.click_pulse = Some(entity);
                fixture.phase = Phase::SavePulse;
            }
            Phase::SavePulse | Phase::Saving => {
                let sequence = session.successful_save_sequence;
                let proof = fixture.proof.as_mut().unwrap();
                proof.successful_save_sequence = sequence;
                if sequence == proof.original_save_sequence {
                    let pending = editor.save_requested || session.saving
                        || world.get_resource::<DraftSaveTask>().is_some_and(|task| task.task.is_some());
                    if pending { fixture.phase = Phase::Saving; }
                    else { fail(&mut fixture, format!("原地图保存未完成：{}", editor.status)); }
                    return;
                }
                if let Some(reason) = world.get_resource::<WaterFileHandoff>()
                    .and_then(|handoff| handoff.unresolved_reason_for(&map))
                {
                    fail(&mut fixture, format!("原保存正式交接失败：{reason}")); return;
                }
                let clean = !editor.dirty && !editor.save_requested && !session.saving
                    && !session.has_map_data_changes();
                let handoff = world.get_resource::<WaterFileHandoff>()
                    .is_some_and(|handoff| handoff.pending_for(&map, session.water_revision, session.terrain_revision))
                    || world.get_resource::<TerrainLiveApplyState>()
                        .is_some_and(|state| state.file_handoff_pending_for(&map, session.terrain_revision));
                proof.draft_clean = clean;
                proof.handoff_complete = !handoff;
                if clean && !handoff {
                    proof.complete = true;
                    fixture.phase = Phase::Complete;
                } else { fixture.phase = Phase::Saving; }
            }
            _ => {}
        }
    });
}

fn begin_author_tools(world: &mut World, fixture: &mut Fixture) {
    match super::author_workflow_validation::begin(world) {
        Ok(true) => fixture.phase = Phase::AuthorTools,
        Ok(false) => fixture.phase = Phase::SaveButton,
        Err(error) => fail(fixture, error),
    }
}
