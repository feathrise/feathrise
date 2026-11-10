//! 植被连续参数的公共控件挂载、输入消费与增量同步系统。

use bevy::prelude::*; // ECS查询、Commands、资源和字体加载。
use feathrise_editor_core::EditorSettings; // 右拖松手持久化字段独立灵敏度。
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputChanged, NumericInputCommitOutcome,
    NumericInputCommitSource, NumericInputCommitted, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputScrubSensitivityChanged, NumericInputScrubSensitivityCommitted,
    NumericInputSetValue, NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle,
    NumericValue, spawn_numeric_scrub_field,
}; // 开箱即用字段拥有完整视觉、输入模型、左右拖和中键精确输入。

use crate::authoring::EditorAuthoringSession; // 当前作者草稿、修订、撤销和编译门。
use crate::resources::SceneEditorState; // 当前植被选择与用户可见状态。
use crate::ui::scene::FONT_PATH; // 动态公共字段与F12静态文字使用同一中文字体。

use super::transaction::{
    VegetationNumericFocus, apply_exact_value, apply_scrub_value, begin_scrub, finish_scrub, focus,
    restore_scrub_raw_origin, rollback_scrub, scrub_identity, scrub_mut, selected_numeric,
    set_focus, set_scrub, take_focus, take_scrub, target_is_current,
}; // 所有作者变更都通过稳定ID事务边界。
use super::{VegetationNumericInput, VegetationNumericInputSlot, VegetationNumericInputState}; // 根与槽位marker以及唯一焦点/拖动状态。

const VEGETATION_NUMERIC_FIELD_WIDTH: f32 = 206.0; // builder占位宽度；实际根在创建帧适配可调侧栏。

/// Added槽位只生成一次完整公共控件子树，业务不再手工拼标签、填充或命中组件。
pub(crate) fn setup_vegetation_numeric_inputs(
    mut commands: Commands,
    slots: Query<(Entity, &VegetationNumericInputSlot), Added<VegetationNumericInputSlot>>,
    asset_server: Res<AssetServer>,
    settings: Res<EditorSettings>,
) {
    let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 五个字段共享一个字体句柄。
    for (slot, marker) in &slots {
        let kind = marker.0; // 冻结字段身份供根marker、规格和设置槽共同使用。
        let options = NumericScrubFieldOptions::new(
            kind.label(),
            kind.unit(),
            kind.initial_value(),
            kind.spec(),
            kind.scrub_config(&settings),
        )
        .with_style(NumericScrubFieldStyle {
            width: VEGETATION_NUMERIC_FIELD_WIDTH, // 创建后使用槽位百分比宽度，不限制侧栏尺寸。
            height: 24.0,                          // 与邻近操作行保持可点击高度。
            ..default()                            // 字号、圆角和间距沿用公共专业样式。
        });
        commands.entity(slot).with_children(|parent| {
            let root = spawn_numeric_scrub_field(
                parent, VegetationNumericInput(kind), font.clone(), options,
            );
            parent.commands().entity(root).entry::<Node>().and_modify(
                |mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                },
            ); // 仅一次适配原公共控件根；数值规格、实时拖动和撤销协议不变。
        });
    }
}

/// 中键字符编辑提交和右键灵敏度提交；左拖Scrub由独立事务系统收口。
#[allow(clippy::too_many_arguments)] // 输入消息、稳定目标、设置和取消桥均是明确业务边界。
pub(crate) fn handle_vegetation_numeric_input(
    mut committed: MessageReader<NumericInputCommitted>,
    mut sensitivity_changed: MessageReader<NumericInputScrubSensitivityChanged>,
    mut sensitivity_committed: MessageReader<NumericInputScrubSensitivityCommitted>,
    active: Res<ActiveNumericInput>,
    inputs: Query<(Entity, &VegetationNumericInput)>,
    mut set_values: MessageWriter<NumericInputSetValue>,
    mut state: ResMut<VegetationNumericInputState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut settings: ResMut<EditorSettings>,
) {
    let active_vegetation = active.entity.and_then(|entity| {
        inputs
            .get(entity)
            .ok()
            .map(|(_, marker)| (entity, marker.0))
    }); // 只接管本面板根，水位和笔刷仍由各自reader处理。
    if let Some((entity, kind)) = active_vegetation {
        let selected = selected_numeric(kind, &editor, &session);
        let focus_matches = focus(&state).is_some_and(|focus| {
            focus.entity == entity
                && selected
                    .as_ref()
                    .is_some_and(|selected| selected.target == focus.target && selected.editable)
        });
        if !focus_matches {
            if let Some(selected) = selected.filter(|selected| selected.editable) {
                set_focus(
                    &mut state,
                    Some(VegetationNumericFocus {
                        entity,
                        target: selected.target,
                    }),
                ); // 中键激活首帧冻结场景、层、组、字段和作者修订。
                editor.status = format!(
                    "正在精确输入{}；Enter或点击空白提交，Esc恢复原值",
                    kind.label()
                );
            } else {
                set_values.write(NumericInputSetValue {
                    entity,
                    value: kind.initial_value(),
                    cancel_edit: true,
                }); // 锁定、规则切换或草稿失效时立即释放隐藏字符缓冲。
                set_focus(&mut state, None);
                editor.status = format!("当前{}不可编辑", kind.label());
            }
        }
    }

    for event in committed.read() {
        let Ok((_, marker)) = inputs.get(event.entity) else {
            continue; // 其他公共数字框的消息由各自业务reader消费。
        };
        if event.source == NumericInputCommitSource::Scrub {
            continue; // 左拖松手只允许事务系统形成一条作者命令。
        }
        let bound_target = focus(&state)
            .filter(|focus| focus.entity == event.entity)
            .map(|focus| focus.target.clone()); // 先复制冻结身份再结束焦点。
        if focus(&state).is_some_and(|focus| focus.entity == event.entity) {
            let _ = take_focus(&mut state); // Accepted/Canceled/Rejected均结束本次业务绑定。
        }
        match event.outcome {
            NumericInputCommitOutcome::Canceled => {
                editor.status = format!("已取消{}精确输入；原值保持不变", marker.0.label());
                continue;
            }
            NumericInputCommitOutcome::Rejected => {
                editor.status = format!("{}不是有效数字，已恢复原值", marker.0.label());
                continue;
            }
            NumericInputCommitOutcome::Accepted if !event.edited => {
                continue; // 未编辑失焦不得量化或重写作者高精度值。
            }
            NumericInputCommitOutcome::Accepted => {} // 有效字符提交进入作者域完整复核。
        }
        let target = bound_target.or_else(|| {
            selected_numeric(marker.0, &editor, &session).map(|selected| selected.target)
        }); // 防御中键激活和提交恰好发生在同一帧。
        let Some(target) = target else {
            editor.status = format!("{}提交时没有有效作者目标", marker.0.label());
            continue;
        };
        match apply_exact_value(&mut editor, &mut session, &target, event.value) {
            Ok(true) => {
                editor.status = format!(
                    "{}已精确设为{}；正在编译正式植被结果",
                    marker.0.label(),
                    display_value(event.value),
                );
            }
            Ok(false) => {
                editor.status = format!("{}保持{}", marker.0.label(), display_value(event.value));
            }
            Err(error) => editor.status = error, // 失败不污染草稿、撤销栈或编译修订。
        }
    }

    for event in sensitivity_changed.read() {
        let Ok((_, marker)) = inputs.get(event.entity) else {
            continue;
        };
        editor.status = format!(
            "正在调整{}拖动灵敏度：×{:.3}",
            marker.0.label(),
            event.value
        ); // 数字框继续显示实际业务值，状态栏单独说明倍率。
    }
    for event in sensitivity_committed.read() {
        let Ok((_, marker)) = inputs.get(event.entity) else {
            continue;
        };
        if marker.0.sensitivity_changed(&settings, event.after) {
            marker.0.set_sensitivity(&mut settings, event.after); // 拖回起点不取得可变资源，避免无意义自动保存。
        }
        editor.status = format!(
            "{}拖动灵敏度已设为 ×{:.3}",
            marker.0.label(),
            marker.0.sensitivity(&settings),
        );
    }

    let focused_is_valid = focus(&state).is_some_and(|focus| {
        target_is_current(&focus.target, &editor, &session)
            && selected_numeric(focus.target.kind, &editor, &session)
                .is_some_and(|selected| selected.editable)
    });
    if focus(&state).is_some() && active_vegetation.is_none() && !focused_is_valid {
        set_focus(&mut state, None); // 草稿换代后不能把迟到失焦消息写到另一层。
    }
}

/// 消费左拖Changed并实时修改一个小型作者标量，Scrub松手只形成一条完整撤销。
pub(crate) fn apply_vegetation_numeric_scrub(
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
    inputs: Query<(Entity, &VegetationNumericInput)>,
    mut state: ResMut<VegetationNumericInputState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
) {
    for event in changed.read() {
        let Ok((_, marker)) = inputs.get(event.entity) else {
            continue; // 只消费植被根的左拖实际值。
        };
        if state.scrub_is_aborted() {
            continue; // 外部换代回滚后，同一次物理拖动的第三个及后续Changed不得重新begin。
        }
        if scrub_identity(&state).is_none() {
            let Some(selected) = selected_numeric(marker.0, &editor, &session) else {
                state.abort_scrub_until_release(event.entity, marker.0); // 首个Changed无稳定目标时也等到松键再重试。
                editor.status = format!("当前没有可编辑的{}目标", marker.0.label());
                continue;
            };
            match begin_scrub(event.entity, &editor, &session, selected) {
                Ok(transaction) => set_scrub(&mut state, transaction), // 第一次真实变化才冻结起点。
                Err(error) => {
                    state.abort_scrub_until_release(event.entity, marker.0); // 建立失败后的余下Changed全部丢弃。
                    editor.status = error;
                    continue;
                }
            }
        }
        if scrub_identity(&state) != Some((event.entity, marker.0)) {
            if let Some(transaction) = take_scrub(&mut state) {
                rollback_scrub(&mut session, &transaction); // 跨根迟到消息不能遗留无历史参数变化。
            }
            state.abort_scrub_until_release(event.entity, marker.0); // 当前物理捕获根保持拒绝到对应松键。
            editor.status = "植被拖动输入根在事务中途发生变化，已恢复安全起点".to_owned();
            continue;
        }
        let result = {
            let Some(transaction) = scrub_mut(&mut state) else {
                continue;
            };
            apply_scrub_value(&editor, &mut session, transaction, event.value)
        };
        match result {
            Ok(true) => {
                editor.status = format!(
                    "正在拖动{}：{}；松手后编译一次正式结果",
                    marker.0.label(),
                    display_value(event.value),
                ); // 列表文字随草稿Changed即时刷新，正式大图编译仍等待松手。
            }
            Ok(false) => {} // 同一吸附档不重复更新状态或分配快照。
            Err(error) => {
                if let Some(transaction) = take_scrub(&mut state) {
                    rollback_scrub(&mut session, &transaction); // 目标失效时恢复仍由本拖动拥有的值。
                    state.abort_scrub_until_release(transaction.entity, transaction.target.kind); // 外部修订/场景换代后吞掉余下物理Changed。
                }
                editor.status = error;
            }
        }
    }

    for event in committed.read() {
        let Ok((_, marker)) = inputs.get(event.entity) else {
            continue;
        };
        if event.source != NumericInputCommitSource::Scrub {
            continue; // 中键精确输入由前一系统处理。
        }
        if state.release_aborted_scrub(event.entity, marker.0) {
            continue; // 异常事务已owned-field回滚；松键只清拒绝门，不进入finish或编译。
        }
        let Some(transaction) = take_scrub(&mut state) else {
            continue; // 未越过阈值或未改变吸附档不建立作者事务。
        };
        if transaction.entity != event.entity || transaction.target.kind != marker.0 {
            rollback_scrub(&mut session, &transaction);
            editor.status = "植被拖动结束消息与活动事务不匹配，已恢复安全起点".to_owned();
            continue;
        }
        if !event.changed {
            match restore_scrub_raw_origin(&mut session, &transaction) {
                Ok(true) => {
                    editor.status =
                        format!("{}已拖回显示起点；作者原始精度保持不变", marker.0.label()); // 只恢复草稿raw，不压入历史、不标dirty也不启动正式编译。
                }
                Ok(false) => {
                    editor.status = format!(
                        "{}拖动结束前当前字段已被后来作者状态接管；未覆盖其结果",
                        marker.0.label()
                    ); // 外部同字段后写值优先于旧拖动raw起点。
                }
                Err(error) => {
                    editor.status = format!(
                        "{}拖回显示起点时恢复作者精度失败：{error}",
                        marker.0.label()
                    )
                } // 损坏冻结快照必须明确报告而不是伪称没有修改。
            }
            continue; // 共享 UI 控件显示起终点相同，本次不进入正式finish与编译链。
        }
        match finish_scrub(&mut editor, &mut session, transaction) {
            Ok(true) => {
                editor.status = format!("{}拖动已应用；正在编译正式植被结果", marker.0.label());
            }
            Ok(false) => {
                editor.status = format!("{}已拖回原值；未产生场景修改", marker.0.label());
            }
            Err(error) => editor.status = error, // 校验失败已由事务恢复起点。
        }
    }
}

/// 撤销、切层、规则切换和设置加载后原地同步值、禁用状态与下一次拖动倍率。
#[allow(clippy::too_many_arguments)] // 四类公共模型状态需要在一个稳定遍历中保持一致。
pub(crate) fn sync_vegetation_numeric_inputs(
    mut commands: Commands,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    settings: Res<EditorSettings>,
    active: Res<ActiveNumericInput>,
    active_scrub: Res<ActiveNumericInputScrub>,
    mut state: ResMut<VegetationNumericInputState>,
    mut inputs: Query<(
        Entity,
        &VegetationNumericInput,
        &NumericInputValue,
        &mut NumericInputScrubConfig,
        Has<NumericInputDisabled>,
    )>,
    mut set_values: MessageWriter<NumericInputSetValue>,
) {
    let scrub_needs_fallback = state.scrub.as_ref().is_some_and(|transaction| {
        let root_matches = inputs
            .get(transaction.entity)
            .ok()
            .is_some_and(|(_, marker, _, _, _)| marker.0 == transaction.target.kind); // 根换代不能把旧事务串到同位置的新字段。
        let target_matches = target_is_current(&transaction.target, &editor, &session)
            && selected_numeric(transaction.target.kind, &editor, &session)
                .is_some_and(|selected| selected.editable); // 场景、稳定层组、修订、锁定和规则必须仍连续。
        !editor.open
            || editor.close_requested
            || editor.new_scene_dialog_open
            || session.saving
            || active_scrub.entity != Some(transaction.entity)
            || !root_matches
            || !target_matches
    }); // 强制隐藏、根/场景换代或物理捕获消失时必须先owned-field恢复，不能直接drop live事务。
    if scrub_needs_fallback {
        let transaction = take_scrub(&mut state).expect("上方已确认存在orphaned植被拖动"); // 保留完整raw起点直到恢复结束。
        let captured_root_still_current = active_scrub.entity == Some(transaction.entity)
            && inputs
                .get(transaction.entity)
                .ok()
                .is_some_and(|(_, marker, _, _, _)| marker.0 == transaction.target.kind); // 无MouseMotion的外部revision也可能在原左键仍按住时触发fallback。
        match restore_scrub_raw_origin(&mut session, &transaction) {
            Ok(true) => {
                if editor.open {
                    editor.status = format!(
                        "{}拖动目标已换代；未提交的实时值已恢复",
                        transaction.target.kind.label()
                    ); // Presentation兜底不压历史、不标dirty也不启动编译。
                }
            }
            Ok(false) => {} // 同字段已被外部作者接管或场景已卸载时保持其权威结果。
            Err(error) => {
                editor.status = format!(
                    "{}拖动目标换代且raw恢复失败：{error}",
                    transaction.target.kind.label()
                ); // 损坏事务明确报告，仍释放旧身份防止跨场景写入。
            }
        }
        if captured_root_still_current {
            state.abort_scrub_until_release(transaction.entity, transaction.target.kind); // Presentation回滚后继续吞同次拖动的后续Changed。
        } else {
            state.aborted_scrub = None; // 根或物理捕获真正消失时旧release不会到达。
        }
    }
    if !editor.open {
        let _ = take_focus(&mut state); // 强制隐藏后精确字符稳定身份也不能跨会话保留。
        state.aborted_scrub = None; // 关闭会话释放任何尚在等待旧release的拒绝门。
    } else if let Some((entity, kind)) = state.aborted_scrub {
        let aborted_root_is_current = active_scrub.entity == Some(entity)
            && inputs
                .get(entity)
                .ok()
                .is_some_and(|(_, marker, _, _, _)| marker.0 == kind); // 原业务根和物理捕获都存在才继续等待松键。
        if !aborted_root_is_current {
            state.aborted_scrub = None; // 根换代后旧release不会到达，安全解除拒绝门。
        }
    }
    for (entity, marker, current, mut scrub, disabled) in &mut inputs {
        let selected = selected_numeric(marker.0, &editor, &session);
        let should_enable = selected.as_ref().is_some_and(|selected| selected.editable);
        if should_enable == disabled {
            if should_enable {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            } // 锁定和组规则变化在同一Presentation阶段更新命中与公共暗色视觉。
        }
        if active_scrub.entity != Some(entity) {
            let sensitivity = marker.0.sensitivity(&settings);
            if scrub.sensitivity != sensitivity {
                scrub.sensitivity = sensitivity; // 设置重载只影响下一次拖动，不改变正在进行的快照。
            }
        }
        if active.entity == Some(entity) || active_scrub.entity == Some(entity) {
            continue; // 字符缓冲和指针捕获期间业务同步不得覆盖显示。
        }
        let target = selected.map_or_else(|| marker.0.initial_value(), |selected| selected.value);
        if current.0 != target {
            set_values.write(NumericInputSetValue {
                entity,
                value: target,
                cancel_edit: true,
            }); // 撤销、重做、切层和规则切换统一走公共外部赋值桥。
        }
    }
}

/// 状态栏以紧凑格式显示真实业务值，不把I32冒充浮点。
fn display_value(value: NumericValue) -> String {
    match value {
        NumericValue::U32(value) => value.to_string(),
        NumericValue::I32(value) => value.to_string(),
        NumericValue::F32(value) => format!("{value:.2}"),
    }
}
