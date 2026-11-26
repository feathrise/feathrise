//! 使用表单的实际按钮和原生输入编辑规则，先核对一次保存的磁盘结果，再进入下一项。
use super::*;
use bevy::text::EditableText;
use layout::Sample;

#[derive(Clone, Copy, Debug)]
enum Step {
    Button(&'static str),
    Page(&'static str),
    Field(&'static str, &'static str),
    Selected(&'static str),
    Check(usize),
    NumberVariable,
    NumberTrigger,
    Shortcut(KeyCode),
    Saved,
    Layout(Sample),
    InspectorWide,
    RuleNarrow,
    ShortWindow,
    InspectorNarrow,
    Returned,
    Corner(bool),
    Drawn,
    BoolValues,
    CompactVariableScroll,
    DeleteRow(&'static str),
    Deleted { id: &'static str, variable: bool },
    TextWithoutChoices,
    TryBackgroundTool(&'static str),
    VariableModalKept,
    RuleToolbarReduced,
    RuleModalKept { editing: bool },
    EnumFields { reopened: bool },
    Stored(&'static str),
    NameFocused,
    Duplicate(bool),
}
use Step::*;

// 变量和触发规则复用编辑对话框，地点区域留在右栏以便直接从视口取点。
const FIRST: &[Step] = &[
    Layout(Sample::Empty),
    Button("RulesNew"),
    Field("RulesFieldId", "gate_open"),
    Button("RulesTypeBool"),
    BoolValues,
    Button("RulesBoolTrue"),
    Button("RulesSave"),
    Stored("gate_open"),
    Layout(Sample::Variables { id: "gate_open", value: "true" }),
    Selected("gate_open"),
    BoolValues,
    Button("RulesCancel"),
    Button("RulesNew"),
    Field("RulesFieldId", "delete_me"),
    Button("RulesTypeBool"),
    Button("RulesSave"),
    Stored("delete_me"),
    Layout(Sample::Variables { id: "delete_me", value: "false" }),
    DeleteRow("delete_me"),
    Deleted { id: "delete_me", variable: true },
    Button("RulesNew"),
    NameFocused,
    Field("RulesFieldId", "a"),
    Button("RulesTypeNumber"),
    Field("RulesFieldDefault", "3"),
    Button("RulesTypeText"),
    TextWithoutChoices,
    Button("RulesTypeNumber"),
    Layout(Sample::Number),
    CompactVariableScroll,
    TryBackgroundTool("交互目录条目触发规则"),
    VariableModalKept,
    Button("RulesSave"),
    Stored("a"),
    Button("RulesNew"),
    Field("RulesFieldId", "a"),
    Button("RulesSave"),
    Duplicate(false),
    Button("RulesCancel"),
    Duplicate(true),
    Button("交互目录条目地点区域"),
    Page("地点区域"),
    Button("RulesNew"),
    Field("RulesFieldId", "east_gate"),
    Field("RulesFieldScene", "river"),
    Field("RulesFieldMin", "0, 0, 0"),
    Field("RulesFieldMax", "4, 3, 6"),
    Button("RulesSave"),
    Stored("east_gate"),
    Layout(Sample::Locations),
    Button("交互目录条目传送点"),
    Returned,
    InspectorNarrow,
    Button("交互目录条目工程变量"),
    Page("工程变量"),
    Selected("a"),
    Layout(Sample::RereadNumber),
    Button("RulesCancel"),
    Button("交互目录条目地点区域"),
    Page("地点区域"),
    Button("RulesNew"),
    Field("RulesFieldId", "drawn_gate"),
    Field("RulesFieldScene", "M1-SAMPLE"),
    Field("RulesFieldMin", "0, 0.5, 0"),
    Field("RulesFieldMax", "1, 3.5, 1"),
    Button("RulesDrawLocation"),
    Corner(false),
    Corner(true),
    Drawn,
    Button("RulesSave"),
    Stored("drawn_gate"),
];

const EDIT: &[Step] = &[
    Button("交互目录条目工程变量"),
    Page("工程变量"),
    Button("RulesNew"),
    Field("RulesFieldId", "dock_ready"),
    Button("RulesTypeBool"),
    Button("RulesSave"),
    Check(0),
    Stored(""),
    Button("交互目录条目触发规则"),
    Page("触发规则"),
    RuleToolbarReduced,
    Button("RulesNew"),
    Field("RulesFieldId", "guard_freed"),
    TryBackgroundTool("交互目录条目地点区域"),
    RuleModalKept { editing: false },
    Field("RulesFieldTrigger", "quest.completed"),
    Field("RulesFieldEventTarget", "dock_guard"),
    Button("RulesOnce"),
    Field("RulesFieldConditionVariable", "dock_ready"),
    Field("RulesFieldConditionValue", "false"),
    Field("RulesFieldActionName", "dock_ready"),
    Field("RulesFieldActionValue", "true"),
    Button("RulesSave"),
    Check(1),
    Stored(""),
    Selected("guard_freed"),
    TryBackgroundTool("交互目录条目工程变量"),
    RuleModalKept { editing: true },
    Button("RulesActionType"),
    Field("RulesFieldActionName", "cinematic.play"),
    Field("RulesFieldActionValue", "guard-unseal"),
    Button("RulesActionAdd"),
    Check(2),
    Shortcut(KeyCode::KeyZ),
    Check(1),
    Shortcut(KeyCode::KeyY),
    Check(2),
    Shortcut(KeyCode::KeyS),
    Stored(""),
    Button("交互目录条目工程变量"),
    Page("工程变量"),
    Button("RulesNew"),
    Field("RulesFieldId", "a"),
    Button("RulesTypeNumber"),
    Button("RulesSave"),
    NumberVariable,
    Stored(""),
    Button("RulesNew"),
    Field("RulesFieldId", "delivery"),
    Button("RulesTypeEnum"),
    Field("RulesFieldChoices", "harbor, mentor"),
    Field("RulesFieldDefault", "harbor"),
    EnumFields { reopened: false },
    Button("RulesSave"),
    Stored("delivery"),
    Button("交互目录条目触发规则"),
    Page("触发规则"),
    Button("RulesNew"),
    Field("RulesFieldId", "draw_at_three"),
    Button("RulesTrigger"),
    Field("RulesFieldTrigger", "a"),
    Button("RulesTo"),
    Field("RulesFieldTo", "3"),
    Button("RulesActionPlayCinematic"),
    Field("RulesFieldActionValue", "draw-river-sword"),
    Button("RulesSave"),
    NumberTrigger,
    Button("RulesNew"),
    Field("RulesFieldId", "delete_rule"),
    Field("RulesFieldTrigger", "quest.completed"),
    Field("RulesFieldEventTarget", "dock_guard"),
    Field("RulesFieldActionName", "dock_ready"),
    Field("RulesFieldActionValue", "false"),
    Button("RulesSave"),
    Stored("delete_rule"),
    DeleteRow("delete_rule"),
    Deleted { id: "delete_rule", variable: false },
    Saved,
];

const REOPEN: &[Step] = &[
    Page("触发规则"),
    RuleToolbarReduced,
    InspectorWide,
    Layout(Sample::RuleWide),
    Selected("guard_freed"),
    Check(2),
    Button("RulesCancel"),
    Selected("draw_at_three"),
    NumberTrigger,
    Button("RulesCancel"),
    Button("交互目录条目工程变量"),
    Page("工程变量"),
    Selected("delivery"),
    EnumFields { reopened: true },
    Button("RulesCancel"),
    Button("交互目录条目触发规则"),
    Page("触发规则"),
    RuleNarrow,
    Layout(Sample::RuleNarrow),
    ShortWindow,
    Selected("draw_at_three"),
    Layout(Sample::RuleScrolled),
    Button("RulesCancel"),
];

pub(super) fn entity(world: &mut World, id: &str) -> Option<Entity> {
    visible(world, |name: &Name| name.as_str() == id)
}

pub(super) fn named_click(world: &mut World, run: &mut Run, window: Entity, id: &str) -> bool {
    let button = entity(world, id);
    if button.is_some_and(|target| !layout::reveal(world, run, window, target)) {
        return false;
    }
    click(world, &mut run.pointer, window, button)
}

pub(super) fn field_text(world: &mut World, id: &str) -> Option<String> {
    let input = entity(world, id)?;
    world
        .get::<EditableText>(input)
        .map(|e| e.value().to_string())
}

/// 右栏按固定记录布局，用名称定位该条记录自己的“编辑”按钮。
pub(super) fn row_slot(world: &mut World, id: &str) -> Option<usize> {
    world
        .query::<(&Name, &Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(name, caption, node, vis)| {
            let slot = name
                .as_str()
                .strip_prefix("RulesCell")?
                .strip_suffix("Id")?
                .parse()
                .ok()?;
            (vis.get()
                && node.size().min_element() > 1.
                && caption.split_whitespace().last() == Some(id))
            .then_some(slot)
        })
}

#[derive(Debug, PartialEq)]
struct DeletionLayout {
    anchors: [(Entity, f32); 2],
    rows: Vec<(String, f32)>,
}

pub(super) struct DeletionCheck {
    layout: DeletionLayout,
    frames: u32,
    before_receipt: u32,
}

fn deletion_layout(world: &mut World, deleted: &str) -> Option<DeletionLayout> {
    let panel = entity(world, "RulesWorkbench")?;
    let list = entity(world, "RulesList")?;
    let anchors = [
        (panel, layout::bounds(world, panel)?.min.y.round()),
        (list, layout::bounds(world, list)?.min.y.round()),
    ];
    // 夹具删除末尾行，其余行保持可见且留在各自原来的纵向位置。
    let mut rows: Vec<_> = world
        .query::<(&Name, &Text, &ComputedNode, &InheritedVisibility, &UiGlobalTransform)>()
        .iter(world)
        .filter(|(name, caption, node, visible, _)| {
            name.as_str().starts_with("RulesCell")
                && name.as_str().ends_with("Id")
                && visible.get()
                && node.size().min_element() > 1.
                && !caption.is_empty()
                && caption.as_str() != deleted
        })
        .map(|(_, caption, _, _, transform)| {
            (caption.as_str().to_owned(), transform.translation.y.round())
        })
        .collect();
    rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    (!rows.is_empty()).then_some(DeletionLayout { anchors, rows })
}

fn bottom_status(world: &mut World) -> Option<String> {
    use super::super::{StatusText, StatusTextRole};
    let summary = visible(world, |text: &StatusText| text.0 == StatusTextRole::Summary)?;
    world
        .get::<Text>(summary)
        .map(|text| text.as_str().to_owned())
}

/// 每份按键先按下再释放，在真实焦点的原生输入框提交 IME。未向文档注册表或表单模型直接写值。
pub(super) fn replace(
    world: &mut World,
    pending: &mut Option<(Entity, bool)>,
    step: &mut u8,
    window: Entity,
    input: Entity,
    text: &str,
) -> bool {
    if pending.is_some() || world.resource::<InputFocus>().get() != Some(input) {
        super::super::raw_click(world, pending, window, input);
        return false;
    }
    if *step > 3 {
        return world
            .get::<EditableText>(input)
            .is_some_and(|t| t.value() == text);
    }
    if *step == 3 {
        window_event(
            world,
            Ime::Commit {
                window,
                value: text.into(),
            },
        );
    } else {
        chord(world, window, *step, KeyCode::KeyA);
    }
    *step += 1;
    false
}

fn chord(world: &mut World, window: Entity, step: u8, code: KeyCode) {
    let logical = Key::Character(
        match code {
            KeyCode::KeyA => "a",
            KeyCode::KeyS => "s",
            KeyCode::KeyZ => "z",
            KeyCode::KeyY => "y",
            _ => unreachable!(),
        }
        .into(),
    );
    match step {
        0 => key_state(
            world,
            window,
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Pressed,
        ),
        1 => key_state(world, window, code, logical, ButtonState::Pressed),
        _ => {
            key_state(world, window, code, logical, ButtonState::Released);
            key_state(
                world,
                window,
                KeyCode::ControlLeft,
                Key::Control,
                ButtonState::Released,
            );
        }
    }
}

fn steps(phase: &str) -> &'static [Step] {
    match phase {
        "empty" => FIRST,
        "draw-reopen" => &[Page("地点区域"), Selected("drawn_gate"), Drawn],
        "reopen" => REOPEN,
        _ => EDIT,
    }
}

pub(super) fn form(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    index: usize,
) -> Result<Option<Stage>, String> {
    let reopening = run.phase == "reopen";
    let script = steps(&run.phase);
    let Some(step) = script.get(index) else {
        if run.phase == "empty" {
            run.center = Value::Null;
            return Ok(Some(Stage::OpenContext));
        }
        return Ok(Some(Stage::Capture));
    };
    let done = match *step {
        Button(id) => named_click(world, run, window, id),
        Page(title) => entity(world, "RulesPageTitle").is_some_and(|e| {
            world
                .get::<Text>(e)
                .is_some_and(|caption| caption.as_str() == title)
        }),
        Field(id, value) => {
            let Some(e) = entity(world, id) else {
                return Ok(None);
            };
            if !layout::reveal(world, run, window, e) {
                return Ok(None);
            }
            replace(world, &mut run.pointer, &mut run.keys, window, e, value)
        }
        Selected(id) => {
            if field_text(world, "RulesFieldId").as_deref() != Some(id) {
                // 用真实名录行定位，少量项目无需显示翻页工具。
                if let Some(slot) = row_slot(world, id)
                    && let Some(row) = entity(world, &format!("RulesRow{slot}"))
                    && layout::reveal(world, run, window, row)
                {
                    click(world, &mut run.pointer, window, Some(row));
                }
                return Ok(None);
            }
            true
        }
        Shortcut(code) => {
            // 使用表单实际可视区域建立快捷键归属，光标不能停在滚动后的旧按钮位置。
            let modal = entity(world, "RulesEditorDialogScroll");
            let Some(panel) = modal.or_else(|| entity(world, "工程交互工作区")) else {
                return Ok(None);
            };
            let Some(visible_panel) = layout::bounds(world, panel).and_then(|bounds| {
                if modal.is_some() {
                    return Some(bounds);
                }
                let area = entity(world, "交互当前工具内容")?;
                Some(bounds.intersect(layout::bounds(world, area)?))
            }) else {
                return Ok(None);
            };
            let point = visible_panel.center();
            run.wheel_point = Some(point);
            move_cursor(world, window, point.as_dvec2());
            if !world
                .get::<bevy::ui::RelativeCursorPosition>(panel)
                .is_some_and(|position| position.cursor_over())
            {
                return Ok(None);
            }
            chord(world, window, run.keys, code);
            run.keys += 1;
            run.keys == 3
        }
        NumberVariable | NumberTrigger => {
            let Some((source, _, ..)) = observe(world) else {
                return Ok(None);
            };
            let v: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
            let rule = v["rules"]
                .as_array()
                .and_then(|rules| rules.iter().find(|r| r["id"] == "draw_at_three"));
            v["variables"]["a"]["default"] == 0
                && (matches!(step, NumberVariable)
                    || (rule.is_some_and(|r| {
                        r["when"] == json!({"changed":"a","to":3})
                            && r["actions"]
                                == json!([{"request":"cinematic.play","target":"draw-river-sword"}])
                    }) && (!reopening
                        || (field_text(world, "RulesFieldTrigger").as_deref() == Some("a")
                            && field_text(world, "RulesFieldTo").as_deref() == Some("3")
                            && field_text(world, "RulesFieldActionName").as_deref()
                                == Some("cinematic.play")))))
        }
        Check(count) => {
            let Some((source, dirty, ..)) = observe(world) else {
                return Ok(None);
            };
            let value: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
            if (reopening && dirty) || value["variables"]["dock_ready"]["default"] != false {
                return Ok(None);
            }
            if count > 0 {
                let Some(rule) = value["rules"]
                    .as_array()
                    .and_then(|r| r.iter().find(|r| r["id"] == "guard_freed"))
                else {
                    return Ok(None);
                };
                let mut actions = vec![json!({"set":"dock_ready","value":true})];
                if count == 2 {
                    actions.push(json!({"request":"cinematic.play","target":"guard-unseal"}));
                }
                if rule["actions"] != json!(actions)
                    || rule["once"] != true
                    || rule["when"] != json!({"event":"quest.completed","target":"dock_guard"})
                    || rule["if"] != json!([{"variable":"dock_ready","equals":false}])
                {
                    return Ok(None);
                }
                if reopening
                    && !(field_text(world, "RulesFieldTrigger").as_deref()
                        == Some("quest.completed")
                        && field_text(world, "RulesFieldEventTarget").as_deref()
                            == Some("dock_guard"))
                {
                    return Ok(None);
                }
            }
            true
        }
        Saved => {
            let Some((_, false, _, status)) = observe(world) else {
                return Ok(None);
            };
            if !status.contains("已保存") {
                return Ok(None);
            }
            let disk = fs::read_to_string(run.work.join("project/assets").join(PATH))
                .map_err(|e| e.to_string())?;
            let mut value: Value = serde_json::from_str(&disk).map_err(|e| e.to_string())?;
            value["rules"][0]["actions"][0]["set"] = json!("missing_rules_variable");
            run.invalid = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
            value["rules"][0]["actions"][0]["set"] = json!("route");
            value["rules"][0]["id"] = json!("smith_route_edited");
            run.edited = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
            run.saved_form = disk;
            true
        }
        Layout(sample) => layout::capture(world, run, window, sample)?,
        InspectorNarrow => {
            run.center = Value::Null;
            if layout::size_inspector(world, run, window, false)
                && directory::capture(world, run, "catalog-narrow", directory::SceneTool::Portal)?
            {
                let Some(center) = central(world).filter(|value| value["tab"] == "Scene") else {
                    return Err("拖窄右栏后没有保留场景".into());
                };
                run.center = center;
                if let Some(record) = run.layout.get_mut("catalog-narrow") {
                    record["center"] = run.center.clone();
                }
                true
            } else {
                false
            }
        }
        InspectorWide => layout::size_inspector(world, run, window, true),
        RuleNarrow => layout::size_inspector(world, run, window, false),
        Returned => directory::capture(world, run, "catalog-return", directory::SceneTool::Portal)?,
        Corner(second) => location::point(world, run, window, second)?,
        Drawn => location::restored(world, run)?,
        BoolValues => {
            let caption = |world: &mut World, name: &str| {
                let button = entity(world, name)?;
                world.get::<Children>(button)?.iter().find_map(|child| {
                    let caption = world.get::<Text>(child)?;
                    layout::glyph_bounds(world, child)?;
                    Some(caption.as_str().to_owned())
                })
            };
            let labels = ["RulesBoolFalse", "RulesBoolTrue"].map(|name| caption(world, name));
            let types = ["RulesTypeNumber", "RulesTypeBool", "RulesTypeText", "RulesTypeEnum"]
                .map(|name| caption(world, name));
            let exact = labels[0].as_deref() == Some("false")
                && labels[1].as_deref() == Some("true")
                && types.each_ref().map(|text| text.as_deref())
                    == ["number", "bool", "text", "enum"].map(Some)
                && entity(world, "RulesBoolTrue")
                    .is_some_and(|button| layout::has_field_caption(world, button, "值"))
                && entity(world, "RulesDelete").is_none();
            if exact {
                run.layout.insert("boolean-value-buttons".into(), json!(labels));
                run.layout.insert("variable-type-buttons".into(), json!(types));
            }
            exact
        }
        CompactVariableScroll => layout::compact_variable_scroll(world, run, window)?,
        DeleteRow(id) => {
            let Some(slot) = row_slot(world, id) else {
                return Ok(None);
            };
            if !named_click(world, run, window, &format!("RulesDeleteRow{slot}")) {
                return Ok(None);
            }
            // Last 阶段刚释放实际按钮，还没有执行删除，这是逐帧检查的基准。
            run.deletion = Some(DeletionCheck {
                layout: deletion_layout(world, id).ok_or("删除前缺少列表布局")?,
                frames: 0,
                before_receipt: 0,
            });
            true
        }
        Deleted { id, variable } => {
            let check = run.deletion.as_mut().ok_or("缺少删除前的布局")?;
            let current = deletion_layout(world, id);
            if current.as_ref() != Some(&check.layout)
                || [
                    "RulesEditorDialog",
                    "RulesStatus",
                    "RulesListSave",
                    "RulesListCancel",
                ]
                .into_iter()
                .any(|name| entity(world, name).is_some())
            {
                return Err(format!("删除「{id}」第 {} 帧的工作台应留在原位，删除前 {:?}，当前 {current:?}", check.frames, check.layout));
            }
            check.frames += 1;
            let target_visible = row_slot(world, id).is_some();
            // 夹具 ID 独一无二，旧正文或未写入的修订都还没有本次删除回执。
            let target_key = format!("\"{id}\"");
            let source = match observe(world) {
                Some((source, false, _, status))
                    if status.contains("已保存") && !source.contains(&target_key) => source,
                _ => {
                    if !target_visible {
                        return Err(format!("删除「{id}」在写盘回执前隐藏了目标行"));
                    }
                    check.before_receipt += 1;
                    return Ok(None);
                }
            };
            let saved = fs::read_to_string(run.work.join("project/assets").join(PATH))
                .map_err(|e| e.to_string())?;
            let doc: Value = serde_json::from_str(&saved).map_err(|e| e.to_string())?;
            let (noun, kind, intact) = if variable {
                ("变量", "variable",
                    doc["variables"] == json!({"gate_open":{"default":true}}))
            } else {
                let rules = doc["rules"].as_array().ok_or("保存文件缺少规则")?;
                ("规则", "rule",
                    rules.iter().all(|rule| rule["id"] != id)
                        && ["guard_freed", "draw_at_three"].into_iter()
                            .all(|keep| rules.iter().any(|rule| rule["id"] == keep)))
            };
            let deleted = source == saved.replace("\r\n", "\n")
                && !target_visible
                && bottom_status(world).as_deref()
                    == Some(format!("已删除{noun}「{id}」").as_str())
                && intact;
            if deleted {
                if check.before_receipt == 0 {
                    return Err(format!("没有采集到删除「{id}」等待回执的帧"));
                }
                run.layout.insert(format!("{kind}-deleted-from-list"), json!(id));
                run.layout.insert(format!("{kind}-deletion-stable-frames"), json!(check.frames));
                run.layout.insert(format!("{kind}-deletion-target-before-receipt-frames"), json!(check.before_receipt));
                run.deletion = None;
            }
            deleted
        }
        TextWithoutChoices => {
            let confirmed = entity(world, "RulesTypeText").is_some_and(|button| {
                world.get::<BorderColor>(button)
                    == Some(&BorderColor::all(
                        feathrise_editor_ui::colors::COPPER_BORDER,
                    ))
            }) && entity(world, "RulesFieldDefault").is_some()
                && entity(world, "RulesFieldChoices").is_none();
            if confirmed {
                run.layout
                    .insert("plain-text-no-options".into(), json!(true));
            }
            confirmed
        }
        TryBackgroundTool(name) => {
            let Some(tab) = entity(world, name) else {
                return Ok(None);
            };
            // 常规点击需命中悬停。对话框打开时仍投递背景位置的实际窗口按下与释放，核对不穿透。
            super::super::raw_click(world, &mut run.pointer, window, tab)
        }
        VariableModalKept => {
            let retained = field_text(world, "RulesFieldId").as_deref() == Some("a")
                && field_text(world, "RulesFieldDefault").as_deref() == Some("3")
                && entity(world, "RulesEditorDialog").is_some()
                && layout::caption(world, "RulesPageTitle").as_deref() == Some("工程变量")
                && fs::read_to_string(run.work.join("project/assets").join(PATH))
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .is_some_and(|value| value["variables"]["a"].is_null());
            if retained {
                run.layout
                    .insert("variable-dialog-blocked-tool-switch".into(), json!(true));
            }
            retained
        }
        RuleToolbarReduced => {
            entity(world, "RulesUndo").is_none() && entity(world, "RulesSource").is_none()
        }
        RuleModalKept { editing } => {
            let title = layout::caption(world, "RulesEditorTitle");
            let expected_title = if editing { "编辑" } else { "新建" };
            let retained = field_text(world, "RulesFieldId").as_deref() == Some("guard_freed")
                && entity(world, "RulesEditorDialog").is_some()
                && layout::caption(world, "RulesPageTitle").as_deref() == Some("触发规则")
                && entity(world, "RulesUndo").is_none()
                && entity(world, "RulesSource").is_none()
                && entity(world, "RulesDelete").is_none()
                && title
                    .as_deref()
                    .is_some_and(|title| title.starts_with(expected_title));
            if retained {
                let sample = if editing {
                    "rule-edited-in-dialog"
                } else {
                    "rule-created-in-dialog"
                };
                run.layout.insert(sample.into(), json!(true));
            } else {
                run.layout_wait = format!("等待规则对话框保留背景点击，标题 {title:?}");
            }
            retained
        }
        EnumFields { reopened } => {
            let options = field_text(world, "RulesFieldChoices").map(|text| {
                text.split(',')
                    .map(str::trim)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            });
            let matches = field_text(world, "RulesFieldId").as_deref() == Some("delivery")
                && field_text(world, "RulesFieldDefault").as_deref() == Some("harbor")
                && options
                    .as_ref()
                    .is_some_and(|options| options == &["harbor", "mentor"])
                && entity(world, "RulesEditorDialog").is_some();
            if matches {
                let name = if reopened {
                    "enum-reread"
                } else {
                    "enum-editor"
                };
                run.layout
                    .insert(name.into(), json!({"default":"harbor","choices":options}));
            }
            matches
        }
        ShortWindow => {
            let mut primary = world.get_mut::<Window>(window).ok_or("缺少实窗")?;
            let width = primary.width();
            primary.set_maximized(false);
            primary.resolution.set(width, 600.);
            true
        }
        Stored(id) => {
            let Some((source, false, _, status)) = observe(world) else {
                return Ok(None);
            };
            if !status.contains("已保存") {
                return Ok(None);
            }
            let saved = fs::read_to_string(run.work.join("project/assets").join(PATH))
                .map_err(|e| e.to_string())?;
            if source != saved.replace("\r\n", "\n") {
                return Ok(None);
            }
            if id.is_empty() {
                true
            } else {
                let doc: Value = serde_json::from_str(&saved).map_err(|e| e.to_string())?;
                let (actual, expected) = match id {
                    "gate_open" => (&doc["variables"][id], json!({"default":true})),
                    "delete_me" => (&doc["variables"][id], json!({"default":false})),
                    "delete_rule" => (
                        doc["rules"].as_array().and_then(|rules| rules.iter().find(|rule| rule["id"] == id))
                            .unwrap_or(&Value::Null),
                        json!({"id":id,"when":{"event":"quest.completed","target":"dock_guard"},
                            "actions":[{"set":"dock_ready","value":false}]}),
                    ),
                    "a" => (&doc["variables"][id], json!({"default":3})),
                    "delivery" => (
                        &doc["variables"][id],
                        json!({"default":"harbor","choices":["harbor","mentor"]}),
                    ),
                    "drawn_gate" => (
                        &doc["locations"][id],
                        run.layout["location-fields"]["range"].clone(),
                    ),
                    _ => (
                        &doc["locations"][id],
                        json!({"scene":"river","min":[0,0,0],"max":[4,3,6]}),
                    ),
                };
                if actual != &expected {
                    return Ok(None);
                }
                let receipt = [bottom_status(world), layout::caption(world, "RulesStatus")]
                    .into_iter()
                    .flatten()
                    .find(|receipt| receipt.contains("已保存") && receipt.contains(id));
                let Some(receipt) = receipt else {
                    return Ok(None);
                };
                run.direct_saves
                    .insert(id.into(), json!({"saved":actual,"receipt":receipt}));
                if id == "a" {
                    run.saved_form = saved;
                }
                true
            }
        }
        NameFocused => entity(world, "RulesFieldId").is_some_and(|field| {
            world.resource::<InputFocus>().get() == Some(field)
                && world
                    .get::<EditableText>(field)
                    .is_some_and(|input| input.value() == "")
        }),
        Duplicate(cancelled) => {
            if cancelled {
                if entity(world, "RulesForm").is_some()
                    || row_slot(world, "a").is_none()
                    || bottom_status(world).as_deref() != Some("已取消本次修改")
                {
                    return Ok(None);
                }
                if entity(world, "RulesStatus").is_some() {
                    return Err("取消提示仍占用工程变量列表的布局".into());
                }
                run.layout.insert("variable-cancelled-in-status-bar".into(), json!(true));
            } else {
                let status = entity(world, "RulesStatus")
                    .and_then(|e| world.get::<Text>(e))
                    .map(|text| text.as_str().to_owned())
                    .unwrap_or_default();
                if !status.contains("已存在")
                    || field_text(world, "RulesFieldId").as_deref() != Some("a")
                {
                    let nodes: Vec<_> = world
                        .query::<(&Name, &Text, &ComputedNode, &InheritedVisibility)>()
                        .iter(world)
                        .filter(|(name, ..)| name.as_str() == "RulesStatus")
                        .map(|(_, text, size, visible)| {
                            (text.as_str().to_owned(), size.size(), visible.get())
                        })
                        .collect();
                    let name = field_text(world, "RulesFieldId");
                    run.layout_wait =
                        format!("重复名称仍应留在对话框，当前名称 {name:?}，状态实体 {nodes:?}");
                    return Ok(None);
                }
                run.diagnostic = Some(status);
            }
            let disk = fs::read_to_string(run.work.join("project/assets").join(PATH))
                .map_err(|e| e.to_string())?;
            if disk != run.saved_form {
                return Err("重复变量保存失败或取消修改改变了先前保存的文件".into());
            }
            true
        }
    };
    Ok(done.then_some(Stage::Form(index + 1)))
}

pub(super) fn describe(phase: &str, index: usize) -> String {
    format!("{:?}", steps(phase).get(index))
}
