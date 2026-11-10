//! 院落按钮、中文名称、中央框选和固定预算视口投影。
//!
//! 【功能设计】按下与拖动只记录矩形；松键后每帧最多检查 128 条摆放记录，
//! 场景、作者修订、视角或视口换代即丢弃未完成结果，直到完整扫描才公开选集。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use bevy::input_focus::{FocusCause, InputFocus}; // 官方中文输入唯一焦点。
use bevy::prelude::*; // ECS 查询、输入边沿、相机与 UI 值。
use bevy::text::{EditableText, LineHeight}; // 官方文字与固定单行度量，不接管预编辑候选。
use bevy::ui::FocusPolicy; // 选区轮廓显式穿透到唯一正式视口。
use bevy::ui::RelativeCursorPosition; // 与既有视口拾取相同的光标来源。
use feathrise_scene_content::{CourtyardPresetRegistry, MAX_COURTYARD_MEMBERS_PER_PRESET}; // 目录和成员硬上限。
use crate::ui::theme as colors; // 选中、悬停和普通状态复用既有水墨铜边。

use super::scene::{
    CourtyardAction, CourtyardButton, CourtyardModeText, CourtyardNameInput, CourtyardPresetText,
    CourtyardSelectionRegion, CourtyardSelectionText, CourtyardStatusText,
}; // 本域 UI 标记和固定按钮动作。
use super::{CourtyardSelectionScan, CourtyardUiRequest, CourtyardUiState, CourtyardViewportMode}; // 作者请求与唯一会话。
use crate::authoring::EditorAuthoringSession; // 草稿记录与修订提供分帧扫描身份。
use crate::components::{SceneEditorCamera, SceneEditorViewport}; // 中央视口与独立离屏相机。
use crate::resources::{SceneEditorState, SceneEditorViewportState}; // F12 可见门与当前 RTT 物理尺寸。
use crate::ui::placement_transform::PlacementTransformInputState; // 单体吸附武装时不能抢占同一落点。
use crate::ui::text_input::single_line_input; // 使用经过验证的官方单行中文控件。
use crate::viewport::modes::{ViewportMode, ViewportModeState}; // 武装院落时统一收口原工具。

const SCAN_RECORDS_PER_FRAME: usize = 128; // 最坏 8192 项需 64 帧，不产生单帧完整扫描。
const CLICK_PICK_HALF_SIZE_PIXELS: f32 = 5.0; // 短点击也允许选择原点附近一项。

/// BSN 只负责创建节点，原生枚举组件在实体首次建立时固定为指针穿透。
pub(super) fn setup_region_pointer_passthrough(
    mut commands: Commands, // 结构性插入只在场景创建首帧执行。
    regions: Query<Entity, Added<CourtyardSelectionRegion>>, // 唯一固定轮廓节点。
) {
    for entity in &regions {
        commands.entity(entity).insert(FocusPolicy::Pass); // 可见选区仍交由父视口接收鼠标。
    }
}

/// 切换场景、关闭或场景草稿撤离时清空仅属于旧场景的成员选集。
pub(super) fn sync_scene_identity(
    editor: Res<SceneEditorState>, // 关闭状态必须停止所有院落视口工具。
    session: Res<EditorAuthoringSession>, // 当前场景身份来自唯一作者草稿。
    registry: Option<Res<CourtyardPresetRegistry>>, // 全局预设目录首载前可不存在。
    mut state: ResMut<CourtyardUiState>, // 同步场景身份和默认目录选择。
) {
    let current = if editor.open {
        session.draft.as_ref().map(|draft| draft.scene_id.clone()) // 只有 F12 正在编辑的场景可保留选集。
    } else {
        None // 关闭 F12 后释放临时输入与跨场景选择。
    };
    if state.scene_id != current {
        state.scene_id = current; // 此后旧扫描结果不能通过场景身份门。
        state.selected_placement_ids.clear(); // 旧场景稳定 ID 不映射到新图。
        state.mode = CourtyardViewportMode::Idle; // 切换场景后不沿用已武装工具。
        state.drag_start = None; // 失效的拖拽矩形立即隐藏。
        state.drag_current = None; // 失效的拖拽终点立即隐藏。
        state.scan = None; // 未完成投影不再消费新图记录。
    }
    if let Some(registry) = registry.as_deref()
        && state
            .selected_preset_id
            .as_deref()
            .is_none_or(|id| registry.resolve(id).is_none())
    {
        state.selected_preset_id = registry.presets().first().map(|preset| preset.id.clone()); // 完整目录热更后选第一项有效组合。
    }
}

/// 固定名称输入框只在实体建立或外部值变化时重绑，打字期间绝不重置光标。
pub(super) fn read_name(
    mut commands: Commands,
    editor: Res<SceneEditorState>,         // 隐藏 F12 不读取残留文字。
    focus: Res<InputFocus>,              // 只有真实官方焦点持有者能覆盖业务名称。
    mut state: ResMut<CourtyardUiState>, // 保存已确认名字供按钮同帧读取。
    mut inputs: Query<(Entity, &mut EditableText), With<CourtyardNameInput>>, // 仅一个静态字段。
    mut bound_entity: Local<Option<Entity>>, // BSN 静态壳重建时重新初始化一次。
) {
    let Ok((entity, mut input)) = inputs.single_mut() else {
        return; // 首帧场景尚未建立。
    };
    if *bound_entity != Some(entity) {
        *input = single_line_input(&state.name, 128); // 首次不能让空 EditableText 覆盖默认名称。
        *bound_entity = Some(entity); // 后续文本由官方编辑器原位维护。
        commands.entity(entity).insert(LineHeight::Px(18.0)); // 与18px文字槽匹配，在32px外框内对称居中；只绑定一次。
        return;
    }
    if !editor.open {
        return; // 关闭时旧焦点释放由 F12 全局输入仲裁处理。
    }
    if focus.get() == Some(entity) {
        let value = input.value().to_string(); // 不包括未确认的输入法拼音。
        if state.name != value {
            state.name = value; // 本帧保存按钮即可得到最后确认字符。
        }
    } else if input.value().to_string() != state.name {
        *input = single_line_input(&state.name, 128); // 作者事务重命名后让失焦文字同步。
    }
}

/// 在目录中选择相邻预设，最多检查固定的 512 项。
fn choose_neighbor(
    state: &mut CourtyardUiState,               // 当前规范 ID 与反馈。
    registry: Option<&CourtyardPresetRegistry>, // 首载前不能猜测预设。
    delta: isize,                               // 上一个或下一个。
) {
    let Some(registry) = registry else {
        state.status = "院落预设目录仍在加载".to_owned(); // 等待 AssetServer 完整热采用。
        return;
    };
    let presets = registry.presets(); // 完整快照的固定上限列表。
    if presets.is_empty() {
        state.status = "还没有院落预设，请先框选并保存".to_owned(); // 空目录是合法初态。
        return;
    }
    let index = state
        .selected_preset_id
        .as_deref()
        .and_then(|id| presets.iter().position(|preset| preset.id == id))
        .unwrap_or(0); // 热更移除所选项后从首项继续。
    let next = (index as isize + delta).rem_euclid(presets.len() as isize) as usize; // 导航可循环。
    state.selected_preset_id = Some(presets[next].id.clone()); // 仅复制短规范 ID。
    state.status = format!("已选择院落：{}", presets[next].label); // 作者立即看到目标。
}

/// 点击左栏动作；保存和旋转只发布请求，落印和移动交给后续视口落点。
pub(super) fn handle_buttons(
    buttons: Query<(&EditorButtonClick, &CourtyardButton), Changed<EditorButtonClick>>, // 只读松开点击。
    name_input: Query<(Entity, &EditableText), With<CourtyardNameInput>>, // 同帧确认文字和组合态。
    mut focus: ResMut<InputFocus>, // 点击名称使用官方光标与 IME。
    registry: Option<Res<CourtyardPresetRegistry>>, // 目录显式选择与存在性校验。
    relocation: Res<PlacementTransformInputState>, // 旧单体移动已有独占鼠标意图。
    mut viewport_mode: ResMut<ViewportModeState>, // 院落工具属于编辑模式，单体输入由模式门收口。
    mut state: ResMut<CourtyardUiState>, // 唯一模式、选集、名称和反馈。
    mut requests: MessageWriter<CourtyardUiRequest>, // 作者后台独立消费强类型意图。
    editor: Res<SceneEditorState>,   // F12 关闭及模态输入门。
) {
    if !editor.open || editor.close_requested || editor.new_scene_dialog_open {
        return; // 不让隐藏或上层模态按钮修改会话。
    }
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue; // Hover 由表现系统独立处理。
        }
        if button.0 == CourtyardAction::FocusName {
            if let Ok((entity, _)) = name_input.single() {
                focus.set(entity, FocusCause::Navigated); // 中文编辑由官方组件接管。
            }
            continue;
        }
        if relocation.relocation_armed()
            && matches!(
                button.0,
                CourtyardAction::BoxSelect
                    | CourtyardAction::Stamp
                    | CourtyardAction::MoveGroup
                    | CourtyardAction::CopyGroup
            )
        {
            state.status = "请先完成或取消当前单体资产吸附".to_owned(); // 两种落点工具不能同时武装。
            continue;
        }
        if let Ok((entity, input)) = name_input.single()
            && focus.get() == Some(entity)
        {
            if input.is_composing() {
                state.status = "请先确认院落名称的输入法候选".to_owned(); // 未确认拼音不能成为目录名称。
                continue;
            }
            state.name = input.value().to_string(); // 失焦按钮读取最后一个已确认字符。
            focus.clear(); // 释放官方焦点给按钮或视口。
        }
        if (state.scan.is_some() || state.drag_start.is_some())
            && matches!(
                button.0,
                CourtyardAction::SaveSelection
                    | CourtyardAction::MoveGroup
                    | CourtyardAction::CopyGroup
                    | CourtyardAction::RotateLeft
                    | CourtyardAction::RotateRight
            )
        {
            state.status = "请等待当前框选完成，再操作院落选集".to_owned(); // 旧选集不能冒充尚未完成的新框选。
            continue;
        }
        match button.0 {
            CourtyardAction::FocusName => unreachable!(), // 已在上方处理。
            CourtyardAction::BoxSelect => {
                state.mode = CourtyardViewportMode::BoxSelect; // 下一次拖拽归院落独占。
                state.drag_start = None; // 新拖拽不能带旧矩形。
                state.drag_current = None; // 新拖拽不能带旧终点。
                state.scan = None; // 新请求替代旧扫描。
                state.status = "在中央视口按住左键拖出选框；按 Esc 可取消".to_owned(); // 明确下一步。
            }
            CourtyardAction::ClearSelection => {
                state.selected_placement_ids.clear(); // 清空稳定 ID，不改变场景内容。
                state.mode = CourtyardViewportMode::Idle; // 旧整体操作失去对象后退出。
                state.scan = None; // 未完成结果不能复活旧选择。
                state.drag_start = None; // 隐藏选框。
                state.drag_current = None; // 隐藏选框。
                state.status = "已清空院落多选".to_owned(); // 明确无作者改动。
            }
            CourtyardAction::SaveSelection => {
                if state.selected_placement_ids.is_empty() {
                    state.status = "请先框选至少一个摆放对象".to_owned(); // 空组合不能写入目录。
                } else if state.name.trim().is_empty() {
                    state.status = "请输入院落名称".to_owned(); // 名称不可为空。
                } else {
                    requests.write(CourtyardUiRequest::SaveSelection {
                        name: state.name.trim().to_owned(), // 作者后台仍检查 Unicode 长度与冲突。
                        placement_ids: state.selected_placement_ids.clone(), // 至多 256 项。
                    });
                    state.status = "正在保存所选院落组合…".to_owned(); // 等待作者写入和热采用回执。
                }
            }
            CourtyardAction::PreviousPreset => choose_neighbor(&mut state, registry.as_deref(), -1), // 浏览上一个。
            CourtyardAction::NextPreset => choose_neighbor(&mut state, registry.as_deref(), 1), // 浏览下一个。
            CourtyardAction::Stamp => {
                if state
                    .selected_preset_id
                    .as_deref()
                    .and_then(|id| registry.as_deref()?.resolve(id))
                    .is_none()
                {
                    state.status = "请先选择有效的院落预设".to_owned(); // 坏热更不盖印失效项。
                } else {
                    state.mode = CourtyardViewportMode::Stamp; // 作者视口落点处理整组印章。
                    state.scan = None; // 从框选切到盖印时丢弃未完成选择。
                    state.drag_start = None; // 旧拖拽不再显示。
                    state.drag_current = None; // 旧拖拽不再显示。
                    state.status = "点击中央视口盖印院落；左转和右转可调整朝向".to_owned(); // 给出后续动作。
                }
            }
            CourtyardAction::MoveGroup | CourtyardAction::CopyGroup => {
                if state.selected_placement_ids.is_empty() {
                    state.status = "请先框选目标院落的成员".to_owned(); // 后台需由选中成员反查副本。
                } else {
                    state.mode = if button.0 == CourtyardAction::MoveGroup {
                        CourtyardViewportMode::MoveGroup // 下一次落点移动整个副本。
                    } else {
                        CourtyardViewportMode::CopyGroup // 下一次落点复制整个副本。
                    };
                    state.scan = None; // 整体变换以已完整发布的选集为准。
                    state.drag_start = None; // 旧拖拽不再显示。
                    state.drag_current = None; // 旧拖拽不再显示。
                    state.status = "点击中央视口指定整个院落的新位置".to_owned(); // 后台校验边界和碰撞。
                }
            }
            CourtyardAction::RotateLeft | CourtyardAction::RotateRight => {
                let turns = if button.0 == CourtyardAction::RotateRight {
                    1
                } else {
                    -1
                }; // 统一九十度整数旋转。
                if state.mode == CourtyardViewportMode::Stamp {
                    state.quarter_turns = (state.quarter_turns + turns).rem_euclid(4); // 印章预览朝向只改会话态。
                    state.status = format!("院落印章朝向 {}°", state.quarter_turns * 90); // 盖印时供作者反馈。
                } else if state.selected_placement_ids.is_empty() {
                    state.status = "请先框选院落成员，再整体旋转".to_owned(); // 空选择不发请求。
                } else {
                    requests.write(CourtyardUiRequest::RotateSelection {
                        placement_ids: state.selected_placement_ids.clone(), // 后台验证为同一个完整副本。
                        quarter_turns: turns,                                // 请求量始终为一刻度。
                    });
                    state.status = "正在检查并旋转整个院落…".to_owned(); // 作者事务稍后回填具体结果。
                }
            }
            CourtyardAction::CancelMode => {
                state.mode = CourtyardViewportMode::Idle; // 归还普通 Inspect 与建筑点放。
                state.drag_start = None; // 丢弃未完成拖拽。
                state.drag_current = None; // 清除选框。
                state.scan = None; // 丢弃未采用扫描。
                state.status = "已取消院落视口工具".to_owned(); // 多选保留供后续使用。
            }
        }
        if state.mode != CourtyardViewportMode::Idle {
            viewport_mode.mode = ViewportMode::Edit; // 盖印保持编辑高亮，模式门暂时让出单体工具。
        }
    }
}

/// 归一化视口点转换为 [0, 1]，允许拖出画面后安全钳制矩形。
fn normalized_point(cursor: &RelativeCursorPosition) -> Option<Vec2> {
    cursor
        .normalized
        .map(|point| (point + Vec2::splat(0.5)).clamp(Vec2::ZERO, Vec2::ONE)) // 与现有 RTT 射线的坐标约定一致。
}

/// Cursor 链在普通拾取前调用；只持有武装模式的左键拖拽。
#[allow(clippy::too_many_arguments)] // 五项身份共同防止 UI 和视口输入串用。
pub(crate) fn viewport_input(
    mouse: Res<ButtonInput<MouseButton>>, // 拖拽从按下边沿持续到松键。
    keyboard: Res<ButtonInput<KeyCode>>,  // Esc 取消当前视口工具。
    focus: Res<InputFocus>,               // 中文字段编辑时 Esc 归文字编辑器。
    name_input: Query<Entity, With<CourtyardNameInput>>, // 焦点仅检查固定一个字段。
    editor: Res<SceneEditorState>,          // F12、模态和保存期间拒绝新扫描。
    modes: Res<ViewportModeState>,        // 中央视口浮动控件拥有指针时不启动框选。
    session: Res<EditorAuthoringSession>, // 冻结当前场景与修订。
    viewport: Option<Res<SceneEditorViewportState>>, // RTT 物理尺寸来自正式视口资源。
    cursor: Query<&RelativeCursorPosition, With<SceneEditorViewport>>, // 跟普通拾取同源坐标。
    camera: Query<&Transform, With<SceneEditorCamera>>, // 跨帧扫描冻结相机姿态。
    mut state: ResMut<CourtyardUiState>,  // 当前拖拽和选集会话。
) {
    if state.mode == CourtyardViewportMode::Idle {
        return; // 普通操作完全不受额外逐帧工作影响。
    }
    if keyboard.just_pressed(KeyCode::Escape)
        && !name_input.iter().any(|entity| focus.get() == Some(entity))
    {
        state.mode = CourtyardViewportMode::Idle; // Esc 取消尚未执行的院落落点。
        state.drag_start = None; // 隐藏本次矩形。
        state.drag_current = None; // 隐藏本次矩形。
        state.scan = None; // 未完成结果绝不晚到采用。
        state.status = "已取消院落视口工具".to_owned(); // 多选仍可继续使用。
        return;
    }
    if state.mode != CourtyardViewportMode::BoxSelect {
        return; // 盖印、移动和复制由作者模块读取同一模式与世界落点。
    }
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || editor.suppress_world_pick_this_frame
        || session.saving
    {
        return; // 关闭、保存与弹框关闭同帧都不能建立或提交拖拽。
    }
    let Some(viewport) = viewport else {
        return; // 首帧未建立离屏相机。
    };
    let Ok(cursor) = cursor.single() else {
        return; // 中央UI尚未就绪。
    };
    if mouse.just_pressed(MouseButton::Left) && cursor.cursor_over() && !modes.pointer_over_controls
    {
        if let Some(point) = normalized_point(cursor) {
            state.drag_start = Some(point); // 按下只存常数大小坐标。
            state.drag_current = Some(point); // 先显示零面积轮廓。
            state.scan = None; // 后一次选择明确取代前一轮。
        }
    }
    if state.drag_start.is_some() && mouse.pressed(MouseButton::Left) {
        if let Some(point) = normalized_point(cursor) {
            state.drag_current = Some(point); // 拖动只更新UI轮廓，不扫描场景记录。
        }
    }
    if !mouse.just_released(MouseButton::Left) {
        return; // 只有松手建立有界扫描。
    }
    let Some(start) = state.drag_start.take() else {
        return; // 按下不在中央视口，松手不能反向选取。
    };
    let end = normalized_point(cursor)
        .or(state.drag_current.take())
        .unwrap_or(start); // 越界松手沿用最后有效终点。
    state.drag_current = None; // 隐藏框线，后续文字显示扫描进度。
    let Some(draft) = session.draft.as_ref() else {
        state.status = "当前没有可编辑场景".to_owned(); // 没有记录源不能启动扫描。
        return;
    };
    let Ok(transform) = camera.get(viewport.camera) else {
        state.status = "院落框选相机尚未就绪".to_owned(); // 相机身份须与 RTT 一致。
        return;
    };
    let size = viewport.render_size.as_vec2(); // 同现有射线使用 RTT 物理像素。
    let a = start * size; // 起点投影到相机像素。
    let b = end * size; // 终点投影到相机像素。
    let mut min = a.min(b); // 允许任意拖动方向。
    let mut max = a.max(b); // 允许任意拖动方向。
    if (max.x - min.x).abs() < CLICK_PICK_HALF_SIZE_PIXELS * 2.0 {
        min.x -= CLICK_PICK_HALF_SIZE_PIXELS; // 短点击仍能选到原点附近对象。
        max.x += CLICK_PICK_HALF_SIZE_PIXELS; // 扩展只影响本次矩形。
    }
    if (max.y - min.y).abs() < CLICK_PICK_HALF_SIZE_PIXELS * 2.0 {
        min.y -= CLICK_PICK_HALF_SIZE_PIXELS; // 横线拖拽也有可见选取宽度。
        max.y += CLICK_PICK_HALF_SIZE_PIXELS; // 扩展只影响本次矩形。
    }
    state.scan = Some(CourtyardSelectionScan {
        scene_id: draft.scene_id.clone(),      // 冻结场景身份。
        revision: session.revision,        // 冻结作者内容修订。
        camera_transform: *transform,      // 相机变动时放弃跨帧组合。
        render_size: viewport.render_size, // RTT尺寸变动时放弃旧矩形。
        min,                               // 左上有效像素。
        max,                               // 右下有效像素。
        cursor: 0,                         // 下一 Tasks 阶段从首记录开始。
        ids: Vec::new(),                   // 逐帧积累，未完成前不公开。
    });
    state.status = "正在框选摆放记录…".to_owned(); // 低帧率时仍能理解等待原因。
}

/// 判断记录原点是否落入屏幕矩形；只接受可见相机前方投影。
fn origin_in_rect(
    camera: &Camera,
    transform: &Transform,
    origin: Vec3,
    scan: &CourtyardSelectionScan,
) -> bool {
    camera
        .world_to_viewport(&GlobalTransform::from(*transform), origin)
        .is_ok_and(|screen| {
            screen.x >= scan.min.x
                && screen.x <= scan.max.x
                && screen.y >= scan.min.y
                && screen.y <= scan.max.y
        }) // 原点来自作者记录，不扫描运行时实体树。
}

/// 固定预算扫描完整场景摆放列表；全部成功后一次性公开选中 ID。
pub(super) fn advance_selection_scan(
    mut state: ResMut<CourtyardUiState>,  // 唯一在途结果与公开选集。
    session: Res<EditorAuthoringSession>, // 作者记录和当前修订。
    viewport: Option<Res<SceneEditorViewportState>>, // 相机与 RTT 尺寸身份。
    cameras: Query<(&Camera, &Transform), With<SceneEditorCamera>>, // 只投影作者记录的世界原点。
) {
    let Some(mut scan) = state.scan.take() else {
        return; // 稳定帧没有任何摆放扫描。
    };
    let Some(draft) = session.draft.as_ref() else {
        state.status = "院落框选已取消：场景草稿已离开".to_owned(); // 不使用旧快照。
        state.mode = CourtyardViewportMode::Idle; // 明确归还视口。
        return;
    };
    let Some(viewport) = viewport else {
        state.status = "院落框选已取消：视口已关闭".to_owned(); // 无相机时安全失败。
        state.mode = CourtyardViewportMode::Idle; // 明确归还视口。
        return;
    };
    let Ok((camera, transform)) = cameras.get(viewport.camera) else {
        state.status = "院落框选已取消：相机已更换".to_owned(); // 不混用不同相机。
        state.mode = CourtyardViewportMode::Idle; // 明确归还视口。
        return;
    };
    if state.scene_id.as_ref() != Some(&scan.scene_id)
        || draft.scene_id != scan.scene_id
        || session.revision != scan.revision
        || viewport.render_size != scan.render_size
        || *transform != scan.camera_transform
        || state.mode != CourtyardViewportMode::BoxSelect
    {
        state.status = "院落框选已取消：场景、作者内容或视角发生变化".to_owned(); // 迟到结果不得混用。
        state.mode = CourtyardViewportMode::Idle; // 允许作者重新框选。
        return;
    }
    let records = &draft.placements.placements; // 唯一权威的显式摆放记录数组。
    let end = scan
        .cursor
        .saturating_add(SCAN_RECORDS_PER_FRAME)
        .min(records.len()); // 固定本帧检查量。
    for record in &records[scan.cursor..end] {
        if origin_in_rect(
            camera,
            transform,
            Vec3::from_array(record.translation),
            &scan,
        ) {
            if scan.ids.len() == MAX_COURTYARD_MEMBERS_PER_PRESET {
                state.status = format!(
                    "框选超过 {} 个成员，请缩小范围",
                    MAX_COURTYARD_MEMBERS_PER_PRESET
                ); // 保留旧选集，不静默截断。
                state.mode = CourtyardViewportMode::Idle; // 本次扫描无法生成合法预设。
                return;
            }
            scan.ids.push(record.id.clone()); // 只复制受硬上限约束的稳定 ID。
        }
    }
    scan.cursor = end; // 下帧从未处理后缀继续。
    if end == records.len() {
        state.selected_placement_ids = scan.ids; // 完整结果一次发布，不显示半成品。
        state.mode = CourtyardViewportMode::Idle; // 框选完成后返回普通视口选择。
        state.status = format!(
            "已框选 {} 个摆放对象，可保存组合或选择现有副本整体编辑",
            state.selected_placement_ids.len()
        ); // 可见结果。
    } else {
        state.status = format!("正在框选摆放记录 {end}/{}…", records.len()); // 显示分帧处理进度。
        state.scan = Some(scan); // 单在途任务，下一帧继续。
    }
}

/// 按值同步院落名称之外的固定文本，目录热更不会留下旧预设名称。
pub(super) fn sync_panel_text(
    state: Res<CourtyardUiState>,                   // 本帧最终会话状态。
    registry: Option<Res<CourtyardPresetRegistry>>, // 全局目录当前完整快照。
    mut texts: Query<
        (
            &mut Text,
            Option<&CourtyardSelectionText>,
            Option<&CourtyardPresetText>,
            Option<&CourtyardModeText>,
            Option<&CourtyardStatusText>,
        ),
        Or<(
            With<CourtyardSelectionText>,
            With<CourtyardPresetText>,
            With<CourtyardModeText>,
            With<CourtyardStatusText>,
        )>,
    >,
) {
    let selected = format!(
        "当前框选：{} 项（上限 {}）",
        state.selected_placement_ids.len(),
        MAX_COURTYARD_MEMBERS_PER_PRESET
    ); // 用户能判断合法容量。
    let preset = match (registry.as_deref(), state.selected_preset_id.as_deref()) {
        (None, _) => "组合预设：加载中…".to_owned(), // 目录首载前不猜测。
        (Some(registry), Some(id)) => registry.resolve(id).map_or_else(
            || "组合预设：当前项已失效".to_owned(),
            |preset| format!("组合预设：{}（{} 项）", preset.label, preset.members.len()),
        ), // 名称和实际成员数来自同一修订。
        (Some(_), None) => "组合预设：暂无，先保存一个院落".to_owned(), // 合法空目录。
    };
    let mode = match state.mode {
        CourtyardViewportMode::Idle => "普通选择", // 普通 Inspect 拾取恢复。
        CourtyardViewportMode::BoxSelect => "框选中", // 左键属于组合选择。
        CourtyardViewportMode::Stamp => "盖印中",  // 左键属于作者落印。
        CourtyardViewportMode::MoveGroup => "整体移动中", // 左键属于整组变换。
        CourtyardViewportMode::CopyGroup => "整体复制中", // 左键属于整组复制。
    };
    let mode = format!(
        "当前：{mode} · 印章 {}°",
        state.quarter_turns.rem_euclid(4) * 90
    ); // 统一显示正角度。
    let status = match registry.as_deref() {
        None => format!("院落预设目录加载中…\n{}", state.status),
        Some(registry) if registry.last_error.is_some() => format!(
            "{}\n院落目录更新失败，仍沿用上一有效快照：{}",
            state.status,
            registry.last_error.as_deref().unwrap_or_default(),
        ),
        Some(_) => state.status.clone(),
    };
    for (mut text, selection_marker, preset_marker, mode_marker, status_marker) in &mut texts {
        let next = if selection_marker.is_some() {
            &selected
        } else if preset_marker.is_some() {
            &preset
        } else if mode_marker.is_some() {
            &mode
        } else if status_marker.is_some() {
            &status
        } else {
            continue; // 查询过滤保证不可达。
        };
        if text.0 != *next {
            text.0.clone_from(next); // 稳定帧不重复标记 Text Changed。
        }
    }
}

/// 院落模式按钮使用已有水墨铜金的选中与悬停反馈。
pub(super) fn sync_button_styles(
    state: Res<CourtyardUiState>, // 当前武装模式决定保持选中态。
    mut buttons: Query<(
        &UiInteraction,
        &CourtyardButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>, // 固定少量按钮。
) {
    for (interaction, action, mut background, mut border) in &mut buttons {
        let selected = matches!(
            (action.0, state.mode),
            (CourtyardAction::BoxSelect, CourtyardViewportMode::BoxSelect)
                | (CourtyardAction::Stamp, CourtyardViewportMode::Stamp)
                | (CourtyardAction::MoveGroup, CourtyardViewportMode::MoveGroup)
                | (CourtyardAction::CopyGroup, CourtyardViewportMode::CopyGroup)
        ); // 仅当前视口模式保持亮底。
        let fill = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED, // 按下优先于模式选中。
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,   // 悬停即时反馈。
            (UiInteraction::None, true) => colors::SELECT_BG,         // 已武装工具保持亮底。
            (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL, // 普通暗墨按钮。
        };
        if background.0 != fill {
            background.0 = fill; // 颜色未变化时不触发渲染更新。
        }
        let outline = BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        }); // 活动按钮强化铜边。
        if *border != outline {
            *border = outline; // 稳定边框保持原位。
        }
    }
}

/// 选框只把两个归一化点映射到视口子节点百分比，不抢输入。
pub(super) fn sync_region(
    state: Res<CourtyardUiState>, // 当前拖拽起点和终点。
    mut regions: Query<&mut Node, With<CourtyardSelectionRegion>>, // 只有中央一个叠加节点。
) {
    let Ok(mut node) = regions.single_mut() else {
        return; // 静态 UI 尚未建立。
    };
    let (Some(start), Some(end)) = (state.drag_start, state.drag_current) else {
        if node.display != Display::None {
            node.display = Display::None; // 扫描阶段和普通模式不显示旧框。
        }
        return;
    };
    let min = start.min(end); // 支持从任意象限拖动。
    let max = start.max(end); // 支持从任意象限拖动。
    node.display = Display::Flex; // 有按下身份时才显示。
    node.left = Val::Percent(min.x * 100.0); // 相对 RTT 画面位置。
    node.top = Val::Percent(min.y * 100.0); // 相对 RTT 画面位置。
    node.width = Val::Percent((max.x - min.x) * 100.0); // 选框宽度随鼠标变化。
    node.height = Val::Percent((max.y - min.y) * 100.0); // 选框高度随鼠标变化。
}

#[cfg(test)]
mod tests {
    use super::*; // 使用真实系统输入门，不创建窗口或渲染。

    #[test]
    fn escape_cancels_armed_stamp_without_viewport_or_name_focus() {
        let mut app = App::new(); // 不安装正式摄像机，只运行院落输入系统。
        app.init_resource::<ButtonInput<MouseButton>>() // 左键保持未按下。
            .init_resource::<ButtonInput<KeyCode>>() // Esc 由 Bevy 输入边沿提供。
            .init_resource::<InputFocus>() // 无官方名称字段焦点。
            .init_resource::<SceneEditorState>() // F12 状态由测试设为打开。
            .init_resource::<EditorAuthoringSession>() // 测试不需要场景内容。
            .init_resource::<ViewportModeState>() // 浮层控件默认不占鼠标。
            .init_resource::<CourtyardUiState>() // 武装印章后模拟 Esc。
            .add_systems(Update, viewport_input); // 使用实际视口模式输入门。
        app.world_mut().resource_mut::<SceneEditorState>().open = true; // 模拟 F12 可见。
        app.world_mut().resource_mut::<CourtyardUiState>().mode = CourtyardViewportMode::Stamp; // 武装一次盖印。
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape); // 建立取消键边沿。
        app.update(); // Esc 在相机不存在时也应先取消模式。
        assert_eq!(
            app.world().resource::<CourtyardUiState>().mode,
            CourtyardViewportMode::Idle
        ); // 旧盖印不能在下一次点击执行。
    }

    #[test]
    fn leaving_scene_clears_multiselection_and_pending_mode() {
        let mut app = App::new(); // 只运行场景身份同步，不依赖 AssetServer。
        app.init_resource::<SceneEditorState>() // 默认编辑器关闭。
            .init_resource::<EditorAuthoringSession>() // 无当前场景草稿。
            .init_resource::<CourtyardUiState>() // 写入旧场景的选集。
            .add_systems(Update, sync_scene_identity); // 使用正式场景身份门。
        {
            let mut state = app.world_mut().resource_mut::<CourtyardUiState>(); // 模拟旧场景选中院落成员。
            state.scene_id =
                Some(feathrise_scene_content::SceneId::parse("MAP-old-courtyard").unwrap()); // 冻结旧场景身份。
            state.selected_placement_ids.push("old-member".to_owned()); // 旧摆放 ID 不应传给新图。
            state.mode = CourtyardViewportMode::MoveGroup; // 旧落点已武装但未执行。
        }
        app.update(); // 编辑器关闭使当前场景身份为 None。
        let state = app.world().resource::<CourtyardUiState>(); // 读取最终输入所有权。
        assert!(state.selected_placement_ids.is_empty()); // 旧成员不会被新图误用。
        assert_eq!(state.mode, CourtyardViewportMode::Idle); // 旧移动请求不能晚到新图。
    }
}
