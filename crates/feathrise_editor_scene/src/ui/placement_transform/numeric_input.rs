//! 七个资产变换字段的精确提交、左拖事务与右拖灵敏度持久化。

use bevy::ecs::system::SystemParam; // 资产句柄与Clipmap曲面收束为一个参数，避免越过Bevy系统元组上限。
use bevy::prelude::*; // Commands、消息、查询和资源系统参数。
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputChanged, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputScrubSensitivityChanged,
    NumericInputScrubSensitivityCommitted, NumericInputSetValue, NumericValue,
};
use feathrise_scene_content::ScenePlacementRecord; // 每次正式历史都保存完整摆放记录快照。
use feathrise_scene::{DynamicClipmapGroundSurface, ScenePlacementInstance}; // Clipmap跟随变换读取已采用三角面；预览同步只复核当前活动根身份。
use feathrise_editor_core::EditorSettings; // 三类变换右拖倍率写回统一自动保存设置。 // 公共控件发布字符、左拖、右拖及外部取消的唯一协议。

use super::actions::sync_placement_record_world_with_terrain; // 数值路径也保留三维补丁的真实源父。
use super::{
    PlacementTransformField, PlacementTransformFocus, PlacementTransformInputState,
    PlacementTransformPatchSource, PlacementTransformScrub, PlacementTransformTarget, selected_target,
}; // 字段语义和稳定作者事务身份。
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, PlacementTerrainSampler, reconcile_record_with_terrain, update_follow_offset_from_world_y,
}; // 完整记录变化进入统一历史，并共享跟随型资产的内存高度语义。
use crate::resources::SceneEditorState; // F12生命周期、dirty和用户可见状态。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 当前完整实例选择与结构索引。
use crate::ui::placement_water::PlacementInstanceIndex; // 连续拖动不扫描最多8192个根。

type PlacementRoots<'w, 's> = Query<'w, 's, (Entity, &'static ScenePlacementInstance)>; // 根索引只返回每ID少量代次。

/// 数值变换共同使用的只读运行时资源；分组只为保持系统参数数量有界。
#[derive(SystemParam)]
pub(crate) struct PlacementTransformRuntime<'w, 's> {
    asset_server: Res<'w, AssetServer>, // 缺根时补作者Scene0预览。
    clipmap_surface: Option<Res<'w, DynamicClipmapGroundSurface>>, // 跟随型Clipmap资产读取正式三角面。
    mesh_edit: Option<Res<'w, crate::authoring::mesh_edit::MeshEditState>>,
    terrain_queries: feathrise_scene::TerrainObjectQueries<'w, 's>,
    source_instances: Query<'w, 's, &'static feathrise_scene::SubsceneInstance>, // 松手也核对同Entity内的批次/代次变化。
}

/// 成功 Changed 只能属于一个真实已采用的源；相同实体地址不能替代批次、代次和完整框架身份。
fn patch_scrub_source(
    draft: &crate::authoring::EditorSceneDraft,
    record: &ScenePlacementRecord,
    terrain_queries: &feathrise_scene::TerrainObjectQueries,
    source_instances: &Query<&feathrise_scene::SubsceneInstance>,
) -> Result<Option<PlacementTransformPatchSource>, String> {
    if record.terrain_patch.is_none() { return Ok(None); }
    let parent = crate::authoring::patch_preview_parent(draft, terrain_queries, record)?
        .map(|parent| source_instances.get(parent)
            .map(|instance| (parent, instance.batch, instance.generation))
            .map_err(|_| "三维地形拖动的源实例身份已失效".to_owned()))
        .transpose()?;
    Ok(Some(PlacementTransformPatchSource {
        parent,
        frame: crate::authoring::placement_pose::placement_record_frame(draft, record)?.as_transform().to_matrix(),
        bounds: crate::authoring::placement_record_bounds(draft, record)?,
    }))
}

/// 局部标量的作者责任不依赖父级继续存在；取消先撤回 owned 值，再尽力恢复尚存会话模型。
fn restore_patch_scrub_value(
    commands: &mut Commands, roots: &PlacementRoots<'_, '_>,
    editor: &mut SceneEditorState, session: &mut EditorAuthoringSession,
    terrain_queries: &feathrise_scene::TerrainObjectQueries,
    source_instances: &Query<&feathrise_scene::SubsceneInstance>,
    transaction: &PlacementTransformScrub, restore_dirty: bool,
) -> Result<bool, String> {
    let source = transaction.patch_source.as_ref().ok_or("三维地形拖动缺少冻结来源")?;
    let (after, transform, source_current, unchanged_outside_transaction) = {
        let draft = session.draft.as_ref().filter(|draft| draft.scene_id == transaction.target.scene_id)
            .ok_or("拖动恢复时场景草稿已经失效")?;
        let current = draft.placements.placements.get(transaction.target.record_index)
            .filter(|record| record.id == transaction.target.placement_id)
            .ok_or("拖动恢复时资产记录已经失效")?;
        if transaction.field.value(current) != transaction.latest_value
            || current.terrain_patch.as_ref().map(|patch| &patch.terrain_id)
                != transaction.before.terrain_patch.as_ref().map(|patch| &patch.terrain_id)
        {
            return Ok(false); // 后来作者值或来源已接管时，旧事务不能解释或覆盖它的局部数字。
        }
        let source_current = patch_scrub_source(draft, current, terrain_queries, source_instances)
            .is_ok_and(|current| current.as_ref() == Some(source));
        let mut after = current.clone();
        transaction.field.set_value(&mut after, transaction.field.value(&transaction.before));
        let transform = after.validated_transform(&draft.scene_id, source.bounds).ok();
        (after, transform, source_current, current == &transaction.latest_record)
    };
    let changed = session.draft.as_ref().unwrap().placements.placements[transaction.target.record_index] != after;
    session.draft.as_mut().unwrap().placements.placements[transaction.target.record_index] = after.clone();
    // 源失效不撤销作者责任：草稿先完成局部回滚；只操作已有会话根，绝不补生或复活正式批次。
    if let Some(root) = session.placement_previews.get(&after.id).copied()
        && roots.get(root).is_ok_and(|(_, instance)| instance.scene_id() == &transaction.target.scene_id
            && instance.id() == after.id && instance.is_active() && instance.is_terrain_patch())
    {
        if let Some(transform) = transform {
            commands.entity(root).insert((transform, ScenePlacementInstance::new(&transaction.target.scene_id, &after)));
        }
        if !source_current || transform.is_none() {
            commands.entity(root).insert(Visibility::Hidden).remove::<ChildOf>();
        } // 保留稳定来源marker，后续完整采用仍可转交；无效局部姿态不会裸露到世界根。
    }
    if changed { session.mark_changed(editor); }
    if restore_dirty && unchanged_outside_transaction { editor.dirty = transaction.dirty_before; }
    Ok(true)
}

/// 当前作者上下文是否允许开始或继续变换输入。
fn input_context_valid(editor: &SceneEditorState, session: &EditorAuthoringSession) -> bool {
    editor.open
        && !editor.new_scene_dialog_open
        && !session.saving
        && session
            .draft
            .as_ref()
            .is_some_and(|draft| editor.selected_scene_id.as_ref() == Some(&draft.scene_id)) // 保存、切换场景和模态弹框都不能接纳旧输入。
}

/// 冻结目标是否仍精确对应当前完整实例选择和作者数组槽。
fn target_is_current(
    target: &PlacementTransformTarget, // 聚焦或拖动开始时冻结的身份。
    browser: &EditorGltfBrowserState,  // 当前完整实例选择。
    session: &EditorAuthoringSession,  // 当前场景、结构修订和草稿数组。
) -> bool {
    if target.structure_revision != session.placement_structure_revision
        || browser.selected_placement_id.as_deref() != Some(target.placement_id.as_str())
    {
        return false; // 删除、复制、切换场景或另选实例都会使旧目标失效。
    }
    session.draft.as_ref().is_some_and(|draft| {
        draft.scene_id == target.scene_id
            && draft
                .placements
                .placements
                .get(target.record_index)
                .is_some_and(|record| record.id == target.placement_id)
    }) // O(1)数组槽仍必须复核稳定ID。
}

/// 读取当前目标记录的字段值；同步焦点时不复制完整记录。
fn target_value(
    target: &PlacementTransformTarget, // 已经复核的稳定目标。
    field: PlacementTransformField,    // 当前数字根字段。
    session: &EditorAuthoringSession,  // 当前权威草稿。
) -> Option<f32> {
    session
        .draft
        .as_ref()
        .filter(|draft| draft.scene_id == target.scene_id)
        .and_then(|draft| draft.placements.placements.get(target.record_index))
        .filter(|record| record.id == target.placement_id)
        .map(|record| field.value(record)) // 只借用一个标量。
}

/// 使用当前场景清单完整验证一条候选记录。
fn validate_record(
    record: &ScenePlacementRecord,       // 已修改的候选完整记录。
    target: &PlacementTransformTarget, // 诊断和场景身份。
    session: &EditorAuthoringSession,  // 当前清单世界范围。
    field: PlacementTransformField, // 建筑仅允许独立LOD参数，不放开占地与地基约束的姿态字段。
) -> Result<Rect, String> {
    if record.building_preset_id.is_some() && !field.is_lod() {
        return Err("建筑变换必须使用“吸附地面”或“建筑旋转90°”并通过后台校验".to_owned()); // 七个普通字段不能绕过占地、门口和五点地基规则。
    }
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| draft.scene_id == target.scene_id)
        .ok_or_else(|| "资产变换所属场景草稿已经失效".to_owned())?;
    let bounds = crate::authoring::placement_record_bounds(draft, record)?;
    record.validated_transform(&target.scene_id, bounds)?; // 与保存和正式运行时共享同一纯校验。
    Ok(bounds) // ECS预览同步继续复用刚验证的范围。
}

/// 写入一个变换字段，并同步维护跟随模式的离地偏移或派生世界Y。
fn set_field_value_with_height_mode(
    field: PlacementTransformField,            // 当前业务字段。
    record: &mut ScenePlacementRecord,           // 待修改完整记录。
    value: f32,                                // 共享 UI 控件已经规范化的目标值。
    sampler: Option<&PlacementTerrainSampler>, // 跟随模式的位置字段必须读取当前地形高度。
) -> Result<bool, String> {
    let before = record.clone(); // 小型完整记录保证任一采样失败都能原子恢复。
    if field.is_lod() && !field.supported_by(record) {
        return Err("当前LOD策略不是自定义，已拒绝隐藏字段的旧输入".into());
    }
    if !field.set_value(record, value) {
        return Ok(false); // 同一吸附档不更新偏移或世界根。
    }
    if field.is_cuboid() || field.is_lod() {
        return Ok(true); // 形状、材质与LOD不改变原点，不采样高度或改写地形绑定。
    }
    let result = if record.height_mode.follows_terrain() {
        match sampler {
            Some(sampler) => match field {
                PlacementTransformField::TranslationX | PlacementTransformField::TranslationZ => {
                    reconcile_record_with_terrain(record, sampler).map(|_| ()) // 移动XZ时保持原离地偏移并刷新世界Y。
                }
                PlacementTransformField::TranslationY => {
                    update_follow_offset_from_world_y(record, sampler) // 编辑世界Y时把差值转换为新的离地偏移。
                }
                PlacementTransformField::Yaw
                | PlacementTransformField::ScaleX
                | PlacementTransformField::ScaleY
                | PlacementTransformField::ScaleZ => Ok(()), // 姿态不改变实例原点的地表采样位置。
                _ => Err("洞口字段只能用于固定高度的三维地形".into()),
            },
            None => Err("当前场景没有可用地形高度".to_owned()), // 跟随记录不得退回基准平面。
        }
    } else {
        Ok(()) // 固定高度只写用户明确选择的标量。
    };
    if let Err(error) = result {
        *record = before; // 采样或偏移失败时完整恢复，不能只恢复X而遗留错误Y。
        return Err(error);
    }
    if let Some(sampler)=sampler && let Err(error)=sampler.refresh_record_binding(record) {*record=before;return Err(error);}
    Ok(true)
}

/// 精确字符提交按冻结身份修改一个字段并形成一条完整记录历史。
#[allow(clippy::too_many_arguments)]
fn apply_exact_value(
    commands: &mut Commands,                               // 同帧更新可见实例根。
    asset_server: &AssetServer,                            // 缺根时生成作者预览。
    roots: &PlacementRoots<'_, '_>,                        // 定向根身份复核。
    root_index: &PlacementInstanceIndex,                   // O(1)稳定ID索引。
    editor: &mut SceneEditorState,                           // dirty和状态栏。
    session: &mut EditorAuthoringSession,                  // 草稿、预览与历史。
    clipmap_surface: Option<&DynamicClipmapGroundSurface>, // Clipmap只共享当前正式曲面快照。
    terrain_queries: &feathrise_scene::TerrainObjectQueries,
    target: &PlacementTransformTarget,                     // 聚焦时冻结目标。
    field: PlacementTransformField,                        // 当前提交字段。
    value: f32,                                            // 共享 UI 控件已经解析并规范化的浮点值。
) -> Result<Option<ScenePlacementRecord>, String> {
    if target.authoring_revision != session.revision {
        return Err(format!(
            "{}输入基于作者修订{}，当前已是{}；过期提交已丢弃",
            field.label(),
            target.authoring_revision,
            session.revision,
        )); // 字符输入期间的任何外部作者变化都优先于旧缓冲。
    }
    let before = session
        .draft
        .as_ref()
        .and_then(|draft| draft.placements.placements.get(target.record_index))
        .filter(|record| record.id == target.placement_id)
        .cloned()
        .ok_or_else(|| "精确输入绑定的资产记录已经失效".to_owned())?; // 边沿克隆唯一完整起点。
    if before.building_preset_id.is_some() && !field.is_lod() {
        return Err("建筑数值变换已禁用；请使用受检移动或90°旋转按钮".to_owned());
    }
    let mut after = before.clone(); // 候选先在局部值中修改，失败不污染草稿。
    let sampler = if !field.is_cuboid() && !field.is_lod() && (after.height_mode.follows_terrain() || after.terrain_binding.is_some()) {
        let draft = session
            .draft
            .as_ref()
            .filter(|draft| draft.scene_id == target.scene_id)
            .ok_or_else(|| "资产变换所属场景草稿已经失效".to_owned())?;
        Some(PlacementTerrainSampler::from_sources(
            draft,
            clipmap_surface,
            session.terrain_revision,
        )?) // 精确输入使用提交瞬间当前地形修订。
    } else {
        None // 固定高度不需要共享大高度图句柄。
    };
    if !set_field_value_with_height_mode(field, &mut after, value, sampler.as_ref())? {
        return Ok(None); // 显式重输同值不制造历史或dirty。
    }
    let bounds = validate_record(&after, target, session, field)?; // 公共完整变换校验先于任何作者写入。
    {
        let record = session
            .draft
            .as_mut()
            .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
            .filter(|record| record.id == target.placement_id)
            .ok_or_else(|| "精确输入写入前资产记录已经失效".to_owned())?;
        *record = after.clone(); // 草稿与随后可见根来自同一完整候选快照。
    }
    if let Err(error) = sync_placement_record_world_with_terrain(
        commands,     // 更新Transform或补作者预览。
        asset_server, // Scene0来源沿用完整记录。
        session,      // 维护预览所有权。
        root_index,   // 定向正式根。
        roots,        // 复核活动门。
        &after,       // 当前完整草稿记录。
        bounds,       // 刚通过验证的场景范围。
        true,         // 变换结果保持可见。
        true,         // 根尚未生成时补预览。
        terrain_queries,
    ) {
        if let Some(record) = session
            .draft
            .as_mut()
            .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
            .filter(|record| record.id == before.id)
        {
            *record = before; // ECS同步失败时恢复完整作者起点。
        }
        return Err(error);
    }
    session.push_bounded_water_history(EditorCommand::PlacementChanged {
        record_index: target.record_index, // 冻结O(1)记录槽。
        before,                            // 输入前完整快照。
        after: after.clone(),              // 输入后完整受检快照。
    }); // 纯变换不会计入网格水预算，但共享同一分支管理入口。
    session.mark_changed(editor); // placements文档进入统一保存门。
    Ok(Some(after)) // 状态栏使用最终规范化值回显。
}

/// 异常或拖回起点时，仅恢复仍由当前左拖拥有的那个标量。
#[allow(clippy::too_many_arguments)]
fn restore_scrub_value(
    commands: &mut Commands,              // 恢复后重施可见根Transform。
    asset_server: &AssetServer,           // 缺根时补作者预览。
    roots: &PlacementRoots<'_, '_>,       // 当前活动根复核。
    root_index: &PlacementInstanceIndex,  // 稳定ID索引。
    editor: &mut SceneEditorState,          // 连续无净变化时恢复dirty门。
    session: &mut EditorAuthoringSession, // 当前草稿可能还含外部其他字段变化。
    clipmap_surface: Option<&DynamicClipmapGroundSurface>, // 恢复跟随位置时复用当前正式Clipmap曲面。
    terrain_queries: &feathrise_scene::TerrainObjectQueries,
    source_instances: &Query<&feathrise_scene::SubsceneInstance>,
    transaction: &PlacementTransformScrub,                 // 完整起点和最后owned标量。
    restore_dirty: bool, // 正常拖回起点可恢复旧dirty，异常冲突保持外部dirty。
) -> Result<bool, String> {
    if transaction.patch_source.is_some() {
        return restore_patch_scrub_value(commands, roots, editor, session, terrain_queries,
            source_instances, transaction, restore_dirty);
    }
    let before_value = transaction.field.value(&transaction.before); // 只取得本事务拥有字段的原始精度值。
    let terrain_revision = session.terrain_revision; // 可变借用草稿前冻结当前作者高度身份。
    let (after, bounds) = {
        let draft = session
            .draft
            .as_mut()
            .filter(|draft| draft.scene_id == transaction.target.scene_id)
            .ok_or_else(|| "拖动恢复时场景草稿已经失效".to_owned())?;
        let current = draft.placements.placements.get(transaction.target.record_index)
            .filter(|record| record.id == transaction.target.placement_id)
            .ok_or("拖动恢复时资产记录已经失效")?;
        let bounds = crate::authoring::placement_record_bounds(draft, current)?;
        let follows_terrain = !transaction.field.is_cuboid() && !transaction.field.is_lod() && draft
            .placements
            .placements
            .get(transaction.target.record_index)
            .filter(|record| record.id == transaction.target.placement_id)
            .is_some_and(|record| record.height_mode.follows_terrain() || record.terrain_binding.is_some()); // 同时冻结关联姿态输入。
        let sampler = follows_terrain
            .then(|| {
                PlacementTerrainSampler::from_sources(draft, clipmap_surface, terrain_revision)
            })
            .transpose()?; // 固定高度恢复不因旧场景缺内存高度场而失败。
        let mut record = draft
            .placements
            .placements
            .get(transaction.target.record_index)
            .filter(|record| record.id == transaction.target.placement_id)
            .cloned()
            .ok_or_else(|| "拖动恢复时资产记录已经失效".to_owned())?;
        if transaction.field.value(&record) != transaction.latest_value {
            return Ok(false); // 同字段已被后来作者值接管时绝不覆盖外部结果。
        }
        if !set_field_value_with_height_mode(
            transaction.field,
            &mut record,
            before_value,
            sampler.as_ref(),
        )? {
            if restore_dirty {
                editor.dirty = transaction.dirty_before; // 本来就等于raw起点时恢复进入前保存门。
            }
            return Ok(true); // 当前字段已经位于起点，无需写ECS。
        }
        record.validated_transform(&draft.scene_id, bounds)?; // 恢复后的完整外部记录仍须合法。
        (record, bounds) // 恢复候选先验证，父级或ECS同步失败不能提前覆盖草稿。
    };
    sync_placement_record_world_with_terrain(
        commands,     // 重施恢复后的完整Transform。
        asset_server, // 缺根时补预览。
        session,      // 维护预览所有权。
        root_index,   // 定向正式根。
        roots,        // 复核活动门。
        &after,       // 当前完整权威记录。
        bounds,       // 当前场景范围。
        true,         // 恢复后保持可见。
        true,         // 根缺失时补预览。
        terrain_queries,
    )?;
    session.draft.as_mut().expect("恢复候选已复核场景")
        .placements.placements[transaction.target.record_index] = after;
    // 回滚同样改变权威草稿：即使不增加撤销且恢复原dirty，也必须使临时值的后台快照失效。
    session.mark_changed(editor);
    if restore_dirty {
        editor.dirty = transaction.dirty_before;
    }
    Ok(true) // 当前事务owned标量已经安全撤销。
}

/// 消费七个字段的公共数值消息；一次左拖无论多少Changed只在松手压一条历史。
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_placement_transform_numeric_input(
    mut commands: Commands, // 精确提交和每个拖动吸附档即时更新可见根。
    mut changed: MessageReader<NumericInputChanged>, // 左拖实际值变化。
    mut committed: MessageReader<NumericInputCommitted>, // Text提交和Scrub松手统一入口。
    mut sensitivity_changed: MessageReader<NumericInputScrubSensitivityChanged>, // 右拖途中状态回显。
    mut sensitivity_committed: MessageReader<NumericInputScrubSensitivityCommitted>, // 右拖松手持久化。
    active: Res<ActiveNumericInput>, // 中键字符焦点用于冻结稳定目标。
    inputs: Query<(Entity, &PlacementTransformField)>, // 七个业务根实体到字段映射。
    runtime: PlacementTransformRuntime, // Scene预览与Clipmap地面共享只读参数组。
    browser: Res<EditorGltfBrowserState>, // 当前完整实例选择。
    root_index: Res<PlacementInstanceIndex>, // 连续Changed按稳定ID定向根。
    roots: PlacementRoots,           // 根索引结果复核活动门。
    mut state: ResMut<PlacementTransformInputState>, // 字符焦点、左拖和吸附模式共享状态。
    mut editor: ResMut<SceneEditorState>, // 生命周期、dirty和状态栏。
    mut settings: ResMut<EditorSettings>, // 右拖松手只写三类变换倍率之一。
    mut session: ResMut<EditorAuthoringSession>, // 草稿、预览和统一历史。
    mut set_values: MessageWriter<NumericInputSetValue>, // 目标失效时取消旧字符缓冲。
) {
    if state.rejected_scrub.is_some_and(|entity| inputs.get(entity).is_err()) {
        state.rejected_scrub = None; // 销毁的字段没有可续接的捕获，不让旧地址阻挡之后重建的控件。
    }
    let mesh_edit_active = runtime.mesh_edit.as_ref().is_some_and(|state| state.active());
    if state.scrub.is_some() && (mesh_edit_active || !input_context_valid(&editor, &session)) {
        let transaction = state.scrub.take().expect("上方已确认资产变换拖动存在");
        state.rejected_scrub = Some(transaction.entity); // 恢复上下文仍须等这笔真实松手，不能继续接收它的后续档。
        match restore_scrub_value(
            &mut commands,
            &runtime.asset_server,
            &roots,
            &root_index,
            &mut editor,
            &mut session,
            runtime.clipmap_surface.as_deref(),
            &runtime.terrain_queries,
            &runtime.source_instances,
            &transaction,
            false,
        ) {
            Ok(true) => editor.status = "资产变换拖动被保存或界面状态打断，实时值已恢复".to_owned(),
            Ok(false) => editor.status = "资产变换拖动目标已由后来作者状态接管".to_owned(),
            Err(error) => editor.status = format!("资产变换拖动中止恢复失败：{error}"),
        }
    }
    if mesh_edit_active {
        for event in committed.read() {
            if event.source == NumericInputCommitSource::Scrub && state.rejected_scrub == Some(event.entity) {
                state.rejected_scrub = None;
            } // 编辑模式也必须结清真实松手；清消息前释放拒收标记，退出后下一笔才能正常开始。
        }
        changed.clear(); sensitivity_changed.clear(); sensitivity_committed.clear();
        state.focus = None;
        return; // 编辑模式不接受隐藏或迟到的完整对象数值提交，局部顶点由自己的冻结事务维护。
    }

    for event in sensitivity_changed.read() {
        let Ok((_, field)) = inputs.get(event.entity) else {
            continue; // 其他数字框由各自业务域消费。
        };
        editor.status = format!("正在调整{}拖动灵敏度：×{:.3}", field.label(), event.value);
    }
    for event in sensitivity_committed.read() {
        let Ok((_, field)) = inputs.get(event.entity) else {
            continue; // 只处理七个资产变换根。
        };
        let sensitivity = if event.after.is_finite() {
            event.after.clamp(0.01, 100.0) // 与公共ScrubConfig标准范围一致。
        } else {
            1.0 // 异常消息回退标准手感，不能污染settings.json。
        };
        settings
            .scene_editor_scrub_sensitivities
            .set(field.scrub_field(), sensitivity); // 三轴同类字段共享一个持久槽。
        editor.status = format!("{}拖动灵敏度已设为 ×{sensitivity:.3}", field.label());
    }

    for event in changed.read() {
        if state.rejected_scrub == Some(event.entity) { continue; } // 只拒绝已经取消的这笔根，不影响其他独立字段。
        let Ok((_, field)) = inputs.get(event.entity) else {
            continue; // 水位、笔刷和网格水Changed由各自系统处理。
        };
        let Some(value) = event.value.as_f32() else {
            editor.status = "资产变换拖动产生了非浮点值".to_owned();
            continue;
        };
        if !input_context_valid(&editor, &session) {
            editor.status = "当前界面或保存状态不能拖动资产变换".to_owned();
            continue;
        }
        if state.scrub.is_none() {
            let Some(target) = selected_target(&browser, &session) else {
                editor.status = "当前没有可编辑的稳定资产选择".to_owned();
                continue;
            };
            let Some(before) = session
                .draft
                .as_ref()
                .and_then(|draft| draft.placements.placements.get(target.record_index))
                .filter(|record| record.id == target.placement_id)
                .cloned()
            else {
                editor.status = "资产拖动起点已经失效".to_owned();
                continue;
            };
            let patch_source = match patch_scrub_source(session.draft.as_ref().unwrap(), &before,
                &runtime.terrain_queries, &runtime.source_instances) {
                Ok(source) => source,
                Err(error) => { editor.status = format!("资产变换预览失败：{error}"); continue; }
            };
            state.scrub = Some(PlacementTransformScrub {
                entity: event.entity,                // 后续Changed和Committed必须来自同一根。
                field: *field,                       // 事务期间不能切轴或字段类别。
                target,                              // 冻结场景和O(1)数组槽。
                latest_value: field.value(&before),  // 首个实际变化前保存原始标量。
                latest_record: before.clone(),      // 取消只撤回本事务责任，保留任何外部字段变化。
                patch_source,
                before,                              // 唯一完整撤销起点。
                expected_revision: session.revision, // 只接受本事务连续推进。
                dirty_before: editor.dirty,          // 拖回起点恢复原保存门。
            });
        }
        let Some(transaction) = state.scrub.as_ref() else {
            continue; // 防御事务建立失败。
        };
        let latest_record_current = session.draft.as_ref()
            .filter(|draft| draft.scene_id == transaction.target.scene_id)
            .and_then(|draft| draft.placements.placements.get(transaction.target.record_index))
            == Some(&transaction.latest_record);
        if transaction.entity != event.entity
            || transaction.field != *field
            || transaction.expected_revision != session.revision
            || !target_is_current(&transaction.target, &browser, &session)
            || !latest_record_current // 每档都复核完整责任快照，不能把同修订外部改值纳入旧手势历史。
        {
            let transaction = state.scrub.take().expect("上方已确认资产变换拖动存在");
            state.rejected_scrub = Some(transaction.entity); // 同修订外部值或选择接管后，旧物理按住不能自动另起事务。
            match restore_scrub_value(
                &mut commands,
                &runtime.asset_server,
                &roots,
                &root_index,
                &mut editor,
                &mut session,
                runtime.clipmap_surface.as_deref(),
                &runtime.terrain_queries,
                &runtime.source_instances,
                &transaction,
                false,
            ) {
                Ok(true) => editor.status = "资产变换拖动身份已变化，实时值已恢复".to_owned(),
                Ok(false) => {
                    editor.status = "资产变换拖动身份已变化，后来作者值保持不变".to_owned()
                }
                Err(error) => editor.status = format!("资产变换拖动身份恢复失败：{error}"),
            }
            continue;
        }
        let target = transaction.target.clone(); // 结束状态资源借用后修改草稿和预览所有权。
        let terrain_revision = session.terrain_revision; // 可变借用草稿前冻结当前作者高度身份。
        let result = (|| -> Result<Option<(ScenePlacementRecord, Rect)>, String> {
            let draft = session
                .draft
                .as_mut()
                .filter(|draft| draft.scene_id == target.scene_id)
                .ok_or_else(|| "资产拖动所属场景已经失效".to_owned())?;
            let current = draft.placements.placements.get(target.record_index)
                .filter(|record| record.id == target.placement_id)
                .ok_or("资产拖动记录已经失效")?;
            if patch_scrub_source(draft, current, &runtime.terrain_queries, &runtime.source_instances)?
                != transaction.patch_source
            {
                return Err("三维地形拖动的源父、批次或框架已换代".into());
            }
            let bounds = crate::authoring::placement_record_bounds(draft, current)?;
            if draft
                .placements
                .placements
                .get(target.record_index)
                .filter(|record| record.id == target.placement_id)
                .is_some_and(|record| record.building_preset_id.is_some()) && !field.is_lod()
            {
                return Err("建筑数值拖动已禁用；请使用受检移动或90°旋转按钮".to_owned());
            }
            let follows_terrain = !field.is_cuboid() && !field.is_lod() && draft
                .placements
                .placements
                .get(target.record_index)
                .filter(|record| record.id == target.placement_id)
                .is_some_and(|record| record.height_mode.follows_terrain() || record.terrain_binding.is_some()); // 同时冻结关联姿态输入。
            let sampler = follows_terrain
                .then(|| {
                    PlacementTerrainSampler::from_sources(
                        draft,
                        runtime.clipmap_surface.as_deref(),
                        terrain_revision,
                    )
                })
                .transpose()?; // 每个吸附档共享当前高度场Arc，不复制高度数据。
            let mut record = draft
                .placements
                .placements
                .get(target.record_index)
                .filter(|record| record.id == target.placement_id)
                .cloned()
                .ok_or_else(|| "资产拖动记录已经失效".to_owned())?;
            if !set_field_value_with_height_mode(*field, &mut record, value, sampler.as_ref())? {
                return Ok(None); // 同一吸附档不重复写ECS或作者修订。
            }
            record.validated_transform(&draft.scene_id, bounds)?;
            Ok(Some((record, bounds))) // 当前档保持纯候选，预览父缺失时不提前写入草稿。
        })();
        let (after, bounds) = match result {
            Ok(Some(value)) => value, // 当前档产生真实合法变化。
            Ok(None) => continue,     // 重复档保持现有事务。
            Err(error) => {
                editor.status = format!("{}拖动值无效：{error}", field.label());
                continue; // 拖动仍保持上一合法档，可继续拉回范围内。
            }
        };
        if let Err(error) = sync_placement_record_world_with_terrain(
            &mut commands,
            &runtime.asset_server,
            &mut session,
            &root_index,
            &roots,
            &after,
            bounds,
            true,
            true,
            &runtime.terrain_queries,
        ) {
            editor.status = format!("资产变换预览失败：{error}");
            continue; // 继续保留上一完整合法档，父级恢复后可接着拖动。
        }
        session.draft.as_mut().expect("当前档已复核场景")
            .placements.placements[target.record_index] = after.clone();
        session.mark_changed(&mut editor); // 每个可见档推进作者身份，迟到后台值失效。
        let transaction = state.scrub.as_mut().expect("当前拖动事务仍存在");
        transaction.latest_value = value; // 松手最终标量用于owned-field恢复。
        transaction.latest_record = after;
        transaction.expected_revision = session.revision; // 下一档只接受本事务刚推进的修订。
        editor.status = format!(
            "正在拖动{}：{value:.precision$}",
            field.label(),
            precision = usize::from(field.decimals()),
        );
    }

    for event in committed.read() {
        if event.source == NumericInputCommitSource::Scrub && state.rejected_scrub == Some(event.entity) {
            state.rejected_scrub = None;
            continue; // 即使字段组件已移除也先结清拒收；迟到松手本身不产生历史。
        }
        let Ok((_, field)) = inputs.get(event.entity) else {
            continue; // 只处理资产变换数字根。
        };
        if event.source == NumericInputCommitSource::Scrub {
            let Some(transaction) = state.scrub.take() else {
                continue; // 未跨阈值或没有真实Changed的单击不形成作者事务。
            };
            if transaction.entity != event.entity || transaction.field != *field {
                if transaction.entity != event.entity {
                    state.rejected_scrub = Some(transaction.entity); // 其他根的结束消息不是原捕获的松手证明。
                }
                let _ = restore_scrub_value(
                    &mut commands,
                    &runtime.asset_server,
                    &roots,
                    &root_index,
                    &mut editor,
                    &mut session,
                    runtime.clipmap_surface.as_deref(),
                    &runtime.terrain_queries,
                    &runtime.source_instances,
                    &transaction,
                    false,
                );
                editor.status = "资产变换拖动结束消息与活动事务不匹配".to_owned();
                continue;
            }
            if !input_context_valid(&editor, &session)
                || transaction.expected_revision != session.revision
                || !target_is_current(&transaction.target, &browser, &session)
            {
                let _ = restore_scrub_value(
                    &mut commands, &runtime.asset_server, &roots, &root_index,
                    &mut editor, &mut session, runtime.clipmap_surface.as_deref(),
                    &runtime.terrain_queries, &runtime.source_instances,
                    &transaction, false,
                );
                editor.status = "资产属性拖动在松手前目标已变化，旧输入已取消".to_owned();
                continue; // 最后一次Changed之后换选或发生作者变化，同样不能发布过期历史。
            }
            if !event.changed || event.outcome != NumericInputCommitOutcome::Accepted {
                match restore_scrub_value(
                    &mut commands,
                    &runtime.asset_server,
                    &roots,
                    &root_index,
                    &mut editor,
                    &mut session,
                    runtime.clipmap_surface.as_deref(),
                    &runtime.terrain_queries,
                    &runtime.source_instances,
                    &transaction,
                    true,
                ) {
                    Ok(true) => editor.status = format!("{}已拖回起点", field.label()),
                    Ok(false) => editor.status = format!("{}已由后来作者值接管", field.label()),
                    Err(error) => editor.status = format!("{}恢复起点失败：{error}", field.label()),
                }
                continue;
            }
            let Some(after) = session
                .draft
                .as_ref()
                .filter(|draft| draft.scene_id == transaction.target.scene_id)
                .and_then(|draft| {
                    draft
                        .placements
                        .placements
                        .get(transaction.target.record_index)
                })
                .filter(|record| record.id == transaction.target.placement_id)
                .cloned()
            else {
                editor.status = "资产变换拖动结束时记录已经失效".to_owned();
                continue;
            };
            let source_valid = session.draft.as_ref().is_some_and(|draft|
                patch_scrub_source(draft, &after, &runtime.terrain_queries, &runtime.source_instances)
                    .is_ok_and(|source| source == transaction.patch_source));
            if !source_valid || after != transaction.latest_record {
                let _ = restore_scrub_value(&mut commands, &runtime.asset_server, &roots, &root_index,
                    &mut editor, &mut session, runtime.clipmap_surface.as_deref(),
                    &runtime.terrain_queries, &runtime.source_instances, &transaction, true);
                editor.status = "资产属性拖动的来源或作者值已变化，旧输入已取消".to_owned();
                continue; // 最后一次 Changed 的来源信用不能跨真实父退役或同Entity换代沿用。
            }
            let session_history = &mut *session; // 拆借ResMut内部两个独立历史字段。
            session_history.push_bounded_water_history(EditorCommand::PlacementChanged {
                record_index: transaction.target.record_index, // 冻结的O(1)记录槽。
                before: transaction.before,                    // 拖动开始时唯一完整快照。
                after,                                         // 松手时唯一完整终点快照。
            }); // 任意数量Changed最终只形成一个可撤销操作。
            editor.status = format!("{}拖动完成；本次拖动可一次撤销", field.label());
            continue;
        }

        let bound_target = state
            .focus
            .as_ref()
            .filter(|focus| focus.entity == event.entity)
            .map(|focus| focus.target.clone()); // 优先使用中键聚焦时冻结的作者身份。
        if state
            .focus
            .as_ref()
            .is_some_and(|focus| focus.entity == event.entity)
        {
            state.focus = None; // 任一Text结果都结束当前业务焦点。
        }
        match event.outcome {
            NumericInputCommitOutcome::Canceled => {
                editor.status = format!("已恢复{}输入前的原值", field.label());
                continue;
            }
            NumericInputCommitOutcome::Rejected => {
                editor.status = format!("{}不是有效数字，已恢复原值", field.label());
                continue;
            }
            NumericInputCommitOutcome::Accepted if !event.edited => continue, // 未编辑失焦不量化作者值。
            NumericInputCommitOutcome::Accepted => {} // 有效字符提交继续业务复核。
        }
        if !input_context_valid(&editor, &session) {
            editor.status = "当前界面或保存状态不能提交资产变换".to_owned();
            continue;
        }
        let target = bound_target.or_else(|| selected_target(&browser, &session));
        let Some(target) = target.filter(|target| target_is_current(target, &browser, &session))
        else {
            editor.status = "资产变换精确输入目标已经失效".to_owned();
            continue;
        };
        let Some(value) = event.value.as_f32() else {
            editor.status = "资产变换数值控件返回了非浮点值".to_owned();
            continue;
        };
        match apply_exact_value(
            &mut commands,
            &runtime.asset_server,
            &roots,
            &root_index,
            &mut editor,
            &mut session,
            runtime.clipmap_surface.as_deref(),
                &runtime.terrain_queries,
            &target,
            *field,
            value,
        ) {
            Ok(Some(record)) => {
                editor.status = format!(
                    "{}已设为{value:.precision$}；资产 {} 已更新",
                    field.label(),
                    record.id,
                    precision = usize::from(field.decimals()),
                )
            }
            Ok(None) => editor.status = format!("{}保持不变", field.label()),
            Err(error) => editor.status = format!("{}提交失败：{error}", field.label()),
        }
    }

    let active_field = active
        .entity
        .and_then(|entity| inputs.get(entity).ok().map(|(_, field)| (entity, *field)));
    let Some((active_entity, active_field)) = active_field else {
        state.focus = None; // 字符焦点结束后不保留旧场景身份。
        return;
    };
    let current_target = selected_target(&browser, &session);
    if !input_context_valid(&editor, &session) || current_target.is_none() {
        set_values.write(NumericInputSetValue {
            entity: active_entity,         // 只取消当前资产字段根。
            value: NumericValue::F32(0.0), // Presentation随后同步真实或安全零值。
            cancel_edit: true,             // 丢弃过期字符缓冲并释放键盘焦点。
        });
        state.focus = None;
        return;
    }
    let current_target = current_target.expect("上方已经确认当前资产目标存在");
    let focus_is_current = state
        .focus
        .as_ref()
        .is_some_and(|focus| focus.entity == active_entity && focus.target == current_target);
    if state.focus.is_some() && !focus_is_current {
        let current = target_value(&current_target, active_field, &session).unwrap_or_default();
        set_values.write(NumericInputSetValue {
            entity: active_entity,
            value: NumericValue::F32(current),
            cancel_edit: true,
        });
        state.focus = None;
        editor.status = "资产属性精确输入的作者目标已变化，旧缓冲已取消".to_owned();
        return; // 已取得字符焦点的缓冲永远属于原对象，不能在换选后重绑到新对象。
    }
    if !focus_is_current {
        state.focus = Some(PlacementTransformFocus {
            entity: active_entity,  // 冻结本次中键输入根。
            target: current_target, // 冻结场景、结构、记录和作者修订。
        });
        let current = state
            .focus
            .as_ref()
            .and_then(|focus| target_value(&focus.target, active_field, &session))
            .unwrap_or_default();
        editor.status = format!(
            "正在精确输入{}（当前{current:.precision$}）；Enter或点击空白提交",
            active_field.label(),
            precision = usize::from(active_field.decimals()),
        );
    }
}

#[cfg(test)]
#[path = "cuboid_numeric_tests.rs"]
mod cuboid_tests;
