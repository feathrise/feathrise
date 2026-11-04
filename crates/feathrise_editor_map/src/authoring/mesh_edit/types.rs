//! 编辑元素、操作工具、固定选择位集与单次变换的冻结身份。

use crate::viewport::placement_move::MoveCamera;
use bevy::prelude::*;
use feathrise_map_content::{MapId, MapPlacementRecord};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MeshElementKind {
    #[default]
    Vertex,
    Edge,
    Face,
}

impl MeshElementKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Vertex => "点",
            Self::Edge => "边",
            Self::Face => "面",
        }
    }
    #[cfg(test)]
    pub(super) fn count(self) -> usize {
        match self {
            Self::Vertex => 8,
            Self::Edge => 12,
            Self::Face => 6,
        }
    }
    #[cfg(test)]
    pub(super) fn all(self) -> u128 {
        (1 << self.count()) - 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeshEditAction {
    Toggle,
    Kind(MeshElementKind),
    Tool(MeshEditTool),
    Move,
    Rotate,
    Scale,
    BoxSelect,
    Extrude,
    Inset,
    Bevel,
    LoopCut,
    Knife,
    Subdivide,
    SelectAll,
    Cancel,
    Confirm,
}

/// 选择元素类型与操作工具彼此独立；切工具不能退出网格编辑或改为变换完整对象。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MeshEditTool {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
}

impl MeshEditTool {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Select => "选择",
            Self::Move => "移动",
            Self::Rotate => "旋转",
            Self::Scale => "缩放",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct EditTarget {
    pub map_id: MapId,
    pub index: usize,
    pub author_revision: u64,
    pub structure_revision: u64,
    pub record: MapPlacementRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveConstraint {
    View,
    X,
    Y,
    Z,
}

impl MoveConstraint {
    pub fn axis(self) -> Option<Vec3> {
        match self {
            Self::View => None,
            Self::X => Some(Vec3::X),
            Self::Y => Some(Vec3::Y),
            Self::Z => Some(Vec3::Z),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::View => "视图平面",
            Self::X => "世界 X",
            Self::Y => "世界 Y",
            Self::Z => "世界 Z",
        }
    }
}

#[derive(Debug)]
pub(super) struct ElementMove {
    pub before: MapPlacementRecord,
    pub preview: MapPlacementRecord,
    pub selected_vertices: u64,
    pub selected_elements: u128,
    pub origin: Vec3,
    pub start_cursor: Option<Vec2>, // 从工具栏开始时，首次真实视口指针才绑定起点，防止模型跳到按钮处。
    pub camera: MoveCamera,
    pub viewport_size: Vec2,
    pub constraint: MoveConstraint,
    pub tool: MeshEditTool,
    pub topology: Option<TopologyOperation>,
    /// 手柄捕获到释放时提交；G/R/S及拓扑预览采用一次点击或Enter确认的模态操作。
    pub dragging: bool,
    pub valid: bool,
    pub snapping: feathrise_editor_core::EditorSnapSettings,
}

/// 拓扑操作与原变换共享冻结记录和一次提交；拖动期间绝不把半成品写入作者地图。
#[derive(Debug, Clone, Copy)]
pub(super) enum TopologyOperation {
    Extrude { face: usize, normal: Vec3 },
    Inset,
    Bevel,
    LoopCut,
    Knife,
    Subdivide,
}

impl TopologyOperation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Extrude { .. } => "挤出",
            Self::Inset => "内插面",
            Self::Bevel => "倒角",
            Self::LoopCut => "环切",
            Self::Knife => "切割",
            Self::Subdivide => "细分",
        }
    }
}

/// B先武装框选，再由一次左键拖动冻结起止；只改选择，不写历史。
#[derive(Debug)]
pub(super) struct BoxSelection {
    pub start: Option<Vec2>,
    pub current: Option<Vec2>,
    pub before: u128,
    pub add: bool,
    pub subtract: bool,
    pub camera: MoveCamera,
    pub viewport_size: Vec2, // 侧栏布局可先于渲染尺寸换代，框选也必须冻结实际节点范围。
}

/// 只缓存当前实例少量活动根；取消只恢复仍归同一记录所有的根，不扫描地图。
#[derive(Clone, Debug)]
pub(super) struct HiddenRoot {
    pub entity: Entity,
    pub visibility: Visibility,
    pub visibility_intent: Option<crate::ui::scene_objects::PlacementVisibilityIntent>, // 无资源旧夹具与生产根的显隐意图明确分开。
}
