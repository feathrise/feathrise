//! 三个工具共用一套有界行池；选择行只移动唯一的原生表单，重排不会丢失输入法或草稿。
use super::{
    layout::{ScrollArea, button, button_with_icon, column, editor, font, info, row},
    *,
};
use feathrise_editor_documents_model::project_rules::{ActionForm, TriggerForm};
use feathrise_editor_ui::data_table::{self, Table};

const CELL_PAD_X: f32 = 8.;
const CELL_GAP: f32 = 10.;
const ACTION_GAP: f32 = 4.;
pub(super) const ROW_ACTION_WIDTH: f32 = 27.;
const ACTION_WIDTH: f32 = 2. * ROW_ACTION_WIDTH + ACTION_GAP;

#[cfg(test)]
#[path = "table_geometry.rs"]
mod geometry;

#[derive(Component)]
pub(super) struct FormHost;

#[derive(Component)]
pub(super) struct RulesTable;

#[derive(Component)]
pub(super) enum TablePart {
    Slot(Option<usize>),
    Summary(usize),
    Heading,
}

#[derive(Component)]
pub(super) enum EditVisual {
    Button,
    Text,
    Icon,
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
            (Tab::Variables, Self::Value) => "值",
            (Tab::Rules, Self::Id) => "规则 ID",
            (Tab::Rules, Self::Kind) => "监听",
            (Tab::Rules, Self::Value) => "执行动作",
            (Tab::Locations, Self::Id) => "地点 ID",
            (Tab::Locations, Self::Kind) => "场景",
            (Tab::Locations, Self::Value) => "范围（XYZ）",
        }
    }
    fn value(self, sheet: &Sheet, tab: Tab, scene: Option<&str>, index: usize) -> String {
        if matches!(self, Self::Id) {
            return sheet.id(tab, scene, index).unwrap_or_default().to_owned();
        }
        match tab {
            Tab::Variables => {
                let v = &sheet.model.variables_in(scene)[index];
                match self {
                    Self::Kind => VariableKind::of(v).label().into(),
                    // 默认值沿用配置中的字面值，布尔在列表和表单都显示 true / false。
                    _ if v.default.is_empty() => "空文本".into(),
                    _ => v.default.replace(['\r', '\n'], " "),
                }
            }
            Tab::Rules => {
                let rule = &sheet.model.rules_in(scene)[index];
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
    slot: Option<usize>,
    column: Column,
}

#[derive(Component)]
pub(super) struct ColumnTrack;

fn cells(p: &mut ChildSpawnerCommands, h: &Handle<Font>, table: Entity, slot: Option<usize>) {
    for column in [Column::Id, Column::Kind, Column::Value] {
        p.spawn((ColumnTrack, data_table::cell(table, column as usize)))
            .with_children(|cell| {
                cell.spawn((
                    Cell { slot, column },
                    ColumnTrack,
                    Name::new(match slot {
                        Some(slot) => format!("RulesCell{slot}{column:?}"),
                        None => format!("RulesHeading{column:?}"),
                    }),
                    Text::default(),
                    TextLayout::no_wrap(),
                    font(h, metrics::FONT_CONTROL),
                    TextColor(if slot.is_some() {
                        c::TEXT_PRIMARY
                    } else {
                        c::TEXT_MUTED
                    }),
                    Node {
                        min_width: px(0),
                        width: percent(100),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                if slot.is_none() && !matches!(column, Column::Value) {
                    cell.spawn((
                        Name::new(format!("RulesResize{}", column as usize)),
                        data_table::resize_handle(table, column as usize),
                    ));
                }
            });
    }
}

fn location_grid() -> Node {
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
fn row_actions(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    table: Entity,
    slot: usize,
    edit_icon: &Handle<Image>,
    delete_icon: &Handle<Image>,
) {
    p.spawn((ColumnTrack, data_table::cell(table, 3)))
        .with_children(|cell| {
            cell.spawn(Node {
                width: percent(100),
                flex_wrap: FlexWrap::NoWrap,
                column_gap: px(ACTION_GAP),
                ..row()
            })
            .with_children(|r| {
                button_with_icon(
                    r,
                    h,
                    format!("RulesRow{slot}"),
                    "编辑",
                    Action::Row(slot),
                    Some(edit_icon),
                );
                button_with_icon(
                    r,
                    h,
                    format!("RulesDeleteRow{slot}"),
                    "删除",
                    Action::DeleteRow(slot),
                    Some(delete_icon),
                );
            });
        });
}
fn table_columns() -> Table {
    use data_table::Column;
    Table::new([
        Column::flex(48., 2.),
        Column::flex(56., 1.),
        Column::flex(43., 2.),
        Column::fixed(ACTION_WIDTH),
    ])
}
pub(super) fn list(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    edit_icon: &Handle<Image>,
    delete_icon: &Handle<Image>,
) {
    p.spawn(Node {
        justify_content: JustifyContent::End,
        ..row()
    })
    .with_children(|r| {
        button(r, h, "RulesNew", "新建变量", Action::New);
    });
    let mut data = p.spawn((
        RulesTable,
        Name::new("RulesDataTable"),
        table_columns(),
        Node {
            width: percent(100),
            ..column(0.)
        },
    ));
    let table = data.id();
    data.with_children(|p| {
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
        p.spawn((
            TablePart::Heading,
            Name::new("RulesHeading"),
            data_table::row(None),
        ))
        .with_children(|r| {
            cells(r, h, table, None);
            r.spawn(data_table::cell(table, 3)).with_child((
                Name::new("RulesHeadingActions"),
                Text::new("操作列"),
                TextLayout::no_wrap(),
                font(h, metrics::FONT_CONTROL),
                TextColor(c::TEXT_MUTED),
                Pickable::IGNORE,
            ));
        });
        for i in 0..ROWS {
            p.spawn((
                TablePart::Slot(Some(i)),
                Name::new(format!("RulesTableRow{i}")),
                column(0.),
            ))
            .with_children(|r| {
                r.spawn((
                    TablePart::Summary(i),
                    Name::new(format!("RulesTableBodyRow{i}")),
                    data_table::row(Some(i)),
                ))
                .with_children(|s| {
                    cells(s, h, table, Some(i));
                    row_actions(s, h, table, i, edit_icon, delete_icon);
                });
            });
        }
    });
    p.spawn((
        Name::new("RulesPagination"),
        Node {
            justify_content: JustifyContent::SpaceBetween,
            flex_wrap: FlexWrap::NoWrap,
            ..row()
        },
    ))
    .with_children(|r| {
        info(r, h, Info::Selection, "RulesSelection");
        super::pager::mount(r, h);
        r.spawn((Group::Location, row())).with_children(|buttons| {
            button(buttons, h, "RulesLocationPrevious", "上一页", Action::Previous);
            button(buttons, h, "RulesLocationNext", "下一页", Action::Next);
        });
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
    let (sheet, selection) = s.list_projection();
    let tab = selection.tab;
    let inline = tab == Tab::Locations;
    let scene = s.scene(tab);
    let rows = sheet.map_or(&[][..], |m| s.rows(m, tab));
    let count = rows.len();
    let start = page(selection, rows) * ROWS;
    let edit_slot = s.sel.item.map(|i| i % ROWS);
    for (entity, part, mut node, mut vis) in &mut parts {
        let shown = match *part {
            TablePart::Slot(None) => inline && s.editing_item && edit_slot.is_none(),
            TablePart::Slot(Some(i)) => start + i < count,
            TablePart::Summary(i) => {
                start + i < count && (!inline || !(s.editing_item && edit_slot == Some(i)))
            }
            TablePart::Heading => !inline && count > 0,
        };
        let is_grid = inline && matches!(part, TablePart::Summary(_));
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
        if inline
            && let TablePart::Slot(slot) = *part
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
        let value = match cell.slot {
            None => label.to_owned(),
            Some(slot) if start + slot < count => {
                let value = cell
                    .column
                    .value(sheet.unwrap(), tab, scene, rows[start + slot]);
                if inline {
                    format!("{label}  {value}")
                } else {
                    value
                }
            }
            Some(_) => String::new(),
        };
        set(&mut caption, &value);
    }
}

pub(super) fn fit(
    s: Res<Studio>,
    scroll: Query<&ComputedNode, With<ScrollArea>>,
    mut tables: Query<&mut Table, With<RulesTable>>,
    mut rows: Query<
        (
            &TablePart,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (Without<ColumnTrack>, Without<EditVisual>),
    >,
    mut cells: Query<
        (&mut Node, Option<&mut TextLayout>),
        (With<ColumnTrack>, Without<EditVisual>),
    >,
    mut edits: Query<(&EditVisual, &mut Node)>,
    added: Query<(), Added<Cell>>,
    mut last: Local<Option<(f32, bool)>>,
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
    let inline = s.sel.tab == Tab::Locations;
    if *last == Some((width, inline)) && added.is_empty() {
        return;
    }
    let mode_changed = last.is_none_or(|(_, previous)| previous != inline) || !added.is_empty();
    *last = Some((width, inline));
    if !inline && !mode_changed {
        return;
    }
    let pair_width = column_width(width);
    if mode_changed {
        for mut table in &mut tables {
            table.set_enabled(!inline);
        }
    }
    // 地点详情仍在卡片内部编辑。切换样式沿用已经绑定的行与表单，不重建操作按钮。
    for (part, mut node, mut fill, mut border) in &mut rows {
        let TablePart::Summary(i) = *part else {
            continue;
        };
        if inline {
            if mode_changed {
                let display = node.display;
                *node = Node {
                    display,
                    ..location_grid()
                };
                *fill = BackgroundColor(c::PANEL_BG);
            }
            node.grid_template_columns = vec![RepeatedGridTrack::px(2, pair_width)];
        } else if mode_changed {
            let (mut layout, color, line, _) = data_table::row(Some(i));
            layout.display = node.display;
            *node = layout;
            *fill = color;
            *border = line;
        }
    }
    for (mut node, layout) in &mut cells {
        if let Some(mut layout) = layout {
            node.width = if inline { px(pair_width) } else { percent(100) };
            layout.linebreak = if inline {
                bevy::text::LineBreak::WordOrCharacter
            } else {
                bevy::text::LineBreak::NoWrap
            };
        } else if inline {
            node.width = px(pair_width);
            node.padding = UiRect::ZERO;
            node.overflow = Overflow::visible();
        }
    }
    if mode_changed {
        for (visual, mut node) in &mut edits {
            match visual {
                EditVisual::Button => {
                    node.width = if inline {
                        percent(100)
                    } else {
                        px(ROW_ACTION_WIDTH)
                    };
                    node.padding = UiRect::horizontal(px(if inline { 8. } else { 0. }));
                }
                EditVisual::Text => show(&mut node, inline),
                EditVisual::Icon => show(&mut node, !inline),
            }
        }
    }
}
