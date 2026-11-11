//! 完整摆放实例的高度模式、结构动作和运行时根定向同步。

use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*; // Commands、输入、Transform、Visibility与资源系统参数。
use feathrise_scene_content::{
    MAX_SCENE_PLACEMENTS_PER_SCENE, ScenePlacementHeightMode,
    ScenePlacementRecord,
}; // 复制数量、水绑定硬门与正式记录格式。
use feathrise_scene::{DynamicClipmapGroundSurface, ScenePlacementInstance}; // Clipmap高度模式读取已采用曲面；活动正式根和作者预览共享稳定身份。

use super::{
    SceneEditorPlacementTransformActionButton, PlacementRelocationIntent, PlacementTransformAction,
    PlacementTransformInputState, selected_target,
}; // 当前功能域按钮、模式和O(1)作者目标。
use crate::authoring::buildings::BuildingEditRequest; // 建筑旋转只提交意图，不直接绕过后台规则。
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, PlacementTerrainSampler, next_author_id,
    set_height_mode_preserving_world_y, spawn_placement_preview,
}; // 复制、变换和删除全部进入统一草稿、预览与历史链路。
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 数值手势占用、工具与状态栏。
use crate::types::EditorTool; // 吸附模式切到Inspect避免同时触发笔刷或新资产放置。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 完整实例选择和结构O(1)索引。
use crate::ui::placement_water::PlacementInstanceIndex; // 稳定ID只命中少量活动、候选和预览根。

/// 把一条已经写入草稿的完整记录定向应用到独立作者预览，正式批次只承担显隐遮蔽。
///
/// `spawn_when_missing`只用于仍应可见的编辑结果；删除动作绝不为缺失正式根反向生成模型。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_placement_record_world(
    commands: &mut Commands,    // 延迟写Transform、Visibility或新WorldAsset根。
    asset_server: &AssetServer, // 根尚未生成时按正式Scene0路径建立作者预览。
    session: &mut EditorAuthoringSession, // 精确预览所有权表避免扫描全部作者根。
    root_index: &PlacementInstanceIndex, // 正式根按稳定ID定向查找。
    roots: &Query<(Entity, &ScenePlacementInstance)>, // 每个候选仍复核SceneId、ID和活动门。
    record: &ScenePlacementRecord, // 当前草稿中的完整权威记录。
    world_bounds: Rect,         // 公共校验使用当前清单XZ边界。
    visible: bool,              // 撤销删除恢复Inherited，删除本身写Hidden。
    spawn_when_missing: bool,   // 变换时允许补预览，删除时禁止。
) -> Result<usize, String> {
    sync_placement_record_world_impl(commands, asset_server, session, root_index, roots,
        record, world_bounds, visible, spawn_when_missing, None)
}

/// 允许静态补丁的路径必须提供真实地形查询；源父尚未采用时不能退回世界根显示。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_placement_record_world_with_terrain(
    commands: &mut Commands, asset_server: &AssetServer, session: &mut EditorAuthoringSession,
    root_index: &PlacementInstanceIndex, roots: &Query<(Entity, &ScenePlacementInstance)>,
    record: &ScenePlacementRecord, world_bounds: Rect, visible: bool, spawn_when_missing: bool,
    terrain_queries: &feathrise_scene::TerrainObjectQueries,
) -> Result<usize, String> {
    sync_placement_record_world_impl(commands, asset_server, session, root_index, roots,
        record, world_bounds, visible, spawn_when_missing, Some(terrain_queries))
}

/// 校验和父级预检必须先于隐藏旧根、回收作者根或更新所有权表，失败只返回错误。
#[allow(clippy::too_many_arguments)]
fn sync_placement_record_world_impl(
    commands: &mut Commands, asset_server: &AssetServer, session: &mut EditorAuthoringSession,
    root_index: &PlacementInstanceIndex, roots: &Query<(Entity, &ScenePlacementInstance)>,
    record: &ScenePlacementRecord, world_bounds: Rect, visible: bool, spawn_when_missing: bool,
    terrain_queries: Option<&feathrise_scene::TerrainObjectQueries>,
) -> Result<usize, String> {
    let draft_scene_id = session
        .draft
        .as_ref()
        .map(|draft| draft.scene_id.clone())
        .ok_or_else(|| "场景作者草稿尚未就绪".to_owned())?; // 不能把记录应用到未知场景根。
    let draft = session.draft.as_ref().ok_or("场景作者草稿尚未就绪")?;
    let bounds = if record.terrain_patch.is_some() {
        crate::authoring::placement_pose::placement_record_frame(draft, record)?;
        crate::authoring::placement_record_bounds(draft, record)?
    } else { world_bounds };
    let transform = record.validated_transform(&draft_scene_id, bounds)?;
    let patch_parent = if visible && record.terrain_patch.is_some() {
        let parent = if let Some(queries) = terrain_queries {
            crate::authoring::patch_preview_parent(draft, queries, record)?
        } else if draft.manifest.terrains.is_some() {
            return Err("三维地形预览需要真实源父上下文".into());
        } else { None };
        parent.map(|parent| (parent, feathrise_scene_content::SceneInstancePath::root(draft_scene_id.clone())
            .terrain(record.terrain_patch.as_ref().and_then(|patch| patch.terrain_id.clone())
                .unwrap_or_else(feathrise_scene_content::TerrainId::legacy))))
    } else { None };
    let attach_preview = |commands: &mut Commands, entity: Entity| {
        if let Some((parent, path)) = &patch_parent {
            commands.entity(entity).insert((ChildOf(*parent), feathrise_scene::TerrainVolumeAuthorPreviewRoot(path.clone())));
        } else if visible && record.terrain_patch.is_some() {
            // 补丁回到旧主地形时脱离旧源；普通同类更新必须保留既有物理/显隐公共父。
            commands.entity(entity).remove::<(ChildOf, feathrise_scene::TerrainVolumeAuthorPreviewRoot)>();
        }
    };
    let visibility = if visible {
        Visibility::Inherited // 恢复时继续服从父层级和正式批次门。
    } else {
        Visibility::Hidden // 删除后立即从作者视口消失但正式根仍可由撤销恢复。
    };
    let author_preview = session.placement_previews.get(&record.id).copied().filter(|entity|
        roots.get(*entity).is_ok_and(|(_, instance)| instance.scene_id() == &draft_scene_id && instance.id() == record.id));
    if author_preview.is_none() {
        // 源父退役可能已级联删除旧根；通过全部前置校验后才撤销陈旧地址，不写死实体或其他拥有者。
        session.placement_previews.remove(&record.id);
    }
    if let Some(entity) = author_preview {
        let builtin_changed = record.asset_target.is_builtin()
            && roots.get(entity).is_ok_and(|(_, instance)| instance.asset_target() != record.asset_target);
        if roots.get(entity).is_ok_and(|(_, instance)| {
            instance.is_terrain_patch() != record.terrain_patch.is_some()
        }) && visible
        {
            commands.entity(entity).try_despawn();
            let replacement =
                spawn_placement_preview(commands, asset_server, &draft_scene_id, record);
            attach_preview(commands, replacement);
            session
                .placement_previews
                .insert(record.id.clone(), replacement);
            return Ok(1); // 分类切换仅替换作者模型，正式碰撞仍等待完整保存候选采用。
        }
        attach_preview(commands, entity);
        commands.entity(entity).insert((
            transform,  // 作者预览同帧采用完整位置、旋转和缩放。
            visibility, // 删除/恢复不销毁可撤销预览身份。
            ScenePlacementInstance::new(&draft_scene_id, record),
        ));
        if builtin_changed {
            feathrise_scene::spawn_builtin_placement_visual(commands, entity, record.asset_target);
        }
        if record.asset_target.is_builtin() {
            feathrise_scene::set_builtin_placement_collision_enabled(commands, entity, visible);
        }
        crate::authoring::placement_preview_ownership::claim_preview(commands, &draft_scene_id, &record.id, entity);
        return Ok(1); // 精确作者预览优先于任何正式旧根。
    }
    if record.building_preset_id.is_some() {
        let mut updated = 0;
        root_index.for_each_active_root(&draft_scene_id, &record.id, roots, |entity| {
            commands.entity(entity).insert((transform, visibility));
            updated += 1;
        });
        if updated > 0 { return Ok(updated); }
    } // 建筑暂沿用受检碰撞链；验证失败和取消也必须恢复原姿态，不能只在提交分支更新。
    if visible && spawn_when_missing {
        // 【需求设计】首次编辑也要取得独立作者根。旧保存候选可以随时采用并退役旧正式树，
        // 不能把后续修改写入该树；静态三维地形仍只预览模型，洞口与正式碰撞等待联合采用。
        let entity = spawn_placement_preview(commands, asset_server, &draft_scene_id, record);
        attach_preview(commands, entity);
        session.placement_previews.insert(record.id.clone(), entity);
        crate::authoring::placement_preview_ownership::claim_preview(commands, &draft_scene_id, &record.id, entity);
        return Ok(1);
    }
    let mut updated = 0usize; // 正常稳定状态只会命中一个活动正式根。
    root_index.for_each_active_root(&draft_scene_id, &record.id, roots, |entity| {
        commands.entity(entity).insert(visibility); // 删除或无自有预览的取消只改显隐，不取得正式姿态/记录写权限。
        if record.asset_target.is_builtin() {
            feathrise_scene::set_builtin_placement_collision_enabled(commands, entity, visible);
        }
        updated = updated.saturating_add(1); // 小集合计数只用于决定是否补作者预览。
    });
    Ok(updated) // 零表示删除时世界里本来就没有可见根，并非作者数据失败。
}

#[cfg(test)]
#[path = "preview_recovery_tests.rs"]
mod preview_recovery_tests;

/// 取得当前选择的完整记录快照并复核所有作者身份。
fn selected_record(
    browser: &EditorGltfBrowserState, // 当前场景树选择。
    session: &EditorAuthoringSession, // 当前草稿与结构修订。
) -> Result<(super::PlacementTransformTarget, ScenePlacementRecord), String> {
    let target = selected_target(browser, session)
        .ok_or_else(|| "当前没有可编辑的稳定资产选择".to_owned())?; // 重复ID或在途结构明确拒绝。
    let record = session
        .draft
        .as_ref()
        .and_then(|draft| draft.placements.placements.get(target.record_index))
        .filter(|record| record.id == target.placement_id)
        .cloned()
        .ok_or_else(|| "选中资产记录已经失效".to_owned())?; // 边沿只克隆一条小型完整记录。
    Ok((target, record)) // 调用方可安全结束只读借用后修改会话。
}

/// 处理高度模式、吸附、复制和删除按钮按下边沿。
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_placement_transform_actions(
    mut commands: Commands, // 复制生成Scene0预览，删除即时隐藏或回收预览。
    buttons: Query<(&EditorButtonClick, &SceneEditorPlacementTransformActionButton), Changed<EditorButtonClick>>, // 固定五按钮输入。
    numeric_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 数值拖动和弦期间拒绝按钮穿透。
    asset_server: Res<AssetServer>,                      // 复制记录立即生成作者预览。
    root_index: Res<PlacementInstanceIndex>,             // 删除正式记录时定向隐藏当前活动根。
    roots: Query<(Entity, &ScenePlacementInstance)>,       // 根索引返回后复核活动身份。
    mut browser: ResMut<EditorGltfBrowserState>,         // 复制选中新ID，删除清理悬空选择。
    mut transform_state: ResMut<PlacementTransformInputState>, // 吸附模式与旧焦点统一收口。
    mut editor: ResMut<SceneEditorState>,                  // 生命周期、工具、dirty和用户状态。
    mut session: ResMut<EditorAuthoringSession>,         // 完整草稿、历史与作者预览所有权。
    mut building_edits: MessageWriter<BuildingEditRequest>, // 建筑动作由专用single-flight校验域采用。
    clipmap_surface: Option<Res<DynamicClipmapGroundSurface>>, // 普通Clipmap资产切换跟随时读取正式三角面。
    batch: Option<Res<crate::authoring::placement_batch::PlacementBatchState>>,
    selection: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    mesh_edit: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
    terrain_context: (feathrise_scene::TerrainObjectQueries, Option<Res<crate::ui::terrain_objects::TerrainObjectsEditorState>>),
) {
    if mesh_edit.as_ref().is_some_and(|state| state.active()) { return; } // 点边面会话冻结完整对象，不能同帧删除或换配方。
    let Some(action) = buttons.iter().find_map(|(interaction, button)| {
        (interaction.is_clicked()).then_some(button.0)
    }) else {
        return; // hover样式由Presentation负责，稳定帧不写资源。
    };
    if batch.as_ref().is_some_and(|batch| batch.busy()) {
        return; // 批量域已取得同一按钮的所有权，不能再执行单对象动作或覆盖其状态。
    }
    if selection.as_ref().is_some_and(|selection| selection.members.len() > 1) {
        editor.status = "多选请使用视口手柄或整组复制、删除按钮".into();
        return; // 单对象高度和数值操作不能只修改整组中的第一项。
    }
    if !editor.open || session.saving || numeric_mouse.blocked() {
        editor.status = "当前正在保存或编辑数值，暂不能执行资产结构操作".to_owned();
        return; // 保存快照和公共鼠标手势期间失败关闭。
    }
    let (target, record) = match selected_record(&browser, &session) {
        Ok(value) => value, // 取得唯一稳定实例。
        Err(error) => {
            editor.status = error; // 结构在途或选择失效时明确提示。
            return;
        }
    };
    let is_building = record.building_preset_id.is_some(); // 建筑禁用普通任意变换路径。
    match action {
        PlacementTransformAction::ToggleCuboidCollision => {
            if let Err(error) = super::cuboid::toggle_collision(
                &target, record, &mut commands, &asset_server, &mut session,
                &root_index, &roots, &mut editor,
            ) {
                editor.status = format!("立方体碰撞设置失败：{error}");
            }
            transform_state.focus = None;
            transform_state.relocation_target = None;
        }
        PlacementTransformAction::ToggleTerrainPatch
        | PlacementTransformAction::ToggleTerrainCutout => {
            if let Err(error) = super::terrain::apply(
                action,
                &target,
                record,
                &mut commands,
                &asset_server,
                &mut session,
                &root_index,
                &roots,
                &mut editor,
                &terrain_context.0,
                terrain_context.1.as_ref().filter(|state| state.scene_id.as_ref() == Some(&target.scene_id))
                    .and_then(|state| state.selected.as_ref()),
            ) {
                editor.status = format!("三维地形设置失败：{error}");
            }
            transform_state.focus = None;
            transform_state.relocation_target = None;
        }
        PlacementTransformAction::Relocate => {
            transform_state.relocation_target = Some(target); // 冻结按钮按下时的场景、下标、ID与结构修订。
            transform_state.relocation_intent = PlacementRelocationIntent::Move;
            transform_state.focus = None; // 旧字符焦点不能跨吸附模式继续提交。
            editor.tool = EditorTool::Inspect; // 禁止同次点击触发笔刷、传送或新资产放置。
            editor.status = if is_building {
                format!(
                    "移动建筑 {}：请在中央视口单击新位置，随后后台检查占地、门口与五点地基",
                    record.id,
                )
            } else {
                format!("吸附地面 {}：请在中央视口单击新的真实地形落点", record.id)
            }; // 建筑点击只提交候选；普通资产继续直接采用真实三角命中。
        }
        PlacementTransformAction::FollowTerrain | PlacementTransformAction::FixedHeight => {
            if record.terrain_patch.is_some() && action == PlacementTransformAction::FollowTerrain {
                editor.status = "三维地形固定在所属源坐标，不能改为跟随地形".into();
                return;
            }
            if is_building {
                editor.status = "建筑固定使用五点地基高度，不能切换普通资产高度模式".to_owned();
                return;
            }
            let mode = if action == PlacementTransformAction::FollowTerrain {
                ScenePlacementHeightMode::FollowTerrain // 按钮身份直接映射正式数据枚举。
            } else {
                ScenePlacementHeightMode::FixedHeight // 固定按钮不依赖内存高度场是否存在。
            };
            let (after, bounds) = {
                let Some(draft) = session.draft.as_ref() else {
                    editor.status = "场景作者草稿尚未就绪".to_owned();
                    return;
                };
                let bounds = match crate::authoring::placement_record_bounds(draft, &record) {
                    Ok(bounds) => bounds,
                    Err(error) => { editor.status = error; return; }
                };
                let sampler = if mode.follows_terrain() {
                    match PlacementTerrainSampler::from_sources(
                        draft,
                        clipmap_surface.as_deref(),
                        session.terrain_revision,
                    ) {
                        Ok(value) => Some(value), // 切入跟随时冻结当前地形采样身份。
                        Err(error) => {
                            editor.status = format!("不能启用跟随地形：{error}");
                            return; // 旧GLTF地形继续保留固定世界Y。
                        }
                    }
                } else {
                    None // 切回固定高度完全不访问内存高度场。
                };
                let mut after = record.clone(); // 候选先局部修改，失败不污染草稿。
                match set_height_mode_preserving_world_y(&mut after, mode, sampler.as_ref()) {
                    Ok(true) => {} // 当前世界Y保持不动，仅切换模式与偏移元数据。
                    Ok(false) => {
                        editor.status = match mode {
                            ScenePlacementHeightMode::FollowTerrain => {
                                "当前资产已经跟随地形".to_owned()
                            }
                            ScenePlacementHeightMode::FixedHeight => {
                                "当前资产已经使用固定高度".to_owned()
                            }
                        };
                        return; // 重复点击不制造历史或dirty。
                    }
                    Err(error) => {
                        editor.status = format!("切换资产高度模式失败：{error}");
                        return;
                    }
                }
                if let Err(error) = after.validated_transform(&draft.scene_id, bounds) {
                    editor.status = format!("切换资产高度模式失败：{error}");
                    return; // 公共运行时校验先于草稿写入。
                }
                (after, bounds) // 借用结束后按冻结O(1)槽写回。
            };
            let Some(current) = session
                .draft
                .as_mut()
                .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
                .filter(|current| current.id == target.placement_id)
            else {
                editor.status = "切换高度模式前资产记录已经失效".to_owned();
                return;
            };
            *current = after.clone(); // 模式、偏移和保持不动的世界Y作为完整记录原子采用。
            if let Err(error) = sync_placement_record_world_with_terrain(
                &mut commands, // 重施完整Transform并补缺失预览。
                &asset_server, // Scene0来源保持不变。
                &mut session,  // 维护作者预览所有权。
                &root_index,   // 稳定ID定向当前活动根。
                &roots,        // 复核批次活动身份。
                &after,        // 新高度语义的完整记录。
                bounds,        // 当前场景正式XZ范围。
                true,          // 模式切换不隐藏实例。
                true,          // 缺根时保持作者结果可见。
                &terrain_context.0,
            ) {
                if let Some(current) = session
                    .draft
                    .as_mut()
                    .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
                    .filter(|current| current.id == record.id)
                {
                    *current = record; // ECS同步失败时恢复完整起点。
                }
                editor.status = format!("切换资产高度模式预览失败：{error}");
                return;
            }
            let session_history = &mut *session; // 拆借ResMut内部两个独立历史字段。
            session_history.push_bounded_water_history(EditorCommand::PlacementChanged {
                record_index: target.record_index, // 冻结的O(1)记录槽。
                before: record,                    // 切换前完整记录。
                after: after.clone(),              // 切换后完整记录。
            });
            transform_state.focus = None; // 旧字符焦点不能跨模式语义继续提交。
            transform_state.relocation_target = None; // 显式按钮动作结束旧落点意图。
            session.mark_placement_structure_changed(&mut editor); // 高度模式改变承托归属，撤销与跟随采样游标同时换代。
            editor.status = match after.height_mode {
                ScenePlacementHeightMode::FollowTerrain => format!(
                    "资产 {} 已改为跟随地形，当前离地偏移 {:.3} m",
                    after.id, after.terrain_offset,
                ),
                ScenePlacementHeightMode::FixedHeight => {
                    format!(
                        "资产 {} 已改为固定高度 {:.3} m",
                        after.id, after.translation[1]
                    )
                }
            };
        }
        PlacementTransformAction::RotateQuarterTurn => {
            if !is_building {
                editor.status = "90°受检旋转仅用于建筑预设实例".to_owned();
                return;
            }
            building_edits.write(BuildingEditRequest::RotateQuarterTurn {
                target: target.building_edit_target(),
            });
            transform_state.focus = None;
            transform_state.scrub = None;
            transform_state.relocation_target = None;
            transform_state.relocation_intent = PlacementRelocationIntent::Move;
            editor.status = format!("已提交建筑 {} 的90°旋转后台校验", record.id);
        }
        PlacementTransformAction::Duplicate => {
            let Some(draft) = session.draft.as_ref() else {
                editor.status = "场景作者草稿尚未就绪".to_owned();
                return;
            };
            if draft.placements.placements.len() >= MAX_SCENE_PLACEMENTS_PER_SCENE {
                editor.status = format!(
                    "当前场景已达到{MAX_SCENE_PLACEMENTS_PER_SCENE}个显式资产上限，不能继续复制"
                );
                return; // 海量重复内容应改用植被系统而不是突破运行时预算。
            }
            if is_building {
                transform_state.relocation_target = Some(target);
                transform_state.relocation_intent = PlacementRelocationIntent::DuplicateBuilding;
                transform_state.focus = None;
                transform_state.scrub = None;
                editor.tool = EditorTool::Inspect;
                editor.status = format!(
                    "复制建筑 {}：请在中央视口单击新位置；校验成功前不会生成重叠副本",
                    record.id,
                );
                return;
            }
            let Some(next_surface_count) = draft
                .placement_water_surface_count
                .checked_add(record.water_surfaces.len())
            else {
                editor.status = "复制资产时整网格水绑定计数发生整数溢出".to_owned();
                return; // 损坏缓存不得通过饱和加法伪装成合法作者状态。
            }; // 含水语义的完整复制必须先证明全图预算不会溢出。
            let scene_id = draft.scene_id.clone(); // 结束只读借用后生成ID和写入数组。
            let bounds = match crate::authoring::placement_record_bounds(draft, &record) {
                Ok(bounds) => bounds,
                Err(error) => { editor.status = format!("复制资产失败：{error}"); return; }
            };
            if record.terrain_patch.is_some()
                && let Err(error) = crate::authoring::patch_preview_parent(draft, &terrain_context.0, &record)
            {
                editor.status = format!("复制三维地形失败：{error}");
                return; // 父级预检先于序号、作者数组和历史变化。
            }
            let mut duplicate = record.clone(); // 复制保留来源、完整变换和网格水体语义。
            duplicate.id = next_author_id(&mut session, "placement"); // 新记录必须拥有唯一稳定实例ID。
            duplicate.courtyard = None; // 单体复制须脱离原院落副本，整组复制由院落工具生成新实例身份。
            if let Err(error) = sync_placement_record_world_with_terrain(
                &mut commands, &asset_server, &mut session, &root_index, &roots,
                &duplicate, bounds, true, true, &terrain_context.0,
            ) {
                editor.status = format!("复制资产预览失败：{error}");
                return;
            }
            let Some(draft) = session.draft.as_mut() else {
                return; // 上方已确认草稿存在，防御未来系统参数重构。
            };
            draft.placement_water_surface_count = next_surface_count; // 使用前置受检结果维护全图O(1)预算缓存。
            draft.placements.placements.push(duplicate.clone()); // 新实例保持追加语义，PlacementAdded撤销可O(1)弹尾。
            let duplicate_index = draft.placements.placements.len() - 1; // 刚追加记录的O(1)槽不等待浏览树重建。
            let session_history = &mut *session; // 拆借统一历史并让含水复制服从已有内存上限。
            session_history.push_bounded_water_history(
                EditorCommand::PlacementAdded(duplicate.clone()), // 来源、变换和水语义完整保留。
            );
            session.mark_placement_structure_changed(&mut editor); // 新根让浏览器分帧重建结构签名。
            browser.selected_placement_id = Some(duplicate.id.clone()); // 结构新代完成前保留用户刚复制的明确目标。
            browser.selection = None; // 新实例尚未选择具体Blender网格。
            transform_state.relocation_target = Some(super::PlacementTransformTarget {
                scene_id,                                                   // 新实例仍属于当前作者场景。
                placement_id: duplicate.id.clone(), // 不等待浏览器新签名发布即可冻结新ID。
                record_index: duplicate_index,      // 刚追加的精确数组槽。
                structure_revision: session.placement_structure_revision, // 使用结构动作完成后的新修订。
                authoring_revision: session.revision, // 落点前任何其他作者动作都会使旧意图失效。
            }); // 复制后立即进入可用落点模式，不依赖最多8192条签名的分帧重建。
            transform_state.relocation_intent = PlacementRelocationIntent::Move;
            transform_state.focus = None; // 旧实例字符焦点失效。
            editor.tool = EditorTool::Inspect; // 落点点击不会再触发原PlaceAsset工具。
            editor.status = format!(
                "已复制资产 {}；请在中央视口单击新的真实地形吸附落点，本次复制可撤销",
                duplicate.id,
            );
        }
        PlacementTransformAction::Delete => {
            let bounds = match session.draft.as_ref().ok_or_else(|| "场景作者草稿尚未就绪".to_owned())
                .and_then(|draft| crate::authoring::placement_record_bounds(draft, &record)) {
                Ok(bounds) => bounds,
                Err(error) => { editor.status = format!("删除资产失败：{error}"); return; }
            };
            let author_preview = session
                .placement_previews
                .get(&record.id)
                .copied()
                .filter(|entity| roots.get(*entity).is_ok_and(|(_, instance)| {
                    instance.scene_id() == &target.scene_id && instance.id() == record.id
                })); // 陈旧地址只撤销映射，真实拥有者匹配才允许销毁。
            if author_preview.is_none()
                && let Err(error) = record.validated_transform(&target.scene_id, bounds) {
                editor.status = format!("删除资产前隐藏预览失败：{error}");
                return; // 保留原无作者预览分支的格式门，但删除不再把作者姿态写回正式根。
            }
            let Some(draft) = session.draft.as_ref() else {
                return; // 上方已确认当前草稿存在。
            };
            if !draft
                .placements
                .placements
                .get(target.record_index)
                .is_some_and(|candidate| candidate.id == record.id)
            {
                editor.status = "选中资产下标已经变化，删除已取消".to_owned();
                return; // O(1)下标失效时不扫描猜测另一条记录。
            }
            if let Err(error) = crate::authoring::placement_deletion::validate_deletions(
                &session, std::iter::once(record.id.as_str()),
            ) {
                editor.status = format!("删除资产失败：{error}");
                return; // 容量不足不能先回收预览、删除记录或更换已有删除票据。
            }
            // 全部失败门之后回收自有预览；正式根由成功采用后的删除票据遮蔽。
            session.placement_previews.remove(&record.id);
            let had_author_preview = author_preview.is_some();
            if let Some(entity) = author_preview {
                commands.entity(entity).try_despawn();
            }
            let draft = session.draft.as_mut().expect("删除前已复核作者草稿");
            draft.placements.placements.remove(target.record_index); // 精确移除冻结数组项并保持其他顺序。
            draft.placement_water_surface_count = draft
                .placement_water_surface_count
                .saturating_sub(record.water_surfaces.len()); // 删除含水资产同步维护全图预算缓存。
            crate::authoring::placement_deletion::mark_deleted(
                &mut commands, &mut session, &target.scene_id, &record.id,
            ); // 票据独立于预览实体存活；旧保存候选后续Add也必须保持隐藏。
            let session_history = &mut *session; // 含水资产删除快照继续服从统一水历史内存门。
            session_history.push_bounded_water_history(EditorCommand::PlacementRemoved {
                index: target.record_index, // 撤销恢复原列表位置。
                record: record.clone(),     // 完整来源、变换和水语义原样恢复。
                had_author_preview,         // 决定撤销是重建预览还是恢复正式根显隐。
            });
            session.mark_placement_structure_changed(&mut editor); // 根和子网格行都必须分帧重建。
            browser.selected_placement_id = None; // 不留下已删除完整实例目标。
            if browser
                .selection
                .as_ref()
                .is_some_and(|selection| selection.placement_id == record.id)
            {
                browser.selection = None; // 所属网格水选择同帧失效。
            }
            transform_state.focus = None; // 字符焦点不能指向已删除记录。
            transform_state.scrub = None; // 正常按钮帧不会存在拖动，仍显式释放防御状态。
            transform_state.relocation_target = None; // 删除结束任何旧落点模式。
            transform_state.relocation_intent = PlacementRelocationIntent::Move;
            editor.status = format!("已删除资产 {}；可通过撤销恢复", record.id);
        }
    }
}
