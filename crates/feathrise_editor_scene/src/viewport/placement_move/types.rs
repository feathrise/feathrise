//! 三轴变换手柄与一次按下冻结的完整身份；不保留场景或模型副本。

use bevy::prelude::*;
use feathrise_scene_content::{SceneId, ScenePlacementRecord, TerrainId, TerrainObjectTransform};

use super::super::modes::ViewportMode;
use super::constants::ROTATION_SEGMENTS;
use super::math::MoveCamera;
use crate::authoring::buildings::BuildingEditTarget;

pub(crate) use feathrise_editor_animation_model::gizmo::MoveAxis;
use feathrise_editor_animation_model::gizmo::MoveConstraint;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MovePlane {
    Xy,
    Xz,
    Yz,
}

impl MovePlane {
    pub(super) const ALL: [Self; 3] = [Self::Xz, Self::Xy, Self::Yz];

    pub(super) fn axes(self) -> [MoveAxis; 2] {
        match self {
            Self::Xy => [MoveAxis::X, MoveAxis::Y],
            Self::Xz => [MoveAxis::X, MoveAxis::Z],
            Self::Yz => [MoveAxis::Y, MoveAxis::Z],
        }
    }

    pub(super) fn normal(self) -> MoveAxis {
        match self {
            Self::Xy => MoveAxis::Z,
            Self::Xz => MoveAxis::Y,
            Self::Yz => MoveAxis::X,
        }
    }

    pub(super) fn move_handle(self) -> MoveHandle {
        match self {
            Self::Xy => MoveHandle::XyPlane,
            Self::Xz => MoveHandle::XzPlane,
            Self::Yz => MoveHandle::YzPlane,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveHandle {
    Axis(MoveAxis),
    XyPlane,
    XzPlane,
    YzPlane,
    RotationY,
    AnimationRotation(MoveConstraint),
    AxisScale(MoveAxis),
    PlaneScale(MovePlane),
    UniformScale,
    Light(super::light::LightHandle),
}

impl MoveHandle {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Axis(axis) | Self::AxisScale(axis) => axis.label(),
            Self::XyPlane | Self::PlaneScale(MovePlane::Xy) => "XY 平面",
            Self::XzPlane | Self::PlaneScale(MovePlane::Xz) => "XZ 平面",
            Self::YzPlane | Self::PlaneScale(MovePlane::Yz) => "YZ 平面",
            Self::RotationY => "Y 轴旋转",
            Self::AnimationRotation(axis) => axis.label(),
            Self::UniformScale => "等比缩放",
            Self::Light(handle) => handle.label(),
        }
    }

    pub(crate) fn moves_axis(self, axis: MoveAxis) -> bool {
        match self {
            Self::Axis(selected) => axis == selected,
            Self::XyPlane => axis != MoveAxis::Z,
            Self::XzPlane => axis != MoveAxis::Y,
            Self::YzPlane => axis != MoveAxis::X,
            _ => false,
        }
    }

    pub(super) fn scales_axis(self, axis: MoveAxis) -> bool {
        match self {
            Self::AxisScale(selected) => axis == selected,
            Self::PlaneScale(plane) => axis != plane.normal(),
            Self::UniformScale => true,
            _ => false,
        }
    }

    pub(crate) fn mode(self) -> ViewportMode {
        match self {
            Self::Axis(_) | Self::XyPlane | Self::XzPlane | Self::YzPlane => ViewportMode::Move,
            Self::RotationY | Self::AnimationRotation(_) => ViewportMode::Rotate,
            Self::AxisScale(_) | Self::PlaneScale(_) | Self::UniformScale => ViewportMode::Scale,
            Self::Light(_) => ViewportMode::Select, // 灯具边界在选择和普通变换工具中均可操作。
        }
    }
}

pub(super) struct MoveDrag {
    pub scene_id: SceneId,
    pub record_index: usize,
    pub structure_revision: u64,
    pub author_revision: u64,
    pub terrain_revision: u64,
    pub terrain_content: Option<[u64; 7]>, // Clipmap 根/清单/作者高度/浅坑/拓扑；局部缺页到达不使同代拖动失效。
    pub before: ScenePlacementRecord,        // 只在按下时克隆一次完整记录，撤销保留含水语义。
    pub preview: ScenePlacementRecord,       // 每帧只修改变换标量，不复制绑定数组。
    pub animation: Option<super::animation::AnimationDrag>, // 时间轴帧姿态有独立提交权，不进入普通对象事务。
    pub applied_world: Mat4, // 只在原World预览成功后推进；跨进程显示不能采用被原工具拒绝的候选。
    pub frame: TerrainObjectTransform, // 按下冻结所属地形完整TRS；记录仍保持源局部格式。
    pub world_origin: Vec3,
    pub world_before: Mat4, // 保留父级非等比缩放与局部yaw组合，不拆世界TRS丢失剪切。
    pub source_bounds: Rect, // 来源被移除时只用于恢复已拥有的原根，不能借它确认新的作者结果。
    pub patch_source: Option<PatchSourceSnapshot>, // 相同作者值不能掩盖真实地形实例换代。
    pub formal_runtime_identity: Option<(Option<SceneId>, u64, usize)>, // 鼠标手势不得跨正式根采用/退役；松手后建筑意图仍归其独立校验事务。
    pub handle: MoveHandle,
    pub viewport_mode: ViewportMode, // 组合模式也冻结实际入口；外部切工具须取消当前手势。
    pub start_point: Vec3, // 按下时的轴最近点或平面交点，保持抓取偏移。
    pub start_cursor: Vec2, // 两逻辑像素以内仍视为单击，不因 Ctrl 把原点悄悄吸到网格。
    pub camera: MoveCamera, // 相机、物理尺寸和逻辑缩放任何一项变化均取消。
    pub viewport_ui_size: Vec2, // UI 先完成布局、RTT 下一阶段才采用新尺寸；release 帧也必须识别这一个阶段差。
    pub building_controls: [u32; 4],
    pub snapping: feathrise_editor_core::EditorSnapSettings, // 按下冻结；设置变更不能中途改变同一手势的步长。
    pub translation_control: bool, // 记录最后实际平移的Ctrl电平；松手等待建筑校验时网格不能改换基准。
    pub valid: bool,           // 当前指针目标失败时禁止松手采用上一位置。
    pub preview_applied: bool, // 普通资产与建筑均暂存根姿态，只有实际预览过才需要恢复。
}

#[derive(Clone, PartialEq)]
pub(super) struct PatchSourceSnapshot {
    pub id: TerrainId,
    pub frame: TerrainObjectTransform,
    pub bounds: Rect,
    pub runtime: Option<PatchRuntimeIdentity>, // 只有旧单主地形允许没有独立运行父级。
}

#[derive(Clone, PartialEq)]
pub(super) struct PatchRuntimeIdentity {
    pub parent: Entity,
    pub batch: Entity,
    pub generation: u64,
    pub source_to_world: Mat4,
}

impl MoveDrag {
    /// 正式共享门切换不触发根成员事件；只比较采用身份，候选进度、尝试或失败不使旧活动根失效。
    pub(super) fn formal_runtime_current(&self, status: Option<&feathrise_scene::ScenePlacementRuntimeStatus>) -> bool {
        match (&self.formal_runtime_identity, status) {
            (Some((map, revision, count)), Some(status)) => map == &status.scene_id
                && *revision == status.revision && *count == status.placement_count,
            (None, None) => true,
            _ => false,
        } // 每帧借用固定大小身份，不扫描根或克隆场景。
    }

    /// 手柄落点与可视网格共用实际策略；Inspect建筑仍遵守按下冻结的原网格优先级。
    pub(super) fn translation_snap(&self) -> feathrise_editor_core::EditorTranslationSnap {
        let native_grid = f32::from_bits(self.building_controls[1]); // controls_identity的第二项是本笔建筑步长。
        if self.before.building_preset_id.is_some()
            && native_grid > 0.0
            && (!self.snapping.enabled || !self.snapping.active(self.translation_control))
        {
            feathrise_editor_core::EditorSnapSettings {
                enabled: true,
                mode: feathrise_editor_core::EditorSnapMode::WorldGrid,
                translation_meters: native_grid,
                ..self.snapping
            }
            .translation_snap(false, self.world_origin.to_array())
        } else {
            self.snapping.translation_snap(self.translation_control, self.world_origin.to_array())
        }
    }

    pub(super) fn preview_world_position(&self) -> Vec3 {
        if let Some(animation) = &self.animation {
            return Vec3::from_array(animation.pose.translation);
        }
        self.frame.as_transform().transform_point(self.preview.translation.into())
    }
}

pub(crate) struct RotationRing {
    pub(crate) world: [Vec3; ROTATION_SEGMENTS + 1],
    pub(crate) screen: [Vec2; ROTATION_SEGMENTS + 1], // 含闭合末点；绘制和拾取使用同一组固定顶点。
}

/// 松手后的建筑预览复用同一拖动快照，等待原校验槽成功采用或恢复作者位置。
pub(super) struct PendingBuildingMove {
    pub target: BuildingEditTarget,
    pub drag: MoveDrag,
}
