//! 资产预览鼠标导航与增量视图应用；不创建新View、不扫描模型或复制图片。

use feathrise_editor_ui::UiInteraction;
use bevy::camera::Projection; // 仅修改已存在的资产预览相机。
use bevy::ecs::system::SystemParam; // 将只读作者输入门组合成一个明确上下文。
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel}; // 原始输入按当前预览画布独占消费。
use bevy::prelude::*; // ECS、UI布局和鼠标按钮。
use bevy::ui::{RelativeCursorPosition, Val2}; // 画布真实命中与不影响布局的图片平移。
use bevy::window::PrimaryWindow; // 非本窗口的滚轮不得影响F12预览。
use feathrise_editor_ui::ActiveNumericInputScrub; // 当前数值拖动优先，不等待旧帧鼠标门。

use super::model::canvas_aspect_ratio; // 无布局时采用与自动取景相同的安全比例。
use super::resources::AssetPreviewNavigation; // 导航独立于异步资产状态。
use super::types::*; // 专用画布、按钮和加载阶段。
use super::view_math::*; // 轨道、缩放、平移与投影的固定成本数学。
use crate::authoring::TerrainExternalChangeState; // Blender冲突模态期间预览也必须让出输入。
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneDeletionTask, SceneEditorNumericScrubMouseGuard,
    SceneEditorState,
}; // 所有编辑器模态和已存在的鼠标所有权均优先于新预览手势。
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState}; // 隐藏资产页不允许后台接收鼠标。

/// 只读上下文不把预览导航扩张为任何作者事务。
#[derive(SystemParam)] // 保持输入系统参数数量和互斥边界清晰。
pub(super) struct PreviewInputContext<'w, 's> {
    animation: Option<Res<'w, super::animation::AnimationEditorState>>,
    rigging: Option<Res<'w, super::rigging::RiggingState>>,
    external: Option<Res<'w, crate::game_authoring::ExternalAssetPreviewRequest>>,
    tool_hosts: Query<'w, 's, &'static crate::game_authoring::DomainToolPreviewHost>,
    editor: Res<'w, SceneEditorState>, // F12显隐、所选路径与新场景弹框。
    sidebar: Res<'w, SceneEditorSidebarState>, // 只有当前资产页或作者场景页可操作。
    layout: Option<Res<'w, crate::resources::SceneEditorLayoutState>>, // 收起右栏后旧画布矩形不能继续捕获导航。
    central: Option<Res<'w, super::AssetPreviewWorkspace>>,
    workspace: Option<Res<'w, crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    settings: Option<Res<'w, crate::resources::SceneEditorSettingsState>>,
    numeric: Res<'w, SceneEditorNumericScrubMouseGuard>, // 和弦及release帧不能开始第二手势。
    active_numeric: Res<'w, ActiveNumericInputScrub>, // 本帧刚开始的数值拖动也立即生效。
    scene_drag: Res<'w, SceneBrowserDragState>, // 场景归类的候选与活动拖动都不能被抢走。
    browser: Res<'w, SceneBrowserState>, // 文件夹模态阻止画布点透。
    deletion: Res<'w, SceneDeletionTask>, // 删除确认期间不能操作底下预览。
    conflict: Res<'w, TerrainExternalChangeState>, // 外部地形冲突同样是全局作者模态。
    windows: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>, // 丢失焦点时取消当前捕获。
}

/// 只有在画布内开始且没有其他按键所有者时，才允许建立新的拖动。
fn begin_preview_drag(image: bool, mouse: &ButtonInput<MouseButton>) -> Option<AssetPreviewDrag> {
    let buttons = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]; // 固定三枚作者鼠标键。
    for button in buttons {
        if !mouse.just_pressed(button) {
            continue; // 从外部按住再移入画布不得重新捕获。
        } // 和弦必须由最初按下的功能持续持有。
        if buttons
            .into_iter()
            .any(|other| other != button && (mouse.pressed(other) || mouse.just_released(other)))
        {
            return None; // 数字框、场景相机或笔刷的另一枚键尚未收口。
        } // 图片支持三键平移，模型保持右转中移。
        return match (image, button) {
            (true, _) => Some(AssetPreviewDrag::ImagePan(button)), // 图片没有旋转模型的语义。
            (false, MouseButton::Right) => Some(AssetPreviewDrag::ModelOrbit), // 对齐场景视口。
            (false, MouseButton::Middle) => Some(AssetPreviewDrag::ModelPan), // 对齐场景视口。
            _ => None, // 模型左键不修改场景或选择场景对象。
        }; // 只冻结一次，不在移动过程中重新猜用途。
    } // 最多检查三个按键。
    None // 普通悬停不取得拖动所有权。
}

/// 每帧读取并消费原始事件；禁用帧也不积压鼠标增量。
pub(super) fn control_asset_preview(
    mut motions: MessageReader<MouseMotion>, // 与场景视口使用各自游标，绝不清空全局事件。
    mut wheels: MessageReader<MouseWheel>,   // Line与Pixel单位先统一再应用倍率。
    mouse: Res<ButtonInput<MouseButton>>,    // 按下起点、持续捕获和松开边沿。
    context: PreviewInputContext,            // 当前F12、模态、窗口与其他鼠标所有权。
    state: Res<AssetPreviewState>,           // 只允许完整加载成功的真实资产交互。
    mut navigation: ResMut<AssetPreviewNavigation>, // 本系统只改只读预览的临时状态。
    canvases: Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<AssetPreviewCanvas>,
    >, // 同时尊重祖先裁剪与前方模态遮挡。
    image_nodes: Query<&ComputedNode, With<AssetPreviewImage>>, // 未缩放图像的实际布局尺寸。
    buttons: Query<
        (&feathrise_editor_ui::EditorButtonClick, &AssetPreviewAction),
        (With<AssetPreviewNavigationButton>, Changed<feathrise_editor_ui::EditorButtonClick>),
    >, // 只消费缩放工具，子资产按钮由独立临时会话系统处理。
    axes: super::orientation::AxisInput,
    view_toolbar: super::view_toolbar::Input,
    mut shading: super::preview_shading::Input,
) {
    let motion = motions
        .read()
        .filter(|event| event.delta.is_finite())
        .fold(Vec2::ZERO, |sum, event| sum + event.delta); // 不乘帧时间。
    let window = context.windows.single().ok(); // 缺失唯一窗口时失败关闭输入。
    let wheel = wheels
        .read()
        .filter(|event| {
            window.is_some_and(|(entity, _)| entity == event.window) && event.y.is_finite()
        })
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y,         // 鼠标整档输入。
            MouseScrollUnit::Pixel => event.y / 27.0, // 触控板像素沿用编辑器每行27px口径。
        })
        .sum::<f32>(); // 即使下面return，本帧事件也不会留到解锁后补执行。
    let mut next = *navigation; // 小型值复制，最后只在真实变化时发布。
    next.wheel_captured = false; // 本帧重新验证可交互画布命中。
    next.mouse_captured = false; // release帧只发布一次，后续由统一鼠标门延续。
    let ready_image = matches!(
        &state.content,
        AssetPreviewContent::Image {
            phase: AssetPreviewPhase::Ready,
            ..
        }
    ); // 尚未加载的图片不可缩放。
    let ready_model = matches!(
        &state.content,
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Ready,
            ..
        }
    ); // 隔离中的模型不可操控。
    let central = context.central.as_ref().zip(context.workspace.as_ref()).is_some_and(|(media, workspace)| {
        media.active(workspace.active_tab) && media.expanded.as_ref().is_some_and(|entry| state.selected_path.as_deref() == Some(entry.path.as_str()))
    }); // 使用当前标签和完整来源身份，不依赖上一帧中央显隐缓存。
    let tool_page = context.external.as_ref().filter(|external| external.active)
        .and_then(|external| external.host).and_then(|host| context.tool_hosts.get(host).ok());
    let preview_page_visible = tool_page.map_or(context.sidebar.right == EditorRightSidebarTab::Scene,
        |host| context.sidebar.tool_page_visible(crate::ui::sidebars::EditorLeftSidebarTab::Domain(host.0)));
    let enabled = context.editor.open
        && (central || context.layout.as_ref().is_none_or(|layout| layout.inspector_shown()))
        && (central || if state.external_preview.is_some() { preview_page_visible } else { context.sidebar.right == EditorRightSidebarTab::Assets }) // 作者预览按其宿主页判断，隐藏页不接收导航输入。
        && (central || state.selected_path == context.editor.selected_asset || state.external_preview.is_some()) // 中央来源固定，不要求覆盖右侧资产选择。
        && (ready_image || ready_model)
        && (!central || context.central.as_ref().is_none_or(|media| !media.restore_view)) // 不把恢复中的输入再覆盖为旧快照。
        && !context.editor.close_requested
        && context.editor.pending_scene_transition.is_none()
        && context.settings.as_ref().is_none_or(|state| !state.open)
        && !context.editor.new_scene_dialog_open
        && context.browser.dialog.is_none()
        && context.deletion.confirmation.is_none()
        && !context.conflict.modal_open()
        && !context.scene_drag.blocks_other_mouse()
        && context.scene_drag.press.is_none()
        && context.active_numeric.entity.is_none()
        && window.is_some_and(|(_, window)| window.focused); // 关闭、换选择、失焦或模态都取消捕获。
    let canvas = canvases
        .single()
        .ok()
        .filter(|(node, _, _)| node.size().is_finite() && node.size().min_element() > 0.0); // 首帧布局未就绪时不做除法。
    if !enabled || canvas.is_none() || context.animation.as_ref().is_some_and(|state| state.owns_mouse())
        || context.rigging.as_ref().is_some_and(|state| state.owns_mouse())
        || (context.numeric.blocked() && next.drag.is_none()) {
        next.drag = None; // 输入资格失效立即停止导航，不沿用上一选择的手势。
        navigation.set_if_neq(next); // 稳定禁用帧不触发Changed。
        return; // 原始事件与Changed查询均已由本次系统tick消费。
    } // 当前持有的预览拖动可越过它自己发布的统一鼠标门。
    let (canvas, cursor, interaction) = canvas.expect("已确认有效的预览画布必须存在"); // 固定唯一画布。
    let hovered = cursor.cursor_over()
        && *interaction != UiInteraction::None
        && !axes.cursor_over()
        && (!central || !shading.pointer.cursor_over());
    let image_size = image_nodes
        .single()
        .map(ComputedNode::size)
        .unwrap_or(Vec2::ZERO); // 图片加载后已有原比例布局。
    next.view.image_pan = clamp_image_pan(
        next.view.image_pan,
        next.view.zoom,
        image_size,
        canvas.size(),
    ); // resize后先与实际已显示偏移对齐。
    let previous_drag = next.drag; // 首次按下帧不误用按下前累计的鼠标位移。
    if next.drag.is_none() && hovered && !context.numeric.blocked() {
        next.drag = begin_preview_drag(ready_image, &mouse); // 必须从画布内部按下开始。
    } // 未悬停时只能继续既有捕获。
    next.mouse_captured = next.drag.is_some(); // 极短点击同帧松开也能阻止场景补触发。
    next.wheel_captured = hovered; // 只阻止当前画布内滚轮，标题和列表继续正常滚动。
    let mut zoom_lines = if hovered || next.drag.is_some() {
        wheel
    } else {
        0.0
    }; // 捕获中滚轮仍只属于预览。
    if next.drag.is_none() && !context.numeric.blocked() {
        if axes.apply(&state, &mut next.view) {
            next.mouse_captured = true; // 方向球释放只调整资产相机，不落入地图拾取。
        }
        if central && ready_model && state.render_visible && shading.apply() {
            next.mouse_captured = true; // 四球点击仅更新资产着色，统一占有松开帧。
        }
        if central && view_toolbar.apply(&state, context.rigging.as_deref(), &mut next.view) {
            next.mouse_captured = true; // 复用同一释放帧门；不把资产视图动作分派到地图。
        }
        for (interaction, action) in &buttons {
            if !interaction.is_clicked() {
                continue; // 悬停与按住只影响按钮配色。
            } // 按钮无需移动鼠标即可缩放或恢复取景。
            match action {
                AssetPreviewAction::Reset => next.view.reset_camera(), // 只还原相机与图片的临时偏移。
                AssetPreviewAction::ZoomIn => zoom_lines += 1.25_f32.ln() / 0.12, // 一次按钮约放大25%。
                AssetPreviewAction::ZoomOut => zoom_lines -= 1.25_f32.ln() / 0.12, // 与放大按钮精确互逆。
                _ => continue, // 查询marker保证正常不会遇到子资产动作，防御测试注入错误组合。
            } // 所有按钮都不修改Asset、场景或撤销栈。
        } // 固定三个按钮，稳定帧没有列表扫描。
    } // 拖动中的和弦不能触发工具条动作。
    if zoom_lines != 0.0 {
        let zoom = zoom_after_scroll(next.view.zoom, zoom_lines); // 有限指数倍率。
        let anchor = if hovered {
            cursor.normalized.unwrap_or(Vec2::ZERO)
        } else {
            Vec2::ZERO
        }; // 按钮围绕中心，滚轮围绕光标。
        if ready_image {
            next.view.image_pan =
                image_pan_after_zoom(next.view.image_pan, anchor, next.view.zoom, zoom); // 图片鼠标下的细节位置保持稳定。
        } // 模型围绕当前轨道目标缩放。
        next.view.zoom = zoom; // 图片与模型共用倍率，但各自独立计算显示。
    } // 无滚轮和按钮时不反复写视图。
    if let Some(drag) = next.drag {
        if previous_drag.is_some()
            && (mouse.pressed(drag.button()) || mouse.just_released(drag.button()))
        {
            match drag {
                AssetPreviewDrag::ImagePan(_) => next.view.image_pan += motion / canvas.size(), // 物理位移转换为画布相对比例。
                AssetPreviewDrag::ModelOrbit => orbit_after_drag(&mut next.view, motion), // 允许完整水平环绕及负俯仰。
                AssetPreviewDrag::ModelPan => {
                    if let Some(bounds) = state.model_bounds {
                        let frame = model_preview_frame(
                            bounds.minimum,
                            bounds.maximum,
                            canvas_aspect_ratio(Some(canvas)),
                            next.view,
                        ); // 复用缓存范围。
                        pan_model_after_drag(&mut next.view, frame, motion, canvas.size().y); // 只修改轨道目标，不移动模型实体。
                    } // 缓存失效时不猜测世界尺度。
                } // 中键拖动保持当前朝向和倍率。
            } // 只执行按下时冻结的一种用途。
        } // 起始帧和失焦后不误用旧位移。
        if !mouse.pressed(drag.button()) {
            next.drag = None; // 松开原键结束动作，和弦release仍由统一门屏蔽。
        } // 其他鼠标键不会接力成为新预览手势。
    } // 没有捕获的普通鼠标移动不改变预览。
    next.view.image_pan = clamp_image_pan(
        next.view.image_pan,
        next.view.zoom,
        image_size,
        canvas.size(),
    ); // 限制图片边缘，避免整图拖出可见区。
    navigation.set_if_neq(next); // 稳定帧既不重建UI，也不更新相机。
}

/// 视图或布局真正变化时才更新相机或图片视觉变换；resize保留导航参数。
pub(super) fn sync_asset_preview_view(
    state: Res<AssetPreviewState>, // 只读取当前图片/模型及缓存范围。
    mut navigation: ResMut<AssetPreviewNavigation>, // 交互倍率和观察方向不受窗口模式改变影响。
    media: Option<Res<super::AssetPreviewWorkspace>>,
    canvases: Query<Ref<ComputedNode>, With<AssetPreviewCanvas>>, // 不只看Changed过滤，否则鼠标操作帧会漏更新。
    mut images: Query<(Ref<ComputedNode>, &mut UiTransform), With<AssetPreviewImage>>, // 视觉缩放不参与Flex测量，不能撑大右侧资产栏。
    mut cameras: Query<(&mut Transform, &mut Projection), With<AssetPreviewCamera>>, // 专用相机，不查询场景或玩法视图。
) {
    let Ok(canvas) = canvases.single() else {
        return; // 固定UI尚未生成时等待下一帧。
    }; // 实际物理尺寸由Bevy维护。
    let changed = state.is_changed() || navigation.is_changed() || canvas.is_changed(); // 无变化即保持静止。
    match &state.content {
        AssetPreviewContent::Image {
            phase: AssetPreviewPhase::Ready,
            ..
        } => {
            let Ok((image, mut transform)) = images.single_mut() else {
                return; // 图片UI未创建时没有可用显示目标。
            }; // 只读取一个图片节点。
            if !changed && !image.is_changed() {
                return; // 纹理稳定时不更新UI矩阵。
            } // 新图片布局完成后还需更新一次适配。
            let pan = clamp_image_pan(
                navigation.view.image_pan,
                navigation.view.zoom,
                image.size(),
                canvas.size(),
            ); // resize后仍约束在可查看边缘。
            let translation = pan * canvas.size() * canvas.inverse_scale_factor(); // UiTransform平移使用逻辑px，避免全屏/DPI下漂移。
            transform.set_if_neq(UiTransform {
                translation: Val2::px(translation.x, translation.y), // 只影响绘制，不改变父节点滚动尺寸。
                scale: Vec2::splat(navigation.view.zoom),            // 两轴同倍率，原图不变形。
                ..default()                                          // 图片不应用模型旋转。
            }); // 父画布Overflow负责裁剪放大后的画面。
        } // 预览没有解码、重采样或生成新纹理。
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Ready | AssetPreviewPhase::RevealPending,
            ..
        } if changed => {
            let Some(bounds) = state.model_bounds else {
                return; // 热更扫描完成前不得复用旧模型范围。
            }; // 当前范围已由固定预算扫描完整采用。
            let mut view = navigation.view;
            if media.as_ref().is_some_and(|media| media.central_active && media.expanded.as_ref().is_some_and(|tab| {
                state.selected_path.as_deref() == Some(tab.path.as_str()) && super::gltf_tree::is_glb(&tab.path)
            })) {
                // 几何换代与自动取景都可能恢复默认角度。在最终相机同步处统一采用放大视角，
                // 并同步手势起点，避免第一次右键环绕跳回小预览的俯视角。
                view.reveal_default_sky();
                if navigation.view != view {
                    navigation.view = view;
                }
            }
            if let Ok((mut transform, mut projection)) = cameras.single_mut() {
                apply_model_preview_frame(
                    bounds.minimum,
                    bounds.maximum,
                    canvas_aspect_ratio(Some(&canvas)),
                    view,
                    &mut transform,
                    &mut projection,
                ); // 手势和resize使用同一结果。
            } // 不创建额外相机，也不重建离屏目标。
        } // 隐藏或未就绪模型不提交观察更新。
        _ => {} // 数据、失败和加载占位保持原行为。
    } // 同时只存在一个可交互预览内容。
}

#[cfg(test)] // 最小ECS回归覆盖手势所有权和输入隔离。
#[path = "navigation_tests.rs"] // 测试夹具与运行时系统分开维护。
mod tests; // 不启动实际GPU窗口。
