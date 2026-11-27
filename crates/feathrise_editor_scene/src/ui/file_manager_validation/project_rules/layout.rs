//! 读取真实排版矩形；用窗口滚轮把折叠和换行后的控件滚进可点击区域，再取 GPU 截图。
use super::*;
use bevy::input::{
    mouse::{MouseScrollUnit, MouseWheel},
    touch::TouchPhase,
};
use bevy::text::TextLayoutInfo;

#[derive(Clone, Copy, Debug)]
pub(super) enum Sample {
    Empty,
    Variables { id: &'static str, value: &'static str },
    Number,
    RereadNumber,
    Locations,
    RuleWide,
    RuleNarrow,
    RuleScrolled,
}

impl Sample {
    fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty-variable",
            Self::Variables { value: "true", .. } => "first-variable",
            Self::Variables { .. } => "false-variable",
            Self::Number => "number-variable",
            Self::RereadNumber => "number-reread",
            Self::Locations => "location",
            Self::RuleWide => "rule-wide",
            Self::RuleNarrow => "rule-narrow",
            Self::RuleScrolled => "rule-narrow-scrolled",
        }
    }
}

pub(super) fn bounds(world: &World, entity: Entity) -> Option<Rect> {
    let center = world.get::<UiGlobalTransform>(entity)?.translation;
    Some(Rect::from_center_size(
        center,
        world.get::<ComputedNode>(entity)?.size(),
    ))
}

/// 沿用 GPU UI 文本渲染的内容框原点、图集矩形和全局变换，得到窗口物理像素中的字形范围。
pub(super) fn glyph_bounds(world: &World, entity: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    let transform = world.get::<UiGlobalTransform>(entity)?.affine();
    world
        .get::<TextLayoutInfo>(entity)?
        .glyphs
        .iter()
        .filter(|g| g.atlas_info.rect.size().min_element() > 0.)
        .map(|g| {
            let origin = node.content_box().min + g.position;
            let half_size = g.atlas_info.rect.size() * 0.5;
            Rect::from_corners(
                transform.transform_point2(origin - half_size),
                transform.transform_point2(origin + half_size),
            )
        })
        .reduce(|a, b| a.union(b))
}

fn rect(world: &mut World, name: &str) -> Result<Rect, String> {
    form::entity(world, name)
        .and_then(|e| bounds(world, e))
        .ok_or_else(|| format!("尚未排版 {name}"))
}

pub(super) fn reveal(world: &mut World, run: &mut Run, window: Entity, target: Entity) -> bool {
    reveal_rules_control(world, window, target, &mut run.wheel_point)
}

/// 规则编辑和演出绑定验收都须通过实际滚轮定位同一表单中的控件。
pub(crate) fn reveal_rules_control(
    world: &mut World,
    window: Entity,
    target: Entity,
    wheel_point: &mut Option<Vec2>,
) -> bool {
    *wheel_point = None;
    let mut ancestor = target;
    let area = loop {
        let Some(parent) = world.get::<ChildOf>(ancestor) else {
            return true;
        };
        ancestor = parent.parent();
        if world.get::<ScrollPosition>(ancestor).is_some()
            && world
                .get::<Node>(ancestor)
                .is_some_and(|node| node.overflow.y == OverflowAxis::Scroll)
        {
            break ancestor;
        }
    };
    let (Some(viewport), Some(control)) = (bounds(world, area), bounds(world, target)) else {
        return false;
    };
    let y = control.center().y;
    if (viewport.min.y..=viewport.max.y).contains(&y) {
        return true;
    }

    let cursor = viewport.center();
    *wheel_point = Some(cursor);
    scroll_lines(world, window, cursor, if y < viewport.min.y { 3. } else { -3. });
    false
}

fn scroll_lines(world: &mut World, window: Entity, position: Vec2, lines: f32) {
    move_cursor(world, window, position.as_dvec2());
    window_event(
        world,
        MouseWheel {
            window,
            unit: MouseScrollUnit::Line,
            x: 0.,
            y: lines,
            phase: TouchPhase::Moved,
        },
    );
}

pub(super) fn compact_variable_scroll(
    world: &mut World,
    run: &mut Run,
    window: Entity,
) -> Result<bool, String> {
    let Some(scroll) = form::entity(world, "RulesEditorDialogScroll") else {
        return Ok(false);
    };
    let viewport = bounds(world, scroll).ok_or("变量弹窗正文尚未排版")?;
    let form = rect(world, "RulesForm")?;
    if run.keys == 0 {
        let cursor = viewport.center();
        run.wheel_point = Some(cursor);
        move_cursor(world, window, cursor.as_dvec2());
        if !world
            .get::<bevy::ui::RelativeCursorPosition>(scroll)
            .is_some_and(|position| position.cursor_over())
        {
            return Ok(false);
        }
        scroll_lines(world, window, cursor, -3.);
    }
    if run.keys < 3 {
        run.keys += 1;
        return Ok(false);
    }
    // 短表单已全部在正文中，向下滚轮不能把它送出视区。
    let visible = viewport.inflate(1.);
    let offset = world.get::<ScrollPosition>(scroll).ok_or("缺少变量弹窗滚动位置")?.y;
    if offset != 0. || !visible.contains(form.min) || !visible.contains(form.max) {
        return Err(format!("短变量表单滚入空白，滚动 {offset}，表单 {form:?}，正文 {viewport:?}"));
    }
    run.layout.insert("compact-variable-scroll".into(), json!({"scroll_y":offset}));
    Ok(true)
}

fn inside_x(inner: Rect, outer: Rect) -> bool {
    // GPU 物理像素只容许边缘取整的误差。
    inner.min.x >= outer.min.x - 1. && inner.max.x <= outer.max.x + 1.
}

pub(super) fn has_field_caption(world: &World, control: Entity, label: &str) -> bool {
    world
        .get::<ChildOf>(control)
        .and_then(|row| world.get::<ChildOf>(row.parent()))
        .and_then(|group| world.get::<Children>(group.parent()))
        .is_some_and(|parts| parts.iter().any(|part| {
            world.get::<Text>(part).is_some_and(|text| text.as_str() == label)
                && glyph_bounds(world, part).is_some()
        }))
}

fn verify_bare_form(world: &mut World) -> Result<(), String> {
    let form = form::entity(world, "RulesForm").ok_or("尚未展示弹窗表单")?;
    if world.get::<Node>(form).is_some_and(|node| node.border != UiRect::ZERO)
        && world.get::<BorderColor>(form).is_some_and(|color| *color != BorderColor::all(Color::NONE))
    {
        return Err("工程弹窗表单仍有可见的内层边框".into());
    }
    Ok(())
}

pub(super) fn coords(rect: Rect) -> [f32; 4] {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
}

/// 右栏的可滚动表单与两行记录使用同一布局，拖动分隔条只改变各项宽度。
fn measured(world: &mut World, sample: Sample) -> Result<Value, String> {
    let panel = rect(world, "RulesWorkbench")?;
    let viewport = rect(world, "交互当前工具内容")?;
    let list = rect(world, "RulesList")?;
    let new = rect(world, "RulesNew").ok();
    let page = match sample {
        Sample::Locations => "地点区域",
        Sample::RuleWide | Sample::RuleNarrow | Sample::RuleScrolled => "触发规则",
        _ => "工程变量",
    };
    if !inside_x(list, viewport)
        || !inside_x(panel, viewport)
        || new.is_some_and(|r| !inside_x(r, list))
    {
        return Err(format!(
            "等待{page}的表格与工具栏正确排版，正文 {viewport:?}，表格 {list:?}，新建 {new:?}"
        ));
    }
    directory::verify_form_only(world, page)?;
    let directory = directory::measure(world)?;
    let details = rect(world, "RulesForm").ok();
    let modal = form::entity(world, "RulesEditorDialogScroll").and_then(|id| {
        let form = form::entity(world, "RulesForm")?;
        let card = world.get::<ChildOf>(id)?.parent();
        bounds(world, card)
            .filter(|_| directory::ancestor(world, form, id))
            .map(|card| (id, card))
    });
    // 变量对话框浮在原工作台上，其表单和背后的只读变量列表分别核对布局。
    let table = table::measure(world, if modal.is_some() { None } else { details })?;
    let mut data = json!({"panel":coords(panel),"scroll_view":coords(viewport),"list":coords(list),
        "new":new.map(coords), "page":page,"directory":directory,"table":table});
    let scroll = modal
        .map(|(id, _)| id)
        .or_else(|| form::entity(world, "交互当前工具内容"))
        .ok_or("缺少实际滚动区")?;
    let y_offset = world.get::<ScrollPosition>(scroll).ok_or("缺少滚动位置")?.y;
    data["scroll_y"] = json!(y_offset);
    match sample {
        Sample::Empty => {
            if form::entity(world, "RulesForm").is_some()
                || form::entity(world, "RulesRow0").is_some()
                || !new.is_some_and(|r| viewport.contains(r.center()))
            {
                return Err("空列表必须能点击新建，不能预先展开未创建的记录".into());
            }
        }
        Sample::Variables { .. } | Sample::Locations | Sample::RuleWide | Sample::RuleNarrow => {
            let id = match sample {
                Sample::Variables { id, .. } => id,
                Sample::Locations => "east_gate",
                _ => "draw_at_three",
            };
            let slot =
                form::row_slot(world, id).ok_or_else(|| format!("未找到已保存的条目 {id}"))?;
            let row = &data["table"]["rows"][slot.to_string()];
            let cells = &row["cells"];
            let edit = rect(world, &format!("RulesRow{slot}"))?;
            if !viewport.contains(edit.center())
                || details.is_some()
                || (matches!(sample, Sample::RuleWide | Sample::RuleNarrow)
                    && [0, 1, 2].map(|i| cells[i]["text"].as_str())
                        != [Some(id), Some("a → 3"), Some("播放剧情演出 · draw-river-sword")])
            {
                return Err(format!(
                    "右栏记录未正确回显：文字 {cells:?}，编辑 {edit:?}"
                ));
            }
            if let Sample::Variables { value, .. } = sample
                && (cells[1]["text"].as_str() != Some("bool")
                    || cells[2]["text"].as_str() != Some(value))
            {
                return Err(format!("变量表未回显已保存的值：{cells:?}"));
            }
            data["row"] = json!({"id":id,"bounds":row["bounds"],"cells":cells,"edit":coords(edit)});
        }
        Sample::Number | Sample::RereadNumber => {
            let (_, dialog) = modal.ok_or("尚未打开变量对话框")?;
            let details = details.ok_or("尚未展示数字变量表单")?;
            verify_bare_form(world)?;
            let field = form::entity(world, "RulesFieldDefault").ok_or("尚未展示值输入框")?;
            let input = rect(world, "RulesFieldId")?;
            let save = rect(world, "RulesSave")?;
            let default = rect(world, "RulesFieldDefault")?;
            let yarn = caption(world, "RulesYarnName").ok_or("尚未展示 Yarn 读取名")?;
            if !inside_x(details, dialog)
                || !inside_x(input, details)
                || !inside_x(default, details)
                || !inside_x(save, dialog)
                || default.min.y + 1. < input.max.y
                || (default.min.x - input.min.x).abs() > 1.
                || save.min.y + 1. < default.max.y.max(input.max.y)
                || !yarn.contains("$project_a")
                || !has_field_caption(world, field, "值")
                || form::field_text(world, "RulesFieldId").as_deref() != Some("a")
                || form::field_text(world, "RulesFieldDefault").as_deref() != Some("3")
                || form::entity(world, "RulesBoolTrue").is_some()
                || form::entity(world, "RulesFieldChoices").is_some()
                || form::row_slot(world, "gate_open").is_none()
            {
                return Err(format!(
                    "等待对话框中的数字变量、Yarn 读取名与保存按钮正确布局，对话框 {dialog:?}，表单 {details:?}，保存 {save:?}"
                ));
            }
            data["form"] = json!({"dialog":coords(dialog),"bounds":coords(details),"input":coords(input),"default":coords(default),"save":coords(save),"yarn":yarn});
        }
        Sample::RuleScrolled => {
            let details = details.ok_or("尚未展示规则表单")?;
            let (_, dialog) = modal.ok_or("尚未打开规则对话框")?;
            verify_bare_form(world)?;
            let scroller = bounds(world, scroll).ok_or("规则对话框正文未排版")?;
            let terminal_field = rect(world, "RulesFieldActionValue")?;
            let save = rect(world, "RulesSave")?;
            if y_offset <= 0.
                || !scroller.contains(terminal_field.center())
                || !dialog.contains(save.center())
                || !inside_x(details, scroller)
            {
                return Err(format!(
                    "短窗规则对话框尚不能滚到末端字段，滚动 {y_offset}，字段 {terminal_field:?}，视区 {scroller:?}"
                ));
            }
            data["form"] = json!({"dialog":coords(dialog),"bounds":coords(details),"scroll_view":coords(scroller),"last_field":coords(terminal_field),"save":coords(save)});
        }
    }
    Ok(data)
}

pub(super) fn caption(world: &mut World, name: &str) -> Option<String> {
    form::entity(world, name)
        .and_then(|e| world.get::<Text>(e))
        .map(|t| t.as_str().to_owned())
}

pub(super) fn size_inspector(world: &mut World, run: &mut Run, window: Entity, wide: bool) -> bool {
    use crate::{
        components::SceneEditorPaneSplitter,
        constants::{EDITOR_INSPECTOR_MAX_WIDTH, EDITOR_INSPECTOR_MIN_WIDTH},
        resources::SceneEditorLayoutState,
        types::EditorPane,
    };
    use feathrise_editor_ui::UiInteraction;
    let width = world.resource::<SceneEditorLayoutState>().inspector_width;
    let wanted = if wide {
        (EDITOR_INSPECTOR_MAX_WIDTH + EDITOR_INSPECTOR_MIN_WIDTH) * 0.5
    } else {
        EDITOR_INSPECTOR_MIN_WIDTH
    };
    if let Some(target) = run.splitter_target {
        run.wheel_point = Some(target);
        move_cursor(world, window, target.as_dvec2());
        if (width - wanted).abs() > 1. {
            return false;
        }
        super::super::mouse(world, window, ButtonState::Released);
        run.splitter_target = None;
        return true;
    }
    if (width - wanted).abs() <= 1. {
        return true;
    }
    let Some(splitter) = visible(world, |s: &SceneEditorPaneSplitter| {
        s.0 == EditorPane::Inspector
    }) else {
        return false;
    };
    let Some(origin) = bounds(world, splitter).map(|r| r.center()) else {
        return false;
    };
    run.wheel_point = Some(origin);
    move_cursor(world, window, origin.as_dvec2());
    if world.get::<UiInteraction>(splitter) == Some(&UiInteraction::Hovered) {
        let scale =
            world.get::<Window>(window).unwrap().scale_factor() * world.resource::<UiScale>().0;
        super::super::mouse(world, window, ButtonState::Pressed);
        run.splitter_target = Some(origin + Vec2::X * (width - wanted) * scale);
    }
    false
}

pub(super) fn capture(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    sample: Sample,
) -> Result<bool, String> {
    let target = match sample {
        Sample::RuleScrolled => form::entity(world, "RulesFieldActionValue"),
        Sample::Number | Sample::RereadNumber => form::entity(world, "RulesFieldId"),
        Sample::Variables { .. } | Sample::Locations => form::entity(world, "RulesRow0"),
        Sample::RuleWide | Sample::RuleNarrow => form::row_slot(world, "draw_at_three")
            .and_then(|i| form::entity(world, &format!("RulesRow{i}"))),
        Sample::Empty => form::entity(world, "RulesNew"),
    };
    let Some(target) = target else {
        return Ok(false);
    };
    if !reveal(world, run, window, target) {
        return Ok(false);
    }
    let mut geometry = match measured(world, sample) {
        Ok(data) => data,
        Err(reason) => {
            run.layout_wait = reason;
            return Ok(false);
        }
    };
    directory::same_directory(run, &geometry["directory"])?;
    if matches!(sample, Sample::RuleNarrow) {
        let wide_sample = &run.layout["rule-wide"];
        let wide = &wide_sample["directory"];
        let narrow = &geometry["directory"];
        let width = |bounds: &Value| {
            bounds[2].as_f64().unwrap_or_default() - bounds[0].as_f64().unwrap_or_default()
        };
        if wide["entity"] != narrow["entity"]
            || width(&narrow["workspace"]) + 1. >= width(&wide["workspace"])
        {
            run.layout_wait = "触发规则验收尚未用同一目录拖窄右栏".into();
            return Ok(false);
        }
        if (width(&wide_sample["table"]["tracks"][3]) - width(&geometry["table"]["tracks"][3]))
            .abs() > 1.
        {
            return Err("拖窄检查器时操作列宽度发生变化".into());
        }
    }
    let name = sample.name();
    let path = run.work.join(format!("rules-{name}.png"));
    let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
        return Ok(false);
    };
    geometry["screenshot"] = shot;
    geometry["center"] = run.center.clone();
    run.layout.insert(name.into(), geometry);
    Ok(true)
}
