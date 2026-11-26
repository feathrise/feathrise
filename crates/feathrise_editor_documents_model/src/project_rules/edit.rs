//! 每次表单操作生成一个可校验的文档候选；同文档引用及其声明共同成功或共同撤回。
use super::{
    ActionForm, LocationForm, RuleForm, RulesForm, TriggerForm, ValueKind, VariableForm,
    form::tag,
    source::{self, Rename},
    validate,
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

fn scope_mut<'a>(
    form: &'a mut RulesForm,
    scene: Option<&str>,
) -> (&'a mut Vec<VariableForm>, &'a mut Vec<RuleForm>) {
    match scene {
        Some(scene) => {
            let scope = form.scenes.entry(scene.to_owned()).or_default();
            (&mut scope.variables, &mut scope.rules)
        }
        None => (&mut form.variables, &mut form.rules),
    }
}

fn retarget(
    rules: &mut [RuleForm],
    old: &str,
    value: &VariableForm,
    previous_kind: ValueKind,
) -> Result<(), String> {
    for rule in rules.iter_mut().filter(|r| uses_variable(r, old)) {
        if previous_kind != value.kind {
            return Err(format!(
                "规则「{}」仍引用变量「{old}」。请先调整该规则，再更换变量类型",
                rule.id
            ));
        }
        if old == value.id {
            continue;
        }
        let replace = |v: &mut String| {
            if v == old {
                v.clone_from(&value.id);
            }
        };
        if let TriggerForm::Changed { variable, .. } = &mut rule.when {
            replace(variable);
        }
        for condition in &mut rule.conditions {
            replace(&mut condition.variable);
        }
        for action in &mut rule.actions {
            if let ActionForm::Set { variable, .. } = action {
                replace(variable);
            }
        }
    }
    Ok(())
}

pub fn apply(source: &str, edit: RuleEdit) -> Result<String, String> {
    apply_scoped(source, None, edit)
}

/// 使用指定的场景 ID 修改该场景的声明；地点仍用工程文档的原有入口。
pub fn apply_scoped(source: &str, scene: Option<&str>, edit: RuleEdit) -> Result<String, String> {
    if scene.is_some_and(|id| id.trim().is_empty()) {
        return Err("场景标识不能为空".into());
    }
    if scene.is_some()
        && matches!(
            &edit,
            RuleEdit::Location { .. } | RuleEdit::RemoveLocation(_)
        )
    {
        return Err("地点应在工程地点列表中编辑".into());
    }
    let before = RulesForm::parse(source)?;
    let mut after = before.clone();
    let mut renames = Vec::new();
    match edit {
        RuleEdit::Variable {
            original,
            mut value,
        } => {
            value.tag = tag(value.tag.as_deref()).map(str::to_owned);
            let (variables, rules) = scope_mut(&mut after, scene);
            if let Some(old) = original.as_deref() {
                let previous_kind = variables[index(variables, old, |v| &v.id)?].kind;
                retarget(rules, old, &value, previous_kind)?;
                if old != value.id {
                    renames.push(Rename {
                        scene: scene.map(str::to_owned),
                        section: "variables",
                        from: old.to_owned(),
                        to: value.id.clone(),
                    });
                }
                upsert(variables, original.as_deref(), value.clone(), |v| &v.id)?;
                if scene.is_none() {
                    for (id, scope) in &mut after.scenes {
                        let local = scope.variables.iter().position(|v| v.id == old);
                        if old != value.id {
                            // 改工程 ID 时保留同名覆盖及其引用；防止新 ID 已有场景值而悄悄改变引用。
                            if scope.variables.iter().any(|v| v.id == value.id)
                                && (local.is_some()
                                    || scope.rules.iter().any(|r| uses_variable(r, old)))
                            {
                                return Err(format!("场景「{id}」已存在变量「{}」", value.id));
                            }
                            if let Some(i) = local {
                                scope.variables[i].id.clone_from(&value.id);
                                renames.push(Rename {
                                    scene: Some(id.clone()),
                                    section: "variables",
                                    from: old.to_owned(),
                                    to: value.id.clone(),
                                });
                            }
                        }
                        if local.is_none() || old != value.id {
                            retarget(&mut scope.rules, old, &value, previous_kind)
                                .map_err(|e| format!("场景「{id}」{e}"))?;
                        }
                    }
                }
            } else {
                upsert(variables, None, value, |v| &v.id)?;
            }
        }
        RuleEdit::Location { original, value } => {
            if let Some(old) = original.as_deref().filter(|o| *o != value.id) {
                for r in after
                    .rules
                    .iter_mut()
                    .chain(after.scenes.values_mut().flat_map(|s| &mut s.rules))
                {
                    if let Some(target) = location_target(r).filter(|t| t.as_str() == old) {
                        target.clone_from(&value.id);
                    }
                }
                renames.push(Rename {
                    scene: None,
                    section: "locations",
                    from: old.to_owned(),
                    to: value.id.clone(),
                });
            }
            upsert(&mut after.locations, original.as_deref(), value, |v| &v.id)?;
        }
        RuleEdit::Rule {
            original,
            mut value,
        } => {
            value.tag = tag(value.tag.as_deref()).map(str::to_owned);
            let (_, rules) = scope_mut(&mut after, scene);
            upsert(rules, original.as_deref(), value, |v| &v.id)?
        }
        RuleEdit::RemoveVariable(key) => {
            let inherits = scene.is_some() && after.variable(None, &key).is_some();
            let (variables, rules) = scope_mut(&mut after, scene);
            if !inherits {
                if let Some(r) = rules.iter().find(|r| uses_variable(r, &key)) {
                    return Err(format!(
                        "规则「{}」正在使用变量「{key}」，请先移除该引用",
                        r.id
                    ));
                }
            }
            let i = index(variables, &key, |v| &v.id)?;
            variables.remove(i);
        }
        RuleEdit::RemoveLocation(key) => {
            for r in after
                .rules
                .iter_mut()
                .chain(after.scenes.values_mut().flat_map(|s| &mut s.rules))
            {
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
            let (_, rules) = scope_mut(&mut after, scene);
            let i = index(rules, &key, |v| &v.id)?;
            rules.remove(i);
        }
        RuleEdit::MoveRule { id, to } => {
            let (_, rules) = scope_mut(&mut after, scene);
            let i = index(rules, &id, |v| &v.id)?;
            if to >= rules.len() {
                return Err("目标规则位置不存在".into());
            }
            let saved = rules.remove(i);
            rules.insert(to, saved);
        }
    }
    if before == after {
        return Ok(source.to_owned());
    }
    let output = source::rewrite(source, &before.json()?, &after.json()?, &renames)?;
    validate(&output)?;
    Ok(output)
}
