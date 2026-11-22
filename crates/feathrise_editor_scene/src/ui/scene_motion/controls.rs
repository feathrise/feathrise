//! 场景动画使用公共时间轴，可选轨道插键。位置和旋转数值由对象属性栏编辑。

use bevy::{prelude::*, text::FontSourceTemplate, ui::{FocusPolicy, InteractionDisabled, RelativeCursorPosition}};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, InteractionButton, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputSetValue, NumericInputSpec, NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle,
    NumericValue, UiInteraction, animation_timeline as timeline, colors, metrics, spawn_numeric_scrub_field,
};
use feathrise_scene_content::{SceneId, ScenePlacementAssetTarget};
use crate::{
    authoring::{EditorAuthoringSession, scene_motion::{SceneMotionPlayback, SceneMotionTrack}},
    resources::SceneEditorState,
    ui::scene::FONT_PATH,
};
use super::{Channel, Context, FPS, SceneMotionButton, SceneMotionField, SelectedKey, Workbench, preview};

#[derive(Component, Clone, Copy)]
enum Panel { Entry, Workbench, Create, Track }
#[derive(Component, Clone, Copy)]
enum Caption { Subject, Play, Playback, AutoPlay, Pending, Tick(usize) }
#[derive(Component, Clone, Copy)]
struct Slot(SceneMotionField);
#[derive(Component, Clone, Copy)]
pub(super) struct Lane(pub(super) Option<Channel>);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct TimelineHitArea;
#[derive(Component, Clone, Copy, Default)]
struct Playhead;
#[derive(Component, Clone, Copy)]
struct Marker { channel: Channel, slot: usize }
#[derive(Component)]
pub(super) struct FieldTarget(pub(super) SceneId);

impl SceneMotionField {
    fn label(self) -> &'static str {
        use SceneMotionField::*;
        match self {
            Frame => "当前帧",
            EndFrame => "结束帧",
        }
    }

    fn value(self, state: &Workbench, duration: f32) -> f32 {
        let seconds = match self { Self::Frame => state.seconds, Self::EndFrame => f64::from(duration) };
        (seconds * FPS).round() as f32
    }
}

fn label(text: &str, tone: Color) -> impl Scene {
    bsn! {
        Text(text)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(metrics::FONT_CONTROL) }
        TextColor(tone) ~{FocusPolicy::Pass}
    }
}

fn button_shell(action: SceneMotionButton) -> impl Scene {
    bsn! {
        InteractionButton ~{action}
        Node { height: px(26), padding: UiRect::horizontal(px(6)), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(2)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

fn button(action: SceneMotionButton, text: &str) -> impl Scene {
    bsn! { @button_shell(action) Children [@label(text, colors::TEXT_PRIMARY)] }
}

fn changing_button(action: SceneMotionButton, text: &str, caption: Caption) -> impl Scene {
    bsn! { @button_shell(action)
        Children [~{caption} @label(text, colors::TEXT_PRIMARY)]
    }
}

pub(in crate::ui) fn entry() -> impl Scene {
    bsn! {
        ~{Panel::Entry}
        Node { display: Display::None, width: percent(100), flex_shrink: 0.0 }
        Children [@button(SceneMotionButton::Open, "编辑场景动画")]
    }
}

fn slot(field: SceneMotionField) -> impl Scene {
    bsn! { ~{Slot(field)}
        Node { flex_grow: 1.0, flex_basis: px(84), min_width: px(0), height: px(26) }
    }
}

fn row() -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), flex_wrap: FlexWrap::Wrap,
            column_gap: px(4), row_gap: px(3), align_items: AlignItems::Center }
    }
}

fn caption_cell(text: &str) -> impl Scene {
    bsn! {
        Node { width: px(152), min_width: px(92), max_width: percent(42), height: percent(100),
            flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::left(px(5)) }
        BackgroundColor(colors::PANEL_BG_STRONG)
        Children [@label(text, colors::TEXT_PRIMARY)]
    }
}

fn tick(index: usize) -> impl Scene {
    let node = timeline::tick_node(index);
    let caption = Caption::Tick(index);
    bsn! { ~{node} BorderColor::all(colors::COPPER_BORDER_WEAK) ~{FocusPolicy::Pass}
        Children [~{caption} @label("0", colors::TEXT_PRIMARY)]
    }
}

fn ruler() -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), height: px(24), flex_shrink: 0.0 }
        Children [
            @caption_cell("轨道 · Ctrl 多选")
        --
            ~{Lane(None)} ~{SceneMotionButton::Seek} @timeline::lane(true)
            Children [@tick(0)-- @tick(2)-- @tick(4)-- @tick(6)-- @tick(8)-- Playhead @timeline::playhead()]
        ]
    }
}

fn marker(channel: Channel, slot: usize) -> impl Scene {
    bsn! { ~{Marker { channel, slot }} @timeline::key_marker() }
}

// 沿用动作时间线的固定可见池，密集轨道仍可用帧尺定位实际键。
fn markers(channel: Channel, start: usize) -> Box<dyn Scene> {
    Box::new(bsn! { ~{FocusPolicy::Pass}
        Node { position_type: PositionType::Absolute, left: px(0), top: px(0),
            width: percent(100), height: percent(100) }
        Children [@marker(channel, start)-- @marker(channel, start+1)-- @marker(channel, start+2)--
            @marker(channel, start+3)-- @marker(channel, start+4)-- @marker(channel, start+5)--
            @marker(channel, start+6)-- @marker(channel, start+7)]
    })
}

fn channel(label: &str, channel: Channel) -> impl Scene {
    let select = match channel {
        Channel::Position => SceneMotionButton::SelectPosition,
        Channel::Rotation => SceneMotionButton::SelectRotation,
        Channel::Scale => SceneMotionButton::SelectScale,
    };
    bsn! {
        Node { width: percent(100), min_width: px(0), height: px(24), flex_shrink: 0.0,
            border: UiRect::bottom(px(1)) }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @caption_cell(label) InteractionButton ~{select}
        --
            ~{Lane(Some(channel))} @timeline::lane(false)
            Children [Playhead @timeline::playhead()--
                @markers(channel, 0)-- @markers(channel, 8)-- @markers(channel, 16)-- @markers(channel, 24)--
                @markers(channel, 32)-- @markers(channel, 40)-- @markers(channel, 48)-- @markers(channel, 56)]
        ]
    }
}

pub(in crate::ui) fn workbench() -> impl Scene {
    use SceneMotionButton::*;
    use SceneMotionField::*;
    bsn! {
        ~{Panel::Workbench}
        TimelineHitArea RelativeCursorPosition::default()
        Node { display: Display::None, flex_direction: FlexDirection::Column, flex_shrink: 0.0,
            width: percent(100), min_width: px(0), row_gap: px(3), padding: UiRect::all(px(4)),
            border: UiRect::top(px(1)) }
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Node { width: percent(100), min_width: px(0), justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center }
            Children [~{Caption::Subject} @label("场景动画", colors::TEXT_PRIMARY)-- @button(Close, "关闭动画")]
        --
            ~{Panel::Create} @row()
            Children [@button(Add, "新建动画")]
        --
            ~{Panel::Track}
            Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3) }
            Children [
                @row() Children [
                    @button(First, "首帧")-- @button(PreviousFrame, "前帧")--
                    @changing_button(TogglePlay, "播放", Caption::Play)-- @button(NextFrame, "后帧")-- @button(Last, "末帧")--
                    @changing_button(CyclePlayback, "单次", Caption::Playback)--
                    @changing_button(ToggleAutoPlay, "手动播放", Caption::AutoPlay)-- @slot(Frame)-- @slot(EndFrame)
                ]
            --
                @ruler()-- @channel("位置", Channel::Position)-- @channel("旋转", Channel::Rotation)-- @channel("缩放", Channel::Scale)
            --
                @row() Children [
                    @button(InsertKey, "插入关键帧 (I)")-- @button(DeleteKey, "删除关键帧")--
                    @button(ResetPose, "还原当前帧")-- ~{Caption::Pending} @label("", colors::TEXT_ACCENT)
                ]
            ]
        ]
    }
}

pub(super) fn install(app: &mut App) {
    app.add_systems(Update,
        (spawn_fields, preview::apply, sync).chain()
            .after(crate::viewport::placement_move::handle_placement_move)
            .in_set(crate::SceneEditorSystemSet::Presentation));
}

fn spawn_fields(mut commands: Commands, assets: Res<AssetServer>, slots: Query<(Entity, &Slot), Added<Slot>>) {
    for (slot, &Slot(field)) in &slots {
        let minimum = if field == SceneMotionField::EndFrame { 1.0 } else { 0.0 };
        let spec = NumericInputSpec::f32(0).with_step(NumericValue::F32(1.0)).with_step_buttons(false)
            .with_live_update(false).with_replace_on_first_key(true).with_min(NumericValue::F32(minimum));
        let options = NumericScrubFieldOptions::new(
            field.label(), "帧", NumericValue::F32(0.0), spec,
            NumericInputScrubConfig::linear(4.0, 1.0, minimum, f32::MAX / 2.0, 1.0),
        ).with_style(NumericScrubFieldStyle {
            width: 120.0, height: 26.0, border_radius: 2.0, column_gap: 3.0,
            label_font_size: metrics::FONT_CONTROL, value_font_size: metrics::FONT_CONTROL, unit_font_size: 9.0,
        });
        let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
        commands.entity(slot).with_children(|slot| {
            let root = spawn_numeric_scrub_field(slot, field, font, options);
            slot.commands().entity(root).insert(NumericInputDisabled)
                .remove::<feathrise_editor_ui::NumericInputScrubSettingsKey>()
                .entry::<Node>().and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                });
        });
    }
}

fn sync(
    mut commands: Commands, editor: Res<SceneEditorState>, session: Res<EditorAuthoringSession>,
    context: Context, state: Res<Workbench>,
    mut panels: Query<(&mut Node, &Panel), (Without<Playhead>, Without<Marker>)>,
    mut playheads: Query<&mut Node, (With<Playhead>, Without<Panel>, Without<Marker>)>,
    mut markers: Query<(&mut Node, &Marker, &Children), (Without<Playhead>, Without<Panel>)>,
    mut marker_colors: Query<&mut TextColor>,
    mut captions: Query<(&mut Text, &Caption)>,
    mut buttons: Query<(Entity,&SceneMotionButton,Has<InteractionDisabled>,&UiInteraction,&mut BackgroundColor), With<InteractionButton>>,
    fields: Query<(Entity, &SceneMotionField, &NumericInputValue, Has<NumericInputDisabled>, Option<&FieldTarget>)>,
    active: Res<ActiveNumericInput>, scrub: Res<ActiveNumericInputScrub>,
    mut values: MessageWriter<NumericInputSetValue>,
) {
    let scene = context.scene(&editor, &session);
    let selected = context.record(&editor, &session);
    let show_entry = selected.is_some() || scene.is_some_and(|scene| context.browser.is_main_glb_selected(scene));
    let opened = state.scene.as_ref().zip(scene).is_some_and(|(a, b)| a == b);
    let motion = session.draft.as_ref().map(|draft| &draft.motion);
    let created = opened && motion.is_some_and(|motion| motion.clip_name().is_some());
    let track = selected.and_then(|(_, record)| motion?.get(&record.id));
    let timing = motion.map(|motion| motion.timing()).unwrap_or_default();
    let duration = timing.duration_seconds;
    let editing = opened && context.can_edit(&editor, &session);
    let can_pose = editing && selected.is_some_and(|(scene, record)| state.can_pose(scene, &record.id));
    let can_add = editing && motion.is_some_and(|motion| motion.available()) && !created;
    let dirty_pose = state.draft.position.is_some() || state.draft.rotation.is_some() || state.draft.scale.is_some();
    let valid_key = track.zip(state.selected_key).is_some_and(|(track, selected)| {
        selected.channel.keys(track).binary_search_by(|key| key.time.total_cmp(&selected.time)).is_ok()
    });

    for (mut node, panel) in &mut panels {
        let visible = match panel {
            Panel::Entry => show_entry,
            Panel::Workbench => opened,
            Panel::Create => opened && !created,
            Panel::Track => created,
        };
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    use SceneMotionButton::*;
    for (entity, button, disabled, interaction, mut background) in &mut buttons {
        let enabled = match button {
            Open => !opened && show_entry && context.can_edit(&editor, &session),
            Close => editing,
            Add => can_add,
            InsertKey => can_pose && state.has_keying_tracks(),
            DeleteKey => can_pose && valid_key,
            ResetPose => can_pose && dirty_pose,
            _ => editing && created,
        };
        if enabled == disabled {
            if enabled { commands.entity(entity).remove::<InteractionDisabled>(); }
            else { commands.entity(entity).insert(InteractionDisabled); }
        }
        let selected = match button {
            SelectPosition => state.keys_channel(Channel::Position),
            SelectRotation => state.keys_channel(Channel::Rotation),
            SelectScale => state.keys_channel(Channel::Scale),
            ToggleAutoPlay => timing.auto_play,
            _ => false,
        };
        let color = if !enabled { colors::SLOT_BG_DISABLED } else if *interaction == UiInteraction::Pressed {
            colors::SELECT_BG_PRESSED
        } else if *interaction == UiInteraction::Hovered { colors::SELECT_BG_HOVER }
        else if selected { colors::SELECT_BG }
        else { colors::SLOT_BG };
        if background.0 != color { background.0 = color; }
    }
    for (entity, field, current, disabled, binding) in &fields {
        let enabled = editing && created;
        if enabled == disabled {
            if enabled { commands.entity(entity).remove::<NumericInputDisabled>(); }
            else { commands.entity(entity).insert(NumericInputDisabled); }
        }
        let bound = enabled.then_some(scene).flatten();
        let rebound = binding.map(|binding| &binding.0) != bound;
        if rebound {
            if let Some(scene) = bound {
                commands.entity(entity).insert(FieldTarget(scene.clone()));
            } else {
                commands.entity(entity).remove::<FieldTarget>();
            }
        } else if active.entity == Some(entity) || scrub.entity == Some(entity) { continue; }
        let value = field.value(&state, duration);
        if rebound || current.0.as_f32() != Some(value) {
            values.write(NumericInputSetValue { entity, value: NumericValue::F32(value), cancel_edit: true });
        }
    }
    let clip_name = motion.and_then(|motion| motion.clip_name()).unwrap_or("场景动画");
    for (mut text, caption) in &mut captions {
        let value = match caption {
            Caption::Subject => selected.map_or_else(|| format!("{clip_name} · 请选择对象"), |(_, record)| {
                let name = if !record.editor.name.is_empty() { &record.editor.name }
                    else if matches!(record.asset_target, ScenePlacementAssetTarget::Cuboid { .. }) { "立方体" }
                    else if record.asset_target.is_builtin() { "网格对象" }
                    else { record.asset_path.rsplit('/').next().unwrap_or(&record.id) };
                format!("{clip_name} · {name}")
            }),
            Caption::Play => if state.playing { "暂停" } else { "播放" }.into(),
            Caption::Playback => match timing.playback {
                SceneMotionPlayback::Once => "单次",
                SceneMotionPlayback::Loop => "循环",
                SceneMotionPlayback::PingPong => "往返",
            }.into(),
            Caption::AutoPlay => if timing.auto_play { "自动播放" } else { "手动播放" }.into(),
            Caption::Pending if state.draft.scale.is_some() => "变换待插帧".into(),
            Caption::Pending => match (state.draft.position.is_some(), state.draft.rotation.is_some()) {
                (true, true) => "位置和旋转待插帧",
                (true, false) => "位置待插帧",
                (false, true) => "旋转待插帧",
                (false, false) => "",
            }.into(),
            Caption::Tick(index) => format!("{:.0}", f64::from(duration) * FPS * *index as f64 / 8.0),
        };
        if text.0 != value { text.0 = value; }
    }
    let left = percent((100.0 * state.seconds / f64::from(duration)).clamp(0.0, 100.0) as f32);
    for mut playhead in &mut playheads { if playhead.left != left { playhead.left = left; } }
    for (mut node, marker, children) in &mut markers {
        let key = track.and_then(|track| displayed_key(track, marker));
        let display = if key.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if let Some(key) = key {
            let left = percent((100.0 * key.time / duration).clamp(0.0, 100.0));
            if node.left != left { node.left = left; }
            let selected = state.selected_key == Some(SelectedKey { channel: marker.channel, time: key.time });
            if let Some(mut color) = children.first().and_then(|entity| marker_colors.get_mut(*entity).ok()) {
                let tone = if selected { colors::TEXT_ACCENT } else { colors::TEXT_VALUE };
                if color.0 != tone { color.0 = tone; }
            }
        }
    }
}

fn displayed_key<'a>(track: &'a SceneMotionTrack, marker: &Marker) -> Option<&'a crate::authoring::scene_motion::SceneMotionKey> {
    let keys = marker.channel.keys(track);
    let index = if keys.len() > timeline::KEY_SLOTS { marker.slot * (keys.len() - 1) / (timeline::KEY_SLOTS - 1) }
        else { marker.slot };
    keys.get(index)
}
