//! Blender整网格水体流速与世界Y高度偏移的专业数值交互域。
//!
//! 【功能设计】公共`NumericInput`拥有字符编辑和左右键拖动仲裁；本模块只冻结
//! placement稳定身份、实时定向写入单个字段、合并一次拖动撤销，并桥接两个独立设置倍率。

mod scene; // 固定槽位与公共NumericInput根的动态挂载。
mod sensitivity; // 右拖灵敏度只写两个独立设置槽，不参与作者撤销。
mod sync; // 选择、撤销与设置换代后的显示值、禁用态及灵敏度同步。
mod systems; // 精确提交、左拖事务、右拖持久化与禁用同步。
mod transaction; // 左拖稳定身份、正常收口与异常安全恢复。

use bevy::prelude::*; // Component、Entity与Resource等轻量ECS类型。
use feathrise_map_content::{
    MAX_MAP_PLACEMENT_WATER_FLOW_SPEED, MAX_MAP_PLACEMENT_WATER_HEIGHT_OFFSET_METERS, MapId,
    MapPlacementRecord, MapPlacementWaterSurface,
}; // 两个字段复用正式JSON范围与完整记录校验。
use feathrise_editor_core::EditorSettings; // 每个字段读取并写回自己的持久灵敏度。
use feathrise_editor_ui::{
    NumericInputScrubConfig, NumericInputScrubMapping, NumericInputSpec, NumericValue,
}; // 共享 UI 控件继续拥有点击、拖动、字符缓冲和显示规范化。

pub(super) use scene::placement_water_numeric_input_slots; // 父属性面板组合两个固定槽位。
pub(crate) use scene::setup_placement_water_numeric_inputs; // Presentation首次挂载公共控件根。
pub(crate) use sync::sync_placement_water_numeric_inputs; // Presentation同步显示值和可编辑状态。
pub(crate) use systems::{
    apply_placement_water_numeric_scrub, handle_placement_water_numeric_input,
}; // lib.rs在Input与Presentation阶段显式注册业务桥。

const FLOW_SPEED_DECIMALS: u8 = 2; // 米每秒流速以百分之一精度足以细调并保持短字段宽度。
const HEIGHT_OFFSET_DECIMALS: u8 = 3; // 世界Y偏移保留毫米级精确输入与吸附。
const FLOW_SPEED_UNITS_PER_PIXEL: f32 = 0.05; // 一倍灵敏度每二十像素改变一倍流速。
const HEIGHT_OFFSET_UNITS_PER_PIXEL: f32 = 0.1; // 一倍灵敏度每像素改变十厘米世界Y。
const FLOW_SPEED_DRAG_STEP: f32 = 0.01; // 左拖流速吸附到显示精度，避免隐藏尾数。
const HEIGHT_OFFSET_DRAG_STEP: f32 = 0.001; // 高度左拖保持毫米级可复现结果。
const SCRUB_SENSITIVITY_MIN: f32 = 0.01; // 最细允许标准手感的百分之一。
const SCRUB_SENSITIVITY_MAX: f32 = 100.0; // 最快限制为标准手感的一百倍。
const SCRUB_DRAG_THRESHOLD_PIXELS: f32 = 4.0; // 四像素以内的左右键位移视为无动作单击。
const SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE: f32 = 48.0; // 右拖四十八像素让本字段倍率翻倍或减半。

/// 两个数值根的业务身份；它同时决定范围、显示、拖动映射和设置字段。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PlacementWaterNumericKind {
    #[default]
    FlowSpeed, // 沿UV0.U下游输运的米每秒流速，范围零至二十。
    HeightOffset, // 只移动被赋水网格的绝对世界Y偏移，单位米。
}

impl PlacementWaterNumericKind {
    /// 属性面板中的短标签。
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::FlowSpeed => "流速", // 表示流纹沿作者米制UV0.U的下游移动速度，不表示流量。
            Self::HeightOffset => "Y偏移", // 明确它沿世界Y而不是网格局部法线移动。
        }
    }

    /// 字段后的紧凑单位。
    pub(super) const fn unit(self) -> &'static str {
        match self {
            Self::FlowSpeed => "m/s", // UV0.U按米制作后，时间输运值具有明确米每秒单位。
            Self::HeightOffset => "m", // 高度偏移使用世界米。
        }
    }

    /// 公共NumericInput的业务规格；中键精确输入不显示任何步进按钮。
    pub(super) const fn spec(self) -> NumericInputSpec {
        match self {
            Self::FlowSpeed => NumericInputSpec::f32(FLOW_SPEED_DECIMALS)
                .with_min(NumericValue::F32(0.0)) // 零速合法并让流纹保持静止。
                .with_max(NumericValue::F32(MAX_MAP_PLACEMENT_WATER_FLOW_SPEED)) // 与公共JSON校验同上限。
                .with_step(NumericValue::F32(FLOW_SPEED_DRAG_STEP)) // 虽不显示步进按钮，仍保持规格完整。
                .with_step_buttons(false) // 旧流速加减按钮被专业数值框完全替换。
                .with_live_update(false) // 字符输入只在Enter或失焦后形成一次作者命令。
                .with_replace_on_first_key(true), // 首个字符直接替换旧值，减少手工删除。
            Self::HeightOffset => NumericInputSpec::f32(HEIGHT_OFFSET_DECIMALS)
                .with_min(NumericValue::F32(
                    -MAX_MAP_PLACEMENT_WATER_HEIGHT_OFFSET_METERS,
                )) // 允许把Blender原网格向世界Y负方向移动一万米。
                .with_max(NumericValue::F32(
                    MAX_MAP_PLACEMENT_WATER_HEIGHT_OFFSET_METERS,
                )) // 世界Y正方向使用完全对称范围。
                .with_step(NumericValue::F32(HEIGHT_OFFSET_DRAG_STEP)) // 保持毫米级输入精度。
                .with_step_buttons(false) // 旧高度加减按钮不再与拖动控件并存。
                .with_live_update(false) // 字符缓冲不逐键热改运行时网格。
                .with_replace_on_first_key(true), // 精确输入首键替换当前偏移。
        }
    }

    /// 当前字段显示和同步使用的小数位数。
    pub(super) const fn decimals(self) -> u8 {
        match self {
            Self::FlowSpeed => FLOW_SPEED_DECIMALS, // 流速显示百分之一米每秒。
            Self::HeightOffset => HEIGHT_OFFSET_DECIMALS, // 世界Y显示毫米级米数。
        }
    }

    /// 从当前水绑定读取本字段值。
    pub(super) const fn surface_value(self, surface: &MapPlacementWaterSurface) -> f32 {
        match self {
            Self::FlowSpeed => surface.flow_speed, // 流速不读取高度或类别。
            Self::HeightOffset => surface.height_offset, // 高度不读取流速或网格原位姿。
        }
    }

    /// 只写当前水绑定中的本字段；调用方负责先完成有限范围检查。
    pub(super) const fn set_surface_value(
        self,                                   // 当前输入根决定唯一可写字段。
        surface: &mut MapPlacementWaterSurface, // 已按冻结下标和稳定身份复核的目标绑定。
        value: f32,                             // 已通过`validate_value`的有限业务值。
    ) {
        match self {
            Self::FlowSpeed => surface.flow_speed = value, // 流速拖动绝不触及高度、类别或稳定身份。
            Self::HeightOffset => surface.height_offset = value, // 高度拖动只改变世界Y米制偏移。
        }
    }

    /// 返回经有限范围检查后的值；异常UI消息必须在作者写入前失败关闭。
    pub(super) fn validate_value(self, value: f32) -> Result<f32, String> {
        let (minimum, maximum) = match self {
            Self::FlowSpeed => (0.0, MAX_MAP_PLACEMENT_WATER_FLOW_SPEED), // 公共流速闭区间。
            Self::HeightOffset => (
                -MAX_MAP_PLACEMENT_WATER_HEIGHT_OFFSET_METERS,
                MAX_MAP_PLACEMENT_WATER_HEIGHT_OFFSET_METERS,
            ), // 世界Y偏移的对称公共闭区间。
        };
        if !value.is_finite() || !(minimum..=maximum).contains(&value) {
            return Err(format!(
                "{}必须是{minimum:.0}～{maximum:.0}之间的有限数字",
                self.label(),
            )); // NaN、无穷或越界值不得进入placements JSON或GPU材质。
        }
        Ok(value) // 有限且位于公共闭区间时原样保留NumericInput规范化结果。
    }

    /// 读取本字段持久设置，并修复手改配置中的非有限或越界值。
    pub(super) fn sensitivity(self, settings: &EditorSettings) -> f32 {
        let value = match self {
            Self::FlowSpeed => {
                settings
                    .map_editor_scrub_sensitivities
                    .mesh_water_flow_speed
            } // 流速只读取自己的倍率槽。
            Self::HeightOffset => {
                settings
                    .map_editor_scrub_sensitivities
                    .mesh_water_height_offset
            } // 高度只读取自己的倍率槽。
        };
        if value.is_finite() {
            value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX) // 合法值仍服从公共安全范围。
        } else {
            1.0 // 非有限配置回退字段标准手感，不污染作者值。
        }
    }

    /// 右拖松开时只写当前字段的独立倍率槽。
    pub(super) fn set_sensitivity(self, settings: &mut EditorSettings, value: f32) -> bool {
        let value = if value.is_finite() {
            value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX) // 持久化前再次收口固定范围。
        } else {
            1.0 // 防御异常消息，正式设置文件永不写入NaN或无穷。
        };
        let stored = match self {
            Self::FlowSpeed => {
                &mut settings
                    .map_editor_scrub_sensitivities
                    .mesh_water_flow_speed
            } // 流速只取得自己的可变倍率槽。
            Self::HeightOffset => {
                &mut settings
                    .map_editor_scrub_sensitivities
                    .mesh_water_height_offset
            } // 高度只取得自己的可变倍率槽。
        };
        if *stored == value {
            return false; // 拖回原倍率或同值提交不得触发settings.json自动保存。
        }
        *stored = value; // 松开右键后只写一次当前字段槽。
        true // 调用方可据此区分真实持久化变化。
    }

    /// 左右拖动配置完全由公共共享 UI 控件解释，中键不进入该配置。
    pub(super) fn scrub_config(self, settings: &EditorSettings) -> NumericInputScrubConfig {
        let (units_per_pixel, drag_step, minimum, maximum) = match self {
            Self::FlowSpeed => (
                FLOW_SPEED_UNITS_PER_PIXEL,         // 流速使用线性米每秒/像素映射。
                FLOW_SPEED_DRAG_STEP,               // 结果吸附到百分之一米每秒。
                0.0,                                // 零速是合法下界。
                MAX_MAP_PLACEMENT_WATER_FLOW_SPEED, // 正式流速上界二十。
            ),
            Self::HeightOffset => (
                HEIGHT_OFFSET_UNITS_PER_PIXEL, // 高度使用线性米/像素映射。
                HEIGHT_OFFSET_DRAG_STEP,       // 结果吸附到毫米。
                -MAX_MAP_PLACEMENT_WATER_HEIGHT_OFFSET_METERS, // 世界Y负向下界。
                MAX_MAP_PLACEMENT_WATER_HEIGHT_OFFSET_METERS, // 世界Y正向上界。
            ),
        };
        NumericInputScrubConfig {
            mapping: NumericInputScrubMapping::Linear { units_per_pixel }, // 零值与正负高度都要求线性映射。
            drag_step,                                  // 左拖结果使用字段自身精度吸附。
            minimum,                                    // UI拖动不得越过公共作者下界。
            maximum,                                    // UI拖动不得越过公共作者上界。
            sensitivity: self.sensitivity(settings),    // 当前字段自己的持久倍率。
            minimum_sensitivity: SCRUB_SENSITIVITY_MIN, // 右拖最小倍率。
            maximum_sensitivity: SCRUB_SENSITIVITY_MAX, // 右拖最大倍率。
            drag_threshold_pixels: SCRUB_DRAG_THRESHOLD_PIXELS, // 无动作单击与正式拖动的固定仲裁阈值。
            sensitivity_pixels_per_octave: SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE, // 倍率指数拖动尺度。
        }
    }
}

/// BSN静态槽位只保存要挂载的字段身份。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PlacementWaterNumericInputSlot(pub(crate) PlacementWaterNumericKind);

/// 公共NumericInput根上的业务标记，消息由实体反查到字段身份。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct PlacementWaterNumericInput(pub(crate) PlacementWaterNumericKind);

/// 精确输入与拖动冻结的稳定作者身份；mesh_index不参与跨导出匹配。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlacementWaterNumericTarget {
    pub(super) map_id: MapId,           // 防止同名placement跨地图串写。
    pub(super) placement_id: String,    // 精确定位所属完整摆放记录。
    pub(super) record_index: usize,     // 完整结构签名提供的O(1)作者数组下标。
    pub(super) surface_index: usize,    // 第一次聚焦冻结的O(1)水绑定下标；结构代次变化立即失效。
    pub(super) node_path: Vec<String>,  // Blender对象的显式稳定路径。
    pub(super) mesh_name: String,       // Blender Mesh datablock显式名称。
    pub(super) authoring_revision: u64, // 精确输入期间拒绝任何外部作者修改。
    pub(super) placement_structure_revision: u64, // 增删/换源后旧下标必须立即失败关闭。
}

impl PlacementWaterNumericTarget {
    /// 当前绑定是否属于这条完整摆放记录中的目标网格。
    pub(super) fn matches_surface(&self, surface: &MapPlacementWaterSurface) -> bool {
        self.node_path == surface.node_path && self.mesh_name == surface.mesh_name // 不读取易漂移mesh_index。
    }

    /// 用冻结下标取得当前稳定绑定；即使同身份意外移动到别处也失败关闭而不退回线性扫描。
    pub(super) fn surface<'a>(
        &self,                          // 冻结placement、下标和Blender稳定身份。
        record: &'a MapPlacementRecord, // 当前草稿中的候选完整摆放记录。
    ) -> Option<&'a MapPlacementWaterSurface> {
        if record.id != self.placement_id {
            return None; // 下标串到另一placement时绝不读取其水字段。
        }
        record
            .water_surfaces
            .get(self.surface_index)
            .filter(|surface| self.matches_surface(surface)) // 结构未换代也继续防御损坏数组重排。
    }

    /// 用冻结下标取得唯一可写绑定；返回前复核完整稳定身份，拖中不扫描其余至多六十四项。
    pub(super) fn surface_mut<'a>(
        &self,                              // 冻结placement、下标和Blender稳定身份。
        record: &'a mut MapPlacementRecord, // 当前草稿中的唯一可变摆放记录。
    ) -> Option<&'a mut MapPlacementWaterSurface> {
        if record.id != self.placement_id {
            return None; // 错placement即使相同数组槽位也不能被旧事务串写。
        }
        record
            .water_surfaces
            .get_mut(self.surface_index)
            .filter(|surface| self.matches_surface(surface)) // 只验证目标项，不建立临时身份集合。
    }
}

/// 一次字符输入聚焦冻结输入根与作者身份。
#[derive(Clone, Debug)]
pub(super) struct PlacementWaterNumericFocus {
    pub(super) entity: Entity, // 允许两个字段之间失焦提交后同帧切换。
    pub(super) target: PlacementWaterNumericTarget, // 切换网格或作者修订时取消旧缓冲。
}

/// 一次左拖事务只保存完整起点和两个标量，松开才克隆最终记录形成一条PlacementChanged。
#[derive(Debug)] // 不实现Clone，编译期阻止松键或异常路径再次深复制完整起点记录。
pub(super) struct PlacementWaterNumericScrub {
    pub(super) entity: Entity, // 事务只能接收同一NumericInput根的Changed与Committed。
    pub(super) kind: PlacementWaterNumericKind, // 流速与高度不能在拖动中途串槽。
    pub(super) target: PlacementWaterNumericTarget, // 第一次真实Changed冻结稳定网格身份。
    pub(super) before: MapPlacementRecord, // 拖动前完整记录作为唯一撤销起点。
    pub(super) initial_value: f32, // 起点字段值用于无净变化判断和异常定向回滚。
    pub(super) latest_value: f32, // 最近一次已实时应用的字段值，不复制其余六十三项路径。
    pub(super) expected_revision: u64, // 只接受本事务自己连续推进的作者修订。
    pub(super) dirty_before: bool, // 拖回完整起点时恢复进入事务前的保存门。
}

/// 两个数值根共享唯一字符绑定与唯一左拖事务，符合共享 UI 控件全局单指针捕获。
#[derive(Resource, Default)]
pub(crate) struct PlacementWaterNumericInputState {
    pub(super) focus: Option<PlacementWaterNumericFocus>, // 当前中键字符输入身份。
    pub(super) scrub: Option<PlacementWaterNumericScrub>, // 当前左拖作者事务。
    pub(super) aborted_scrub: Option<(Entity, PlacementWaterNumericKind)>, // 异常回滚后吞掉同一次物理拖动的迟到Changed，直到对应松键。
    pub(super) placement_handoff_frozen: bool, // 交接进入边沿只发布一次明确禁用提示。
}

impl PlacementWaterNumericInputState {
    /// 编辑器关闭、目标失效或事务收口后释放全部冻结身份。
    pub(super) fn clear(&mut self) {
        *self = Self::default(); // 不保留跨地图或跨F12会话的网格引用。
    }

    /// 异常中止已经回滚的物理左拖，后续Changed不得重新建立作者事务。
    pub(super) fn abort_scrub_until_release(
        &mut self,                       // 唯一网格水输入状态。
        entity: Entity,                  // 共享 UI 控件仍捕获中的原业务根。
        kind: PlacementWaterNumericKind, // 流速和高度的松键不能互相清除拒绝门。
    ) {
        self.aborted_scrub = Some((entity, kind)); // 只保存Copy身份，不延长placement或路径生命周期。
    }

    /// 只要旧物理拖动尚未松开，就拒绝所有后续Changed重新begin。
    pub(super) const fn scrub_is_aborted(&self) -> bool {
        self.aborted_scrub.is_some() // 共享 UI 控件全局一次只捕获一个专业数值根。
    }

    /// 对应Scrub提交只负责释放拒绝门，不再形成作者收口。
    pub(super) fn release_aborted_scrub(
        &mut self,                       // 唯一网格水输入状态。
        entity: Entity,                  // 松键消息根必须与异常时冻结根一致。
        kind: PlacementWaterNumericKind, // 字段身份也必须一致，避免迟到旧消息误清新门。
    ) -> bool {
        if self.aborted_scrub != Some((entity, kind)) {
            return false; // 非对应松键由正常事务路径继续处理。
        }
        self.aborted_scrub = None; // 当前物理拖动已经结束，下一次按下可重新begin。
        true
    }
}

/// 拖中只按冻结下标修改一个专业数值；不克隆记录、不扫描其他绑定，也不做全记录校验。
pub(super) fn set_record_numeric_value_during_scrub(
    record: &mut MapPlacementRecord,      // 当前稳定placement完整记录。
    target: &PlacementWaterNumericTarget, // 指向已存在水绑定的稳定身份。
    kind: PlacementWaterNumericKind,      // 本次只允许修改一个字段。
    value: f32,                           // 已由NumericInput规范化的候选值。
) -> Result<bool, String> {
    let value = kind.validate_value(value)?; // 在任何作者写入前复核有限范围。
    let surface = target
        .surface_mut(record)
        .ok_or_else(|| "当前网格水绑定下标或稳定身份已经失效".to_owned())?; // 拖中不能线性寻找被重排到别处的同名项。
    let before = kind.surface_value(surface); // 当前目标单字段足以判定是否跨入新吸附档。
    if before == value {
        return Ok(false); // 同一位级值不推进修订、不热改材质也不制造历史。
    }
    kind.set_surface_value(surface, value); // 每个新档只写一个f32，完整校验推迟到松开边沿。
    Ok(true) // 当前字段产生真实、已通过自身有限范围门的变化。
}

/// 精确字符提交仍立即全量校验完整记录；失败时只恢复刚写入的目标字段。
pub(super) fn set_record_numeric_value(
    record: &mut MapPlacementRecord,      // 当前稳定placement完整记录。
    target: &PlacementWaterNumericTarget, // 指向已存在水绑定的冻结下标和稳定身份。
    kind: PlacementWaterNumericKind,      // 本次只允许修改一个字段。
    value: f32,                           // 已由NumericInput规范化的候选值。
) -> Result<bool, String> {
    let before = target
        .surface(record)
        .map(|surface| kind.surface_value(surface))
        .ok_or_else(|| "当前网格水绑定下标或稳定身份已经失效".to_owned())?; // 精确提交同样拒绝下标漂移。
    if !set_record_numeric_value_during_scrub(record, target, kind, value)? {
        return Ok(false); // 同值精确提交不创建临时集合或作者历史。
    }
    if let Err(error) = record.validate_water_surfaces() {
        let surface = target
            .surface_mut(record)
            .ok_or_else(|| "公共校验失败后目标水绑定也已失效".to_owned())?; // 单线程借用域内结构不应改变，仍失败关闭。
        kind.set_surface_value(surface, before); // 完整记录不合法时精确恢复唯一刚写字段。
        return Err(error); // 把正式数据错误原样交给作者状态栏。
    }
    Ok(true) // 当前字段产生真实、已通过公共格式校验的变化。
}

#[cfg(test)]
mod tests; // 字段独立性、范围、灵敏度和一次拖动历史在纯测试中锁定。
