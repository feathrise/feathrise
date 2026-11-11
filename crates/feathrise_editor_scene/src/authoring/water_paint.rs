//! 静水R8覆盖笔刷，地形高度由独立地形工具维护。
//!
//! 【功能设计】普通绘制与Shift擦除都只修改水域覆盖，不自动下挖或回填地形。

use std::sync::Arc; // 共享R8在后台物化独立写侧并用指针身份拒绝迟到结果。

use bevy::prelude::*; // 输入、时间和ECS资源。
use bevy::tasks::AsyncComputeTaskPool; // 最高16MiB R8复制不得进入输入帧。
use bevy::tasks::futures::check_ready; // Update只非阻塞轮询唯一物化任务。
use bevy::ui::RelativeCursorPosition; // 笔刷只在中央编辑视口内生效。
use feathrise_scene_content::SceneWaterLayer; // 待补刷段继续绑定按下时稳定层规格。

mod bed; // 纯R8/GLB顶点局部算法与可见半径回归测试独立维护。
#[cfg(test)]
pub(crate) use bed::paint_roundtrip_fixture;
mod history; // 撤销检查点与收口转换独立维护，入口文件保持单一输入职责。
mod snapshot; // 整笔Arc版本与共享GLB后台写侧。
mod object_press; // 水位平面起笔冻结同次按压、源画布、水位及输入所有权。
pub(crate) use snapshot::{poll_terrain_materialization, start_terrain_materialization_if_shared}; // 地形笔刷和旧高度历史也必须兼容长期快照共享。

use self::bed::{WaterSegmentOutcome, paint_water_segment}; // 固定访问预算只处理水域覆盖。
use super::paint::{PAINT_RESPONSE_PER_SECOND, hovered}; // 复用统一时间响应和视口命中。
use super::resources::{
    EditorAuthoringSession, PendingWaterBrushSample, WaterBrushStroke, WaterMaskMaterializeTask,
}; // 新水笔划只记录R8；旧联合高度历史继续兼容，整图写侧复制使用独立single-flight任务。
use crate::components::SceneEditorViewport; // 中央视口业务标记。
use crate::resources::SceneEditorState; // 当前工具、笔刷参数与状态提示。
use crate::types::EditorTool; // 河水工具身份。

use self::history::{cancel_water_stroke, checkpoint_water_stroke, finish_water_stroke};

pub(crate) fn poll_mask_materialization(
    session: &mut EditorAuthoringSession,
    materialize: &mut WaterMaskMaterializeTask,
) {
    let Some(task) = materialize.task.as_mut() else {
        return;
    };
    let Some((scene_id, layer_id, revision, source, pixels)) = check_ready(task) else {
        return;
    };
    materialize.task = None;
    if session.water_revision != revision {
        return; // 任务期间其他作者操作已换代，迟到副本不能覆盖新画布。
    }
    let Some(mask) = session
        .draft
        .as_mut()
        .filter(|draft| draft.scene_id == scene_id)
        .and_then(|draft| draft.water_masks.get_mut(&layer_id))
    else {
        return;
    };
    if Arc::ptr_eq(mask, &source) {
        *mask = Arc::new(pixels); // 完整Vec一次采用；输入帧没有逐字节复制。
    }
}

pub(crate) fn start_mask_materialization_if_shared(
    session: &EditorAuthoringSession,
    layer_id: &str,
    materialize: &mut WaterMaskMaterializeTask,
) -> Result<bool, String> {
    if materialize.task.is_some() {
        return Ok(true); // 全局唯一复制槽仍在工作，当前笔划保持原落点等待。
    }
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "场景作者数据尚未就绪".to_owned())?;
    let mask = draft
        .water_masks
        .get(layer_id)
        .ok_or_else(|| format!("静水体{layer_id}缺少R8作者源"))?;
    if Arc::strong_count(mask) <= 1 {
        return Ok(false);
    }
    let scene_id = draft.scene_id.clone();
    let layer_id = layer_id.to_owned();
    let revision = session.water_revision;
    let source = mask.clone();
    materialize.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let pixels = source.as_ref().clone(); // 整图复制在线程池执行，完成前当前Arc保持权威。
        (scene_id, layer_id, revision, source, pixels)
    }));
    Ok(true)
}

enum WaterSampleProgress {
    Complete,                         // 整段完成，调用方才可推进落点或收口。
    Pending(PendingWaterBrushSample), // 分帧游标或撤销检查点已经推进，下一帧原位继续。
    Waiting,                          // R8仍被后台读取，冻结段保持不变。
}

fn pending_origin_is_current(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    stroke: &WaterBrushStroke,
) -> bool {
    stroke.scene_id.as_ref().is_some_and(|scene_id| {
        editor.open
            && !session.saving
            && super::water_canvas::writable(session, editor)
            && editor.selected_scene_id.as_ref() == Some(scene_id)
            && session
                .draft
                .as_ref()
                .is_some_and(|draft| &draft.scene_id == scene_id)
    })
}

fn should_sample_water_button(pressed: bool, just_pressed: bool) -> bool {
    pressed || just_pressed // Bevy允许同一Update观察到press+release，此时pressed已经为false。
}

fn combine_brush_amount(accumulated: f32, current: f32) -> f32 {
    let accumulated = accumulated.clamp(0.0, 1.0);
    let current = current.clamp(0.0, 1.0);
    1.0 - (1.0 - accumulated) * (1.0 - current) // 多帧等待仍按渐近混合累计，不随帧数线性溢出。
}

fn queue_water_tail(
    stroke: &mut WaterBrushStroke,
    pending: PendingWaterBrushSample,
    from: Vec3,
    to: Vec3,
    amount: f32,
) {
    if let Some(queued) = stroke.queued_sample.as_mut() {
        queued.to = to; // 有界队列只保留最新端点，起点仍是当前pending的冻结终点。
        queued.amount = combine_brush_amount(queued.amount, amount);
        return;
    }
    stroke.queued_sample = Some(PendingWaterBrushSample {
        from,
        to,
        radius: pending.radius, // backlog期间冻结当前笔刷几何与Shift语义，不能重解释旧轨迹。
        erase: pending.erase,
        amount: amount.clamp(0.0, 1.0),
        cursor: 0,
        mask_changed: false,
        height_changed: false,
    });
}

fn apply_frozen_water_sample(
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    stroke: &mut WaterBrushStroke,
    materialize: &mut WaterMaskMaterializeTask,
    layer: &SceneWaterLayer,
    mut sample: PendingWaterBrushSample,
) -> Result<WaterSampleProgress, String> {
    snapshot::capture_stroke_snapshot(session, stroke, true); // 绘制和擦除均只冻结R8；不保留或复制地形快照。
    if start_mask_materialization_if_shared(session, &layer.id, materialize)? {
        editor.status = format!(
            "正在后台准备水域层“{}”的独立R8写侧；冻结落点会在完成后继续",
            layer.label
        );
        return Ok(WaterSampleProgress::Waiting);
    }
    let water_level = stroke
        // 长期历史Arc不能用等待读者释放代替后台写侧准备。
        .water_level
        .ok_or_else(|| "河水笔划缺少按下时冻结的水位".to_owned())?;
    let outcome = session
        .draft
        .as_mut()
        .ok_or_else(|| "场景作者数据尚未就绪".to_owned())
        .and_then(|draft| {
            paint_water_segment(
                draft,
                layer,
                sample.from,
                sample.to,
                sample.radius,
                water_level,
                sample.erase,
                sample.amount,
                sample.cursor,
                stroke,
            )
        })?;
    let WaterSegmentOutcome::Progressed {
        changed,
        next_cursor,
        complete,
        checkpoint_required,
    } = outcome
    else {
        editor.status = "水域作者源仍由上一后台预览读取；冻结落点会在释放后自动补画".to_owned();
        return Ok(WaterSampleProgress::Waiting);
    };
    sample.cursor = next_cursor;
    sample.mask_changed |= changed.mask_changed;
    sample.height_changed |= changed.height_changed;
    if checkpoint_required {
        checkpoint_water_stroke(session, stroke);
        editor.status = format!("正在分块处理水域层“{}”；松开后整笔可一次撤销", layer.label);
        return Ok(WaterSampleProgress::Pending(sample));
    }
    if !complete {
        editor.status = format!(
            "正在分帧处理水域层“{}”的高密度笔划；松开左键后也会自动完成",
            layer.label
        );
        return Ok(WaterSampleProgress::Pending(sample));
    }
    stroke.segment_start = Some(sample.from); // 即时地表覆盖只刷新本次真正受理的完整刷段。
    stroke.last_world = Some(sample.to); // 只有完整受理线段才推进连续采样端点。
    if sample.mask_changed || sample.height_changed {
        session.mark_water_stroke_changed(editor, &layer.id, sample.height_changed);
        editor.status = if sample.erase {
            format!("正在擦除水域层“{}”；河床保持不变", layer.label)
        } else {
            format!("正在绘制水域层“{}”；地形保持不变", layer.label,)
        };
    }
    Ok(WaterSampleProgress::Complete)
}

/// 左键绘制当前R8水域，Shift+左键擦除；两者都不修改GLB顶点。
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_water_brush(
    mouse: Res<ButtonInput<MouseButton>>, // 左键形成连续复合笔划。
    keyboard: Res<ButtonInput<KeyCode>>,  // Shift切换为只擦R8。
    time: Res<Time<Real>>,                // 连续按住按真实时间向固定目标收敛。
    viewport: Query<&RelativeCursorPosition, With<SceneEditorViewport>>, // 拒绝视口外点透绘制。
    mut editor: ResMut<SceneEditorState>,   // 读取工具参数并发布状态与dirty。
    mut session: ResMut<EditorAuthoringSession>, // 只修改当前根水或私有水画布的R8覆盖。
    mut stroke: ResMut<WaterBrushStroke>, // 保存整笔稳定层、水位与首次值。
    mut materialize: ResMut<WaterMaskMaterializeTask>, // 共享大R8只在后台复制一次。
    plane: Option<Res<crate::viewport::water_plane::WaterPlaneCursor>>, // 只读当前水位平面，不请求地形网格或Mip0。
    mut object_press: Local<Option<object_press::ObjectWaterPress>>,
    mut object_active: Local<Option<object_press::ObjectWaterPress>>,
    mut accepted_segment: Local<u64>,
) {
    poll_mask_materialization(&mut session, &mut materialize);
    poll_terrain_materialization(&mut session, &mut materialize); // 完整GLB写侧只在身份仍有效时采用。
    if !stroke.active { *object_active = None; }
    if stroke.active && object_active.as_ref().is_none_or(|press|
        !object_press::origin_is_current(press, &session)) {
        cancel_water_stroke(&mut stroke);
        *object_press = None;
        *object_active = None;
        return; // 先拒绝换源，再处理锁定或隐藏；不能用新画布同名层为旧笔划生成after_mask。
    }
    if !super::water_canvas::writable(&session, &editor) {
        stroke.pending_sample = None;
        stroke.queued_sample = None;
        stroke.pending_ground = None;
        if stroke.active
            && session
                .draft
                .as_ref()
                .is_some_and(|draft| stroke.scene_id.as_ref() == Some(&draft.scene_id))
        {
            finish_water_stroke(&mut editor, &mut session, &mut stroke);
        } else {
            cancel_water_stroke(&mut stroke);
        }
        return; // 对象锁定、隐藏或目标画布换代后，旧冻结段不得恢复写入。
    }
    let selecting_water =
        keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight); // Ctrl+左键由中央视口吸取完整水域对象，绝不能同时写R8或地形顶点。
    let active_context = editor.open && !editor.close_requested && editor.tool == EditorTool::Water
        && !selecting_water && !session.saving && editor.water_level_from_cursor_requested.is_none();
    // 子场景只读门和模式浮层能在Cursor后半程撤回世界点；不能绕过它直接消费平面缓存。
    let paint_point = plane.as_deref().filter(|plane| plane.available
        && plane.world.is_some() && plane.world == editor.brush_cursor_world).and_then(|plane| plane.point);
    let segment = plane.as_deref().map_or(0, |plane| plane.segment);
    object_press::update(&mut object_press,
        active_context && !stroke.active && !editor.save_requested && paint_point.is_some()
            && !editor.undo_requested && !editor.redo_requested && hovered(&viewport),
        &editor, &session, plane.as_deref(), &mouse);
    let press_current = object_active.as_ref().is_some_and(|press|
        object_press::active_is_current(press, &editor, &session, plane.as_deref()));
    if stroke.active && ((!mouse.pressed(MouseButton::Left) && !mouse.just_pressed(MouseButton::Left))
        || editor.save_requested || editor.undo_requested || editor.redo_requested
        || !active_context || !press_current) {
        stroke.release_requested = true; // 已受理段仍排空，失权后的新完成点不能入队。
    }
    if mouse.just_released(MouseButton::Left) && stroke.active {
        stroke.release_requested = true; // 即使作者源仍共享，松键意图也随冻结段保留。
    }
    if let Some(sample) = stroke.pending_sample {
        if !pending_origin_is_current(&editor, &session, &stroke) {
            cancel_water_stroke(&mut stroke);
            editor.status =
                "待补水笔刷段所属场景或作者生命周期已经变化；已丢弃旧段以避免跨场景写入".to_owned();
            return;
        }
        let pending_layer = stroke.layer_id.as_deref().and_then(|layer_id| {
            session
                .draft
                .as_ref()
                .and_then(|draft| draft.water.as_ref())
                .and_then(|water| water.layers.iter().find(|layer| layer.id == layer_id))
                .cloned()
        });
        let Some(layer) = pending_layer else {
            stroke.pending_sample = None;
            stroke.queued_sample = None;
            editor.status = "冻结水笔刷段所属稳定层已经失效；本笔已安全停止".to_owned();
            finish_water_stroke(&mut editor, &mut session, &mut stroke);
            return;
        };
        if (mouse.pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left))
            && !selecting_water
            && editor.tool == EditorTool::Water
            && !editor.close_requested
            && hovered(&viewport)
            && press_current && (!stroke.release_requested || mouse.just_released(MouseButton::Left))
            && let Some(world) = paint_point
        {
            let amount = editor.brush_strength
                * time.delta_secs().clamp(0.0, 0.05)
                * PAINT_RESPONSE_PER_SECOND;
            let from = if *accepted_segment == segment { sample.to } else { world };
            if *accepted_segment == segment || stroke.queued_sample.is_none() {
                queue_water_tail(&mut stroke, sample, from, world, amount);
                *accepted_segment = segment;
            } // 离开后重新进入不跨空窗补线；已有尾段先排空，继续保持一个有界尾段。
        }
        let progress = apply_frozen_water_sample(
            &mut editor,
            &mut session,
            &mut stroke,
            &mut materialize,
            &layer,
            sample,
        );
        object_press::record_revision(&mut object_active, &session);
        match progress {
            Ok(WaterSampleProgress::Waiting) => return, // 保留完整段与松键意图，不形成空笔。
            Ok(WaterSampleProgress::Pending(sample)) => {
                stroke.pending_sample = Some(sample); // 游标已经前进，但端点仍不能提前采用。
                return;
            }
            Ok(WaterSampleProgress::Complete) => {
                stroke.pending_sample = None;
                if let Some(queued) = stroke.queued_sample.take() {
                    stroke.pending_sample = Some(queued); // 先补齐等待期间的尾段，再响应松键收口。
                } else if stroke.release_requested || !mouse.pressed(MouseButton::Left) {
                    finish_water_stroke(&mut editor, &mut session, &mut stroke);
                }
                return; // 每帧最多补做一段，不能让恢复帧突破固定网格访问预算。
            }
            Err(error) => {
                stroke.pending_sample = None; // 确定坏输入不在稳定帧无限重试。
                stroke.queued_sample = None; // 同一冻结语义的尾段也不能绕过坏输入继续写入。
                editor.status = format!("冻结河水笔划失败：{error}");
                if stroke.release_requested || !mouse.pressed(MouseButton::Left) {
                    finish_water_stroke(&mut editor, &mut session, &mut stroke);
                }
                return;
            }
        }
    }
    if stroke.active && stroke.release_requested {
        finish_water_stroke(&mut editor, &mut session, &mut stroke);
        return;
    }
    let current_scene = session.draft.as_ref().map(|draft| draft.scene_id.clone());
    if stroke.ground_scene.is_some() && stroke.ground_scene != current_scene {
        *stroke = WaterBrushStroke::default(); // 同名水层也不能跨场景接续旧落点。
    }
    if !active_context {
        stroke.pending_ground = None; // 清理旧版地面等待状态，普通静水不再排队请求地形。
        if stroke.active {
            finish_water_stroke(&mut editor, &mut session, &mut stroke); // 切工具、关闭或开始保存时先形成完整命令。
        }
        return; // 非活动上下文不读取光标或作者数组。
    }
    let layer = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water.as_ref())
        .and_then(|water| water.layers.get(editor.selected_water_layer))
        .cloned(); // 冻结稳定层身份，后续刷段只按当前水位平面和独立范围采样。
    let Some(layer) = layer else {
        stroke.pending_ground = None;
        return; // 当前场景没有有效所选水层时保持作者源原样。
    };
    let Some(world) = paint_point else {
        // 离图只切断采样线段，保持整次按压的撤销身份。
        stroke.last_world = None; // 圆完全离图时切断线段，重新进入不得横穿场景补水。
        stroke.segment_start = None; // 覆盖也不能用旧起点跨界面空窗补出长条。
        if mouse.just_released(MouseButton::Left) && stroke.active {
            finish_water_stroke(&mut editor, &mut session, &mut stroke); // 场景外松手仍完整收口本次R8历史。
        }
        return; // 笔刷圆与场景已无交集时不开始或延伸笔划。
    };
    if !hovered(&viewport) {
        // 离开视口暂停采样，不把按压拆成多笔。
        stroke.last_world = None; // 返回视口后从当前点继续，不能横穿界面补线。
        if mouse.just_released(MouseButton::Left) {
            finish_water_stroke(&mut editor, &mut session, &mut stroke);
        } // 视口外松手仍结束同一笔。
        return; // 不穿透侧栏绘制。
    }
    let starting = !stroke.active && object_press.is_some()
        && should_sample_water_button(mouse.pressed(MouseButton::Left), mouse.just_pressed(MouseButton::Left));
    if starting {
        *object_active = object_press.take();
        let Some(scene_id) = session.draft.as_ref().map(|draft| draft.scene_id.clone()) else {
            return;
        };
        stroke.active = true; // 从本帧开始形成一条复合撤销命令。
        stroke.pending_ground = None; // 普通水位平面起笔不继承旧版地形等待意图。
        stroke.ground_scene = Some(scene_id.clone());
        *accepted_segment = segment;
        stroke.snapshot_mask = None; // 独立第二笔不能复用上一笔最初版本。
        stroke.snapshot_terrain = None; // 新刷水笔划不修改地形，也不保留地形快照。
        stroke.snapshot_changed = false; // 新笔重新累计变化。
        stroke.snapshot_height_changed = false; // 擦除不继承旧地形变化。
        stroke.scene_id = Some(scene_id); // pending段只允许回写这张草稿。
        stroke.layer_id = Some(layer.id.clone()); // 整笔稳定绑定按下时的水层身份。
        stroke.water_level = Some(layer.water_level); // 整笔预览稳定绑定按下时水位。
        stroke.mask_before.clear(); // 新笔重新记录R8首次值。
        stroke.height_before.clear(); // 新笔不继承旧联合命令中的GLB顶点差分。
        stroke.last_world = Some(world); // 第一帧从实际落点开始，不连接上一笔。
        stroke.pending_sample = None; // 新笔没有继承任何旧共享等待段。
        stroke.queued_sample = None; // 新笔没有继承旧pending期间的尾端轨迹。
        stroke.release_requested = mouse.just_released(MouseButton::Left); // 同一输入帧press+release也必须在补画后收口。
    }
    if should_sample_water_button(
        mouse.pressed(MouseButton::Left),
        mouse.just_pressed(MouseButton::Left),
    ) && stroke.active
        && stroke.layer_id.as_deref() == Some(layer.id.as_str())
    {
        let erase = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight); // Shift只擦R8。
        let amount = if starting {
            editor.brush_strength // 单击立即产生明确的水域覆盖变化。
        } else {
            editor.brush_strength * time.delta_secs().clamp(0.0, 0.05) * PAINT_RESPONSE_PER_SECOND // 按真实时间向固定目标收敛。
        };
        let previous = stroke.last_world.unwrap_or(world);
        let from = if *accepted_segment == segment { previous } else { world };
        *accepted_segment = segment;
        let sample = PendingWaterBrushSample {
            from,
            to: world,
            radius: editor.brush_radius,
            erase,
            amount,
            cursor: 0,
            mask_changed: false,
            height_changed: false,
        };
        stroke.segment_start = Some(from); // 同帧完成时覆盖与R8写入使用同一段。
        stroke.pending_sample = Some(sample); // 先冻结再尝试，任何共享等待或同帧松键都不会丢段。
        let progress = apply_frozen_water_sample(
            &mut editor,
            &mut session,
            &mut stroke,
            &mut materialize,
            &layer,
            sample,
        );
        object_press::record_revision(&mut object_active, &session);
        match progress {
            Ok(WaterSampleProgress::Complete) => stroke.pending_sample = None,
            Ok(WaterSampleProgress::Pending(sample)) => {
                stroke.pending_sample = Some(sample);
                return;
            }
            Ok(WaterSampleProgress::Waiting) => return,
            Err(error) => {
                stroke.pending_sample = None;
                editor.status = format!("河水笔划失败：{error}");
            }
        }
    }
    if mouse.just_released(MouseButton::Left) {
        if stroke.pending_sample.is_none() {
            finish_water_stroke(&mut editor, &mut session, &mut stroke); // 无待补段时立即形成完整R8命令。
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod object_tests;
