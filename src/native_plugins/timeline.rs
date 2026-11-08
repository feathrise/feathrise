//! 通用原生时间轴：插件拥有条块语义，宿主把选择、拖动和缩放作为有界事务回传。
use feathrise_editor_ui::InteractionButton;
use super::{
    components::{ToolButton, ToolNumber, UiAction},
    resources::NativeTools,
    ui::{button, caption, row},
};
use bevy::{input::mouse::AccumulatedMouseMotion, prelude::*, ui::FocusPolicy};
use feathrise_editor_core::PresetWorkspaceTab;
use feathrise_editor_map::game_authoring::map_api::ui::vegetation_preset_editor::VegetationPresetEditorState;
use feathrise_editor_ui::{ButtonColors, EditorLiteralColor, colors as c};
use feathrise_plugin_api::{Request, Snapshot, Timeline, TimelineBlock};

const TRACK_LABEL_WIDTH: f32 = 144.;
const TRACK_HEIGHT: f32 = 38.;
const BLOCK_HEIGHT: f32 = 24.;

#[derive(Component, Clone)]
pub(super) struct Block {
    index: usize,
    document: u32,
    lane: String,
    id: String,
    seconds_per_pixel: f64,
    canvas: Entity,
    edge: i8,
    point: bool,
}
#[derive(Component)]
pub(super) struct Seek {
    index: usize,
    document: u32,
    pan: f64,
    span: f64,
}
struct Drag {
    index: usize,
    document: u32,
    lane: String,
    id: String,
    delta: f64,
    scale: f64,
    edge: i8,
    origin_x: Option<f32>,
    moved: bool,
}

#[derive(Component)]
pub(super) struct TrackLayout {
    index: usize,
    document: u32,
    lane: String,
    row: Entity,
    geometry: Option<(u32, u32, u64)>,
}
#[derive(Component)]
pub(super) struct LaneItem { canvas: Entity, block: String, offset: f32 }
#[derive(Component)]
pub(super) struct BlockLabel { index: usize, document: u32, block: String }
#[derive(Component)]
pub(super) struct Readout { index: usize, document: u32 }
#[derive(Component)]
pub(super) struct Playhead { index: usize, document: u32, pan: f64, span: f64 }
#[derive(Component)]
struct Ruler;
#[derive(Component)]
pub(super) struct TimelineCanvasRoot;
#[derive(Resource, Default)]
pub(super) struct Gestures {
    drag: Option<Drag>,
}
impl Gestures {
    pub(super) fn active(&self) -> bool { self.drag.is_some() }
}

#[derive(Default)]
pub(super) struct Playback {
    owner: Option<(usize, u32)>,
    revision: u64,
    cursor: f64,
    elapsed: f64,
}

/// 时间轴只是作者游标；10Hz Seek不修改草稿内容身份，因此不会反复重启真实技能预览。
pub(super) fn tick(
    mut state: ResMut<NativeTools>,
    workspace: Res<VegetationPresetEditorState>,
    gestures: Res<Gestures>,
    time: Res<Time<Real>>,
    guard: Res<feathrise_editor_core::EditorTextInputGuard>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut clock: Local<Playback>,
) {
    let current = if windows.single().is_ok_and(|window| window.focused)
        && !guard.block_shortcuts
        && !state.input_pending
        && gestures.drag.is_none()
    {
        state.tools.iter().find_map(|(&index, tool)| {
            let axis = tool.snapshot.timeline.as_ref()?;
            (axis.playing
                && workspace.active_tab
                    == PresetWorkspaceTab::Domain(tool.domain, tool.snapshot.active.into()))
            .then_some((
                index,
                tool.snapshot.active,
                tool.snapshot.revision,
                axis.cursor,
                axis.duration,
                tool.busy(),
            ))
        })
    } else {
        None
    };
    let Some((index, document, revision, cursor, duration, busy)) = current else {
        *clock = Playback::default();
        return;
    };
    if clock.owner != Some((index, document)) {
        *clock = Playback {
            owner: Some((index, document)),
            revision,
            cursor,
            elapsed: 0.,
        };
    } else if clock.revision != revision {
        clock.revision = revision;
        clock.cursor = cursor;
    }
    clock.elapsed += time.delta_secs_f64().min(0.25);
    if busy || clock.elapsed < 0.1 {
        return;
    }
    let next = (clock.cursor + clock.elapsed).min(duration);
    let Some(tool) = state.tools.get_mut(&index) else {
        return;
    };
    if tool
        .send(
            Request::Action {
                document,
                action: "timeline_seek".into(),
                value: serde_json::json!({"cursor":next,"revision":revision,"playback":true})
                    .to_string(),
            },
            false,
        )
        .is_ok()
    {
        clock.cursor = next;
        clock.elapsed = 0.;
        if next >= duration {
            let _ = tool.send(
                Request::Action {
                    document,
                    action: "timeline_play".into(),
                    value: serde_json::json!({"playing":false}).to_string(),
                },
                false,
            );
        }
    }
}

fn action(document: u32, name: &str, value: serde_json::Value) -> UiAction {
    UiAction::Request(Request::Action {
        document,
        action: name.into(),
        value: value.to_string(),
    })
}
pub(super) fn render(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    index: usize,
    s: &Snapshot,
) {
    let Some(axis) = &s.timeline else { return; };
    let span = axis.span.unwrap_or(axis.duration / axis.zoom);
    row(parent, |tools| {
        button(tools, font, if axis.playing { "暂停" } else { "播放" }, index,
            action(s.active, "timeline_play", serde_json::json!({"playing":!axis.playing})), axis.playing);
        for (label, zoom) in [("缩小", axis.zoom / 1.25), ("放大", axis.zoom * 1.25)] {
            button(tools, font, label, index, action(s.active, "timeline_zoom",
                serde_json::json!({"zoom":zoom.clamp(0.1,100.)})), false);
        }
        for (label, pan) in [("较早", axis.pan - span * 0.25), ("较晚", axis.pan + span * 0.25)] {
            button(tools, font, label, index, action(s.active, "timeline_pan",
                serde_json::json!({"pan":pan.clamp(0.,axis.duration)})), false);
        }
        button(tools, font, "全轴", index,
            action(s.active, "timeline_fit", serde_json::json!({})), false);
        tools.spawn((caption(font, &format!("游标 {:.2}s / {:.2}s · ×{:.2}", axis.cursor,
            axis.duration, axis.zoom), 10.5), TextLayout::no_wrap()));
    });
    parent.spawn((Readout { index, document: s.active }, EditorLiteralColor,
        tinted_caption(font, &readout(axis, false), 10., c::TEXT_ACCENT)));
    // 原版目录、刻度和轨道共用同一144px分界；只用内部单边分隔，不另套闭合外框。
    parent.spawn((TimelineCanvasRoot, Node {
        width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
        flex_shrink: 0., ..default()
    }, BackgroundColor(c::PANEL_BG_STRONG))).with_children(|root| {
        root.spawn((Ruler, Node {
            width: percent(100), min_width: px(0), height: px(28), min_height: px(28),
            flex_shrink: 0., ..default()
        })).with_children(|ruler| {
            ruler.spawn((tinted_caption(font, &format!("轨道 · ×{:.2}", axis.zoom), 10., c::TEXT_MUTED),
                TextLayout::no_wrap(), Node {
                    width: px(TRACK_LABEL_WIDTH), min_width: px(TRACK_LABEL_WIDTH), flex_shrink: 0.,
                    align_items: AlignItems::Center, padding: UiRect::horizontal(px(8)), ..default()
                }));
            ruler.spawn((Seek { index, document: s.active, pan: axis.pan, span },
                Interaction::None, bevy::ui::RelativeCursorPosition::default(),
                canvas_node(28.), FocusPolicy::Block)).with_children(|ticks| {
                for tick in 0..=4 {
                    ticks.spawn((tinted_caption(font, &format!("{:.2}s", tick_seconds(axis.pan, span, tick)), 9.5, c::TEXT_MUTED),
                        TextLayout::no_wrap(), Node {
                            position_type: PositionType::Absolute,
                            left: if tick == 4 { Val::Auto } else { percent(tick as f32 * 25.) },
                            right: if tick == 4 { px(4) } else { Val::Auto }, top: px(4), ..default()
                        }));
                }
                spawn_playhead(ticks, index, s.active, axis, span);
            });
        });
        for lane in &axis.lanes {
            let color = lane.presentation.color.map_or(c::TEXT_MUTED, rgba);
            let mut row = root.spawn((Node {
                width: percent(100), min_width: px(0), height: px(TRACK_HEIGHT),
                min_height: px(TRACK_HEIGHT), flex_shrink: 0., border: UiRect::bottom(px(1)),
                ..default()
            }, BorderColor::all(c::COPPER_BORDER_WEAK),
                BackgroundColor(if lane.presentation.selected { c::SELECT_BG.with_alpha(0.35) }
                    else { c::PANEL_BG_STRONG })));
            let row_entity = row.id();
            row.with_children(|line| {
                line.spawn((Node {
                    width: px(TRACK_LABEL_WIDTH), min_width: px(TRACK_LABEL_WIDTH), flex_shrink: 0.,
                    align_items: AlignItems::Center, border: UiRect::left(px(3)),
                    padding: UiRect::horizontal(px(3)), ..default()
                }, BorderColor::all(color), EditorLiteralColor)).with_children(|label| {
                    if lane.presentation.select {
                        lane_button(label, font, &lane.label, index,
                            action(s.active, "timeline_select", serde_json::json!({"lane":lane.id})),
                            lane.presentation.selected, true);
                    } else {
                        label.spawn((caption(font, &lane.label, 10.), TextLayout::no_wrap(),
                            Node { min_width: px(0), overflow: Overflow::clip(), ..default() }));
                    }
                    if lane.presentation.add {
                        lane_button(label, font, "+", index,
                            action(s.active, "timeline_add_block", serde_json::json!({"lane":lane.id})),
                            false, false);
                    }
                });
                let mut canvas = line.spawn((Seek { index, document: s.active, pan: axis.pan, span },
                    Interaction::None, bevy::ui::RelativeCursorPosition::default(),
                    TrackLayout { index, document: s.active, lane: lane.id.clone(),
                        row: row_entity, geometry: None },
                    canvas_node(TRACK_HEIGHT - 1.), FocusPolicy::Block));
                let canvas_entity = canvas.id();
                canvas.with_children(|track| {
                    for tick in 1..4 {
                        track.spawn((Node { position_type: PositionType::Absolute,
                            left: percent(tick as f32 * 25.), width: px(1), height: percent(100),
                            ..default() }, BackgroundColor(c::COPPER_BORDER_WEAK),
                            FocusPolicy::Pass, Pickable::IGNORE));
                    }
                    for block in &lane.blocks {
                        let Some((left, right)) = visible_interval(block, axis.pan, span) else { continue; };
                        let reference = reference_id(block);
                        if let Some(reference) = reference {
                            // 只读锚点仅保留上下2px细带，主条块的分层位置是唯一纵向身份。
                            let offset = if block.presentation.reference_band == 0 { 2. } else { TRACK_HEIGHT - 5. };
                            track.spawn((LaneItem { canvas: canvas_entity, block: reference.into(), offset },
                                Node { position_type: PositionType::Absolute, left: percent(left),
                                    width: percent(right - left), min_width: px(2), top: px(offset),
                                    height: px(2), ..default() },
                                BackgroundColor(c::TEXT_MUTED.with_alpha(0.42)), FocusPolicy::Pass, Pickable::IGNORE));
                            continue;
                        }
                        let fill = block.presentation.color.or(lane.presentation.color)
                            .map_or(c::BUTTON_DEV_SMALL_NORMAL, rgba);
                        let point = block.presentation.point;
                        let widget = Block {
                            index, document: s.active, lane: lane.id.clone(), id: block.id.clone(),
                            seconds_per_pixel: span,
                            canvas: canvas_entity, edge: 0, point,
                        };
                        let mut entity = track.spawn((EditorLiteralColor, LaneItem { canvas: canvas_entity,
                            block: block.id.clone(), offset: (TRACK_HEIGHT - BLOCK_HEIGHT) * 0.5 },
                            Node { position_type: PositionType::Absolute, left: percent(left),
                                width: if point { px(6) } else { percent(right - left) }, min_width: px(6),
                                margin: UiRect::left(px(if point { -3. } else { 0. })),
                                height: px(BLOCK_HEIGHT), top: px((TRACK_HEIGHT - BLOCK_HEIGHT) * 0.5),
                                align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                                border: UiRect::bottom(px(if block.selected { 2 } else { 0 })),
                                border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip(), ..default()
                            }, BorderColor::all(c::TEXT_PRIMARY), BackgroundColor(fill)));
                        if !block.presentation.read_only {
                            entity.insert((InteractionButton, bevy::ui::RelativeCursorPosition::default(),
                                widget.clone(), palette(fill), FocusPolicy::Block));
                        } else { entity.insert((FocusPolicy::Pass, Pickable::IGNORE)); }
                        entity.with_children(|bar| {
                            if !point {
                                bar.spawn((BlockLabel { index, document: s.active, block: block.id.clone() },
                                    caption(font, &block.label, 10.), TextLayout::no_wrap()));
                                if !block.presentation.read_only {
                                    if block.presentation.resize_start { spawn_handle(bar, &widget, true); }
                                    if block.presentation.resize_end && !block.presentation.open_end {
                                        spawn_handle(bar, &widget, false);
                                    }
                                }
                            }
                        });
                    }
                    spawn_playhead(track, index, s.active, axis, span);
                });
            });
        }
    });
}

fn canvas_node(height: f32) -> Node {
    Node { position_type: PositionType::Relative, width: px(0), min_width: px(0), flex_grow: 1.,
        flex_shrink: 1., height: px(height), overflow: Overflow::clip(), ..default() }
}
fn rgba([r,g,b,a]: [f32; 4]) -> Color { Color::srgba(r,g,b,a) }
fn tinted_caption(font: &Handle<Font>, label: &str, size: f32, color: Color) -> impl Bundle {
    (Text::new(label), TextFont { font: font.clone().into(),
        font_size: feathrise_editor_ui::metrics::font_size(size).into(), ..default() },
        TextColor(color), Pickable::IGNORE)
}
fn palette(color: Color) -> ButtonColors {
    let base = color.to_srgba();
    let lift = |value: f32| Color::srgba((base.red + value).min(1.), (base.green + value).min(1.),
        (base.blue + value).min(1.), base.alpha);
    ButtonColors::new(color, lift(0.08), lift(0.16))
}
fn lane_button(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, label: &str,
    index: usize, action: UiAction, selected: bool, grow: bool) {
    let fill = if selected { c::SELECT_BG } else { Color::NONE };
    parent.spawn((InteractionButton, ToolButton { index, action }, Node {
        min_width: px(0), height: px(30), flex_grow: if grow { 1. } else { 0. },
        padding: UiRect::horizontal(px(4)), align_items: AlignItems::Center,
        border: UiRect::bottom(px(if selected { 2 } else { 0 })), overflow: Overflow::clip(),
        ..default()
    }, BorderColor::all(c::TEXT_ACCENT), BackgroundColor(fill),
        ButtonColors::new(fill, c::BUTTON_DEV_SMALL_HOVERED, c::BUTTON_DEV_SMALL_PRESSED)))
        .with_child((caption(font, label, 10.), TextLayout::no_wrap()));
}
fn spawn_handle(parent: &mut ChildSpawnerCommands, widget: &Block, left: bool) {
    let mut widget = widget.clone();
    widget.edge = if left { -1 } else { 1 };
    let fill = c::TEXT_ACCENT.with_alpha(0.72);
    parent.spawn((InteractionButton, widget, EditorLiteralColor, bevy::ui::RelativeCursorPosition::default(), Node {
        position_type: PositionType::Absolute, left: if left { px(0) } else { Val::Auto },
        right: if left { Val::Auto } else { px(0) }, width: px(6), height: percent(100), ..default()
    }, BackgroundColor(fill), palette(fill), FocusPolicy::Block));
}
fn spawn_playhead(parent: &mut ChildSpawnerCommands, index: usize, document: u32,
    axis: &Timeline, span: f64) {
    parent.spawn((Playhead { index, document, pan: axis.pan, span }, EditorLiteralColor, Node {
        position_type: PositionType::Absolute, left: percent(axis_percent(axis.cursor, axis.pan, span)),
        top: px(0), width: px(2), height: percent(100), ..default()
    }, BackgroundColor(c::TEXT_ACCENT), if (axis.pan..=axis.pan + span).contains(&axis.cursor) {
        Visibility::Inherited
    } else { Visibility::Hidden }, FocusPolicy::Pass, Pickable::IGNORE));
}
fn visible_interval(block: &TimelineBlock, pan: f64, span: f64) -> Option<(f32,f32)> {
    if block.end < pan || block.start > pan + span { return None; }
    Some((axis_percent(block.start,pan,span),axis_percent(block.end,pan,span)))
}
fn axis_percent(time: f64, pan: f64, span: f64) -> f32 {
    // 极短但合法的窗口可令轴外游标比值成为∞；先钳制再转f32，隐藏节点也不能带入无穷布局值。
    (((time-pan)/span).clamp(0.,1.)*100.) as f32
}
fn tick_seconds(pan: f64, span: f64, tick: usize) -> f64 {
    pan + span * (tick as f64 / 4.)
}
fn reference_id(block: &TimelineBlock) -> Option<&str> {
    block.presentation.reference_of.as_deref().or_else(|| {
        // 兼容已安装的早期作者包；新描述通过显式只读关联，宿主不再依赖此命名约定。
        block.id.strip_prefix("ghost/").and_then(|rest| rest.split_once('/').map(|(_, id)| id))
    })
}
fn readout(axis: &Timeline, dragging: bool) -> String {
    let Some((lane, block)) = axis.lanes.iter().find_map(|lane|
        lane.blocks.iter().find(|block| block.selected).map(|block| (lane,block))) else {
        return "单击刻度或空白定位游标 · + 在游标添加 · 拖动条块或端点 · Shift 自由拖动 / Esc 取消".into();
    };
    let timing = if block.presentation.point { format!("时刻 {:.2}s", block.start) }
        else { format!("起止 {} · 时长 {}s", block.label, if block.presentation.open_end {
            "∞".into() } else { format!("{:.2}", block.end - block.start) }) };
    format!("{} · {} #{} · {timing} · Shift 自由拖动 / Esc 取消",
        if dragging { "拖动中" } else { "已选择" }, lane.label, block.id)
}

/// 只刷新当前可见轴；拖动中保留原命中实体、横轴和分层，数据仍以插件已采用快照为准。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn sync(
    state: Res<NativeTools>,
    gestures: Res<Gestures>,
    mut layouts: Query<(Entity, &mut TrackLayout, &ComputedNode)>,
    blocks: Query<(Entity, &Block)>,
    mut nodes: Query<(Entity, &mut Node, Option<&LaneItem>), Without<Playhead>>,
    mut playheads: Query<(&Playhead, &mut Node, &mut Visibility)>,
    mut labels: Query<(&mut Text, Option<&BlockLabel>, Option<&Readout>),
        Or<(With<BlockLabel>, With<Readout>)>>,
    inputs: Query<(Entity, &ToolNumber, &feathrise_editor_ui::NumericInputValue,
        &feathrise_editor_ui::NumericInputConfig)>,
    mut numeric_values: MessageWriter<feathrise_editor_ui::NumericInputSetValue>,
) {
    for (entity, mut layout, computed) in &mut layouts {
        let Some(axis) = state.tools.get(&layout.index)
            .filter(|tool| tool.snapshot.active == layout.document)
            .and_then(|tool| tool.snapshot.timeline.as_ref()) else { continue; };
        let Some(lane) = axis.lanes.iter().find(|lane| lane.id == layout.lane) else { continue; };
        let width = computed.size().x * computed.inverse_scale_factor();
        if !width.is_finite() || width <= 0. || gestures.drag.as_ref()
            .is_some_and(|drag| drag.index == layout.index && drag.document == layout.document) {
            continue;
        }
        let span = axis.span.unwrap_or(axis.duration / axis.zoom);
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        axis.pan.to_bits().hash(&mut hash);
        span.to_bits().hash(&mut hash);
        for block in lane.blocks.iter().filter(|block| !block.presentation.read_only && reference_id(block).is_none()) {
            block.id.hash(&mut hash);
            block.start.to_bits().hash(&mut hash);
            block.end.to_bits().hash(&mut hash);
            block.presentation.point.hash(&mut hash);
        }
        let geometry = (computed.size().x.to_bits(), computed.inverse_scale_factor().to_bits(), hash.finish());
        if layout.geometry == Some(geometry) { continue; }
        let spans = lane.blocks.iter().filter(|block| !block.presentation.read_only && reference_id(block).is_none())
            .filter_map(|block| {
                let (left, right) = visible_interval(block, axis.pan, span)?;
                let minimum = 6. / width * 100.;
                let point = block.presentation.point;
                let margin = if point { -minimum * 0.5 } else { 0. };
                let narrow = point || right - left < minimum;
                Some((block.id.clone(), (left + margin).max(0.),
                    left + margin + (right - left).max(minimum) + if narrow { 2. / width * 100. } else { 0. }))
            }).collect();
        let (lanes, count) = pack_lanes(spans);
        for (_, mut node, item) in &mut nodes {
            if let Some(item) = item && item.canvas == entity && let Some(lane) = lanes.get(&item.block) {
                node.top = px(*lane as f32 * TRACK_HEIGHT + item.offset);
            }
        }
        let height = count.max(1) as f32 * TRACK_HEIGHT;
        if let Ok((_, mut node, _)) = nodes.get_mut(entity) { node.height = px(height - 1.); }
        if let Ok((_, mut node, _)) = nodes.get_mut(layout.row) {
            node.height = px(height);
            node.min_height = px(height);
        }
        layout.geometry = Some(geometry);
    }
    if !state.is_changed() { return; }
    for (entity, widget) in &blocks {
        if widget.edge != 0 { continue; }
        let Some(axis) = state.tools.get(&widget.index)
            .filter(|tool| tool.snapshot.active == widget.document)
            .and_then(|tool| tool.snapshot.timeline.as_ref()) else { continue; };
        let Some(block) = axis.lanes.iter().find(|lane| lane.id == widget.lane)
            .and_then(|lane| lane.blocks.iter().find(|block| block.id == widget.id)) else { continue; };
        let Ok((_, mut node, _)) = nodes.get_mut(entity) else { continue; };
        let span = axis.span.unwrap_or(axis.duration / axis.zoom);
        if let Some((left, right)) = visible_interval(block, axis.pan, span) {
            node.display = Display::Flex;
            node.left = percent(left);
            node.width = if widget.point { px(6) } else { percent(right - left) };
            node.border = UiRect::bottom(px(if block.selected { 2 } else { 0 }));
        } else { node.display = Display::None; }
    }
    for (marker, mut node, mut visibility) in &mut playheads {
        let Some(axis) = state.tools.get(&marker.index)
            .filter(|tool| tool.snapshot.active == marker.document)
            .and_then(|tool| tool.snapshot.timeline.as_ref()) else { continue; };
        node.left = percent(axis_percent(axis.cursor,marker.pan,marker.span));
        *visibility = if (marker.pan..=marker.pan + marker.span).contains(&axis.cursor) {
            Visibility::Inherited
        } else { Visibility::Hidden };
    }
    for (mut text, block, readout_marker) in &mut labels {
        let (index, document) = if let Some(marker) = block { (marker.index, marker.document) }
            else if let Some(marker) = readout_marker { (marker.index, marker.document) } else { continue; };
        let Some(axis) = state.tools.get(&index).filter(|tool| tool.snapshot.active == document)
            .and_then(|tool| tool.snapshot.timeline.as_ref()) else { continue; };
        let value = if let Some(marker) = block {
            axis.lanes.iter().flat_map(|lane| &lane.blocks).find(|block| block.id == marker.block)
                .map(|block| block.label.clone())
        } else { Some(readout(axis, gestures.drag.as_ref()
            .is_some_and(|drag| drag.index == index && drag.document == document))) };
        if let Some(value) = value && text.0 != value { text.0 = value; }
    }
    if let Some(drag) = &gestures.drag && let Some(tool) = state.tools.get(&drag.index) {
        for (entity, input, current, config) in &inputs {
            if input.index != drag.index || input.document != drag.document { continue; }
            let Some(value) = tool.snapshot.fields.iter().find(|field| field.id == input.field)
                .and_then(|field| field.value.parse::<f32>().ok()).filter(|value|value.is_finite()) else { continue; };
            let value = feathrise_editor_ui::normalize_numeric_value(
                feathrise_editor_ui::NumericValue::F32(value), config.0);
            if current.0 != value {
                // 外部回显走原数值桥，不伪造Committed，也不为同次拖动追加历史。
                numeric_values.write(feathrise_editor_ui::NumericInputSetValue { entity, value, cancel_edit: true });
            }
        }
    }
}

/// 仅在真实布局或可见条块变化时O(n log n)分层，普通相接区间复用一层。
fn pack_lanes(mut spans: Vec<(String, f32, f32)>) -> (std::collections::HashMap<String, usize>, usize) {
    use std::{cmp::Reverse, collections::{BinaryHeap, HashMap}};
    spans.sort_by(|left, right| left.1.total_cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    let mut occupied: BinaryHeap<Reverse<(u32, usize)>> = BinaryHeap::new();
    let mut assignments = HashMap::with_capacity(spans.len());
    let mut count = 0;
    for (id, start, end) in spans {
        let start = start.max(0.).to_bits();
        let lane = if occupied.peek().is_some_and(|Reverse((end, _))| *end <= start) {
            occupied.pop().unwrap().0.1
        } else { let lane = count; count += 1; lane };
        occupied.push(Reverse((end.max(0.).to_bits(), lane)));
        assignments.insert(id, lane);
    }
    (assignments, count)
}

fn send(
    state: &mut NativeTools,
    index: usize,
    document: u32,
    name: &str,
    payload: serde_json::Value,
) {
    if let Some(tool) = state.tools.get_mut(&index) {
        let request = Request::Action {
            document,
            action: name.into(),
            value: payload.to_string(),
        };
        // 高频中间帧可合并，begin/commit/cancel边沿永不丢弃或越序。
        if name == "timeline_update" {
            if let Some(job) = tool.queue.back_mut() {
                if matches!(&job.request,Request::Action{action,document:old,..} if action==name && *old==document)
                {
                    job.request = request;
                    return;
                }
            }
        }
        if let Err(error) = tool.send(request, false) {
            tool.snapshot.status = error;
        }
    }
}
pub(super) fn input(
    mut state: ResMut<NativeTools>,
    mut gestures: ResMut<Gestures>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    workspace: Res<VegetationPresetEditorState>,
    guard: Res<feathrise_editor_core::EditorTextInputGuard>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    blocks: Query<(&Block, &Interaction, &bevy::ui::RelativeCursorPosition)>,
    tracks: Query<(&Seek, &bevy::ui::RelativeCursorPosition, &ComputedNode)>,
) {
    let window = windows.single().ok();
    let cursor_x = window.and_then(Window::physical_cursor_position).map(|position| position.x);
    if let Some(drag) = gestures.drag.as_mut() {
        let visible = state.tools.get(&drag.index).is_some_and(|tool| {
            workspace.active_tab == PresetWorkspaceTab::Domain(tool.domain, drag.document.into())
                && tool.snapshot.active == drag.document && tool.snapshot.timeline.is_some()
        });
        if keys.just_pressed(KeyCode::Escape) || !visible || window.is_none_or(|window| !window.focused) {
            send(&mut state, drag.index, drag.document, "timeline_cancel",
                serde_json::json!({"block":drag.id}));
            gestures.drag = None;
            return;
        }
        // 光标与ComputedNode都取物理像素；侧栏、UI缩放及DPI只在按下时换算一次。
        let previous_delta = drag.delta;
        if let (Some(origin), Some(current)) = (drag.origin_x, cursor_x) {
            drag.delta = (current - origin) as f64;
        } else { drag.delta += motion.delta.x as f64; }
        drag.moved |= drag.delta.abs() >= 3.;
        let snap = !keys.pressed(KeyCode::ShiftLeft) && !keys.pressed(KeyCode::ShiftRight)
            && !keys.pressed(KeyCode::AltLeft) && !keys.pressed(KeyCode::AltRight);
        let payload = if drag.moved {
            serde_json::json!({"block":drag.id,"lane":drag.lane,"delta":drag.delta*drag.scale,
                "snap":snap,"target":match drag.edge{-1=>"start",1=>"end",_=>"move"}})
        } else { serde_json::json!({"block":drag.id}) };
        if mouse.just_released(MouseButton::Left) || !mouse.pressed(MouseButton::Left) {
            send(&mut state, drag.index, drag.document, "timeline_commit", payload);
            gestures.drag = None;
        } else if drag.moved && (drag.delta != previous_delta || motion.delta.x != 0. || keys.any_just_pressed([
            KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::AltLeft, KeyCode::AltRight])
            || keys.any_just_released([KeyCode::ShiftLeft, KeyCode::ShiftRight,
                KeyCode::AltLeft, KeyCode::AltRight])) {
            send(&mut state, drag.index, drag.document, "timeline_update", payload);
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) || state.input_pending || guard.block_shortcuts {
        return;
    }
    // 明确的6px端点子控件优先，不再把整条两端各15%当作隐形缩放热区。
    let pressed = blocks.iter().filter(|(_, interaction, cursor)|
        **interaction == Interaction::Pressed && cursor.cursor_over())
        .min_by_key(|(block, _, _)| if block.edge == 0 { 1 } else { 0 });
    if let Some((block, _, _)) = pressed {
        let Ok((_, _, track)) = tracks.get(block.canvas) else { return; };
        let visible = state.tools.get(&block.index).is_some_and(|tool| {
            workspace.active_tab == PresetWorkspaceTab::Domain(tool.domain, block.document.into())
        });
        if !visible || !track.size().x.is_finite() || track.size().x <= 0. { return; }
        send(&mut state, block.index, block.document, "timeline_select",
            serde_json::json!({"block":block.id,"lane":block.lane}));
        send(&mut state, block.index, block.document, "timeline_begin",
            serde_json::json!({"block":block.id,"lane":block.lane}));
        gestures.drag = Some(Drag {
            index: block.index, document: block.document, lane: block.lane.clone(), id: block.id.clone(),
            delta: 0.,
            scale: block.seconds_per_pixel / track.size().x as f64, edge: block.edge,
            origin_x: cursor_x, moved: false,
        });
        return;
    }
    for (track, cursor, _) in &tracks {
        let visible = state.tools.get(&track.index).is_some_and(|tool| {
            workspace.active_tab == PresetWorkspaceTab::Domain(tool.domain, track.document.into())
        });
        if visible && cursor.cursor_over() && let Some(pos) = cursor.normalized {
            send(&mut state, track.index, track.document, "timeline_seek",
                serde_json::json!({"cursor":track.pan+pos.x as f64*track.span}));
            return;
        }
    }
}

#[cfg(feature = "validation")]
#[derive(Clone, serde::Serialize)]
pub(super) struct ValidationBar {
    pub lane: String,
    pub block: String,
    pub edge: i8,
    pub point: bool,
    pub rect: [f32; 4],
    pub fill: [f32; 4],
    pub literal: bool,
    pub canvas_width: f32,
    pub span: f64,
}
#[cfg(feature = "validation")]
#[derive(Clone, serde::Serialize)]
pub(super) struct ValidationGeometry {
    pub bounds: [f32; 4],
    pub logical_per_pixel: f32,
    pub ruler: [f32; 4],
    pub bars: Vec<ValidationBar>,
    pub references: Vec<[f32; 4]>,
    pub playheads: Vec<[f32; 4]>,
}

/// 验收只读真实布局和正式组件；截图与像素核对另在validation模块完成。
#[cfg(feature = "validation")]
pub(super) fn validation_geometry(world: &mut World, index: usize, document: u32)
    -> Result<ValidationGeometry, String> {
    let rect = |node: &ComputedNode, at: &UiGlobalTransform| {
        [at.translation.x - node.size().x * 0.5, at.translation.y - node.size().y * 0.5,
            node.size().x, node.size().y]
    };
    let (bounds,logical_per_pixel) = world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<TimelineCanvasRoot>>()
        .iter(world).next().map(|(node,at)|(rect(node,at),node.inverse_scale_factor()))
        .ok_or("真实时间轴画布尚未挂载")?;
    let ruler = world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<Ruler>>()
        .iter(world).next().map(|(node,at)|rect(node,at)).ok_or("真实刻度行尚未挂载")?;
    let bars = world.query::<(&Block,&ComputedNode,&UiGlobalTransform,&BackgroundColor,Has<EditorLiteralColor>)>()
        .iter(world).filter(|(widget,node,_,_,_)|widget.index==index && widget.document==document && !node.is_empty())
        .map(|(widget,node,at,fill,literal)| {
            let rgba=fill.0.to_srgba();
            ValidationBar {lane:widget.lane.clone(),block:widget.id.clone(),edge:widget.edge,
                point:widget.point,rect:rect(node,at),fill:[rgba.red,rgba.green,rgba.blue,rgba.alpha],literal,
                canvas_width:world.get::<ComputedNode>(widget.canvas).map_or(0.,|node|node.size().x),
                span:widget.seconds_per_pixel}
        }).collect();
    let references = world.query_filtered::<(&LaneItem,&ComputedNode,&UiGlobalTransform),Without<Block>>()
        .iter(world).filter(|(_,node,_)|!node.is_empty())
        .map(|(_,node,at)|rect(node,at)).collect();
    let playheads = world.query::<(&Playhead,&ComputedNode,&UiGlobalTransform,&Visibility,Has<EditorLiteralColor>)>()
        .iter(world).filter(|(marker,node,_,visibility,literal)|marker.index==index
            && marker.document==document && !node.is_empty() && **visibility!=Visibility::Hidden && *literal)
        .map(|(_,node,at,_,_)|rect(node,at)).collect();
    Ok(ValidationGeometry {bounds,logical_per_pixel,ruler,bars,references,playheads})
}

/// 只注入内置验收的按下边沿；命中位置须已由真实UI布局计算，原input继续负责事务和DLL请求。
#[cfg(feature = "validation")]
pub(super) fn validation_press_end(world: &mut World, index: usize, document: u32,
    lane: &str, block: &str) -> Result<(),String> {
    let entity=world.query::<(Entity,&Block,&bevy::ui::RelativeCursorPosition)>()
        .iter(world).find(|(_,widget,cursor)|widget.index==index && widget.document==document
            && widget.lane==lane && widget.id==block && widget.edge==1 && cursor.cursor_over())
        .map(|(entity,_,_)|entity).ok_or("真实尾抓手尚未取得布局命中，拒绝伪造命中状态")?;
    world.entity_mut(entity).insert(Interaction::Pressed);
    world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    Ok(())
}

#[cfg(feature = "validation")]
pub(super) fn validation_drag_state(world: &World) -> Option<serde_json::Value> {
    world.get_resource::<Gestures>()?.drag.as_ref().map(|drag|serde_json::json!({
        "index":drag.index,"document":drag.document,"lane":drag.lane,"block":drag.id,
        "edge":drag.edge,"moved":drag.moved,"delta_pixels":drag.delta,"seconds_per_pixel":drag.scale}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_plugin_api::{TimelineBlockPresentation, TimelineLane, TimelineLanePresentation};

    #[test]
    fn canvas_uses_original_ruler_filled_bars_points_handles_and_two_thin_reference_bands() {
        fn spawn(mut commands: Commands) {
            let mut live = TimelineBlock { id:"stable-main".into(),label:"0.00–0.80s".into(),
                start:0.,end:0.8,selected:true,presentation:TimelineBlockPresentation {
                    color:Some([0.18,0.48,0.52,0.88]),..Default::default() } };
            let mut blocks = vec![live.clone()];
            for band in [0,1] {
                let mut reference = live.clone();
                reference.id = format!("reference-{band}");
                reference.presentation.read_only = true;
                reference.presentation.reference_of = Some(live.id.clone());
                reference.presentation.reference_band = band;
                blocks.push(reference);
            }
            live.id = "stable-event".into(); live.start=0.9; live.end=0.9;
            live.presentation.point=true; blocks.push(live);
            let snapshot = Snapshot {active:7,timeline:Some(Timeline {
                duration:1.,cursor:0.5,playing:false,zoom:1.,pan:0.,span:Some(1.),
                lanes:vec![TimelineLane {id:"track".into(),label:"轨道".into(),blocks,
                    presentation:TimelineLanePresentation {color:Some([0.18,0.48,0.52,0.88]),
                        select:true,add:true,..Default::default()} }],
            }),..Default::default()};
            commands.spawn(Node::default()).with_children(|root|
                render(root, &Handle::default(), 1, &snapshot));
        }
        let mut app=App::new(); app.add_systems(Startup,spawn); app.update();
        let world=app.world_mut();
        assert_eq!(world.query::<&Ruler>().iter(world).count(),1);
        let widgets:Vec<_>=world.query::<(&Block,&Node)>().iter(world).collect();
        assert_eq!(widgets.iter().filter(|(block,_)|block.id=="stable-main").count(),3,
            "主条块及两个明确抓手保留同一稳定身份");
        assert_eq!(widgets.iter().filter(|(block,_)|block.id=="stable-event").count(),1);
        let point=widgets.iter().find(|(block,_)|block.id=="stable-event").unwrap();
        assert_eq!(point.1.width,px(6));
        assert!(widgets.iter().all(|(block,_)|!block.id.starts_with("reference-")),
            "参考条没有按钮或拖动目标");
        let references:Vec<_>=world.query::<(&LaneItem,&Node)>().iter(world)
            .filter(|(_,node)|node.height==px(2)).collect();
        assert_eq!(references.len(),2);
        assert_eq!(references[0].1.top,px(2));
        assert_eq!(references[1].1.top,px(TRACK_HEIGHT-5.));
        assert!(references.iter().all(|(item,_)|item.block=="stable-main"));
        assert_eq!(world.query_filtered::<&Playhead,With<EditorLiteralColor>>().iter(world).count(),2);
    }

    #[test]
    fn packed_hit_regions_separate_dense_points_and_reuse_adjacent_interval_rows() {
        let (lanes,count)=pack_lanes(vec![("a".into(),0.,10.),("b".into(),0.,20.),
            ("c".into(),20.,30.)]);
        assert_eq!(count,2);
        assert_eq!(lanes["a"],lanes["c"]);
        let (_,count)=pack_lanes((0..96).map(|id|(id.to_string(),49.,51.)).collect());
        assert_eq!(count,96,"密集命中标记不叠成同一个不可点击目标");
    }

    #[test]
    fn finite_extreme_windows_keep_hidden_cursor_layout_and_ticks_finite() {
        let tiny=f64::from_bits(1);
        assert_eq!(axis_percent(f64::MAX,0.,tiny),100.);
        assert_eq!(axis_percent(0.,f64::MAX,tiny),0.);
        assert_eq!(axis_percent(0.,0.,tiny),0.);
        assert_eq!(axis_percent(f64::MAX,0.,f64::MAX),100.);
        for tick in 0..=4 {
            assert!(tick_seconds(0.,f64::MAX,tick).is_finite());
            assert!(tick_seconds(f64::MAX*0.5,f64::MAX*0.5,tick).is_finite());
        }
        assert_eq!(tick_seconds(0.,f64::MAX,4),f64::MAX);
    }
}
