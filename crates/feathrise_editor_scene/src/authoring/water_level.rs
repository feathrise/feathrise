//! 河水静止水位的精确提交、左拖实时预览与单次撤销事务。
//!
//! 【功能设计】文本输入与“取地形水位”仍是离散作者操作；左拖途中每个新吸附档只换代
//! single-flight真实水面预览，松开后把整段拖动合并为一个起点到终点命令。

#[cfg(test)]
#[path = "water_level/tests.rs"]
mod tests;
#[path = "water_level/transaction.rs"]
mod transaction; // 冲突或量化回起点时只恢复稳定水层的水位字段。 // 业务范围、历史收口与owned-value回滚回归。

use bevy::prelude::*; // 公共数字消息、编辑器资源和ECS查询。
use feathrise_scene_content::{SceneTerrainKind, SceneWaterAsset}; // 水位只修改正式共享水域描述中的稳定层。
use feathrise_editor_ui::{
    ActiveNumericInputScrub, NumericInputChanged, NumericInputCommitSource, NumericInputCommitted,
}; // Changed驱动实时水面，全局捕获身份防止异常后重绑。

use super::resources::{EditorAuthoringSession, EditorCommand, WaterLevelScrubState}; // 水位拖动共享草稿修订、撤销栈和稳定事务身份。
use super::water_network::sync_water_network_connections; // 静水位与河流首尾、瀑布静水端保持同一作者事务连续。
use crate::components::SceneEditorWaterLevelInput; // 只消费F12水位根的公共数字消息。
use crate::constants::{MAX_WATER_LEVEL_METERS, MIN_WATER_LEVEL_METERS}; // 输入与作者应用共享防损坏范围。
use crate::resources::SceneEditorState; // 精确请求、dirty门和用户可见状态。
use crate::types::{WaterLevelInputTarget, WaterLevelSetRequest}; // 场景、修订和稳定层联合身份。
use transaction::rollback_water_level_scrub_if_owned; // 异常不用完整旧水网快照覆盖外部结构。

/// 按场景与稳定层ID修改一份水域描述；失败时保证任何层均不发生变化。
pub(crate) fn set_water_level(
    water: &mut SceneWaterAsset,
    request: &WaterLevelSetRequest,
) -> Result<(String, f32, f32), String> {
    if !request.water_level.is_finite()
        || !(MIN_WATER_LEVEL_METERS..=MAX_WATER_LEVEL_METERS).contains(&request.water_level)
    {
        return Err(format!(
            "水位必须是{MIN_WATER_LEVEL_METERS:.0}～{MAX_WATER_LEVEL_METERS:.0}m之间的有限数字"
        ));
    }
    if water.scene_id != request.target.scene_id {
        return Err(format!(
            "水位输入属于场景{}，当前水域文件属于{}；过期请求已丢弃",
            request.target.scene_id, water.scene_id
        ));
    }
    let (label, before) = {
        let layer = water
            .layers
            .iter_mut()
            .find(|layer| layer.id == request.target.layer_id)
            .ok_or_else(|| format!("水域层{}已不存在，水位请求已丢弃", request.target.layer_id))?;
        let before = layer.water_level;
        if before != request.water_level {
            layer.water_level = request.water_level; // 真实位级变化才写作者描述。
        }
        (layer.label.clone(), before) // 先结束层可变借用，再同步引用该层的全部连接端点。
    };
    if before != request.water_level {
        sync_water_network_connections(water); // 精确输入、取地形水位和实时scrub共用这一原子派生步骤。
    }
    Ok((label, before, request.water_level))
}

/// 作者应用失败时阻止同帧保存旧值，并保留真正失败原因供用户修正。
fn reject_water_level_request(editor: &mut SceneEditorState, reason: impl Into<String>) {
    let reason = reason.into();
    editor.status = if std::mem::take(&mut editor.save_requested) {
        format!("{reason}；本次保存已取消")
    } else {
        reason
    };
}

/// 消费精确输入或“取地形水位”请求；每次有效提交形成一条可撤销JSON参数操作。
pub(crate) fn apply_water_level_requests(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut ground_cursor: Option<ResMut<crate::viewport::vegetation_ground::VegetationGroundCursor>>, // Clipmap水位采样消费独立用途身份。
    clipmap_surface: Option<feathrise_scene::TerrainObjectClipmapGround>, // 只接受同代已采用真实三角面。
) {
    if editor.water_level_from_cursor_requested.is_none()
        && editor.water_level_set_requested.is_none()
    {
        return; // 没有离散水位操作时稳定帧不访问草稿。
    }
    let cursor_target = editor.water_level_from_cursor_requested.clone();
    let use_cursor = cursor_target.is_some();
    let exact_request = editor.water_level_set_requested.take();
    if !editor.open || editor.close_requested || editor.pending_scene_transition.is_some() {
        editor.water_level_from_cursor_requested = None;
        return; // F12关闭入口统一丢弃尚未进入作者域的离散请求。
    }
    if session.saving {
        editor.water_level_from_cursor_requested = None;
        editor.status = "保存期间不能修改静水体水位".to_owned();
        return;
    }
    let request = if let Some(target) = cursor_target {
        if target.authoring_revision != session.revision {
            editor.water_level_from_cursor_requested = None;
            reject_water_level_request(
                &mut editor,
                format!(
                    "取水位请求基于作者修订{}，当前已是修订{}；请重新选择地形点",
                    target.authoring_revision, session.revision
                ),
            );
            return;
        }
        let Some((clipmap, selected_layer_id)) = session.draft.as_ref().and_then(|draft| {
            (draft.scene_id == target.scene_id).then_some(())?;
            let layer_id = draft
                .water
                .as_ref()?
                .layers
                .get(editor.selected_water_layer)?
                .id
                .clone();
            Some((
                draft.manifest.terrain.kind == SceneTerrainKind::SparseClipmap,
                layer_id,
            ))
        }) else {
            editor.water_level_from_cursor_requested = None;
            editor.status = "取水位目标场景或静水层已经变化；请重新点击取地形水位".to_owned();
            return;
        };
        if selected_layer_id != target.layer_id || editor.tool != crate::types::EditorTool::Water {
            editor.water_level_from_cursor_requested = None;
            editor.status = "当前静水层已经变化；请重新点击取地形水位".to_owned();
            return;
        }
        let cursor_level = if clipmap {
            ground_cursor.as_deref_mut().and_then(|cursor| {
                cursor.take_water_level_hit(
                    clipmap_surface.as_deref()?.snapshot.as_ref()?,
                    &target.scene_id,
                    &target.layer_id,
                )
            })
        } else {
            editor.cursor_world
        };
        let Some(cursor_level) = cursor_level.map(|point| point.y) else {
            if clipmap {
                editor.status =
                    "请把光标移到当前已采用的Geometry Clipmap真实地形上；高度就绪后自动取得水位"
                        .to_owned();
                return; // 跨侧栏到视口期间保留冻结身份，不使用水平面或旧层命中。
            }
            editor.water_level_from_cursor_requested = None;
            editor.status = "视口光标尚未命中当前已采用地形，无法取得水位".to_owned();
            return;
        };
        editor.water_level_from_cursor_requested = None;
        WaterLevelSetRequest {
            target,
            water_level: cursor_level.clamp(MIN_WATER_LEVEL_METERS, MAX_WATER_LEVEL_METERS),
        }
    } else if let Some(request) = exact_request {
        request
    } else {
        return;
    };
    if request.target.authoring_revision != session.revision {
        reject_water_level_request(
            &mut editor,
            format!(
                "水位输入基于作者修订{}，当前已是修订{}；过期请求已丢弃",
                request.target.authoring_revision, session.revision
            ),
        );
        return;
    }
    let result = session
        .draft
        .as_mut()
        .ok_or_else(|| "当前场景没有可编辑湖泊/池塘".to_owned())
        .and_then(|draft| {
            if draft.scene_id != request.target.scene_id {
                return Err(format!(
                    "水位输入属于场景{}，当前草稿属于{}；过期请求已丢弃",
                    request.target.scene_id, draft.scene_id
                ));
            }
            draft
                .water
                .as_mut()
                .ok_or_else(|| "当前场景没有可编辑湖泊/池塘".to_owned())
                .and_then(|water| set_water_level(water, &request))
        });
    let (label, before, after) = match result {
        Ok(change) => change,
        Err(error) => {
            reject_water_level_request(&mut editor, error);
            return;
        }
    };
    if before == after {
        if use_cursor {
            editor.tool = crate::types::EditorTool::Water; // 显式按钮保持河水工具，文本失焦不抢当前工具。
        }
        editor.status = format!("水域层“{label}”静止水位保持{after:.3}m");
        return;
    }
    let session = &mut *session; // 显式解引用ResMut后可同时独占拆借undo与redo字段。
    session.push_bounded_water_history(EditorCommand::WaterLevelChanged {
        layer_id: request.target.layer_id.clone(),
        before,
        after,
    });
    session.mark_water_definition_changed(&mut editor); // JSON变化只换代水域预览，不改写GLB地形。
    if use_cursor {
        editor.tool = crate::types::EditorTool::Water;
    }
    editor.status = format!("水域层“{label}”静止水位已设为{after:.3}m；正在刷新真实水面");
}

/// 当前选择解析为左拖起点；只在第一次真实Changed时冻结身份和高精度作者值。
pub(crate) fn begin_water_level_scrub(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    scrub: &mut WaterLevelScrubState,
) -> Result<(), String> {
    if !editor.open || editor.close_requested || editor.new_scene_dialog_open {
        return Err("当前界面状态不能拖动静水体水位".to_owned());
    }
    if session.saving {
        return Err("保存期间不能拖动静水体水位".to_owned());
    }
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "当前场景没有可编辑湖泊/池塘".to_owned())?;
    let layer = draft
        .water
        .as_ref()
        .and_then(|water| water.layers.get(editor.selected_water_layer))
        .ok_or_else(|| "当前场景没有可编辑湖泊/池塘".to_owned())?;
    scrub.target = Some(WaterLevelInputTarget {
        scene_id: draft.scene_id.clone(),
        layer_id: layer.id.clone(),
        authoring_revision: session.revision,
    });
    scrub.before = layer.water_level; // 保留未量化作者值作为整段拖动撤销起点。
    scrub.after = layer.water_level;
    scrub.expected_revision = session.revision;
    scrub.dirty_before = editor.dirty;
    scrub.aborted = false; // 新事务允许正常接受Changed。
    Ok(())
}

/// 冲突或异常中断只回滚仍由本事务拥有的水位，并锁存到松手。
pub(crate) fn abort_water_level_scrub(
    editor: &mut SceneEditorState,          // 底栏、dirty与保存拒绝门。
    session: &mut EditorAuthoringSession, // 当前草稿可能已有外部新分支。
    scrub: &mut WaterLevelScrubState,     // 持有before/latest、修订与稳定层身份。
    reason: impl Into<String>,            // 面向作者的确定取消原因。
) {
    let same_branch = session.revision == scrub.expected_revision; // 只有无外部作者分支才可恢复进入前dirty。
    let rolled_back = rollback_water_level_scrub_if_owned(session, scrub); // 同图同层且水位仍为latest时才写before。
    if rolled_back && same_branch {
        editor.dirty = scrub.dirty_before; // 纯异常取消不留下本拖动制造的保存提示。
    }
    scrub.aborted = true; // 保留到Scrub Committed，后续Changed不能重新begin。
    reject_water_level_request(editor, reason); // 同时取消可能的同帧保存请求。
}

/// 把松开前的整段实时修改收口成至多一条撤销命令。
pub(crate) fn finish_water_level_scrub(
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    scrub: &mut WaterLevelScrubState,
    label: &str,
) {
    let Some(target) = scrub.target.as_ref() else {
        return; // 未跨入新吸附档的拖动没有作者事务。
    };
    if scrub.before != scrub.after {
        session.push_bounded_water_history(EditorCommand::WaterLevelChanged {
            layer_id: target.layer_id.clone(), // 始终使用第一次变化时冻结的稳定层ID。
            before: scrub.before,
            after: scrub.after,
        });
        editor.status = format!(
            "水域层“{label}”静止水位已拖到{:.3}m；本次拖动形成一个可撤销操作",
            scrub.after
        );
    } else {
        editor.dirty = scrub.dirty_before; // 拖回精确起点不应制造新的未保存状态。
        editor.status = format!("水域层“{label}”静止水位保持{:.3}m", scrub.after);
    }
    scrub.clear();
}

/// 消费水位根的左拖Changed并在Scrub松开时合并撤销；每次实时变化沿用现有single-flight预览。
pub(crate) fn apply_water_level_scrub(
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
    inputs: Query<Entity, With<SceneEditorWaterLevelInput>>,
    active_scrub: Res<ActiveNumericInputScrub>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut scrub: ResMut<WaterLevelScrubState>,
) {
    let input_entity = inputs.single().ok(); // 控件换代期间仍推进reader，但不接受无主变化。
    let context_invalid =
        !editor.open || editor.close_requested || editor.new_scene_dialog_open || session.saving; // 隐藏、模态或保存都是实时水位的异常中断边沿。
    if scrub.target.is_some() && !scrub.aborted && context_invalid {
        abort_water_level_scrub(
            &mut editor,
            &mut session,
            &mut scrub,
            "水位拖动因编辑器上下文变化而取消",
        ); // 关闭过程也不遗留未入历史水位。
    }
    if scrub.aborted && active_scrub.entity != input_entity {
        scrub.clear(); // 物理拖动已结束却丢失Committed时释放取消锁存。
    }
    for event in changed.read() {
        if Some(event.entity) != input_entity {
            continue;
        }
        let Some(water_level) = event.value.as_f32() else {
            continue;
        };
        if scrub.aborted {
            continue; // 冲突回滚后同一物理拖动的后续Changed全部丢弃。
        }
        if !water_level.is_finite()
            || !(MIN_WATER_LEVEL_METERS..=MAX_WATER_LEVEL_METERS).contains(&water_level)
        {
            reject_water_level_request(&mut editor, "水位拖动产生了越界数值，已拒绝本帧变化");
            continue;
        }
        if scrub.target.is_none()
            && let Err(error) = begin_water_level_scrub(&editor, &session, &mut scrub)
        {
            reject_water_level_request(&mut editor, error);
            continue;
        }
        let Some(target) = scrub.target.clone() else {
            continue;
        };
        if session.revision != scrub.expected_revision {
            let reason = format!(
                "水位拖动期望作者修订{}，当前已是{}；实时拖动已停止",
                scrub.expected_revision, session.revision
            ); // 可变借用事务前先冻结诊断数值。
            abort_water_level_scrub(&mut editor, &mut session, &mut scrub, reason);
            continue; // 取消锁存到松手，不会在同次拖动重绑。
        }
        let request = WaterLevelSetRequest {
            target: target.clone(),
            water_level,
        };
        let result = session
            .draft
            .as_mut()
            .ok_or_else(|| "当前场景没有可编辑湖泊/池塘".to_owned())
            .and_then(|draft| {
                if draft.scene_id != target.scene_id {
                    return Err(format!(
                        "水位拖动属于场景{}，当前草稿属于{}；实时拖动已停止",
                        target.scene_id, draft.scene_id
                    ));
                }
                draft
                    .water
                    .as_mut()
                    .ok_or_else(|| "当前场景没有可编辑湖泊/池塘".to_owned())
                    .and_then(|water| set_water_level(water, &request))
            });
        let (label, before, after) = match result {
            Ok(change) => change,
            Err(error) => {
                abort_water_level_scrub(&mut editor, &mut session, &mut scrub, error); // 稳定层失效或异常值只定点回滚。
                continue;
            }
        };
        if before == after {
            continue; // 同一吸附档不推进修订，也不重复提交后台预览。
        }
        session.mark_water_definition_changed(&mut editor); // single-flight只保留最新期望修订且同时最多一个任务。
        scrub.after = after;
        scrub.expected_revision = session.revision; // 下一帧只接受本事务刚推进后的联合身份。
        editor.status = format!("正在拖动水域层“{label}”静止水位：{after:.3}m");
    }

    for event in committed.read() {
        if Some(event.entity) != input_entity || event.source != NumericInputCommitSource::Scrub {
            continue; // 文本输入由离散请求路径处理，其他数字框由自己的业务模块处理。
        }
        let Some(target) = scrub.target.clone() else {
            continue; // 没有真实Changed的左拖不生成空撤销。
        };
        if scrub.aborted {
            scrub.clear(); // 对应物理拖动松手后才释放冲突取消锁存。
            continue;
        }
        if !event.changed {
            let rolled_back = rollback_water_level_scrub_if_owned(&mut session, &scrub); // 回到显示起点时恢复高精度raw before。
            if rolled_back {
                editor.dirty = scrub.dirty_before; // 正常raw no-op不留dirty。
            }
            editor.status = "水位拖动已回到显示起点，没有产生撤销".to_owned(); // 不留历史。
            scrub.clear(); // 正常松手立即结束事务。
            continue;
        }
        if session.revision != scrub.expected_revision {
            abort_water_level_scrub(
                &mut editor,
                &mut session,
                &mut scrub,
                "水位拖动结束前作者修订已变化；未合并为撤销命令",
            );
            scrub.clear();
            continue;
        }
        let label = session
            .draft
            .as_ref()
            .filter(|draft| draft.scene_id == target.scene_id)
            .and_then(|draft| draft.water.as_ref())
            .and_then(|water| {
                water
                    .layers
                    .iter()
                    .find(|layer| layer.id == target.layer_id)
            })
            .map(|layer| (layer.label.clone(), layer.water_level));
        let Some((label, current_value)) = label else {
            abort_water_level_scrub(
                &mut editor,
                &mut session,
                &mut scrub,
                "水位拖动绑定的场景或水层已失效",
            ); // 稳定层丢失不会产生历史。
            scrub.clear();
            continue;
        };
        if current_value != scrub.after {
            abort_water_level_scrub(
                &mut editor,
                &mut session,
                &mut scrub,
                "水位拖动结束时同字段已被外部修改；未覆盖新值",
            ); // 同修订的缺陷路径也用latest所有权门保护。
            scrub.clear();
            continue;
        }
        finish_water_level_scrub(&mut editor, &mut session, &mut scrub, &label);
    }
}
