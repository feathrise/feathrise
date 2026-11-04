//! 植被作者R8局部笔刷、写时复制与撤销差分。

use std::sync::Arc; // 后台编译共享旧修订，当前作者层首次落笔才写时复制。

use bevy::prelude::*; // 输入、真实时间、ECS资源和世界坐标。
use bevy::ui::RelativeCursorPosition; // 左键只在中央编辑视口内绘制。

use super::paint::{
    byte_changes, hovered, interpolated_dab_count, paint_density_neighborhood_with_flow,
    vegetation_paint_exposure,
}; // 地表、植被和河水共享相同米制连续笔刷核心。
use super::resources::{EditorAuthoringSession, EditorCommand, VegetationBrushStroke}; // 作者画布修订、局部撤销与稳定笔划身份。
use crate::components::MapEditorViewport; // 视口外按键不得写作者数据。
use crate::resources::MapEditorState; // 当前工具、笔刷参数与可见状态。
use crate::types::EditorTool; // 只有植被工具消费当前作者层。

mod object_press;
use object_press::ObjectVegetationPress;
mod path;
use path::{Sample, VegetationPaintPath};

#[cfg(test)]
#[path = "vegetation_paint/object_tests.rs"]
pub(crate) mod object_tests;

/// 植被笔刷只修改当前独立作者R8；正式运行时层由后台编译结果原子更新。
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_vegetation_brush(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    viewport: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut stroke: ResMut<VegetationBrushStroke>,
    surface: Option<feathrise_scene::TerrainObjectClipmapGround>, // 跨采样插值也不得越过缺页区域。
    ground: Option<Res<crate::viewport::vegetation_ground::VegetationGroundCursor>>, // 独立原笔刷测试不依赖场景查询资源。
    objects_cursor: Option<Res<super::terrain_ground::TerrainGroundCursor>>,
    mut object_press: Local<Option<ObjectVegetationPress>>, // 异步起笔只保留本次按压的固定身份。
    mut path: Local<VegetationPaintPath>,
    scope: Option<Res<super::vegetation_paint_scope::VegetationPaintSettings>>,
) {
    let closing = editor.save_requested || editor.close_requested || editor.undo_requested || editor.redo_requested;
    let draining = stroke.active && path.pending.is_some() && (closing || !mouse.pressed(MouseButton::Left));
    let selecting_layer =
        keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight); // Ctrl+左键由视口吸取整层，绝不能同时写入一枚R8纹素。
    let volume_context = ground
        .as_ref()
        .is_some_and(|g| g.volume_paint_ray().is_some());
    let brush_context = editor.open
        && scope.as_ref().is_none_or(|scope| scope.scope==super::vegetation_paint_scope::PaintScope::Current)
        && editor.tool == EditorTool::Vegetation
        && super::terrain_documents::canvas_writable(&session, &editor)
        && (draining || !selecting_layer)
        && (draining || ground.as_ref().is_none_or(|ground| ground.source_authoring_available()))
        && !session.saving
        && (draining || hovered(&viewport)); // 视口外不受理新点；已受理冻结段正常松手/保存仍应排空。
    let active_context = brush_context && !volume_context;
    let clipmap = session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.uses_legacy_clipmap_cursor());
    let objects = session.draft.as_ref()
        .is_some_and(|draft| draft.manifest.terrains.is_some());
    let map_id = session.draft.as_ref().map(|draft| draft.map_id.clone()); // 换图不可比较同名画布。
    if stroke.ground_map.is_some() && stroke.ground_map != map_id {
        *stroke = VegetationBrushStroke::default();
    } // 丢弃旧图笔划身份。
    let selected_id = session
        .draft
        .as_ref()
        .and_then(|draft| draft.vegetation.as_ref())
        .and_then(|v| v.authoring.as_ref())
        .and_then(|a| a.layers.get(editor.selected_vegetation_layer))
        .filter(|layer| layer.enabled && !layer.locked)
        .map(|layer| layer.id.clone()); // 等待只接受有效作者层。
    let identity = map_id.clone().zip(selected_id); // 稳定地图与层共同标识按压。
    let erase = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if !stroke.active {
        *path = VegetationPaintPath::default();
    } else if !path.current(&session) || (objects && path.press.as_ref().is_none_or(|press|
        !object_press::author_is_current(press, &editor, &session)
            || (!draining && mouse.pressed(MouseButton::Left) && !object_press::active_is_current(
                press, &editor, &session, objects_cursor.as_deref())))) {
        // 已受理笔划与同一次按压绑定，焦点/画布/相机换代后不能恢复旧按住继续写。
        finish_vegetation_stroke(&mut editor, &mut session, &mut stroke);
        *path = VegetationPaintPath::default();
        return;
    }
    object_press::update(
        &mut object_press,
        objects && active_context && !stroke.active && !editor.save_requested && !editor.close_requested
            && ground.as_ref().is_none_or(|ground| ground.source_authoring_available()),
        &mut editor, &session, objects_cursor.as_deref(), &mouse, erase,
    ); // 显式网格按下先撤销旧悬停；等待同次按压的新命中，不能丢掉起笔边沿。
    if !active_context {
        stroke.pending_ground = None;
        if stroke.active {
            finish_vegetation_stroke(&mut editor, &mut session, &mut stroke);
        }
        // 不能在三维面往室外XZ写密度；保留同次按压，回到外部真实地表后接续独立R8笔划。
        stroke.volume_handoff =
            if brush_context && volume_context && mouse.pressed(MouseButton::Left) {
                identity
            } else {
                None
            };
        return;
    }
    if clipmap
        && (mouse.just_pressed(MouseButton::Left)
            || (identity.is_some()
                && stroke.volume_handoff == identity
                && mouse.pressed(MouseButton::Left)))
    {
        stroke.pending_ground = identity.clone();
        stroke.volume_handoff = None;
    } // press时无命中也保留当前按压意图。
    if !mouse.pressed(MouseButton::Left) || stroke.pending_ground != identity {
        stroke.pending_ground = None;
        stroke.volume_handoff = None;
    } // 松手或换层立即失效，绝不补画。
    if (clipmap || objects)
        && stroke.active
        && stroke.layer_id.as_ref().is_some_and(|layer| {
            identity
                .as_ref()
                .is_none_or(|(_, selected)| layer != selected)
        })
    {
        finish_vegetation_stroke(&mut editor, &mut session, &mut stroke);
    } // 换层先完成原层历史。
    // 连续画笔消费原射线的真实完成点；它可以略落后于正在移动的鼠标，不能借严格悬停空窗丢掉轨迹。
    let object_hit = objects_cursor.as_deref().and_then(|cursor| cursor.paint_hit());
    let paint_point = if objects {
        object_hit.and_then(|hit| super::terrain_documents::paint_point_on_terrain(
            &session, &editor, Some(&hit.terrain), Some(hit.point),
        ))
    } else {
        let point = editor.brush_cursor_world
            .filter(|_| !clipmap || ground.as_ref().is_some_and(|ground| ground.paint_ready));
        super::terrain_documents::paint_point(&session, &editor, point)
    };
    let waiting = (objects && objects_cursor.as_ref().is_some_and(|cursor| cursor.paint_pending()))
        || (clipmap && ground.as_ref().is_some_and(|ground| ground.pending_same_context));
    if let Some(sample) = path.pending {
        // 松手后排空已经受理的有界前缀；新命中不得在松手后进入队列。
        path.release |= !mouse.pressed(MouseButton::Left) || closing;
        if let Some(world) = paint_point.filter(|_| !path.release) {
            let previous = path.tail.back().map_or(sample.to, |tail| tail.to);
            let connected = if objects {
                object_hit.and_then(|hit| super::terrain_documents::paint_point_on_terrain(
                    &session, &editor, Some(&hit.terrain), hit.from,
                )) == Some(previous)
            } else {
                !clipmap || surface.as_ref().and_then(|surface| surface.snapshot.as_ref())
                    .is_some_and(|snapshot| snapshot.segment_is_known(previous, world))
            };
            let (amount, mut exposure) = vegetation_paint_exposure(editor.brush_strength, time.delta_secs(), false);
            exposure += std::mem::take(&mut stroke.waiting_exposure);
            let from = if connected && !path.disconnected { previous } else { world };
            path.queue(Sample { from, to: world, radius: editor.brush_radius, erase,
                amount, exposure });
        } else if waiting && !path.release {
            stroke.waiting_exposure += vegetation_paint_exposure(
                editor.brush_strength, time.delta_secs(), false,
            ).1;
        } else if !waiting {
            path.disconnected = true;
        }
        let layer = session.draft.as_ref().and_then(|draft| draft.vegetation.as_ref())
            .and_then(|vegetation| vegetation.authoring.as_ref())
            .and_then(|authoring| authoring.layers.iter().find(|layer| Some(&layer.id) == stroke.layer_id.as_ref()))
            .cloned();
        if let Some(layer) = layer {
            path.pending = path::apply(sample, &layer, &mut session, &mut editor, &mut stroke)
                .or_else(|| path.tail.pop_front());
            path.record_revision(&session);
        } else { path.pending = None; path.tail.clear(); path.release = true; }
        if path.pending.is_none() && path.release {
            finish_vegetation_stroke(&mut editor, &mut session, &mut stroke);
        }
        return;
    }
    let Some(world) = paint_point else {
        if waiting && mouse.pressed(MouseButton::Left) {
            if stroke.active {
                stroke.waiting_exposure += vegetation_paint_exposure(
                    editor.brush_strength, time.delta_secs(), false,
                ).1;
            }
            stroke.segment_start = None;
            return;
        } // 同代CPU延迟仅暂停写入，下一真实完成点仍连接上次采样。
        stroke.last_world = None; // 圆完全离图时切断快速拖动插值，重新进入只从新点开始。
        stroke.waiting_exposure = 0.0; // 真实未命中或失去作者权限不能将空白时长补到下一落点。
        stroke.segment_start = None; // 即时覆盖也不得用旧点跨图刷新一条伪刷段。
        if mouse.just_released(MouseButton::Left) && stroke.active {
            finish_vegetation_stroke(&mut editor, &mut session, &mut stroke); // 地图外松手仍完整提交撤销差分。
        }
        return; // 笔刷圆与地图已无交集时不猜测二维落点。
    };
    let layer = session
        .draft
        .as_ref()
        .and_then(|draft| draft.vegetation.as_ref())
        .and_then(|vegetation| vegetation.authoring.as_ref())
        .and_then(|authoring| authoring.layers.get(editor.selected_vegetation_layer))
        .cloned(); // 小型描述克隆解除后续草稿可变借用冲突。
    let Some(layer) = layer else {
        return; // 当前地图没有可绘制作者层。
    };
    if layer.locked {
        if mouse.just_pressed(MouseButton::Left) {
            editor.status = format!("作者植被图层“{}”已锁定，未执行笔刷", layer.label); // 锁定不改变最终编译可见性。
        }
        return;
    }
    if !layer.enabled {
        if mouse.just_pressed(MouseButton::Left) {
            editor.status = format!("作者植被图层“{}”已隐藏，请先启用后绘制", layer.label); // 隐藏层保留R8但不参与编译。
        }
        return;
    }
    let starting = if objects {
        !stroke.active && object_press.is_some() && mouse.pressed(MouseButton::Left)
    } else {
        mouse.just_pressed(MouseButton::Left)
            || (clipmap && !stroke.active && stroke.pending_ground.is_some()
                && mouse.pressed(MouseButton::Left))
    }; // 按住等待后首次真实命中只开始一次；取消后的按压不得复活。
    if starting {
        path.press = object_press.clone();
        *object_press = None;
        stroke.pending_ground = None; // 起笔消费意图，不能重复清空撤销差分。
        stroke.ground_map = (clipmap || objects).then_some(map_id.clone()).flatten(); // 多网格笔划同样不得跨地图复用。
        stroke.active = true; // 开始一个稳定作者层笔划。
        stroke.layer_id = Some(layer.id.clone()); // 切层时不会污染当前撤销记录。
        stroke.before.clear(); // 新笔划重新记录每个纹素首次值。
        stroke.last_world = Some(world); // 单击也拥有首个连续采样点。
        stroke.waiting_exposure = 0.0;
        stroke.segment_start = Some(world); // 即时覆盖使用同一刷段身份。
        path.record_revision(&session);
    }
    if mouse.pressed(MouseButton::Left)
        && stroke.active
        && stroke.layer_id.as_deref() == Some(layer.id.as_str())
    {
        let (amount, mut flow_exposure) =
            vegetation_paint_exposure(editor.brush_strength, time.delta_secs(), starting);
        flow_exposure += std::mem::take(&mut stroke.waiting_exposure);
        let previous = stroke.last_world.unwrap_or(world); // 先保留已采用上一采样。
        let from = if objects {
            object_hit.and_then(|hit| super::terrain_documents::paint_point_on_terrain(
                &session, &editor, Some(&hit.terrain), hit.from,
            )).filter(|from| *from == previous).unwrap_or(world)
        } else if clipmap
            && surface
                .as_ref()
                .and_then(|surface| surface.snapshot.as_ref())
                .is_some_and(|snapshot| !snapshot.segment_is_known(previous, world))
        {
            world
        } else {
            previous
        }; // 有界缺页矩形检测失败即断开，绝不跨空洞补画。 // 首帧退化为同一点。
        stroke.segment_start = Some(from); // 覆盖系统可回显完整相邻刷段。
        path.pending = path::apply(Sample { from, to: world, radius: editor.brush_radius,
            erase, amount, exposure: flow_exposure }, &layer, &mut session, &mut editor, &mut stroke);
        path.record_revision(&session);
    }
    if !mouse.pressed(MouseButton::Left) && path.pending.is_none() {
        finish_vegetation_stroke(&mut editor, &mut session, &mut stroke); // 松手形成一个完整撤销命令。
    }
}

fn finish_vegetation_stroke(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    stroke: &mut VegetationBrushStroke,
) {
    if !stroke.active {
        return; // 没有活动笔划时保持撤销栈不变。
    }
    stroke.active = false; // 当前按压事务结束。
    stroke.pending_ground = None;
    stroke.ground_map = None; // 结束即撤销迟到起笔资格。
    stroke.last_world = None; // 下一笔不连接上一笔终点。
    stroke.waiting_exposure = 0.0;
    stroke.segment_start = None; // 即时刷段身份失效。
    let layer_id = stroke.layer_id.take(); // 冻结本次作者层身份。
    let changes = layer_id
        .as_ref()
        .and_then(|layer_id| {
            session
                .draft
                .as_ref()
                .and_then(|draft| draft.vegetation_author_densities.get(layer_id))
        })
        .map(|density| byte_changes(&stroke.before, density.as_slice())) // 只保留真实前后差异。
        .unwrap_or_default(); // 换图导致画布消失时安全丢弃未完成撤销。
    stroke.before.clear(); // 释放局部首次值集合。
    if let Some(layer_id) = layer_id
        && !changes.is_empty()
    {
        session
            .undo
            .push(EditorCommand::VegetationChanged { layer_id, changes }); // 一次笔划对应一次Ctrl+Z。
        session.redo.clear(); // 新作者修改使旧重做分支失效。
        editor.status = "植被作者笔划完成；后台正在编译最终密度并刷新正式四级LOD预览".to_owned(); // 明确预览采用链路。
    }
}
