//! 用 UI 渲染器使用的字形四边形验证表格。节点中心对齐时，错误的文本测量仍可能把字画到相邻行。
use super::*;

#[derive(Debug)]
struct Cell {
    text: String,
    bounds: Rect,
    ink: Rect,
}

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
                ((left - bounds.min.x).abs() < 1. && (right - bounds.max.x).abs() < 1.)
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
    let variables = page.as_deref() == Some("工程变量");
    let single_line = matches!(page.as_deref(), Some("工程变量" | "触发规则"));
    if matches!(page.as_deref(), Some("工程变量" | "触发规则"))
        && ["RulesSource", "RulesUndo"]
            .into_iter()
            .any(|name| form::entity(world, name).is_some())
    {
        return Err("工程变量或触发规则显示了高级源码或撤销按钮".into());
    }
    let slots: Vec<usize> = world
        .query::<(&Name, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, node, vis)| vis.get() && node.size().min_element() > 1.)
        .filter_map(|(name, ..)| name.as_str().strip_prefix("RulesRow")?.parse().ok())
        .collect();
    let heading = form::entity(world, "RulesHeadingId");
    if heading.is_some() != (single_line && !slots.is_empty()) {
        return Err("工程列表表头显隐不正确".into());
    }
    if let Some(heading) = heading {
        let column = world.get::<ChildOf>(heading).ok_or("表头没有列")?.parent();
        let parent = world.get::<ChildOf>(column).ok_or("表头没有行")?.parent();
        let heading = ["Id", "Kind", "Value"].map(|field| {
            named(world, &format!("RulesHeading{field}")).and_then(|e| cell(world, e, true))
        });
        let [id, kind, value] = heading;
        let headings = [id?, kind?, value?];
        let expected = if variables {
            ["变量名称", "类型", "值"]
        } else {
            ["规则 ID", "监听", "执行动作"]
        };
        if headings.each_ref().map(|cell| cell.text.as_str()) != expected {
            return Err(format!("工程列表表头不匹配：{headings:?}"));
        }
        check_band(
            &headings,
            layout::bounds(world, parent).ok_or("表头未排版")?,
        )?;
        data["headings"] = json!(record(&headings));
    }
    for slot in slots {
        let row = named(world, &format!("RulesTableRow{slot}"))?;
        let owner = layout::bounds(world, row).ok_or("行尚未排版")?;
        if details.is_some_and(|form| form.intersect(owner).size().min_element() > 1.) {
            return Err(format!(
                "已保存记录 {slot} 与当前编辑表单重叠：{owner:?}，表单 {details:?}"
            ));
        }
        let mut items = Vec::new();
        for field in ["Id", "Kind", "Value"] {
            let e = named(world, &format!("RulesCell{slot}{field}"))?;
            items.push(cell(world, e, single_line)?);
        }
        let edit = named(world, &format!("RulesRow{slot}"))?;
        if single_line {
            let delete = named(world, &format!("RulesDeleteRow{slot}"))?;
            items.push(icon_button(world, edit, "RulesRowEditIcon", "编辑")?);
            items.push(icon_button(world, delete, "RulesRowDeleteIcon", "删除")?);
            check_band(&items, owner)?;
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
            json!({"bounds":layout::coords(owner), "cells":record(&items)});
    }
    Ok(data)
}
