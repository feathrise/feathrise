//! 真实版本一文档的表单往返、原始拼写保护、引用迁移及错误回滚。
use super::*;
use serde_json::{Value, json};

fn variable(id: &str, kind: ValueKind, default: &str) -> VariableForm {
    VariableForm {
        id: id.into(),
        kind,
        default: default.into(),
        choices: None,
    }
}
fn rule(id: &str) -> RuleForm {
    RuleForm {
        id: id.into(),
        when: TriggerForm::Event {
            event: "quest.completed".into(),
            target: None,
        },
        once: false,
        conditions: Vec::new(),
        actions: vec![ActionForm::Request {
            request: "cinematic.play".into(),
            target: "古渡".into(),
        }],
    }
}
fn doc(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn source_order_and_untouched_lexemes_survive_edits_renames_and_noops() {
    let src=r#"{  "version" : 1,  "locations": {}, "variables" : { "z" : {"default": 1.000e+0}, "\u0061" : { "choices":null, "default":"\u4e2d" } },
 "rules" : [{ "once":false, "id":"保留", "when":{"event":"other","target":null}, "if":[], "actions":[ { "request":"play","target":"\u4e2d" } ] }] }"#.replace('\n',"\r\n");
    let parsed = RulesForm::parse(&src).unwrap();
    assert_eq!(
        parsed
            .variables
            .iter()
            .map(|v| v.id.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
    let mut item = parsed.variables[0].clone();
    assert_eq!(
        apply(
            &src,
            RuleEdit::Variable {
                original: Some("z".into()),
                value: item.clone()
            }
        )
        .unwrap(),
        src
    );
    item.default = "2".into();
    let changed = apply(
        &src,
        RuleEdit::Variable {
            original: Some("z".into()),
            value: item.clone(),
        },
    )
    .unwrap();
    assert_eq!(changed, src.replace("1.000e+0", "2"));
    item.id = "总数".into();
    let renamed = apply(
        &changed,
        RuleEdit::Variable {
            original: Some("z".into()),
            value: item,
        },
    )
    .unwrap();
    assert_eq!(renamed, changed.replace("\"z\"", "\"总数\""));
}

#[test]
fn variable_rename_follows_typed_references_only_and_rejects_breaking_edits() {
    let src = r#"{"version":1,"variables":{"flag":{"default":false},"other":{"default":"flag"}},"rules":[
        {"id":"watch","when":{"changed":"flag","from":false,"to":true},"if":[{"variable":"flag","equals":true}],
         "actions":[{"set":"flag","value":false},{"request":"flag","target":"flag"},{"set":"other","value":"flag"}]}]}"#;
    let renamed = apply(
        src,
        RuleEdit::Variable {
            original: Some("flag".into()),
            value: variable("已交谈", ValueKind::Bool, "真"),
        },
    )
    .unwrap();
    let r = doc(&renamed);
    assert_eq!(r["variables"]["已交谈"]["default"], true);
    assert_eq!(
        r["rules"][0]["when"],
        json!({"changed":"已交谈","from":false,"to":true})
    );
    assert_eq!(r["rules"][0]["if"][0]["variable"], "已交谈");
    assert_eq!(
        r["rules"][0]["actions"],
        json!([{"set":"已交谈","value":false},{"request":"flag","target":"flag"},{"set":"other","value":"flag"}])
    );
    for edit in [
        RuleEdit::RemoveVariable("flag".into()),
        RuleEdit::Variable {
            original: Some("flag".into()),
            value: variable("flag", ValueKind::Text, "false"),
        },
    ] {
        assert!(apply(src, edit).unwrap_err().contains("watch"));
    }
    assert!(
        apply(
            src,
            RuleEdit::Variable {
                original: Some("flag".into()),
                value: variable("other", ValueKind::Bool, "true")
            }
        )
        .unwrap_err()
        .contains("已存在")
    );
}

#[test]
fn actions_conditions_and_rules_keep_authored_order_and_scalar_types() {
    let src = r#"{"version":1,"variables":{"n":{"default":1},"s":{"default":"","choices":["","1"]}},"rules":[
      {"id":"a","when":{"changed":"s","to":"","from":null},"once":false,"if":[{"variable":"s","equals":""},{"variable":"n","equals":1e0}],
       "actions":[ { "set":"n", "value":1.00 }, { "set":"s", "value":"1" } ]},
      {"id":"b","when":{"event":"other"},"actions":[{"request":"play","target":"b"}]}]}"#;
    let mut a = RulesForm::parse(src).unwrap().rules.remove(0);
    assert_eq!(
        a.when,
        TriggerForm::Changed {
            variable: "s".into(),
            from: None,
            to: Some(String::new())
        }
    );
    a.conditions.swap(0, 1);
    a.actions.swap(0, 1);
    a.once = true;
    let out = apply(
        src,
        RuleEdit::Rule {
            original: Some("a".into()),
            value: a.clone(),
        },
    )
    .unwrap();
    assert!(out.contains(r#"{ "set":"s", "value":"1" }, { "set":"n", "value":1.00 }"#));
    assert!(out.contains(r#""equals":1e0"#) && out.contains(r#""from":null"#));
    let moved = apply(
        &out,
        RuleEdit::MoveRule {
            id: "b".into(),
            to: 0,
        },
    )
    .unwrap();
    assert_eq!(doc(&moved)["rules"][0]["id"], "b");
    a.when = TriggerForm::Event {
        event: "custom.event".into(),
        target: None,
    };
    a.conditions.clear();
    let out = apply(
        &moved,
        RuleEdit::Rule {
            original: Some("a".into()),
            value: a.clone(),
        },
    )
    .unwrap();
    let r = &doc(&out)["rules"][1];
    assert_eq!(r["when"], json!({"event":"custom.event"}));
    assert!(r.get("if").is_none());
    a.actions.clear();
    assert!(
        apply(
            &out,
            RuleEdit::Rule {
                original: Some("a".into()),
                value: a
            }
        )
        .unwrap_err()
        .contains("动作")
    );
}

#[test]
fn locations_and_events_share_only_explicit_location_targets() {
    let src = r#"{"version":1,"rules":[{"id":"a","when":{"event":"location.entered","target":"gate"},"actions":[{"request":"custom","target":"gate"}]},
        {"id":"b","when":{"event":"quest.accepted","target":"gate"},"actions":[{"request":"custom","target":"gate"}]}]}"#;
    let mut area = LocationForm {
        id: "gate".into(),
        scene: "汴河".into(),
        min: ["-1".into(), "0".into(), "0".into()],
        max: ["4".into(), "3".into(), "2".into()],
    };
    let src = apply(
        src,
        RuleEdit::Location {
            original: None,
            value: area.clone(),
        },
    )
    .unwrap();
    assert!(
        apply(&src, RuleEdit::RemoveLocation("gate".into()))
            .unwrap_err()
            .contains("规则「a」")
    );
    area.id = "城门".into();
    let out = apply(
        &src,
        RuleEdit::Location {
            original: Some("gate".into()),
            value: area.clone(),
        },
    )
    .unwrap();
    let row = doc(&out);
    assert_eq!(row["rules"][0]["when"]["target"], "城门");
    assert_eq!(row["rules"][0]["actions"][0]["target"], "gate");
    assert_eq!(row["rules"][1]["when"]["target"], "gate");
    area.max[2] = "0".into();
    assert!(
        apply(
            &out,
            RuleEdit::Location {
                original: Some("城门".into()),
                value: area
            }
        )
        .unwrap_err()
        .contains("min")
    );
    let less = apply(&out, RuleEdit::RemoveRule("a".into())).unwrap();
    let less = apply(&less, RuleEdit::RemoveLocation("城门".into())).unwrap();
    assert_eq!(doc(&less)["locations"], json!({}));
}

#[test]
fn blank_legacy_document_adds_first_complete_rule_and_checks_typed_inputs() {
    let src = "{\"version\":1}\n";
    let mut option = variable("收件人", ValueKind::Text, "");
    option.choices = Some(vec!["".into(), "a,b".into(), " x ".into()]);
    let src = apply(
        src,
        RuleEdit::Variable {
            original: None,
            value: option.clone(),
        },
    )
    .unwrap();
    assert_eq!(RulesForm::parse(&src).unwrap().variables, [option.clone()]);
    let mut r = rule("first");
    r.actions.push(ActionForm::Set {
        variable: "收件人".into(),
        value: " x ".into(),
    });
    let src = apply(
        &src,
        RuleEdit::Rule {
            original: None,
            value: r,
        },
    )
    .unwrap();
    option.choices = Some(vec!["".into()]);
    assert!(
        apply(
            &src,
            RuleEdit::Variable {
                original: Some("收件人".into()),
                value: option
            }
        )
        .unwrap_err()
        .contains("first")
    );
    for (kind, input) in [
        (ValueKind::Number, "NaN"),
        (ValueKind::Number, "1e40"),
        (ValueKind::Bool, "是的"),
    ] {
        assert!(
            apply(
                &src,
                RuleEdit::Variable {
                    original: None,
                    value: variable("invalid", kind, input)
                }
            )
            .is_err()
        );
    }
    let duplicates = r#"{"version":1,"variables":{"a":{"default":1},"\u0061":{"default":2}}}"#;
    assert!(super::validate(duplicates).unwrap_err().contains("重复"));
    assert!(RulesForm::parse(duplicates).unwrap_err().contains("重复"));
    assert!(RulesForm::parse(EXAMPLE).is_ok());
}
