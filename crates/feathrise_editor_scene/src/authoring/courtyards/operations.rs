//! 院落盖印、整体移动/旋转/复制的单在途后台校验和原子采用。

use std::collections::BTreeSet; // 移动原组时排除旧成员，只处理固定上限的 ID。
use std::time::{SystemTime, UNIX_EPOCH}; // 新副本身份仅在作者显式操作边沿生成。

use bevy::prelude::*; // 鼠标输入、资源和实体命令。
use bevy::tasks::AsyncComputeTaskPool; // 五点地基与碰撞检查绝不在主线程执行。
use bevy::tasks::futures::check_ready; // 后台单任务只非阻塞轮询。
use bevy::ui::RelativeCursorPosition; // 仅中央编辑视口真实点击触发落印。
use feathrise_scene_content::{
    BuildingPresetDescriptor, BuildingPresetRegistry, CourtyardPresetRegistry,
    MAX_COURTYARD_MEMBERS_PER_PRESET, MAX_SCENE_PLACEMENTS_PER_SCENE, ScenePlacementRecord,
}; // 已解析规范预设、成员预算和正式记录复用场景公共协议。

use super::geometry::{instantiate_preset, transform_existing_member}; // 当前副本局部修改保留在整组变换中。
use super::state::{
    CourtyardAuthoringState, OperationIdentity, OperationResult, queue_visual_sync,
}; // 唯一任务槽和有界世界同步。
use super::types::CourtyardGroundWork;
use crate::authoring::buildings::{
    BuildingAuthoringState, courtyard_validator_snapshot, validate_courtyard_batch,
}; // 院落建筑与单栋建筑共享 BVH、地基和门前规则。
use crate::authoring::{EditorAuthoringSession, EditorCommand}; // 场景记录、预览与撤销共用原作者协议。
use crate::components::SceneEditorViewport; // 中央离屏视口是唯一落点入口。
use crate::resources::SceneEditorState; // 真实光标世界点和场景编辑状态。
use crate::ui::courtyard::{CourtyardUiRequest, CourtyardUiState, CourtyardViewportMode}; // 组合 UI 只提交操作意图。
use crate::ui::gltf_browser::EditorGltfBrowserState;
use feathrise_scene::DynamicClipmapGroundSurface; // 新副本选择转到第一条成员。

/// 下一次视口左键冻结当前操作；UI 框选已在 Cursor 阶段独占输入。
#[allow(clippy::too_many_arguments)]
pub(super) fn request_viewport(
    mouse: Res<ButtonInput<MouseButton>>,
    viewport: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    courtyard_registry: Option<Res<CourtyardPresetRegistry>>,
    building_registry: Option<Res<BuildingPresetRegistry>>,
    building_state: Res<BuildingAuthoringState>,
    clipmap: Option<Res<DynamicClipmapGroundSurface>>,
    mut ground_cursor: Option<ResMut<crate::viewport::VegetationGroundCursor>>,
    mut ui: ResMut<CourtyardUiState>,
    mut state: ResMut<CourtyardAuthoringState>,
) {
    let mode = ui.mode;
    let is_clipmap = session.draft.as_ref().is_some_and(|draft| {
        draft.manifest.terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap
    });
    if is_clipmap
        && (editor.save_requested
            || editor.close_requested
            || editor.new_scene_dialog_open
            || editor.suppress_world_pick_this_frame
            || session.saving)
    {
        if let Some(ground) = ground_cursor.as_deref_mut() {
            ground.cancel_courtyard_click();
        }
        return;
    }
    if !matches!(
        mode,
        CourtyardViewportMode::Stamp
            | CourtyardViewportMode::MoveGroup
            | CourtyardViewportMode::CopyGroup
    ) || (!is_clipmap && !mouse.just_pressed(MouseButton::Left))
        || !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || editor.suppress_world_pick_this_frame
        || session.saving
        || !viewport
            .single()
            .is_ok_and(RelativeCursorPosition::cursor_over)
    {
        return; // 非本域模式、保存期或视口外点击不生成候选。
    }
    let clicked = if is_clipmap {
        let target = super::geometry::clipmap_cursor_target(
            &ui,
            [
                building_registry
                    .as_deref()
                    .map_or(0, |registry| registry.revision),
                courtyard_registry
                    .as_deref()
                    .map_or(0, |registry| registry.revision),
            ],
        );
        clipmap
            .as_deref()
            .and_then(|surface| surface.snapshot.as_ref())
            .zip(ground_cursor)
            .zip(target)
            .and_then(|((snapshot, mut ground), target)| {
                match ground.take_author_click_result(
                    snapshot,
                    &snapshot.scene_id,
                    target,
                    session.revision,
                ) {
                    Some(feathrise_scene::ClipmapGroundHit::Hit(point)) => Some(point),
                    _ => None,
                }
            })
    } else {
        editor.cursor_world
    };
    if is_clipmap && clicked.is_none() {
        return;
    }
    let result = (|| -> Result<(), String> {
        let point = clicked.ok_or_else(|| "院落落点没有命中当前地形".to_owned())?;
        let courtyard_registry = courtyard_registry
            .as_deref()
            .ok_or_else(|| "院落目录尚未就绪".to_owned())?;
        let building_registry = building_registry
            .as_deref()
            .ok_or_else(|| "建筑预设目录尚未就绪".to_owned())?;
        if courtyard_registry.revision == 0 {
            return Err("院落目录尚未形成有效快照".to_owned()); // 坏热更保留的旧完整快照仍可继续用于已存在模板。
        }
        let (source, candidates, creates_new, description) = match mode {
            CourtyardViewportMode::Stamp => {
                let id = ui
                    .selected_preset_id
                    .as_deref()
                    .ok_or_else(|| "请先选择院落模板".to_owned())?;
                let preset = courtyard_registry
                    .resolve(id)
                    .ok_or_else(|| "所选院落模板已失效".to_owned())?;
                let (instance_id, placement_ids) =
                    next_group_ids(&mut state, &session, preset.members.len())?;
                let records = instantiate_preset(
                    preset,
                    &placement_ids,
                    &instance_id,
                    point,
                    (ui.quarter_turns * 90) as f32,
                )?;
                (
                    Vec::new(),
                    records,
                    true,
                    format!("已盖印院落“{}”", preset.label),
                )
            }
            CourtyardViewportMode::MoveGroup | CourtyardViewportMode::CopyGroup => {
                let source = selected_group(&state, &session, &ui.selected_placement_ids)?;
                let copy = mode == CourtyardViewportMode::CopyGroup;
                let (instance_id, placement_ids) = if copy {
                    let (id, ids) = next_group_ids(&mut state, &session, source.len())?;
                    (Some(id), Some(ids))
                } else {
                    (None, None)
                };
                let yaw = source[0]
                    .1
                    .courtyard
                    .as_ref()
                    .expect("选组已校验归属")
                    .yaw_degrees;
                let records = source
                    .iter()
                    .enumerate()
                    .map(|(ordinal, (_, record))| {
                        transform_existing_member(
                            record,
                            instance_id.as_deref(),
                            point,
                            yaw,
                            placement_ids.as_ref().map(|ids| ids[ordinal].as_str()),
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (
                    source,
                    records,
                    copy,
                    if copy {
                        "已复制院落"
                    } else {
                        "已移动院落"
                    }
                    .to_owned(),
                )
            }
            _ => unreachable!("入口已限定模式"),
        };
        submit_operation(
            &session,
            courtyard_registry,
            building_registry,
            &building_state,
            &mut state,
            source,
            candidates,
            creates_new,
            description,
            Some(mode),
            clipmap.as_deref(),
            super::geometry::clipmap_cursor_target(
                &ui,
                [building_registry.revision, courtyard_registry.revision],
            ),
        )
    })();
    let message = match result {
        Ok(()) => "正在后台校验整个院落的地基、占地和门前空间…".to_owned(),
        Err(error) => format!("院落操作未开始：{error}"),
    };
    ui.status = message.clone();
    editor.status = message;
}

/// 左栏整组旋转使用同一后台规则，不先修改任一建筑的可见角度。
#[allow(clippy::too_many_arguments)]
pub(super) fn request_rotation(
    mut requests: MessageReader<CourtyardUiRequest>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    courtyard_registry: Option<Res<CourtyardPresetRegistry>>,
    building_registry: Option<Res<BuildingPresetRegistry>>,
    building_state: Res<BuildingAuthoringState>,
    clipmap: Option<Res<DynamicClipmapGroundSurface>>,
    mut ui: ResMut<CourtyardUiState>,
    mut state: ResMut<CourtyardAuthoringState>,
) {
    for request in requests.read() {
        let CourtyardUiRequest::RotateSelection {
            placement_ids,
            quarter_turns,
        } = request
        else {
            continue;
        };
        let result = (|| -> Result<(), String> {
            if !editor.open || session.saving {
                return Err("场景正在保存或编辑器未打开".to_owned());
            }
            if *quarter_turns == 0 || quarter_turns.unsigned_abs() > 4 {
                return Err("整体旋转只接受四个九十度档位".to_owned());
            }
            let courtyard_registry = courtyard_registry
                .as_deref()
                .ok_or_else(|| "院落目录尚未就绪".to_owned())?;
            let building_registry = building_registry
                .as_deref()
                .ok_or_else(|| "建筑目录尚未就绪".to_owned())?;
            let source = selected_group(&state, &session, placement_ids)?;
            let group = source[0].1.courtyard.as_ref().expect("选组已校验归属");
            let origin = Vec3::from_array(group.origin);
            let yaw = (group.yaw_degrees + (*quarter_turns * 90) as f32).rem_euclid(360.0);
            let candidates = source
                .iter()
                .map(|(_, record)| transform_existing_member(record, None, origin, yaw, None))
                .collect::<Result<Vec<_>, _>>()?;
            submit_operation(
                &session,
                courtyard_registry,
                building_registry,
                &building_state,
                &mut state,
                source,
                candidates,
                false,
                "已旋转整个院落".to_owned(),
                None,
                clipmap.as_deref(),
                None,
            )
        })();
        let message = match result {
            Ok(()) => "正在后台校验旋转后的整个院落…".to_owned(),
            Err(error) => format!("院落旋转未开始：{error}"),
        };
        ui.status = message.clone();
        editor.status = message;
    }
}

/// 通过分帧完成的索引取得完整同组记录；选择任意一个成员即可操作全组。
fn selected_group(
    state: &CourtyardAuthoringState,
    session: &EditorAuthoringSession,
    selected_ids: &[String],
) -> Result<Vec<(usize, ScenePlacementRecord)>, String> {
    if selected_ids.len() > MAX_COURTYARD_MEMBERS_PER_PRESET {
        return Err("当前选集超过院落成员上限".to_owned()); // 即使内部消息异常也不在帧关键路径遍历无界选集。
    }
    if !state.index.ready_for(session) {
        return Err("院落成员索引正在分帧准备".to_owned());
    }
    let selected_id = selected_ids
        .first()
        .ok_or_else(|| "请先选择院落中的一个成员".to_owned())?;
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "当前场景草稿不存在".to_owned())?;
    let index = *state
        .index
        .by_placement_id
        .get(selected_id)
        .ok_or_else(|| "所选成员已不存在".to_owned())?;
    let record = draft
        .placements
        .placements
        .get(index)
        .filter(|record| &record.id == selected_id)
        .ok_or_else(|| "所选成员的索引已经失效".to_owned())?;
    let group = record
        .courtyard
        .as_ref()
        .ok_or_else(|| "所选成员尚未编入院落".to_owned())?;
    for selected_id in selected_ids.iter().skip(1) {
        let selected_index = *state
            .index
            .by_placement_id
            .get(selected_id)
            .ok_or_else(|| format!("选中成员{selected_id}已不存在"))?;
        let selected_group = draft
            .placements
            .placements
            .get(selected_index)
            .and_then(|record| record.courtyard.as_ref())
            .ok_or_else(|| format!("选中成员{selected_id}不属于院落"))?;
        if selected_group.instance_id != group.instance_id {
            return Err("当前选中了多个不同院落，请只保留目标副本的成员".to_owned());
        }
    }
    let members = state
        .index
        .by_instance_id
        .get(&group.instance_id)
        .ok_or_else(|| "院落实例没有可用成员索引".to_owned())?;
    if members.is_empty() || members.len() > MAX_COURTYARD_MEMBERS_PER_PRESET {
        return Err("院落成员数量超过整组操作上限".to_owned());
    }
    let mut source = Vec::with_capacity(members.len());
    for &index in members {
        let record = draft
            .placements
            .placements
            .get(index)
            .ok_or_else(|| "院落成员索引越界".to_owned())?;
        if !record.editor.selectable() { return Err(format!("院落成员{}已隐藏或锁定，请先在场景树解除保护", record.id)); }
        let current = record
            .courtyard
            .as_ref()
            .ok_or_else(|| "院落成员归属已失效".to_owned())?;
        if current.instance_id != group.instance_id
            || current.preset_id != group.preset_id
            || current.origin != group.origin
            || current.yaw_degrees != group.yaw_degrees
        {
            return Err("院落成员的整体枢轴不一致，请先修复场景数据".to_owned());
        }
        if !record.water_surfaces.is_empty() {
            return Err(format!(
                "院落成员{}带有整网格水语义，暂不支持整体操作",
                record.id
            )); // 克隆前拒绝大节点列表。
        }
        source.push((index, record.clone()));
    }
    Ok(source)
}

/// 新副本使用跨会话唯一身份；场景内碰撞仍由完整 ID 索引复核。
fn next_group_ids(
    state: &mut CourtyardAuthoringState,
    session: &EditorAuthoringSession,
    count: usize,
) -> Result<(String, Vec<String>), String> {
    if !state.index.ready_for(session) {
        return Err("成员索引正在分帧准备".to_owned());
    }
    state.ticket = state.ticket.wrapping_add(1).max(1);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("生成副本身份失败：{error}"))?
        .as_millis();
    let base = format!("courtyard-{millis}-{}", state.ticket);
    let instance_id = format!("{base}-instance");
    if state.index.by_instance_id.contains_key(&instance_id) {
        return Err("新院落实例 ID 与现有场景冲突，请重试".to_owned());
    }
    let ids = (0..count)
        .map(|index| format!("{base}-placement-{}", index + 1))
        .collect::<Vec<_>>();
    if ids
        .iter()
        .any(|id| state.index.by_placement_id.contains_key(id))
    {
        return Err("新摆放 ID 与现有场景冲突，请重试".to_owned());
    }
    Ok((instance_id, ids))
}

/// 完整候选只在当前目录与作者结构仍一致时进入唯一 CPU 校验槽。
#[allow(clippy::too_many_arguments)]
fn submit_operation(
    session: &EditorAuthoringSession,
    courtyard_registry: &CourtyardPresetRegistry,
    building_registry: &BuildingPresetRegistry,
    building_state: &BuildingAuthoringState,
    state: &mut CourtyardAuthoringState,
    source: Vec<(usize, ScenePlacementRecord)>,
    candidates: Vec<ScenePlacementRecord>,
    creates_new: bool,
    description: String,
    required_mode: Option<CourtyardViewportMode>,
    clipmap: Option<&DynamicClipmapGroundSurface>,
    input_target: Option<crate::viewport::vegetation_ground::GroundCursorTarget>,
) -> Result<(), String> {
    if state.operation_pending() || state.catalog_task.is_some() || state.visual_sync_pending() {
        return Err("上一项院落任务尚未完成".to_owned()); // 不堆积鼠标点击任务。
    }
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "场景草稿尚未就绪".to_owned())?;
    if candidates.is_empty() || candidates.len() > MAX_COURTYARD_MEMBERS_PER_PRESET {
        return Err("院落成员数量超出单次印章上限".to_owned());
    }
    if creates_new
        && draft
            .placements
            .placements
            .len()
            .saturating_add(candidates.len())
            > MAX_SCENE_PLACEMENTS_PER_SCENE
    {
        return Err(format!(
            "院落落印将超过场景 {MAX_SCENE_PLACEMENTS_PER_SCENE} 个摆放上限"
        ));
    }
    let mut validator =
        courtyard_validator_snapshot(session, building_registry, building_state, clipmap)?;
    if let Some(origin) = candidates
        .first()
        .and_then(|r| r.courtyard.as_ref())
        .map(|g| Vec3::from_array(g.origin))
    {
        validator.sampler = validator
            .sampler
            .take()
            .map(|s| s.for_selected_surface(origin));
    }
    let ground_content = validator
        .sampler
        .as_ref()
        .and_then(crate::authoring::PlacementTerrainSampler::clipmap_content_revision);
    let excluded_ids = if creates_new {
        BTreeSet::new()
    } else {
        source.iter().map(|(_, record)| record.id.clone()).collect()
    }; // 移动/旋转可穿过旧本组占地。
    let mut resolved = Vec::<(ScenePlacementRecord, Option<BuildingPresetDescriptor>)>::with_capacity(
        candidates.len(),
    );
    for mut record in candidates {
        if !record.water_surfaces.is_empty() {
            return Err(format!(
                "院落成员{}带有整网格水语义，暂不支持整组操作",
                record.id
            )); // 水绑定须走独立证据与预算链。
        }
        let preset = match record.building_preset_id.as_deref() {
            Some(id) => {
                let preset = building_registry
                    .resolve(id)
                    .cloned()
                    .ok_or_else(|| format!("院落成员{}引用的建筑预设{id}不存在", record.id))?;
                record.building_preset_id = Some(preset.id.clone()); // 历史别名在新整组操作边界迁移到规范 ID。
                Some(preset)
            }
            None => None,
        };
        resolved.push((record, preset));
    }
    state.ticket = state.ticket.wrapping_add(1).max(1);
    let identity = OperationIdentity {
        scene_id: draft.scene_id.clone(),
        author_revision: session.revision,
        terrain_revision: session.terrain_revision,
        ground_content,
        structure_revision: session.placement_structure_revision,
        building_revision: building_registry.revision,
        courtyard_revision: courtyard_registry.revision,
        ticket: state.ticket,
    };
    let task_identity = identity.clone();
    state.operation_task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let work = CourtyardGroundWork {
            validator,
            candidates: resolved,
            excluded_ids,
            query: None,
        };
        run_ground_operation(
            OperationResult {
                identity: task_identity,
                source,
                creates_new,
                result: Err("正在等待院落精确地基".into()),
                description,
                required_mode,
                ground_work: None,
                input_target,
            },
            work,
        )
    }));
    Ok(())
}

/// 同一整组CPU槽分期取得有限样本，全部点就绪后继续原地基、占地和门区规则。
fn run_ground_operation(
    mut result: OperationResult,
    mut work: CourtyardGroundWork,
) -> OperationResult {
    match work
        .validator
        .prepare_ground(&work.candidates, &mut work.query)
    {
        Ok(true) => {
            result.result =
                validate_courtyard_batch(&work.validator, work.candidates, &work.excluded_ids);
        }
        Ok(false) => {
            result.ground_work = Some(work);
        }
        Err(error) => {
            result.result = Err(error);
        }
    }
    result
}

/// 完整校验结果再次复核作者、目录、选取与票据，随后只压入一条场景历史命令。
#[allow(clippy::too_many_arguments)]
pub(super) fn poll_operation(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    courtyard_registry: Option<Res<CourtyardPresetRegistry>>,
    building_registry: Option<Res<BuildingPresetRegistry>>,
    clipmap: Option<Res<DynamicClipmapGroundSurface>>,
    mut browser: ResMut<EditorGltfBrowserState>,
    mut ui: ResMut<CourtyardUiState>,
    mut state: ResMut<CourtyardAuthoringState>,
) {
    if let Some(task) = state.operation_task.as_mut() {
        let Some(result) = check_ready(task) else {
            return;
        };
        state.operation_task = None;
        state.operation_waiting = Some(result);
    }
    let Some(mut result) = state.operation_waiting.take() else {
        return;
    };
    let valid = session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.scene_id == result.identity.scene_id)
        && editor.selected_scene_id.as_ref() == Some(&result.identity.scene_id)
        && session.revision == result.identity.author_revision
        && session.terrain_revision == result.identity.terrain_revision
        && session.placement_structure_revision == result.identity.structure_revision
        && building_registry
            .as_deref()
            .is_some_and(|registry| registry.revision == result.identity.building_revision)
        && courtyard_registry
            .as_deref()
            .is_some_and(|registry| registry.revision == result.identity.courtyard_revision)
        && state.ticket == result.identity.ticket
        && result.required_mode.is_none_or(|mode| ui.mode == mode)
        && result.input_target.as_ref().is_none_or(|target| {
            super::geometry::clipmap_cursor_target(
                &ui,
                [
                    result.identity.building_revision,
                    result.identity.courtyard_revision,
                ],
            )
            .as_ref()
                == Some(target)
        })
        && editor.open
        && !editor.close_requested
        && !editor.new_scene_dialog_open
        && !session.saving;
    if !valid {
        ui.status = "院落校验结果已过期，请按当前场景和选择重新操作".to_owned();
        editor.status = ui.status.clone();
        return;
    }
    let draft = session.draft.as_ref().unwrap();
    match crate::authoring::buildings::content_is_current(
        result.identity.ground_content,
        draft,
        clipmap.as_deref(),
        session.terrain_revision,
    ) {
        Ok(false) => {
            state.operation_waiting = Some(result);
            return;
        }
        Err(error) => {
            ui.status = error;
            editor.status = ui.status.clone();
            return;
        }
        Ok(true) => {}
    }
    if let Some(work) = result.ground_work.as_mut() {
        let Ok(sampler) = crate::authoring::PlacementTerrainSampler::from_sources(
            draft,
            clipmap.as_deref(),
            session.terrain_revision,
        ) else {
            state.operation_waiting = Some(result);
            return;
        };
        match work.query.as_ref().unwrap().can_retry(&sampler) {
            Ok(false) => {
                state.operation_waiting = Some(result);
                return;
            }
            Err(error) => {
                ui.status = error;
                editor.status = ui.status.clone();
                return;
            }
            Ok(true) => {}
        }
        work.validator.sampler = Some(sampler);
        let work = result.ground_work.take().unwrap();
        state.operation_task = Some(
            AsyncComputeTaskPool::get().spawn(async move { run_ground_operation(result, work) }),
        );
        return;
    }
    let records = match result.result {
        Ok(records) => records,
        Err(error) => {
            ui.status = format!("院落位置无效：{error}");
            editor.status = ui.status.clone();
            return;
        }
    };
    let apply = apply_complete_batch(
        &mut editor,
        &mut session,
        &mut browser,
        &mut ui,
        &mut state,
        &result.source,
        records,
        result.creates_new,
    );
    let message = match apply {
        Ok(()) => format!("{}；可整组撤销", result.description),
        Err(error) => format!("院落采用失败：{error}"),
    };
    ui.status = message.clone();
    editor.status = message;
}

/// 先预检所有目标，再同时写入场景草稿、世界预览和一条可撤销命令。
#[allow(clippy::too_many_arguments)]
fn apply_complete_batch(
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    browser: &mut EditorGltfBrowserState,
    ui: &mut CourtyardUiState,
    state: &mut CourtyardAuthoringState,
    source: &[(usize, ScenePlacementRecord)],
    records: Vec<ScenePlacementRecord>,
    creates_new: bool,
) -> Result<(), String> {
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "场景草稿已经卸载".to_owned())?;
    let bounds = draft
        .manifest
        .world_bounds()
        .ok_or_else(|| "场景范围无效".to_owned())?;
    if creates_new
        && draft
            .placements
            .placements
            .len()
            .saturating_add(records.len())
            > MAX_SCENE_PLACEMENTS_PER_SCENE
    {
        return Err("在后台校验期间场景摆放数量已满".to_owned());
    }
    if !creates_new && source.len() != records.len() {
        return Err("整组原成员与结果数量不一致".to_owned());
    }
    let mut new_ids = BTreeSet::new(); // 最多 256 条，在当前帧只做候选内部去重。
    for (ordinal, record) in records.iter().enumerate() {
        record.validated_transform(&draft.scene_id, bounds)?; // 画面同步前完整复核正式变换。
        if !creates_new {
            let (index, before) = &source[ordinal];
            if draft.placements.placements.get(*index) != Some(before) || record.id != before.id {
                return Err(format!("院落原成员{}已改变", before.id)); // 任何一条失效都拒绝半组采用。
            }
        } else if !new_ids.insert(record.id.as_str()) {
            return Err(format!("新院落内部摆放 ID {} 重复", record.id));
        }
    }
    let scene_id = draft.scene_id.clone();
    let base_len = draft.placements.placements.len(); // 新成员始终追加，队列记录精确数组槽位。
    let changed = if creates_new {
        Vec::new()
    } else {
        source
            .iter()
            .zip(records.iter())
            .map(|((index, before), after)| (*index, before.clone(), after.clone()))
            .collect::<Vec<_>>()
    };
    let change = crate::authoring::CourtyardBatchChange::new(
        if creates_new {
            records.clone()
        } else {
            Vec::new()
        },
        changed,
    )?; // 所有可失败的历史预算与成员身份检查必须在写入作者草稿前完成。
    if creates_new {
        let draft = session.draft.as_mut().expect("上方已确认草稿存在");
        for record in &records {
            draft.placements.placements.push(record.clone());
        } // 所有成员一次进入权威作者数据。
    } else {
        let draft = session.draft.as_mut().expect("上方已确认草稿存在");
        for ((index, _before), after) in source.iter().zip(records.iter()) {
            draft.placements.placements[*index] = after.clone();
        }
    }
    let selected = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    if let Some(first) = selected.first() {
        browser.selected_placement_id = Some(first.clone());
    }
    ui.selected_placement_ids = selected;
    ui.scene_id = Some(scene_id);
    if ui.mode != CourtyardViewportMode::Stamp {
        ui.mode = CourtyardViewportMode::Idle;
    } // Stamp 保持可连续盖印。
    session.push_bounded_water_history(EditorCommand::CourtyardBatch(Box::new(change)));
    session.mark_placement_structure_changed(editor); // 组枢轴或成员表换代，场景树与索引下一帧重建。
    let updates = if creates_new {
        records
            .into_iter()
            .enumerate()
            .map(|(offset, record)| (base_len + offset, record, true))
            .collect()
    } else {
        source
            .iter()
            .zip(records)
            .map(|((index, _), record)| (*index, record, true))
            .collect()
    };
    queue_visual_sync(state, session, updates); // 每帧最多16个正式根或Scene0预览，避免整组单帧峰值。
    Ok(())
}
