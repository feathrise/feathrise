//! 右侧蒙皮工具页和画布左侧选骨入口，复用编辑器现有主题与按钮。

use super::super::{animation::AnimationEditorState, rigging::RiggingState};
use super::{
    edit::Tool,
    resources::{Session, WeightEditor},
    systems::{Gate, Target},
};
use crate::{
    components::SceneEditorRightPanelScroll,
    resources::SceneEditorState,
    ui::{
        scene::FONT_PATH,
        sidebars::{
            EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarPage,
            SceneEditorSidebarState,
        },
        theme as colors,
        tooltips::{EditorTooltipTarget, TooltipAction},
    },
    viewport::modes::{MODE_BUTTON_SIZE, MODE_ICON_SIZE, MODE_RAIL_WIDTH, ViewportMode, mode_icon_path},
};
use bevy::{
    asset::HandleTemplate,
    gltf::Gltf,
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, InteractionDisabled, RelativeCursorPosition},
};
use feathrise_editor_ui::{
    ButtonColors, InteractionButton, UiInteraction, localization::EditorLocale, metrics,
};

const NORMAL: ButtonColors = ButtonColors::new(
    colors::BUTTON_DEV_SMALL_NORMAL,
    colors::BUTTON_DEV_SMALL_HOVERED,
    colors::BUTTON_DEV_SMALL_PRESSED,
);
const SELECTED: ButtonColors = ButtonColors::new(
    colors::SELECT_BG,
    colors::SELECT_BG_HOVER,
    colors::SELECT_BG_PRESSED,
);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Open,
    Discard,
    OpenSaved,
    Rest,
    Compare,
    Heat,
    Skeleton,
    Visible,
    Mirror,
    Mask,
    Edit,
    Mode(Tool),
    Mesh(i8),
    Lock,
    Clear,
    Apply,
    Assign,
    Remove,
}

#[derive(Component, Clone, Copy)]
pub(super) enum Label {
    Notice,
    EntryNotice,
    Target,
    Geometry,
    Joint,
    Selection,
    Legend,
    Button(Action),
}

#[derive(Component, Clone, Default)]
pub(super) struct Entry;

#[derive(Component, Clone, Default)]
pub(in crate::ui::asset_preview) struct BoneToolRail;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct EditControls;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Amount,
    Radius,
    Strength,
}

fn label(role: Label) -> impl Scene {
    let layout = if matches!(role, Label::Button(_)) {
        TextLayout::new(Justify::Center, bevy::text::LineBreak::NoWrap)
    } else {
        TextLayout::new(Justify::Left, bevy::text::LineBreak::AnyCharacter)
    };
    bsn! {
        ~{role} Text("") ~{FocusPolicy::Pass}
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(metrics::FONT_CONTROL)
        }
        TextColor(colors::TEXT_PRIMARY)
        ~{layout}
        Node { min_width: px(0), flex_shrink: 0.0, overflow: Overflow::clip_x() }
    }
}

fn button(action: Action) -> impl Scene {
    bsn! {
        InteractionButton ~{action} ~{NORMAL}
        Node {
            min_height: px(24), min_width: px(0), flex_grow: 1.0, flex_basis: px(0),
            justify_content: JustifyContent::Center, align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(4)), border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)), overflow: Overflow::clip_x()
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@label(Label::Button(action))]
    }
}

fn row() -> impl Scene {
    bsn! { Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, column_gap: px(3) } }
}

pub(in crate::ui::asset_preview) fn entry() -> impl Scene {
    bsn! {
        Entry
        Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column,
            row_gap: px(3), flex_shrink: 0.0 }
        Children [@button(Action::Open)--@label(Label::EntryNotice)]
    }
}

pub(in crate::ui::asset_preview) fn viewport_tools() -> impl Scene {
    bsn! {
        BoneToolRail RelativeCursorPosition::default() UiInteraction::default()
        ~{FocusPolicy::Block} ZIndex(25)
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: px(8), top: px(8), width: px(MODE_RAIL_WIDTH),
            flex_direction: FlexDirection::Column, row_gap: px(2),
            padding: UiRect::all(px(2)), border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(2)),
        }
        BackgroundColor(colors::PANEL_BG_STRONG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @mode_button(ViewportMode::Select, Action::Mode(Tool::Bone), TooltipAction::WeightSelect)--
            @mode_button(ViewportMode::Edit, Action::Edit, TooltipAction::WeightEdit)
        ]
    }
}

fn mode_button(mode: ViewportMode, action: Action, tooltip: TooltipAction) -> impl Scene {
    let icon = HandleTemplate::Path(mode_icon_path(mode).into());
    bsn! {
        InteractionButton ~{action} ~{NORMAL} ~{FocusPolicy::Block}
        EditorTooltipTarget(tooltip)
        Node {
            width: px(MODE_BUTTON_SIZE), height: px(MODE_BUTTON_SIZE), flex_shrink: 0.0,
            justify_content: JustifyContent::Center, align_items: AlignItems::Center,
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            ImageNode { image: icon, color: colors::TEXT_PRIMARY }
            Node { width: px(MODE_ICON_SIZE), height: px(MODE_ICON_SIZE), flex_shrink: 0.0 }
            ~{FocusPolicy::Pass} Pickable::IGNORE
        ]
    }
}

fn edit_controls() -> impl Scene {
    bsn! {
        EditControls
        Node { display: Display::None, flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            @row() Children [@button(Action::Mesh(-1))--@button(Action::Mesh(1))]--
            @row() Children [@button(Action::Lock)]--
            @row() Children [@button(Action::Mode(Tool::Point))--@button(Action::Mode(Tool::Box))--@button(Action::Clear)]--
            @row() Children [@button(Action::Mode(Tool::Add))--@button(Action::Mode(Tool::Subtract))--@button(Action::Mode(Tool::Smooth))]--
            @row() Children [@button(Action::Visible)--@button(Action::Mirror)--@button(Action::Mask)]--
            @row() Children [@button(Action::Apply)--@button(Action::Remove)--@button(Action::Assign)]--
            @label(Label::Selection)--
            @label(Label::Legend)--
            @row() Children [@button(Action::Compare)--@button(Action::Rest)]--
            @row() Children [@button(Action::OpenSaved)]
        ]
    }
}

pub(crate) fn sidebar_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Weights))
        SceneEditorRightPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node {
            display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(), row_gap: px(4), padding: UiRect::all(px(3))
        }
        Children [
            @label(Label::Target)--
            @label(Label::Notice)--
            @label(Label::Geometry)--
            @label(Label::Joint)--
            @edit_controls()--
            @row() Children [@button(Action::Heat)--@button(Action::Skeleton)]--
            @row() Children [@button(Action::Discard)]
        ]
    }
}

pub(super) fn tr(
    locale: EditorLocale,
    chinese: &'static str,
    english: &'static str,
) -> &'static str {
    if locale == EditorLocale::English {
        english
    } else {
        chinese
    }
}

fn caption(action: Action, session: Option<&Session>, locale: EditorLocale) -> &'static str {
    let (zh, en) = match action {
        Action::Open => ("编辑蒙皮权重", "Edit skin weights"),
        Action::Discard if session.is_some_and(Session::dirty) => ("丢弃草稿", "Discard draft"),
        Action::Discard => ("关闭工具", "Close tool"),
        Action::OpenSaved => ("打开另存模型", "Open saved model"),
        Action::Rest => ("静置编辑", "Rest pose"),
        Action::Compare if session.is_some_and(|session| session.original) => {
            ("查看草稿", "Show draft")
        }
        Action::Compare => ("查看原始", "Show original"),
        Action::Heat => ("热力图", "Heatmap"),
        Action::Skeleton => ("骨架", "Skeleton"),
        Action::Visible => ("可见面", "Visible only"),
        Action::Mirror => ("镜像", "Mirror"),
        Action::Mask => ("仅选区", "Selection mask"),
        Action::Mesh(-1) => ("上一图元", "Previous primitive"),
        Action::Mesh(_) => ("下一图元", "Next primitive"),
        Action::Lock
            if session.is_some_and(|session| session.current_locks()[session.joint as usize]) =>
        {
            ("解锁权重", "Unlock weights")
        }
        Action::Lock => ("锁定权重", "Lock weights"),
        Action::Clear => ("清选", "Clear"),
        Action::Edit => ("编辑权重", "Edit weights"),
        Action::Mode(Tool::Bone) => ("选骨骼", "Pick bone"),
        Action::Mode(Tool::Point) => ("选点", "Vertex"),
        Action::Mode(Tool::Box) => ("框选", "Box"),
        Action::Mode(Tool::Set) => ("设定权重", "Set weight"),
        Action::Mode(Tool::Add) => ("增重", "Add"),
        Action::Mode(Tool::Subtract) => ("减重", "Subtract"),
        Action::Mode(Tool::Smooth) => ("平滑", "Smooth"),
        Action::Apply if session.is_some_and(|session| session.tool == Tool::Set) => {
            ("应用数值", "Apply value")
        }
        Action::Apply => ("设定权重", "Set weight"),
        Action::Assign => ("指定归属", "Assign"),
        Action::Remove => ("移除影响", "Remove"),
    };
    tr(locale, zh, en)
}

pub(super) fn present(
    mut commands: Commands,
    state: Res<WeightEditor>,
    target: Target,
    gate: Gate,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    locale: Option<Res<EditorLocale>>,
    animation: Option<Res<AnimationEditorState>>,
    rigging: Option<Res<RiggingState>>,
    gltfs: Option<Res<Assets<Gltf>>>,
    mut buttons: Query<(
        Entity,
        &Action,
        &UiInteraction,
        &mut ButtonColors,
        &mut BackgroundColor,
        Has<InteractionDisabled>,
    )>,
    mut roots: Query<
        (&mut Node, Has<Entry>, Has<EditControls>),
        Or<(With<Entry>, With<BoneToolRail>, With<EditControls>)>,
    >,
    mut labels: Query<(&Label, &mut Text)>,
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    let entry = sidebar.right == EditorRightSidebarTab::Assets
        && target
            .entry(rigging.as_deref(), gltfs.as_deref())
            .is_some_and(|path| editor.selected_asset.as_deref() == Some(path));
    let session = state
        .session
        .as_ref()
        .filter(|session| target.matches(&session.key));
    let rail = sidebar.right == EditorRightSidebarTab::Weights
        && gate.open(&editor)
        && target.preview.render_visible
        && session.is_some_and(|session| !session.stale);
    let working = state.busy();
    let motion = animation
        .as_ref()
        .is_some_and(|animation| animation.weight_motion_active());
    let returning = state.problem.is_some()
        && animation
            .as_ref()
            .is_some_and(|animation| animation.weight_rest_active());
    let editable = session.is_some_and(|session| !session.stale && !session.original) && !motion;
    let editing = session.is_some_and(|session| session.tool != Tool::Bone);
    for (mut node, is_entry, is_edit) in &mut roots {
        let visible = if is_entry { entry } else if is_edit { editing } else { rail };
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, action, interaction, mut palette, mut background, disabled) in &mut buttons {
        let enabled = !working
            && match *action {
                Action::Open => entry && gate.can_press(&editor),
                Action::Discard => session.is_some(),
                Action::OpenSaved => {
                    session.is_some_and(|session| session.saved_path.is_some() && !session.dirty())
                }
                Action::Rest => session.is_some() && (motion || returning),
                Action::Mode(Tool::Bone) | Action::Edit => rail,
                Action::Mode(tool) => session.is_some() && (!motion || tool == Tool::Box),
                Action::Apply | Action::Assign | Action::Remove => {
                    editable
                        && session.is_some_and(|session| {
                            !session.drafts[session.surface].selection.is_empty()
                                && !session.current_locks()[session.joint as usize]
                        })
                }
                Action::Mirror => session.is_some_and(|session| {
                    session.doc.surfaces[session.surface].skin.mirror.is_some()
                }),
                Action::Mesh(_) => session.is_some_and(|session| session.doc.surfaces.len() > 1),
                _ => session.is_some(),
            };
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let selected = session.is_some_and(|session| match *action {
            Action::Mode(tool) => session.tool == tool,
            Action::Edit => editing,
            Action::Apply => session.tool == Tool::Set,
            Action::Compare => session.original,
            Action::Heat => session.heat,
            Action::Skeleton => session.show_skeleton,
            Action::Visible => session.visible_only,
            Action::Mirror => session.mirror,
            Action::Mask => session.mask,
            Action::Lock => session.current_locks()[session.joint as usize],
            _ => false,
        });
        let desired = if selected { SELECTED } else { NORMAL };
        if *palette != desired {
            *palette = desired;
            background.0 = match *interaction {
                UiInteraction::None => desired.normal,
                UiInteraction::Hovered => desired.hovered,
                UiInteraction::Pressed => desired.pressed,
            };
        }
    }
    for (&label, mut text) in &mut labels {
        let next = match label {
            Label::Button(Action::Rest) if returning => {
                tr(locale, "返回问题帧", "Return to issue").into()
            }
            Label::Button(Action::Rest) if motion && state.problem.is_some() => {
                tr(locale, "继续修权", "Edit selection").into()
            }
            Label::Button(Action::Mode(Tool::Box)) if motion => {
                tr(locale, "框选问题帧", "Box on pose").into()
            }
            Label::Button(action) => caption(action, session, locale).to_owned(),
            Label::EntryNotice if entry && session.is_none() => state.notice.clone(),
            Label::EntryNotice => String::new(),
            Label::Notice if !state.notice.is_empty() => state.notice.clone(),
            Label::Notice if motion && editing => tr(
                locale,
                "选择「框选问题帧」后在画布框选，进入静置修权。",
                "Choose Box on pose, then drag on the model to edit the selection.",
            )
            .into(),
            Label::Notice => String::new(),
            _ => session
                .map(|session| session_label(label, session, locale))
                .unwrap_or_default(),
        };
        if text.0 != next {
            text.0 = next;
        }
    }
}

fn session_label(label: Label, session: &Session, locale: EditorLocale) -> String {
    let surface = &session.doc.surfaces[session.surface];
    let joint = &surface.skin.joints[session.joint as usize];
    let draft = &session.drafts[session.surface];
    match label {
        Label::Target => {
            if session.dirty() {
                format!(
                    "{}  ·  {}",
                    session.key.path,
                    tr(locale, "未保存", "Unsaved")
                )
            } else {
                session.key.path.clone()
            }
        }
        Label::Geometry => format!(
            "{}\nMesh {}/{} · {} {}",
            surface.name,
            surface.mesh,
            surface.primitive,
            draft.weights.len(),
            tr(locale, "顶点", "vertices")
        ),
        Label::Joint => format!(
            "{}  {}/{}",
            joint.name,
            usize::from(session.joint) + 1,
            surface.skin.joints.len()
        ),
        Label::Legend if session.heat && !session.original => tr(
            locale,
            "蓝 0 · 红 1 · 粉色为选区",
            "Blue 0 · Red 1 · Pink selection",
        )
        .into(),
        Label::Selection => {
            if let [index] = draft.selection.as_ref() {
                let row = draft.weights[*index as usize];
                let values = row
                    .joints
                    .into_iter()
                    .zip(row.weights)
                    .filter(|(_, weight)| *weight > 0.0)
                    .map(|(joint, weight)| {
                        format!(
                            "{}  {:.2}%",
                            surface.skin.joints[joint as usize].name,
                            weight * 100.0
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{} {}\n{values}", tr(locale, "顶点", "Vertex"), index)
            } else {
                format!(
                    "{} {}",
                    draft.selection.len(),
                    tr(locale, "个顶点已选", "vertices selected")
                )
            }
        }
        _ => String::new(),
    }
}
