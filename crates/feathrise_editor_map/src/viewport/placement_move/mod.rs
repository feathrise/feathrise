//! 单资产移动、Y轴旋转与等比缩放手柄：保留原入口路径，共用实时预览及原作者事务。
//!
//! 【功能设计】拖动快照独占输入；预览不写作者草稿，松手只提交一条历史。
//! 不遍历场景或网格，不新增相机或 GPU 读回；多选复用共同位移并使用单个有界CPU槽。

mod constants; // 圆环和中心方块的固定预算及逻辑像素规格。
mod group; // 多选共同移动、旋转和缩放，常数节点预览、分帧捕获与完整单次历史。
mod group_types;
mod group_world;
mod input; // 按下、预览、提交和身份失效取消。
mod math; // 真实相机投影、轴/平面拾取与冻结约束。
mod render; // 发布原手柄语义，显示几何由SDK共享；输入与事务仍由本模块拥有。
mod resources; // 单一拖动、建筑校验暂存与静默释放门。
mod rotation; // 固定圆环投影、拾取和相对Y角，拒绝端视与轴心退化。
mod scale; // 视口中心方块、等比倍率和共同限幅。
mod source_identity; // 静态地形手势冻结真实来源身份，缺失或运行代际变化时整笔取消。
mod transaction; // 单记录高度规则、当前权威恢复和一次撤销。
mod types; // 轴/平面手柄及冻结拖动身份。

pub(crate) use input::handle_placement_move;
pub(crate) use input::MoveInputContext; // 框选复用同一套模态、文字焦点与鼠标导航门。
pub(crate) use input::selected_record; // 剪贴板单选沿用同一地图、候选层与稳定下标核对。
pub(crate) use math::{MoveCamera, handle_point, handle_translation, handle_translation_mode}; // 框选和地形手柄使用同一真实相机与吸附相位，避免第二套数学。

pub(crate) use render::draw_placement_move;
pub(crate) use render::{axis_colors, visible_axes}; // 地形、子地图与静水保留相同颜色和原相机逐轴退化门。
pub(crate) use resources::PlacementMoveState;
pub(crate) use rotation::{pick_rotation, rotation_ring, rotation_yaw};
pub(crate) use types::{MoveAxis, MoveHandle};
pub(crate) use scale::{pick_scale, group_scale_factor};

use super::modes::{ViewportMode, ViewportModeState};
use crate::resources::MapEditorState;
use crate::types::EditorTool;

/// W 在独立移动模式与选择模式之间切换，复用模式按钮的启用入口。
pub(crate) fn toggle_placement_move(
    editor: &mut MapEditorState,
    mode: Option<&mut ViewportModeState>,
) {
    toggle_placement_mode(editor, mode, ViewportMode::Move);
}

/// W/E/R共用互斥切换；再次按当前模式键回到选择，切换到另一手柄不关闭整个变换入口。
pub(crate) fn toggle_placement_mode(
    editor: &mut MapEditorState,
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
    editor: &mut MapEditorState,
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
        ViewportMode::Move => "移动模式：当前目标已锁定，换对象请回选择模式。拖动三色轴或铜色方框沿 XZ 平面移动；Ctrl 使用设置中的移动步长，Esc 取消。建筑沿用建筑网格。",
        ViewportMode::Rotate => "旋转模式：当前目标已锁定，换对象请回选择模式。拖动 Y 轴圆环；Ctrl 使用设置中的角度步长，Esc 取消。建筑按 90° 档位旋转并校验；端视时请切到顶视或斜视。",
        ViewportMode::Scale => "缩放模式：当前目标已锁定，换对象请回选择模式。拖动中心方块，向右/上放大、向左/下缩小；Ctrl 使用设置中的倍率步长，Esc 取消。建筑保持预设尺寸。",
        ViewportMode::Select => "已回到选择模式",
        ViewportMode::Edit => "已回到编辑模式",
    }
    .into();
}

#[cfg(test)]
pub(crate) mod interaction_tests;
#[cfg(test)]
mod tests; // 真实交互、撤销、取消和投影回归。 // 真实输入系统与公共鼠标门的单记录事务回归。
