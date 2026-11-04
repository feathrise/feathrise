//! 当前静水层水位与公共专业数值控件的作者请求桥。
//!
//! 左键拖实际水位、右键拖独立灵敏度、中键进入精确输入；Esc只在字符编辑期间
//! 恢复进入前数值。本模块仅负责冻结作者身份，并把有效提交转为现有可撤销水位操作。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // 原生挂载公共控件、查询焦点并同步禁用态。
use feathrise_editor_core::EditorSettings; // 水位字段独立右拖灵敏度进入统一settings.json。
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputCommitOutcome,
    NumericInputCommitSource, NumericInputCommitted, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputScrubMapping, NumericInputScrubSensitivityChanged,
    NumericInputScrubSensitivityCommitted, NumericInputSetValue, NumericInputSpec,
    NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue,
    round_to_decimals, spawn_numeric_scrub_field,
}; // 不复制字符编辑、三种鼠标手势、自动fill、失焦提交、Esc字符恢复或视觉逻辑。

use crate::authoring::EditorAuthoringSession; // 当前草稿提供选中水层的稳定身份和正式水位。
use crate::components::{MapEditorWaterLevelInput, MapEditorWaterLevelInputSlot}; // BSN槽位与业务输入根。
use crate::constants::{
    MAX_WATER_LEVEL_METERS, MIN_WATER_LEVEL_METERS, WATER_LEVEL_INPUT_DECIMALS,
}; // UI校验与作者域共享同一业务边界。
use crate::resources::MapEditorState; // 保存输入绑定、精确请求和用户可见状态。
use crate::types::{WaterLevelInputTarget, WaterLevelSetRequest}; // 提交后按完整冻结身份进入作者域。

use super::scene::FONT_PATH; // 动态挂载控件仍复用编辑器统一中文字体。

const WATER_LEVEL_UNITS_PER_PIXEL: f32 = 0.1; // 一倍灵敏度下水平一像素改变零点一米。
const WATER_LEVEL_SCRUB_STEP: f32 = 0.001; // 左拖继续保留毫米级可复现吸附。
const SCRUB_SENSITIVITY_MIN: f32 = 0.01; // 水位最细允许标准手感的百分之一。
const SCRUB_SENSITIVITY_MAX: f32 = 100.0; // 水位最快限制为标准手感的一百倍。
const SCRUB_DRAG_THRESHOLD_PIXELS: f32 = 4.0; // 四像素以内的左右键位移保持无动作单击。
const SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE: f32 = 48.0; // 右拖四十八像素翻倍或减半。

fn water_level_scrub_sensitivity(settings: &EditorSettings) -> f32 {
    let value = settings.map_editor_scrub_sensitivities.water_level;
    if value.is_finite() {
        value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX)
    } else {
        1.0 // 手改配置中的NaN或无穷不能污染水位作者值。
    }
}

fn water_level_scrub_config(settings: &EditorSettings) -> NumericInputScrubConfig {
    NumericInputScrubConfig {
        mapping: NumericInputScrubMapping::Linear {
            units_per_pixel: WATER_LEVEL_UNITS_PER_PIXEL,
        },
        drag_step: WATER_LEVEL_SCRUB_STEP,
        minimum: MIN_WATER_LEVEL_METERS,
        maximum: MAX_WATER_LEVEL_METERS,
        sensitivity: water_level_scrub_sensitivity(settings),
        minimum_sensitivity: SCRUB_SENSITIVITY_MIN,
        maximum_sensitivity: SCRUB_SENSITIVITY_MAX,
        drag_threshold_pixels: SCRUB_DRAG_THRESHOLD_PIXELS,
        sensitivity_pixels_per_octave: SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE,
    }
}

/// 精确字符、左拖吸附和自动进度共同读取同一正式水位范围。
fn water_level_input_spec() -> NumericInputSpec {
    NumericInputSpec::f32(WATER_LEVEL_INPUT_DECIMALS)
        .with_min(NumericValue::F32(MIN_WATER_LEVEL_METERS))
        .with_max(NumericValue::F32(MAX_WATER_LEVEL_METERS))
        .with_step(NumericValue::F32(WATER_LEVEL_SCRUB_STEP))
        .with_step_buttons(false) // 用户要求精确输入，旧水位加减按钮不再并存。
        .with_live_update(false) // 一次Enter/失焦只形成一条撤销命令和一次水面换代。
        .with_replace_on_first_key(true) // 聚焦后首个数值直接替换旧水位，减少逐字删除。
}

/// 当前选择解析为完整作者目标与正式水位，所有输入同步只读取这一个入口。
fn selected_water_layer(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
) -> Option<(WaterLevelInputTarget, f32)> {
    let draft = session.draft.as_ref()?;
    let layer = draft
        .water
        .as_ref()?
        .layers
        .get(editor.selected_water_layer)?;
    Some((
        WaterLevelInputTarget {
            map_id: draft.map_id.clone(),
            layer_id: layer.id.clone(),
            authoring_revision: session.revision,
        },
        layer.water_level,
    ))
}

/// 首次聚焦尚未绑定不算过期；地图、层或作者修订任一变化都会使旧输入失效。
fn water_level_binding_is_stale(
    bound: Option<&WaterLevelInputTarget>,
    selected: Option<&WaterLevelInputTarget>,
) -> bool {
    selected.is_none() || bound.is_some_and(|bound| Some(bound) != selected)
}

/// 输入失败与同帧保存使用一个出口，避免旧值继续保存后覆盖真正错误状态。
fn reject_water_level_input(editor: &mut MapEditorState, reason: impl Into<String>) {
    let reason = reason.into();
    editor.status = if std::mem::take(&mut editor.save_requested) {
        format!("{reason}；本次保存已取消")
    } else {
        reason
    };
}

/// 在BSN固定槽位内用editor_ui builder挂载完整专业数值控件。
pub(crate) fn setup_water_level_input(
    mut commands: Commands,
    slots: Query<Entity, Added<MapEditorWaterLevelInputSlot>>,
    asset_server: Res<AssetServer>,
    settings: Res<EditorSettings>,
) {
    for slot in &slots {
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 与静态BSN文字使用同一字体资产。
        let spec = water_level_input_spec(); // 公共输入层与作者二次校验共享完整范围和毫米步进。
        let initial = NumericValue::F32(0.0); // 尚无水层绑定时安全显示零并由同步系统禁用。
        let style = NumericScrubFieldStyle {
            width: 125.0,          // builder占位；返回根立即适配独占整行的水位槽。
            height: 24.0,          // 与周围静水作者按钮行高一致。
            border_radius: 2.0,    // 延续紧凑属性框小圆角。
            column_gap: 2.0,       // “水位”、数值和单位保持紧凑间隔。
            label_font_size: ui_metrics::FONT_HINT,  // 与原水位标签字号一致。
            value_font_size: ui_metrics::FONT_CONTROL, // 毫米精度数值保持主要视觉层级。
            unit_font_size: 9.0,   // 米制单位作为辅助信息。
        };
        let options = NumericScrubFieldOptions::new(
            "水位",                              // 左侧稳定业务名。
            "m",                                 // 正式世界高度单位为米。
            initial,                             // builder首帧建立公共数值模型。
            spec,                                // 中键字符编辑保留三位小数。
            water_level_scrub_config(&settings), // 左拖水位与右拖独立倍率保持原手感。
        )
        .with_style(style); // 只覆盖水位栏所需外观，交互完全复用公共实现。
        commands.entity(slot).with_children(|slot| {
            let root = spawn_numeric_scrub_field(
                slot,                     // 完整控件作为固定槽位的唯一子项。
                MapEditorWaterLevelInput, // 业务marker直接挂在返回根上供消息查询。
                font.clone(),             // 标签、数值与单位共享F12中文字体。
                options,                  // builder一次生成文字、自动fill与三种鼠标手势。
            );
            slot.commands()
                .entity(root)
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                }); // 整行水位随侧栏伸缩，仍由原公共控件独占输入与事务。
        });
    }
}

/// 消费公共控件提交并冻结完整作者目标；字符编辑仍完全由共享 UI 控件处理。
pub(crate) fn handle_water_level_input(
    mut committed: MessageReader<NumericInputCommitted>,
    mut sensitivity_changed: MessageReader<NumericInputScrubSensitivityChanged>,
    mut sensitivity_committed: MessageReader<NumericInputScrubSensitivityCommitted>,
    active: Res<ActiveNumericInput>,
    inputs: Query<Entity, With<MapEditorWaterLevelInput>>,
    mut set_values: MessageWriter<NumericInputSetValue>,
    mut editor: ResMut<MapEditorState>,
    mut settings: ResMut<EditorSettings>,
    session: Res<EditorAuthoringSession>,
) {
    let Ok(input_entity) = inputs.single() else {
        committed.clear(); // 控件尚未挂载时丢弃其他界面的历史提交，不让reader无界滞留。
        sensitivity_changed.clear(); // 动态控件尚未生成时同样推进右拖即时消息reader。
        sensitivity_committed.clear(); // 无主右拖提交不得在控件稍后出现时误写设置。
        editor.water_level_input_target = None; // UI换代或异常重复根不得留下永久快捷键焦点门。
        editor.water_level_set_requested = None; // 没有对应控件时也不能保留无主作者请求。
        return;
    };
    let selected = selected_water_layer(&editor, &session);
    let input_is_active = active.entity == Some(input_entity);
    let binding_is_stale = water_level_binding_is_stale(
        editor.water_level_input_target.as_ref(),
        selected.as_ref().map(|(target, _)| target),
    );
    let input_context_invalid =
        !editor.open || editor.close_requested || editor.new_map_dialog_open || session.saving;
    let must_cancel = input_is_active && (input_context_invalid || binding_is_stale);
    if must_cancel {
        let value = selected.as_ref().map_or(0.0, |(_, level)| *level);
        set_values.write(NumericInputSetValue {
            entity: input_entity,
            value: NumericValue::F32(value),
            cancel_edit: true,
        }); // F12关闭、保存、切图或切层时绝不留下隐藏键盘焦点。
        editor.water_level_input_target = None;
    } else if input_is_active && editor.water_level_input_target.is_none() {
        if let Some((target, _)) = selected.as_ref() {
            editor.water_level_input_target = Some(target.clone()); // 首帧焦点冻结地图、修订与稳定层身份。
            editor.status = format!(
                "正在输入水位：Enter或点击空白提交，Esc恢复；范围{MIN_WATER_LEVEL_METERS:.0}～{MAX_WATER_LEVEL_METERS:.0}m"
            );
        }
    }

    let cursor_overrides_input = editor.water_level_from_cursor_requested.is_some(); // 点击“取地形水位”时丢弃同帧失焦文本提交。
    for event in committed.read() {
        if event.entity != input_entity {
            continue;
        }
        if event.source == NumericInputCommitSource::Scrub {
            continue; // 左拖由作者事务系统实时采用并在松开合并撤销，不能再冒充一次文本请求。
        }
        let target = editor
            .water_level_input_target
            .take()
            .or_else(|| selected.as_ref().map(|(target, _)| target.clone()));
        if cursor_overrides_input {
            continue; // 光标地形高度具有本次显式按钮操作的最终优先级。
        }
        if input_context_invalid {
            continue; // F12两种关闭入口和模态切换统一取消，绝不在隐藏过程中写作者数据。
        }
        let Some(target) = target else {
            editor.status = "当前地图没有可编辑湖泊/池塘".to_owned();
            continue;
        };
        match event.outcome {
            NumericInputCommitOutcome::Canceled => {
                editor.status = "已取消水位输入；原水位保持不变".to_owned();
                continue;
            }
            NumericInputCommitOutcome::Rejected => {
                reject_water_level_input(&mut editor, "水位不是有效数字，已恢复原值");
                continue;
            }
            NumericInputCommitOutcome::Accepted if !event.edited => {
                continue; // 未编辑失焦不得把高精度作者值悄悄量化为输入框的三位小数。
            }
            NumericInputCommitOutcome::Accepted => {}
        }
        let Some(water_level) = event.value.as_f32() else {
            reject_water_level_input(&mut editor, "水位输入控件返回了非浮点值，已保留原水位");
            continue;
        };
        if !water_level.is_finite()
            || !(MIN_WATER_LEVEL_METERS..=MAX_WATER_LEVEL_METERS).contains(&water_level)
        {
            reject_water_level_input(
                &mut editor,
                format!(
                    "水位必须是{MIN_WATER_LEVEL_METERS:.0}～{MAX_WATER_LEVEL_METERS:.0}m之间的有限数字；原值未修改"
                ),
            );
            continue; // 业务层拒绝越界值，不让公共控件的格式化悄悄改变正式数据。
        }
        editor.water_level_set_requested = Some(WaterLevelSetRequest {
            target,
            water_level,
        }); // 作者域随后在同一Input阶段复核身份、登记撤销并推进正式GPU水面修订。
    }
    for event in sensitivity_changed.read() {
        if event.entity == input_entity {
            editor.status = format!("正在调整水位拖动灵敏度：×{:.3}", event.value); // 水位数字继续显示实际米数，状态栏实时说明右拖倍率。
        }
    }
    for event in sensitivity_committed.read() {
        if event.entity != input_entity {
            continue;
        }
        let value = event
            .after
            .clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX);
        if settings.map_editor_scrub_sensitivities.water_level != value {
            settings.map_editor_scrub_sensitivities.water_level = value; // 松开右键才触发统一设置自动保存。
        }
        editor.status = format!("水位拖动灵敏度已设为 ×{value:.3}");
    }
}

/// 把当前草稿水位与可用性回写公共控件；活动编辑期间绝不覆盖用户缓冲。
pub(crate) fn sync_water_level_input(
    mut commands: Commands,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    active: Res<ActiveNumericInput>,
    active_scrub: Res<ActiveNumericInputScrub>,
    settings: Res<EditorSettings>,
    mut inputs: Query<
        (
            Entity,
            &NumericInputValue,
            Has<NumericInputDisabled>,
            &mut NumericInputScrubConfig,
        ),
        With<MapEditorWaterLevelInput>,
    >,
    mut set_values: MessageWriter<NumericInputSetValue>,
) {
    let Ok((input_entity, current, disabled, mut scrub)) = inputs.single_mut() else {
        return;
    };
    let selected = selected_water_layer(&editor, &session);
    let should_enable = editor.open
        && !editor.close_requested
        && !editor.new_map_dialog_open
        && !session.saving
        && selected.is_some();
    if should_enable == disabled {
        if should_enable {
            commands
                .entity(input_entity)
                .remove::<NumericInputDisabled>();
        } else {
            commands.entity(input_entity).insert(NumericInputDisabled);
        }
    }
    if active_scrub.entity != Some(input_entity) {
        let sensitivity = water_level_scrub_sensitivity(&settings);
        if scrub.sensitivity != sensitivity {
            scrub.sensitivity = sensitivity; // 启动加载或外部恢复设置后更新下一次拖动手感。
        }
    }
    if active.entity == Some(input_entity) || active_scrub.entity == Some(input_entity) {
        return; // 共享 UI 控件拥有字符缓冲或指针捕获时，作者同步不能覆盖当前显示。
    }
    let target = selected.as_ref().map_or(0.0, |(_, level)| {
        round_to_decimals(*level, WATER_LEVEL_INPUT_DECIMALS)
    });
    if current.0.as_f32() != Some(target) {
        set_values.write(NumericInputSetValue {
            entity: input_entity,
            value: NumericValue::F32(target),
            cancel_edit: true,
        }); // 切层、撤销、重做、取地形水位和草稿重载均走公共外部赋值桥。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 验证水位业务边界独立于渲染与完整编辑器场景。

    #[test]
    fn millimeter_precision_preserves_signed_water_levels() {
        assert_eq!(round_to_decimals(-2.75, WATER_LEVEL_INPUT_DECIMALS), -2.75);
        assert_eq!(
            round_to_decimals(37.125, WATER_LEVEL_INPUT_DECIMALS),
            37.125
        );
    }

    #[test]
    fn editor_water_level_range_rejects_non_finite_and_outside_values() {
        let accepted = |value: f32| {
            value.is_finite() && (MIN_WATER_LEVEL_METERS..=MAX_WATER_LEVEL_METERS).contains(&value)
        };
        assert!(accepted(-10_000.0));
        assert!(accepted(10_000.0));
        assert!(!accepted(-10_000.001));
        assert!(!accepted(10_000.001));
        assert!(!accepted(f32::NAN));
        assert!(!accepted(f32::INFINITY));
    }

    #[test]
    fn water_level_input_spec_matches_authoring_range_and_scrub_step() {
        let spec = water_level_input_spec();
        assert_eq!(
            spec.min.and_then(NumericValue::as_f32),
            Some(MIN_WATER_LEVEL_METERS)
        );
        assert_eq!(
            spec.max.and_then(NumericValue::as_f32),
            Some(MAX_WATER_LEVEL_METERS)
        );
        assert_eq!(
            spec.step.and_then(NumericValue::as_f32),
            Some(WATER_LEVEL_SCRUB_STEP)
        );
    }

    #[test]
    fn first_focus_binds_but_an_existing_binding_cannot_cross_authoring_identity() {
        let target = |map_id: &str, layer_id: &str, authoring_revision| WaterLevelInputTarget {
            map_id: feathrise_map_content::MapId::parse(map_id).expect("测试地图ID有效"),
            layer_id: layer_id.to_owned(),
            authoring_revision,
        };
        let river = target("MAP-a", "river", 7);
        assert!(!water_level_binding_is_stale(None, Some(&river))); // 首次聚焦必须允许建立绑定。
        assert!(!water_level_binding_is_stale(Some(&river), Some(&river)));
        assert!(water_level_binding_is_stale(
            Some(&river),
            Some(&target("MAP-a", "pond", 7))
        ));
        assert!(water_level_binding_is_stale(
            Some(&river),
            Some(&target("MAP-b", "river", 7))
        ));
        assert!(water_level_binding_is_stale(
            Some(&river),
            Some(&target("MAP-a", "river", 8))
        ));
        assert!(water_level_binding_is_stale(Some(&river), None));
    }
}
