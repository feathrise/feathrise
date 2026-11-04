//! 积水笔刷分块后台计算，松手形成完整历史；GPU图像只发布完整且同地图的版本。
use super::super::{EditorAuthoringSession, EditorCommand}; // 统一线性历史和内存预算。
use super::types::{PaintResult, Sample}; // 无ECS借用的任务输入输出。
use super::{PuddleAuthoringState, PuddleSnapshot, apply_snapshot}; // 唯一版本采用入口。
use crate::viewport::vegetation_ground::{GroundCursorTarget, VegetationGroundCursor}; // 独立积水只受理共享精确光标的当前完成点。
use crate::{
    components::MapEditorViewport,
    resources::MapEditorState,
    types::{EditorTool, EditorView},
};
use bevy::prelude::*; // ECS、输入和实时计时。
use bevy::tasks::{AsyncComputeTaskPool, futures::check_ready}; // 单在途任务非阻塞轮询。
use bevy::ui::RelativeCursorPosition; // 只受理中央视口输入。
use feathrise_map_content::{MapPuddleDescriptor, MapPuddleLiveOverride}; // 公共作者协议与渲染覆盖。
use std::sync::Arc; // 笔刷复用局部算法，完整版本共享。 // 当前作者工具和视口。

pub(crate) fn apply_puddle_brush(
    // 输入帧不复制或扫描完整R8。
    mouse: Res<ButtonInput<MouseButton>>, // 当前鼠标按压。
    keys: Res<ButtonInput<KeyCode>>,      // Shift擦除、Ctrl选择。
    time: Res<Time<Real>>,                // 编辑器笔刷不随世界暂停冻结。
    viewport: Query<&RelativeCursorPosition, With<MapEditorViewport>>, // 防止侧栏点击穿透。
    mut editor: ResMut<MapEditorState>,   // 公共笔刷半径及强度。
    mut session: ResMut<EditorAuthoringSession>, // 当前完整草稿与撤销栈。
    mut state: ResMut<PuddleAuthoringState>, // 单在途操作和尾段。
    scrub: Res<crate::resources::MapEditorNumericScrubMouseGuard>, // 数值拖动不能绘制地图。
    browser: Res<crate::resources::MapBrowserState>, // 文件夹模态不允许地图落笔。
    drag: Res<crate::resources::MapBrowserDragState>, // 地图拖动独占当前鼠标。
    conflict: Res<crate::authoring::TerrainExternalChangeState>, // Blender冲突对话框期间只轮询后台。
    surface: Option<feathrise_scene::TerrainObjectClipmapGround>, // 新采样和待补任务绑定同代作者高度。
    ground: Option<Res<VegetationGroundCursor>>, // GLB独立作者测试仍可不安装Clipmap场景。
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>, // 浮层按下和测绘观察视图不武装延迟起笔。
) {
    let writable = super::super::terrain_documents::canvas_writable(&session, &editor);
    if !writable {
        state.pending = None;
        state.pending_ground = None;
        state.released = true;
        // 锁定后未采用的后台块继续非阻塞回收；已采用部分仍按原画布收口历史。
    }
    let clipmap = session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.uses_legacy_clipmap_cursor());
    let map_id = session.draft.as_ref().map(|draft| draft.map_id.clone());
    let snapshot = surface
        .as_ref()
        .and_then(|surface| surface.snapshot.as_ref());
    let ground_content = snapshot
        .filter(|snapshot| {
            Some(&snapshot.map_id) == map_id.as_ref()
                && snapshot.authoring_height_revision() == Some(session.terrain_revision)
        })
        .map(|snapshot| snapshot.content_revision_words());
    if state.before.is_some() {
        if !session.draft.as_ref().is_some_and(|draft| {
            state
                .current
                .as_ref()
                .is_some_and(|current| current.is_current(draft))
                && state.map_id.as_ref() == Some(&draft.map_id)
        }) {
            state.before = None;
            state.current = None;
            state.changed = false; // 外部重载/换图的新像素不能用旧before构造混合撤销。
            state.pending = None;
            state.released = true;
            state.last = None;
        } else if state.expected_revision != session.revision
            || (clipmap && ground_content.is_some() && state.ground_content != ground_content)
        {
            state.pending = None;
            state.released = true;
            state.last = None; // 已采用部分仍可收口；旧高度尾段和迟到CPU结果停止写入。
        }
    }
    // 首先收取已经完成的后台块。
    if state.completed.is_none()
        && let Some(result) = state.task.as_mut().and_then(check_ready)
    {
        // check_ready不等待工作线程。
        state.task = None; // 真实CPU任务结束后释放句柄，未采用候选仍继续占槽。
        state.completed = Some(result); // 未采用完整候选仍占槽，不并发启动下一张写侧。
    }
    if let Some(result) = state.completed.take() {
        let source_is_current = writable
            && session.revision == result.revision
            && session.draft.as_ref().is_some_and(|draft| {
                draft.map_id == result.map_id && result.source.is_current(draft)
            });
        if source_is_current && result.ground_content.is_some() && ground_content.is_none() {
            state.completed = Some(result);
            editor.status = "正在等待原作者高度内容；积水完整候选暂停采用".into();
        } else {
            if writable
                && session.revision == result.revision
                && result
                    .ground_content
                    .is_none_or(|content| ground_content == Some(content))
                && session.draft.as_ref().is_some_and(|draft| {
                    draft.map_id == result.map_id && result.source.is_current(draft)
                })
            {
                // 地图与修订共同匹配。
                if result.changed {
                    // 未变化不制造保存责任。
                    apply_snapshot(
                        session.draft.as_mut().expect("已核对当前草稿"),
                        &result.snapshot,
                    ); // 描述与像素原子采用。
                    state.current = Some(result.snapshot); // 归属本笔的最后完整画布，也是历史收口的唯一after来源。
                    session.mark_changed(&mut editor); // 后台块换代后旧任务不能再覆盖。
                    state.expected_revision = session.revision;
                    state.changed = true; // 整笔历史稍后收口。
                } // 完成本块采用。
            } else {
                // 并发编辑、换图或重载使任务失效。
                state.pending = None; // 不把旧尾段写入新地图。
                state.released = true; // 已采用部分仍可在原地图收口。
            } // 结束完整身份门。
        }
    } // 结束非阻塞轮询。
    if state.map_id.is_some()
        && state.map_id.as_ref() != session.draft.as_ref().map(|draft| &draft.map_id)
    {
        // 换图不复用相同纹素下标。
        state.before = None; // 旧地图历史由会话加载统一回收。
        state.current = None;
        state.pending_ground = None;
        state.ground_content = None;
        state.pending = None; // 旧未受理尾段丢弃。
        state.map_id = None; // 运行中旧任务保留至自然结束。
        state.changed = false; // 新图不得继承旧笔状态。
    } // 结束换图失效。
    let context = editor.open && writable
        && !ground.as_deref().is_some_and(|ground|ground.volume_paint_ray().is_some()) // 三维命中交给同一持久体积作者事务，禁止重复写二维R8。
        && !editor.close_requested
        && editor.tool == EditorTool::Puddles
        && !session.saving
        && (!clipmap
            || (!editor.save_requested
                && matches!(editor.view, EditorView::Perspective | EditorView::Top)
                && !mode.as_ref().is_some_and(|mode| mode.pointer_over_controls)))
        && !scrub.blocked()
        && browser.dialog.is_none()
        && !drag.blocks_other_mouse()
        && !conflict.modal_open()
        && super::super::paint::hovered(&viewport); // 公共交互门；后台轮询不被模态暂停。
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight); // 不把选择手势写成刷印。
    let intent = map_id.map(|map_id| (map_id, session.terrain_revision));
    if clipmap {
        if !context
            || control
            || !mouse.pressed(MouseButton::Left)
            || state.pending_ground != intent
        {
            state.pending_ground = None;
        }
        if context
            && !control
            && !state.busy()
            && mouse.just_pressed(MouseButton::Left)
            && mouse.pressed(MouseButton::Left)
        {
            state.pending_ground = intent; // 缺页不创建画布；仅本次持续按住可在就绪后起笔。
        }
    } else {
        state.pending_ground = None; // 地形路线改变后不能恢复旧Clipmap起笔意图。
    }
    let paint_point = if clipmap {
        snapshot
            .and_then(|snapshot| {
                session.draft.as_ref().and_then(|draft| {
                    ground.as_ref().and_then(|ground| {
                        ground.accepted_paint_hit(
                            snapshot,
                            &draft.map_id,
                            &GroundCursorTarget::Puddles,
                            session.terrain_revision,
                        )
                    })
                })
            })
            .filter(|world| editor.brush_cursor_world == Some(*world)) // 模式浮层撤销点后不能穿透绘制。
    } else {
        editor.brush_cursor_world
    };
    let paint_point = super::super::terrain_documents::paint_point(&session, &editor, paint_point);
    let starting = context
        && !control
        && !state.busy()
        && ((!clipmap && mouse.just_pressed(MouseButton::Left))
            || (clipmap
                && paint_point.is_some()
                && mouse.pressed(MouseButton::Left)
                && (mouse.just_pressed(MouseButton::Left) || state.pending_ground.is_some())));
    if starting {
        // 只开始一个完整按压事务。
        if let Some(draft) = session.draft.as_ref() {
            // 地图作者数据已完整就绪。
            state.before = Some(PuddleSnapshot {
                descriptor: draft.terrain_manifest().puddles.clone(),
                mask: draft.puddle_mask.clone(),
            }); // O(1)冻结起点。
            state.current = state.before.clone();
            state.pending_ground = None;
            state.ground_content = if clipmap { ground_content } else { None };
            state.expected_revision = session.revision;
            state.map_id = Some(draft.map_id.clone()); // 与整笔快照绑定。
            state.released = false; // 松手后才收口历史。
            state.changed = false; // 新笔没有已采用变化。
            state.last = None; // 不连接前一笔终点。
        } // 结束新笔冻结。
    } // 结束按压检测。
    if state.before.is_some() && (!mouse.pressed(MouseButton::Left) || !context || control) {
        state.released = true;
    } // 视口外松手、工具切换同样收口。
    if paint_point.is_none()
        && (!clipmap
            || !ground
                .as_ref()
                .is_some_and(|ground| ground.pending_same_context))
    {
        state.last = None;
    } // 圆完全离图时切断插值，重新进入不跨接旧笔迹。
    if state.before.is_some()
        && !state.released
        && context
        && let Some(world) = paint_point
    {
        // 无真实地形命中不猜测位置。
        let amount = editor.brush_strength
            * if starting {
                1.0
            } else {
                time.delta_secs().clamp(0.0, 0.05) * 4.0
            }; // 单击立即可见、长帧不过冲。
        let erase = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if !clipmap || state.pending.is_none_or(|sample| sample.erase == erase) {
            let amount = (amount + state.pending.map_or(0.0, |sample| sample.amount)).min(1.0);
            state.pending = Some(Sample {
                world,
                radius: editor.brush_radius,
                amount,
                erase,
            }); // 只合并已取得精确点的同语义尾样本，查询等待不累积强度。
        } // Shift变化时先完成旧尾段，不把绘制强度重新解释为擦除。
    } // 结束本帧输入采样。
    if state.task.is_none()
        && state.completed.is_none()
        && (state.ground_content.is_none() || state.ground_content == ground_content)
        && let Some(sample) = state.pending.take()
        && let Some(draft) = session.draft.as_ref()
    {
        // 只允许一个写侧任务。
        let input = PuddleSnapshot {
            descriptor: draft.terrain_manifest().puddles.clone(),
            mask: draft.puddle_mask.clone(),
        }; // 全图仅浅共享。
        let map_id = draft.map_id.clone(); // 完成值冻结当前地图。
        let revision = session.revision; // 迟到结果不覆盖任何并发作者修改。
        let folder = draft.terrain_folder().to_owned(); // 首次创建只使用当前私有目录。
        let metric = super::super::terrain_documents::PaintMetricSpace::from_draft(draft);
        let min = metric.bounds(draft.terrain_manifest().world_min);
        let max = metric.bounds(draft.terrain_manifest().world_max);
        let from = state
            .last
            .filter(|previous| {
                !clipmap
                    || snapshot
                        .is_some_and(|snapshot| snapshot.segment_is_known(*previous, sample.world))
            })
            .unwrap_or(sample.world); // 中间高度未知时只画当前真实点，不跨缺页连接。
        let task_ground_content = state.ground_content;
        state.last = Some(sample.world); // 已受理尾端成为下一块起点。
        let from = metric.point(from);
        let sample = Sample {
            world: metric.point(sample.world),
            ..sample
        }; // 圆刷半径保持世界米数，地址仍属于本对象R8。
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            // 全图复制与所有像素计算离开帧关键路径。
            let descriptor = input
                .descriptor
                .clone()
                .unwrap_or_else(|| MapPuddleDescriptor {
                    source_path: format!("{folder}/puddles.potential.r8"),
                    texture_path: format!("{folder}/puddles.potential.ktx2"),
                    resolution: [1024, 1024],
                }); // 独立1MiB默认画布。
            let mut pixels = input
                .mask
                .as_ref()
                .map(|mask| mask.as_ref().clone())
                .unwrap_or_else(|| vec![0; 1024 * 1024]); // 写侧在后台物化。
            let changed =
                super::helpers::paint(&mut pixels, descriptor.resolution, min, max, from, sample); // 每个包围盒纹素只计算一次且不保留局部树。
            PaintResult {
                map_id,
                revision,
                ground_content: task_ground_content,
                source: input,
                snapshot: PuddleSnapshot {
                    descriptor: Some(descriptor),
                    mask: Some(Arc::new(pixels)),
                },
                changed,
            } // 只返回完整候选。
        })); // 下一帧非阻塞轮询。
    } // 结束单槽启动。
    if state.released
        && state.task.is_none()
        && state.completed.is_none()
        && state.pending.is_none()
        && let Some(before) = state.before.take()
    {
        // 全部已受理输入完成后才压栈。
        if state.changed
            && let Some(draft) = session.draft.as_ref()
            && state
                .current
                .as_ref()
                .is_some_and(|current| current.is_current(draft))
        {
            // 至少一个后台块真实改变像素。
            let after = state.current.take().expect("已复核本笔最终版本");
            session.push_bounded_water_history(EditorCommand::PuddlesChanged { before, after }); // 整笔一次撤销并服从像素快照预算。
            editor.status = "积水潜力笔划完成；Shift擦除，切换工具查看随天气变化的水洼".into(); // 显示实际作者语义。
        } // 无变化不制造命令。
        state.map_id = None; // 下一笔重新绑定当前地图。
        state.current = None;
        state.ground_content = None;
        state.changed = false; // 整笔累计状态清零。
        state.last = None; // 下一笔不插值跨接。
    } // 结束整笔事务。
} // 结束作者笔刷系统。

pub(crate) fn sync_puddle_preview(
    // 图像创建及全图复制继续后台化。
    mut editor: ResMut<MapEditorState>, // 作者开关和预览工具状态。
    session: Res<EditorAuthoringSession>, // 当前已经完整采用的作者源。
    mut state: ResMut<PuddleAuthoringState>, // 唯一图像准备任务和前一有效版本。
    mut images: ResMut<Assets<Image>>,  // 仅主线程写入资产集合。
    handoff: Option<Res<feathrise_map_content::MapPuddleSaveHandoff>>, // 本次保存的R8内容尚未被正式场景采用时保留草稿。
    mut live: ResMut<MapPuddleLiveOverride>,                     // 正式渲染统一消费的公开小资源。
) {
    // 非阻塞轮询完整CPU Image。
    let draft = session.draft.as_ref().filter(|draft| {
        editor.open
            || draft.puddles_dirty
            || handoff
                .as_ref()
                .is_some_and(|handoff| handoff.map_id.as_ref() == Some(&draft.map_id))
    }); // 未保存画布关闭F12后仍可在玩法中测试。
    if let Some((map_id, source, resolution, image)) =
        state.preview_task.as_mut().and_then(check_ready)
    {
        // 不等待图像复制。
        state.preview_task = None; // 保留单在途背压。
        if draft.is_some_and(|draft| {
            draft.map_id == map_id
                && draft
                    .terrain_manifest()
                    .puddles
                    .as_ref()
                    .is_some_and(|layer| layer.resolution == resolution)
                && draft
                    .puddle_mask
                    .as_ref()
                    .is_some_and(|mask| Arc::ptr_eq(mask, &source))
        }) {
            // 地图及实际版本必须仍匹配。
            match image {
                // 完整失败不会替换旧GPU画布。
                Ok(image) => {
                    // 本次全部Mip已经准备完成。
                    state.image = Some(images.add(image)); // 主线程只移动完整资产。
                    state.preview_resolution = Some(resolution); // 尺寸与像素身份共同采用。
                    state.preview_source = Some(source); // 强Arc证明实际源版本。
                    state.preview_map = Some(map_id); // 当前图像准确归属。
                    state.preview_failure = None; // 成功版本清除旧失败诊断。
                } // 完成同版本GPU候选采用。
                Err(error) => {
                    // 原画布和dirty责任都保留。
                    editor.status = error.clone(); // 作者看见明确解码失败。
                    state.preview_failure = Some((map_id, source, resolution, error)); // 禁止同版本自动忙重试。
                } // 修改或重载产生新身份后可重试。
            } // 结束完整预览结果处理。
        } // 迟到图像直接失效。
    } // 结束图像任务轮询。
    let Some(draft) = draft else {
        // 关闭时立即归还正式来源。
        if live.map_id.is_some() {
            *live = MapPuddleLiveOverride::default();
        } // 无需保持作者GPU覆盖。
        state.preview_source = None; // 释放旧作者版本身份。
        state.preview_map = None; // 重新打开时重新核对完整地图。
        state.image = None; // 释放旧GPU强句柄。
        return; // 运行中任务待自然完成后拒收。
    }; // 当前作者地图已存在。
    let matches_source = !state.preview_pending_for(draft); // 不扫描整图比较。
    if state.preview_task.is_none()
        && !matches_source
        && state.preview_error_for(draft).is_none() // 失败版本等待作者修改或重载。
        && let (Some(mask), Some(layer)) = (&draft.puddle_mask, &draft.terrain_manifest().puddles)
    {
        // 一次只准备当前最新完整版本。
        let source = mask.clone(); // 不深拷贝输入。
        let resolution = layer.resolution; // 已受检作者尺寸。
        let map_id = draft.map_id.clone(); // 冻结图像所属地图。
        state.preview_task = Some(AsyncComputeTaskPool::get().spawn(async move {
            // 图像像素分配不在帧线程。
            let image = super::image::build(source.as_slice(), resolution); // 与正式保存共用全Mip R8派生。
            (map_id, source, resolution, image) // 完整CPU候选等待主线程采用。
        })); // 稳定帧不重复启动。
    } // 结束图像准备。
    if draft.puddle_mask.is_some() && state.preview_map.as_ref() != Some(&draft.map_id) {
        // 首次打开F12尚未完成图像准备时继续显示正式来源，不能用空图临时擦掉已保存水洼。
        if live.map_id.is_some() {
            *live = MapPuddleLiveOverride::default();
        } // 同时撤销另一地图遗留的作者覆盖。
        return; // 当前图像任务仍继续运行，完整结果到达后再发布蓝色诊断或实际潜力。
    }
    let mask = if draft.puddle_mask.is_none() {
        None
    } else if state.preview_map.as_ref() == Some(&draft.map_id) {
        state.image.clone()
    } else {
        None
    }; // 换图决不沿用旧GPU图。
    let preview = editor.open && editor.tool == EditorTool::Puddles; // 诊断色仅随当前工具开启。
    if live.map_id.as_ref() != Some(&draft.map_id)
        || live.mask != mask
        || live.preview != preview
        || live.world_min != Vec2::from_array(draft.terrain_manifest().world_min)
        || live.world_max != Vec2::from_array(draft.terrain_manifest().world_max)
    {
        // 稳定帧不写资源Changed。
        live.map_id = Some(draft.map_id.clone()); // 空画布同样显式覆盖旧正式状态。
        live.mask = mask; // 旧完整图或当前完整图，绝不发布半图。
        live.world_min = Vec2::from_array(draft.terrain_manifest().world_min); // GPU端点契约。
        live.world_max = Vec2::from_array(draft.terrain_manifest().world_max); // GPU端点契约。
        live.preview = preview; // 地形材质可以显示蓝色潜力遮罩。
        live.revision = live.revision.wrapping_add(1); // 每次原子采用都有明确修订。
    } // 结束公共覆盖发布。
} // 结束GPU预览系统。
