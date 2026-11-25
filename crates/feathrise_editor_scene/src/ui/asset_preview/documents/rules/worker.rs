//! 规则完整解析、局部写改和建议索引均在同一工作槽中完成，仅返回可原子采用的正文。
use super::types::*;
use feathrise_editor_documents_model::project_rules::{
    self as model, RuleEdit, RuleForm, RulesForm,
};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Change {
    Apply,
    Delete,
    RuleUp,
    RuleDown,
    ConditionAdd,
    ConditionDelete,
    ActionAdd,
    ActionDelete,
    ActionUp,
    ActionDown,
}
pub(super) struct Submission {
    pub change: Change,
    pub form: Form,
}
pub(super) struct Reply {
    pub sheet: Arc<Sheet>,
    pub selection: Selection,
    pub text: Option<(Arc<str>, Vec<Arc<str>>)>,
}
pub(super) fn prepare(
    source: Arc<str>,
    old: Option<Arc<Sheet>>,
    mut sel: Selection,
    want: Option<String>,
    input: Option<Submission>,
    pages: Option<Vec<Arc<str>>>,
) -> Result<Reply, String> {
    let mut wanted = want;
    let rewrite = if let Some(Submission { change, form }) = input {
        let prev = old.as_ref().ok_or("规则尚未载入")?;
        let original = sel
            .item
            .and_then(|i| prev.id(sel.tab, i))
            .map(str::to_owned);
        let edit = if matches!(change, Change::RuleUp | Change::RuleDown) {
            let index = sel.item.ok_or("请先选择要移动的规则")?;
            let target = if change == Change::RuleUp {
                index.checked_sub(1)
            } else {
                index.checked_add(1)
            };
            let to = target
                .filter(|i| *i < prev.model.rules.len())
                .ok_or("规则已在该方向的端点")?;
            let id = original.ok_or("规则已变化")?;
            wanted = Some(id.clone());
            RuleEdit::MoveRule { id, to }
        } else if change == Change::Delete {
            let id = original.ok_or("请先选择要删除的项目")?;
            wanted = None;
            match sel.tab {
                Tab::Variables => RuleEdit::RemoveVariable(id),
                Tab::Rules => RuleEdit::RemoveRule(id),
                Tab::Locations => RuleEdit::RemoveLocation(id),
            }
        } else {
            wanted = Some(form.id.trim().into());
            match sel.tab {
                Tab::Variables => RuleEdit::Variable {
                    original,
                    value: form.variable()?,
                },
                Tab::Locations => RuleEdit::Location {
                    original,
                    value: form.location()?,
                },
                Tab::Rules => {
                    let mut rule = sel.rule(&prev.model).cloned().unwrap_or_else(|| RuleForm {
                        id: String::new(),
                        when: form.trigger(),
                        once: false,
                        conditions: Vec::new(),
                        actions: Vec::new(),
                    });
                    rule.id = form.id.trim().into();
                    rule.when = form.trigger();
                    rule.once = form.once;
                    match change {
                        Change::ConditionAdd => {
                            sel.condition = rule.conditions.len();
                            rule.conditions.push(form.condition());
                        }
                        Change::ConditionDelete => {
                            if sel.condition >= rule.conditions.len() {
                                return Err("尚未选择条件".into());
                            }
                            rule.conditions.remove(sel.condition);
                        }
                        _ => {
                            if let Some(c) = rule.conditions.get_mut(sel.condition) {
                                *c = form.condition()
                            } else if !form.condition_variable.is_empty() {
                                rule.conditions.push(form.condition());
                            }
                        }
                    }
                    match change {
                        Change::ActionAdd => {
                            sel.action = rule.actions.len();
                            rule.actions.push(form.action());
                        }
                        Change::ActionDelete => {
                            if sel.action >= rule.actions.len() {
                                return Err("尚未选择动作".into());
                            }
                            rule.actions.remove(sel.action);
                        }
                        Change::ActionUp | Change::ActionDown => {
                            let at = sel.action;
                            let next = if change == Change::ActionUp {
                                at.checked_sub(1)
                            } else {
                                at.checked_add(1)
                            };
                            let Some(next) = next.filter(|i| *i < rule.actions.len()) else {
                                return Err("动作已在该方向的端点".into());
                            };
                            rule.actions[at] = form.action();
                            rule.actions.swap(at, next);
                            sel.action = next;
                        }
                        _ => {
                            if let Some(a) = rule.actions.get_mut(sel.action) {
                                *a = form.action()
                            } else {
                                rule.actions.push(form.action());
                            }
                        }
                    }
                    RuleEdit::Rule {
                        original,
                        value: rule,
                    }
                }
            }
        };
        // 为词元局部修改提供原始换行，包括同一文件内混用的 LF 与 CRLF。原生编辑器另持 LF 坐标。
        let pages = pages.as_deref().ok_or("规则编辑缺少原始文档")?;
        let joined;
        let raw = if let [only] = pages {
            only.as_ref()
        } else {
            joined = pages.iter().map(AsRef::as_ref).collect::<String>();
            &joined
        };
        let s: Arc<str> = model::apply(raw, edit)?.into();
        (s.as_ref() != raw).then_some(s)
    } else {
        None
    };
    let model = RulesForm::parse(rewrite.as_deref().unwrap_or(&source))?;
    let sheet = Arc::new(Sheet::new(model));
    let len = sheet.len(sel.tab);
    sel.item = wanted
        .as_ref()
        .and_then(|id| (0..len).find(|i| sheet.id(sel.tab, *i) == Some(id.as_str())))
        .or_else(|| (len > 0).then(|| sel.item.unwrap_or(0).min(len - 1)));
    if let Some(rule) = sel.rule(&sheet.model) {
        sel.condition = sel.condition.min(rule.conditions.len().saturating_sub(1));
        sel.action = sel.action.min(rule.actions.len().saturating_sub(1));
    }
    let text = rewrite.map(|disk| {
        let normalized = if disk.contains("\r\n") {
            disk.replace("\r\n", "\n").into()
        } else {
            disk.clone()
        };
        (normalized, vec![disk])
    });
    Ok(Reply {
        sheet,
        selection: sel,
        text,
    })
}
