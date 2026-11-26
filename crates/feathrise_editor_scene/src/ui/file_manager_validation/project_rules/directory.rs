//! 从真实交互目录切页，用中文布局和 GPU 回读核对场景工具隔离与工程入口。
use super::*;
use crate::{
    components::{SceneEditorPane, SceneEditorPaneSplitter},
    types::EditorPane,
};
use bevy::text::TextLayoutInfo;

const ROWS: &[&[&str]] = &[
    &["传送点", "空气墙", "动画触发", "地点区域"],
    &["工程变量", "触发规则"],
];

pub(super) fn ancestor(world: &World, mut child: Entity, parent: Entity) -> bool {
    while child != parent {
        let Some(relation) = world.get::<ChildOf>(child) else {
            return false;
        };
        child = relation.parent();
    }
    true
}

pub(super) fn measure(world: &mut World) -> Result<Value, String> {
    let nav = form::entity(world, "交互工具目录").ok_or("尚未显示交互工具目录")?;
    let nav_rect = layout::bounds(world, nav).ok_or("尚未排版目录")?;
    if world
        .query::<&Name>()
        .iter(world)
        .filter(|name| name.as_str() == "交互工具目录")
        .count()
        != 1
    {
        return Err("场景和工程工具应共用右栏中唯一的目录实体".into());
    }
    let entries: Vec<_> = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .filter_map(|(entity, name)| {
            name.as_str()
                .strip_prefix("交互目录条目")
                .map(|label| (entity, label.to_owned()))
        })
        .filter(|(entity, _)| ancestor(world, *entity, nav))
        .map(|(_, label)| label)
        .collect();
    if entries.len() != ROWS.iter().map(|labels| labels.len()).sum::<usize>()
        || entries.iter().any(|entry| entry == "资产摆放")
    {
        return Err(format!("交互目录应仅有六个工具，实际为 {entries:?}"));
    }
    if world
        .query::<(Entity, &Name)>()
        .iter(world)
        .any(|(e, name)| {
            matches!(
                name.as_str(),
                "交互目录场景分组" | "交互目录工程分组" | "工程规则模板菜单" | "从示例创建"
            ) && ancestor(world, e, nav)
        })
        || world
            .query::<(Entity, &Text)>()
            .iter(world)
            .any(|(e, caption)| {
                matches!(caption.as_str(), "场景" | "工程" | "···") && ancestor(world, e, nav)
            })
    {
        return Err("交互目录仍包含分组标题或示例菜单".into());
    }
    let owner = visible(world, |p: &SceneEditorPane| p.0 == EditorPane::Inspector)
        .ok_or("点击工具后右栏消失")?;
    let body = form::entity(world, "交互当前工具内容").ok_or("尚未显示当前工具正文")?;
    if !ancestor(world, nav, owner)
        || !ancestor(world, body, owner)
        || visible(world, |p: &SceneEditorPaneSplitter| {
            p.0 == EditorPane::Inspector
        })
        .is_none()
    {
        return Err("切换工具后，目录、正文或拖宽窄用的分隔条离开了右栏".into());
    }
    if let Some(sheet) = form::entity(world, "RulesWorkbench") {
        let host = form::entity(world, "交互工具表单").ok_or("工程表单缺少右栏挂载点")?;
        if !ancestor(world, sheet, host) || !ancestor(world, host, body) {
            return Err("工程表单没有挂载到右栏当前工具正文".into());
        }
    }
    let owner_bounds = layout::bounds(world, owner).ok_or("尚未排版交互工作区")?;
    let body = layout::bounds(world, body).ok_or("尚未排版工具正文")?;
    if [nav_rect, body].iter().any(|rect| {
        !owner_bounds.inflate(1.).contains(rect.min) || !owner_bounds.inflate(1.).contains(rect.max)
    }) {
        return Err(format!(
            "交互目录或正文越出工作区：{nav_rect:?}，{body:?}，工作区 {owner_bounds:?}"
        ));
    }
    if nav_rect.max.y > body.min.y + 1. {
        return Err(format!(
            "六个固定入口应保留在正文上方，目录 {nav_rect:?}，正文 {body:?}"
        ));
    }
    let mut rows = Vec::new();
    let mut first_row: Option<Vec<Rect>> = None;
    for &labels in ROWS {
        let mut records = Vec::new();
        let mut buttons = Vec::<Rect>::new();
        for &label in labels {
            let name = format!("交互目录条目{label}");
            let id = form::entity(world, &name).ok_or_else(|| format!("缺少{name}"))?;
            let rect = layout::bounds(world, id).ok_or_else(|| format!("尚未排版{name}"))?;
            if !ancestor(world, id, nav)
                || !nav_rect.inflate(1.).contains(rect.min)
                || !nav_rect.inflate(1.).contains(rect.max)
                || buttons
                    .iter()
                    .any(|other| other.intersect(rect).size().min_element() > 1.)
            {
                return Err(format!(
                    "目录边界或按钮重叠：{name} {rect:?}，目录 {nav_rect:?}"
                ));
            }
            // “不竖排”由中文实际排版的行号、字形数与按钮边界验证，不只检查组件上声明的宽度。
            let (text_id, _, shaped, text_bounds) = world
                .query::<(Entity, &Text, &TextLayoutInfo, &ComputedNode)>()
                .iter(world)
                .find(|(e, value, _, node)| {
                    value.as_str() == label && node.size().x > 1. && ancestor(world, *e, id)
                })
                .ok_or_else(|| format!("{label}尚未显示中文按钮文字"))?;
            let ink =
                layout::glyph_bounds(world, text_id).ok_or_else(|| format!("{label}尚未塑形"))?;
            if !rect.inflate(1.).contains(ink.min)
                || !rect.inflate(1.).contains(ink.max)
                || shaped.glyphs.len() < label.chars().count()
                || shaped.glyphs.iter().any(|g| g.line_index != 0)
                || shaped.size.x > rect.width() + 1.
                || text_bounds.size().x > rect.width() + 1.
            {
                return Err(format!(
                    "{label}文字超出按钮或换为多行，文字 {:?}，按钮 {rect:?}",
                    shaped.size
                ));
            }
            buttons.push(rect);
            records.push(
                json!({"label":label,"bounds":layout::coords(rect),"glyphs":layout::coords(ink)}),
            );
        }
        // 两行共用同一四列栅格，第二行只占前两列；拖动侧栏不会改变按钮的行列。
        let first = buttons[0];
        if buttons.iter().any(|rect| {
            (rect.min.y - first.min.y).abs() > 1.
                || (rect.max.y - first.max.y).abs() > 1.
                || (rect.width() - first.width()).abs() > 1.
        }) || buttons
            .windows(2)
            .any(|pair| pair[0].max.x > pair[1].min.x + 1.)
            || first_row.as_ref().is_some_and(|top| {
                top[0].max.y > first.min.y + 1.
                    || buttons.iter().zip(top).any(|(button, column)| {
                        (button.min.x - column.min.x).abs() > 1.
                            || (button.max.x - column.max.x).abs() > 1.
                    })
            })
        {
            return Err(format!("交互目录应固定首行四项、次行两项：{buttons:?}"));
        }
        if first_row.is_none() {
            first_row = Some(buttons);
        }
        rows.push(records);
    }
    Ok(
        json!({"entity":nav.to_bits(),"workspace":layout::coords(owner_bounds),
        "bounds":layout::coords(nav_rect),"content":layout::coords(body),"placement":"上方",
        "row_counts":[4,2],"rows":rows}),
    )
}

/// 跨场景、工程表单和再次返回后，比较实窗里实际挂载的目录和恢复的右栏尺寸。
pub(super) fn same_directory(run: &Run, current: &Value) -> Result<(), String> {
    if let Some(first) = run.layout.get("catalog-portal-default")
        && current["entity"] != first["entity"]
    {
        return Err("切页后常驻目录实体被替换".into());
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SceneTool {
    Portal,
    Boundary,
    MotionTrigger,
}
impl SceneTool {
    const ALL: [Self; 3] = [Self::Portal, Self::Boundary, Self::MotionTrigger];

    fn marker(self) -> &'static str {
        match self {
            Self::Portal => "1 · 当前源点（已有）",
            Self::Boundary => "1 · 空气墙对象",
            Self::MotionTrigger => "动画触发",
        }
    }
}

fn scene_labels(
    world: &mut World,
    run: &mut Run,
    tool: SceneTool,
) -> Result<Option<Vec<String>>, String> {
    let Some(body) = form::entity(world, "交互当前工具内容") else {
        return Ok(None);
    };
    if form::entity(world, "RulesWorkbench").is_some() {
        run.layout_wait = "场景工具打开时工程表单仍可见".into();
        return Ok(None);
    }
    let labels: Vec<_> = world
        .query::<(Entity, &Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(e, _, node, vis)| {
            vis.get() && node.size().min_element() > 1. && ancestor(world, *e, body)
        })
        .map(|(_, text, ..)| text.as_str().to_owned())
        .collect();
    if !labels.iter().any(|label| label == tool.marker()) {
        run.layout_wait = format!("等待场景正文显示{}，实窗文字为 {labels:?}", tool.marker());
        return Ok(None);
    }
    // 目录可以同时显示入口名称，正文只能显示这一项工具的可见控件。
    if SceneTool::ALL
        .iter()
        .any(|&other| other != tool && labels.iter().any(|label| label == other.marker()))
        || labels.iter().any(|label| {
            matches!(
                label.as_str(),
                "资产摆放"
                    | "待摆放资产"
                    | "选择资产"
                    | "更换资产"
                    | "在视口摆放"
                    | "结束摆放"
                    | "添加为子场景"
                    | "工程变量"
                    | "触发规则"
                    | "地点区域"
                    | "从示例创建"
                    | "创建触发规则"
            )
        })
    {
        return Err(format!("场景工具正文混入其他页面控件：{labels:?}"));
    }
    Ok(Some(labels))
}

pub(super) fn verify_form_only(world: &mut World, page: &str) -> Result<(), String> {
    if layout::caption(world, "RulesPageTitle").as_deref() != Some(page) {
        return Err(format!("等待右栏显示 {page}"));
    }
    let body = form::entity(world, "交互当前工具内容").ok_or("缺少工具正文")?;
    let unrelated = world
        .query::<(Entity, &Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .any(|(e, text, node, vis)| {
            vis.get()
                && node.size().min_element() > 1.
                && ancestor(world, e, body)
                && SceneTool::ALL
                    .iter()
                    .any(|tool| tool.marker() == text.as_str())
        });
    if unrelated {
        return Err(format!("{page}正文混入了其他场景工具控件"));
    }
    Ok(())
}

pub(super) fn capture(
    world: &mut World,
    run: &mut Run,
    name: &str,
    tool: SceneTool,
) -> Result<bool, String> {
    let Some(labels) = scene_labels(world, run, tool)? else {
        return Ok(false);
    };
    let mut geometry = match measure(world) {
        Ok(value) => value,
        Err(reason) => {
            run.layout_wait = reason;
            return Ok(false);
        }
    };
    same_directory(run, &geometry)?;
    if matches!(name, "catalog-narrow" | "catalog-return") {
        let wide = run
            .layout
            .get("catalog-portal")
            .ok_or("缺少拖宽右栏后的目录证据")?;
        let width = |doc: &Value| {
            doc["workspace"][2].as_f64().unwrap_or_default()
                - doc["workspace"][0].as_f64().unwrap_or_default()
        };
        if (name == "catalog-narrow" && width(&geometry) + 1. >= width(wide))
            || (name == "catalog-return" && geometry["workspace"] != wide["workspace"])
        {
            run.layout_wait = format!("右栏未实际缩窄或切换工具改变了右栏尺寸：{geometry}");
            return Ok(false);
        }
    }
    let path = run.work.join(format!("rules-{name}.png"));
    let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
        return Ok(false);
    };
    geometry["screenshot"] = shot;
    geometry["labels"] = json!(labels);
    geometry["center"] = run.center.clone();
    run.layout.insert(name.into(), geometry);
    Ok(true)
}

pub(super) fn before_open(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    index: usize,
) -> Result<Option<Stage>, String> {
    let done = match index {
        0 => capture(world, run, "catalog-portal-default", SceneTool::Portal)?,
        1 => {
            run.center = Value::Null;
            layout::size_inspector(world, run, window, true)
        }
        2 => {
            let Some(center) = central(world).filter(|value| value["tab"] == "Scene") else {
                return Err("拖宽右栏后没有保留场景".into());
            };
            run.center = center;
            capture(world, run, "catalog-portal", SceneTool::Portal)?
        }
        3 => form::named_click(world, run, window, "交互目录条目空气墙"),
        4 => capture(world, run, "catalog-boundary", SceneTool::Boundary)?,
        5 => form::named_click(world, run, window, "交互目录条目动画触发"),
        6 => capture(
            world,
            run,
            "catalog-motion-trigger",
            SceneTool::MotionTrigger,
        )?,
        7 => form::named_click(world, run, window, "交互目录条目传送点"),
        8 => {
            // 进入工程页前再确认传送点成为当前页，而不是动画页还留在原侧栏。
            let Some(body) = form::entity(world, "交互当前工具内容") else {
                return Ok(None);
            };
            visible(world, |text: &Text| {
                text.as_str() == SceneTool::Portal.marker()
            })
            .is_some_and(|e| ancestor(world, e, body))
        }
        _ => return Ok(Some(Stage::Open)),
    };
    Ok(done.then_some(Stage::Directory(index + 1)))
}

pub(super) fn on_document(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    index: usize,
) -> Result<Option<Stage>, String> {
    let finish = ROWS.iter().map(|labels| labels.len()).sum::<usize>() * 2;
    if index == finish {
        let path = run.work.join("rules-catalog-context.png");
        let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
            return Ok(None);
        };
        let record = run
            .layout
            .get_mut("catalog-context")
            .ok_or("缺少六项直达记录")?;
        record["screenshot"] = shot;
        return Ok(Some(Stage::Capture));
    }
    let label = ROWS
        .iter()
        .flat_map(|labels| labels.iter())
        .nth(index / 2)
        .copied()
        .ok_or("未知右栏工具")?;
    if index % 2 == 0 {
        return Ok(
            form::named_click(world, run, window, &format!("交互目录条目{label}"))
                .then_some(Stage::ContextTools(index + 1)),
        );
    }
    if let Some(&tool) = SceneTool::ALL.get(index / 2) {
        if scene_labels(world, run, tool)?.is_none() {
            return Ok(None);
        }
    } else {
        // 前段保留了场景作用域，六项直达名字和变量工具正文的标题各有意义。
        let page = if label == "工程变量" { "场景变量" } else { label };
        if let Err(reason) = verify_form_only(world, page) {
            run.layout_wait = reason;
            return Ok(None);
        }
    }
    let geometry = match measure(world) {
        Ok(value) => value,
        Err(reason) => {
            run.layout_wait = reason;
            return Ok(None);
        }
    };
    same_directory(run, &geometry)?;
    let record = run
        .layout
        .entry("catalog-context")
        .or_insert_with(|| json!({"center":run.center,"tools":[]}));
    record["tools"]
        .as_array_mut()
        .ok_or("六项直达记录无效")?
        .push(json!({"tool":label,"directory":geometry}));
    Ok(Some(Stage::ContextTools(index + 1)))
}
