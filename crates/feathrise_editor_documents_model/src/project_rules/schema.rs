//! 作者保存前核对版本 1 的声明与引用。执行事件与动作属于工程运行时。
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    #[serde(default)]
    variables: BTreeMap<String, Variable>,
    #[serde(default)]
    locations: BTreeMap<String, Location>,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    scene: String,
    min: [f32; 3],
    max: [f32; 3],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variable {
    default: Value,
    choices: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    id: String,
    when: When,
    #[serde(default, rename = "if")]
    conditions: Vec<Test>,
    #[serde(default, rename = "once")]
    _once: bool,
    actions: Vec<Action>,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum When {
    Changed {
        changed: String,
        from: Option<Value>,
        to: Option<Value>,
    },
    Event {
        event: String,
        target: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Test {
    variable: String,
    equals: Value,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Action {
    Set { set: String, value: Value },
    Request { request: String, target: String },
}

impl Variable {
    fn accepts(&self, value: &Value) -> bool {
        match (&self.default, value) {
            (Value::Bool(_), Value::Bool(_)) => true,
            (Value::Number(_), Value::Number(number)) => {
                number.as_f64().is_some_and(|n| (n as f32).is_finite())
            }
            (Value::String(_), Value::String(text)) => self
                .choices
                .as_ref()
                .is_none_or(|options| options.contains(text)),
            _ => false,
        }
    }
}

pub fn validate(source: &str) -> Result<(), String> {
    let file: File =
        serde_json::from_str(source).map_err(|error| format!("工程规则结构无效：{error}"))?;
    if file.version != 1 {
        return Err("工程规则 version 需要为 1".into());
    }
    for (id, area) in &file.locations {
        if id.trim().is_empty() || area.scene.trim().is_empty() {
            return Err(format!("地点「{id}」需要非空的标识和场景名"));
        }
        if (0..3).any(|axis| {
            !area.min[axis].is_finite()
                || !area.max[axis].is_finite()
                || area.min[axis] >= area.max[axis]
        }) {
            return Err(format!(
                "地点「{id}」的 min/max 每轴须为有限数值，且 min 小于 max"
            ));
        }
    }
    for (key, variable) in &file.variables {
        if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(format!("变量名「{key}」须由字母、数字或下划线组成"));
        }
        if let Some(choices) = &variable.choices {
            if !variable.default.is_string()
                || choices.iter().collect::<HashSet<_>>().len() != choices.len()
            {
                return Err(format!(
                    "变量「{key}」的 choices 需要不重复的字符串选项和字符串默认值"
                ));
            }
        }
        if !variable.accepts(&variable.default) {
            return Err(format!("变量「{key}」的 default 类型或选项无效"));
        }
    }
    let mut ids = HashSet::new();
    for (index, rule) in file.rules.iter().enumerate() {
        if rule.id.trim().is_empty() || !ids.insert(&rule.id) {
            return Err(format!("第 {} 条规则的 id 为空或重复", index + 1));
        }
        let check = |key: &str, values: &[&Value]| -> Result<(), String> {
            let variable = file
                .variables
                .get(key)
                .ok_or_else(|| format!("规则「{}」引用了未声明的变量「{key}」", rule.id))?;
            if values.iter().any(|value| !variable.accepts(value)) {
                return Err(format!(
                    "规则「{}」中的「{key}」值与声明的类型或选项不符",
                    rule.id
                ));
            }
            Ok(())
        };
        match &rule.when {
            When::Changed { changed, from, to } => {
                check(changed, &[])?;
                for value in [from, to].into_iter().flatten() {
                    check(changed, &[value])?;
                }
            }
            When::Event { event, target }
                if event.trim().is_empty()
                    || target.as_ref().is_some_and(|t| t.trim().is_empty()) =>
            {
                return Err(format!("规则「{}」的事件名或事件目标为空", rule.id));
            }
            When::Event { .. } => {}
        }
        for condition in &rule.conditions {
            check(&condition.variable, &[&condition.equals])?;
        }
        if rule.actions.is_empty() {
            return Err(format!("规则「{}」至少需要一个动作", rule.id));
        }
        for action in &rule.actions {
            match action {
                Action::Set { set, value } => check(set, &[value])?,
                Action::Request { request, target }
                    if request.trim().is_empty() || target.trim().is_empty() =>
                {
                    return Err(format!("规则「{}」的请求名与目标均须填写", rule.id));
                }
                Action::Request { .. } => {}
            }
        }
    }
    Ok(())
}
