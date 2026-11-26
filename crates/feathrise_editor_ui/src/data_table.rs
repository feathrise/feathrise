//! 通用作者表格。表头和行槽共享列宽，行外的业务状态和按钮由调用者管理。

use crate::{InteractionButton, UiInteraction, UiInteractionSystems, colors};
use bevy::{input::InputSystems, prelude::*, ui::UiSystems, window::PrimaryWindow};

#[derive(Clone, Copy)]
pub enum Column {
    Flex { min: f32, weight: f32 },
    Fixed(f32),
}

impl Column {
    pub const fn flex(min: f32, weight: f32) -> Self {
        Self::Flex { min, weight }
    }

    pub const fn fixed(width: f32) -> Self {
        Self::Fixed(width)
    }
}

#[derive(Component)]
pub struct Table {
    columns: Vec<Column>,
    widths: Vec<f32>,
    measured: f32,
    minimum_scale: f32,
    enabled: bool,
}

impl Table {
    pub fn new(columns: impl Into<Vec<Column>>) -> Self {
        let columns = columns.into();
        Self {
            widths: vec![0.; columns.len()],
            columns,
            measured: 0.,
            minimum_scale: 1.,
            enabled: true,
        }
    }

    /// 允许调用者把同一组原生控件暂时交给其他呈现方式，保留已调好的列宽。
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn fit(&mut self, available: f32) {
        let (mut fixed, mut minimum, mut weight) = (0., 0., 0.);
        for &column in &self.columns {
            match column {
                Column::Fixed(width) => fixed += width,
                Column::Flex { min, weight: grow } => {
                    minimum += min;
                    weight += grow;
                }
            }
        }
        // 正常栏宽保留数据列最小值。极窄布局只收缩数据列，固定操作始终保留图标宽度。
        let flex_space = (available - fixed).max(0.);
        self.minimum_scale = if minimum > flex_space {
            flex_space / minimum
        } else {
            1.
        };
        let spare = (flex_space - minimum).max(0.);
        for (width, &column) in self.widths.iter_mut().zip(&self.columns) {
            *width = match column {
                Column::Fixed(width) => width,
                Column::Flex { min, weight: grow } => {
                    min * self.minimum_scale
                        + if weight > 0. {
                            spare * grow / weight
                        } else {
                            0.
                        }
                }
            };
        }
        self.measured = available;
    }

    fn apply_drag(&mut self, drag: Drag, x: f32) {
        let [
            Column::Flex {
                min: left_min,
                weight: left_weight,
            },
            Column::Flex {
                min: right_min,
                weight: right_weight,
            },
        ] = self.columns[drag.left..drag.left + 2]
        else {
            return;
        };
        let left_min = left_min * self.minimum_scale;
        let right_min = right_min * self.minimum_scale;
        let extra = drag.pair_width - left_min - right_min;
        if extra <= 0. {
            return;
        }
        // 把相邻两列的剩余宽度重新分配，其他列保持位置；比例保留到后续窗口缩放。
        let width =
            (drag.left_width + x - drag.origin).clamp(left_min, drag.pair_width - right_min);
        let weight = left_weight + right_weight;
        let left = (width - left_min) / extra * weight;
        if left != left_weight {
            for (index, grow) in [(drag.left, left), (drag.left + 1, weight - left)] {
                if let Column::Flex { weight, .. } = &mut self.columns[index] {
                    *weight = grow;
                }
            }
            self.fit(self.measured);
        }
    }
}

#[derive(Component)]
struct Cell {
    table: Entity,
    column: usize,
}

#[derive(Component, Clone, Copy)]
struct ResizeHandle {
    table: Entity,
    left: usize,
}

#[derive(Clone, Copy)]
struct Drag {
    handle: Entity,
    table: Entity,
    left: usize,
    origin: f32,
    last_cursor: f32,
    scale: f32,
    window_width: u32,
    measured: f32,
    left_width: f32,
    pair_width: f32,
}

/// 供宿主沿用已有鼠标所有权门，释放该手势时也不能触发视口输入。
#[derive(Resource, Default)]
pub struct ResizeCapture {
    drag: Option<Drag>,
    release: bool,
}

impl ResizeCapture {
    pub fn owns_mouse(&self) -> bool {
        self.drag.is_some() || self.release
    }
}

/// 表头传 `None`，数据行传可见序号。行线与隔行底色使用当前编辑器主题。
pub fn row(ordinal: Option<usize>) -> (Node, BackgroundColor, BorderColor, Pickable) {
    let fill = match ordinal {
        None => colors::PANEL_BG_STRONG,
        Some(index) if index % 2 == 0 => colors::PANEL_BG,
        Some(_) => colors::PANEL_BG_SECTION,
    };
    (
        Node {
            width: percent(100),
            min_height: px(if ordinal.is_some() { 38. } else { 30. }),
            flex_shrink: 0.,
            align_items: AlignItems::Stretch,
            border: UiRect::bottom(px(1)),
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(colors::COPPER_BORDER_WEAK),
        Pickable::IGNORE,
    )
}

/// 列容器负责裁剪文本和动作，文字子节点可按业务需要设置换行策略。
pub fn cell(table: Entity, column: usize) -> impl Bundle {
    (
        Cell { table, column },
        Node {
            min_width: px(0),
            width: px(0),
            flex_shrink: 0.,
            align_items: AlignItems::Center,
            overflow: Overflow::clip_x(),
            ..default()
        },
        Pickable::IGNORE,
    )
}

/// 放入可调整列的表头内，线贴右边，向本列内留出足够的命中面。
pub fn resize_handle(table: Entity, left_column: usize) -> impl Bundle {
    (
        ResizeHandle {
            table,
            left: left_column,
        },
        InteractionButton,
        Node {
            position_type: PositionType::Absolute,
            top: px(0),
            right: px(0),
            bottom: px(0),
            width: px(9),
            border: UiRect::right(px(1)),
            ..default()
        },
        BorderColor::all(colors::COPPER_BORDER_WEAK),
    )
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ResizeCapture>()
        .add_systems(
            PreUpdate,
            resize
                .after(InputSystems)
                .after(UiSystems::Focus)
                .after(UiInteractionSystems::Sync),
        )
        // 业务在 Update 中复用行池或切换面板，表格在实际排版前统一落列宽。
        .add_systems(
            PostUpdate,
            (measure, fit_cells).chain().before(UiSystems::Layout),
        );
}

fn measure(mut tables: Query<(&ComputedNode, &mut Table)>) {
    for (node, mut table) in &mut tables {
        let width = node.content_box().width() * node.inverse_scale_factor();
        // 收起面板时布局归零，重新打开仍使用用户已经调好的分配比例。
        if table.enabled && width > 0. && width != table.measured {
            table.fit(width);
        }
    }
}

fn fit_cells(
    tables: Query<&Table>,
    changed: Query<&Table, Changed<Table>>,
    added: Query<(), Added<Cell>>,
    mut cells: Query<(Ref<Cell>, &mut Node)>,
) {
    if added.is_empty() && changed.is_empty() {
        return;
    }
    for (cell, mut node) in &mut cells {
        let table = if cell.is_added() {
            tables.get(cell.table)
        } else {
            changed.get(cell.table)
        };
        let Ok(table) = table else { continue };
        if !table.enabled {
            continue;
        }
        let Some(&width) = table.widths.get(cell.column) else {
            continue;
        };
        let data = matches!(table.columns[cell.column], Column::Flex { .. });
        let padding = if data {
            UiRect::horizontal(px(6.0_f32.min(width / 2.)))
        } else {
            UiRect::ZERO
        };
        let justify = if data {
            JustifyContent::Start
        } else {
            JustifyContent::Center
        };
        let overflow = Overflow::clip_x();
        if node.width != px(width)
            || node.padding != padding
            || node.justify_content != justify
            || node.overflow != overflow
        {
            node.width = px(width);
            node.padding = padding;
            node.justify_content = justify;
            node.overflow = overflow;
        }
    }
}

fn resize(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    scale: Option<Res<UiScale>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut capture: ResMut<ResizeCapture>,
    mut tables: Query<&mut Table>,
    mut handles: Query<(
        Entity,
        &ResizeHandle,
        &UiInteraction,
        &ComputedNode,
        &InheritedVisibility,
        &mut BorderColor,
    )>,
) {
    if capture.release {
        capture.release = false;
    }
    let ui_scale = scale.as_ref().map_or(1., |scale| scale.0.max(0.1));
    let cursor = window
        .single()
        .ok()
        .filter(|window| window.focused)
        .and_then(|window| {
            let scale = window.scale_factor() * ui_scale;
            let x = window.physical_cursor_position()?.x / scale;
            Some((x, scale, window.resolution.physical_width()))
        });
    let held = mouse
        .as_ref()
        .is_some_and(|mouse| mouse.pressed(MouseButton::Left));
    let released = mouse
        .as_ref()
        .is_some_and(|mouse| mouse.just_released(MouseButton::Left));
    if let Some(drag) = capture.drag {
        // 缩放或窗口改宽时立即收口，避免布局尚未重算就拿新坐标修改旧列宽。
        if let Some((x, scale, width)) = cursor
            && (held || released)
            && scale == drag.scale
            && width == drag.window_width
            && handles
                .get(drag.handle)
                .is_ok_and(|(_, _, _, node, visible, _)| {
                    visible.get() && node.size().min_element() > 0.
                })
            && let Ok(mut table) = tables.get_mut(drag.table)
            && table.enabled
            && table.measured == drag.measured
        {
            if x != drag.last_cursor {
                table.apply_drag(drag, x);
                capture.drag = Some(Drag {
                    last_cursor: x,
                    ..drag
                });
            }
            if released {
                capture.drag = None;
                capture.release = true;
            }
        } else {
            capture.drag = None;
            capture.release = true;
        }
    } else if let Some((x, scale, window_width)) = cursor
        && held
        && mouse.as_ref().is_some_and(|mouse| {
            mouse.just_pressed(MouseButton::Left)
                && ![MouseButton::Right, MouseButton::Middle]
                    .into_iter()
                    .any(|button| {
                        mouse.pressed(button)
                            || mouse.just_pressed(button)
                            || mouse.just_released(button)
                    }) // 数值右键拖动、视口中键拖动及其释放帧保持原有鼠标所有权。
        })
    {
        for (id, handle, interaction, node, visible, _) in &mut handles {
            if *interaction == UiInteraction::None
                || !visible.get()
                || node.size().min_element() <= 0.
            {
                continue;
            }
            let Ok(table) = tables.get(handle.table) else {
                continue;
            };
            if !table.enabled {
                continue;
            }
            let Some([Column::Flex { .. }, Column::Flex { .. }]) =
                table.columns.get(handle.left..=handle.left + 1)
            else {
                continue;
            };
            let left_width = table.widths[handle.left];
            capture.drag = Some(Drag {
                handle: id,
                table: handle.table,
                left: handle.left,
                origin: x,
                last_cursor: x,
                scale,
                window_width,
                measured: table.measured,
                left_width,
                pair_width: left_width + table.widths[handle.left + 1],
            });
            break;
        }
    }
    for (id, _, interaction, _, _, mut border) in &mut handles {
        let color = if *interaction != UiInteraction::None
            || capture.drag.is_some_and(|drag| drag.handle == id)
        {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        };
        border.set_if_neq(BorderColor::all(color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_spares_other_columns_and_retains_widths_after_narrowing() {
        let mut table = Table::new([
            Column::flex(32., 2.),
            Column::flex(44., 1.),
            Column::flex(32., 1.),
            Column::fixed(72.),
        ]);
        table.fit(260.);
        assert_eq!(table.widths, [72., 64., 52., 72.]);
        table.apply_drag(
            Drag {
                handle: Entity::PLACEHOLDER,
                table: Entity::PLACEHOLDER,
                left: 0,
                origin: 100.,
                last_cursor: 100.,
                scale: 1.,
                window_width: 1600,
                measured: 260.,
                left_width: 72.,
                pair_width: 136.,
            },
            125.,
        );
        assert_eq!(table.widths, [92., 44., 52., 72.]);
        table.fit(120.);
        assert_eq!(table.widths[3], 72.);
        assert!((table.widths.iter().sum::<f32>() - 120.).abs() < 0.01);
        table.fit(260.);
        assert_eq!(table.widths, [92., 44., 52., 72.]);
    }
}
