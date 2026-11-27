//! 用真实右栏验证单行分页，数据分布涵盖五页及筛选后的两条单页记录。
use super::*;
use bevy::ui::InteractionDisabled;

enum Step {
    Open,
    Click(&'static str),
    Check(usize, &'static str),
    CheckEditing,
    Rebind(bool),
}
use Step::*;

// 共用同一套操作序列，单页状态用 0 表示。先拍初始页，再检验两种翻页方式和过滤。
const STEPS: &[Step] = &[
    Open,
    Check(1, "initial"),
    Click("RulesNext"),
    Check(2, "next"),
    Click("RulesPrevious"),
    Check(1, "previous"),
    Click("RulesPage4"),
    Check(4, "direct"),
    Click("RulesNext"),
    Check(5, "last"),
    Click("RulesRow0"),
    CheckEditing,
    Click("RulesCancel"),
    Check(5, "after_cancel"),
    Click("RulesPage4"),
    Check(4, "back"),
    Click("RulesListTagPicker"),
    Click("RulesListOption2"),
    Check(0, "filtered"),
    Click("RulesListTagPicker"),
    Click("RulesListOption0"),
    Check(1, "reset"),
    Rebind(false),
    Rebind(true),
    Check(1, "after_rebind"),
];

pub(super) fn advance(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    index: usize,
) -> Result<Option<Stage>, String> {
    let (title, noun, prefix, report, sample) = if index < STEPS.len() {
        ("工程变量", "变量", "p", "variable-pager", "variable-pager-first")
    } else {
        ("触发规则", "规则", "r", "rule-pager", "rule-pager-last")
    };
    let step = &STEPS[index % STEPS.len()];
    let ready = match *step {
        Open => form::named_click(world, run, window, &format!("交互目录条目{title}")),
        CheckEditing => form::field_text(world, "RulesFieldId").as_deref() == Some(&format!("{prefix}33")),
        Rebind(delete) => row_rebind::advance(world, run, window, prefix, delete)?,
        Click(name) => {
            if !form::named_click(world, run, window, name) {
                run.layout_wait = super::button_diagnostic(world, name);
                false
            } else {
                true
            }
        }
        Check(page, event) => {
            if layout::caption(world, "RulesPageTitle").as_deref() != Some(title) {
                return Ok(None);
            }
            let mut state = match measure(world, noun, prefix, page) {
                Ok(state) => state,
                Err(reason) => {
                    run.layout_wait = reason;
                    return Ok(None);
                }
            };
            if (prefix == "p" && event == "initial") || (prefix == "r" && event == "last") {
                let path = run.work.join(format!("rules-{sample}.png"));
                let Some(screenshot) = evidence::capture(world, &mut run.capture, &path)? else {
                    return Ok(None);
                };
                state["screenshot"] = screenshot.clone();
                run.layout.insert(sample.into(), json!({"screenshot":screenshot}));
            }
            run.layout.entry(report.to_owned()).or_insert_with(|| json!({}))[event] = state;
            true
        }
    };
    if !ready {
        return Ok(None);
    }
    if index + 1 == 2 * STEPS.len() {
        if observe(world).is_none_or(|(_, dirty, editing, _)| dirty || editing) {
            return Err("翻页与标签筛选不应修改规则文件或打开编辑表单".into());
        }
        return Ok(Some(Stage::Capture));
    }
    Ok(Some(Stage::Paging(index + 1)))
}

fn measure(world: &mut World, noun: &str, prefix: &str, page: usize) -> Result<Value, String> {
    let filtered = page == 0;
    let expected = if filtered {
        vec![1, 35]
    } else {
        let first = (page - 1) * 8 + 1;
        (first..=(first + 7).min(35)).collect()
    };
    let rows: Vec<_> = (0..8).filter_map(|slot| {
        layout::caption(world, &format!("RulesCell{slot}Id"))
    }).collect();
    if rows != expected.iter().map(|i| format!("{prefix}{i:02}")).collect::<Vec<_>>() {
        return Err(format!("预计分页 {page} 的记录序号 {expected:?}，实际 {rows:?}"));
    }
    let total = if filtered { 2 } else { 35 };
    if layout::caption(world, "RulesSelection").as_deref() != Some(&format!("共 {total} 个{noun}")) {
        return Err(format!("页脚应显示 {total} 个{noun}"));
    }
    let Some((_, count)) = named_bounds(world, "RulesSelection") else {
        return Err("尚未排版分页计数".into());
    };
    let Some((_, last_row)) = named_bounds(world, &format!("RulesTableBodyRow{}", rows.len() - 1)) else {
        return Err("尚未排版分页末行".into());
    };
    if count.min.y + 1. < last_row.max.y {
        return Err("总数应显示在最后一行下面".into());
    }
    if filtered {
        if form::entity(world, "RulesPager").is_some() {
            return Err("筛选到一页时仍显示页码和箭头".into());
        }
        return Ok(json!({"total":total,"rows":rows,"pager":false}));
    }

    let (group, pager) = named_bounds(world, "RulesPager").ok_or("分页控件组尚未排版")?;
    let (footer, frame) = named_bounds(world, "RulesPagination").ok_or("分页页脚尚未排版")?;
    if !directory::ancestor(world, group, footer)
        || !frame.inflate(1.).contains(pager.min)
        || !frame.inflate(1.).contains(pager.max)
        || (count.center().y - pager.center().y).abs() > 1.5
        || count.max.x > pager.min.x + 1.
    {
        return Err(format!("计数与分页控件应在窄栏同一行，左 {count:?}，右 {pager:?}，页脚 {frame:?}"));
    }

    let pages: Vec<_> = (1..=5).filter(|number| form::entity(world, &format!("RulesPage{number}")).is_some()).collect();
    let first = if page <= 2 { 1 } else { 2 };
    if pages != (first..first + 4).collect::<Vec<_>>() {
        return Err(format!("第 {page} 页的可见页码不完整：{pages:?}"));
    }
    let mut previous = pager.min.x;
    let mut selected_fill = None;
    let mut regular_fills = Vec::new();
    let controls = std::iter::once(("RulesPrevious".to_owned(), "‹".to_owned(), page == 1, false))
        .chain(pages.iter().map(|number| (format!("RulesPage{number}"), number.to_string(), *number == page, true)))
        .chain(std::iter::once(("RulesNext".to_owned(), "›".to_owned(), page == 5, false)));
    for (name, label, disabled, numbered) in controls {
        let (entity, extent) = named_bounds(world, &name).ok_or_else(|| format!("未显示分页控件 {name}"))?;
        if extent.min.x + 1. < previous
            || !pager.inflate(1.).contains(extent.min)
            || !pager.inflate(1.).contains(extent.max)
            || !directory::ancestor(world, entity, group)
            || world.get::<InteractionDisabled>(entity).is_some() != disabled
        {
            return Err(format!("分页控件 {name} 排版或禁用状态不符"));
        }
        previous = extent.max.x;
        let label = world.get::<Children>(entity).and_then(|children| children.iter().find_map(|child| {
            let text = world.get::<Text>(child)?;
            (text.as_str() == label).then_some(child)
        })).ok_or_else(|| format!("分页控件 {name} 缺少实际箭头或页码"))?;
        let ink = layout::glyph_bounds(world, label).ok_or_else(|| format!("分页控件 {name} 尚未生成字形"))?;
        if !extent.inflate(1.).contains(ink.min) || !extent.inflate(1.).contains(ink.max) {
            return Err(format!("分页控件 {name} 的字形被截断"));
        }
        if numbered {
            let fill = world.get::<BackgroundColor>(entity).ok_or("页码缺少背景")?.0;
            if disabled { selected_fill = Some(fill); } else { regular_fills.push(fill); }
        }
    }
    if selected_fill.is_none_or(|fill| regular_fills.contains(&fill)) {
        return Err("当前页应高亮，区分于可点击页码".into());
    }
    Ok(json!({"current":page,"pages":pages,"total":total,"rows":rows,"count":layout::coords(count),
        "pager":layout::coords(pager),"previous_disabled":page==1,"next_disabled":page==5}))
}

fn named_bounds(world: &mut World, name: &str) -> Option<(Entity, Rect)> {
    let entity = form::entity(world, name)?;
    Some((entity, layout::bounds(world, entity)?))
}
