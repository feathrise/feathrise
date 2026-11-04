//! 河流与瀑布控制点高度、宽度、深度和流速的公共专业数值输入领域。
//!
//! 【功能设计】左拖只改内存草稿与Gizmo，松手才推进一次水预览修订并压入一条撤销；
//! 右拖只更新每个逻辑字段自己的持久灵敏度，中键单击进入公共精确输入。

mod scene; // 固定BSN槽位首次挂载完整公共控件。
mod sync; // 选择、撤销、连接约束和设置变化后的字段同步。
mod systems; // 精确提交、左拖单事务与右拖灵敏度消息处理。
mod transaction; // 冲突或异常中断仅恢复本事务拥有的稳定点单字段。

use bevy::prelude::*; // 业务marker、稳定实体身份与跨帧输入资源。
use feathrise_map_content::{
    MAX_MAP_WATER_FLOW_SPEED, MAX_MAP_WATER_SPLINE_DEPTH_METERS, MAX_MAP_WATER_SPLINE_WIDTH_METERS,
    MIN_MAP_WATER_SPLINE_DEPTH_METERS, MIN_MAP_WATER_SPLINE_WIDTH_METERS, MapId, MapWaterAsset,
    MapWaterSplinePoint,
}; // 公共数值范围直接复用冻结作者格式。
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::MapEditorScrubField; // 四个水网字段各自拥有持久倍率槽。
use feathrise_editor_ui::{
    NumericInputScrubConfig, NumericInputScrubMapping, NumericInputSpec, NumericValue,
}; // 共享 UI 控件统一拥有指针仲裁、字符编辑与数值规范化。

use super::helpers::{
    active_feature_identity, active_points, point_mut_by_id, point_position_by_id,
}; // 当前选择只用于建立稳定目标，拖动过程完全按ID访问。
use crate::authoring::resources::EditorAuthoringSession; // 当前草稿和作者修订用于冻结目标。
use crate::constants::{MAX_WATER_LEVEL_METERS, MIN_WATER_LEVEL_METERS}; // 控制点Y沿用项目米制水位安全范围。
use crate::resources::MapEditorState; // 河流/瀑布工具和当前控制点选择。
use transaction::rollback_water_network_scrub_if_owned; // 取消路径不会用旧完整水网快照覆盖外部修改。

pub(crate) use scene::setup_water_network_numeric_inputs; // Presentation首次创建四个公共控件根。
pub(crate) use sync::sync_water_network_numeric_inputs; // Presentation增量同步值、禁用态和倍率。
pub(crate) use systems::handle_water_network_numeric_inputs; // Input消费四个字段的公共消息。

const SCRUB_SENSITIVITY_MIN: f32 = 0.01; // 右拖最细允许标准手感百分之一。
const SCRUB_SENSITIVITY_MAX: f32 = 100.0; // 右拖最快限制为标准手感一百倍。
const SCRUB_DRAG_THRESHOLD_PIXELS: f32 = 4.0; // 四像素以内的左右键位移保持无动作单击。
const SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE: f32 = 48.0; // 右拖四十八像素使倍率翻倍或减半。

/// 四个字段的稳定业务身份；它同时决定范围、显示、映射和设置槽。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WaterNetworkNumericKind {
    #[default]
    Height, // 世界Y高度；端点连接决定该值时由同步系统禁用。
    Width,     // 当前截面的完整水面宽度，单位米。
    Depth,     // 当前截面的玩法水深，单位米。
    FlowSpeed, // 当前点通往下一段的材质流速，单位米每秒。
}

impl WaterNetworkNumericKind {
    /// 左栏字段短标签。
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Height => "高度",    // 明确编辑世界Y而不是局部偏移。
            Self::Width => "宽度",     // 表示完整截面宽度。
            Self::Depth => "深度",     // 表示水下玩法深度。
            Self::FlowSpeed => "流速", // 表示沿样条下游的输运速度。
        }
    }

    /// 字段右侧单位。
    pub(super) const fn unit(self) -> &'static str {
        match self {
            Self::FlowSpeed => "m/s",                        // 流速使用米每秒。
            Self::Height | Self::Width | Self::Depth => "m", // 其余字段统一使用世界米。
        }
    }

    /// 共享 UI 控件精确输入规格；旧加减按钮不再与专业字段并存。
    pub(super) const fn spec(self) -> NumericInputSpec {
        match self {
            Self::Height => NumericInputSpec::f32(2)
                .with_min(NumericValue::F32(MIN_WATER_LEVEL_METERS)) // 与静水和地形作者安全范围一致。
                .with_max(NumericValue::F32(MAX_WATER_LEVEL_METERS)) // 防止极端Y破坏相机与网格精度。
                .with_step(NumericValue::F32(0.01)) // 精确到厘米即可稳定表达河面落差。
                .with_step_buttons(false) // 迁移后不再显示公共加减按钮。
                .with_live_update(false) // 字符缓冲只在提交时写作者数据。
                .with_replace_on_first_key(true), // 中键输入首字符替换旧值。
            Self::Width => NumericInputSpec::f32(2)
                .with_min(NumericValue::F32(MIN_MAP_WATER_SPLINE_WIDTH_METERS)) // 复用正式最小非退化宽度。
                .with_max(NumericValue::F32(MAX_MAP_WATER_SPLINE_WIDTH_METERS)) // 复用正式地图级最大宽度。
                .with_step(NumericValue::F32(0.01)) // 厘米级足以精调岸线。
                .with_step_buttons(false) // 旧半米按钮由连续拖动取代。
                .with_live_update(false) // 精确输入提交前不逐字符重建。
                .with_replace_on_first_key(true), // 首字符替换当前宽度。
            Self::Depth => NumericInputSpec::f32(2)
                .with_min(NumericValue::F32(MIN_MAP_WATER_SPLINE_DEPTH_METERS)) // 保持正式最小水深。
                .with_max(NumericValue::F32(MAX_MAP_WATER_SPLINE_DEPTH_METERS)) // 保持正式最大水深。
                .with_step(NumericValue::F32(0.01)) // 厘米级水深精调。
                .with_step_buttons(false) // 不保留重复加减入口。
                .with_live_update(false) // 字符提交形成唯一命令。
                .with_replace_on_first_key(true), // 首字符替换当前深度。
            Self::FlowSpeed => NumericInputSpec::f32(2)
                .with_min(NumericValue::F32(0.0)) // 零速合法并允许静止河段。
                .with_max(NumericValue::F32(MAX_MAP_WATER_FLOW_SPEED)) // 与冻结水域格式同上限。
                .with_step(NumericValue::F32(0.01)) // 百分之一米每秒足以调流纹。
                .with_step_buttons(false) // 旧四分之一米按钮完全移除。
                .with_live_update(false) // 字符只在Enter或失焦提交。
                .with_replace_on_first_key(true), // 首字符替换当前流速。
        }
    }

    /// 读取当前点中由本字段拥有的标量。
    pub(super) const fn point_value(self, point: &MapWaterSplinePoint) -> f32 {
        match self {
            Self::Height => point.position[1],   // 世界Y只读取坐标第二项。
            Self::Width => point.width,          // 宽度不串读水深。
            Self::Depth => point.depth,          // 深度不串读流速。
            Self::FlowSpeed => point.flow_speed, // 流速不串读段类别。
        }
    }

    /// 只写当前点中的一个字段；调用方已完成范围和连接约束校验。
    pub(super) const fn set_point_value(self, point: &mut MapWaterSplinePoint, value: f32) {
        match self {
            Self::Height => point.position[1] = value, // X/Z保持逐位不变。
            Self::Width => point.width = value,        // 只改完整截面宽度。
            Self::Depth => point.depth = value,        // 只改玩法水深。
            Self::FlowSpeed => point.flow_speed = value, // 只改作者流速。
        }
    }

    /// 复核UI消息中的有限闭区间值，异常消息不得进入作者草稿。
    pub(super) fn validate_value(self, value: f32) -> Result<f32, String> {
        let (minimum, maximum) = match self {
            Self::Height => (MIN_WATER_LEVEL_METERS, MAX_WATER_LEVEL_METERS), // 世界Y沿用统一作者范围。
            Self::Width => (
                MIN_MAP_WATER_SPLINE_WIDTH_METERS,
                MAX_MAP_WATER_SPLINE_WIDTH_METERS,
            ), // 宽度直接复用正式范围。
            Self::Depth => (
                MIN_MAP_WATER_SPLINE_DEPTH_METERS,
                MAX_MAP_WATER_SPLINE_DEPTH_METERS,
            ), // 深度直接复用正式范围。
            Self::FlowSpeed => (0.0, MAX_MAP_WATER_FLOW_SPEED), // 流速使用零至正式上限。
        };
        if !value.is_finite() || !(minimum..=maximum).contains(&value) {
            return Err(format!(
                "{}必须是{minimum:.2}～{maximum:.2}之间的有限数字",
                self.label(),
            )); // NaN、无穷和越界值全部失败关闭。
        }
        Ok(value) // 合法值原样交给稳定点写入。
    }

    /// 返回本字段对应的独立设置键。
    pub(super) const fn sensitivity_field(self) -> MapEditorScrubField {
        match self {
            Self::Height => MapEditorScrubField::WaterControlHeight, // 高度手感独立持久化。
            Self::Width => MapEditorScrubField::WaterControlWidth,   // 宽度手感独立持久化。
            Self::Depth => MapEditorScrubField::WaterControlDepth,   // 深度手感独立持久化。
            Self::FlowSpeed => MapEditorScrubField::WaterControlFlowSpeed, // 流速手感独立持久化。
        }
    }

    /// 读取并修复本字段当前持久灵敏度。
    pub(super) fn sensitivity(self, settings: &EditorSettings) -> f32 {
        let value = settings
            .map_editor_scrub_sensitivities
            .get(self.sensitivity_field()); // 统一注册表按逻辑字段而非控件实体保存。
        if value.is_finite() {
            value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX) // 手改设置仍服从公共安全范围。
        } else {
            1.0 // 非有限配置回退标准手感。
        }
    }

    /// 左值拖动和右倍率拖动共享同一公共配置。
    pub(super) fn scrub_config(self, settings: &EditorSettings) -> NumericInputScrubConfig {
        let (mapping, step, minimum, maximum) = match self {
            Self::Height => (
                NumericInputScrubMapping::Linear {
                    units_per_pixel: 0.10, // 一倍灵敏度每像素改变十厘米高度。
                },
                0.01, // 结果吸附厘米。
                MIN_WATER_LEVEL_METERS,
                MAX_WATER_LEVEL_METERS,
            ),
            Self::Width => (
                NumericInputScrubMapping::Exponential {
                    exponent_per_pixel: 0.018, // 正值宽度按比例变化，大小河道都可细调。
                },
                0.01, // 结果吸附厘米。
                MIN_MAP_WATER_SPLINE_WIDTH_METERS,
                MAX_MAP_WATER_SPLINE_WIDTH_METERS,
            ),
            Self::Depth => (
                NumericInputScrubMapping::Exponential {
                    exponent_per_pixel: 0.018, // 跨数量级水深仍保持相对手感。
                },
                0.01, // 结果吸附厘米。
                MIN_MAP_WATER_SPLINE_DEPTH_METERS,
                MAX_MAP_WATER_SPLINE_DEPTH_METERS,
            ),
            Self::FlowSpeed => (
                NumericInputScrubMapping::Linear {
                    units_per_pixel: 0.02, // 一倍灵敏度每五十像素改变一米每秒。
                },
                0.01, // 结果吸附百分之一米每秒。
                0.0,
                MAX_MAP_WATER_FLOW_SPEED,
            ),
        };
        NumericInputScrubConfig {
            mapping,                                            // 共享 UI 控件解释线性或指数映射。
            drag_step: step,                                    // 业务精度决定吸附档。
            minimum,                                            // 左拖不得越过正式下界。
            maximum,                                            // 左拖不得越过正式上界。
            sensitivity: self.sensitivity(settings),            // 每个字段自己的持久倍率。
            minimum_sensitivity: SCRUB_SENSITIVITY_MIN,         // 统一最细倍率。
            maximum_sensitivity: SCRUB_SENSITIVITY_MAX,         // 统一最快倍率。
            drag_threshold_pixels: SCRUB_DRAG_THRESHOLD_PIXELS, // 无动作单击和正式拖动的仲裁阈值。
            sensitivity_pixels_per_octave: SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE, // 对称倍率拖动尺度。
        }
    }
}

/// BSN固定槽位保存要挂载的字段身份。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WaterNetworkNumericInputSlot(pub(crate) WaterNetworkNumericKind);

/// 公共NumericInput根上的水网字段marker。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct WaterNetworkNumericInput(pub(super) WaterNetworkNumericKind); // 系统注册跨越authoring模块边界，marker类型需保持crate可见。

/// 跨帧输入冻结的稳定控制点身份；数组下标只用于建立目标，绝不进入事务。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WaterNetworkNumericTarget {
    pub(super) map_id: MapId,           // 防止同名特征跨地图串写。
    pub(super) feature_id: String,      // 河流或瀑布稳定ID。
    pub(super) point_id: String,        // 路径内稳定控制点ID。
    pub(super) waterfall: bool,         // true走瀑布集合，false走河流集合。
    pub(super) authoring_revision: u64, // 精确输入期间任何外部作者变化都使目标失效。
}

/// 一次中键精确输入冻结根实体和稳定作者目标。
#[derive(Clone, Debug)]
pub(super) struct WaterNetworkNumericFocus {
    pub(super) entity: Entity, // 提交消息必须来自同一公共根。
    pub(super) target: WaterNetworkNumericTarget, // 失焦点击切换路径也不会串写新选择。
}

/// 一次左拖只保存完整水域起点；中间帧不推进昂贵水网预览修订。
#[derive(Debug)]
pub(super) struct WaterNetworkNumericScrub {
    pub(super) entity: Entity, // Changed和Committed必须来自同一公共根。
    pub(super) kind: WaterNetworkNumericKind, // 拖动期间字段身份不可切换。
    pub(super) target: WaterNetworkNumericTarget, // 每帧只按稳定ID定位控制点。
    pub(super) before: MapWaterAsset, // 松手生成唯一完整前值撤销快照。
    pub(super) before_value: f32, // 建立事务时该稳定点单字段的精确值。
    pub(super) latest_value: f32, // 最近一次由本事务拥有的可见值。
    pub(super) aborted: bool,  // 冲突后持有取消态到松手，防止同次拖动重绑新点。
}

/// 四个水网数字根共享唯一焦点和唯一左拖事务。
#[derive(Resource, Default)]
pub(crate) struct WaterNetworkNumericInputState {
    pub(super) focus: Option<WaterNetworkNumericFocus>, // 当前中键字符输入身份。
    pub(super) scrub: Option<WaterNetworkNumericScrub>, // 当前左值拖动身份。
}

impl WaterNetworkNumericInputState {
    /// 关闭、切图或目标失效后释放全部冻结身份。
    pub(super) fn clear(&mut self) {
        *self = Self::default(); // 下一次交互必须从当前权威重新绑定。
    }
}

/// 从当前工具和选择建立稳定点目标。
pub(super) fn selected_target(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
) -> Option<WaterNetworkNumericTarget> {
    let draft = session.draft.as_ref()?; // 没有作者草稿时字段全部禁用。
    let water = draft.water.as_ref()?; // 当前地图必须有水域文档。
    let (feature_id, waterfall) = active_feature_identity(water, editor)?; // 工具必须选中河流或瀑布。
    let point = active_points(water, editor)?.get(editor.selected_water_control_point?)?; // 数组下标只在此刻解析一次。
    Some(WaterNetworkNumericTarget {
        map_id: draft.map_id.clone(),         // 冻结真实地图稳定身份。
        feature_id,                           // 冻结当前特征稳定ID。
        point_id: point.id.clone(),           // 冻结当前点稳定ID。
        waterfall,                            // 冻结所属集合。
        authoring_revision: session.revision, // 精确输入拒绝后续外部分支。
    })
}

/// 按稳定目标读取控制点。
pub(super) fn target_point<'a>(
    water: &'a MapWaterAsset,
    target: &WaterNetworkNumericTarget,
) -> Option<&'a MapWaterSplinePoint> {
    let points = if target.waterfall {
        &water
            .waterfalls
            .iter()
            .find(|feature| feature.id == target.feature_id)?
            .points // 稳定瀑布ID定位路径。
    } else {
        &water
            .rivers
            .iter()
            .find(|feature| feature.id == target.feature_id)?
            .points // 稳定河流ID定位路径。
    };
    points.iter().find(|point| point.id == target.point_id) // 稳定点ID拒绝数组重排误写。
}

/// 判断控制点Y是否由连接派生；此时高度字段必须禁用而非写后吸回。
pub(super) fn target_height_is_derived(
    water: &MapWaterAsset,
    target: &WaterNetworkNumericTarget,
) -> bool {
    if target.waterfall {
        let Some(feature) = water
            .waterfalls
            .iter()
            .find(|feature| feature.id == target.feature_id)
        else {
            return false; // 失效目标由整体禁用逻辑处理。
        };
        let Some(index) = feature
            .points
            .iter()
            .position(|point| point.id == target.point_id)
        else {
            return false; // 失效点不冒充连接约束。
        };
        return (index == 0 && feature.source.is_some())
            || (index + 1 == feature.points.len() && feature.target.is_some()); // 单点只要任一端连接就由同步规则决定Y。
    }
    let Some(feature) = water
        .rivers
        .iter()
        .find(|feature| feature.id == target.feature_id)
    else {
        return false; // 失效目标由整体禁用逻辑处理。
    };
    let Some(index) = feature
        .points
        .iter()
        .position(|point| point.id == target.point_id)
    else {
        return false; // 失效点不冒充连接约束。
    };
    (index == 0 && feature.source_layer_id.is_some())
        || (index + 1 == feature.points.len() && feature.target_layer_id.is_some()) // 河首河尾分别跟随已连接静水层。
}

/// 按稳定目标写单字段；高度连接约束和有限范围在任何草稿修改前复核。
pub(super) fn set_target_value(
    water: &mut MapWaterAsset,
    target: &WaterNetworkNumericTarget,
    kind: WaterNetworkNumericKind,
    value: f32,
) -> Result<bool, String> {
    let value = kind.validate_value(value)?; // 防御异常UI消息。
    if kind == WaterNetworkNumericKind::Height && target_height_is_derived(water, target) {
        return Err("当前控制点高度由水体连接决定；请先断开对应端点".to_owned()); // 禁止写后被连接同步吸回。
    }
    let point = point_mut_by_id(
        water,
        &target.feature_id,
        &target.point_id,
        target.waterfall,
    )
    .ok_or_else(|| "当前水网控制点稳定身份已经失效".to_owned())?; // 不退回旧数组下标。
    let before = kind.point_value(point); // 单字段足以判断当前吸附档是否变化。
    if before == value {
        return Ok(false); // 同值不改变草稿或状态栏。
    }
    kind.set_point_value(point, value); // 只写当前字段，连接同步由调用方随后统一执行。
    Ok(true) // 当前字段产生真实变化。
}

/// 读取目标字段值，供异常恢复和同步共用同一稳定路径。
pub(super) fn target_value(
    water: &MapWaterAsset,
    target: &WaterNetworkNumericTarget,
    kind: WaterNetworkNumericKind,
) -> Option<f32> {
    if kind == WaterNetworkNumericKind::Height {
        point_position_by_id(
            water,
            &target.feature_id,
            &target.point_id,
            target.waterfall,
        )
        .map(|position| position[1]) // 高度复用既有稳定位置读取函数。
    } else {
        target_point(water, target).map(|point| kind.point_value(point)) // 其他字段直接读取稳定点。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 纯字段逻辑不启动窗口或渲染器。
    use feathrise_map_content::{MapWaterRiver, MapWaterSegmentKind}; // 构造最小连接河流。

    fn point(id: &str, y: f32) -> MapWaterSplinePoint {
        MapWaterSplinePoint {
            id: id.to_owned(),                    // 测试稳定点身份。
            position: [0.0, y, 0.0],              // 只关心世界Y。
            width: 6.0,                           // 合法默认宽度。
            depth: 1.5,                           // 合法默认深度。
            flow_speed: 2.0,                      // 合法默认流速。
            outgoing: MapWaterSegmentKind::River, // 普通河段类别。
        }
    }

    #[test]
    fn connected_river_endpoint_disables_only_derived_height() {
        let mut water = MapWaterAsset::default(); // 从空水域构造两点河流。
        water.rivers.push(MapWaterRiver {
            id: "river".to_owned(),                               // 稳定特征ID。
            label: "测试河".to_owned(),                           // 作者显示名。
            source_layer_id: Some("lake".to_owned()),             // 首点高度由连接决定。
            target_layer_id: None,                                // 末点保持自由高度。
            points: vec![point("start", 5.0), point("end", 4.0)], // 两个稳定控制点。
            base_color_srgba: [0.1, 0.2, 0.3, 0.7],               // 合法水材质颜色。
            perceptual_roughness: 0.2,                            // 合法水材质粗糙度。
        });
        let start = WaterNetworkNumericTarget {
            map_id: water.map_id.clone(),   // 测试地图身份。
            feature_id: "river".to_owned(), // 指向河流。
            point_id: "start".to_owned(),   // 指向首点。
            waterfall: false,               // 使用河流集合。
            authoring_revision: 0,          // 纯函数不读取修订。
        };
        let mut end = start.clone(); // 末点共享同一河流身份。
        end.point_id = "end".to_owned(); // 只切换稳定点ID。
        assert!(target_height_is_derived(&water, &start)); // 首点高度必须禁用。
        assert!(!target_height_is_derived(&water, &end)); // 未连接末点仍可编辑高度。
        assert!(
            set_target_value(&mut water, &start, WaterNetworkNumericKind::Width, 8.0)
                .expect("连接端点宽度仍应可编辑")
        ); // 连接只拥有高度，不应禁用其他三个字段。
    }
}
