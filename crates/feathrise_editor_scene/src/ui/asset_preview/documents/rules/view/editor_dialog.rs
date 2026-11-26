//! 变量和规则共用独立编辑弹窗，标题、正文、状态与操作区沿用唯一的原生表单。
use super::super::{DocumentState, Studio, Tab, shown_document};
use super::{Field, Info, PanelBody, c, field_shown, show, table::FormHost};
use crate::components::SceneEditorRoot;
use bevy::{
    input::mouse::MouseWheel,
    input_focus::{FocusCause, InputFocus, tab_navigation::TabGroup},
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::UiInteraction;

#[derive(Component)]
pub(crate) struct RulesEditorDialogOverlay;
#[derive(Component)]
pub(super) struct FormActions;
#[derive(Component)]
pub(super) struct DialogScroll;
#[derive(Component)]
pub(super) struct DialogFrame;
#[derive(Component)]
pub(super) struct Slots {
    header: Entity,
    body: Entity,
    footer: Entity,
    status: Entity,
}

pub(in super::super) fn is_open(s: &Studio, docs: &DocumentState) -> bool {
    matches!(s.sel.tab, Tab::Variables | Tab::Rules)
        && s.editing_item
        && shown_document(s, docs).is_some()
}

pub(super) fn mount(mut commands: Commands, roots: Query<Entity, Added<SceneEditorRoot>>) {
    let Ok(root) = roots.single() else { return };
    let overlay = commands
        .spawn((
            RulesEditorDialogOverlay,
            Name::new("RulesEditorDialog"),
            ChildOf(root),
            GlobalZIndex(890), // 高于工作区和文件浮窗，设置及场景确认仍有更高优先级。
            FocusPolicy::Block,
            UiInteraction::default(),
            Visibility::Hidden,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(8)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(c::DIALOG_BACKDROP),
        ))
        .id();
    let frame = commands
        .spawn((
            ChildOf(overlay),
            DialogFrame,
            TabGroup::modal(), // Tab 只在弹窗输入框间循环，中央文档的焦点不参与。
            Node {
                width: px(480),
                max_width: percent(100),
                max_height: percent(94),
                ..feathrise_editor_ui::dialog_panel_node()
            },
            BackgroundColor(c::DIALOG_BG),
            BorderColor::all(c::DIALOG_BORDER),
        ))
        .id();
    let header = commands
        .spawn((
            ChildOf(frame),
            feathrise_editor_ui::dialog_header_node(),
            BackgroundColor(c::DIALOG_HEADER_BG),
            BorderColor::all(c::DIALOG_BORDER),
        ))
        .id();
    let body = commands
        .spawn((
            ChildOf(frame),
            DialogScroll,
            Name::new("RulesEditorDialogScroll"),
            RelativeCursorPosition::default(),
            ScrollPosition::default(),
            Node {
                // 卡片达到窗口上限时，只有正文收缩，标题和操作区留在原位。
                overflow: Overflow::scroll_y(),
                ..feathrise_editor_ui::dialog_body_node()
            },
        ))
        .id();
    let status = commands
        .spawn((
            ChildOf(frame),
            Node {
                width: percent(100),
                min_width: px(0),
                padding: UiRect::horizontal(px(12)),
                flex_shrink: 0.,
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .id();
    let footer = commands
        .spawn((
            ChildOf(frame),
            feathrise_editor_ui::dialog_footer_node(),
            BorderColor::all(c::DIALOG_BORDER),
        ))
        .id();
    commands.entity(overlay).insert(Slots {
        header,
        body,
        footer,
        status,
    });
}

pub(super) fn sync(
    s: Res<Studio>,
    docs: Res<DocumentState>,
    mut overlays: Query<(&Slots, &mut Node, &mut Visibility), With<RulesEditorDialogOverlay>>,
    mut frames: Query<&mut Node, (With<DialogFrame>, Without<RulesEditorDialogOverlay>)>,
    mut form: Query<
        (Entity, &mut Node, &mut BackgroundColor),
        (
            With<FormHost>,
            Without<RulesEditorDialogOverlay>,
            Without<DialogFrame>,
        ),
    >,
    actions: Query<Entity, With<FormActions>>,
    panel: Query<Entity, With<PanelBody>>,
    labels: Query<(Entity, &Info)>,
    fields: Query<(Entity, &Field)>,
    mut focus: ResMut<InputFocus>,
    mut scrolls: Query<&mut ScrollPosition, With<DialogScroll>>,
    mut commands: Commands,
) {
    let Ok((slots, mut node, mut visibility)) = overlays.single_mut() else {
        return;
    };
    let open = is_open(&s, &docs);
    if open && let Ok(mut frame) = frames.single_mut() {
        let width = px(if s.sel.tab == Tab::Rules { 640 } else { 480 });
        if frame.width != width {
            frame.width = width;
        }
    }
    // 字段类型切换可能收起已取得焦点的字段，让 Tab 从本弹窗的名称框继续。
    if open
        && focus.get().is_none_or(|entity| {
            fields
                .get(entity)
                .is_ok_and(|(_, field)| !field_shown(s.sel.tab, &s.form, *field))
        })
        && let Some((entity, _)) = fields.iter().find(|(_, field)| **field == Field::Id)
    {
        focus.set(entity, FocusCause::Navigated);
    }
    if open == (node.display != Display::None) {
        return;
    }
    let (Ok((form, mut form_node, mut form_color)), Ok(actions), Ok(panel)) =
        (form.single_mut(), actions.single(), panel.single())
    else {
        return;
    };
    show(&mut node, open);
    *visibility = if open {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if open {
        // 标题、字段和按钮共享弹窗外壳，正文只留其自身的内边距。
        form_node.border = UiRect::ZERO;
        form_node.padding = UiRect::ZERO;
        form_color.0 = Color::NONE;
        commands.entity(slots.body).add_child(form);
        commands.entity(slots.footer).add_child(actions);
        if let Ok(mut scroll) = scrolls.get_mut(slots.body) {
            scroll.0 = Vec2::ZERO;
        }
    } else {
        // 地点区域继续使用原表格挂载，保持场景视口取点可用。
        form_node.border = UiRect::all(px(1));
        form_node.padding = UiRect::all(px(9));
        form_color.0 = c::PANEL_BG_SECTION;
        commands.entity(form).add_child(actions);
    }
    for (entity, label) in &labels {
        match label {
            Info::EditorTitle if open => {
                commands.entity(slots.header).add_child(entity);
            }
            Info::EditorTitle => {
                commands.entity(form).insert_child(0, entity);
            }
            Info::Status if open => {
                commands.entity(slots.status).add_child(entity);
            }
            Info::Status => {
                commands.entity(panel).insert_child(1, entity);
            }
            _ => (),
        }
    }
}

pub(super) fn scroll(
    mut wheels: MessageReader<MouseWheel>,
    forms: Query<(&ComputedNode, &ChildOf), With<FormHost>>,
    mut bodies: Query<
        (
            Entity,
            &RelativeCursorPosition,
            &ComputedNode,
            &mut ScrollPosition,
            &InheritedVisibility,
        ),
        With<DialogScroll>,
    >,
) {
    let delta = wheels
        .read()
        .filter(|wheel| wheel.y.is_finite())
        .map(|wheel| crate::ui::systems::editor_scroll_delta(wheel.unit, wheel.y))
        .sum::<f32>();
    let (Ok((body, cursor, node, mut scroll, visible)), Ok((form, parent))) =
        (bodies.single_mut(), forms.single())
    else {
        return;
    };
    if visible.get() && parent.parent() == body {
        // 滚动终点取唯一表单的实测高度，类型切换或放大窗口后也收回旧滚动位置。
        let max =
            ((form.size().y - node.content_box().height()) * node.inverse_scale_factor()).max(0.);
        let delta = if cursor.cursor_over() { delta } else { 0. };
        let next = (scroll.y + delta).clamp(0., max);
        if scroll.y != next {
            scroll.y = next;
        }
    }
}
