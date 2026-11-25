//! 所选位置或缩放轨道的分量曲线。手柄只在释放时提交一笔原有关键帧历史。

mod plot;
use super::{
    context::AnimationContext,
    resources::{AnimationEditorState, Edit, PendingAction},
};
use bevy::{prelude::*, text::FontSourceTemplate, ui::RelativeCursorPosition};
use feathrise_editor_animation_model::{
    data::Channel,
    media::{AssetPreviewState, AssetPreviewWorkspace},
};
use feathrise_editor_ui::{
    EditorButtonClick, InteractionButton, colors, localization::label_scene, metrics,
};
use plot::Plot;

// 绘图采样与原时间轴的图形槽同量级；完整轨道仍按当前秒制窗口二分采样。
const SEGMENTS: usize = super::components::KEY_SLOTS;
#[derive(Component, Clone, Copy)]
enum ButtonKind {
    Toggle,
    Axis(usize),
}
#[derive(Component, Clone, Default)]
struct Graph;
#[derive(Component, Clone, Default)]
struct AxisRow;
#[derive(Component, Clone, Copy)]
enum Part {
    Segment(usize),
    Join(usize),
    Arm(bool),
    Grip(bool),
    Key,
}
#[derive(Clone, Copy, PartialEq)]
struct Target {
    source: u64,
    revision: u64,
    clip: usize,
    channel: usize,
    key: Option<usize>,
}
#[derive(Clone, Copy)]
struct Drag {
    target: Target,
    plot: Plot,
    outgoing: bool,
    slope: f32,
    origin: Vec2,
    handle: Vec2,
}
#[derive(Clone, Copy)]
struct ScrollReveal {
    scroller: Entity,
    before: f32,
    shown: f32,
}
#[derive(Resource, Default)]
pub(super) struct CurveEditor {
    open_source: Option<u64>,
    reveal: bool,
    scroll: Option<ScrollReveal>,
    axis: usize,
    drag: Option<Drag>,
}
#[derive(Component, Clone, Default)]
struct Displayed(Option<(Target, Plot)>);

fn button(label: &'static str, kind: ButtonKind) -> impl Scene {
    bsn! { InteractionButton ~{kind}
        Node { height: px(24), padding: UiRect::horizontal(px(5)), align_items: AlignItems::Center, border: px(1) }
        BorderColor::all(colors::COPPER_BORDER_WEAK) BackgroundColor(colors::SLOT_BG)
        Children [Text(label) @label_scene(label) Pickable::IGNORE
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)),
                font_size: px(metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)]
    }
}

pub(super) fn panel() -> impl Scene {
    bsn! { Node { width: percent(100), flex_direction: FlexDirection::Column, flex_shrink: 0.0 }
        Children [
            Node { width: percent(100), column_gap: px(4) }
            Children [@button("曲线", ButtonKind::Toggle)--
                AxisRow Node { display: Display::None, column_gap: px(3) }
                Children [@button("X", ButtonKind::Axis(0))-- @button("Y", ButtonKind::Axis(1))-- @button("Z", ButtonKind::Axis(2))]]
            -- Graph Displayed::default() InteractionButton RelativeCursorPosition::default()
            Node { display: Display::None, width: percent(100), height: px(102), flex_shrink: 0.0, overflow: Overflow::clip() }
            BackgroundColor(colors::PANEL_BG_STRONG)
        ]
    }
}

pub(super) fn install(app: &mut App) {
    use feathrise_editor_animation_model::assembly::AnimationSystems;
    app.init_resource::<CurveEditor>()
        .add_systems(
            Update,
            input
                .after(super::timeline::input)
                .in_set(AnimationSystems::Input)
                .in_set(feathrise_editor_core::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (setup, render)
                .chain()
                .in_set(AnimationSystems::Presentation)
                .in_set(feathrise_editor_core::SceneEditorSystemSet::Presentation),
        )
        .add_systems(PostUpdate, reveal_graph.after(bevy::ui::UiSystems::Layout));
}

/// 展开后只滚动一次所属时间轴面板，依据实际布局露出整张曲线，后续滚轮仍由作者控制。
fn reveal_graph(
    mut curve: ResMut<CurveEditor>,
    state: Res<AnimationEditorState>,
    graph: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<Graph>>,
    parents: Query<&ChildOf>,
    mut scrollers: Query<(
        &Node,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
    )>,
) {
    if curve.open_source.is_none() && curve.scroll.is_none() {
        return;
    }
    if !state.editable()
        || curve.open_source != Some(state.source_revision)
        || selection(&state).is_none()
    {
        curve.open_source = None;
        curve.reveal = false;
        // 收起或切换资产后回到展开前的位置；手动滚动过的面板保留作者选择。
        if let Some(saved) = curve.scroll.take()
            && let Ok((_, viewport, _, mut scroll)) = scrollers.get_mut(saved.scroller)
        {
            let maximum = (viewport.content_size().y - viewport.size().y).max(0.0)
                * viewport.inverse_scale_factor();
            if (scroll.y - saved.shown).abs() < 0.5
                || (scroll.y - saved.shown.min(maximum)).abs() < 0.5
            {
                scroll.y = saved.before.min(maximum);
            }
        }
        return;
    }
    if !curve.reveal {
        return;
    }
    let Ok((entity, node, position)) = graph.single() else {
        return;
    };
    if node.size().y <= 1.0 {
        return;
    }
    curve.reveal = false;
    for ancestor in parents.iter_ancestors(entity) {
        let Ok((style, viewport, center, mut scroll)) = scrollers.get_mut(ancestor) else {
            continue;
        };
        if style.overflow.y != OverflowAxis::Scroll {
            continue;
        }
        let top = position.translation.y - node.size().y * 0.5;
        let bottom = top + node.size().y;
        let visible_top = center.translation.y - viewport.size().y * 0.5;
        let visible_bottom = visible_top + viewport.size().y;
        let delta = if top < visible_top || node.size().y > viewport.size().y {
            top - visible_top
        } else {
            (bottom - visible_bottom).max(0.0)
        };
        if delta != 0.0 {
            let before = scroll.y;
            let maximum = (viewport.content_size().y - viewport.size().y).max(0.0)
                * viewport.inverse_scale_factor();
            scroll.y = (before + delta * viewport.inverse_scale_factor()).clamp(0.0, maximum);
            curve.scroll = Some(ScrollReveal {
                scroller: ancestor,
                before,
                shown: scroll.y,
            });
        }
        break;
    }
}

fn setup(mut commands: Commands, graphs: Query<Entity, Added<Graph>>) {
    for graph in &graphs {
        commands.entity(graph).with_children(|parent| {
            let mut add = |part, color| {
                parent.spawn((
                    part,
                    Pickable::IGNORE,
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    BackgroundColor(color),
                ));
            };
            for i in 0..SEGMENTS {
                add(Part::Segment(i), colors::TEXT_ACCENT);
                add(Part::Join(i), colors::TEXT_ACCENT);
            }
            for outgoing in [false, true] {
                add(Part::Arm(outgoing), colors::TEXT_MUTED);
            }
            for outgoing in [false, true] {
                add(Part::Grip(outgoing), colors::TEXT_PRIMARY);
            }
            add(Part::Key, colors::TEXT_ACCENT);
        });
    }
}

#[cfg(feature = "validation")]
pub(super) fn visible_control(world: &mut World, grip: bool) -> Option<Entity> {
    world
        .query::<(
            Entity,
            Option<&ButtonKind>,
            Option<&Part>,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&bevy::ui::CalculatedClip>,
            &InheritedVisibility,
        )>()
        .iter(world)
        .find_map(|(e, button, part, node, position, clip, v)| {
            let chosen = if grip {
                matches!(part, Some(Part::Grip(true)))
            } else {
                matches!(button, Some(ButtonKind::Toggle))
            };
            if !chosen || node.size().min_element() <= 1.0 || !v.get() {
                return None;
            }
            let bounds = Rect::from_center_size(position.translation, node.size());
            clip.is_none_or(|clip| {
                feathrise_editor_ui::helpers::clipped_ui_bounds(clip, bounds)
                    .is_some_and(|visible| visible.contains(position.translation))
            })
            .then_some(e)
        })
}

fn selection(state: &AnimationEditorState) -> Option<(Target, &Channel, f32, f32)> {
    let s = state.current()?;
    let index = s.channel?;
    let channel = s.document.clips.get(s.clip)?.channels.get(index)?;
    if !matches!(channel.path.as_str(), "translation" | "scale") {
        return None;
    }
    Some((
        Target {
            source: state.source_revision,
            revision: state.revision,
            clip: s.clip,
            channel: index,
            key: s.key,
        },
        channel,
        s.window_start,
        s.window_span,
    ))
}

fn input(
    context: AnimationContext,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    mut state: ResMut<AnimationEditorState>,
    mut curve: ResMut<CurveEditor>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<(&ButtonKind, &EditorButtonClick), Changed<EditorButtonClick>>,
    graph: Query<(&RelativeCursorPosition, &ComputedNode, &Displayed), With<Graph>>,
) {
    if !state.editable()
        || !context.available(&preview, &media)
        || !context.source_matches(&state, &preview, &media, context.host.asset_root())
        || state.busy()
        || state.playing
        || context.host.weights_own_viewport
        || state.current().is_some_and(|s| s.pose.is_some())
        || keys.just_pressed(KeyCode::Escape)
    {
        curve.drag = None;
        return;
    }
    for (kind, click) in &buttons {
        if !click.is_clicked() {
            continue;
        }
        match *kind {
            ButtonKind::Toggle => {
                curve.open_source = (curve.open_source.is_none()).then_some(state.source_revision);
                curve.reveal = curve.open_source.is_some();
            }
            ButtonKind::Axis(axis) => curve.axis = axis,
        }
        curve.drag = None;
    }
    let Some((target, channel, _, span)) = selection(&state) else {
        curve.drag = None;
        return;
    };
    let Ok((cursor, node, displayed)) = graph.single() else {
        return;
    };
    let Some((shown, plot)) = displayed
        .0
        .filter(|(shown, _)| *shown == target && curve.open_source == Some(target.source))
    else {
        curve.drag = None;
        return;
    };
    let Some(index) = target.key else {
        return;
    };
    let Some(key) = channel.keys.get(index) else {
        return;
    };
    let size = node.size() * node.inverse_scale_factor();
    let position = cursor.normalized.map(|v| v + Vec2::splat(0.5));
    if mouse.just_pressed(MouseButton::Left)
        && cursor.cursor_over()
        && let Some(position) = position
    {
        for outgoing in [false, true] {
            let Some(point) = plot::handle(channel, index, curve.axis, outgoing, span) else {
                continue;
            };
            if ((plot.point(point.x, point.y) - position) * size).length() <= 8.0 {
                let slope = if outgoing {
                    &key.out_tangent
                } else {
                    &key.in_tangent
                }[curve.axis];
                curve.drag = Some(Drag {
                    target: shown,
                    plot,
                    outgoing,
                    slope,
                    origin: position,
                    handle: plot.point(point.x, point.y),
                });
                break;
            }
        }
    }
    let axis = curve.axis;
    let Some(drag) = &mut curve.drag else {
        return;
    };
    if drag.target != target {
        curve.drag = None;
        return;
    }
    if let Some(position) = position.filter(|p| *p != drag.origin) {
        // 保留抓取点与手柄中心的偏移，单击手柄不会创建一笔微小的浮点修改。
        let desired = drag.plot.sample(drag.handle + (position - drag.origin));
        let dt = desired.x - key.time;
        // 手柄保持在关键帧各自的一侧；跨过关键帧时沿用上次有效斜率。
        if dt.abs() * size.x / span >= 1.0 && (dt > 0.0) == drag.outgoing {
            let slope = (desired.y - key.value[axis]) / dt;
            if slope.is_finite() {
                drag.slope = slope;
            }
        }
    } else if position == Some(drag.origin) {
        drag.slope = if drag.outgoing {
            &key.out_tangent
        } else {
            &key.in_tangent
        }[axis];
    }
    if mouse.pressed(MouseButton::Left) {
        state.mouse_owned = true;
        return;
    }
    let drag = curve.drag.take().unwrap();
    let mut after = key.clone();
    let tangent = if drag.outgoing {
        &mut after.out_tangent
    } else {
        &mut after.in_tangent
    };
    if tangent[axis] != drag.slope {
        tangent[axis] = drag.slope;
        state.pending = Some(PendingAction::Edit(Edit::Key {
            clip: target.clip,
            channel: target.channel,
            before: Some(key.clone()),
            after: Some(after),
        }));
    }
    state.mouse_owned = true;
}

fn layout(node: &mut Node, transform: &mut UiTransform, a: Vec2, b: Vec2, width: f32, dot: bool) {
    let center = (a + b) * 0.5;
    let length = if dot { width } else { a.distance(b).max(1.0) };
    node.display = Display::Flex;
    node.width = px(length);
    node.height = px(width);
    node.left = px(center.x - length * 0.5);
    node.top = px(center.y - width * 0.5);
    transform.rotation = if dot {
        Rot2::IDENTITY
    } else {
        Rot2::radians((b.y - a.y).atan2(b.x - a.x))
    };
}

#[allow(clippy::type_complexity)]
fn render(
    state: Res<AnimationEditorState>,
    curve: Res<CurveEditor>,
    mut graphs: Query<
        (&ComputedNode, &mut Node, &mut Displayed),
        (With<Graph>, Without<Part>, Without<AxisRow>),
    >,
    mut axes: Query<&mut Node, (With<AxisRow>, Without<Graph>, Without<Part>)>,
    mut parts: Query<(&Part, &mut Node, &mut UiTransform)>,
    mut buttons: Query<(&ButtonKind, &mut BackgroundColor)>,
    mut last: Local<Option<(Target, usize, Vec2, u32, u32, Option<(bool, u32)>)>>,
) {
    let Ok((computed, mut node, mut displayed)) = graphs.single_mut() else {
        return;
    };
    let selected = selection(&state);
    let visible =
        state.editable() && curve.open_source == Some(state.source_revision) && selected.is_some();
    let display = if visible {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    if let Ok(mut row) = axes.single_mut()
        && row.display != display
    {
        row.display = display;
    }
    for (action, mut background) in &mut buttons {
        let selected = match *action {
            ButtonKind::Toggle => visible,
            ButtonKind::Axis(i) => i == curve.axis,
        };
        let color = if selected {
            colors::SELECT_BG
        } else {
            colors::SLOT_BG
        };
        if background.0 != color {
            background.0 = color;
        }
    }
    let size = computed.size() * computed.inverse_scale_factor();
    if !visible || size.min_element() <= 0.0 {
        displayed.0 = None;
        *last = None;
        return;
    }
    let (target, channel, start, span) = selected.unwrap();
    let axis = curve.axis;
    let drag = curve.drag.filter(|drag| drag.target == target);
    let stamp = (
        target,
        axis,
        size,
        start.to_bits(),
        span.to_bits(),
        drag.map(|d| (d.outgoing, d.slope.to_bits())),
    );
    if last.as_ref() == Some(&stamp) {
        return;
    }
    *last = Some(stamp);
    let preview =
        drag.and_then(|d| channel.preview_tangent(target.key?, axis, d.outgoing, d.slope));
    let at = |time| {
        let source = preview
            .as_ref()
            .filter(|c| time >= c.keys[0].time && time <= c.keys.last().unwrap().time)
            .unwrap_or(channel);
        source.sample(time)[axis]
    };
    let values: [f32; SEGMENTS + 1] =
        std::array::from_fn(|i| at(start + span * i as f32 / SEGMENTS as f32));
    let mut handles = [None; 2];
    let key = target.key.and_then(|k| channel.keys.get(k));
    if let (Some(k), Some(key)) = (target.key, key) {
        for (i, outgoing) in [false, true].into_iter().enumerate() {
            handles[i] = plot::handle(channel, k, axis, outgoing, span).map(|mut p| {
                if let Some(d) = drag.filter(|d| d.outgoing == outgoing) {
                    p.y = key.value[axis] + (p.x - key.time) * d.slope;
                }
                p
            });
        }
    }
    let plot = drag.map_or_else(
        || {
            let extra = handles.iter().flatten().map(|p| p.y);
            let (lo, hi) = values
                .iter()
                .copied()
                .chain(extra)
                .fold((values[0], values[0]), |(l, h), v| (l.min(v), h.max(v)));
            let margin = if lo == hi {
                (lo.abs() * 0.1).max(0.1)
            } else {
                (hi - lo) * 0.12
            };
            Plot {
                start,
                span,
                low: lo - margin,
                high: hi + margin,
            }
        },
        |d| d.plot,
    );
    displayed.0 = Some((target, plot));
    let key_point = key.map(|k| plot.point(k.time, k.value[axis]) * size);
    let handle = |outgoing: bool| handles[outgoing as usize].map(|p| plot.point(p.x, p.y) * size);
    for (part, mut node, mut transform) in &mut parts {
        let segment = match *part {
            Part::Segment(i) | Part::Join(i) => {
                let a = plot.point(start + span * i as f32 / SEGMENTS as f32, values[i]) * size;
                let b = plot.point(
                    start + span * (i + 1) as f32 / SEGMENTS as f32,
                    values[i + 1],
                ) * size;
                let corner = Vec2::new(b.x, a.y);
                match (part, channel.interpolation.as_str()) {
                    (Part::Join(_), "STEP") if a.y != b.y => Some((corner, b, 1.5, false)),
                    (Part::Join(_), _) => None,
                    (_, "STEP") => Some((a, corner, 1.5, false)),
                    _ => Some((a, b, 1.5, false)),
                }
            }
            Part::Arm(out) => key_point.zip(handle(out)).map(|(a, b)| (a, b, 1.0, false)),
            Part::Grip(out) => handle(out).map(|p| (p, p, 8.0, true)),
            Part::Key => key_point.map(|p| (p, p, 6.0, true)),
        };
        if let Some((a, b, w, dot)) = segment {
            layout(&mut node, &mut transform, a, b, w, dot);
        } else if node.display != Display::None {
            node.display = Display::None;
        }
    }
}
