//! 地表笔刷输入、共享画布后台等待与单笔局部撤销收口。

use super::{
    // 引入地表作者层事务所需类型和纯助手。
    PAINT_RESPONSE_PER_SECOND,
    byte_changes,
    hovered,
    interpolated_dab_count,     // 完成当前地表作者事务的受检参数或值转换。
    paint_surface_neighborhood, // 完成当前地表作者事务的受检参数或值转换。
}; // 复用原四槽局部算法和连续采样公式。
use crate::authoring::resources::{EditorAuthoringSession, EditorCommand, SurfaceBrushStroke}; // 作者历史和两张像素图属于同一会话。
use crate::authoring::surface_layer::ensure_selected_surface_layer; // 首次真实落笔才创建场景局部引用。
use crate::authoring::surface_layer_edit::{
    // 引入地表作者层事务所需类型和纯助手。
    PendingSurfaceBrushSample,
    SurfaceLayerEditTask,
    start_surface_materialization_if_shared, // 保持当前地表操作的数据和原子采用边界。
}; // 删除和写侧准备共用一个后台CPU槽。
use crate::components::SceneEditorViewport; // 中央视口业务身份。
use crate::resources::SceneEditorState; // 工具、笔刷参数、场景选择和状态。
use crate::types::{EditorTool, EditorView}; // 笔刷只在顶视/透视中受理明确的XZ位置。
use crate::viewport::SurfaceStrokeTrail; // 只记录真实受理样本，不把鼠标悬停误作绘制。
use crate::viewport::vegetation_ground::{GroundCursorTarget, VegetationGroundCursor}; // Clipmap地表与其他作者工具共用唯一精确射线槽。
use crate::authoring::terrain_ground::TerrainGroundCursor;
use bevy::prelude::*; // 输入、真实时间、世界坐标与资源参数。
use bevy::ui::RelativeCursorPosition; // 地表落笔只在中央编辑视口内开始。
use feathrise_scene_content::SurfacePresetRegistry; // 懒加入当前目录中的真实PBR预设。
use std::sync::Arc; // 只有独占作者源允许局部写入，绝不在输入帧复制整图。 // 只在地表工具下开始新笔。

mod object_press;
mod types;
use types::{ObjectSurfacePress, SurfaceSampleProgress};

const MAX_QUEUED_SURFACE_SAMPLES: usize = 256; // 约9KiB标量轨迹；独立于32印/帧的像素处理预算。

/// 地表响应按渐近贡献合并，等待再久仍为有限比例，不用额外刷印追赶后台延迟。
fn combined_amount(a: f32, b: f32) -> f32 {
    1.0 - (1.0 - a.clamp(0.0, 1.0)) * (1.0 - b.clamp(0.0, 1.0))
}

fn consume_waiting_amount(stroke: &mut SurfaceBrushStroke, erase: bool, amount: f32) -> f32 {
    stroke.waiting_amount.take().filter(|(waiting_erase, _)| *waiting_erase == erase)
        .map_or(amount, |(_, waiting)| combined_amount(waiting, amount))
}

fn origin_is_current(
    // 地表作者操作在受检输入下执行当前步骤。
    editor: &SceneEditorState,          // 当前F12场景选择、保存关闭意图与状态。
    session: &EditorAuthoringSession, // 完整作者修订共同防止迟到结果回写。
    stroke: &SurfaceBrushStroke,      // 完成当前地表作者事务的受检参数或值转换。
) -> bool {
    // 当前受检分支或参数组结束。
    // 每个补画段都复核开始时的稳定作者身份。
    editor.open // 完成当前地表作者事务的受检参数或值转换。
        && !session.saving // 依据完整作者条件选择有限处理分支。
        && super::super::terrain_documents::canvas_writable(session, editor)
        && stroke.expected_revision == session.revision // 完整作者修订共同防止迟到结果回写。
        && stroke.expected_surface_revision == session.surface_revision // 同场景作者源重新加载也不能冒充本笔原图。
        && editor.selected_scene_id == stroke.scene_id // 稳定场景身份必须与当前草稿一致。
        && session.draft.as_ref().is_some_and(|draft| { // 只读借用当前完整作者值。
            // 未授权跨场景、其他作者修改和保存都取消旧段。
            Some(&draft.scene_id) == stroke.scene_id.as_ref() // 稳定场景身份必须与当前草稿一致。
                && draft // 依据完整作者条件选择有限处理分支。
                    .surface // 地表局部层表是唯一作者索引依据。
                    .as_ref() // 只读借用当前完整作者值。
                    .and_then(|surface| surface.layers.get(stroke.layer_index)) // 冻结场景局部层位置，防止删除或绘制其他作者层。
                    .is_some_and(|layer| Some(layer.id.as_str()) == stroke.layer_id.as_deref()) // 同一个预设的不同层不能互相替代。
        }) // 完整来源一致时才可回写。
} // 来源校验结束。

fn queue_tail(stroke: &mut SurfaceBrushStroke, sample: PendingSurfaceBrushSample) -> bool {
    // B→C、C→D分别冻结；即使共线也不合并曝光，否则C处停留会加深整条B→C。
    if sample.from == sample.to
        && let Some(queued) = stroke.queued_samples.back_mut()
        && queued.from == queued.to
        && queued.to == sample.to
        && queued.radius == sample.radius
        && queued.erase == sample.erase
    {
        queued.amount = combined_amount(queued.amount, sample.amount);
        stroke.tail_disconnected = false;
        return true;
    }
    if stroke.queued_samples.len() >= MAX_QUEUED_SURFACE_SAMPLES {
        // 已受理路径全部保留；暂不受理的新位置不得在背压解除后被直线追赶。
        stroke.tail_disconnected = true;
        return false;
    }
    stroke.queued_samples.push_back(sample);
    stroke.tail_disconnected = false;
    true
}

fn apply_sample(
    // 地表作者操作在受检输入下执行当前步骤。
    editor: &mut SceneEditorState, // 当前F12场景选择、保存关闭意图与状态。
    session: &mut EditorAuthoringSession, // 完整作者修订共同防止迟到结果回写。
    stroke: &mut SurfaceBrushStroke, // 完成当前地表作者事务的受检参数或值转换。
    task: &mut SurfaceLayerEditTask, // 地表删除和写侧准备共用single-flight任务槽。
    sample: PendingSurfaceBrushSample, // 完成当前地表作者事务的受检参数或值转换。
    trail: &mut SurfaceStrokeTrail, // 成功局部写入后发布轻量轨迹。
) -> Result<SurfaceSampleProgress, String> {
    // 当前受检分支或参数组结束。
    // Waiting不推进端点；Applied只消费当前预算内真正完成的前缀。
    if start_surface_materialization_if_shared(session, task)? {
        // 依据完整作者条件选择有限处理分支。
        // 结构历史或预览持有原Arc时在后台复制。
        editor.status = "正在后台准备地表独立画布；当前落点将在完成后继续，松开也会补画".to_owned(); // 用户知道该笔已经被接收。
        return Ok(SurfaceSampleProgress::Waiting); // 保留冻结样本，不在输入帧等待任务。
    } // 共享写侧门结束。
    let draft = session // 只操作已经完整加载的当前场景草稿。
        .draft // 只操作已经完整加载的当前场景草稿。
        .as_mut() // 独占借用当前作者字段，尚未写入任何像素。
        .ok_or_else(|| "当前场景作者数据尚未就绪".to_owned())?; // 来源已受检，仍防御草稿缺失。
    // 落点已经属于对象源坐标；轴向米制空间使世界圆刷在源上成为正确椭圆，UV与字节地址不变。
    let metric = super::super::terrain_documents::PaintMetricSpace::from_draft(draft);
    // 预算只限制本帧推进距离，不能把整段稀释成32个相隔很远的圆印；剩余完整段继续占原槽。
    let total_dabs = super::brush_path_dab_count(metric.point(sample.from), metric.point(sample.to), sample.radius);
    let fraction = (super::MAX_INTERPOLATED_DABS_PER_FRAME as f32 / total_dabs as f32).min(1.0);
    let endpoint = sample.from.lerp(sample.to, fraction);
    let remaining = (fraction < 1.0).then_some(PendingSurfaceBrushSample { from: endpoint, ..sample });
    let sample = PendingSurfaceBrushSample { to: endpoint, ..sample };
    let source_min = metric.bounds(draft.terrain_manifest().world_min);
    let source_max = metric.bounds(draft.terrain_manifest().world_max);
    let from = metric.point(sample.from);
    let to = metric.point(sample.to);
    let surface = draft // 保持当前地表操作的数据和原子采用边界。
        .surface // 地表局部层表是唯一作者索引依据。
        .as_ref() // 只读借用当前完整作者值。
        .ok_or_else(|| "当前场景没有地表层定义".to_owned())?; // 当前局部层表是笔刷唯一身份来源。
    let layer_count = // 保持当前地表操作的数据和原子采用边界。
        u32::try_from(surface.layers.len()).map_err(|_| "地表层数量超出u32索引范围".to_owned())?; // 不增加产品层数限制。
    let weights = draft // 保持当前地表操作的数据和原子采用边界。
        .surface_weights // 读取或传递同版本RGBA8权重作者源。
        .as_mut() // 独占借用当前作者字段，尚未写入任何像素。
        .and_then(Arc::get_mut) // 只继续处理存在且身份仍匹配的作者值。
        .ok_or_else(|| "地表权重独立写侧尚未准备完成".to_owned())?; // get_mut只检查所有权，不会触发整图复制。
    let indices = draft // 保持当前地表操作的数据和原子采用边界。
        .surface_indices // 读取或传递同版本小端RGBA32局部索引源。
        .as_mut() // 独占借用当前作者字段，尚未写入任何像素。
        .and_then(Arc::get_mut) // 只继续处理存在且身份仍匹配的作者值。
        .ok_or_else(|| "地表索引独立写侧尚未准备完成".to_owned())?; // 两张写侧全部存在后才修改像素。
    let dab_count = interpolated_dab_count(from, to, sample.radius); // 保留原每帧最多32个插值dab预算。
    let mut changed = false; // 没有真实变化时不推进作者修订。
    for dab in 1..=dab_count {
        // 依据完整作者条件选择有限处理分支。
        // 逐局部邻域绘制，整图物化已经完全留在后台。
        changed |= paint_surface_neighborhood(
            // 完成当前地表作者事务的受检参数或值转换。
            // 四槽索引与权重在同一次局部算法调用中更新。
            indices,                                      // 已独占的局部索引作者源。
            weights,                                      // 已独占的归一化权重作者源。
            surface.resolution,                           // 两张作者图使用同一分辨率。
            source_min,                                   // 当前对象源坐标下界。
            source_max,                                   // 当前对象源坐标上界。
            from.lerp(to, dab as f32 / dab_count as f32), // 世界米制空间中按真实距离连续补点。
            sample.radius,                                // 当前冻结段的米制半径。
            stroke.layer_index as u32,                    // 整笔绑定的真实场景局部层。
            layer_count,                                  // 正式u32局部索引范围。
            sample.erase,                                 // 等待期间不重解释Shift语义。
            sample.amount,                                // 单击或累计后的有限强度。
            &mut stroke.before_indices,                   // 首次触碰索引字节形成一条局部撤销。
            &mut stroke.before_weights,                   // 首次触碰权重字节与索引共同收口。
        ); // 当前dab原子完成。
    } // 完整冻结段完成。
    stroke.last_world = Some(sample.to); // 只有成功应用全部dab才推进下一个连续段起点。
    if changed {
        // 依据完整作者条件选择有限处理分支。
        // 没有变化的空白擦除不制造脏状态。
        session.mark_surface_changed(editor); // 统一推进地表预览、保存与完整作者修订。
        editor.status = if sample.erase {
            // 完成当前地表作者事务的受检参数或值转换。
            "正在擦除当前地表层".to_owned() // 回归检查当前作者定义、像素或生命周期不变量。
        } else {
            // 当前受检分支或参数组结束。
            "正在绘制当前地表层".to_owned() // 回归检查当前作者定义、像素或生命周期不变量。
        }; // 原笔刷状态继续保持。
    } // 真实修改发布结束。
    stroke.expected_revision = session.revision; // 只接受当前笔自己刚推进后的完整作者版本。
    stroke.expected_surface_revision = session.surface_revision; // 本笔真实修改后同步更新期望地表版本。
    trail.record(
        session,
        sample.from,
        sample.to,
        sample.radius,
        sample.erase,
        sample.amount,
    ); // 短点击即使同帧收口仍保留真实路径。
    Ok(SurfaceSampleProgress::Applied(remaining)) // 只消费已经实际刷过的前缀，长段仍保持同一笔撤销。
} // 受检局部样本应用结束。

fn finish_surface_stroke(
    // 地表作者操作在受检输入下执行当前步骤。
    editor: &mut SceneEditorState, // 当前F12场景选择、保存关闭意图与状态。
    session: &mut EditorAuthoringSession, // 完整作者修订共同防止迟到结果回写。
    stroke: &mut SurfaceBrushStroke, // 完成当前地表作者事务的受检参数或值转换。
) {
    // 当前受检分支或参数组结束。
    // 整笔完成后只形成一条局部差分历史。
    if !stroke.active {
        // 依据完整作者条件选择有限处理分支。
        return; // 本帧不再继续消费或修改作者数据。
    } // 空状态不制造撤销命令。
    let weight_changes = session // 保持当前地表操作的数据和原子采用边界。
        .draft // 只操作已经完整加载的当前场景草稿。
        .as_ref() // 只读借用当前完整作者值。
        .and_then(|draft| draft.surface_weights.as_ref()) // 读取或传递同版本RGBA8权重作者源。
        .map(|weights| byte_changes(&stroke.before_weights, weights)) // 完成当前地表作者事务的受检参数或值转换。
        .unwrap_or_default(); // 只扫描本笔首次触及字节。
    let index_changes = session // 保持当前地表操作的数据和原子采用边界。
        .draft // 只操作已经完整加载的当前场景草稿。
        .as_ref() // 只读借用当前完整作者值。
        .and_then(|draft| draft.surface_indices.as_ref()) // 读取或传递同版本小端RGBA32局部索引源。
        .map(|indices| byte_changes(&stroke.before_indices, indices)) // 完成当前地表作者事务的受检参数或值转换。
        .unwrap_or_default(); // 索引差分与权重同一收口边界。
    *stroke = SurfaceBrushStroke::default(); // 清除稳定身份、冻结段和松键状态，为下笔重新绑定。
    if !weight_changes.is_empty() || !index_changes.is_empty() {
        // 依据完整作者条件选择有限处理分支。
        // 单侧变化也属于一条完整地表笔划。
        session.undo.push(EditorCommand::SurfaceChanged {
            // 完整作者动作进入唯一线性撤销历史。
            weight_changes, // 完成当前地表作者事务的受检参数或值转换。
            index_changes,  // 完成当前地表作者事务的受检参数或值转换。
        }); // 两张作者图共用一次撤销。
        session.redo.clear(); // 新笔打断原重做分支。
        editor.status = "地表笔划完成；后台预览与KTX2派生等待保存".to_owned(); // 只在待补段全部完成后提示笔划完成。
    } // 非空历史登记结束。
} // 笔划收口结束。

fn stop_stale_stroke(
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    stroke: &mut SurfaceBrushStroke,
) -> bool {
    // 普通连续笔和共享等待笔使用相同的换代保护。
    if !stroke.active || origin_is_current(editor, session, stroke) {
        // 新笔或仍然有效的当前笔无需清理。
        return false; // 继续按原输入处理完整样本。
    } // 作者来源校验结束。
    if session.surface_revision == stroke.expected_surface_revision // 其他作者域修改只终止旧输入，当前地表字节仍然属于本笔。
        && session.draft.as_ref().is_some_and(|draft| Some(&draft.scene_id) == stroke.scene_id.as_ref())
    // 同一场景地表没有发生重新加载或版本替换。
    {
        // 只有原地表版本仍然有效时才能计算已完成部分的局部撤销。
        finish_surface_stroke(editor, session, stroke); // 已完成字节保持可撤销，未应用旧段安全停止。
    } else {
        // 地表本身换代时，新像素绝不能用于旧笔before差分。
        *stroke = SurfaceBrushStroke::default(); // 丢弃旧笔临时状态，当前新草稿和历史完全不变。
    } // 失效笔划清理结束。
    editor.save_requested = false; // 未完成旧笔不能紧接着保存另一版地表并掩盖取消。
    editor.cancel_close_request("待补地表笔划的场景或作者内容已变化，已停止旧段"); // 继续显示F12及实际取消原因。
    true // 调用方当前帧不再把同一按键重新解释为其他作者笔划。
} // 统一换代笔划处理结束。

#[allow(clippy::too_many_arguments)] // 输入与所有权门按各自资源保持清晰。
pub(crate) fn apply_surface_brush(
    // 地表作者操作在受检输入下执行当前步骤。
    // 原笔刷入口名称保持不变，插件继续沿用原系统次序。
    mouse: Res<ButtonInput<MouseButton>>, // 左键开始、延伸和结束一笔。
    keyboard: Res<ButtonInput<KeyCode>>,  // Shift冻结当前段的擦除语义。
    time: Res<Time<Real>>,                // 连续笔刷按真实帧时间计算强度。
    viewport: Query<&RelativeCursorPosition, With<SceneEditorViewport>>, // 只允许中央视口开始新笔。
    mut editor: ResMut<SceneEditorState>,   // 当前场景、工具、参数和状态。
    mut session: ResMut<EditorAuthoringSession>, // 当前定义、配对像素和统一历史。
    mut stroke: ResMut<SurfaceBrushStroke>, // 待补段及首次字节差分。
    surface_presets: Option<Res<SurfacePresetRegistry>>, // 当前原子全局预设目录。
    mut task: ResMut<SurfaceLayerEditTask>, // 与删层共用single-flight写侧准备槽。
    mut trail: ResMut<SurfaceStrokeTrail>, // 即时辅助线与PBR采用之间的固定容量缓存。
    surface: Option<feathrise_scene::TerrainObjectClipmapGround>, // 冻结二维样本也必须绑定同代作者高度内容。
    ground: Option<Res<VegetationGroundCursor>>,            // 独立GLB笔刷回归不要求安装场景插件。
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>, // 浮层按下不能武装延迟起笔。
    objects_cursor: Option<Res<TerrainGroundCursor>>,
    mut object_press: Local<Option<ObjectSurfacePress>>,
    mut active_object_press: Local<Option<ObjectSurfacePress>>,
) {
    // 当前受检分支或参数组结束。
    // 输入系统不得调用任何整图复制。
    trail.reconcile(&session); // 先清除外部作者换代，随后本笔合法修订由record接续。
    if stroke.active // 依据完整作者条件选择有限处理分支。
        && (mouse.just_released(MouseButton::Left) // 依据完整作者条件选择有限处理分支。
            || editor.close_requested // 依据完整作者条件选择有限处理分支。
            || editor.tool != EditorTool::Surface)
    // 依据完整作者条件选择有限处理分支。
    {
        // 满足上述完整条件后才进入当前分支。
        // 关闭或切工具同样先补完旧笔。
        stroke.release_requested = true; // 松键意图保留到最后待补段收口。
    } // 收口意图捕获结束。
    if stop_stale_stroke(&mut editor, &mut session, &mut stroke) {
        // 同图地表换代和跨场景切换都先结束旧输入。
        trail.clear(); // 已取消输入不留下像是成功受理的路径。
        return; // 本帧不允许旧笔借用新地表继续绘制或构造历史。
    } // 统一作者换代检查结束。
    let clipmap = session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.uses_legacy_clipmap_cursor());
    let objects = session.draft.as_ref()
        .is_some_and(|draft| draft.manifest.terrains.is_some());
    let mut active_context = editor.open
        && !ground
            .as_ref()
            .is_some_and(|g| g.volume_paint_ray().is_some())
        && !editor.close_requested
        && editor.tool == EditorTool::Surface
        && !session.saving
        && !task.is_deleting()
        && hovered(&viewport)
        && (!clipmap && !objects
            || (!editor.save_requested
                && matches!(editor.view, EditorView::Perspective | EditorView::Top)
                && !mode.as_ref().is_some_and(|mode| mode.pointer_over_controls)
                && !keyboard.pressed(KeyCode::ControlLeft)
                && !keyboard.pressed(KeyCode::ControlRight)))
        && ground.as_ref().is_none_or(|ground| ground.source_authoring_available()); // Ctrl选层和子场景范围保护不能武装延迟起笔。
    let erase = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if !stroke.active { *active_object_press = None; }
    if objects && stroke.active && active_object_press.as_ref().is_some_and(|active|
        !object_press::active_is_current(active, &editor, &session, objects_cursor.as_deref())) {
        active_context = false;
        stroke.release_requested = true;
        stroke.last_world = None; // 相机、按压或鼠标所有权变更只收口已受理段，不能跨门续画。
    }
    object_press::update(
        &mut object_press, objects && active_context && !stroke.active,
        &mut editor, &session, objects_cursor.as_deref(), &mouse, erase,
    );
    let target = session
        .draft
        .as_ref()
        .and_then(|draft| draft.surface.as_ref())
        .and_then(|surface| surface.layers.get(editor.selected_surface_layer))
        .map(|layer| GroundCursorTarget::Surface {
            layer_id: layer.id.clone(),
            preset_id: editor.surface_selected_preset_id.clone(),
        });
    let scene_id = session.draft.as_ref().map(|draft| draft.scene_id.clone());
    let snapshot = surface
        .as_ref()
        .and_then(|surface| surface.snapshot.as_ref());
    let ground_content = snapshot
        .filter(|snapshot| {
            Some(&snapshot.scene_id) == scene_id.as_ref()
                && snapshot.authoring_height_revision() == Some(session.terrain_revision)
        })
        .map(|snapshot| snapshot.content_revision_words()); // 已受理段允许相机换代，作者高度内容必须仍一致。
    let intent = scene_id.clone().zip(target.clone()).map(|(scene_id, target)| {
        (
            scene_id,
            target,
            session.terrain_revision,
            session.surface_revision,
        )
    });
    if clipmap {
        if stroke.active
            && (stroke.ground_target != target
                || (ground_content.is_some() && stroke.ground_content != ground_content))
        {
            finish_surface_stroke(&mut editor, &mut session, &mut stroke);
            trail.clear();
            return; // 换层/预设或高度内容换代只保留已完成差分，不补画旧段。
        }
        if !active_context || !mouse.pressed(MouseButton::Left) || stroke.pending_ground != intent {
            stroke.pending_ground = None;
        }
        if active_context
            && !stroke.active
            && mouse.just_pressed(MouseButton::Left)
            && mouse.pressed(MouseButton::Left)
        {
            stroke.pending_ground = intent; // 尚无精确点时不建笔、不修改任何作者字节。
        }
        if stroke.active && !mouse.pressed(MouseButton::Left) {
            stroke.release_requested = true; // 模态吞掉松键边沿时也能正常收口已受理段。
        }
        if stroke.active && ground_content.is_none() {
            stroke.last_world = None;
            stroke.waiting_amount = None;
            stroke.tail_disconnected = true;
            if stroke.pending_sample.is_none() && (stroke.release_requested || !active_context) {
                finish_surface_stroke(&mut editor, &mut session, &mut stroke);
            }
            editor.status = "正在等待原作者高度内容；已受理地表样本暂停".into();
            return; // 未知不冒充换代：保留冻结段，恢复同一内容后才允许补画。
        }
    } else {
        stroke.pending_ground = None; // 旧Clipmap按压意图不跨地形路线复用。
    }
    let paint_point = if clipmap {
        snapshot
            .zip(scene_id.as_ref())
            .zip(target.as_ref())
            .and_then(|((snapshot, scene_id), target)| {
                ground.as_ref().and_then(|ground| {
                    ground.accepted_paint_hit(snapshot, scene_id, target, session.terrain_revision)
                })
            })
            .filter(|world| editor.brush_cursor_world == Some(*world)) // 模式浮层撤销落点后仍禁止穿透写入。
    } else {
        editor.brush_cursor_world
    };
    let object_hit = objects_cursor.as_ref().and_then(|cursor| cursor.paint_hit());
    let paint_point = if objects {
        object_hit.and_then(|hit| super::super::terrain_documents::paint_point_on_terrain(
            &session, &editor, Some(&hit.terrain), Some(hit.point)))
    } else {
        super::super::terrain_documents::paint_point(&session, &editor, paint_point)
    };
    let object_from = object_hit.and_then(|hit| hit.from.and_then(|from|
        super::super::terrain_documents::paint_point_on_terrain(&session, &editor, Some(&hit.terrain), Some(from))));
    let query_pending = if objects {
        objects_cursor.as_ref().is_some_and(|cursor| cursor.paint_pending())
    } else {
        clipmap && ground.as_ref().is_some_and(|ground| ground.pending_same_context)
    };
    if stroke.active {
        if !active_context || stroke.release_requested || !mouse.pressed(MouseButton::Left) {
            stroke.waiting_amount = None;
        } else if paint_point.is_none() {
            if query_pending {
                let amount = editor.brush_strength * time.delta_secs().clamp(0.0, 0.05) * PAINT_RESPONSE_PER_SECOND;
                let previous = stroke.waiting_amount.filter(|(waiting_erase, _)| *waiting_erase == erase)
                    .map_or(0.0, |(_, amount)| amount);
                stroke.waiting_amount = Some((erase, combined_amount(previous, amount)));
            } else {
                stroke.waiting_amount = None; // 真正缺页、漏点和越界不把等待时长涂到下一处。
            }
        }
    }
    if let Some(sample) = stroke.pending_sample {
        if !active_context || (paint_point.is_none()
            && !(objects && objects_cursor.as_ref().is_some_and(|cursor| cursor.paint_pending()))
            && !(clipmap && ground.as_ref().is_some_and(|ground| ground.pending_same_context)))
        {
            stroke.tail_disconnected = true; // 后台正常等待保留连接，明确漏采样、视口或权限断点不能跨越。
        }
        // 依据完整作者条件选择有限处理分支。
        // 已受理的冻结段优先于新输入或关闭动作。
        if !editor.close_requested && editor.tool == EditorTool::Surface // 关闭或切工具不接收新的尾段，但当前释放坐标仍需保留。
            && (mouse.pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left)) // 待补期间鼠标在松键帧移动也不得丢失末段。
            && hovered(&viewport) // 依据完整作者条件选择有限处理分支。
            && active_context && !stroke.release_requested && sample.erase == erase
            && let Some(world) = paint_point
        // 依据完整作者条件选择有限处理分支。
        {
            // 满足上述完整条件后才进入当前分支。
            // 待补期间按受理顺序保留真实折线；每帧只追加一个固定大小标量段。
            let amount = editor.brush_strength // 保持当前地表操作的数据和原子采用边界。
                * time.delta_secs().clamp(0.0, 0.05) // 完成当前地表作者事务的受检参数或值转换。
                * PAINT_RESPONSE_PER_SECOND; // 使用原时间响应公式。
            let amount = consume_waiting_amount(&mut stroke, erase, amount);
            let previous = stroke.queued_samples.back().map_or(sample.to, |tail| tail.to);
            let connected = !stroke.tail_disconnected
                && (!clipmap || snapshot.is_some_and(|snapshot| snapshot.segment_is_known(previous, world)))
                && (!objects || object_from == Some(previous));
            queue_tail(&mut stroke, PendingSurfaceBrushSample {
                from: if connected { previous } else { world },
                to: world,
                radius: editor.brush_radius,
                erase,
                amount,
            });
        } else if paint_point.is_some() {
            stroke.waiting_amount = None; // 当前点未获受理，不能把其曝光寄存到以后其他轨迹。
        } // 尾段输入捕获结束。
        let backpressured = stroke.tail_disconnected && stroke.queued_samples.len() >= MAX_QUEUED_SURFACE_SAMPLES;
        match apply_sample(
            &mut editor,
            &mut session,
            &mut stroke,
            &mut task,
            sample,
            &mut trail,
        ) {
            // 依据完整作者条件选择有限处理分支。
            // 每帧最多处理一个冻结段。
            Ok(SurfaceSampleProgress::Waiting) => {
                if backpressured { editor.status = "地表笔划待补轨迹已满，正在排空已受理路径；新落点暂缓受理".into(); }
                return; // 独立写侧仍未完成，原段与松键意图继续保留。
            }
            Ok(SurfaceSampleProgress::Applied(remaining)) => {
                // 保持当前地表操作的数据和原子采用边界。
                // 当前长段前缀仍占原槽；仅完整完成后取下一条已受理真段，折点与曝光不重解释。
                stroke.pending_sample = remaining.or_else(|| stroke.queued_samples.pop_front());
                if stroke.pending_sample.is_none() // 依据完整作者条件选择有限处理分支。
                    && (stroke.release_requested || !mouse.pressed(MouseButton::Left))
                // 依据完整作者条件选择有限处理分支。
                {
                    // 满足上述完整条件后才进入当前分支。
                    // 所有已受理轨迹完成才解除active。
                    finish_surface_stroke(&mut editor, &mut session, &mut stroke); // 关闭和保存门下一帧才可继续。
                } // 待补笔划收口结束。
            } // 完整补画处理结束。
            Err(error) => {
                // 完成当前地表作者事务的受检参数或值转换。
                // 分配失败或作者源损坏不能稳定帧无限重试。
                finish_surface_stroke(&mut editor, &mut session, &mut stroke); // 已完成部分仍保持可撤销，失败段停止。
                editor.cancel_close_request(format!("地表笔划失败：{error}")); // 显示实际错误而非悄然漏掉点击。
                trail.clear(); // 失败段不能继续显示为待接管的轨迹。
            } // 冻结段错误处理结束。
        } // 本帧冻结段处理结束。
        if backpressured && stroke.active {
            editor.status = "地表笔划待补轨迹已满，正在排空已受理路径；新落点暂缓受理".into();
        }
        return; // 不在恢复帧继续消耗第二个新段。
    } // 待补优先分支结束。
    if !active_context {
        // 依据完整作者条件选择有限处理分支。
        // 离开工具或视口时先把已有局部笔划收口。
        finish_surface_stroke(&mut editor, &mut session, &mut stroke); // 无待补段才允许立即结束。
        return; // 非活动上下文不读取或修改像素。
    } // 活动上下文检查结束。
    let Some(world) = paint_point else {
        // 保持当前地表操作的数据和原子采用边界。
        // 圆完全离图时切断连续段。
        if (objects && !objects_cursor.as_ref().is_some_and(|cursor| cursor.paint_pending()))
            || (!objects && (!clipmap || !ground
                .as_ref()
                .is_some_and(|ground| ground.pending_same_context)))
        {
            stroke.last_world = None; // 明确缺页/未命中切断连续段，正常单飞射线延迟只暂停。
        }
        if mouse.just_released(MouseButton::Left) || (clipmap && !mouse.pressed(MouseButton::Left))
        {
            // 依据完整作者条件选择有限处理分支。
            finish_surface_stroke(&mut editor, &mut session, &mut stroke); // 完成当前地表作者事务的受检参数或值转换。
        } // 图外松键也形成完整撤销边界。
        return; // 当前没有合法场景采样位置。
    }; // 世界落点读取结束。
    let starting = if objects {
        object_press.is_some() && mouse.pressed(MouseButton::Left)
    } else {
        mouse.just_pressed(MouseButton::Left)
            || (clipmap
                && !stroke.active
                && stroke.pending_ground.is_some()
                && mouse.pressed(MouseButton::Left))
    };
    if starting {
        *active_object_press = object_press.clone();
        *object_press = None; // 真实命中后只启动一次；等待写侧由原笔划资源负责，不再保留起笔权限。
        // 依据完整作者条件选择有限处理分支。
        // 真实落笔仍只编辑明确选中且匹配预设的已有作者层。
        let layer = match ensure_selected_surface_layer(
            // 保持当前地表操作的数据和原子采用边界。
            &mut editor,                // 当前F12场景选择、保存关闭意图与状态。
            &mut session,               // 完整作者修订共同防止迟到结果回写。
            surface_presets.as_deref(), // 保持当前地表操作的数据和原子采用边界。
        ) {
            // 当前受检分支或参数组结束。
            // 明确作者选择优先保留同预设独立层。
            Ok(layer) => layer, // 已有局部层或本次新增层都具有真实场景位置。
            Err(error) => {
                // 完成当前地表作者事务的受检参数或值转换。
                editor.status = error; // 完成当前地表作者事务的受检参数或值转换。
                return; // 本帧不再继续消费或修改作者数据。
            } // 无有效层时不建立空笔。
        }; // 懒加入选择完成。
        let Some(draft) = session.draft.as_ref() else {
            // 只读借用当前完整作者值。
            return; // 本帧不再继续消费或修改作者数据。
        }; // 上一步保证草稿存在，仍保持防御检查。
        let layer_id = draft // 稳定作者层和预设引用保持真实对应关系。
            .surface // 地表局部层表是唯一作者索引依据。
            .as_ref() // 只读借用当前完整作者值。
            .and_then(|surface| surface.layers.get(layer)) // 按已冻结下标或独占所有权取得受检目标。
            .map(|layer| layer.id.clone()); // 冻结真实作者ID，不使用预设ID代替。
        editor.selected_surface_layer = layer; // UI与笔刷同步同一个局部选择。
        *stroke = SurfaceBrushStroke {
            // 完成当前地表作者事务的受检参数或值转换。
            active: true, // 完成当前地表作者事务的受检参数或值转换。
            scene_id: Some(draft.scene_id.clone()), // 稳定场景身份必须与当前草稿一致。
            layer_id,     // 稳定作者层和预设引用保持真实对应关系。
            layer_index: layer, // 冻结场景局部层位置，防止删除或绘制其他作者层。
            expected_revision: session.revision, // 完整作者修订共同防止迟到结果回写。
            expected_surface_revision: session.surface_revision, // 记录当前地表实际版本供待补与换代历史保护。
            ground_target: clipmap.then_some(target).flatten(),
            ground_content: if clipmap { ground_content } else { None }, // 后台写侧完成后仍复核原高度内容。
            last_world: Some(world), // 完成当前地表作者事务的受检参数或值转换。
            release_requested: mouse.just_released(MouseButton::Left), // 完成当前地表作者事务的受检参数或值转换。
            ..Default::default() // 完成当前地表作者事务的受检参数或值转换。
        }; // 同帧按下松开也要保留第一次落点。
    } // 新笔稳定身份冻结结束。
    if (mouse.pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Left)) && stroke.active
    // 依据完整作者条件选择有限处理分支。
    {
        // 满足上述完整条件后才进入当前分支。
        // 极短同帧点击也至少受理一个样本。
        let amount = if starting {
            // 保持当前地表操作的数据和原子采用边界。
            editor.brush_strength // 完成当前地表作者事务的受检参数或值转换。
        } else {
            // 当前受检分支或参数组结束。
            editor.brush_strength * time.delta_secs().clamp(0.0, 0.05) * PAINT_RESPONSE_PER_SECOND // 完成当前地表作者事务的受检参数或值转换。
        }; // 保留明确单击与时间连续响应。
        let amount = consume_waiting_amount(&mut stroke, erase, amount);
        let sample = PendingSurfaceBrushSample {
            // 保持当前地表操作的数据和原子采用边界。
            from: stroke
                .last_world
                .filter(|_| !stroke.tail_disconnected)
                .filter(|previous| {
                    if objects { object_from == Some(*previous) }
                    else { !clipmap || snapshot
                            .is_some_and(|snapshot| snapshot.segment_is_known(*previous, world))
                    }
                })
                .unwrap_or(world), // 不跨缺页补画二维地表线段。
            to: world,                   // 完成当前地表作者事务的受检参数或值转换。
            radius: editor.brush_radius, // 完成当前地表作者事务的受检参数或值转换。
            erase,                       // 完成当前地表作者事务的受检参数或值转换。
            amount,                      // 完成当前地表作者事务的受检参数或值转换。
        }; // 先冻结完整语义再尝试独立写侧。
        stroke.tail_disconnected = false; // 背压后的第一次新受理已独立成点，不会追赶被跳过的鼠标路径。
        stroke.pending_sample = Some(sample); // 任何异步等待都不能丢失本次点击。
        match apply_sample(
            &mut editor,
            &mut session,
            &mut stroke,
            &mut task,
            sample,
            &mut trail,
        ) {
            // 依据完整作者条件选择有限处理分支。
            // 独占时本帧立即完成，共享时后台准备。
            Ok(SurfaceSampleProgress::Applied(remaining)) => stroke.pending_sample = remaining, // 仅实际完成整段后才能接收下一帧轨迹。
            Ok(SurfaceSampleProgress::Waiting) => return, // 等待期间保留active，关闭和保存门不能提前通过。
            Err(error) => {
                // 完成当前地表作者事务的受检参数或值转换。
                finish_surface_stroke(&mut editor, &mut session, &mut stroke); // 完成当前地表作者事务的受检参数或值转换。
                editor.cancel_close_request(format!("地表笔划失败：{error}")); // 完成当前地表作者事务的受检参数或值转换。
                trail.clear(); // 失败后立即撤去辅助线。
                return; // 本帧不再继续消费或修改作者数据。
            } // 失败不会无界重试或误报完成。
        } // 新样本应用结束。
    } // 按住或短按采样结束。
    if mouse.just_released(MouseButton::Left) || stroke.release_requested {
        // 依据完整作者条件选择有限处理分支。
        // 没有待补段时立即收口。
        finish_surface_stroke(&mut editor, &mut session, &mut stroke); // 两张作者图合并成唯一可撤销笔划。
    } // 正常松键处理结束。
} // 地表笔刷输入系统结束。

#[cfg(test)] // 真实ECS输入和后台轮询回归无需场景窗口或渲染世界。
mod tests; // 短按、待补关闭保存与同图地表换代分别覆盖。
#[cfg(test)]
mod object_tests;
