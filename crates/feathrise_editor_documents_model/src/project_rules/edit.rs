//! 每次表单操作生成一个可校验的文档候选；同文档引用及其声明共同成功或共同撤回。
use super::{
    ActionForm, LocationForm, RuleForm, RulesForm, TriggerForm, VariableForm, source, validate,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleEdit {
    Variable {
        original: Option<String>,
        value: VariableForm,
    },
    Rule {
        original: Option<String>,
        value: RuleForm,
    },
    Location {
        original: Option<String>,
        value: LocationForm,
    },
    RemoveVariable(String),
    RemoveRule(String),
    RemoveLocation(String),
    MoveRule {
        id: String,
        to: usize,
    },
}
fn upsert<T>(
    rows: &mut Vec<T>,
    original: Option<&str>,
    value: T,
    id: fn(&T) -> &str,
) -> Result<(), String> {
    let name = id(&value);
    if Some(name) != original && rows.iter().any(|r| id(r) == name) {
        return Err(format!("标识「{name}」已存在，请使用不同的标识"));
    }
    if let Some(old) = original {
        let row = rows
            .iter_mut()
            .find(|r| id(r) == old)
            .ok_or_else(|| format!("原来的条目「{old}」已不存在，请重新选择"))?;
        *row = value;
    } else {
        rows.push(value);
    }
    Ok(())
}
fn index<T>(rows: &[T], key: &str, id: fn(&T) -> &str) -> Result<usize, String> {
    rows.iter()
        .position(|r| id(r) == key)
        .ok_or_else(|| format!("找不到条目「{key}」"))
}
fn uses_variable(rule: &RuleForm, key: &str) -> bool {
    matches!(&rule.when,TriggerForm::Changed{variable,..} if variable==key)
        || rule.conditions.iter().any(|c| c.variable == key)
        || rule
            .actions
            .iter()
            .any(|a| matches!(a,ActionForm::Set{variable,..} if variable==key))
}
fn location_target(rule: &mut RuleForm) -> Option<&mut String> {
    match &mut rule.when {
        TriggerForm::Event {
            event,
            target: Some(target),
        } if matches!(event.as_str(), "location.entered" | "location.exited") => Some(target),
        _ => None,
    }
}

pub fn apply(source: &str, edit: RuleEdit) -> Result<String, String> {
    let before = RulesForm::parse(source)?;
    let mut after = before.clone();
    let mut rename = None;
    match edit {
        RuleEdit::Variable { original, value } => {
            if let Some(old) = original.as_deref() {
                let previous = &after.variables[index(&after.variables, old, |v| &v.id)?];
                for rule in &mut after.rules {
                    if !uses_variable(rule, old) {
                        continue;
                    }
                    if previous.kind != value.kind {
                        return Err(format!(
                            "规则「{}」仍引用变量「{old}」。请先调整该规则，再更换变量类型",
                            rule.id
                        ));
                    }
                    if old != value.id {
                        let replace = |v: &mut String| {
                            if v == old {
                                v.clone_from(&value.id);
                            }
                        };
                        if let TriggerForm::Changed { variable, .. } = &mut rule.when {
                            replace(variable);
                        }
                        for c in &mut rule.conditions {
                            replace(&mut c.variable);
                        }
                        for a in &mut rule.actions {
                            if let ActionForm::Set { variable, .. } = a {
                                replace(variable);
                            }
                        }
                    }
                }
                if old != value.id {
                    rename = Some(("variables", old.to_owned(), value.id.clone()));
                }
            }
            upsert(&mut after.variables, original.as_deref(), value, |v| &v.id)?;
        }
        RuleEdit::Location { original, value } => {
            if let Some(old) = original.as_deref().filter(|o| *o != value.id) {
                for r in &mut after.rules {
                    if let Some(target) = location_target(r).filter(|t| t.as_str() == old) {
                        target.clone_from(&value.id);
                    }
                }
                rename = Some(("locations", old.to_owned(), value.id.clone()));
            }
            upsert(&mut after.locations, original.as_deref(), value, |v| &v.id)?;
        }
        RuleEdit::Rule { original, value } => {
            upsert(&mut after.rules, original.as_deref(), value, |v| &v.id)?
        }
        RuleEdit::RemoveVariable(key) => {
            if let Some(r) = after.rules.iter().find(|r| uses_variable(r, &key)) {
                return Err(format!(
                    "规则「{}」正在使用变量「{key}」，请先移除该引用",
                    r.id
                ));
            }
            let i = index(&after.variables, &key, |v| &v.id)?;
            after.variables.remove(i);
        }
        RuleEdit::RemoveLocation(key) => {
            for r in &mut after.rules {
                if location_target(r).is_some_and(|t| t == &key) {
                    return Err(format!(
                        "规则「{}」正在监听地点「{key}」，请先移除该引用",
                        r.id
                    ));
                }
            }
            let i = index(&after.locations, &key, |v| &v.id)?;
            after.locations.remove(i);
        }
        RuleEdit::RemoveRule(key) => {
            let i = index(&after.rules, &key, |v| &v.id)?;
            after.rules.remove(i);
        }
        RuleEdit::MoveRule { id, to } => {
            let i = index(&after.rules, &id, |v| &v.id)?;
            if to >= after.rules.len() {
                return Err("目标规则位置不存在".into());
            }
            let saved = after.rules.remove(i);
            after.rules.insert(to, saved);
        }
    }
    if before == after {
        return Ok(source.to_owned());
    }
    let output = source::rewrite(source, &before.json()?, &after.json()?, rename.as_ref())?;
    validate(&output)?;
    Ok(output)
}
