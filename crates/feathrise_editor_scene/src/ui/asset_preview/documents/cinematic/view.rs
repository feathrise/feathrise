//! 中央视口、四条时间尺、可编辑片段属性。标记按当前像素分桶，共用角色/场景时间轴的取时与绘制。
use super::super::components::DocumentCenterHost;
use super::{model::*, *};
use bevy::{
    prelude::*,
    text::{EditableText, LineHeight},
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::{InteractionButton, animation_timeline as axis, colors as c};
#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(crate) struct FilmImage;
#[derive(Component)]
pub(super) struct Subtitle;
#[derive(Component)]
pub(super) struct Status;
#[derive(Component)]
pub(super) struct Clock;
#[derive(Component)]
pub(super) struct Tick(usize);
#[derive(Component)]
pub(super) struct MarkerCaption;
#[derive(Component)]
pub(super) struct LaneWidget(pub Option<Lane>);
#[derive(Component)]
pub(super) struct Playhead;
#[derive(Component)]
pub(super) struct MarkerWidget {
    pub lane: Lane,
    pub index: usize,
    pub slot: usize,
}
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Play,
    Stop,
    Save,
    Export,
    Advanced,
    Settings,
    Lane(Lane),
    New,
    Replace,
    Remove,
    Capture,
    Pose,
    Clip,
    Subtitle,
    Yarn,
    Previous,
    Next,
    Smooth,
    Wait,
    ControlCamera,
    ControlPlayer,
    ApplySettings,
    UseSelection,
    PickClip,
    Undo,
    Redo,
    Go,
}
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Field {
    At,
    Length,
    Target,
    Instance,
    Name,
    Position,
    Rotation,
    Scale,
    Fov,
}
const FIELDS: [Field; 9] = [
    Field::At,
    Field::Length,
    Field::Target,
    Field::Instance,
    Field::Name,
    Field::Position,
    Field::Rotation,
    Field::Scale,
    Field::Fov,
];
#[derive(Component)]
pub(super) struct FieldRow(pub Field);
#[derive(Component)]
pub(super) struct Label(Field);
#[derive(Component)]
pub(super) struct OptionLabel(pub Action);
pub(super) fn font(h: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: h.clone().into(),
        font_size: size.into(),
        ..default()
    }
}
fn row() -> Node {
    Node {
        width: percent(100),
        min_width: px(0),
        align_items: AlignItems::Center,
        column_gap: px(3),
        row_gap: px(2),
        flex_wrap: FlexWrap::Wrap,
        flex_shrink: 0.,
        ..default()
    }
}
pub(super) fn button(p: &mut ChildSpawnerCommands, h: &Handle<Font>, label: &str, action: Action) {
    p.spawn((
        InteractionButton,
        action,
        FocusPolicy::Block,
        Node {
            height: px(22),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(5)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(c::SLOT_BG),
        BorderColor::all(c::COPPER_BORDER_WEAK),
    ))
    .with_child((
        OptionLabel(action),
        Text::new(label),
        font(h, 11.),
        TextColor(c::TEXT_PRIMARY),
        Pickable::IGNORE,
    ));
}
fn field(p: &mut ChildSpawnerCommands, h: &Handle<Font>, kind: Field) {
    p.spawn((FieldRow(kind), row())).with_children(|r| {
        r.spawn((
            Label(kind),
            Text::default(),
            font(h, 11.),
            TextColor(c::TEXT_MUTED),
            Node {
                width: px(65),
                ..default()
            },
        ));
        r.spawn((
            kind,
            crate::ui::text_input::EditorTextInput,
            bevy::input_focus::tab_navigation::TabIndex(0),
            FocusPolicy::Block,
            bevy::text::TextCursorStyle::default(),
            EditableText {
                allow_newlines: false,
                ..default()
            },
            TextLayout::no_wrap(),
            font(h, 11.),
            LineHeight::Px(16.),
            TextColor(c::TEXT_VALUE),
            BackgroundColor(c::INPUT_BG),
            BorderColor::all(c::COPPER_BORDER_WEAK),
            Node {
                flex_grow: 1.,
                flex_basis: px(0),
                min_width: px(30),
                height: px(21),
                border: UiRect::all(px(1)),
                padding: UiRect::horizontal(px(3)),
                overflow: Overflow::clip(),
                ..default()
            },
        ));
    });
}
pub(super) fn mount(
    mut commands: Commands,
    hosts: Query<Entity, Added<DocumentCenterHost>>,
    server: Res<AssetServer>,
) {
    let h = feathrise_editor_core::editor_font(&server, feathrise_editor_core::FONT_PATH);
    for host in &hosts {
        commands.entity(host).with_children(|parent| {
            parent
                .spawn((
                    Panel,
                    Node {
                        display: Display::None,
                        width: percent(100),
                        height: percent(100),
                        min_width: px(0),
                        min_height: px(0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(4)),
                        row_gap: px(3),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(c::PANEL_BG_STRONG),
                ))
                .with_children(|p| {
                    p.spawn(row()).with_children(|r| {
                        for (name, a) in [
                            ("播放 / 暂停", Action::Play),
                            ("退出试播", Action::Stop),
                            ("演出设置", Action::Settings),
                            ("保存", Action::Save),
                            ("撤销", Action::Undo),
                            ("重做", Action::Redo),
                            ("导出视频…", Action::Export),
                            ("高级源码", Action::Advanced),
                        ] {
                            button(r, &h, name, a);
                        }
                    });
                    p.spawn(Node {
                        width: percent(100),
                        flex_grow: 1.,
                        min_height: px(55),
                        min_width: px(0),
                        column_gap: px(5),
                        ..default()
                    })
                    .with_children(|middle| {
                        middle
                            .spawn((
                                FilmImage,
                                bevy::ui::widget::ViewportNode { camera: None },
                                RelativeCursorPosition::default(),
                                Node {
                                    flex_grow: 1.,
                                    flex_basis: px(0),
                                    width: px(0),
                                    min_width: px(0),
                                    height: percent(100),
                                    align_items: AlignItems::FlexEnd,
                                    justify_content: JustifyContent::Center,
                                    padding: UiRect::all(px(8)),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                BackgroundColor(Color::BLACK),
                            ))
                            .with_child((
                                Subtitle,
                                Text::default(),
                                font(&h, 17.),
                                TextColor(Color::WHITE),
                                BackgroundColor(Color::srgba(0., 0., 0., 0.65)),
                                Pickable::IGNORE,
                            ));
                        middle
                            .spawn((
                                ScrollPosition::default(),
                                RelativeCursorPosition::default(),
                                crate::components::SceneEditorRightPanelScroll,
                                Node {
                                    width: percent(38),
                                    max_width: px(330),
                                    min_width: px(172),
                                    height: percent(100),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(2),
                                    overflow: Overflow::scroll_y(),
                                    ..default()
                                },
                            ))
                            .with_children(|form| {
                                form.spawn(row()).with_children(|r| {
                                    for (name, a) in [
                                        ("变换", Action::Pose),
                                        ("已有动作", Action::Clip),
                                        ("字幕", Action::Subtitle),
                                        ("Yarn 节点", Action::Yarn),
                                    ] {
                                        button(r, &h, name, a)
                                    }
                                });
                                for kind in FIELDS {
                                    field(form, &h, kind);
                                }
                                form.spawn(row()).with_children(|r| {
                                    for (name, a) in [
                                        ("平滑", Action::Smooth),
                                        ("等待对白", Action::Wait),
                                        ("接管镜头", Action::ControlCamera),
                                        ("接管玩家", Action::ControlPlayer),
                                        ("采用所选物件", Action::UseSelection),
                                        ("选择已有动作…", Action::PickClip),
                                    ] {
                                        button(r, &h, name, a)
                                    }
                                });
                                form.spawn(row()).with_children(|r| {
                                    for (name, a) in [
                                        ("新增", Action::New),
                                        ("更新选中", Action::Replace),
                                        ("删除选中", Action::Remove),
                                        ("记录视口机位", Action::Capture),
                                        ("应用设置", Action::ApplySettings),
                                        ("定位时间", Action::Go),
                                    ] {
                                        button(r, &h, name, a)
                                    }
                                });
                            });
                    });
                    p.spawn(row()).with_children(|r| {
                        button(r, &h, "上一项", Action::Previous);
                        button(r, &h, "下一项", Action::Next);
                        r.spawn((
                            Clock,
                            Text::default(),
                            font(&h, 12.),
                            TextColor(c::TEXT_VALUE),
                        ));
                    });
                    for lane in std::iter::once(None).chain(super::model::LANES.map(Some)) {
                        p.spawn(Node {
                            width: percent(100),
                            height: px(22),
                            flex_shrink: 0.,
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|r| {
                            r.spawn(Node {
                                width: px(58),
                                ..default()
                            })
                            .with_children(|side| {
                                if let Some(lane) = lane {
                                    button(side, &h, lane.label(), Action::Lane(lane));
                                } else {
                                    side.spawn((
                                        Text::new("时间 / 秒"),
                                        font(&h, 10.),
                                        TextColor(c::TEXT_MUTED),
                                    ));
                                }
                            });
                            let parent = r.target_entity();
                            r.commands()
                                .spawn_scene(axis::lane(lane.is_none()))
                                .insert((LaneWidget(lane), ChildOf(parent)))
                                .with_children(|axis_row| {
                                    let canvas = axis_row.target_entity();
                                    axis_row.commands().spawn_scene(axis::playhead()).insert((
                                        Playhead,
                                        ZIndex(4),
                                        ChildOf(canvas),
                                    ));
                                    if let Some(lane) = lane {
                                        for slot in 0..axis::KEY_SLOTS {
                                            axis_row
                                                .spawn((
                                                    MarkerWidget {
                                                        lane,
                                                        index: usize::MAX,
                                                        slot,
                                                    },
                                                    InteractionButton,
                                                    FocusPolicy::Block,
                                                    Node {
                                                        display: Display::None,
                                                        position_type: PositionType::Absolute,
                                                        top: px(3),
                                                        min_width: px(5),
                                                        height: px(14),
                                                        overflow: Overflow::clip(),
                                                        padding: UiRect::horizontal(px(2)),
                                                        border: UiRect::right(px(1)),
                                                        ..default()
                                                    },
                                                    BackgroundColor(c::TEXT_ACCENT),
                                                    BorderColor::all(c::TRACK_BG),
                                                ))
                                                .with_child((
                                                    MarkerCaption,
                                                    Text::default(),
                                                    font(&h, 9.),
                                                    TextLayout::no_wrap(),
                                                    TextColor(Color::BLACK),
                                                    Pickable::IGNORE,
                                                ));
                                        }
                                    } else {
                                        for i in 0..=8 {
                                            axis_row
                                                .spawn((
                                                    axis::tick_node(i),
                                                    BorderColor::all(c::COPPER_BORDER_WEAK),
                                                    Pickable::IGNORE,
                                                ))
                                                .with_child((
                                                    Tick(i),
                                                    Text::default(),
                                                    font(&h, 10.),
                                                    TextColor(c::TEXT_MUTED),
                                                    Pickable::IGNORE,
                                                ));
                                        }
                                    }
                                });
                        });
                    }
                    p.spawn((
                        Status,
                        Text::default(),
                        font(&h, 11.),
                        TextColor(c::TEXT_MUTED),
                        Node {
                            min_height: px(17),
                            flex_shrink: 0.,
                            ..default()
                        },
                    ));
                });
        });
    }
}
pub(super) fn display(
    s: Res<Studio>,
    doc: Res<DocumentState>,
    mut panels: Query<&mut Node, With<Panel>>,
    mut texts: Query<(&mut Text, Has<Status>), Or<(With<Status>, With<Clock>)>>,
) {
    let yes = active(&doc).is_some();
    for mut n in &mut panels {
        let display = if yes { Display::Flex } else { Display::None };
        if n.display != display {
            n.display = display;
        }
    }
    if !yes {
        return;
    }
    for (mut txt, status) in &mut texts {
        let value = if status {
            let saved = doc.current().map_or("", |d| d.status.as_str());
            [s.notice.as_str(), saved]
                .into_iter()
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
        } else if let Some(t) = &s.timeline {
            format!(
                "{}    {:.2} / {:.2} 秒    {}",
                t.film.name,
                s.cursor,
                t.film.duration,
                s.lane.label()
            )
        } else {
            "正在读取演出…".into()
        };
        if txt.as_str() != value {
            **txt = value;
        }
    }
}
pub(super) fn field_value<'a>(f: &'a Form, k: Field) -> &'a str {
    match k {
        Field::At => &f.at,
        Field::Length => &f.length,
        Field::Target => &f.target,
        Field::Instance => &f.instance,
        Field::Name => &f.name,
        Field::Position => &f.position,
        Field::Rotation => &f.rotation,
        Field::Scale => &f.scale,
        Field::Fov => &f.fov,
    }
}
fn label(k: Field, s: &Studio) -> Option<&'static str> {
    if s.settings {
        return match k {
            Field::Name => Some("演出名称"),
            Field::Length => Some("总时长 秒"),
            _ => None,
        };
    }
    match k {
        Field::At => Some("时间 秒"),
        Field::Length => (s.kind != Kind::Pose).then_some("时长 秒"),
        Field::Target => match s.lane {
            Lane::Camera => None,
            Lane::Dialogue => Some("台词 ID"),
            _ => Some("目标 ID"),
        },
        Field::Instance => matches!(s.lane, Lane::Actor | Lane::Object).then_some("实例路径"),
        Field::Name => match s.kind {
            Kind::Clip => Some("动作名"),
            Kind::Subtitle => Some("字幕正文"),
            Kind::Yarn => Some("Yarn 节点"),
            _ => None,
        },
        Field::Position => (s.kind == Kind::Pose).then_some("位置 XYZ"),
        Field::Rotation => (s.kind == Kind::Pose).then_some("旋转 °XYZ"),
        Field::Scale => (s.kind == Kind::Pose).then_some("缩放 XYZ"),
        Field::Fov => (s.lane == Lane::Camera).then_some("视角 °"),
    }
}
pub(super) fn form(
    s: Res<Studio>,
    mut version: Local<Option<u64>>,
    mut inputs: Query<(&Field, &mut EditableText)>,
    mut labels: Query<(&Label, &mut Text), Without<OptionLabel>>,
    mut rows: Query<(&FieldRow, &mut Node), Without<Action>>,
    mut buttons: Query<(&Action, &mut Node), Without<FieldRow>>,
    mut captions: Query<(&OptionLabel, &mut Text), Without<Label>>,
) {
    if s.timeline.is_none() || *version == Some(s.form_revision) {
        return;
    }
    *version = Some(s.form_revision);
    for (k, mut input) in &mut inputs {
        let value = field_value(&s.form, *k);
        if input.value() != value {
            crate::ui::text_input::replace_single_line_value(&mut input, value, None);
        }
    }
    for (row, mut n) in &mut rows {
        n.display = if label(row.0, &s).is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (k, mut text) in &mut labels {
        if let Some(l) = label(k.0, &s) {
            **text = l.into();
        }
    }
    for (a, mut n) in &mut buttons {
        let visible = match a {
            Action::ControlCamera | Action::ControlPlayer | Action::ApplySettings => s.settings,
            Action::Smooth => !s.settings && s.kind == Kind::Pose,
            Action::Wait => !s.settings && s.kind == Kind::Yarn,
            Action::Capture => !s.settings && s.lane == Lane::Camera,
            Action::PickClip => !s.settings && s.kind == Kind::Clip,
            Action::UseSelection => !s.settings && s.lane == Lane::Object,
            Action::Pose | Action::Clip => {
                !s.settings && matches!(s.lane, Lane::Actor | Lane::Object)
            }
            Action::Subtitle | Action::Yarn => !s.settings && s.lane == Lane::Dialogue,
            Action::Replace | Action::Remove => !s.settings && s.selection.is_some(),
            Action::New | Action::Go => !s.settings,
            _ => true,
        };
        n.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (a, mut text) in &mut captions {
        let (l, v) = match a.0 {
            Action::Smooth => ("平滑", s.form.smooth),
            Action::Wait => ("等待对白", s.form.wait),
            Action::ControlCamera => ("接管镜头", s.control_camera),
            Action::ControlPlayer => ("接管玩家", s.control_player),
            _ => continue,
        };
        **text = format!("{l} {}", if v { "✓" } else { "–" });
    }
}
pub(super) fn markers(
    s: Res<Studio>,
    mut markers: Query<
        (
            &mut MarkerWidget,
            &mut Node,
            &mut BackgroundColor,
            &Children,
        ),
        Without<Playhead>,
    >,
    mut heads: Query<&mut Node, (With<Playhead>, Without<MarkerWidget>)>,
    mut ticks: Query<(&Tick, &mut Text), Without<MarkerCaption>>,
    mut captions: Query<&mut Text, With<MarkerCaption>>,
    mut painted: Local<Option<(usize, Option<Item>)>>,
) {
    let Some(t) = &s.timeline else { return };
    let total = t.film.duration.max(f32::EPSILON);
    for mut n in &mut heads {
        let left = percent(100. * s.cursor / total);
        if n.left != left {
            n.left = left;
        }
    }
    let key = (Arc::as_ptr(t) as usize, s.selection);
    if *painted == Some(key) {
        return;
    }
    *painted = Some(key);
    for (tick, mut text) in &mut ticks {
        let sec = total * tick.0 as f32 / 8.;
        let label = format!("{sec:.1}");
        if text.as_str() != label {
            **text = label;
        }
    }
    for (mut w, mut n, mut color, children) in &mut markers {
        let m = &t.lanes[w.lane.index()];
        let i = if m.len() <= axis::KEY_SLOTS {
            w.slot
        } else {
            m.partition_point(|k| k.at < total * (w.slot as f32 / axis::KEY_SLOTS as f32))
        };
        let marker = m.get(i).filter(|v| {
            m.len() <= axis::KEY_SLOTS
                || w.slot + 1 == axis::KEY_SLOTS
                || v.at < total * ((w.slot + 1) as f32 / axis::KEY_SLOTS as f32)
        });
        if let Some(v) = marker {
            if let Some(&child) = children.first()
                && let Ok(mut caption) = captions.get_mut(child)
            {
                let label = if v.end > v.at {
                    t.label(v.item).chars().take(48).collect::<String>()
                } else {
                    String::new()
                };
                if caption.as_str() != label {
                    **caption = label;
                }
            }
            w.index = i;
            n.display = Display::Flex;
            n.left = percent((100. * v.at / total).min(99.));
            n.width = if v.end > v.at {
                percent(100. * (v.end - v.at) / total)
            } else {
                px(7)
            };
            color.0 = if s.selection == Some(v.item) {
                c::TEXT_ACCENT
            } else {
                [
                    Color::srgb(0.38, 0.65, 0.93),
                    Color::srgb(0.32, 0.75, 0.51),
                    Color::srgb(0.88, 0.60, 0.35),
                    Color::srgb(0.70, 0.54, 0.88),
                ][w.lane.index()]
            };
        } else {
            w.index = usize::MAX;
            n.display = Display::None;
        }
    }
}
