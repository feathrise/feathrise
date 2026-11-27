//! 通过演出设置的原生控件新增播放条件与收尾动作，并验证字幕等待及独立进程回读。

use super::super::policy::{
    Action as PolicyAction, ConditionKind, Field as PolicyField, FinishKind, Repeat, ValueKind,
};
use super::*;
use view::{Action as UiAction, Field as UiField};

enum Step {
    Action(UiAction),
    Field(UiField, &'static str),
    Capture,
    Applied,
    Saved,
    Complete,
}
use Step::*;
const STEPS: &[Step] = &[
    Action(UiAction::Settings),
    Action(UiAction::Policy(PolicyAction::Repeat(Repeat::Quest))),
    Field(UiField::Policy(PolicyField::RepeatQuest), "guard_delivery"),
    Action(UiAction::Policy(PolicyAction::ConditionAdd)),
    Field(
        UiField::Policy(PolicyField::ConditionTarget),
        "guard_delivery",
    ),
    Field(UiField::Policy(PolicyField::Status), "active"),
    Field(UiField::Policy(PolicyField::Stage), "1"),
    Action(UiAction::Policy(PolicyAction::ConditionAdd)),
    Action(UiAction::Policy(PolicyAction::Condition(
        ConditionKind::Item,
    ))),
    Field(UiField::Policy(PolicyField::ConditionTarget), "name:守印剑"),
    Action(UiAction::Policy(PolicyAction::ConditionAdd)),
    Action(UiAction::Policy(PolicyAction::Condition(
        ConditionKind::Seen,
    ))),
    Field(UiField::Policy(PolicyField::ConditionTarget), "unseal"),
    Action(UiAction::Policy(PolicyAction::Seen)),
    Action(UiAction::Policy(PolicyAction::ConditionAdd)),
    Action(UiAction::Policy(PolicyAction::ConditionDelete)),
    Action(UiAction::Policy(PolicyAction::ConditionPrevious)),
    Action(UiAction::Policy(PolicyAction::ConditionNext)),
    Action(UiAction::Policy(PolicyAction::FinishAdd)),
    Field(UiField::Policy(PolicyField::FinishTarget), "gate_open"),
    Field(UiField::Policy(PolicyField::Value), "3"),
    Action(UiAction::Policy(PolicyAction::FinishAdd)),
    Action(UiAction::Policy(PolicyAction::Value(ValueKind::Bool))),
    Field(UiField::Policy(PolicyField::FinishTarget), "cinematic_done"),
    Action(UiAction::Policy(PolicyAction::Bool)),
    Action(UiAction::Policy(PolicyAction::FinishAdd)),
    Action(UiAction::Policy(PolicyAction::Value(ValueKind::Text))),
    Field(UiField::Policy(PolicyField::FinishTarget), "sword_route"),
    Field(UiField::Policy(PolicyField::Value), "guard"),
    Action(UiAction::Policy(PolicyAction::FinishAdd)),
    Action(UiAction::Policy(PolicyAction::Finish(FinishKind::Quest))),
    Field(UiField::Policy(PolicyField::FinishTarget), "guard_delivery"),
    Field(UiField::Policy(PolicyField::From), "1"),
    Field(UiField::Policy(PolicyField::To), "2"),
    Action(UiAction::Policy(PolicyAction::FinishAdd)),
    Action(UiAction::Policy(PolicyAction::FinishDelete)),
    Action(UiAction::Policy(PolicyAction::FinishPrevious)),
    Action(UiAction::Policy(PolicyAction::FinishNext)),
    Capture,
    Action(UiAction::ApplySettings),
    Applied,
    Action(UiAction::Save),
    Saved,
    Action(UiAction::Lane(Lane::Dialogue)),
    Field(UiField::At, "0"),
    Action(UiAction::Go),
    Action(UiAction::Previous),
    Action(UiAction::Wait),
    Action(UiAction::Replace),
    Action(UiAction::Save),
    Saved,
    Complete,
];

pub(super) fn check_policy(film: &feathrise_cinematic::Cinematic) -> Result<(), String> {
    let expected = json!({
        "repeat":{"once_per_quest":{"quest":"guard_delivery"}},
        "conditions":[
            {"kind":"quest","quest":"guard_delivery","status":"active","stage":1},
            {"kind":"has_item","item":"name:守印剑"},
            {"kind":"seen","cinematic":"unseal","seen":true}
        ],
        "on_finish":[
            {"kind":"set_variable","key":"gate_open","value":3.0},
            {"kind":"set_variable","key":"cinematic_done","value":true},
            {"kind":"set_variable","key":"sword_route","value":"guard"},
            {"kind":"quest_objective","quest":"guard_delivery","from":1,"to":2}
        ]
    });
    let actual =
        json!({"repeat":film.repeat,"conditions":film.conditions,"on_finish":film.on_finish});
    if actual != expected {
        return Err(format!("可视演出设置与作者输入不符：{actual}"));
    }
    Ok(())
}

pub(super) fn step(world: &mut World, r: &mut Run, w: Entity) -> Result<bool, String> {
    if !stable(world) {
        return Ok(false);
    }
    let Some(step) = STEPS.get(r.stage - 42) else {
        return Err("未知演出设置验收阶段".into());
    };
    match step {
        Action(a) => Ok(action(world, r, w, *a)),
        Field(field, value) => {
            let Some(entity) = visible(world, |f: &view::Field| f == field) else {
                return Ok(false);
            };
            if !reveal_rules_control(world, w, entity, &mut r.wheel_point) {
                return Ok(false);
            }
            Ok(set(world, *field, value))
        }
        Capture => {
            let Some(shot) = evidence::capture(
                world,
                &mut r.capture,
                &r.work.join("cinematic-settings.png"),
            )?
            else {
                return Ok(false);
            };
            r.policy_shot = Some(shot);
            Ok(true)
        }
        Applied => {
            check_policy(&world.resource::<Studio>().timeline.as_ref().unwrap().film)?;
            Ok(true)
        }
        Saved => Ok(world
            .resource::<DocumentState>()
            .current()
            .is_some_and(|doc| !doc.dirty() && doc.status.contains("已保存"))),
        Complete => {
            let film = &world.resource::<Studio>().timeline.as_ref().unwrap().film;
            check_policy(film)?;
            if !film.dialogue[0].wait {
                return Err("字幕等待控件未写入演出".into());
            }
            finish(world, r, None);
            Ok(false)
        }
    }
}
