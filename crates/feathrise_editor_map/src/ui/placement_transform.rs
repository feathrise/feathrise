//! 已摆放场景资产的完整实例选择、专业变换输入与结构操作。
//!
//! 【功能设计】场景树根或其任一网格都只选中一条`MapPlacementRecord`；位置、
//! Y轴旋转、三轴缩放、高度模式、复制与删除直接修改正式草稿，并复用统一撤销、保存和热接管链路。

mod actions; // 高度模式、复制、删除及运行时根定向同步。
mod cuboid; // 内置立方体的米制尺寸、材质和盒碰撞，复用完整记录事务。
mod numeric_input; // 七个专业数字框的精确提交、单次拖动撤销和灵敏度持久化。
mod relocation; // “吸附地面”的真实三角落点、撤销提交与纯语义回归。
mod scene; // 右栏紧凑水墨铜边属性面板与固定数值槽。
mod sync; // 选择换代后的显隐、摘要、数值、禁用态与按钮样式同步。
mod terrain; // 静态三维地形分类与矩形洞口元数据，沿用完整摆放历史。
#[cfg(test)]
mod terrain_patch_interaction_tests; // 真实属性消息、源父层级与完整记录事务的回归。

use bevy::prelude::*; // Component、Entity、Rect和Resource等领域类型。
use feathrise_editor_ui::{NumericInputScrubConfig, NumericInputSpec, NumericValue};
use feathrise_map_content::{
    MAX_MAP_PLACEMENT_SCALE, MIN_MAP_PLACEMENT_SCALE, MapId, MapPlacementRecord,
}; // 专业输入与正式运行时共享同一缩放边界和记录格式。
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::MapEditorScrubField; // 三类变换手感各自持久化一个倍率。 // 公共控件继续拥有字符输入、左右拖动、周期角和显示规范化。

pub(crate) use actions::{handle_placement_transform_actions, sync_placement_record_world, sync_placement_record_world_with_terrain}; // Input处理高度模式、复制和删除按钮。
pub(crate) use numeric_input::handle_placement_transform_numeric_input; // Input消费七字段公共消息。
pub(crate) use relocation::apply_selected_placement_relocation; // Tasks在现有地形光标更新后采用一次真实三角落点。
pub(super) use scene::placement_transform_panel; // 右栏场景页只组合一个完整功能域Scene。
pub(crate) use scene::setup_placement_transform_numeric_inputs; // Presentation只为新增槽位挂载一次控件。
pub(crate) use sync::sync_placement_transform_panel; // Presentation统一同步显隐、值和交互视觉。

const TRANSLATION_Y_MIN: f32 = -1_000_000.0; // Y轴不受地图XZ边界约束，但仍限制异常精确输入。
const TRANSLATION_Y_MAX: f32 = 1_000_000.0; // 覆盖现有地形高度同时阻断极端世界包围盒。
const TRANSLATION_UNITS_PER_PIXEL: f32 = 0.1; // 一倍灵敏度每像素移动十厘米。
const TRANSLATION_STEP: f32 = 0.001; // 精确到毫米并与三位小数显示一致。
const ROTATION_UNITS_PER_PIXEL: f32 = 0.5; // 一倍灵敏度每像素旋转半度。
const ROTATION_STEP: f32 = 0.1; // Yaw显示与拖动吸附到十分之一度。
const SCALE_EXPONENT_PER_PIXEL: f32 = 0.01; // 指数拖动在小缩放附近保留精调空间。
const SCALE_STEP: f32 = 0.001; // 三位小数避免拖动结果藏有不可见尾数。

/// 七个数值根的稳定业务身份。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PlacementTransformField {
    #[default]
    TranslationX, // 世界X位置。
    TranslationY, // 世界Y位置。
    TranslationZ, // 世界Z位置。
    Yaw,          // 绕世界Y轴旋转。
    ScaleX,       // 局部X缩放。
    ScaleY,       // 局部Y缩放。
    ScaleZ,       // 局部Z缩放。
    CutoutMinX,
    CutoutMaxX,
    CutoutMinZ,
    CutoutMaxZ,
    CuboidSizeX,
    CuboidSizeY,
    CuboidSizeZ,
    CuboidColorR,
    CuboidColorG,
    CuboidColorB,
    CuboidRoughness,
}

impl PlacementTransformField {
    pub(super) const fn is_cuboid(self) -> bool {
        matches!(self, Self::CuboidSizeX | Self::CuboidSizeY | Self::CuboidSizeZ
            | Self::CuboidColorR | Self::CuboidColorG | Self::CuboidColorB | Self::CuboidRoughness)
    }
    pub(super) fn supported_by(self, record: &MapPlacementRecord) -> bool {
        if matches!(self, Self::CuboidSizeX | Self::CuboidSizeY | Self::CuboidSizeZ) {
            matches!(record.asset_target, feathrise_map_content::MapPlacementAssetTarget::Cuboid { .. })
        } else { !self.is_cuboid() || record.asset_target.is_builtin() }
    } // 顶点编辑后的网格仍可改材质，但不能用长宽高输入覆盖已变形的共享顶点。
    pub(super) const fn is_cutout(self) -> bool {
        matches!(
            self,
            Self::CutoutMinX | Self::CutoutMaxX | Self::CutoutMinZ | Self::CutoutMaxZ
        )
    }
    #[cfg(test)] // 字段全集只供纯字段映射回归，运行时槽位由BSN显式固定顺序。
    pub(super) const ALL: [Self; 7] = [
        Self::TranslationX, // 第一行位置X。
        Self::TranslationY, // 第二行位置Y。
        Self::TranslationZ, // 第三行位置Z。
        Self::Yaw,          // 第四行Y轴角度。
        Self::ScaleX,       // 第五行缩放X。
        Self::ScaleY,       // 第六行缩放Y。
        Self::ScaleZ,       // 第七行缩放Z。
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::TranslationX => "位置 X", // 明确这是世界平移而非局部轴。
            Self::TranslationY => "位置 Y", // 高度可独立精调。
            Self::TranslationZ => "位置 Z", // 世界南北轴。
            Self::Yaw => "旋转 Y",          // 当前格式只持久化Y轴旋转。
            Self::ScaleX => "缩放 X",       // 三轴缩放保持独立。
            Self::ScaleY => "缩放 Y",       // 三轴缩放保持独立。
            Self::ScaleZ => "缩放 Z",       // 三轴缩放保持独立。
            Self::CutoutMinX => "洞口 X 下界",
            Self::CutoutMaxX => "洞口 X 上界",
            Self::CutoutMinZ => "洞口 Z 下界",
            Self::CutoutMaxZ => "洞口 Z 上界",
            Self::CuboidSizeX => "尺寸 X",
            Self::CuboidSizeY => "尺寸 Y",
            Self::CuboidSizeZ => "尺寸 Z",
            Self::CuboidColorR => "颜色 R",
            Self::CuboidColorG => "颜色 G",
            Self::CuboidColorB => "颜色 B",
            Self::CuboidRoughness => "粗糙度",
        }
    }

    pub(super) const fn unit(self) -> &'static str {
        match self {
            Self::TranslationX | Self::TranslationY | Self::TranslationZ => "m", // 世界位置使用米。
            Self::Yaw => "°", // 旋转使用角度而不是弧度。
            Self::ScaleX | Self::ScaleY | Self::ScaleZ => "×", // 缩放是无量纲倍率。
            Self::CuboidColorR | Self::CuboidColorG | Self::CuboidColorB => "",
            Self::CuboidRoughness => "",
            _ => "m",
        }
    }

    pub(super) const fn decimals(self) -> u8 {
        match self {
            Self::Yaw => 1, // 角度显示十分之一度。
            Self::CuboidColorR | Self::CuboidColorG | Self::CuboidColorB => 0,
            _ => 3,         // 位置和缩放保留三位小数。
        }
    }

    pub(super) const fn scrub_field(self) -> MapEditorScrubField {
        match self {
            Self::TranslationX | Self::TranslationY | Self::TranslationZ => {
                MapEditorScrubField::PlacementTranslation
            } // 三轴平移共享统一米制手感。
            Self::Yaw => MapEditorScrubField::PlacementRotation, // 旋转使用独立角度手感。
            Self::ScaleX | Self::ScaleY | Self::ScaleZ => MapEditorScrubField::PlacementScale, // 三轴缩放共享统一倍率手感。
            _ => MapEditorScrubField::PlacementTranslation,
        }
    }

    pub(super) fn limits(self, bounds: Option<Rect>) -> (f32, f32) {
        match self {
            Self::TranslationX => bounds
                .map(|bounds| (bounds.min.x, bounds.max.x))
                .unwrap_or((TRANSLATION_Y_MIN, TRANSLATION_Y_MAX)), // 清单异常时先禁用，宽范围只供首帧壳。
            Self::TranslationZ => bounds
                .map(|bounds| (bounds.min.y, bounds.max.y))
                .unwrap_or((TRANSLATION_Y_MIN, TRANSLATION_Y_MAX)), // Rect的Y分量对应世界Z。
            Self::TranslationY => (TRANSLATION_Y_MIN, TRANSLATION_Y_MAX), // 高度只要求有限安全范围。
            Self::Yaw => (0.0, 360.0), // 公共NumericInput按半开区间循环。
            Self::ScaleX | Self::ScaleY | Self::ScaleZ => {
                (MIN_MAP_PLACEMENT_SCALE, MAX_MAP_PLACEMENT_SCALE)
            } // 与保存和运行时候选完全一致。
            Self::CuboidSizeX | Self::CuboidSizeY | Self::CuboidSizeZ => (0.001, 10_000.0),
            Self::CuboidColorR | Self::CuboidColorG | Self::CuboidColorB => (0.0, 255.0),
            Self::CuboidRoughness => (0.0, 1.0),
            _ => (TRANSLATION_Y_MIN, TRANSLATION_Y_MAX),
        }
    }

    pub(super) fn spec(self, bounds: Option<Rect>) -> NumericInputSpec {
        let (minimum, maximum) = self.limits(bounds); // 动态地图边界直接限制X/Z精确输入。
        NumericInputSpec::f32(self.decimals())
            .with_min(NumericValue::F32(minimum)) // 精确输入复用正式下界。
            .with_max(NumericValue::F32(maximum)) // 精确输入复用正式上界。
            .with_step(NumericValue::F32(match self {
                Self::Yaw => ROTATION_STEP, // 角度吸附到十分之一度。
                Self::ScaleX | Self::ScaleY | Self::ScaleZ => SCALE_STEP, // 缩放吸附到千分之一。
                Self::CuboidColorR | Self::CuboidColorG | Self::CuboidColorB => 1.0,
                _ => TRANSLATION_STEP,      // 平移吸附到毫米。
            }))
            .with_wrap(self == Self::Yaw) // 360度与0度使用周期语义。
            .with_step_buttons(false) // 专业属性框不生成另一套加减按钮。
            .with_live_update(false) // 字符输入只在提交时形成一条历史。
            .with_replace_on_first_key(true) // 首键替换旧值，适合坐标精确录入。
    }

    pub(super) fn scrub_config(
        self,
        settings: &EditorSettings,
        bounds: Option<Rect>,
    ) -> NumericInputScrubConfig {
        let sensitivity = settings
            .map_editor_scrub_sensitivities
            .get(self.scrub_field()); // 每类变换读取自己的持久倍率。
        let (minimum, maximum) = self.limits(bounds); // 左拖与精确输入共享同一业务范围。
        match self {
            Self::ScaleX | Self::ScaleY | Self::ScaleZ => NumericInputScrubConfig::exponential(
                SCALE_EXPONENT_PER_PIXEL, // 正比例拖动覆盖千分之一到一千倍而不丢低值精度。
                SCALE_STEP,               // 最终结果吸附到三位小数。
                minimum,                  // 正缩放闭区间下界。
                maximum,                  // 正缩放闭区间上界。
                sensitivity,              // 用户持久手感倍率。
            ),
            Self::Yaw => NumericInputScrubConfig::linear(
                ROTATION_UNITS_PER_PIXEL, // 固定角度/像素映射。
                ROTATION_STEP,            // 十分之一度吸附。
                minimum,                  // 零度起点。
                maximum,                  // 三百六十度由InputSpec循环回零。
                sensitivity,              // 旋转独立倍率。
            ),
            Self::CuboidColorR | Self::CuboidColorG | Self::CuboidColorB => {
                NumericInputScrubConfig::linear(1.0, 1.0, minimum, maximum, sensitivity)
            }
            Self::CuboidRoughness => {
                NumericInputScrubConfig::linear(0.005, 0.001, minimum, maximum, sensitivity)
            }
            _ => NumericInputScrubConfig::linear(
                TRANSLATION_UNITS_PER_PIXEL, // 固定米/像素映射。
                TRANSLATION_STEP,            // 毫米吸附。
                minimum,                     // 当前轴正式下界。
                maximum,                     // 当前轴正式上界。
                sensitivity,                 // 平移共用倍率。
            ),
        }
    }

    pub(super) fn value(self, record: &MapPlacementRecord) -> f32 {
        if self.is_cuboid() {
            return cuboid::field_value(self, record).unwrap_or_default();
        }
        match self {
            Self::TranslationX => record.translation[0], // 世界X。
            Self::TranslationY => record.translation[1], // 世界Y。
            Self::TranslationZ => record.translation[2], // 世界Z。
            Self::Yaw => record.yaw_degrees,             // 作者角度。
            Self::ScaleX => record.scale[0],             // X倍率。
            Self::ScaleY => record.scale[1],             // Y倍率。
            Self::ScaleZ => record.scale[2],             // Z倍率。
            _ => record
                .terrain_patch
                .as_ref()
                .and_then(|patch| patch.cutout)
                .map_or(0.0, |hole| match self {
                    Self::CutoutMinX => hole.min[0],
                    Self::CutoutMaxX => hole.max[0],
                    Self::CutoutMinZ => hole.min[1],
                    _ => hole.max[1],
                }),
        }
    }

    pub(super) fn set_value(self, record: &mut MapPlacementRecord, value: f32) -> bool {
        if self.is_cuboid() {
            return cuboid::set_field_value(self, record, value);
        }
        let target = match self {
            Self::TranslationX => &mut record.translation[0], // 只写世界X。
            Self::TranslationY => &mut record.translation[1], // 只写世界Y。
            Self::TranslationZ => &mut record.translation[2], // 只写世界Z。
            Self::Yaw => &mut record.yaw_degrees,             // 只写Y轴角度。
            Self::ScaleX => &mut record.scale[0],             // 只写缩放X。
            Self::ScaleY => &mut record.scale[1],             // 只写缩放Y。
            Self::ScaleZ => &mut record.scale[2],             // 只写缩放Z。
            _ => {
                let Some(hole) = record
                    .terrain_patch
                    .as_mut()
                    .and_then(|patch| patch.cutout.as_mut())
                else {
                    return false;
                };
                match self {
                    Self::CutoutMinX => &mut hole.min[0],
                    Self::CutoutMaxX => &mut hole.max[0],
                    Self::CutoutMinZ => &mut hole.min[1],
                    _ => &mut hole.max[1],
                }
            }
        };
        if *target == value {
            return false; // 同一吸附档不推进作者修订或重复写Transform。
        }
        *target = value; // 候选值随后仍必须通过完整记录公共校验。
        true // 调用方继续更新预览和事务终点。
    }
}

/// 右栏BSN固定槽位，Presentation只在Added帧生成一次公共数值框。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PlacementTransformInputSlot(pub(crate) PlacementTransformField);

/// 面板根marker；无完整实例选择时整个区域退出布局。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MapEditorPlacementTransformPanel;

/// 属性按能力连续排版；不把基础几何误当成外部GLB的地形作者源。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PlacementPropertiesSection {
    #[default]
    Transform,
    Geometry,
    Material,
    Collision,
}

/// 尺寸行连同轴标签一起按生成器能力显隐，外部网格不保留空白尺寸行。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MapEditorCuboidGeometryRow;

impl PlacementPropertiesSection {
    pub(super) fn supported_by(self, record: &MapPlacementRecord) -> bool {
        match self {
            Self::Transform | Self::Geometry => true,
            Self::Material | Self::Collision => record.asset_target.is_builtin(),
        } // 外部资产继续使用自己的材质和碰撞来源，不出现无法写回的立方体控件。
    }
}

/// 高度模式摘要；对象身份与资产来源由独立检查器概览呈现。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MapEditorPlacementTransformSummary;

/// 此定位入口复用原聚焦动作，仅在能定位当前场景选择的原工具上下文可用。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct MapEditorPlacementTransformLocate;

/// 属性面板独立动作，不扩大全局编辑工具枚举。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PlacementTransformAction {
    #[default]
    Relocate, // “吸附地面”复用首次摆放命中，下一次中央视口点击重定位完整实例。
    FollowTerrain,       // 保持当前世界Y并改用“R16地表+离地偏移”语义。
    FixedHeight,         // 保持当前世界Y并停止响应后续地形变化。
    RotateQuarterTurn,   // 建筑专用：顺时针旋转90°并走完整后台占地与地基校验。
    Duplicate,           // 克隆完整记录并立即进入吸附落点模式。
    Delete,              // 删除当前完整记录，保留统一撤销能力。
    ToggleTerrainPatch,  // 普通GLB与静态三维地形之间切换，开启时固定当前世界高度。
    ToggleTerrainCutout, // 一条记录内的可撤销矩形开口，不建立独立孤立洞口实体。
    ToggleCuboidCollision, // 内置立方体的简单盒碰撞与外观参数一起持久化。
}

/// 下一次中央视口落点对应移动现有记录，还是延迟创建建筑副本。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PlacementRelocationIntent {
    #[default]
    Move, // 普通资产或建筑移动现有记录。
    DuplicateBuilding, // 建筑校验成功前不创建任何临时重叠记录。
}

/// 高度模式、吸附、复制和删除按钮的稳定业务marker。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MapEditorPlacementTransformActionButton(pub(crate) PlacementTransformAction);

/// 一次精确输入或左拖冻结的O(1)作者目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlacementTransformTarget {
    pub(super) map_id: MapId,           // 同名实例不能跨地图复用。
    pub(super) placement_id: String,    // 完整记录稳定ID。
    pub(super) record_index: usize,     // 浏览器结构索引提供O(1)数组槽。
    pub(super) structure_revision: u64, // 增删实例后旧下标立即失效。
    pub(super) authoring_revision: u64, // 字符焦点期间外部作者变化令提交失效。
}

/// 中键字符输入冻结的根与作者目标。
#[derive(Clone, Debug)]
pub(super) struct PlacementTransformFocus {
    pub(super) entity: Entity, // 必须与最终Committed消息根一致。
    pub(super) target: PlacementTransformTarget, // 聚焦时完整冻结地图和数组身份。
}

/// 补丁数值拖动冻结完整来源身份，拒绝同一父实体被新代次复用。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlacementTransformPatchSource {
    pub(super) parent: Option<(Entity, Entity, u64)>, // 同一实体也必须保持原批次和代次；旧主地形没有独立父。
    pub(super) frame: Mat4,                         // 完整源矩阵参与事务身份，不能在拖动中换坐标语义。
    pub(super) bounds: Rect,                        // 取消源失效事务仍按开始时的局部范围恢复已有模型。
}

/// 一次左拖保存起点和最后责任快照，松手形成至多一条`PlacementChanged`。
#[derive(Clone, Debug)]
pub(super) struct PlacementTransformScrub {
    pub(super) entity: Entity,                   // 活动物理拖动根。
    pub(super) field: PlacementTransformField,   // 事务期间唯一字段。
    pub(super) target: PlacementTransformTarget, // 稳定地图和记录身份。
    pub(super) before: MapPlacementRecord,       // 拖动前完整撤销快照。
    pub(super) latest_record: MapPlacementRecord, // 仅用于辨别外部字段接管，取消不能清除外部dirty。
    pub(super) patch_source: Option<PlacementTransformPatchSource>, // 松手须再次证明真实源没有退役或换代。
    pub(super) latest_value: f32,                // 最后已采用吸附档。
    pub(super) expected_revision: u64,           // 只接受本事务连续推进的作者修订。
    pub(super) dirty_before: bool,               // 拖回起点时恢复原保存门。
}

/// 七字段字符焦点、唯一左拖事务和一次视口吸附模式共享一个轻量状态。
#[derive(Resource, Default)]
pub(crate) struct PlacementTransformInputState {
    pub(super) focus: Option<PlacementTransformFocus>, // 当前中键精确输入身份。
    pub(super) scrub: Option<PlacementTransformScrub>, // 当前左拖事务。
    pub(super) rejected_scrub: Option<Entity>, // 取消后的同根Changed持续拒收，直到真实Scrub松手，不能将事务空窗当作新笔。
    pub(super) relocation_target: Option<PlacementTransformTarget>, // 下一次中央视口左键绑定的完整稳定资产。
    pub(super) relocation_intent: PlacementRelocationIntent, // 建筑复制与普通移动共享一个视口点击门。
}

impl PlacementTransformInputState {
    /// 完整来源捕获必须等待业务事务收口，不能用鼠标捕获状态代替数值Changed/Committed生命周期。
    pub(crate) fn transaction_active(&self) -> bool { self.scrub.is_some() }

    /// 位置、旋转、缩放的实时投影直接借用原数值事务；颜色、洞口等属性由各自领域发布。
    pub(crate) fn embedded_scrub(&self) -> Option<(Entity, &feathrise_map_content::MapId, usize, u64, u64,
        &MapPlacementRecord, &MapPlacementRecord)> {
        let scrub = self.scrub.as_ref()?;
        if !matches!(scrub.field, PlacementTransformField::TranslationX | PlacementTransformField::TranslationY
            | PlacementTransformField::TranslationZ | PlacementTransformField::Yaw | PlacementTransformField::ScaleX
            | PlacementTransformField::ScaleY | PlacementTransformField::ScaleZ) { return None; }
        Some((scrub.entity, &scrub.target.map_id, scrub.target.record_index, scrub.target.authoring_revision,
            scrub.target.structure_revision, &scrub.before, &scrub.latest_record))
    }

    /// 普通资产与建筑共用射线，建筑最终高度仍交给五点地基；复制不能变成迟到移动。
    pub(crate) fn relocation_ground_target(
        &self,
        building: bool,
        controls: [u32; 4],
        catalog_revision: u64,
    ) -> Option<crate::viewport::vegetation_ground::GroundCursorTarget> {
        let placement_id = self.relocation_target.as_ref()?.placement_id.clone();
        Some(if building {
            crate::viewport::vegetation_ground::GroundCursorTarget::RelocateBuilding {
                placement_id,
                duplicate: self.relocation_intent == PlacementRelocationIntent::DuplicateBuilding,
                controls,
                catalog_revision,
            }
        } else {
            crate::viewport::vegetation_ground::GroundCursorTarget::RelocatePlacement(placement_id)
        })
    }
    /// 放置新资产系统据此让出同一帧左键，避免吸附时再生成一份模型。
    pub(crate) fn relocation_armed(&self) -> bool {
        self.relocation_target.is_some() // 只公开布尔门，不允许其他功能域改写冻结身份。
    }

    /// Clipmap光标查询只读冻结目标的O(1)身份；实际采用仍由吸附系统复核完整记录。
    pub(crate) fn relocation_ground_identity(&self) -> Option<(&MapId, usize, &str, u64, u64)> {
        self.relocation_target.as_ref().map(|target| {
            (
                &target.map_id,
                target.record_index,
                target.placement_id.as_str(),
                target.structure_revision,
                target.authoring_revision,
            )
        }) // 不向视口开放可变目标或复制完整字符串。
    }
}

impl PlacementTransformTarget {
    /// 转为建筑域可跨系统消息携带的同一冻结身份。
    pub(super) fn building_edit_target(&self) -> crate::authoring::buildings::BuildingEditTarget {
        crate::authoring::buildings::BuildingEditTarget {
            map_id: self.map_id.clone(),
            placement_id: self.placement_id.clone(),
            record_index: self.record_index,
            structure_revision: self.structure_revision,
            author_revision: self.authoring_revision,
        }
    }
}

/// 从当前完整结构签名取得唯一作者目标，不扫描最多8192条placements。
pub(super) fn selected_target(
    browser: &crate::ui::gltf_browser::EditorGltfBrowserState, // 当前场景树完整实例选择。
    session: &crate::authoring::EditorAuthoringSession,        // 当前草稿与结构修订。
) -> Option<PlacementTransformTarget> {
    let placement_id = browser.selected_placement_id.as_ref()?; // 根或子网格必须已经发布完整实例选择。
    let record_index =
        browser.placement_record_index(placement_id, session.placement_structure_revision)?; // 在途旧签名拒绝提供下标。
    let draft = session.draft.as_ref()?; // 没有权威草稿时不能编辑运行时根。
    draft
        .placements
        .placements
        .get(record_index)
        .filter(|record| record.id == *placement_id && record.editor.selectable() && !record.terrain_patch.as_ref().is_some_and(|p| p.sculpted_volume))?; // 保护对象只能从管理入口恢复，自动体积源仍由地形工具维护。
    Some(PlacementTransformTarget {
        map_id: draft.map_id.clone(),       // 冻结当前地图身份。
        placement_id: placement_id.clone(), // 单次边沿拥有短稳定ID。
        record_index,                       // 后续Changed不扫描Vec。
        structure_revision: session.placement_structure_revision, // 结构变化立即失败关闭。
        authoring_revision: session.revision, // 字符输入只接受同一作者修订。
    })
}

#[cfg(test)]
mod tests {
    use super::*; // 纯字段映射回归不构造完整UI World。

    fn record() -> MapPlacementRecord {
        MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "placement-transform".to_owned(), // 稳定测试身份。
            asset_path: "gltf/test.glb".to_owned(), // 不实际加载资产。
            asset_target: Default::default(),     // 字段映射不改变文件内目标。
            building_preset_id: None,             // 变换字段夹具保持普通资产身份。
            courtyard: None,                      // 独立摆放记录不属于任何院落副本。
            translation: [1.0, 2.0, 3.0],         // 三轴使用不同值防止串槽。
            height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 字段映射基线使用兼容固定高度。
            terrain_offset: 0.0,        // 固定模式没有有效离地偏移。
            yaw_degrees: 45.0,          // 旋转独立于位置和缩放。
            scale: [4.0, 5.0, 6.0],     // 三轴使用不同值防止串槽。
            water_surfaces: Vec::new(), // 变换操作不得触及水绑定。
        }
    }

    #[test]
    fn transform_fields_write_only_their_owned_scalar() {
        for (index, field) in PlacementTransformField::ALL.into_iter().enumerate() {
            let mut value = record(); // 每个字段都从同一完整基线开始。
            let before_water = value.water_surfaces.clone(); // 明确锁定非变换数据不受影响。
            assert!(field.set_value(&mut value, 10.0 + index as f32));
            assert_eq!(field.value(&value), 10.0 + index as f32); // 当前字段采用新值。
            assert_eq!(value.water_surfaces, before_water); // Blender水体语义保持不变。
        }
    }
}
