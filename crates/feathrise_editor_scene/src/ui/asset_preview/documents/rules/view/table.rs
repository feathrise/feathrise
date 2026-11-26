//! 三个工具共用一套有界行池；选择行只移动唯一的原生表单，重排不会丢失输入法或草稿。
use super::{
    layout::{ScrollArea, button, button_with_icon, column, editor, font, info, row},
    *,
};
use feathrise_editor_documents_model::project_rules::{ActionForm, TriggerForm};

const CELL_PAD_X: f32 = 8.;
const CELL_GAP: f32 = 10.;
const COMPACT_CELL_GAP: f32 = 6.;
const ACTION_GAP: f32 = 4.;
pub(super) const ROW_ACTION_WIDTH: f32 = 34.;

#[cfg(test)]
#[path = "table_geometry.rs"]
mod geometry;

#[derive(Component)]
pub(super) struct FormHost;

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
    fn value(self, sheet: &Sheet, tab: Tab, index: usize) -> String {
        if matches!(self, Self::Id) {
            return sheet.id(tab, index).unwrap_or_default().to_owned();
        }
        match tab {
            Tab::Variables => {
                let v = &sheet.model.variables[index];
                match self {
                    Self::Kind => VariableKind::of(v).label().into(),
                    // 默认值沿用配置中的字面值，布尔在列表和表单都显示 true / false。
                    _ if v.default.is_empty() => "空文本".into(),
                    _ => v.default.replace(['\r', '\n'], " "),
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
    slot: Option<usize>,
    column: Column,
}

#[derive(Component)]
pub(super) struct ColumnTrack(Column);

fn cells(p: &mut ChildSpawnerCommands, h: &Handle<Font>, slot: Option<usize>) {
    for column in [Column::Id, Column::Kind, Column::Value] {
        // Bevy 的裁剪从父节点生效；独立的列容器阻止长字符串画到邻列。
        let node = Node { min_width: px(0), ..default() };
        p.spawn((ColumnTrack(column), node.clone(), Pickable::IGNORE))
            .with_child((
                Cell { slot, column },
                ColumnTrack(column),
                Name::new(match slot {
                    Some(slot) => format!("RulesCell{slot}{column:?}"),
                    None => format!("RulesHeading{column:?}"),
                }),
                Text::default(),
                TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                font(h, metrics::FONT_CONTROL),
                TextColor(if slot.is_some() { c::TEXT_PRIMARY } else { c::TEXT_MUTED }),
                node,
                Pickable::IGNORE,
            ));
    }
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
fn row_actions(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    slot: usize,
    edit_icon: &Handle<Image>,
    delete_icon: &Handle<Image>,
) {
    p.spawn(Node {
        flex_wrap: FlexWrap::NoWrap,
        column_gap: px(ACTION_GAP),
        ..row()
    })
    .with_children(|r| {
        button_with_icon(r, h, format!("RulesRow{slot}"), "编辑", Action::Row(slot), Some(edit_icon));
        button_with_icon(r, h, format!("RulesDeleteRow{slot}"), "删除", Action::DeleteRow(slot), Some(delete_icon));
    });
}
pub(super) fn list(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    edit_icon: &Handle<Image>,
    delete_icon: &Handle<Image>,
) {
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
    p.spawn((TablePart::Heading, grid()))
        .with_children(|r| cells(r, h, None));
    for i in 0..ROWS {
        p.spawn((
            TablePart::Slot(Some(i)),
            Name::new(format!("RulesTableRow{i}")),
            column(0.),
        ))
        .with_children(|r| {
            r.spawn((TablePart::Summary(i), grid(), BackgroundColor(c::PANEL_BG)))
                .with_children(|s| {
                    cells(s, h, Some(i));
                    row_actions(s, h, i, edit_icon, delete_icon);
                });
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
    let (sheet, selection) = s.list_projection();
    let tab = selection.tab;
    let inline = tab == Tab::Locations;
    let count = sheet.map_or(0, |m| m.len(tab));
    let start = selection.item.unwrap_or(0) / ROWS * ROWS;
    let edit_slot = s.sel.item.map(|i| i % ROWS);
    for (entity, part, mut node, mut vis) in &mut parts {
        let shown = match *part {
            TablePart::Slot(None) => inline && s.editing_item && edit_slot.is_none(),
            TablePart::Slot(Some(i)) => start + i < count,
            TablePart::Summary(i) => start + i < count
                && (!inline || !(s.editing_item && edit_slot == Some(i))),
            TablePart::Heading => !inline && count > 0,
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
                let value = cell.column.value(sheet.unwrap(), tab, start + slot);
                if inline { format!("{label}  {value}") } else { value }
            }
            Some(_) => String::new(),
        };
        set(&mut caption, &value);
    }
}

pub(super) fn fit(
    s: Res<Studio>,
    scroll: Query<&ComputedNode, With<ScrollArea>>,
    mut rows: Query<(&TablePart, &mut Node), (Without<ColumnTrack>, Without<EditVisual>)>,
    mut cells: Query<(&ColumnTrack, &mut Node, Option<&mut TextLayout>), Without<EditVisual>>,
    mut edits: Query<(&EditVisual, &mut Node)>,
    added: Query<(), Added<Cell>>,
    mut last: Local<Option<(f32, Tab)>>,
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
    if *last == Some((width, s.sel.tab)) && added.is_empty() {
        return;
    }
    *last = Some((width, s.sel.tab));
    let compact = s.sel.tab != Tab::Locations;
    // 变量和规则共享单行轨道，固定给两个操作图标留位置。窄栏也要放下 number。
    let actions_width = 2. * ROW_ACTION_WIDTH + ACTION_GAP;
    let available = (width - 2. * CELL_PAD_X - 3. * COMPACT_CELL_GAP - actions_width).max(0.);
    let (kind_width, value_width) = if s.sel.tab == Tab::Variables {
        let kind = 44_f32.min(available);
        (kind, (available - kind) * 0.48)
    } else {
        let value = (available * 0.38).max(48.).min(available);
        ((available - value) * 0.46, value)
    };
    let widths = [available - kind_width - value_width, kind_width, value_width, actions_width];
    let compact_tracks: Vec<_> = widths
        .into_iter()
        .map(|width| RepeatedGridTrack::px(1, width))
        .collect();
    let pair_width = column_width(width);
    for (part, mut row) in &mut rows {
        if !matches!(part, TablePart::Slot(_)) {
            row.column_gap = px(if compact { COMPACT_CELL_GAP } else { CELL_GAP });
            row.grid_template_columns = if compact {
                compact_tracks.clone()
            } else {
                vec![RepeatedGridTrack::px(2, pair_width)]
            };
        }
    }
    for (track, mut node, layout) in &mut cells {
        node.width = px(if compact { widths[track.0 as usize] } else { pair_width });
        if let Some(mut layout) = layout {
            layout.linebreak = if compact {
                bevy::text::LineBreak::NoWrap
            } else {
                bevy::text::LineBreak::WordOrCharacter
            };
        } else {
            node.overflow = if compact { Overflow::clip_x() } else { Overflow::visible() };
        }
    }
    for (visual, mut node) in &mut edits {
        match visual {
            EditVisual::Button => node.width = if compact { px(ROW_ACTION_WIDTH) } else { percent(100) },
            EditVisual::Text => show(&mut node, !compact),
            EditVisual::Icon => show(&mut node, compact),
        }
    }
}
