//! 原生表单与版本一规则文档之间的映射。输入保留文本语义，直到应用时才按变量声明解析。
use super::{source, validate};
use serde_json::{Map, Number, Value, json};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Number,
    Text,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableForm {
    pub id: String,
    pub kind: ValueKind,
    pub default: String,
    pub choices: Option<Vec<String>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocationForm {
    pub id: String,
    pub scene: String,
    pub min: [String; 3],
    pub max: [String; 3],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriggerForm {
    Event {
        event: String,
        target: Option<String>,
    },
    Changed {
        variable: String,
        from: Option<String>,
        to: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConditionForm {
    pub variable: String,
    pub equals: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionForm {
    Set { variable: String, value: String },
    Request { request: String, target: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleForm {
    pub id: String,
    pub when: TriggerForm,
    pub once: bool,
    pub conditions: Vec<ConditionForm>,
    pub actions: Vec<ActionForm>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RulesForm {
    pub variables: Vec<VariableForm>,
    pub rules: Vec<RuleForm>,
    pub locations: Vec<LocationForm>,
}

fn input(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn optional(value: &Value) -> Option<String> {
    (!value.is_null()).then(|| input(value))
}
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}

impl RulesForm {
    pub fn parse(source: &str) -> Result<Self, String> {
        validate(source)?;
        let document: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
        let mut form = Self::default();
        for id in source::keys(source, "variables")? {
            let v = &document["variables"][&id];
            let kind = match v["default"] {
                Value::Bool(_) => ValueKind::Bool,
                Value::Number(_) => ValueKind::Number,
                _ => ValueKind::Text,
            };
            form.variables.push(VariableForm {
                id,
                kind,
                default: input(&v["default"]),
                choices: v["choices"]
                    .as_array()
                    .map(|a| a.iter().map(input).collect()),
            });
        }
        for id in source::keys(source, "locations")? {
            let v = &document["locations"][&id];
            let axes = |key: &str| std::array::from_fn(|i| input(&v[key][i]));
            form.locations.push(LocationForm {
                id,
                scene: input(&v["scene"]),
                min: axes("min"),
                max: axes("max"),
            });
        }
        for rule in rows(&document["rules"]) {
            let w = &rule["when"];
            let when = if let Some(name) = w["event"].as_str() {
                TriggerForm::Event {
                    event: name.into(),
                    target: optional(&w["target"]),
                }
            } else {
                TriggerForm::Changed {
                    variable: input(&w["changed"]),
                    from: optional(&w["from"]),
                    to: optional(&w["to"]),
                }
            };
            let conditions = rows(&rule["if"])
                .iter()
                .map(|v| ConditionForm {
                    variable: input(&v["variable"]),
                    equals: input(&v["equals"]),
                })
                .collect();
            let actions = rows(&rule["actions"])
                .iter()
                .map(|v| {
                    if let Some(set) = v["set"].as_str() {
                        ActionForm::Set {
                            variable: set.into(),
                            value: input(&v["value"]),
                        }
                    } else {
                        ActionForm::Request {
                            request: input(&v["request"]),
                            target: input(&v["target"]),
                        }
                    }
                })
                .collect();
            form.rules.push(RuleForm {
                id: input(&rule["id"]),
                when,
                once: rule["once"].as_bool().unwrap_or(false),
                conditions,
                actions,
            });
        }
        Ok(form)
    }

    pub(super) fn json(&self) -> Result<Value, String> {
        let kinds: HashMap<_, _> = self
            .variables
            .iter()
            .map(|v| (v.id.as_str(), v.kind))
            .collect();
        let mut variables = Map::new();
        for v in &self.variables {
            let mut def = json!({"default":v.kind.parse(&v.default).map_err(|e|format!("变量「{}」默认值：{e}",v.id))?});
            if let Some(options) = &v.choices {
                def["choices"] = json!(options);
            }
            variables.insert(v.id.clone(), def);
        }
        let mut locations = Map::new();
        for v in &self.locations {
            let axes = |field: &str, values: &[String; 3]| {
                values
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        ValueKind::Number.parse(s).map_err(|e| {
                            format!("地点「{}」{field} 的 {} 轴：{e}", v.id, ["X", "Y", "Z"][i])
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            };
            locations.insert(
                v.id.clone(),
                json!({"scene":v.scene,"min":axes("min",&v.min)?,"max":axes("max",&v.max)?}),
            );
        }
        let mut rules = Vec::new();
        for rule in &self.rules {
            let typed = |key: &str, text: &str| {
                let kind = kinds
                    .get(key)
                    .ok_or_else(|| format!("规则「{}」引用了未声明的变量「{key}」", rule.id))?;
                kind.parse(text)
                    .map_err(|e| format!("规则「{}」变量「{key}」：{e}", rule.id))
            };
            let when = match &rule.when {
                TriggerForm::Event { event, target } => {
                    let mut w = json!({"event":event});
                    if let Some(t) = target {
                        w["target"] = json!(t);
                    }
                    w
                }
                TriggerForm::Changed { variable, from, to } => {
                    let mut w = json!({"changed":variable});
                    for (field, datum) in [("from", from), ("to", to)] {
                        if let Some(text) = datum {
                            w[field] = typed(variable, text)?;
                        }
                    }
                    w
                }
            };
            let mut actions = Vec::new();
            for action in &rule.actions {
                actions.push(match action {
                    ActionForm::Set { variable, value } => {
                        json!({"set":variable,"value":typed(variable,value)?})
                    }
                    ActionForm::Request { request, target } => {
                        json!({"request":request,"target":target})
                    }
                });
            }
            let mut v = json!({"id":rule.id,"when":when,"actions":actions});
            if rule.once {
                v["once"] = json!(true);
            }
            if !rule.conditions.is_empty() {
                let mut tests = Vec::new();
                for c in &rule.conditions {
                    tests.push(
                        json!({"variable":c.variable,"equals":typed(&c.variable,&c.equals)?}),
                    );
                }
                v["if"] = Value::Array(tests);
            }
            rules.push(v);
        }
        Ok(json!({"version":1,"variables":variables,"locations":locations,"rules":rules}))
    }
}

impl ValueKind {
    fn parse(self, raw: &str) -> Result<Value, String> {
        match self {
            Self::Text => Ok(Value::String(raw.into())),
            Self::Bool => match raw.trim() {
                "true" | "真" => Ok(Value::Bool(true)),
                "false" | "假" => Ok(Value::Bool(false)),
                _ => Err("布尔值请填 true 或 false（真或假）".into()),
            },
            Self::Number => {
                let text = raw.trim();
                let n = text
                    .parse::<f64>()
                    .ok()
                    .filter(|v| (*v as f32).is_finite())
                    .ok_or_else(|| format!("「{raw}」不是有限的 32 位数字"))?;
                // JSON 形式可直接保留整数精度；输入框的 +1 或 .5 也转成合法 JSON 数字。
                let number = text
                    .parse::<Number>()
                    .ok()
                    .or_else(|| Number::from_f64(n))
                    .unwrap();
                Ok(Value::Number(number))
            }
        }
    }
}
