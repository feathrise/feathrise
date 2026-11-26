//! 三个工具共用一套有界行池；选择行只移动唯一的原生表单，重排不会丢失输入法或草稿。
use super::{
    layout::{ScrollArea, button, column, editor, font, info, row},
    *,
};
use feathrise_editor_documents_model::project_rules::{ActionForm, TriggerForm};

const CELL_PAD_X: f32 = 8.;
const CELL_GAP: f32 = 10.;

#[cfg(test)]
#[path = "table_geometry.rs"]
mod geometry;

#[derive(Component)]
pub(super) struct FormHost;

#[derive(Component)]
pub(super) enum TablePart {
    Slot(Option<usize>),
    Summary(usize),
}

#[derive(Clone, Copy, Debug)]
enum Column {
    Id,
    Kind,
    Value,
}
impl Column {
    fn label(self, tab: Tab) -> &'static str {
        match (tab, self) {
            (Tab::Variables, Self::Id) => "变量名称",
            (Tab::Variables, Self::Kind) => "类型",
            (Tab::Variables, Self::Value) => "默认值",
            (Tab::Rules, Self::Id) => "规则 ID",
            (Tab::Rules, Self::Kind) => "监听",
            (Tab::Rules, Self::Value) => "执行动作",
            (Tab::Locations, Self::Id) => "地点 ID",
            (Tab::Locations, Self::Kind) => "场景",
            (Tab::Locations, Self::Value) => "范围（XYZ）",
        }
    }
    fn value(self, sheet: &Sheet, tab: Tab, index: usize) -> String {
        if matches!(self, Self::Id) {
            return sheet.id(tab, index).unwrap_or_default().to_owned();
        }
        match tab {
            Tab::Variables => {
                let v = &sheet.model.variables[index];
                match self {
                    Self::Kind => match v.kind {
                        ValueKind::Bool => "布尔",
                        ValueKind::Number => "数字",
                        ValueKind::Text => "文本",
                    }
                    .into(),
                    _ if v.kind == ValueKind::Bool => {
                        if v.default == "true" { "真" } else { "假" }.into()
                    }
                    _ if v.default.is_empty() => "空文本".into(),
                    _ => v.default.clone(),
                }
            }
            Tab::Rules => {
                let rule = &sheet.model.rules[index];
                match self {
                    Self::Kind => match &rule.when {
                        TriggerForm::Event { event, .. } => event_label(event).into(),
                        TriggerForm::Changed { variable, .. } => format!("{variable} 变化"),
                    },
                    _ => match rule.actions.first() {
                        Some(ActionForm::Request { request, .. }) => request_label(request).into(),
                        Some(ActionForm::Set { variable, .. }) => format!("设置 {variable}"),
                        None => "—".into(),
                    },
                }
            }
            Tab::Locations => {
                let loc = &sheet.model.locations[index];
                match self {
                    Self::Kind => loc.scene.clone(),
                    _ => format!("{} → {}", loc.min.join(", "), loc.max.join(", ")),
                }
            }
        }
    }
}

#[derive(Component)]
pub(super) struct Cell {
    slot: usize,
    column: Column,
}

fn cells(p: &mut ChildSpawnerCommands, h: &Handle<Font>, slot: usize) {
    for column in [Column::Id, Column::Kind, Column::Value] {
        p.spawn((
            Cell { slot, column },
            Name::new(format!("RulesCell{slot}{column:?}")),
            Text::default(),
            TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
            font(h, metrics::FONT_CONTROL),
            TextColor(c::TEXT_PRIMARY),
            Node {
                min_width: px(0),
                ..default()
            },
            Pickable::IGNORE,
        ));
    }
    button(p, h, format!("RulesRow{slot}"), "编辑", Action::Row(slot));
}

fn grid() -> Node {
    Node {
        display: Display::Grid,
        width: percent(100),
        align_items: AlignItems::Center,
        column_gap: px(CELL_GAP),
        row_gap: px(5),
        padding: UiRect::axes(px(CELL_PAD_X), px(6)),
        flex_shrink: 0.,
        ..default()
    }
}

/// 文本叶子必须用与轨道一致的实际宽度测量换行。只给父节点 fr 轨道时，Bevy 的
/// 自动宽度文本会用逐字换行的高度居中，字形却在最终列宽上绘制，越出所在记录。
fn column_width(width: f32) -> f32 {
    ((width - 2. * CELL_PAD_X - CELL_GAP) * 0.5).max(0.)
}
pub(super) fn list(p: &mut ChildSpawnerCommands, h: &Handle<Font>) {
    p.spawn(Node {
        justify_content: JustifyContent::SpaceBetween,
        ..row()
    })
    .with_children(|r| {
        info(r, h, Info::Selection, "RulesSelection");
        button(r, h, "RulesNew", "新建变量", Action::New);
    });
    p.spawn((TablePart::Slot(None), column(0.)))
        .with_children(|r| {
            r.spawn((
                FormHost,
                Group::Editor,
                Name::new("RulesForm"),
                Node {
                    width: percent(100),
                    padding: UiRect::all(px(9)),
                    border: UiRect::all(px(1)),
                    ..column(7.)
                },
                BackgroundColor(c::PANEL_BG_SECTION),
                BorderColor::all(c::COPPER_BORDER),
            ))
            .with_children(|f| editor(f, h));
        });
    for i in 0..ROWS {
        p.spawn((
            TablePart::Slot(Some(i)),
            Name::new(format!("RulesTableRow{i}")),
            column(0.),
        ))
        .with_children(|r| {
            r.spawn((TablePart::Summary(i), grid(), BackgroundColor(c::PANEL_BG)))
                .with_children(|s| cells(s, h, i));
        });
    }
    p.spawn(row()).with_children(|r| {
        button(r, h, "RulesPrevious", "上一页", Action::Previous);
        button(r, h, "RulesNext", "下一页", Action::Next);
    });
}

pub(super) fn sync(
    s: Res<Studio>,
    docs: Res<DocumentState>,
    mut parts: Query<(Entity, &TablePart, &mut Node, &mut Visibility)>,
    form: Query<(Entity, &ChildOf), With<FormHost>>,
    mut values: Query<(&Cell, &mut Text)>,
    added: Query<(), Added<FormHost>>,
    mut painted: Local<Option<u64>>,
    mut commands: Commands,
) {
    if active(&docs).is_none() {
        return;
    }
    if !s.is_changed() && !docs.is_changed() && added.is_empty() {
        return;
    }
    let tab = s.sel.tab;
    let count = s.sheet.as_ref().map_or(0, |m| m.len(tab));
    let start = s.sel.item.unwrap_or(0) / ROWS * ROWS;
    let edit_slot = s.sel.item.map(|i| i % ROWS);
    for (entity, part, mut node, mut vis) in &mut parts {
        let shown = match *part {
            TablePart::Slot(None) => s.editing_item && edit_slot.is_none(),
            TablePart::Slot(Some(i)) => start + i < count,
            TablePart::Summary(i) => start + i < count && !(s.editing_item && edit_slot == Some(i)),
        };
        let is_grid = !matches!(part, TablePart::Slot(_));
        let display = if !shown {
            Display::None
        } else if is_grid {
            Display::Grid
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        vis.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if let TablePart::Slot(slot) = *part
            && s.editing_item
            && edit_slot == slot
            && let Ok((editor, parent)) = form.single()
            && parent.parent() != entity
        {
            commands.entity(entity).add_child(editor);
        }
    }
    if *painted == Some(s.revision) && added.is_empty() {
        return;
    }
    *painted = Some(s.revision);
    for (cell, mut caption) in &mut values {
        let label = cell.column.label(tab);
        let value = if start + cell.slot < count {
            let value = cell
                .column
                .value(s.sheet.as_ref().unwrap(), tab, start + cell.slot);
            format!("{label}  {value}")
        } else {
            String::new()
        };
        set(&mut caption, &value);
    }
}

pub(super) fn fit(
    scroll: Query<&ComputedNode, With<ScrollArea>>,
    mut rows: Query<(&TablePart, &mut Node), Without<Cell>>,
    mut cells: Query<&mut Node, With<Cell>>,
    added: Query<(), Added<Cell>>,
    mut last: Local<Option<f32>>,
) {
    let Ok(view) = scroll.single() else {
        *last = None;
        return;
    };
    let width = view.size().x * view.inverse_scale_factor();
    if width <= 0. {
        *last = None;
        return;
    }
    if *last == Some(width) && added.is_empty() {
        return;
    }
    *last = Some(width);
    let width = column_width(width);
    for (part, mut row) in &mut rows {
        if matches!(part, TablePart::Summary(_)) {
            row.grid_template_columns = vec![RepeatedGridTrack::px(2, width)];
        }
    }
    for mut node in &mut cells {
        node.width = px(width);
    }
}
