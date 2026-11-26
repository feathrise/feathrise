//! 变量和规则的单行页脚。固定四个页码槽，超出后随当前页移动，按钮不随数据量增长。

use super::{layout::font, *};
use crate::ui::tooltips::{EditorTooltipTarget, TooltipAction};
use bevy::ui::InteractionDisabled;
use feathrise_editor_ui::EditorButtonClick;

const SLOTS: usize = 4;

#[derive(Component)]
pub(super) struct Pager;

#[derive(Component)]
pub(super) enum PagerButton {
    Previous,
    Number(usize),
    Next,
}

impl PagerButton {
    fn action(&self, first: usize) -> Action {
        match self {
            Self::Previous => Action::Previous,
            Self::Number(slot) => Action::Page(first + slot),
            Self::Next => Action::Next,
        }
    }
}

pub(super) fn mount(p: &mut ChildSpawnerCommands, h: &Handle<Font>) {
    p.spawn((
        Pager,
        Name::new("RulesPager"),
        Visibility::Hidden,
        Node {
            display: Display::None,
            align_items: AlignItems::Center,
            column_gap: px(3),
            flex_shrink: 0.,
            ..default()
        },
    ))
    .with_children(|p| {
        button(p, h, PagerButton::Previous, "RulesPrevious", "‹");
        for slot in 0..SLOTS {
            let page = slot + 1;
            button(
                p,
                h,
                PagerButton::Number(slot),
                format!("RulesPage{page}"),
                &page.to_string(),
            );
        }
        button(p, h, PagerButton::Next, "RulesNext", "›");
    });
}

fn button(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    slot: PagerButton,
    name: impl Into<String>,
    caption: &str,
) {
    let action = slot.action(0);
    let tooltip = match slot {
        PagerButton::Previous => Some(TooltipAction::RulesPreviousPage),
        PagerButton::Next => Some(TooltipAction::RulesNextPage),
        PagerButton::Number(_) => None,
    };
    let mut control = p.spawn((
        slot,
        action,
        InteractionButton,
        InteractionDisabled,
        FocusPolicy::Block,
        Name::new(name.into()),
        Node {
            min_width: px(22),
            height: px(metrics::CONTROL_HEIGHT),
            flex_shrink: 0.,
            padding: UiRect::horizontal(px(2)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(metrics::RADIUS)),
            ..default()
        },
        BackgroundColor(c::SLOT_BG),
        BorderColor::all(c::COPPER_BORDER_WEAK),
    ));
    if let Some(tooltip) = tooltip {
        control.insert(EditorTooltipTarget(tooltip));
    }
    control.with_child((
        Text::new(caption),
        TextLayout::no_wrap(),
        font(
            h,
            if tooltip.is_some() {
                19.
            } else {
                metrics::FONT_CONTROL
            },
        ),
        TextColor(c::TEXT_PRIMARY),
        Pickable::IGNORE,
    ));
}

pub(super) fn sync(
    s: Res<Studio>,
    docs: Res<DocumentState>,
    mut bars: Query<(&mut Node, &mut Visibility), (With<Pager>, Without<PagerButton>)>,
    mut buttons: Query<(
        Entity,
        &PagerButton,
        &mut Action,
        &mut Name,
        &mut Node,
        &Children,
        &UiInteraction,
        &mut EditorButtonClick,
        &mut BackgroundColor,
        &mut BorderColor,
        Has<InteractionDisabled>,
    )>,
    mut labels: Query<(&mut Text, &mut TextColor)>,
    hovered: Query<(), (With<PagerButton>, Changed<UiInteraction>)>,
    added: Query<(), Added<Pager>>,
    mut last: Local<Option<(u64, Tab, usize, usize)>>,
    mut commands: Commands,
) {
    if !s.is_changed() && !docs.is_changed() && hovered.is_empty() && added.is_empty() {
        return;
    }
    let (sheet, selection) = s.list_projection();
    let tab = selection.tab;
    let rows = sheet.map_or(&[][..], |sheet| s.rows(sheet, tab));
    let current = page(selection, rows);
    let total = rows.len().div_ceil(ROWS);
    let shown = tab != Tab::Locations && total > 1 && listed_document(&s, &docs).is_some();
    let identity = shown.then_some((s.revision, tab, current, total));
    let changed = *last != identity;
    *last = identity;
    for (mut node, mut visibility) in &mut bars {
        show(&mut node, shown);
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let first = current.saturating_sub(1).min(total.saturating_sub(SLOTS));
    for (
        id,
        slot,
        mut action,
        mut name,
        mut node,
        children,
        interaction,
        mut click,
        mut background,
        mut border,
        disabled,
    ) in &mut buttons
    {
        let next = slot.action(first);
        let (visible, current_page, edge) = match next {
            Action::Previous => (shown, false, current == 0),
            Action::Next => (shown, false, current + 1 >= total),
            Action::Page(page) => (shown && page < total, page == current, false),
            _ => unreachable!(),
        };
        show(&mut node, visible);
        if changed || *action != next {
            // 页码槽复用时作废旧按压，松手只能作用于按下时那一页。
            click.invalidate();
        }
        if !visible {
            continue;
        }
        let inactive = edge || current_page;
        if disabled != inactive {
            if inactive {
                commands.entity(id).insert(InteractionDisabled);
            } else {
                commands.entity(id).remove::<InteractionDisabled>();
            }
        }
        let (mut label, mut tint) = labels.get_mut(children[0]).expect("分页按钮文字");
        if *action != next {
            *action = next;
            if let Action::Page(page) = next {
                let caption = (page + 1).to_string();
                *name = Name::new(format!("RulesPage{caption}"));
                **label = caption;
            }
        }
        background.set_if_neq(BackgroundColor(if current_page {
            c::SELECT_BG
        } else if edge {
            c::SLOT_BG_DISABLED
        } else {
            match interaction {
                UiInteraction::Pressed => c::SELECT_BG_PRESSED,
                UiInteraction::Hovered => c::SELECT_BG_HOVER,
                _ => c::SLOT_BG,
            }
        }));
        border.set_if_neq(BorderColor::all(if current_page {
            c::COPPER_BORDER
        } else {
            c::COPPER_BORDER_WEAK
        }));
        tint.set_if_neq(TextColor(if edge { c::TEXT_DIM } else { c::TEXT_PRIMARY }));
    }
}
