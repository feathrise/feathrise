//! 固定动作表只投影有界快照；更新身份时取消旧点击和数字缓冲，防止跨动作误写。

use feathrise_editor_ui::UiInteraction;
use super::super::types::AssetPreviewSubAssetPanel;
use super::{components::*, resources::AnimationEditorState, types::AnimationView};
use crate::ui::theme as colors;
use bevy::{prelude::*, ui::InteractionDisabled};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick, NumericInputDisabled,
    NumericInputSetValue, NumericInputValue, NumericValue, round_to_decimals,
};

fn ratio(time: f32, duration: f32) -> f32 {
    if duration.is_finite() && duration > 0.0 {
        (time / duration).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn button_label(action: AnimationAction, view: &AnimationView) -> String {
    match action {
        AnimationAction::FitTimeline => "适配".into(),
        AnimationAction::ZoomIn => "＋".into(),
        AnimationAction::ZoomOut => "－".into(),
        AnimationAction::ToggleBoneFilter => if view.selected_bone_only {
            "所选骨骼"
        } else {
            "全部通道"
        }
        .into(),
        AnimationAction::PreviousBone => "上骨骼".into(),
        AnimationAction::NextBone => "下骨骼".into(),
        AnimationAction::MovePose => "移动 G".into(),
        AnimationAction::RotatePose => "旋转 R".into(),
        AnimationAction::InsertPose => "插入姿势".into(),
        AnimationAction::ToggleAutoKey => if view.auto_key {
            "自动插帧 开"
        } else {
            "自动插帧 关"
        }
        .into(),
        AnimationAction::ResetPose => "还原姿势".into(),
        AnimationAction::BindSelectedModel => "绑定所选模型".into(),
        AnimationAction::UnbindModel => "仅骨架".into(),
        AnimationAction::CopyKeys => "复制".into(),
        AnimationAction::PasteKeys => "粘贴".into(),
        AnimationAction::PreviousClip => "前动作".into(),
        AnimationAction::NextClip => "后动作".into(),
        AnimationAction::FirstFrame => "首帧".into(),
        AnimationAction::PreviousFrame => "前帧".into(),
        AnimationAction::PreviousKey => "前键".into(),
        AnimationAction::TogglePlayback => if view.playing { "暂停" } else { "播放" }.into(),
        AnimationAction::NextFrame => "后帧".into(),
        AnimationAction::NextKey => "后键".into(),
        AnimationAction::LastFrame => "末帧".into(),
        AnimationAction::ToggleLoop => if view.looping {
            "循环 开"
        } else {
            "循环 关"
        }
        .into(),
        AnimationAction::Save => if view.dirty { "保存 *" } else { "保存" }.into(),
        AnimationAction::Reload => "重载动作".into(),
        AnimationAction::Undo => "撤销".into(),
        AnimationAction::Redo => "重做".into(),
        AnimationAction::ToggleFps => format!("{} fps", view.fps),
        AnimationAction::CycleSpeed => format!("{}×", view.speed),
        AnimationAction::PreviousChannelPage => "前通道".into(),
        AnimationAction::NextChannelPage => "后通道".into(),
        AnimationAction::InsertKey => "插入关键帧".into(),
        AnimationAction::DeleteKey => "删除关键帧".into(),
        AnimationAction::CycleInterpolation => {
            if view.interpolation.is_empty() {
                "插值".into()
            } else {
                view.interpolation.clone()
            }
        }
    }
}

fn enabled(action: AnimationAction, view: &AnimationView) -> bool {
    if !view.visible || view.busy || view.clip_count == 0 {
        return false;
    }
    match action {
        AnimationAction::PreviousClip => view.clip_index > 0,
        AnimationAction::NextClip => view.clip_index + 1 < view.clip_count,
        AnimationAction::Save => view.editable && view.dirty && !view.pose_dirty,
        AnimationAction::Reload => !view.dirty,
        AnimationAction::Undo => view.editable && (view.can_undo || view.pose_dirty),
        AnimationAction::Redo => view.editable && view.can_redo,
        AnimationAction::PreviousChannelPage => view.page > 0,
        AnimationAction::NextChannelPage => (view.page + 1) * CHANNEL_SLOTS < view.channel_count,
        AnimationAction::CycleInterpolation => {
            view.editable && view.selected_channel.is_some() && !view.pose_dirty
        }
        AnimationAction::InsertKey => {
            view.editable && view.selected_channel.is_some()
        }
        AnimationAction::PreviousKey | AnimationAction::NextKey => view.selected_channel.is_some(),
        AnimationAction::DeleteKey => {
            view.editable && (!view.selected.0.is_empty() || view.selected_key.is_some())
        }
        AnimationAction::CopyKeys => !view.selected.0.is_empty(),
        AnimationAction::PasteKeys
        | AnimationAction::InsertPose
        | AnimationAction::MovePose
        | AnimationAction::RotatePose => view.editable,
        AnimationAction::ResetPose => view.pose_dirty,
        _ => true,
    }
}

fn set_disabled(commands: &mut Commands, entity: Entity, enabled: bool, disabled: bool) {
    if enabled == disabled {
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

fn background(interaction: &UiInteraction, enabled: bool, selected: bool) -> BackgroundColor {
    BackgroundColor(if !enabled {
        colors::SLOT_BG_DISABLED
    } else if *interaction == UiInteraction::Pressed {
        colors::SELECT_BG_PRESSED
    } else if *interaction == UiInteraction::Hovered {
        colors::SELECT_BG_HOVER
    } else if selected {
        colors::SELECT_BG
    } else {
        colors::SLOT_BG
    })
}

pub(super) fn sync(
    mut commands: Commands,
    state: Res<AnimationEditorState>,
    mut nodes: ParamSet<(
        Query<
            (
                &mut Node,
                Option<&AnimationPanel>,
                Option<&AnimationPoseToolbar>,
                Option<&AnimationRow>,
                Option<&AnimationPlayhead>,
                Option<&AnimationNumericSlot>,
                Option<&AssetPreviewSubAssetPanel>,
            ),
            Or<(
                With<AnimationPanel>,
                With<AnimationPoseToolbar>,
                With<AnimationRow>,
                With<AnimationPlayhead>,
                With<AnimationNumericSlot>,
                With<AssetPreviewSubAssetPanel>,
            )>,
        >,
        Query<(
            Entity,
            &mut AnimationKeyButton,
            &mut Node,
            &UiInteraction,
            &mut BackgroundColor,
            Has<InteractionDisabled>,
            Option<&mut EditorButtonClick>,
        )>,
    )>,
    mut buttons: Query<
        (
            Entity,
            &mut AnimationButton,
            &UiInteraction,
            &mut BackgroundColor,
            Has<InteractionDisabled>,
            Option<&mut EditorButtonClick>,
        ),
        Without<AnimationKeyButton>,
    >,
    mut channels: Query<
        (
            Entity,
            &mut AnimationChannelButton,
            &UiInteraction,
            &mut BackgroundColor,
            Has<InteractionDisabled>,
            Option<&mut EditorButtonClick>,
        ),
        (Without<AnimationButton>, Without<AnimationKeyButton>),
    >,
    mut tracks: Query<(&mut AnimationTrack, &ComputedNode)>,
    mut selection_boxes: Query<
        (&AnimationSelectionBox, &mut Node),
        (
            Without<AnimationPanel>,
            Without<AnimationPoseToolbar>,
            Without<AnimationRow>,
            Without<AnimationPlayhead>,
            Without<AnimationNumericSlot>,
            Without<AssetPreviewSubAssetPanel>,
            Without<AnimationKeyButton>,
        ),
    >,
    mut labels: Query<(&AnimationLabel, &mut Text)>,
    active: Res<ActiveNumericInput>,
    mut active_scrub: ResMut<ActiveNumericInputScrub>,
    mut inputs: Query<(
        Entity,
        &mut AnimationNumericInput,
        &NumericInputValue,
        Has<NumericInputDisabled>,
        Option<&mut feathrise_editor_ui::NumericInputScrubConfig>,
    )>,
    mut set_values: MessageWriter<NumericInputSetValue>,
) {
    let view = state.view();
    let mut capacities = [KEY_SLOTS; CHANNEL_SLOTS];
    for (track, node) in &tracks {
        if let Some(row) = track.row
            && node.size().x > 0.0
        {
            capacities[row] =
                ((node.size().x * node.inverse_scale_factor() / 14.0) as usize).clamp(1, KEY_SLOTS);
        }
    }
    let ready = view.visible && !view.busy && view.clip_count > 0;
    for (mut node, panel, toolbar, row, playhead, numeric, subassets) in &mut nodes.p0() {
        if panel.is_some() || toolbar.is_some() {
            let display = if view.visible {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        } else if let Some(row) = row {
            let display = if view.rows.get(row.0).is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        } else if playhead.is_some() {
            let left = percent(ratio(view.position - view.start, view.span) * 100.0);
            if node.left != left {
                node.left = left;
            }
        } else if let Some(slot) = numeric {
            let display = match slot.0 {
                AnimationNumericField::Value(axis) if axis >= view.key_values.len() && axis > 2 => {
                    Display::None
                }
                _ => Display::Flex,
            };
            if node.display != display {
                node.display = display;
            }
        } else if subassets.is_some() && view.visible && node.display != Display::None {
            // 动作表已提供完整通道入口；中央保留实际骨架画布，不再重复堆叠静态primitive目录。
            node.display = Display::None;
        }
    }
    for (entity, mut marker, mut node, interaction, mut color, disabled, click) in &mut nodes.p1() {
        let channel = view.rows.get(marker.row);
        // 窄栏以真实像素宽度进一步聚合图形，命中仍由完整曲线二分决定，避免菱形挤成一条白线。
        let stride = channel.map_or(1, |row| {
            row.keys.len().div_ceil(capacities[marker.row]).max(1)
        });
        let key = channel.and_then(|row| row.keys.get(marker.slot * stride));
        let channel_index = channel.map(|row| row.index);
        let index = key.map(|key| key.index);
        if marker.binding != view.binding
            || marker.channel != channel_index
            || marker.index != index
        {
            if let Some(mut click) = click {
                click.invalidate();
            }
            marker.binding = view.binding;
            marker.channel = channel_index;
            marker.index = index;
        }
        let display = if key.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some(key) = key {
            node.width = px(if key.clustered || stride > 1 {
                5.0
            } else {
                14.0
            });
            // 端点留出半个菱形命中宽度，首末帧均可完整点击。
            let left = percent(
                (ratio(
                    key.time + if key.selected { view.move_delta } else { 0.0 } - view.start,
                    view.span,
                ) * 100.0)
                    .clamp(1.0, 99.0),
            );
            if node.left != left {
                node.left = left;
            }
        }
        let enabled = ready && key.is_some();
        set_disabled(&mut commands, entity, enabled, disabled);
        let selected = key.is_some_and(|key| key.selected);
        color.set_if_neq(
            if enabled && !selected && *interaction == UiInteraction::None {
                BackgroundColor(Color::NONE) // 普通键只有菱形，避免通用按钮底色把密集动作画成一排方框。
            } else {
                background(interaction, enabled, selected)
            },
        );
    }
    for (entity, mut marker, interaction, mut color, disabled, click) in &mut buttons {
        if marker.binding != view.binding {
            if let Some(mut click) = click {
                click.invalidate();
            }
            marker.binding = view.binding;
        }
        let enabled = enabled(marker.action, &view);
        set_disabled(&mut commands, entity, enabled, disabled);
        let selected = (marker.action == AnimationAction::TogglePlayback && view.playing)
            || (marker.action == AnimationAction::ToggleLoop && view.looping)
            || (marker.action == AnimationAction::ToggleAutoKey && view.auto_key)
            || (marker.action == AnimationAction::MovePose
                && view.pose_tool == super::types::PoseTool::Move)
            || (marker.action == AnimationAction::RotatePose
                && view.pose_tool == super::types::PoseTool::Rotate);
        color.set_if_neq(background(interaction, enabled, selected));
    }
    for (entity, mut marker, interaction, mut color, disabled, click) in &mut channels {
        let row = view.rows.get(marker.slot);
        let index = row.map(|row| row.index);
        if marker.binding != view.binding || marker.index != index {
            if let Some(mut click) = click {
                click.invalidate();
            }
            marker.binding = view.binding;
            marker.index = index;
        }
        let enabled = ready && row.is_some();
        set_disabled(&mut commands, entity, enabled, disabled);
        color.set_if_neq(background(
            interaction,
            enabled,
            row.is_some_and(|row| row.selected),
        ));
    }
    for (marker, mut node) in &mut selection_boxes {
        if let Some((a, b, start, end)) = view
            .box_selection
            .filter(|(a, b, _, _)| marker.0 >= (*a).min(*b) && marker.0 <= (*a).max(*b))
        {
            let _ = (a, b);
            node.display = Display::Flex;
            let left = ratio(start.min(end) - view.start, view.span);
            node.left = percent(left * 100.0);
            node.width = percent((ratio(start.max(end) - view.start, view.span) - left) * 100.0);
        } else {
            node.display = Display::None;
        }
    }
    for (mut track, _) in &mut tracks {
        if track.binding != view.binding {
            track.binding = view.binding;
        }
    }
    for (kind, mut text) in &mut labels {
        let value = match *kind {
            AnimationLabel::Clip => {
                if view.clip_count > 0 {
                    format!(
                        "动作 {}/{} · {}",
                        view.clip_index + 1,
                        view.clip_count,
                        view.clip_name
                    )
                } else {
                    "动作编辑".into()
                }
            }
            AnimationLabel::Time => format!(
                "{:.1} / {:.1} 帧",
                view.position * view.fps as f32,
                view.duration * view.fps as f32
            ),
            AnimationLabel::Pages => format!("{}条通道 · 帧", view.channel_count),
            AnimationLabel::Bone => format!("骨骼：{}", view.bone_name),
            AnimationLabel::Model => {
                if !view.model_status.is_empty() {
                    view.model_status.clone()
                } else {
                    "在资产列表选角色，再绑定预览".into()
                }
            }
            AnimationLabel::Hint => "局部XYZ · 旋转单位为度 · 中键精确输入".into(),
            AnimationLabel::Status => {
                let state = if view.busy {
                    "正在处理"
                } else if !view.editable {
                    "只读"
                } else if view.dirty {
                    "未保存"
                } else {
                    "已保存"
                };
                if view.status.is_empty() {
                    state.into()
                } else {
                    format!("{state} · {}", view.status)
                }
            }
            AnimationLabel::Channel(slot) => view
                .rows
                .get(slot)
                .map(|row| {
                    let path = match row.path.as_str() {
                        "translation" => "位置",
                        "rotation" => "旋转",
                        "scale" => "缩放",
                        "weights" => "形态键",
                        _ => &row.path,
                    };
                    format!("{} · {path}", row.name)
                })
                .unwrap_or_default(),
            AnimationLabel::Tick(index) => format!(
                "{:.0}",
                (view.start + view.span * index as f32 / 8.0) * view.fps as f32
            ),
            AnimationLabel::Button(action) => button_label(action, &view),
        };
        if text.as_str() != value {
            **text = value;
        }
    }
    for (entity, mut marker, current, disabled, config) in &mut inputs {
        let target = match marker.field {
            AnimationNumericField::Frame => view.key_time.map(|time| time * view.fps as f32),
            AnimationNumericField::Value(axis) => view.key_values.get(axis).copied(),
        };
        let enabled = ready
            && view.editable
            && target.is_some()
            && !(view.pose_dirty && marker.field == AnimationNumericField::Frame);
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        let rebound = marker.binding != view.binding;
        if rebound {
            marker.binding = view.binding;
        }
        // 身份变化必须终止旧输入；同身份的表现刷新绝不覆盖正在录入或拖动的缓冲。
        let captured = active_scrub.entity == Some(entity);
        if captured && (rebound || !enabled) {
            // 公共SetValue只清字符焦点；本域仅在Committed写草稿，可直接结束旧捕获，避免松键改写新目标。
            *active_scrub = ActiveNumericInputScrub::default();
        }
        let editing = active.entity == Some(entity) || active_scrub.entity == Some(entity);
        if !editing
            && matches!(marker.field, AnimationNumericField::Value(_))
            && let Some(mut config) = config
        {
            let rotation = state
                .current()
                .and_then(|s| {
                    s.channel
                        .map(|i| s.document.clips[s.clip].channels[i].path.as_str())
                })
                .is_some_and(|path| path == "rotation");
            // 角度按半度/像素拖动，避免沿用四元数的千分之一灵敏度导致手感迟钝。
            let step = if rotation { 0.5 } else { 0.01 };
            config.mapping = feathrise_editor_ui::NumericInputScrubMapping::Linear {
                units_per_pixel: step,
            };
            config.drag_step = if rotation { 0.1 } else { 0.001 };
        }
        let decimals = match marker.field {
            AnimationNumericField::Frame => 3,
            AnimationNumericField::Value(_) => 5,
        };
        let target = round_to_decimals(target.unwrap_or(0.0), decimals);
        if rebound || (!enabled && editing) || (!editing && current.0.as_f32() != Some(target)) {
            set_values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(target),
                cancel_edit: true,
            });
        }
    }
}
