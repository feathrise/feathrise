//! 界面提交前读取同一批原生输入。字段修改、子项操作与原文档撤销链按一次作者确认登记。
use super::{
    view::{Action, OPTIONS, ROWS},
    *,
};
use bevy::{input_focus::InputFocus, text::EditableText};
use feathrise_editor_documents_model::project_rules::ValueKind;
use feathrise_editor_ui::EditorButtonClick;

pub(super) fn suggest<'a>(
    sheet: &'a Sheet,
    form: &Form,
    field: Field,
    i: usize,
) -> Option<&'a str> {
    let m = &sheet.model;
    let var = match field {
        Field::Trigger if !form.changed => return sheet.events.get(i).map(String::as_str),
        Field::ActionName if form.request => return sheet.requests.get(i).map(String::as_str),
        Field::ActionName | Field::ConditionVariable | Field::Trigger => {
            return m.variables.get(i).map(|v| v.id.as_str());
        }
        Field::EventTarget
            if matches!(
                form.trigger.as_str(),
                "location.entered" | "location.exited"
            ) =>
        {
            return m.locations.get(i).map(|v| v.id.as_str());
        }
        Field::EventTarget => {
            return sheet
                .event_targets
                .get(&form.trigger)?
                .get(i)
                .map(String::as_str);
        }
        Field::ActionValue if form.request => {
            return sheet
                .request_targets
                .get(&form.action_name)?
                .get(i)
                .map(String::as_str);
        }
        Field::Default => {
            if form.kind == ValueKind::Bool {
                return ["false", "true"].get(i).copied();
            }
            form.id.as_str()
        }
        Field::From | Field::To => form.trigger.as_str(),
        Field::ConditionValue => form.condition_variable.as_str(),
        Field::ActionValue => form.action_name.as_str(),
        _ => return None,
    };
    let v = m.variables.get(*sheet.var_index.get(var)?)?;
    if v.kind == ValueKind::Bool {
        ["false", "true"].get(i).copied()
    } else {
        v.choices.as_ref()?.get(i).map(String::as_str)
    }
}
fn next(i: usize, len: usize, forward: bool) -> Option<usize> {
    (len > 0).then(|| {
        if forward {
            (i + 1) % len
        } else {
            (i + len - 1) % len
        }
    })
}
fn submit(s: &mut Studio, d: &DocumentState, change: Change, after: After) -> Result<(), String> {
    start(
        s,
        d,
        Some(Submission {
            change,
            form: s.form.clone(),
        }),
        after,
    )
}
pub(super) fn input(
    mut s: ResMut<Studio>,
    mut docs: ResMut<DocumentState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    editor: Res<crate::resources::SceneEditorState>,
    settings: Option<Res<crate::resources::SceneEditorSettingsState>>,
    guard: Res<crate::resources::EditorTextInputGuard>,
    model: Option<Res<crate::workbench::WorkbenchModel>>,
    buttons: Query<(&Action, &EditorButtonClick), Changed<EditorButtonClick>>,
    fields: Query<(Entity, &Field, &EditableText)>,
    source: Query<Entity, With<DocumentSource>>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || settings.as_ref().is_some_and(|v| v.open)
        || model.as_ref().is_some_and(|m| m.modal || m.busy)
        || active(&docs).is_none()
    {
        return;
    }
    let mut action = buttons
        .iter()
        .find_map(|(a, c)| c.is_clicked().then_some(*a));
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if control && !guard.block_shortcuts {
        // 查找框或其他面板拥有焦点时不截获其撤销。Ctrl+S 在当前文档内由表单统一收口。
        let owns = focus.get().is_none_or(|e| fields.get(e).is_ok());
        for (key, command, enabled) in [
            (KeyCode::KeyS, Action::Save, !shift),
            (
                KeyCode::KeyZ,
                if shift { Action::Redo } else { Action::Undo },
                owns,
            ),
            (KeyCode::KeyY, Action::Redo, owns),
        ] {
            if enabled && keys.just_pressed(key) {
                action = Some(command);
                keys.clear_just_pressed(key);
                break;
            }
        }
    }
    let Some(a) = action else {
        return;
    };
    if guard.block_shortcuts
        || fields.iter().any(|(_, _, t)| {
            t.is_composing() || t.pending_paste.is_some() || !t.pending_edits.is_empty()
        })
    {
        s.notice = "请先完成当前输入，再执行操作".into();
        return;
    }
    if let Some(job) = s.job.as_mut() {
        if matches!(a, Action::Save) && job.editing {
            job.after = After::Save;
        } else {
            s.notice = "正在处理工程规则…".into();
        }
        return;
    }
    for (_, f, input) in &fields {
        if view::field_shown(s.sel.tab, &s.form, *f) {
            let dest = s.form.field(*f);
            if input.value() != dest.as_str() {
                *dest = input.value().to_string();
            }
        }
    }
    focus.clear();
    let result = (|| -> Result<(), String> {
        let sheet = s.sheet.clone();
        if matches!(a, Action::Source | Action::Save) {
            let after = if a == Action::Source {
                After::Source
            } else {
                After::Save
            };
            if sheet.is_some() && s.form != s.baseline {
                submit(&mut s, &docs, Change::Apply, after)?;
            } else {
                finish(&mut docs, after, &mut focus, source.single().ok());
            }
            return Ok(());
        }
        if matches!(a, Action::Undo | Action::Redo) {
            if let Some(doc) = docs.current_mut() {
                doc.travel(a == Action::Redo);
            }
            s.notice.clear();
            return Ok(());
        }
        let m = sheet
            .as_ref()
            .ok_or("请先在高级源码修复文件，或等待规则加载完成")?;
        let tab = s.sel.tab;
        s.notice.clear();
        let change = match a {
            Action::Apply => Some(Change::Apply),
            Action::Delete => Some(Change::Delete),
            Action::ConditionAdd => Some(Change::ConditionAdd),
            Action::ConditionDelete => Some(Change::ConditionDelete),
            Action::ActionAdd => Some(Change::ActionAdd),
            Action::ActionDelete => Some(Change::ActionDelete),
            Action::ActionUp => Some(Change::ActionUp),
            Action::ActionDown => Some(Change::ActionDown),
            Action::RuleUp => Some(Change::RuleUp),
            Action::RuleDown => Some(Change::RuleDown),
            _ => None,
        };
        if let Some(change) = change {
            if change != Change::Apply || s.sel.item.is_none() || s.form != s.baseline {
                submit(&mut s, &docs, change, After::Stay)?;
            }
            return Ok(());
        }
        match a {
            Action::Tab(t) => s.select(t, (m.len(t) > 0).then_some(0)),
            Action::New => s.select(tab, None),
            Action::Row(slot) => {
                let i = s.sel.item.unwrap_or(0) / ROWS * ROWS + slot;
                if i < m.len(tab) {
                    s.select(tab, Some(i));
                }
            }
            Action::Previous | Action::Next => {
                let i = s.sel.item.map_or_else(
                    || (m.len(tab) > 0).then_some(0),
                    |i| next(i, m.len(tab), a == Action::Next),
                );
                s.select(tab, i);
            }
            Action::Type => {
                s.form.kind = match s.form.kind {
                    ValueKind::Bool => ValueKind::Number,
                    ValueKind::Number => ValueKind::Text,
                    ValueKind::Text => ValueKind::Bool,
                };
                s.form.default_value = match s.form.kind {
                    ValueKind::Bool => "false",
                    ValueKind::Number => "0",
                    ValueKind::Text => "",
                }
                .into();
            }
            Action::Trigger => {
                s.form.changed = !s.form.changed;
                s.form.trigger.clear();
                s.suggest = None;
            }
            Action::Once => s.form.once = !s.form.once,
            Action::From => {
                s.form.use_from = !s.form.use_from;
                if !s.form.use_from {
                    s.form.from.clear();
                }
            }
            Action::To => {
                s.form.use_to = !s.form.use_to;
                if !s.form.use_to {
                    s.form.to.clear();
                }
            }
            Action::ActionType => {
                s.form.request = !s.form.request;
                s.form.action_name.clear();
                s.form.action_value.clear();
                s.suggest = None;
            }
            Action::ConditionPrevious | Action::ConditionNext => {
                if let Some(r) = s.sel.rule(&m.model)
                    && let Some(i) = next(
                        s.sel.condition,
                        r.conditions.len(),
                        a == Action::ConditionNext,
                    )
                {
                    s.sel.condition = i;
                    s.form.set_condition(r.conditions.get(i));
                    s.baseline.set_condition(r.conditions.get(i));
                }
            }
            Action::ActionPrevious | Action::ActionNext => {
                if let Some(r) = s.sel.rule(&m.model)
                    && let Some(i) = next(s.sel.action, r.actions.len(), a == Action::ActionNext)
                {
                    s.sel.action = i;
                    s.form.set_action(r.actions.get(i));
                    s.baseline.set_action(r.actions.get(i));
                }
            }
            Action::Suggest(f) => s.suggest = Some((f, 0)),
            Action::OptionsPrevious | Action::OptionsNext => {
                if let Some((f, base)) = s.suggest {
                    let next = if a == Action::OptionsPrevious {
                        base.saturating_sub(OPTIONS)
                    } else {
                        base + OPTIONS
                    };
                    if suggest(m, &s.form, f, next).is_some() {
                        s.suggest = Some((f, next));
                    }
                }
            }
            Action::OptionsClose => s.suggest = None,
            Action::Option(i) => {
                if let Some((f, base)) = s.suggest
                    && let Some(value) = suggest(m, &s.form, f, base + i)
                {
                    *s.form.field(f) = value.to_owned();
                    if f == Field::From {
                        s.form.use_from = true;
                    }
                    if f == Field::To {
                        s.form.use_to = true;
                    }
                    s.suggest = None;
                }
            }
            _ => {}
        }
        s.refresh();
        Ok(())
    })();
    if let Err(e) = result {
        s.notice = e;
    }
}
