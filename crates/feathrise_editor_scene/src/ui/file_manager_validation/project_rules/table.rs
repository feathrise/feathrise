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
fn cell(world: &World, id: Entity) -> Result<Cell, String> {
    let label = if world.get::<Text>(id).is_some() {
        id
    } else {
        world
            .get::<Children>(id)
            .and_then(|children| children.iter().find(|&e| world.get::<Text>(e).is_some()))
            .ok_or("表格控件缺少文字")?
    };
    Ok(Cell {
        text: world.get::<Text>(label).unwrap().as_str().to_owned(),
        bounds: layout::bounds(world, id).ok_or("表格控件尚未排版")?,
        ink: layout::glyph_bounds(world, label).ok_or("尚未生成表格字形")?,
    })
}

fn named(world: &mut World, name: &str) -> Result<Entity, String> {
    form::entity(world, name).ok_or_else(|| format!("尚未显示 {name}"))
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
    if form::entity(world, "RulesHeadingId").is_some() {
        return Err("右栏记录仍显示横排表头".into());
    }
    let slots: Vec<usize> = world
        .query::<(&Name, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, node, vis)| vis.get() && node.size().min_element() > 1.)
        .filter_map(|(name, ..)| name.as_str().strip_prefix("RulesRow")?.parse().ok())
        .collect();
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
            items.push(cell(world, e)?);
        }
        let edit = named(world, &format!("RulesRow{slot}"))?;
        items.push(cell(world, edit)?);
        check_band(&items[..2], owner)?;
        check_band(&items[2..], owner)?;
        if items[..2].iter().any(|upper| {
            items[2..]
                .iter()
                .any(|lower| upper.bounds.max.y > lower.bounds.min.y + 1.)
        }) {
            return Err(format!("右栏记录的上下两行重叠：{items:?}"));
        }
        data["rows"][slot.to_string()] =
            json!({"bounds":layout::coords(owner), "cells":record(&items)});
    }
    Ok(data)
}
