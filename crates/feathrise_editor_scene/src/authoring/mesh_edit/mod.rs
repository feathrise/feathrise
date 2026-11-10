//! 当前单个有界网格的点、边、面编辑；最多64共享点/64面，不扫描场景或生成外部模型。
//!
//! 【需求设计】Tab切对象/编辑模式，1/2/3选择元素；原竖栏承接G/R/S与XYZ约束，B框选及拓扑操作共用会话。
//! 所有建模手势独立预览，确认后才采用完整记录与一条撤销；旧八点来源仍兼容坐标编辑。

mod geometry;
mod mesh_geometry; // 有界可变拓扑的选择、拾取和框选；与旧八点夹具保持兼容。
pub(crate) mod transform_math; // 移动/旋转/缩放手柄共享有界几何、投影拾取和世界变换逆算。
mod input;
mod preview;
mod render;
mod resources;
mod types;
mod topology_ops; // 受检局部拓扑操作，不改变完整实例姿态或原有历史合同。
mod operation_preview; // 不同建模操作共享冻结记录及预览参数计算。

pub(crate) use resources::MeshEditState;
pub(crate) use input::MeshEditExternalOwners; // 视口其他工具复用相同外部输入所有者，避免维护两套菜单门。
pub(crate) use types::{MoveConstraint, MeshEditAction, MeshEditTool, MeshElementKind};

use bevy::prelude::*;

/// 统一 UI 请求、元素快捷键和手势必须先于普通对象输入及公共鼠标门收口。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MeshEditInput;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<MeshEditState>()
        .add_systems(Update, input::handle_input
            .in_set(MeshEditInput)
            .in_set(crate::SceneEditorSystemSet::Input)
            .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
            .before(crate::viewport::placement_move::handle_placement_move)
            .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard))
        .add_systems(Update, render::draw_elements
            .in_set(crate::SceneEditorSystemSet::Presentation));
}

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;
