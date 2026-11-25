//! 使用表单的实际按钮和原生输入编辑规则。每一步只在对应的控件或草稿结果已出现后推进。
use super::*;
use bevy::text::EditableText;

#[derive(Clone, Copy, Debug)]
enum Step {
    Button(&'static str),
    Field(&'static str, &'static str),
    Selected(&'static str),
    Check(usize),
    NumberVariable,
    NumberTrigger,
    Shortcut(KeyCode),
    Saved,
}
use Step::*;

const EDIT: &[Step] = &[
    Button("RulesTabVariables"),
    Button("RulesNew"),
    Field("RulesFieldId", "dock_ready"),
    Button("RulesApply"),
    Check(0),
    Button("RulesTabRules"),
    Button("RulesNew"),
    Field("RulesFieldId", "guard_freed"),
    Field("RulesFieldTrigger", "quest.completed"),
    Field("RulesFieldEventTarget", "dock_guard"),
    Button("RulesOnce"),
    Field("RulesFieldConditionVariable", "dock_ready"),
    Field("RulesFieldConditionValue", "false"),
    Field("RulesFieldActionName", "dock_ready"),
    Field("RulesFieldActionValue", "true"),
    Button("RulesApply"),
    Check(1),
    Button("RulesActionType"),
    Field("RulesFieldActionName", "cinematic.play"),
    Field("RulesFieldActionValue", "guard-unseal"),
    Button("RulesActionAdd"),
    Check(2),
    Shortcut(KeyCode::KeyZ),
    Check(1),
    Shortcut(KeyCode::KeyY),
    Check(2),
    Button("RulesTabVariables"),
    Button("RulesNew"),
    Field("RulesFieldId", "a"),
    Button("RulesType"),
    Button("RulesApply"),
    NumberVariable,
    Button("RulesTabRules"),
    Button("RulesNew"),
    Field("RulesFieldId", "draw_at_three"),
    Button("RulesTrigger"),
    Field("RulesFieldTrigger", "a"),
    Button("RulesTo"),
    Field("RulesFieldTo", "3"),
    Button("RulesActionType"),
    Button("RulesSuggestActionName"),
    Button("RulesOption0"),
    Field("RulesFieldActionValue", "draw-river-sword"),
    Button("RulesApply"),
    NumberTrigger,
    Button("RulesSave"),
    Saved,
];

const REOPEN: &[Step] = &[
    Button("RulesTabRules"),
    Selected("guard_freed"),
    Check(2),
    Selected("draw_at_three"),
    NumberTrigger,
];

pub(super) fn entity(world: &mut World, id: &str) -> Option<Entity> {
    visible(world, |name: &Name| name.as_str() == id)
}

pub(super) fn named_click(world: &mut World, run: &mut Run, window: Entity, id: &str) -> bool {
    let button = entity(world, id);
    click(world, &mut run.pointer, window, button)
}

fn field_text(world: &mut World, id: &str) -> Option<String> {
    let input = entity(world, id)?;
    world
        .get::<EditableText>(input)
        .map(|e| e.value().to_string())
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

pub(super) fn form(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    index: usize,
) -> Result<Option<Stage>, String> {
    let reopening = run.phase == "reopen";
    let script = if reopening { REOPEN } else { EDIT };
    let Some(step) = script.get(index) else {
        return Ok(Some(Stage::Capture));
    };
    let done = match *step {
        Button(id) => named_click(world, run, window, id),
        Field(id, value) => {
            let Some(e) = entity(world, id) else {
                return Ok(None);
            };
            replace(world, &mut run.pointer, &mut run.keys, window, e, value)
        }
        Selected(id) => {
            if field_text(world, "RulesFieldId").as_deref() != Some(id) {
                named_click(world, run, window, "RulesNext");
                return Ok(None);
            }
            true
        }
        Shortcut(code) => {
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
                    }) && field_text(world, "RulesFieldTrigger").as_deref() == Some("a")
                        && field_text(world, "RulesFieldTo").as_deref() == Some("3")
                        && field_text(world, "RulesFieldActionName").as_deref()
                            == Some("cinematic.play")))
        }
        Check(count) => {
            let Some((source, dirty, ..)) = observe(world) else {
                return Ok(None);
            };
            let value: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
            if dirty == reopening || value["variables"]["dock_ready"]["default"] != false {
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
    };
    Ok(done.then_some(Stage::Form(index + 1)))
}

pub(super) fn describe(reopen: bool, index: usize) -> String {
    format!("{:?}", if reopen { REOPEN } else { EDIT }.get(index))
}
