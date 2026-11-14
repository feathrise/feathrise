//! F12场景编辑器的延迟关闭门。
mod boundary; // 非当前玩法场景的独立后台空气墙校验。
#[cfg(test)]
mod spawn; // 无承托位置直接进入试玩，不再以出生点查询阻止关闭。
#[cfg(test)]
mod vegetation; // 草地后台更新不阻塞F12退出的调度回归。
#[cfg(test)]
mod save_wait; // 保存预检与在途事务保留具体关闭等待原因。
#[cfg(test)]
mod domain; // 扩展域关闭回执必须属于当前场景、文档、世代和修订。

use bevy::prelude::*; // ECS资源、主窗口查询和系统参数。
use bevy::window::{PrimaryWindow, Window}; // 真正关闭时归还中文IME。
use feathrise_scene_content::{SceneId, SceneTerrainKind}; // 只有可编辑GLB三维地形需要联合应用门，水面门按稳定场景身份判断。

use crate::authoring::{
    // 判断笔划与当前草稿，并读取实时地形完成证据。
    BoundaryEditState,      // 活动控制点拖动必须先形成一次完整撤销和碰撞修订。
    EditorAuthoringSession, // 当前作者草稿及地形专用修订号。
    TerrainBrushStroke,     // 活动笔划必须先收口为完整撤销命令。
    WaterBrushStroke,       // 复合河水笔划必须先形成完整R8/GLB顶点撤销命令。
    WaterFileHandoff,       // 保存后的正式R8与动态水新代次完成前保持F12关闭门。
    WaterNetworkEditState,  // 连续水网控制点必须松手形成完整前后快照。
    WaterPreviewState,      // 全部水层必须与当前未保存GLB地形属于同一联合修订。
};
#[cfg(test)]
use crate::authoring::TerrainLiveApplyState; // 退休有限网格测试仍可构造原候选；正式关闭不再等待GLB地形交接。
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneCreationTask, SceneDeletionTask, SceneEditorState,
    SceneFolderPersistence,
}; // 关闭请求、虚拟目录、拖拽、创建与未确认删除事务。

use super::actions::set_ime_enabled; // 与新建弹框共用唯一IME开关实现。

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct AuthoringCloseGate<'w, 's> {
    objects: Option<Res<'w, super::scene_objects::SceneObjectState>>,
    object_visibility: Option<Res<'w, super::scene_objects::ObjectVisibilityState>>,
    commands: Commands<'w, 's>, // 回到真实玩法才归还启动模式；空世界关闭保留无菜单窗口上下文。
    world: crate::resources::SceneEditorWorldSession<'w>, // 所有关闭来源在最终采用前再次验证真实进图状态。
    active_scene: Option<Res<'w, feathrise_scene_content::ActiveDynamicScene>>, // 正式目标身份，不从诊断猜测。
    scene: Option<Res<'w, State<feathrise_scene_core::state::ScenePhase>>>, // 菜单里没有可等待的场景物理。
    boundary_source: Option<Res<'w, feathrise_scene::DynamicSceneBoundaryOverride>>, // 同修订不可变快照。
    boundary_validation: Local<'s, boundary::BoundaryCloseValidation>, // 非当前场景只需后台校验。
    boundary_numeric: Option<Res<'w, crate::authoring::BoundaryThicknessInputState>>, // 数值事务不允许半途捕获。
    boundary_status: Option<Res<'w, feathrise_scene::DynamicSceneBoundaryStatus>>, // 空气墙后台校验及最新物理发布证据。
    state: Option<ResMut<'w, crate::game_authoring::scene_interaction::SceneInteractionState>>, // 独立旧关闭测试可省略新域。
    puddles: Option<Res<'w, crate::authoring::PuddleAuthoringState>>, // 完整积水整笔和GPU画布就绪后才归还玩法。
    surface_task: Option<Res<'w, crate::authoring::SurfaceLayerEditTask>>, // 合并地表关闭条件，避免系统参数超过Bevy上限。
    surface_stroke: Option<Res<'w, crate::authoring::SurfaceBrushStroke>>, // 保留尚未完成的地表笔划关闭门。
    vegetation_cross: Option<Res<'w, crate::authoring::vegetation_paint_scope::VegetationPaintState>>,
    content_bindings:Option<Res<'w,crate::authoring::content_bindings::ContentTerrainFollowState>>,
}

/// 关闭只消费后台严格校验与原子发布证据，不同步遍历作者点。
fn boundary_preview_blocks_close(
    editor: &mut SceneEditorState,
    status: Option<&feathrise_scene::DynamicSceneBoundaryStatus>,
    scene_id: &SceneId,
    required_revision: Option<u64>,
) -> bool {
    use feathrise_scene::DynamicSceneBoundaryPhase;
    let Some(status) = status else {
        editor.cancel_close_request("空气墙运行时尚未就绪，本次关闭已取消");
        return true;
    };
    if status.scene_id.as_ref() != Some(scene_id)
        || required_revision.is_some_and(|revision| status.override_revision != Some(revision))
        || status.phase == DynamicSceneBoundaryPhase::Loading
    {
        editor.status = "正在后台校验并应用最新空气墙；完成后自动关闭编辑器".to_owned();
        return true; // 特别覆盖松手当帧立即F12：旧碰撞的Ready不是新草稿的完成证据。
    }
    if status.phase == DynamicSceneBoundaryPhase::Failed || status.validation_error.is_some() {
        let error = status.validation_error.as_deref().unwrap_or(&status.detail);
        editor.cancel_close_request(format!("空气墙尚未就绪，本次关闭已取消：{error}"));
        return true; // 开放外框/单点挡墙恢复作者输入，合法内部开放折线不要求闭合。
    }
    false // Ready（或正式无墙Disabled）已经在同一Commands边界完成物理接管。
}

/// 返回当前联合水面是否仍阻止关闭；失败态解除请求，让作者重新取得编辑能力而非永久锁死。
fn water_preview_blocks_close(
    editor: &mut SceneEditorState, // 更新等待状态或解除已经失败的关闭请求。
    water_preview: &WaterPreviewState, // 读取同一联合修订的成功或失败证据。
    scene_id: &SceneId,              // 当前作者场景稳定身份。
    water_revision: u64,         // 当前全部水层联合修订。
    terrain_revision: u64,       // 与水层配套的当前GLB地形修订。
) -> bool {
    if water_preview.ready_for(scene_id, water_revision, terrain_revision) {
        return false; // 完整水面与河床已经原子采用，不再阻止本次关闭。
    }
    if water_preview.failed_for(scene_id, water_revision, terrain_revision) {
        editor.close_requested = false; // 失败修订不会自动重建；解除关闭态后作者才能继续补画、撤销或调整水位。
        editor.status =
            "当前水域未形成有效预览，本次关闭已取消；请补全水网、修正静水河床或撤销后再关闭"
                .to_owned(); // 明确恢复操作入口，不能让F12永久卡在禁止编辑的关闭等待态。
        return true; // 保持编辑器与物理暂停，等待作者产生新修订，而不是暴露旧水面和新河床的半套结果。
    }
    editor.status = "正在后台完成全部静水、河流与瀑布的联合预览；完成后自动关闭编辑器".to_owned(); // 正常任务只短暂等待，不改变关闭意图。
    true // 后台处理中继续保持联合门，成功前不暴露旧水面和新河床组合。
}

const SAVE_CLOSE_WAITING_PREFIX: &str = "保存预检等待：";
const SAVE_CLOSE_ACTIVE_PREFIX: &str = "保存执行中：";
const SAVE_CLOSE_SUFFIX: &str = "；保存完成后自动关闭编辑器";

/// 保存Tasks已经给出当前等待原因；关闭门只补充阶段与关闭意图，不能覆盖真实卡点。
fn save_close_wait_status(status: &str, saving: bool) -> String {
    // 连续等待帧先剥离本门上次加的装饰，防止同一句每帧重复增长。
    let detail = status.strip_suffix(SAVE_CLOSE_SUFFIX).unwrap_or(status);
    let detail = detail
        .strip_prefix(SAVE_CLOSE_WAITING_PREFIX)
        .or_else(|| detail.strip_prefix(SAVE_CLOSE_ACTIVE_PREFIX))
        .unwrap_or(detail);
    let detail = match detail {
        "" | "正在确认未保存地形画面与物理碰撞；完成后自动关闭编辑器"
        | "场景保存事务尚未完成；完成后自动继续关闭" => {
            if saving {
                "正在等待后台保存事务完成"
            } else {
                "正在等待场景保存前置条件就绪"
            }
        }
        reason => reason,
    };
    format!(
        "{}{detail}{SAVE_CLOSE_SUFFIX}",
        if saving {
            SAVE_CLOSE_ACTIVE_PREFIX
        } else {
            SAVE_CLOSE_WAITING_PREFIX
        }
    )
}

/// 未保存地形的画面与碰撞完整采用后，才真正关闭F12并解除下一帧物理暂停。
pub(crate) fn finish_scene_editor_close(
    mut editor: ResMut<SceneEditorState>, // 消费统一的键盘或按钮关闭请求。
    session: Res<EditorAuthoringSession>, // 判断当前草稿是否存在未保存高度变化。
    stroke: Res<TerrainBrushStroke>,    // 活动笔划必须先由作者系统收口为稳定修订。
    water_stroke: Res<WaterBrushStroke>, // 普通河水笔划同时持有R8与GLB顶点局部差分。
    water_preview: Res<WaterPreviewState>, // 读取同一水域/地形修订的全层完成证据。
    water_handoff: Res<WaterFileHandoff>, // 同场景同水修订的保存后正式交接必须完整完成。
    boundary_edit: Res<BoundaryEditState>, // 未松手控制点不能带着逐帧中间态退出。
    water_network_edit: Res<WaterNetworkEditState>, // 河流或瀑布拖点同样不能丢失松手事务。
    mut creation: ResMut<SceneCreationTask>, // 保持原关闭语义：取消尚未启动的保存后创建。
    mut deletion: ResMut<SceneDeletionTask>, // 关闭时取消未确认弹框，但已确认事务继续原子完成。
    folder_persistence: Option<Res<SceneFolderPersistence>>, // 独立关闭测试可省略；正式插件等待归类写入完成。
    drag: Option<Res<SceneBrowserDragState>>,                // 活动拖拽必须先收到真实release边沿。
    mut browser: Option<ResMut<SceneBrowserState>>,          // 真正关闭时清理文件夹模态输入状态。
    mut windows: Query<&mut Window, With<PrimaryWindow>>,  // 真正关闭时才归还中文IME。
    mut close_gate: AuthoringCloseGate, // 收拢作者关闭所需资源，不超过Bevy系统参数限制。
) {
    if !editor.open || !editor.close_requested {
        return; // 没有关闭请求时稳定帧不读取草稿细节。
    }
    if session.model_vegetation_busy {
        editor.status = "正在收口模型植被笔划；完成后自动关闭编辑器".into();
        return;
    }
    if close_gate.vegetation_cross.as_ref().is_some_and(|state|state.busy()) {
        editor.status="正在收口跨网格植被笔划并发布完整草地；完成后自动关闭编辑器".into();
        return;
    }
    if close_gate.objects.as_ref().is_some_and(|state| state.busy())
        || !super::scene_objects::visibility_restored(close_gate.object_visibility.as_deref()) {
        editor.status = "正在恢复编辑器隐藏对象；完成后自动关闭编辑器".into();
        return; // 未恢复完整显示前保持原关闭门，空图和没有隐藏对象时直接通过。
    }
    if editor.scene_transition_in_progress
        || (!close_gate.world.ready() && !close_gate.world.idle_menu())
    {
        editor.status = "正在完成场景加载或切换；完成后自动关闭编辑器".to_owned();
        return; // 保留关闭意图；稳定空世界可直接收起，半份加载中的场景仍需完整采用后再放行。
    }
    if close_gate
        .boundary_numeric
        .as_ref()
        .is_some_and(|numeric| numeric.busy())
    {
        editor.cancel_close_request("空气墙数值尚未提交；请先完成或取消数值编辑，再关闭编辑器");
        return; // 修订不变的scrub中间态不能进入关闭校验或正式碰撞。
    }
    if let Some(error) = close_gate.puddles.as_ref().and_then(|state| {
        session
            .draft
            .as_ref()
            .and_then(|draft| state.preview_error_for(draft))
    }) {
        // 当前预览失败不能无限阻塞关闭。
        editor.cancel_close_request(&format!("积水预览失败：{error}；请修正或撤销后再关闭编辑器")); // 保留作者会话和完整旧画面。
        return; // 不把失败误认为就绪。
    } // 结束失败关闭门。
    if close_gate.puddles.as_ref().is_some_and(|state| {
        state.busy()
            || session
                .draft
                .as_ref()
                .is_some_and(|draft| state.preview_pending_for(draft))
    }) {
        // 不能丢弃已受理后台刷段。
        editor.status = "积水笔划正在后台完成；完成后继续关闭编辑器".into(); // 保留关闭意图。
        return; // 后续系统继续非阻塞收口。
    } // 结束积水关闭门。
    if close_gate
        .surface_task
        .as_ref()
        .is_some_and(|task| task.is_pending())
        || close_gate
            .surface_stroke
            .as_ref()
            .is_some_and(|stroke| stroke.active)
    {
        // 后台结果和本帧Presentation补笔必须同时完成。
        editor.status = "地表图层事务正在后台完成；完成后自动继续关闭".to_owned(); // 保留关闭意图和旧完整画面。
        return; // 不在像素或撤销记录尚未采用时释放作者会话。
    } // 独立关闭测试未安装地表域时沿用原关闭门。
    if editor.save_requested || session.saving {
        editor.status = save_close_wait_status(&editor.status, session.saving);
        return; // 保存快照落盘并发布热交接前不能提前解除F12暂停。
    }
    if creation.task.is_some() {
        editor.status = "新场景创建事务尚未完成；完成后自动继续关闭".to_owned();
        return; // catalog与归类联合发布必须在Tasks系统仍活跃时完整收口。
    }
    if folder_persistence
        .as_ref()
        .is_some_and(|state| state.busy())
    {
        editor.status = "场景归类保存尚未完成；完成后自动继续关闭".to_owned();
        return; // 关闭不能丢失乐观写入结果或其失败回滚。
    }
    if drag
        .as_ref()
        .is_some_and(|state| state.blocks_other_mouse())
    {
        editor.status = "请先松开正在拖拽的场景，再关闭编辑器".to_owned();
        return; // release帧必须由拖拽状态机决定归类提交或取消。
    }
    if let Some(draft) = session.draft.as_ref()
        && water_handoff.pending_for(
            &draft.scene_id,
            session.water_revision,
            session.terrain_revision,
        )
    {
        editor.status = if let Some(reason) = water_handoff.unresolved_reason_for(&draft.scene_id) {
            format!("正式场景缓存尚未恢复完整：{reason}；本次关闭已阻止，请重新保存当前场景")
        } else {
            "正在完成保存或回滚后的正式水域与摆放新修订原子热交接；完成后自动关闭编辑器".to_owned()
        };
        return; // 同场景正式缓存责任完成前继续用编辑视口遮住运行时重载中间态，作者新修订也不能绕过。
    }
    if deletion.pending_start.is_some() || deletion.task.is_some() {
        editor.status = "已确认的场景回收事务尚未完成；完成后自动继续关闭".to_owned(); // 不让关闭取消安全切换场景并把计划永久卡住。
        return; // 保持F12和驱动系统活跃，背景任务仍是非阻塞的。
    }
    if stroke.active {
        editor.status = "正在结束当前地形笔划；碰撞完成后自动关闭编辑器".to_owned(); // 不截断撤销记录。
        return; // apply_terrain_brush会在同帧主动结束笔划，下一帧继续确认。
    }
    if water_stroke.active {
        editor.status = "正在结束当前静水笔划；水面更新完成后自动关闭编辑器".to_owned(); // 静水笔刷不再自动修改河床。
        return; // apply_water_brush会先把R8与GLB顶点差分收进同一撤销命令。
    }
    if boundary_edit.dragging() {
        editor.status = "请先松开正在拖动的区域边界控制点，再关闭编辑器".to_owned(); // 物理只在松手后重建一次。
        return; // 保持F12和物理暂停，避免只更新画面点位。
    }
    if water_network_edit.dragging() {
        editor.status = "请先松开正在拖动的水网控制点，再关闭编辑器".to_owned();
        return; // 实时中间态只有松手后才拥有完整撤销记录。
    }
    if let Some(draft) = session.draft.as_ref()
        && draft.manifest.is_data_driven()
        && draft.boundaries.is_some()
    {
        let required_revision = (draft.boundaries_dirty
            || draft.boundary_catalog_registration_required)
            .then_some(session.boundary_revision);
        let current_scene = boundary::is_runtime_scene(
            close_gate.active_scene.as_deref(),
            close_gate.scene.as_deref().map(State::get),
            &draft.scene_id,
        );
        let blocked = if current_scene {
            boundary_preview_blocks_close(
                &mut editor,
                close_gate.boundary_status.as_deref(),
                &draft.scene_id,
                required_revision,
            )
        } else {
            let AuthoringCloseGate {
                boundary_validation,
                boundary_source,
                ..
            } = &mut close_gate;
            boundary_validation.blocks_close(
                &mut editor,
                draft,
                session.boundary_revision,
                boundary_source.as_deref(),
            )
        };
        if blocked {
            return; // 不同域关闭门继续按原顺序处理，只有空气墙域改为后台完成证据。
        }
    }
    let water_requirement = session.draft.as_ref().and_then(|draft| {
        (draft.manifest.is_data_driven()
            && draft.manifest.terrain.kind == SceneTerrainKind::SparseClipmap
            && draft.water.as_ref().is_some_and(|water| {
                !water.layers.is_empty() || !water.rivers.is_empty() || !water.waterfalls.is_empty()
            })) // 任意静水、河流或瀑布定义都必须等待当前水预览修订。
        .then(|| {
            (
                draft.scene_id.clone(),
                session.water_revision,
                session.terrain_revision,
            )
        })
    });
    if let Some((scene_id, water_revision, terrain_revision)) = water_requirement
        && water_preview_blocks_close(
            &mut editor,      // 失败时恢复编辑，处理中保留关闭意图。
            &water_preview,   // 使用当前联合预览完成证据。
            &scene_id,          // 只接受当前作者场景结果。
            water_revision,   // 只接受当前水域修订。
            terrain_revision, // 只接受当前配套河床修订。
        )
    {
        return; // 后台处理中继续保持联合门，成功前不暴露正式运行时的旧水/新地形组合。
    }
    if let Some(state) = close_gate.state.as_ref().filter(|state| state.enabled) {
        if state.text_input_active || state.unfinished_shape {
            editor.cancel_close_request("扩展工具仍有未完成输入，请先确认或取消");
            return;
        }
    }
    if let Some(bindings)=close_gate.content_bindings.as_deref()
        && bindings.blocks_close(&mut editor,&session) {return;}
    if let Some(state) = close_gate.state.as_mut().filter(|state| state.enabled)
        && !state.prepare_close(&session) {
        editor.status = state.status.clone();
        return;
    }
    editor.open = false; // 没有未保存地形要求或联合接管已确认，现在才隐藏F12。
    editor.empty_workspace_closed = close_gate.world.idle_menu();
    if !editor.empty_workspace_closed {
        close_gate.commands.remove_resource::<feathrise_scene_core::state::EditorStartup>();
    } // 空世界保留启动标记以禁止菜单；独立关闭状态阻止它再次自动打开F12。
    editor.close_requested = false; // 当前关闭请求已经完整消费。
    editor.cancel_pending_scene_transition(); // 保持旧语义：关闭时取消弹框和等待中的自动切换场景。
    if creation.cancel_pending_after_save() {
        editor.save_requested = false; // 自动保存尚未启动时一并取消；在途保存仍按原子事务完成。
    }
    deletion.cancel_confirmation(); // 尚未点击确认的场景删除不应在下次打开F12时重新出现。
    if let Some(browser) = browser.as_deref_mut() {
        browser.dialog = None; // 尚未确认的文件夹创建、重命名或删除没有文件副作用。
        browser.last_error = None; // 下次打开不继承旧模态校验文字。
    }
    set_ime_enabled(&mut windows, false); // 下一帧统一暂停解除前先归还系统输入法。
}

#[cfg(test)]
mod tests {
    use super::*; // 最小ECS应用验证关闭门，不启动窗口、渲染或物理插件。

    #[test]
    fn wall_close_requires_latest_revision_and_rejects_incomplete_outer_wall() {
        use feathrise_scene::{DynamicSceneBoundaryPhase, DynamicSceneBoundaryStatus};
        let map = SceneId::parse("MAP-wall-close").unwrap();
        let mut editor = SceneEditorState {
            open: true,
            ..default()
        };
        editor.request_close();
        let mut status = DynamicSceneBoundaryStatus {
            scene_id: Some(map.clone()),
            phase: DynamicSceneBoundaryPhase::Ready,
            override_revision: Some(4),
            ..default()
        };
        assert!(boundary_preview_blocks_close(
            &mut editor,
            Some(&status),
            &map,
            Some(5)
        ));
        assert!(editor.close_requested); // 旧物理已Ready仍需等待刚松手修订。
        status.override_revision = Some(5);
        status.validation_error = Some("场景外框必须闭合".to_owned());
        assert!(boundary_preview_blocks_close(
            &mut editor,
            Some(&status),
            &map,
            Some(5)
        ));
        assert!(!editor.close_requested); // 已确定的作者错误恢复编辑，不能永久等待。
        editor.request_close();
        status.validation_error = None; // 两点内部开放挡墙严格校验成功。
        assert!(!boundary_preview_blocks_close(
            &mut editor,
            Some(&status),
            &map,
            Some(5)
        ));
    }

    #[test] // 活动笔划必须先稳定结束，无地形要求时下一帧才可关闭。
    fn active_stroke_blocks_close_but_clean_session_closes_after_it_finishes() {
        let mut app = App::new(); // 只建立Update调度和本系统所需资源。
        app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active)); // 本测试已处于真实玩法场景。
        app.insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(SceneId::parse("MAP-close").unwrap()) });
        let mut editor = SceneEditorState {
            // 模拟用户已打开F12并按下关闭。
            open: true,  // 编辑器当前可见且玩法暂停。
            ..default()  // 其余状态不影响本测试。
        };
        editor.request_close(); // 登记延迟关闭意图。
        app.insert_resource(editor); // 安装待消费的编辑器状态。
        app.init_resource::<EditorAuthoringSession>(); // 空草稿表示没有未保存地形要求。
        app.init_resource::<TerrainLiveApplyState>(); // 没有候选也不影响干净会话关闭。
        app.init_resource::<WaterPreviewState>(); // 空草稿没有联合水面关闭要求。
        app.init_resource::<WaterFileHandoff>(); // 没有保存后正式水域交接。
        app.init_resource::<BoundaryEditState>(); // 空边界拖动状态不会阻止干净会话关闭。
        app.init_resource::<WaterNetworkEditState>(); // 空水网拖动状态不会阻止干净会话关闭。
        app.init_resource::<SceneCreationTask>(); // 保留正式关闭清理依赖。
        app.init_resource::<SceneDeletionTask>(); // 保留正式未确认删除清理依赖。
        app.insert_resource(TerrainBrushStroke {
            // 模拟鼠标仍处于一次地形笔划中。
            active: true, // 第一帧必须保持F12打开。
            ..default()   // 差分与压平目标保持空。
        });
        app.init_resource::<WaterBrushStroke>(); // 当前没有复合河水笔划。
        app.add_systems(Update, finish_scene_editor_close); // 注册正式联合关闭系统。

        app.update(); // 活动笔划阻止关闭。
        assert!(app.world().resource::<SceneEditorState>().open); // 暂停门仍然有效。
        app.world_mut().resource_mut::<TerrainBrushStroke>().active = false; // 模拟作者系统已经收口笔划。
        app.update(); // 空草稿不需要等待碰撞，允许完成关闭。
        let editor = app.world().resource::<SceneEditorState>(); // 读取最终关闭状态。
        assert!(!editor.open); // 第二帧已经真正退出F12。
        assert!(!editor.close_requested); // 请求必须被完整消费，不能污染下次打开。
    }

    #[test] // 河水失败修订不能同时禁止编辑并永久等待一个不会重建的任务。
    fn failed_water_preview_cancels_close_and_restores_editing() {
        let scene_id = SceneId::parse("MAP-water-close-test").expect("测试SceneId应有效"); // 建立稳定联合修订身份。
        let mut editor = SceneEditorState {
            open: true,            // F12当前可见且保持玩法暂停。
            close_requested: true, // 模拟用户已经再次按下F12。
            ..default()            // 其他UI状态不影响水面关闭门。
        };
        let preview = WaterPreviewState {
            failed_scene_id: Some(scene_id.clone()), // 当前场景预览已经明确失败。
            failed_water_revision: 7,            // 失败绑定当前水域修订。
            failed_terrain_revision: 11,         // 失败绑定同笔河床修订。
            ..default()                          // 没有完成值或在途任务。
        };

        assert!(water_preview_blocks_close(
            &mut editor, // 失败分支必须修改关闭请求。
            &preview,    // 提供确定失败证据。
            &scene_id,     // 使用完全相同场景身份。
            7,           // 使用完全相同水域修订。
            11,          // 使用完全相同河床修订。
        ));
        assert!(editor.open); // 数据无效时继续保持F12和物理暂停，不能暴露半套运行时结果。
        assert!(!editor.close_requested); // 关键回归：下一帧水笔刷重新可编辑，不再形成永久死锁。
        assert!(editor.status.contains("关闭已取消")); // 作者能直接理解为何本次没有退出以及下一步如何恢复。
    }

    #[test] // 保存落盘和热交接尚未发布前，关闭门必须保持可取消而不是提前退出。
    fn saving_transaction_waits_for_completion_and_second_f12_can_cancel() {
        let mut app = App::new(); // 最小调度足够验证保存忙碌门与F12状态机。
        app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active)); // 保存门只适用于已进入的场景。
        app.insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(SceneId::parse("MAP-close").unwrap()) });
        let mut editor = SceneEditorState {
            open: true,            // 编辑器当前可见。
            close_requested: true, // 模拟保存后立即按下第一次F12。
            ..default()            // 其余输入状态不影响保存门。
        };
        editor.tool = crate::types::EditorTool::Water; // 锁定回归场景：当前仍选择河水笔刷。
        app.insert_resource(editor); // 安装待关闭状态。
        app.insert_resource(EditorAuthoringSession {
            saving: true, // IO任务尚未完成，不能提前关闭并解除玩法暂停。
            ..default()
        });
        app.init_resource::<TerrainBrushStroke>(); // 当前没有活动地形笔划。
        app.init_resource::<WaterBrushStroke>(); // 当前没有活动河水笔划。
        app.init_resource::<TerrainLiveApplyState>(); // 文件交接尚未开始。
        app.init_resource::<WaterPreviewState>(); // 空草稿无需额外水面门。
        app.init_resource::<WaterFileHandoff>(); // 当前没有保存后正式水域交接。
        app.init_resource::<BoundaryEditState>(); // 当前没有边界拖动。
        app.init_resource::<WaterNetworkEditState>(); // 当前没有水网控制点拖动。
        app.init_resource::<SceneCreationTask>(); // 保留正式系统参数。
        app.init_resource::<SceneDeletionTask>(); // 保留正式系统参数。
        app.add_systems(Update, finish_scene_editor_close); // 运行正式关闭门。

        app.update(); // 保存事务必须阻止本帧真正关闭。
        {
            let editor = app.world().resource::<SceneEditorState>();
            assert!(editor.open); // F12界面继续存在，后台任务仍可推进。
            assert!(editor.close_requested); // 第一次F12意图保留到保存完成。
            assert_eq!(editor.tool, crate::types::EditorTool::Water); // 保存门不得切换河水工具。
            assert!(editor.status.starts_with(SAVE_CLOSE_ACTIVE_PREFIX)); // 在途保存与尚未启动的预检等待明确区分。
        }
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .toggle_close_request(); // 模拟等待期间第二次按F12。
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.open); // 取消等待后仍在同一作者会话。
        assert!(!editor.close_requested); // 自定义笔刷下一阶段重新取得显示资格。
        assert_eq!(editor.tool, crate::types::EditorTool::Water); // 河水工具选择完整保留。
    }
}
