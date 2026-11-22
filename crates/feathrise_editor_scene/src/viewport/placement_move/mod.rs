//! 场景资产的移动、旋转、缩放与组合手柄，共用实时预览及原作者事务。
//!
//! 【功能设计】拖动快照独占输入；预览不写作者草稿，松手只提交一条历史。
//! 不遍历场景或网格，不新增相机或 GPU 读回；多选复用共同位移并使用单个有界CPU槽。

mod animation; // 场景动画复用对象手势，写回当前帧的临时姿势。
mod constants; // 手柄绘制和拾取的逻辑像素规格。
mod group; // 多选共同移动、旋转和缩放，常数节点预览、分帧捕获与完整单次历史。
mod group_types;
mod group_world;
mod input; // 按下、预览、提交和身份失效取消。
mod math; // 真实相机投影、轴/平面拾取与冻结约束。
mod render; // 编辑器场景几何与拾取共用，工程画布沿用原 SDK 语义。
mod resources; // 单一拖动、建筑校验暂存与静默释放门。
mod rotation; // 固定圆环投影、拾取和相对Y角，拒绝端视与轴心退化。
mod scale; // 三轴方帽、平面和等比中心的几何与共同限幅。
mod source_identity; // 静态地形手势冻结真实来源身份，缺失或运行代际变化时整笔取消。
mod source_node; // 外部 GLB 子网格在同一视口使用原手柄，提交原文件的局部姿势。
mod transaction; // 单记录高度规则、当前权威恢复和一次撤销。
mod types; // 轴/平面手柄及冻结拖动身份。

pub(crate) use input::handle_placement_move;
pub(crate) use input::MoveInputContext; // 框选复用同一套模态、文字焦点与鼠标导航门。
pub(crate) use input::selected_record; // 剪贴板单选沿用同一场景、候选层与稳定下标核对。
pub(crate) use math::{MoveCamera, handle_point, handle_translation, handle_translation_mode}; // 框选和地形手柄使用同一真实相机与吸附相位，避免第二套数学。
pub(crate) use math::plane_corners;

pub(crate) use render::draw_placement_move;
#[cfg(feature = "validation")]
pub(crate) use animation::{validation_animation_drag_active, validation_animation_drag_points};
pub(crate) use render::{active_move_color, axis_colors, handle_color, transform_axis_color, visible_axes};
pub(crate) use resources::PlacementMoveState;
pub(crate) use rotation::{pick_rotation, rotation_ring, rotation_yaw};
pub(crate) use types::{MoveAxis, MoveHandle};
pub(crate) use scale::{pick_scale, group_scale_factor};

use super::modes::{ViewportMode, ViewportModeState};
use crate::resources::SceneEditorState;
use crate::types::EditorTool;

/// W 在独立移动模式与选择模式之间切换，复用模式按钮的启用入口。
pub(crate) fn toggle_placement_move(
    editor: &mut SceneEditorState,
    mode: Option<&mut ViewportModeState>,
) {
    toggle_placement_mode(editor, mode, ViewportMode::Move);
}

/// W/E/R共用互斥切换；再次按当前模式键回到选择，切换到另一手柄不关闭整个变换入口。
pub(crate) fn toggle_placement_mode(
    editor: &mut SceneEditorState,
    mode: Option<&mut ViewportModeState>,
    requested: ViewportMode,
) {
    let active = editor.placement_transform_enabled
        && mode.as_ref().is_none_or(|mode| mode.mode == requested);
    set_placement_mode(
        editor,
        mode,
        if active {
            ViewportMode::Select
        } else {
            requested
        },
    );
}

/// 按钮重复点击保持当前模式，进入变换前保存原工具；切回编辑由模式门恢复。
pub(crate) fn set_placement_mode(
    editor: &mut SceneEditorState,
    mode: Option<&mut ViewportModeState>,
    requested: ViewportMode,
) {
    if let Some(mode) = mode {
        mode.remember_tool(editor.tool);
        mode.mode = requested;
    }
    editor.placement_transform_enabled = requested.transforms_placement();
    if requested != ViewportMode::Edit {
        editor.tool = EditorTool::Inspect;
    }
    editor.status = match requested {
        ViewportMode::Move => "移动模式，可在场景树切换对象。拖动箭头沿轴移动，拖动方框沿平面移动。Ctrl 吸附，Esc 取消。",
        ViewportMode::Rotate => "旋转模式，可在场景树切换对象。拖动旋转圆环，Ctrl 使用设置中的角度步长，Esc 取消。建筑按 90° 档位旋转并校验。",
        ViewportMode::Scale => "缩放模式。轴端方块和平面框缩放对应分量。拖动中心圆或外环向内缩小、向外放大。Ctrl 吸附，Esc 取消。",
        ViewportMode::Transform => "变换模式。箭头移动，圆环旋转，方帽和平面框缩放对应分量，中心圆等比缩放。Ctrl 吸附，Esc 取消。",
        ViewportMode::Select => "已回到选择模式",
        ViewportMode::Edit => "已回到编辑模式",
    }
    .into();
}

#[cfg(test)]
pub(crate) mod interaction_tests;
#[cfg(test)]
mod tests; // 真实交互、撤销、取消和投影回归。 // 真实输入系统与公共鼠标门的单记录事务回归。
