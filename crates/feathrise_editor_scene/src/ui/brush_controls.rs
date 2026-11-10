//! F12中央视口四个上下文笔刷字段与公共专业数值控件的业务桥。
//!
//! 【功能设计】左键水平拖动实时修改实际数值，右键水平拖动只修改当前字段的
//! 独立灵敏度，中键进入精确字符输入；Esc只在字符编辑期间恢复进入前数值。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN静态槽位、动态公共控件挂载和业务资源查询。
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::SceneEditorScrubField; // 四个字段的右拖灵敏度进入统一settings.json持久化。
use crate::ui::theme as colors;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputChanged, NumericInputConfig, NumericInputCommitOutcome,
    NumericInputCommitSource, NumericInputCommitted, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputScrubMapping, NumericInputScrubSensitivityChanged,
    NumericInputScrubSensitivityCommitted, NumericInputSetValue, NumericInputSpec,
    NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue,
    spawn_numeric_scrub_field,
}; // 一次复用公共完整子树、三种手势、自动fill、显示模型与业务消息。

use crate::components::{SceneEditorBrushSlider, SceneEditorBrushSliderKind}; // 静态槽位与公共控件根共享稳定字段身份。
use crate::resources::SceneEditorState; // 数值立即更新统一笔刷半径、强度和地形高度调整速度。
use crate::types::EditorTool; // 控件可见性必须与当前工具实际消费参数一致。

use super::scene::FONT_PATH; // 与完整F12外壳共享同一中文字体资产。
use super::viewport_toolbar::ViewportBrushParameters; // 原字段适用规则只控制同排参数显隐，不改变工具栏高度。

pub(crate) const BRUSH_RADIUS_MIN_METERS: f32 = 2.0; // 与原快捷键允许的最细世界笔刷一致。
pub(crate) const BRUSH_RADIUS_MAX_METERS: f32 = 1_000.0; // 保留大场景快速铺陈所需的一公里上限。
/// 局部三维笔刷与世界高度笔刷使用不同尺度，字符输入、拖动及快捷键必须共同切换。
pub(crate) fn brush_radius_bounds(editor: &SceneEditorState) -> (f32, f32) {
    if editor.tool.is_volume_sculpt() {
        (0.5, 8.0)
    } else {
        (BRUSH_RADIUS_MIN_METERS, BRUSH_RADIUS_MAX_METERS)
    }
}
pub(crate) const BRUSH_STRENGTH_MIN: f32 = 0.05; // 最弱档仍产生明确且可见的作者变化。
pub(crate) const BRUSH_STRENGTH_MAX: f32 = 1.0; // 百分之百为单次笔刷完整作用。
pub(crate) const TERRAIN_SPEED_MIN_METERS_PER_SECOND: f32 = 0.5; // 半米每秒配合低强度可完成小高度精修。
pub(crate) const TERRAIN_SPEED_MAX_METERS_PER_SECOND: f32 = 100.0; // 百米每秒允许大场景快速塑形。

const SCRUB_DRAG_THRESHOLD_PIXELS: f32 = 4.0; // 四物理像素以内仍按普通左键单击处理。
const SCRUB_SENSITIVITY_MIN: f32 = 0.01; // 最细允许标准手感的百分之一。
const SCRUB_SENSITIVITY_MAX: f32 = 100.0; // 最快限制为标准手感的一百倍。
const SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE: f32 = 48.0; // 右拖四十八像素使当前倍率翻倍或减半。
const EXPONENTIAL_SCRUB_PER_PIXEL: f32 = std::f32::consts::LN_2 / 48.0; // 一倍灵敏度下四十八像素使正数翻倍。

/// 四种高度雕刻共用米制速度；三维密度刷与纹理绘制仍保留各自的有效参数。
fn brush_field_applies(kind: SceneEditorBrushSliderKind, editor: &SceneEditorState) -> bool {
    // 扩展散布与地表工具共用同一可见性及输入规则。
    if editor.domain_brush_active {
        // 扩展散布圆刷只消费半径和最小间距。
        return matches!(
            kind,
            SceneEditorBrushSliderKind::Radius | SceneEditorBrushSliderKind::Spacing
        ); // 不显示无效强度或升降速度。
    } // 普通地表工具继续使用原字段规则。
    if matches!(kind, SceneEditorBrushSliderKind::Spacing) {
        return false;
    } // 最小间距只属于扩展散布散布。
    let tool = editor.tool; // 保留地表工具的参数适用范围。
    match kind {
        // 高度雕刻统一用每秒最大位移控制快慢，平滑和压平也不能隐藏实际消费的速度。
        SceneEditorBrushSliderKind::TerrainSpeed => {
            matches!(tool, EditorTool::Raise | EditorTool::Lower | EditorTool::Smooth | EditorTool::Flatten)
        } // 三维密度工具尚未接入米制高度速度，不能展示无效参数。
        _ => matches!(
            tool,
            EditorTool::Raise
                | EditorTool::VolumeDig
                | EditorTool::VolumeFill
                | EditorTool::VolumeSmooth
                | EditorTool::VolumeFlatten
                | EditorTool::Lower
                | EditorTool::Smooth
                | EditorTool::Flatten
                | EditorTool::Surface
                | EditorTool::Puddles // 积水同样消费半径与强度，不显示地形升降速度。
                | EditorTool::Vegetation
                | EditorTool::Water
        ), // 查看、摆放及样条工具不显示无效笔刷参数。
    }
}

/// 一次笔刷左拖冻结业务raw起点和最后一次由本事务写入的raw值。
#[derive(Clone, Copy, Debug)]
struct BrushNumericScrub {
    entity: Entity,                 // 共享 UI 控件公共根身份，根换代后旧事务不能串写。
    kind: SceneEditorBrushSliderKind, // 半径、强度和速度各自独立。
    raw_start: f32,                 // 不经过显示精度量化的进入拖动前业务值。
    latest_raw: f32,                // 最近一次Changed实际写入SceneEditorState的位级值。
}

/// 笔刷没有场景撤销，但仍需保留raw起点并隔离异常物理拖动的迟到Changed。
#[derive(Resource, Default)]
pub(crate) struct BrushNumericInputState {
    scrub: Option<BrushNumericScrub>, // 首个真实Changed才建立的轻量事务。
    aborted_scrub: Option<(Entity, SceneEditorBrushSliderKind)>, // 外部/模态换代后等到对应release的拒绝门。
}

impl BrushNumericInputState {
    /// 异常结束后继续吞掉同一次左键按住期间的Changed。
    fn abort_until_release(&mut self, entity: Entity, kind: SceneEditorBrushSliderKind) {
        self.aborted_scrub = Some((entity, kind)); // Copy身份足以隔离，不保留任何作者对象。
    }

    /// 对应Scrub松键只释放拒绝门，已经恢复的raw不得再次提交。
    fn release_aborted(&mut self, entity: Entity, kind: SceneEditorBrushSliderKind) -> bool {
        if self.aborted_scrub != Some((entity, kind)) {
            return false; // 非对应字段继续走正常事务路径。
        }
        self.aborted_scrub = None; // 下一次独立按下可以重新冻结raw起点。
        true
    }
}

/// BSN静态树只使用这个marker预留槽位，业务marker只挂在builder返回的根上。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BrushNumericInputSlot(SceneEditorBrushSliderKind); // 仅因Bevy公开系统签名提升可见性，不作为模块API导出。

/// 声明一个笔刷数值槽；Added系统再用editor_ui builder挂载完整控件。
fn brush_slider_field(kind: SceneEditorBrushSliderKind) -> impl Scene {
    bsn! {
        BrushNumericInputSlot(kind)
        Node {
            min_width: px(160), // 窄窗保留可读字宽，多余字段横滚，不挤成小框或增高。
            max_width: px(220), // 宽窗口仍保留紧凑属性框，不拉成横贯视口的长轨道。
            flex_basis: px(160), // 宽窗适度伸展，横滚时仍保留一个完整字段。
            flex_grow: 1.0,
            flex_shrink: 0.0,
            height: px(26), // 与视图按钮共享桌面工具密度，保留完整鼠标命中高度。
        }
    }
}

/// 与方向及投影按钮同排的参数组；没有适用参数时收起该组，但不重建数值根或改变工具栏行高。
pub(super) fn brush_controls_panel() -> impl Scene {
    bsn! {
        ViewportBrushParameters
        Node {
            display: Display::None, // 首次业务同步前不短暂暴露失效字段。
            min_width: px(0),
            height: px(26), // 与视图按钮共享中心线，工具栏始终只占一排。
            flex_grow: 1.0,
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::FlexStart,
            column_gap: px(6),
        }
        Children [
            @brush_slider_field(SceneEditorBrushSliderKind::Radius)-- // builder随后生成“笔刷大小”完整控件。
            @brush_slider_field(SceneEditorBrushSliderKind::Spacing)-- // 扩展散布间距复用公共输入桥。
            @brush_slider_field(SceneEditorBrushSliderKind::Strength)-- // 百分比显示与业务零至一转换保持不变。
            @brush_slider_field(SceneEditorBrushSliderKind::TerrainSpeed) // 雕刻速度继续使用指数左拖映射。
        ]
    }
}

/// 公共控件中显示的中文标签与笔刷业务字段一一对应。
fn field_label(kind: SceneEditorBrushSliderKind) -> &'static str {
    match kind {
        SceneEditorBrushSliderKind::Spacing => "最小间距",
        SceneEditorBrushSliderKind::Radius => "笔刷大小", // 世界笔刷半径。
        SceneEditorBrushSliderKind::Strength => "笔刷强度", // 单次笔刷作用百分比。
        SceneEditorBrushSliderKind::TerrainSpeed => "雕刻速度", // 地形每秒最大高度变化米数。
    }
}

/// 公共控件中显示的单位只做UI边界转换，不改正式业务存储。
fn field_unit(kind: SceneEditorBrushSliderKind) -> &'static str {
    match kind {
        SceneEditorBrushSliderKind::Spacing => "m",
        SceneEditorBrushSliderKind::Radius => "m", // 笔刷半径以米表示。
        SceneEditorBrushSliderKind::Strength => "%", // 强度以整数百分比表示。
        SceneEditorBrushSliderKind::TerrainSpeed => "m/s", // 雕刻速度以米每秒表示。
    }
}

/// 参数区统一为26px控件高度，标签/值/单位可读；交互结构仍完全由公共builder拥有。
fn brush_field_style() -> NumericScrubFieldStyle {
    NumericScrubFieldStyle {
        width: 160.0,          // 仅作为builder初值，随后将根宽度绑定到弹性槽位。
        height: 26.0,          // 与同组视图切换一致。
        border_radius: 3.0,
        column_gap: 4.0,       // 标签、数值和单位清晰分开，仍共用一个可交互根。
        label_font_size: ui_metrics::FONT_HINT,
        value_font_size: ui_metrics::FONT_CONTROL,
        unit_font_size: 10.0,  // 单位保持辅助层级，不改变业务显示换算。
    }
}

/// 返回当前业务值在公共数字框中的显示单位；强度在输入边界转换为整数百分比。
fn displayed_value(kind: SceneEditorBrushSliderKind, editor: &SceneEditorState) -> f32 {
    match kind {
        SceneEditorBrushSliderKind::Spacing => editor.brush_spacing,
        SceneEditorBrushSliderKind::Radius => editor.brush_radius,
        SceneEditorBrushSliderKind::Strength => editor.brush_strength * 100.0,
        SceneEditorBrushSliderKind::TerrainSpeed => editor.terrain_sculpt_speed,
    }
}

/// 直接读取笔刷算法实际使用的raw业务值，不经过百分比或显示小数转换。
fn raw_brush_value(kind: SceneEditorBrushSliderKind, editor: &SceneEditorState) -> f32 {
    match kind {
        SceneEditorBrushSliderKind::Spacing => editor.brush_spacing,
        SceneEditorBrushSliderKind::Radius => editor.brush_radius, // 世界半径raw允许来自快捷键的非显示档尾数。
        SceneEditorBrushSliderKind::Strength => editor.brush_strength, // 归一化强度raw不先乘100。
        SceneEditorBrushSliderKind::TerrainSpeed => editor.terrain_sculpt_speed, // 米每秒raw原样冻结。
    }
}

/// 只在字段仍等于本事务末值时恢复raw起点，后来同字段写入拥有更高优先级。
fn restore_brush_scrub_if_owned(
    editor: &mut SceneEditorState,    // 当前笔刷会话是唯一可写业务状态。
    transaction: BrushNumericScrub, // Copy事务包含raw起点与末值。
) -> bool {
    if raw_brush_value(transaction.kind, editor).to_bits() != transaction.latest_raw.to_bits() {
        return false; // 外部快捷键或模式逻辑已经接管同字段时绝不覆盖。
    }
    match transaction.kind {
        SceneEditorBrushSliderKind::Spacing => editor.brush_spacing = transaction.raw_start,
        SceneEditorBrushSliderKind::Radius => editor.brush_radius = transaction.raw_start, // 位级恢复隐藏半径尾数。
        SceneEditorBrushSliderKind::Strength => editor.brush_strength = transaction.raw_start, // 不经百分比往返恢复强度raw。
        SceneEditorBrushSliderKind::TerrainSpeed => {
            editor.terrain_sculpt_speed = transaction.raw_start; // 位级恢复隐藏速度尾数。
        }
    }
    true // 本事务仍拥有字段，恢复过程没有场景撤销或dirty语义。
}

/// 每个字段的精确输入范围与拖动吸附完全一致，避免单击输入产生另一套合法值域。
fn numeric_spec(kind: SceneEditorBrushSliderKind) -> NumericInputSpec {
    let (minimum, maximum, step, decimals) = match kind {
        SceneEditorBrushSliderKind::Spacing => (0.01, f32::MAX, 0.01, 2),
        SceneEditorBrushSliderKind::Radius => {
            (BRUSH_RADIUS_MIN_METERS, BRUSH_RADIUS_MAX_METERS, 0.5, 1)
        }
        SceneEditorBrushSliderKind::Strength => (
            BRUSH_STRENGTH_MIN * 100.0,
            BRUSH_STRENGTH_MAX * 100.0,
            1.0,
            0,
        ),
        SceneEditorBrushSliderKind::TerrainSpeed => (
            TERRAIN_SPEED_MIN_METERS_PER_SECOND,
            TERRAIN_SPEED_MAX_METERS_PER_SECOND,
            0.5,
            1,
        ),
    };
    NumericInputSpec::f32(decimals)
        .with_min(NumericValue::F32(minimum))
        .with_max(NumericValue::F32(maximum))
        .with_step(NumericValue::F32(step))
        .with_step_buttons(false)
        .with_live_update(false) // 字符缓冲只在Enter或失焦后进入业务；左拖仍由公共Changed实时发布。
        .with_replace_on_first_key(true)
}

fn numeric_spec_for_tool(
    kind: SceneEditorBrushSliderKind,
    editor: &SceneEditorState,
) -> NumericInputSpec {
    let spec = numeric_spec(kind);
    if kind == SceneEditorBrushSliderKind::Radius && editor.tool.is_volume_sculpt() {
        let (minimum, maximum) = brush_radius_bounds(editor);
        spec.with_min(NumericValue::F32(minimum))
            .with_max(NumericValue::F32(maximum))
            .with_step(NumericValue::F32(0.1))
    } else {
        spec
    }
}

/// 从统一设置取得当前字段倍率；非法手改配置在控件边界回退一倍并钳制。
fn scrub_sensitivity(kind: SceneEditorBrushSliderKind, settings: &EditorSettings) -> f32 {
    let value = match kind {
        SceneEditorBrushSliderKind::Spacing => settings.scene_editor_scrub_sensitivities.brush_spacing,
        SceneEditorBrushSliderKind::Radius => settings.scene_editor_scrub_sensitivities.brush_radius,
        SceneEditorBrushSliderKind::Strength => {
            settings.scene_editor_scrub_sensitivities.brush_strength
        }
        SceneEditorBrushSliderKind::TerrainSpeed => {
            settings.scene_editor_scrub_sensitivities.terrain_speed
        }
    };
    if value.is_finite() {
        value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX)
    } else {
        1.0 // NaN与无穷设置不能污染拖动映射。
    }
}

/// 创建字段专属相对拖动映射；指数参数保留大小与速度的低值精修空间。
fn scrub_config(kind: SceneEditorBrushSliderKind, sensitivity: f32) -> NumericInputScrubConfig {
    let (mapping, drag_step, minimum, maximum) = match kind {
        SceneEditorBrushSliderKind::Spacing => (
            NumericInputScrubMapping::Exponential {
                exponent_per_pixel: EXPONENTIAL_SCRUB_PER_PIXEL,
            },
            0.01,
            0.01,
            f32::MAX,
        ),
        SceneEditorBrushSliderKind::Radius => (
            NumericInputScrubMapping::Exponential {
                exponent_per_pixel: EXPONENTIAL_SCRUB_PER_PIXEL,
            },
            0.5,
            BRUSH_RADIUS_MIN_METERS,
            BRUSH_RADIUS_MAX_METERS,
        ),
        SceneEditorBrushSliderKind::Strength => (
            NumericInputScrubMapping::Linear {
                units_per_pixel: 1.0, // 显示值每像素一个百分点。
            },
            1.0,
            BRUSH_STRENGTH_MIN * 100.0,
            BRUSH_STRENGTH_MAX * 100.0,
        ),
        SceneEditorBrushSliderKind::TerrainSpeed => (
            NumericInputScrubMapping::Exponential {
                exponent_per_pixel: EXPONENTIAL_SCRUB_PER_PIXEL,
            },
            0.5,
            TERRAIN_SPEED_MIN_METERS_PER_SECOND,
            TERRAIN_SPEED_MAX_METERS_PER_SECOND,
        ),
    };
    NumericInputScrubConfig {
        mapping,
        drag_step,
        minimum,
        maximum,
        sensitivity,
        minimum_sensitivity: SCRUB_SENSITIVITY_MIN,
        maximum_sensitivity: SCRUB_SENSITIVITY_MAX,
        drag_threshold_pixels: SCRUB_DRAG_THRESHOLD_PIXELS,
        sensitivity_pixels_per_octave: SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE,
    }
}

fn scrub_config_for_tool(
    kind: SceneEditorBrushSliderKind,
    sensitivity: f32,
    editor: &SceneEditorState,
) -> NumericInputScrubConfig {
    let mut scrub = scrub_config(kind, sensitivity);
    if kind == SceneEditorBrushSliderKind::Radius && editor.tool.is_volume_sculpt() {
        (scrub.minimum, scrub.maximum) = brush_radius_bounds(editor);
        scrub.drag_step = 0.1;
    }
    scrub
}

/// BSN首次生成槽位后，通过editor_ui开箱即用builder挂载三个完整数值控件。
pub(crate) fn configure_brush_slider_focus(
    mut commands: Commands,
    tracks: Query<(Entity, &BrushNumericInputSlot), Added<BrushNumericInputSlot>>, // 只处理静态槽位，业务根不会再匹配。
    asset_server: Res<AssetServer>, // 动态builder使用与F12外壳一致的中文字体。
    editor: Res<SceneEditorState>,
    settings: Res<EditorSettings>,
) {
    let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 三个字段复用同一AssetServer句柄。
    for (slot_entity, slider) in &tracks {
        let kind = slider.0; // 闭包挂载前冻结当前字段身份。
        let spec = numeric_spec_for_tool(kind, &editor); // 精确输入和左拖共享当前工具范围。
        let initial = NumericValue::F32(displayed_value(kind, &editor)); // 首帧直接显示当前编辑器业务值。
        let options = NumericScrubFieldOptions::new(
            field_label(kind),                                      // builder生成稳定中文标签。
            field_unit(kind),                                       // builder生成业务单位。
            initial,                                                // builder同时建立公共数值模型。
            spec, // 中键字符输入使用原有精度与范围。
            scrub_config_for_tool(kind, scrub_sensitivity(kind, &settings), &editor), // 左拖映射与独立右拖倍率保持不变。
        )
        .with_style(brush_field_style()); // 只改变局部排版，不改变字符编辑、三种手势或数值映射。
        commands.entity(slot_entity).with_children(|slot| {
            let root = spawn_numeric_scrub_field(
                slot,                       // 公共控件挂在当前固定槽位。
                SceneEditorBrushSlider(kind), // 业务marker挂在builder返回的根上。
                font.clone(),               // 数值、标签与单位共享编辑器字体。
                options,                    // 一次生成文字、fill与全部手势组件。
            );
            slot.commands()
                .entity(root)
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = Val::Percent(100.0);
                    node.min_width = Val::Px(0.0);
                    node.max_width = Val::Percent(100.0);
                }); // 只绑定builder返回根的宽度；标签、数值、单位、fill及输入状态机仍由公共控件维护。
        });
    }
}

/// 把公共数字值写回笔刷业务状态；返回用户可见的最终说明。
fn apply_numeric_brush_value(
    kind: SceneEditorBrushSliderKind,
    value: f32,
    editor: &mut SceneEditorState,
) -> Option<String> {
    if !value.is_finite() {
        return None; // 公共控件已拒绝非法字符，业务边界再次拒绝非有限消息。
    }
    match kind {
        SceneEditorBrushSliderKind::Spacing => {
            editor.brush_spacing = value.max(0.01);
            Some(format!("最小间距已调整为 {:.2} 米", editor.brush_spacing))
        }
        SceneEditorBrushSliderKind::Radius => {
            let (minimum, maximum) = brush_radius_bounds(editor);
            let value = value.clamp(minimum, maximum);
            editor.brush_radius = value;
            Some(format!("笔刷大小已调整为 {value:.1} 米"))
        }
        SceneEditorBrushSliderKind::Strength => {
            let percent = value.clamp(BRUSH_STRENGTH_MIN * 100.0, BRUSH_STRENGTH_MAX * 100.0);
            editor.brush_strength = percent / 100.0; // 只有UI边界使用百分比，作者算法继续使用零至一强度。
            Some(format!("笔刷强度已调整为 {percent:.0}%"))
        }
        SceneEditorBrushSliderKind::TerrainSpeed => {
            let value = value.clamp(
                TERRAIN_SPEED_MIN_METERS_PER_SECOND,
                TERRAIN_SPEED_MAX_METERS_PER_SECOND,
            );
            editor.terrain_sculpt_speed = value;
            Some(format!("地形雕刻速度已调整为 {value:.1} 米/秒"))
        }
    }
}

/// 消费左拖实时值、精确输入提交和右拖灵敏度提交。
pub(crate) fn handle_brush_numeric_inputs(
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
    mut sensitivity_changed: MessageReader<NumericInputScrubSensitivityChanged>,
    mut sensitivity_committed: MessageReader<NumericInputScrubSensitivityCommitted>,
    sliders: Query<&SceneEditorBrushSlider>,
    mut state: ResMut<BrushNumericInputState>,
    mut editor: ResMut<SceneEditorState>,
    mut settings: ResMut<EditorSettings>,
) {
    for event in changed.read() {
        let Ok(slider) = sliders.get(event.entity) else {
            continue; // 其他界面的公共数字框由自己的业务reader处理。
        };
        if state.aborted_scrub.is_some() {
            continue; // 异常恢复后的余下物理Changed必须丢弃，不能重新冻结新raw起点。
        }
        let context_valid = editor.open
            && !editor.close_requested
            && !editor.new_scene_dialog_open
            && brush_field_applies(slider.0, &editor); // 切工具隐藏的字段同样不能继续消费拖动。
        if state.scrub.is_none() {
            if !context_valid {
                state.abort_until_release(event.entity, slider.0); // 首个Changed已经越过阈值，但上下文失效时等到松键再重试。
                continue;
            }
            let raw_start = raw_brush_value(slider.0, &editor); // 第一条Changed写入前冻结完整业务raw值。
            state.scrub = Some(BrushNumericScrub {
                entity: event.entity,  // 后续消息只能来自同一公共根。
                kind: slider.0,        // 字段身份在整个物理拖动中固定。
                raw_start,             // 高于显示精度的尾数原样保留。
                latest_raw: raw_start, // 第一条Changed应用前字段仍归起点值所有。
            });
        }
        let Some(transaction) = state.scrub else {
            continue; // 建立失败或异常分支不允许继续写值。
        };
        if transaction.entity != event.entity || transaction.kind != slider.0 || !context_valid {
            let _ = restore_brush_scrub_if_owned(&mut editor, transaction); // 只恢复仍由旧事务拥有的字段。
            state.scrub = None; // 完整raw事务已经安全结束。
            state.abort_until_release(event.entity, slider.0); // 余下物理Changed保持拒绝到当前release。
            editor.status = "笔刷拖动目标或界面已换代；未提交值已安全恢复".to_owned();
            continue;
        }
        if raw_brush_value(transaction.kind, &editor).to_bits() != transaction.latest_raw.to_bits()
        {
            state.scrub = None; // 后来同字段值已经接管，旧事务不能再写。
            state.abort_until_release(event.entity, slider.0); // 防止下一Changed重新begin覆盖后来值。
            editor.status = "笔刷字段已被后来状态接管；旧拖动已停止".to_owned();
            continue;
        }
        if let Some(value) = event.value.as_f32()
            && let Some(status) = apply_numeric_brush_value(slider.0, value, &mut editor)
        {
            if let Some(transaction) = state.scrub.as_mut() {
                transaction.latest_raw = raw_brush_value(slider.0, &editor); // 保存实际业务raw终点而不是显示字符串。
            }
            editor.status = status; // 左拖每个新吸附档即时驱动真实笔刷并回显。
        } else if let Some(transaction) = state.scrub.take() {
            let _ = restore_brush_scrub_if_owned(&mut editor, transaction); // 非法公共消息不留下半事务值。
            state.abort_until_release(event.entity, slider.0); // 同一次物理拖动剩余消息全部丢弃。
        }
    }
    for event in committed.read() {
        let Ok(slider) = sliders.get(event.entity) else {
            continue;
        };
        if !brush_field_applies(slider.0, &editor) {
            // 工具切换后的迟到提交不能修改已隐藏参数。
            if let Some(transaction) = state.scrub.take() {
                // 收口仍由旧事务拥有的拖动值。
                let _ = restore_brush_scrub_if_owned(&mut editor, transaction); // 与既有隐藏取消行为一致。
            }
            state.release_aborted(event.entity, slider.0); // 释放已结束的物理拖动拒绝门。
            continue;
        }
        if event.source == NumericInputCommitSource::Scrub {
            if state.release_aborted(event.entity, slider.0) {
                continue; // 异常raw已恢复，松键只解除拒绝门。
            }
            let Some(transaction) = state.scrub.take() else {
                continue; // 未产生Changed的左键点击不建立业务事务。
            };
            if transaction.entity != event.entity || transaction.kind != slider.0 {
                let _ = restore_brush_scrub_if_owned(&mut editor, transaction); // 错根提交不得遗留旧实时值。
                continue;
            }
            if !event.changed {
                let restored = restore_brush_scrub_if_owned(&mut editor, transaction); // 显示起终点相同时位级恢复raw。
                editor.status = if restored {
                    format!("{}已拖回显示起点；原始精度保持不变", field_label(slider.0))
                } else {
                    format!("{}已被后来状态接管；未覆盖其结果", field_label(slider.0))
                };
                continue; // no-op不再进入通用Accepted分支量化一次。
            }
            if raw_brush_value(transaction.kind, &editor).to_bits()
                == transaction.latest_raw.to_bits()
            {
                editor.status = format!("{}拖动已应用", field_label(slider.0)); // changed=true保持实时终点，不重复改写业务值。
            } else {
                editor.status = format!("{}已被后来状态接管；旧拖动未覆盖", field_label(slider.0)); // release前外部后写值优先。
            }
            continue;
        }
        match event.outcome {
            NumericInputCommitOutcome::Canceled => {
                editor.status = "已取消笔刷数值输入；原值保持不变".to_owned();
            }
            NumericInputCommitOutcome::Rejected => {
                editor.status = "笔刷数值不是有效数字，已恢复原值".to_owned();
            }
            NumericInputCommitOutcome::Accepted if event.edited => {
                if let Some(value) = event.value.as_f32()
                    && let Some(status) = apply_numeric_brush_value(slider.0, value, &mut editor)
                {
                    editor.status = status; // 精确输入和左拖松开共用同一业务范围转换。
                }
            }
            NumericInputCommitOutcome::Accepted => {} // 未编辑失焦不制造状态变化。
        }
    }
    for event in sensitivity_changed.read() {
        let Ok(slider) = sliders.get(event.entity) else {
            continue;
        };
        let label = match slider.0 {
            SceneEditorBrushSliderKind::Spacing => "最小间距",
            SceneEditorBrushSliderKind::Radius => "笔刷大小",
            SceneEditorBrushSliderKind::Strength => "笔刷强度",
            SceneEditorBrushSliderKind::TerrainSpeed => "雕刻速度",
        };
        editor.status = format!("正在调整{label}拖动灵敏度：×{:.3}", event.value); // 数字框继续显示实际值，状态栏实时说明右拖结果。
    }
    for event in sensitivity_committed.read() {
        let Ok(slider) = sliders.get(event.entity) else {
            continue;
        };
        let value = event
            .after
            .clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX);
        let (field, label) = match slider.0 {
            SceneEditorBrushSliderKind::Spacing => (SceneEditorScrubField::BrushSpacing, "最小间距"),
            SceneEditorBrushSliderKind::Radius => (SceneEditorScrubField::BrushRadius, "笔刷大小"),
            SceneEditorBrushSliderKind::Strength => (SceneEditorScrubField::BrushStrength, "笔刷强度"),
            SceneEditorBrushSliderKind::TerrainSpeed => {
                (SceneEditorScrubField::TerrainSpeed, "雕刻速度")
            }
        };
        if settings.scene_editor_scrub_sensitivities.get(field) != value {
            settings.scene_editor_scrub_sensitivities.set(field, value); // 比较完成后才取得ResMut可变引用，拖回起点不会误触发设置自动保存。
        }
        editor.status = format!("{label}拖动灵敏度已设为 ×{value:.3}");
    }
}

/// 同步快捷键、会话默认值和已加载灵敏度到四个公共数值框。
pub(crate) fn sync_brush_sliders(
    mut commands: Commands,
    mut editor: ResMut<SceneEditorState>,
    settings: Res<EditorSettings>,
    active_input: Res<ActiveNumericInput>,
    active_scrub: Res<ActiveNumericInputScrub>,
    mut state: ResMut<BrushNumericInputState>,
    mut sliders: Query<(
        Entity,
        &SceneEditorBrushSlider,
        &NumericInputValue,
        &mut NumericInputConfig,
        &mut NumericInputScrubConfig,
        Has<NumericInputDisabled>,
    )>,
    mut set_values: MessageWriter<NumericInputSetValue>,
    mut slots: Query<(&BrushNumericInputSlot, &mut Node), Without<ViewportBrushParameters>>, // 隐藏完整字段槽位。
    mut panels: Query<
        &mut Node,
        (With<ViewportBrushParameters>, Without<BrushNumericInputSlot>),
    >, // 与槽位查询显式互斥，参数组收起不影响外层固定行高。
) {
    let should_enable = editor.open && !editor.close_requested && !editor.new_scene_dialog_open; // 隐藏或模态上下文不得保留旧鼠标命中。
    let has_parameters = [
        SceneEditorBrushSliderKind::Radius,
        SceneEditorBrushSliderKind::Spacing,
        SceneEditorBrushSliderKind::Strength,
        SceneEditorBrushSliderKind::TerrainSpeed,
    ]
    .into_iter()
    .any(|kind| brush_field_applies(kind, &editor)); // 固定四个协议字段，不扫描场景或对象。
    for mut node in &mut panels {
        let display = if has_parameters {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    } // 非笔刷工具仅收起该组；失效输入和拖动仍由下面的原取消/禁用门收口。
    for (slot, mut node) in &mut slots {
        // 固定四个槽位，不扫描场景内容。
        let display = if brush_field_applies(slot.0, &editor) {
            Display::Flex
        } else {
            Display::None
        }; // 隐藏包含标签、数值和命中区的完整控件。
        if node.display != display {
            node.display = display;
        } // 稳定帧不重复触发布局。
    }
    let scrub_needs_fallback = state.scrub.is_some_and(|transaction| {
        !should_enable
            || !brush_field_applies(transaction.kind, &editor) // 参数失效时取消仍在进行的拖动。
            || active_scrub.entity != Some(transaction.entity)
            || !sliders
                .get(transaction.entity)
                .ok()
                .is_some_and(|(_, marker, _, _, _, _)| marker.0 == transaction.kind)
            || raw_brush_value(transaction.kind, &editor).to_bits()
                != transaction.latest_raw.to_bits()
    }); // 强制隐藏、根/捕获换代或外部同字段接管时先安全结束轻量事务。
    if scrub_needs_fallback {
        let transaction = state.scrub.take().expect("上方已确认存在orphaned笔刷拖动"); // Copy事务在清理前仍保留raw起点。
        let captured_root_still_current = active_scrub.entity == Some(transaction.entity)
            && sliders
                .get(transaction.entity)
                .ok()
                .is_some_and(|(_, marker, _, _, _, _)| marker.0 == transaction.kind); // 无MouseMotion的模态换代仍可能发生在左键按住期间。
        let restored = restore_brush_scrub_if_owned(&mut editor, transaction); // 外部同字段后写值不匹配latest时保持不变。
        if restored && editor.open {
            editor.status = format!(
                "{}拖动上下文已换代；未提交值已恢复",
                field_label(transaction.kind)
            );
        }
        if captured_root_still_current {
            state.abort_until_release(transaction.entity, transaction.kind); // 继续吞同一次按住周期的后续Changed。
        } else {
            state.aborted_scrub = None; // 根或捕获消失后旧release不会到达。
        }
    }
    if let Some((entity, kind)) = state.aborted_scrub {
        let captured_root_still_current = active_scrub.entity == Some(entity)
            && sliders
                .get(entity)
                .ok()
                .is_some_and(|(_, marker, _, _, _, _)| marker.0 == kind); // 只在原根仍被共享 UI 控件捕获时等待对应松键。
        if !captured_root_still_current {
            state.aborted_scrub = None; // 根换代时安全释放拒绝门。
        }
    }
    for (entity, slider, current, mut config, mut scrub, disabled) in &mut sliders {
        let should_enable = should_enable && brush_field_applies(slider.0, &editor); // 隐藏根同步禁用公共输入与鼠标捕获。
        if should_enable == disabled {
            if should_enable {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            } // 公共输入根统一拒绝隐藏F12和新建场景模态后的三种鼠标手势。
        }
        if active_input.entity != Some(entity) && active_scrub.entity != Some(entity) {
            let spec = numeric_spec_for_tool(slider.0, &editor);
            if config.0 != spec {
                config.0 = spec; // 模式切换更新现有控件，不重建或改写正在编辑的字符缓冲。
            }
            let range = scrub_config_for_tool(slider.0, scrub.sensitivity, &editor);
            if (scrub.minimum, scrub.maximum, scrub.drag_step)
                != (range.minimum, range.maximum, range.drag_step)
            {
                scrub.minimum = range.minimum;
                scrub.maximum = range.maximum;
                scrub.drag_step = range.drag_step;
            }
        }
        let target = displayed_value(slider.0, &editor);
        if active_input.entity != Some(entity)
            && active_scrub.entity != Some(entity)
            && current.0.as_f32() != Some(target)
        {
            set_values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(target),
                cancel_edit: true,
            }); // 快捷键等外部改值经公共模型桥回写，不直接篡改精确输入缓冲。
        }
        if active_scrub.entity != Some(entity) {
            let sensitivity = scrub_sensitivity(slider.0, &settings);
            if scrub.sensitivity != sensitivity {
                scrub.sensitivity = sensitivity; // 设置加载或外部恢复后更新下一次左拖手感。
            }
        }
    }
    // 进度底色由editor_ui根据NumericInputValue与映射类型自动同步，业务层不再重复计算。
}

/// 悬停和按压只强化紧凑数值区，不改变业务值或生成额外实体。
pub(crate) fn sync_brush_slider_interaction_styles(
    mut sliders: Query<
        (&UiInteraction, &mut BackgroundColor, &mut BorderColor),
        (Changed<UiInteraction>, With<SceneEditorBrushSlider>),
    >,
) {
    for (interaction, mut background, mut border) in &mut sliders {
        let (next_background, next_border) = match interaction {
            UiInteraction::None => (colors::INPUT_BG, colors::INPUT_BORDER),
            UiInteraction::Hovered => (colors::COMPACT_BUTTON_BG, colors::COPPER_BORDER),
            UiInteraction::Pressed => (colors::INPUT_ACTIVE_BG, colors::COPPER_BORDER),
        };
        if background.0 != next_background {
            background.0 = next_background;
        }
        let next_border = BorderColor::all(next_border);
        if *border != next_border {
            *border = next_border;
        }
    }
}

#[cfg(test)]
#[path = "brush_controls/tests.rs"]
mod tests; // 测试单独拆分，生产模块保持在项目五百行维护上限内。

#[cfg(test)] // 真实UI系统接线回归独立于纯笔刷算法。
#[path = "brush_controls/puddle_tests.rs"]
mod puddle_tests; // 验证控件显隐、数值提交和工具高亮。
