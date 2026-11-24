//! 固定动作表只投影有界快照；更新身份时取消旧点击和数字缓冲，防止跨动作误写。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_animation_model::media::AssetPreviewSubAssetPanel;
use super::{
    components::*,
    docking::PropertySection,
    resources::AnimationEditorState,
    types::AnimationView,
};
use feathrise_editor_ui::colors;
use bevy::{prelude::*, ui::InteractionDisabled};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick, NumericInputDisabled,
    NumericInputSetValue, NumericInputValue, NumericValue, round_to_decimals,
    localization::EditorLocale,
};

fn ratio(time: f32, duration: f32) -> f32 {
    if duration.is_finite() && duration > 0.0 {
        (time / duration).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn button_label(
    action: AnimationAction,
    view: &AnimationView,
    in_place: bool,
    editing: bool,
    locale: EditorLocale,
) -> String {
    match action {
        AnimationAction::CreateClip => locale.text("animation.mode.create").into(),
        AnimationAction::ToggleEditing => locale.text(if editing {
            "animation.mode.exit"
        } else {
            "animation.mode.edit"
        }).into(),
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
        AnimationAction::InsertPose if view.pose_bones > 0 => format!("插入姿势（{} 骨）", view.pose_bones),
        AnimationAction::InsertPose => "插入姿势".into(),
        AnimationAction::ToggleAutoKey => if view.auto_key {
            "自动插帧 开"
        } else {
            "自动插帧 关"
        }
        .into(),
        AnimationAction::ResetPose => "还原姿势".into(),
        AnimationAction::BindSelectedModel => "绑定所选模型".into(),
        AnimationAction::UnbindModel if in_place => "结束动作预览".into(),
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

fn enabled(
    action: AnimationAction,
    state: &AnimationEditorState,
    view: &AnimationView,
    writing_ui: bool,
) -> bool {
    if view.busy {
        return false;
    }
    let author = writing_ui && view.editable;
    let direct_source = !state.in_place_rig_preview();
    match action {
        AnimationAction::CreateClip => state.can_create_clip(),
        AnimationAction::ToggleEditing => if state.editing_animation() {
            !view.pose_dirty
        } else {
            state.can_edit_selected()
        },
        // 撤销唯一的新动画之后，保存和历史仍属于当前源。
        AnimationAction::Save => author && view.dirty && !view.pose_dirty,
        AnimationAction::Reload => direct_source && author && !view.dirty,
        AnimationAction::Undo => author && (view.can_undo || view.pose_dirty),
        AnimationAction::Redo => author && view.can_redo,
        _ if !view.visible || view.clip_count == 0 => false,
        AnimationAction::PreviousClip => direct_source && view.clip_index > 0,
        AnimationAction::NextClip => direct_source && view.clip_index + 1 < view.clip_count,
        AnimationAction::BindSelectedModel => direct_source,
        AnimationAction::PreviousChannelPage => view.page > 0,
        AnimationAction::NextChannelPage => (view.page + 1) * CHANNEL_SLOTS < view.channel_count,
        AnimationAction::CycleInterpolation => {
            author && view.selected_channel.is_some() && !view.pose_dirty
        }
        AnimationAction::InsertKey => {
            author && view.selected_channel.is_some()
        }
        AnimationAction::PreviousKey | AnimationAction::NextKey => view.selected_channel.is_some(),
        AnimationAction::DeleteKey => {
            author && (!view.selected.0.is_empty() || view.selected_key.is_some())
        }
        AnimationAction::CopyKeys => writing_ui && !view.selected.0.is_empty(),
        AnimationAction::PasteKeys
        | AnimationAction::InsertPose
        | AnimationAction::MovePose
        | AnimationAction::RotatePose => author,
        AnimationAction::ToggleAutoKey => writing_ui,
        AnimationAction::ResetPose => writing_ui && view.pose_dirty,
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
    dock: super::docking::Context,
    mut nodes: ParamSet<(
        Query<
            (
                &mut Node,
                Option<&AnimationPanel>,
                Option<&AnimationPoseToolbar>,
                Option<&AnimationRow>,
                Option<&AnimationPlayhead>,
                Option<&AnimationNumericSlot>,
                Option<&AnimationRulerTick>,
                Option<&AssetPreviewSubAssetPanel>,
                Option<&AnimationWriteControls>,
                Has<AnimationDirectSourceControls>,
                Option<&PropertySection>,
            ),
            Or<(
                With<AnimationPanel>,
                With<AnimationPoseToolbar>,
                With<AnimationRow>,
                With<AnimationPlayhead>,
                With<AnimationNumericSlot>,
                With<AnimationRulerTick>,
                With<AssetPreviewSubAssetPanel>,
                With<AnimationWriteControls>,
                With<AnimationDirectSourceControls>,
                With<PropertySection>,
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
            Without<AnimationRulerTick>,
            Without<AssetPreviewSubAssetPanel>,
            Without<AnimationWriteControls>,
            Without<AnimationDirectSourceControls>,
            Without<PropertySection>,
            Without<AnimationKeyButton>,
        ),
    >,
    mut labels: Query<(&AnimationLabel, &mut Text)>,
    locale: Option<Res<EditorLocale>>,
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
    let locale = locale.as_deref().copied().unwrap_or_default();
    let mut capacities = [KEY_SLOTS; CHANNEL_SLOTS];
    let mut ruler_width = 0.0;
    for (track, node) in &tracks {
        if let Some(row) = track.row
            && node.size().x > 0.0
        {
            capacities[row] =
                ((node.size().x * node.inverse_scale_factor() / 14.0) as usize).clamp(1, KEY_SLOTS);
        } else if track.row.is_none() {
            ruler_width = node.size().x * node.inverse_scale_factor();
        }
    }
    let expanded_glb = dock.central_glb().is_some();
    let in_place = state.in_place_rig_preview();
    let editing = state.editing_animation();
    let writing_ui = editing && !dock.weights_own_viewport();
    let ready = view.visible && !view.busy && view.clip_count > 0;
    let clip_ui = view.visible && view.clip_count > 0;
    for (mut node, panel, toolbar, row, playhead, numeric, tick, subassets, write, direct, property)
        in &mut nodes.p0()
    {
        if panel.is_some() || toolbar.is_some() || write.is_some() || direct || property.is_some() {
            // 预览保留原片段和播放区，写按钮、数值框和姿势工具跟随显式编辑。
            let shown = if panel.is_some() {
                view.visible || editing
            } else if let Some(write) = write {
                writing_ui && (clip_ui || matches!(write, AnimationWriteControls::History))
            } else if toolbar.is_some() {
                writing_ui && clip_ui
            } else if direct {
                clip_ui && !in_place
            } else {
                clip_ui || (view.visible && in_place && property == Some(&PropertySection::Binding))
            };
            let display = if shown {
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
        } else if let Some(tick) = tick {
            // 末帧文字收在尺内，需要给前一个刻度留出两倍标签宽度。
            let show = !expanded_glb || ruler_width >= RULER_TICK_WIDTH * 8.0
                || tick.0 == 0 || tick.0 == 8
                || (ruler_width >= RULER_TICK_WIDTH * 4.0 && tick.0 == 4);
            let display = if show { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        } else if subassets.is_some()
            && !expanded_glb
            && view.visible
            && node.display != Display::None
        {
            // 中央 GLB 的子资产在左树，其他动画预览仍沿用原紧凑布局。
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
            let width = px(if key.clustered || stride > 1 {
                5.0
            } else {
                14.0
            });
            // 静止菱形无需重新同步到布局引擎，播放时只更新真实变动的控件。
            if node.width != width {
                node.width = width;
            }
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
        let enabled = !state.weight_rest_active()
            && ((in_place && marker.action == AnimationAction::UnbindModel)
                || enabled(marker.action, &state, &view, writing_ui));
        set_disabled(&mut commands, entity, enabled, disabled);
        let selected = (marker.action == AnimationAction::TogglePlayback && view.playing)
            || (marker.action == AnimationAction::ToggleLoop && view.looping)
            || (marker.action == AnimationAction::ToggleAutoKey && view.auto_key)
            || (marker.action == AnimationAction::ToggleEditing && (writing_ui || state.can_edit_selected()))
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
                    locale.format("animation.clip.current", &[
                        ("index", (view.clip_index + 1).to_string()),
                        ("count", view.clip_count.to_string()),
                        ("name", view.clip_name.clone()),
                    ])
                } else {
                    locale.text("label.animation-editor").into()
                }
            }
            AnimationLabel::ClipParameters => {
                if view.clip_count == 0 {
                    String::new()
                } else {
                    // 属性只回显当前片段及稳定的播放参数。逐帧播放头仍由原时间轴展示。
                    let key = if view.looping { "animation.loop.on" } else { "animation.loop.off" };
                    locale.format("animation.clip.parameters", &[
                        ("duration", format!("{:.2}", view.duration)),
                        ("fps", view.fps.to_string()),
                        ("speed", view.speed.to_string()),
                        ("loop", locale.text(key).into()),
                    ])
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
            AnimationLabel::Hint => "局部XYZ · 旋转单位为度 · 左键单击精确输入".into(),
            AnimationLabel::Status => {
                let state = if view.busy {
                    "正在处理"
                } else if !writing_ui || !view.editable {
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
            AnimationLabel::Button(action) => button_label(action, &view, in_place, editing, locale),
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
            && writing_ui
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_actions_stay_read_only_until_explicit_edit_and_weights_hide_animation_writes() {
        use crate::{data::tests::test_document, resources::Session};
        use feathrise_editor_animation_model::host::AnimationHost;

        let source = std::path::PathBuf::from("fixture.glb");
        let mut state = AnimationEditorState::default();
        state.active = Some(source.clone());
        state.sessions.insert(source, Session::new(test_document()));
        state.visible = true;
        let mut app = App::new();
        app.insert_resource(state)
            .init_resource::<AnimationHost>()
            .init_resource::<ActiveNumericInput>()
            .init_resource::<ActiveNumericInputScrub>()
            .add_message::<NumericInputSetValue>()
            .add_systems(Update, sync);
        let world = app.world_mut();
        let spawn_button = |world: &mut World, action| {
            world.spawn((
                AnimationButton { action, ..default() },
                UiInteraction::None,
                Node::default(),
                BackgroundColor::default(),
            )).id()
        };
        let launch = spawn_button(world, AnimationAction::ToggleEditing);
        let insert = spawn_button(world, AnimationAction::InsertKey);
        world.entity_mut(insert).insert(AnimationWriteControls::Clip);
        let playback = spawn_button(world, AnimationAction::TogglePlayback);
        let pose = world.spawn((AnimationPoseToolbar, Node::default())).id();
        let history = world.spawn((AnimationWriteControls::History, Node::default())).id();
        let shown = |app: &App, entity| app.world().get::<Node>(entity).unwrap().display != Display::None;
        let available = |app: &App, entity| app.world().get::<InteractionDisabled>(entity).is_none();

        app.update();
        assert!(available(&app, playback));
        assert!(!available(&app, launch));
        assert!(!available(&app, insert) && !shown(&app, insert));
        assert!(!shown(&app, pose) && !shown(&app, history));

        assert!(app.world_mut().resource_mut::<AnimationEditorState>().select_current_clip());
        app.update();
        assert!(available(&app, launch));
        assert!(!shown(&app, insert) && !shown(&app, pose));

        assert!(app.world_mut().resource_mut::<AnimationEditorState>().enter_animation_edit());
        app.update();
        assert!(available(&app, insert) && shown(&app, insert));
        assert!(shown(&app, pose) && shown(&app, history));

        app.world_mut().resource_mut::<AnimationHost>().weights_own_viewport = true;
        app.update();
        assert!(available(&app, playback));
        assert!(!available(&app, insert) && !shown(&app, insert));
        assert!(!shown(&app, pose) && !shown(&app, history));

        // 唯一片段撤销后，历史仍可见，骨架写控件随片段消失。
        app.world_mut().resource_mut::<AnimationHost>().weights_own_viewport = false;
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        std::sync::Arc::make_mut(&mut state.current_mut().unwrap().document).clips.clear();
        app.update();
        assert!(available(&app, launch) && shown(&app, history));
        assert!(!shown(&app, insert) && !shown(&app, pose));
    }

    #[test]
    fn key_marker_width_only_changes_when_timeline_density_changes() {
        let (mut app, _, _) = crate::editor_tests::app_context();
        app.init_resource::<ActiveNumericInput>()
            .init_resource::<ActiveNumericInputScrub>()
            .add_message::<NumericInputSetValue>()
            .add_systems(Update, sync);
        let track = app.world_mut().spawn((
            AnimationTrack { row: Some(0), ..default() },
            ComputedNode {
                size: Vec2::new(400.0, 24.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
        )).id();
        let marker = app.world_mut().spawn((
            AnimationKeyButton::default(),
            Node::default(),
            UiInteraction::None,
            BackgroundColor::default(),
        )).id();
        let node_width_and_tick = |app: &App| {
            let node = app.world().entity(marker).get_ref::<Node>().unwrap();
            (node.width, node.last_changed())
        };

        app.update();
        let wide = node_width_and_tick(&app);
        assert_eq!(wide.0, px(14.0));
        app.update();
        assert_eq!(node_width_and_tick(&app), wide);

        app.world_mut().get_mut::<ComputedNode>(track).unwrap().size.x = 14.0;
        app.update();
        let clustered = node_width_and_tick(&app);
        assert_eq!(clustered.0, px(5.0));
        assert_ne!(clustered.1, wide.1);
        app.update();
        assert_eq!(node_width_and_tick(&app), clustered);
    }
}
