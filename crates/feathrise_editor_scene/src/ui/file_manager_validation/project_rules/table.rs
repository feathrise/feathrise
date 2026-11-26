//! 用 UI 渲染器使用的字形四边形验证表格。节点中心对齐时，错误的文本测量仍可能把字画到相邻行。
use super::*;
use feathrise_editor_ui::UiInteraction;

#[derive(Debug)]
struct Cell {
    text: String,
    bounds: Rect,
    ink: Rect,
}

const FIELDS: [&str; 3] = ["Id", "Kind", "Value"];
const HEADINGS: [&str; 4] = ["Id", "Kind", "Value", "Actions"];

fn contains(outer: Rect, inner: Rect) -> bool {
    let outer = outer.inflate(1.);
    outer.contains(inner.min) && outer.contains(inner.max)
}

/// 按钮的字在直接子实体，表头和记录字段的字在控件本身。
fn cell(world: &World, id: Entity, clip: bool) -> Result<Cell, String> {
    let label = if world.get::<Text>(id).is_some() {
        id
    } else {
        world
            .get::<Children>(id)
            .and_then(|children| children.iter().find(|&e| world.get::<Text>(e).is_some()))
            .ok_or("表格控件缺少文字")?
    };
    let bounds = layout::bounds(world, id).ok_or("表格控件尚未排版")?;
    let mut ink = layout::glyph_bounds(world, label).ok_or("尚未生成表格字形")?;
    if clip {
        let track = parent_bounds(world, label)?;
        if world
            .get::<TextLayout>(label)
            .is_none_or(|text| text.linebreak != bevy::text::LineBreak::NoWrap)
        {
            return Err("变量记录可以自动换行".into());
        }
        let Some(bevy::ui::CalculatedClip::Rects(clips)) = world.get(label) else {
            return Err("单行记录没有继承列裁剪".into());
        };
        // 读取渲染器实际使用的列裁剪。外层滚动裁剪会随列表位置变化。
        let clip = clips
            .iter()
            .find_map(|clip| {
                let to_world = clip.world_to_clip_local.inverse();
                let left = to_world.transform_point2(Vec2::new(clip.rect.min.x, 0.)).x;
                let right = to_world.transform_point2(Vec2::new(clip.rect.max.x, 0.)).x;
                (left >= track.min.x - 1. && right <= track.max.x + 1. && right > left)
                    .then_some((left, right))
            })
            .ok_or("字形裁剪与列宽不匹配")?;
        ink.min.x = ink.min.x.max(clip.0);
        ink.max.x = ink.max.x.min(clip.1);
    }
    Ok(Cell {
        text: world.get::<Text>(label).unwrap().as_str().to_owned(),
        bounds,
        ink,
    })
}

fn named(world: &mut World, name: &str) -> Result<Entity, String> {
    form::entity(world, name).ok_or_else(|| format!("尚未显示 {name}"))
}

fn parent_bounds(world: &World, entity: Entity) -> Result<Rect, String> {
    world
        .get::<ChildOf>(entity)
        .and_then(|parent| layout::bounds(world, parent.parent()))
        .ok_or_else(|| "表格列尚未排版".into())
}

fn track_matches(a: Rect, b: Rect) -> bool {
    (a.min.x - b.min.x).abs() <= 1. && (a.max.x - b.max.x).abs() <= 1.
}

fn heading_tracks(world: &mut World) -> Result<[Rect; 4], String> {
    let [id, kind, value, actions] = HEADINGS.map(|field| {
        named(world, &format!("RulesHeading{field}")).and_then(|entity| parent_bounds(world, entity))
    });
    Ok([id?, kind?, value?, actions?])
}

fn icon_button(world: &World, button: Entity, icon_name: &str, label: &str) -> Result<Cell, String> {
    let children = world.get::<Children>(button).ok_or("行尾按钮没有图标")?;
    if children.iter().any(|e| {
        world.get::<Text>(e).is_some()
            && world.get::<Node>(e).is_some_and(|node| node.display != Display::None)
    }) {
        return Err(format!("行尾{label}按钮仍显示文字"));
    }
    let icon = children.iter().find(|&e| {
        world.get::<Name>(e).is_some_and(|name| name.as_str() == icon_name)
            && world.get::<ImageNode>(e).is_some_and(|image| {
                world.resource::<Assets<Image>>().get(&image.image).is_some()
            })
    }).ok_or_else(|| format!("行尾{label}图标尚未载入"))?;
    let ink = layout::bounds(world, icon).ok_or("行尾图标尚未排版")?;
    if ink.size().min_element() <= 1. {
        return Err(format!("行尾{label}图标不可见"));
    }
    Ok(Cell {
        text: label.into(),
        bounds: layout::bounds(world, button).ok_or("行尾按钮未排版")?,
        ink,
    })
}

fn check_band(items: &[Cell], owner: Rect) -> Result<(), String> {
    // 以各列实际字形带判断是否同一行，容许汉字、拉丁字母、数字的上下缘天然不同。
    let (top, bottom) = items
        .iter()
        .fold((f32::NEG_INFINITY, f32::INFINITY), |(top, bottom), c| {
            (top.max(c.ink.min.y), bottom.min(c.ink.max.y))
        });
    if items.iter().any(|c| {
        !contains(owner, c.bounds)
            || !contains(owner, c.ink)
            || !(top - 1. ..=bottom + 1.).contains(&c.ink.center().y)
    }) || items.windows(2).any(|pair| {
        pair[0].bounds.max.x > pair[1].bounds.min.x + 1.
            || pair[0].ink.max.x > pair[1].ink.min.x + 1.
    }) {
        return Err(format!(
            "表格文字或控件越过本行、列互相覆盖或文字未对齐：行 {owner:?}，列 {items:?}"
        ));
    }
    Ok(())
}

fn record(items: &[Cell]) -> Vec<Value> {
    items.iter().map(|c| json!({"text":c.text,"bounds":layout::coords(c.bounds),"glyphs":layout::coords(c.ink)})).collect()
}

pub(super) fn measure(world: &mut World, details: Option<Rect>) -> Result<Value, String> {
    let mut data = json!({"rows":{}});
    let page = layout::caption(world, "RulesPageTitle");
    let variables = matches!(page.as_deref(), Some("工程变量" | "场景变量"));
    let single_line = variables || matches!(page.as_deref(), Some("触发规则" | "场景触发"));
    if single_line
        && ["RulesSource", "RulesUndo"]
            .into_iter()
            .any(|name| form::entity(world, name).is_some())
    {
        return Err("工程变量或触发规则显示了高级源码或撤销按钮".into());
    }
    let mut slots: Vec<usize> = world
        .query::<(&Name, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, node, vis)| vis.get() && node.size().min_element() > 1.)
        .filter_map(|(name, ..)| name.as_str().strip_prefix("RulesRow")?.parse().ok())
        .collect();
    slots.sort_unstable();
    let heading = form::entity(world, "RulesHeadingId");
    if heading.is_some() != (single_line && !slots.is_empty()) {
        return Err("工程列表表头显隐不正确".into());
    }
    let mut tracks = Vec::new();
    if heading.is_some() {
        let parent = named(world, "RulesHeading")?;
        let header = layout::bounds(world, parent).ok_or("表头未排版")?;
        let mut headings = Vec::new();
        for field in HEADINGS {
            let entity = named(world, &format!("RulesHeading{field}"))?;
            let heading = cell(world, entity, true)?;
            let track = parent_bounds(world, entity)?;
            if field == "Actions"
                && !contains(track, layout::glyph_bounds(world, entity).ok_or("操作列标题尚未排版")?)
            {
                return Err("固定末列未完整显示「操作列」标题".into());
            }
            headings.push(heading);
            tracks.push(track);
        }
        let expected = if variables {
            ["变量名称", "类型", "值", "操作列"]
        } else {
            ["规则 ID", "监听", "执行动作", "操作列"]
        };
        if headings.iter().map(|cell| cell.text.as_str()).ne(expected) {
            return Err(format!("工程列表表头不匹配：{headings:?}"));
        }
        check_band(&headings, header)?;
        for index in 0..2 {
            let handle = named(world, &format!("RulesResize{index}"))?;
            let edge = layout::bounds(world, handle).ok_or("表头拖动把手尚未排版")?;
            if !contains(header, edge) || (edge.max.x - tracks[index].max.x).abs() > 1. {
                return Err(format!("第 {index} 个拖动把手未贴合相邻列的边界"));
            }
        }
        if form::entity(world, "RulesResize2").is_some() {
            return Err("固定操作列不应具有可拖动的分界".into());
        }
        data["headings"] = json!(record(&headings));
        data["tracks"] = json!(tracks.iter().copied().map(layout::coords).collect::<Vec<_>>());
    }
    let mut previous_fill = None;
    let mut last_row = None;
    for slot in slots {
        let part = if single_line { "RulesTableBodyRow" } else { "RulesTableRow" };
        let row = named(world, &format!("{part}{slot}"))?;
        let owner = layout::bounds(world, row).ok_or("行尚未排版")?;
        if details.is_some_and(|form| form.intersect(owner).size().min_element() > 1.) {
            return Err(format!(
                "已保存记录 {slot} 与当前编辑表单重叠：{owner:?}，表单 {details:?}"
            ));
        }
        let mut items = Vec::new();
        for (index, field) in FIELDS.into_iter().enumerate() {
            let e = named(world, &format!("RulesCell{slot}{field}"))?;
            if single_line && !track_matches(parent_bounds(world, e)?, tracks[index]) {
                return Err(format!("第 {slot} 行的 {field} 列没有与表头对齐"));
            }
            items.push(cell(world, e, single_line)?);
        }
        let edit = named(world, &format!("RulesRow{slot}"))?;
        let mut style = Value::Null;
        if single_line {
            let delete = named(world, &format!("RulesDeleteRow{slot}"))?;
            let edit = icon_button(world, edit, "RulesRowEditIcon", "编辑")?;
            let delete = icon_button(world, delete, "RulesRowDeleteIcon", "删除")?;
            let buttons = edit.bounds.union(delete.bounds);
            if !track_matches(buttons, tracks[3]) {
                return Err(format!("操作列与第 {slot} 行编辑和删除图标组的宽度不符"));
            }
            items.extend([edit, delete]);
            check_band(&items, owner)?;
            let fill = world.get::<BackgroundColor>(row).ok_or("数据行缺少底色")?.0;
            let line = world.get::<BorderColor>(row).ok_or("数据行缺少分隔线颜色")?.bottom;
            let bottom = world.get::<ComputedNode>(row).ok_or("数据行尚未排版")?.border().max_inset.y;
            if fill.to_srgba().alpha <= 0. || previous_fill == Some(fill) {
                return Err(format!("第 {slot} 行的隔行底色未与前一行区分"));
            }
            if bottom <= 0. || line.to_srgba().alpha <= 0. || line == fill {
                return Err(format!("第 {slot} 行缺少实际可见的行分隔线"));
            }
            previous_fill = Some(fill);
            last_row = Some(owner);
            let rgba = fill.to_srgba();
            style = json!({"background":[rgba.red,rgba.green,rgba.blue,rgba.alpha],"bottom_border":bottom,
                "operations":layout::coords(buttons)});
        } else {
            items.push(cell(world, edit, false)?);
            check_band(&items[..2], owner)?;
            check_band(&items[2..], owner)?;
            if items[..2].iter().any(|upper| {
                items[2..]
                    .iter()
                    .any(|lower| upper.bounds.max.y > lower.bounds.min.y + 1.)
            }) {
                return Err(format!("右栏记录的上下两行重叠：{items:?}"));
            }
        }
        data["rows"][slot.to_string()] =
            json!({"bounds":layout::coords(owner), "cells":record(&items),"style":style});
    }
    if let Some(last_row) = last_row {
        let footer = named(world, "RulesPagination")?;
        let count = named(world, "RulesSelection")?;
        let extent = layout::bounds(world, count).ok_or("表格总数尚未排版")?;
        if !directory::ancestor(world, count, footer)
            || extent.min.y + 1. < last_row.max.y
            || extent.height() <= 1.
        {
            return Err("记录数应始终位于表格最后一行下方的页脚".into());
        }
        data["footer"] = json!({"count":layout::coords(extent)});
    }
    Ok(data)
}

pub(super) struct ColumnDrag {
    tracks: [Rect; 4],
    table: Rect,
    center: Option<Value>,
    target: Vec2,
    direction: f32,
}

pub(super) fn resize(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    separator: usize,
    sample: &str,
) -> Result<bool, String> {
    let tracks = match heading_tracks(world) {
        Ok(tracks) => tracks,
        Err(reason) => {
            run.layout_wait = reason;
            return Ok(false);
        }
    };
    let root = named(world, "RulesDataTable")?;
    let table = layout::bounds(world, root).ok_or("通用表格尚未排版")?;
    if run.column_drag.is_none() {
        let grip = named(world, &format!("RulesResize{separator}"))?;
        if !layout::reveal(world, run, window, grip) {
            return Ok(false);
        }
        let origin = layout::bounds(world, grip).ok_or("拖动把手尚未排版")?.center();
        run.wheel_point = Some(origin);
        move_cursor(world, window, origin.as_dvec2());
        if world.get::<UiInteraction>(grip) != Some(&UiInteraction::Hovered) {
            run.layout_wait = format!("等待鼠标进入表头分隔线 {separator}，当前位置 {origin:?}");
            return Ok(false);
        }
        let scale = world.get::<Window>(window).ok_or("缺少验收窗口")?.scale_factor()
            * world.resource::<UiScale>().0;
        // 从默认分配较宽的名称或值列转出空间，避开相邻列到达既有最小宽度的情形。
        let direction = if separator == 0 { -1. } else { 1. };
        run.column_drag = Some(ColumnDrag {
            tracks,
            table,
            center: central(world),
            target: origin + Vec2::X * (direction * 24. * scale),
            direction,
        });
        super::super::mouse(world, window, ButtonState::Pressed);
        return Ok(false);
    }
    let before = run.column_drag.as_ref().unwrap();
    if central(world) != before.center
        || !track_matches(table, before.table)
        || (table.min.y - before.table.min.y).abs() > 1.
        || (table.max.y - before.table.max.y).abs() > 1.
    {
        return Err("拖动表头分隔线改变了中央页面或表格整体边界".into());
    }
    if run.keys == 0 {
        // 与现有检查器分隔条相同，按住左键后用真实窗口光标事件跨到新的列边界。
        run.wheel_point = Some(before.target);
        move_cursor(world, window, before.target.as_dvec2());
        run.keys = 1;
        return Ok(false);
    }
    let delta = tracks[separator].width() - before.tracks[separator].width();
    if delta * before.direction <= 2. {
        run.layout_wait = format!("表头分隔线 {separator} 尚未改变列宽，原 {:?}，现 {tracks:?}", before.tracks);
        return Ok(false);
    }
    if run.keys == 1 {
        super::super::mouse(world, window, ButtonState::Released);
        run.keys = 2;
        return Ok(false);
    }
    let adjacent = separator + 1;
    if (tracks[adjacent].width() - before.tracks[adjacent].width() + delta).abs() > 2.
        || tracks.iter().zip(before.tracks).enumerate().any(|(index, (&after, before))| {
            index != separator && index != adjacent && !track_matches(after, before)
        })
    {
        return Err("表头拖动未在相邻数据列内分配宽度，或移动了固定操作列".into());
    }
    let geometry = match measure(world, None) {
        Ok(data) => data,
        Err(reason) => {
            run.layout_wait = reason;
            return Ok(false);
        }
    };
    let path = run.work.join(format!("rules-{sample}.png"));
    let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
        return Ok(false);
    };
    let before = run.column_drag.take().unwrap();
    run.layout.insert(sample.to_owned(), json!({
        "separator":separator, "before":before.tracks.map(layout::coords),
        "after":tracks.map(layout::coords), "operations_width":tracks[3].width(),
        "delta":delta, "geometry":geometry, "center":before.center, "screenshot":shot,
    }));
    Ok(true)
}
