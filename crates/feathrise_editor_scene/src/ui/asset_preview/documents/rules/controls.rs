//! 界面提交前读取同一批原生输入。字段修改、子项操作与原文档撤销链按一次作者确认登记。
use super::{
    view::{Action, OPTIONS, ROWS},
    *,
};
use bevy::{input_focus::InputFocus, text::EditableText};
use feathrise_editor_documents_model::project_rules::ValueKind;
use feathrise_editor_ui::EditorButtonClick;
use std::borrow::Cow;

pub(super) fn suggest<'a>(
    sheet: &'a Sheet,
    form: &Form,
    field: Field,
    i: usize,
) -> Option<Cow<'a, str>> {
    if field == Field::Default {
        if form.kind != VariableKind::Enum {
            return None;
        }
        // 候选来自本次输入，尚未保存的枚举同样可以选默认值。
        return super::values::parse_choices(&form.choices)
            .ok()??
            .into_iter()
            .nth(i)
            .map(Cow::Owned);
    }
    saved_suggestion(sheet, form, field, i).map(Cow::Borrowed)
}

fn saved_suggestion<'a>(
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
        Field::From | Field::To => form.trigger.as_str(),
        Field::ConditionValue => form.condition_variable.as_str(),
        Field::ActionValue => form.action_name.as_str(),
        _ => return None,
    };
    let v = m.variables.get(sheet.position(Tab::Variables, var)?)?;
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
// 导航先算出明确的目标，当前字段校验成功以后才切换。点新建时始终保留一行空表单。
fn destination(a: Action, current: Selection, sheet: &Sheet) -> Option<Selection> {
    let tab = current.tab;
    let len = sheet.len(tab);
    let (tab, item) = match a {
        Action::New => (tab, None),
        Action::Row(_) | Action::Previous | Action::Next => {
            let page = current.item.unwrap_or(0) / ROWS;
            let i = match a {
                Action::Row(slot) => page * ROWS + slot,
                Action::Previous => page.checked_sub(1)? * ROWS,
                _ => (page + 1) * ROWS,
            };
            if i >= len {
                return None;
            }
            (tab, Some(i))
        }
        Action::ConditionPrevious | Action::ConditionNext => {
            let count = current.rule(&sheet.model)?.conditions.len();
            let condition = next(current.condition, count, a == Action::ConditionNext)?;
            return Some(Selection {
                condition,
                ..current
            });
        }
        Action::ActionPrevious | Action::ActionNext => {
            let count = current.rule(&sheet.model)?.actions.len();
            let action = next(current.action, count, a == Action::ActionNext)?;
            return Some(Selection { action, ..current });
        }
        _ => return None,
    };
    Some(Selection {
        tab,
        item,
        ..default()
    })
}
fn submit(s: &mut Studio, d: &DocumentState, change: Change, save: bool) -> Result<(), String> {
    start(
        s,
        d,
        Some(Submission {
            change,
            form: s.form.clone(),
        }),
        save,
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
    mut overlay: Option<ResMut<feathrise_editor_core::EditorHostOverlay>>,
    model: Option<Res<crate::workbench::WorkbenchModel>>,
    buttons: Query<(&Action, &EditorButtonClick), Changed<EditorButtonClick>>,
    fields: Query<(Entity, &Field, Ref<EditableText>)>,
    pointer: Query<&bevy::ui::RelativeCursorPosition, With<view::Panel>>,
    source: Query<Entity, With<DocumentSource>>,
    mut workspace: Option<ResMut<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    mut draw: MessageWriter<super::location_draw::DrawLocation>,
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
    if active_document(&docs)
        .is_some_and(|d| docs.is_loading(&d.path) || docs.file_change_pending(&d.path))
    {
        return;
    }
    let modal = view::editor_dialog_open(&s, &docs);
    if modal && let Some(overlay) = &mut overlay {
        overlay.active = true;
    }
    let mut action = buttons
        .iter()
        .find_map(|(a, c)| c.is_clicked().then_some(*a));
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let shown = shown_document(&s, &docs).is_some();
    if control && !guard.block_shortcuts && shown {
        // 查找框或其他面板拥有焦点时不截获其撤销。Ctrl+S 在当前文档内由表单统一收口。
        let owns = match focus.get() {
            Some(entity) => fields.get(entity).is_ok(),
            None => modal || pointer
                .single()
                .is_ok_and(|position| position.cursor_over()),
        };
        for (key, command, enabled) in [
            (
                KeyCode::KeyS,
                Action::Save,
                owns && !shift
                    && (s.editing_item || active_document(&docs).is_some_and(|d| d.dirty())),
            ),
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
    if modal {
        if keys.just_pressed(KeyCode::Escape) {
            action = Some(Action::Cancel);
        }
        // 读取本弹窗命令后吞掉场景、文档与全局快捷键的边沿；保留按住状态供原生输入法。
        let edges: Vec<_> = keys.get_just_pressed().chain(keys.get_just_released()).copied().collect();
        for key in edges {
            keys.clear_just_pressed(key);
            keys.clear_just_released(key);
        }
    }
    // 实时反映已确认的原生输入；移动光标、IME 候选或未变字段不会刷新整个面板。
    if s.editing_item && s.sheet.is_some() && s.key.as_ref() == active(&docs).as_ref() {
        for (_, field, text) in &fields {
            if (action.is_some() || text.is_changed())
                && !text.is_composing()
                && view::field_shown(s.sel.tab, &s.form, *field)
            {
                let current = s.bypass_change_detection().form.field(*field);
                if text.value() != current.as_str() {
                    *current = text.value().to_string();
                    s.notice.clear();
                }
            }
        }
    }
    let Some(a) = action else {
        return;
    };
    if !shown || (a == Action::Source && s.sel.tab != Tab::Locations) {
        return;
    }
    if guard.block_shortcuts
        || fields.iter().any(|(_, _, t)| {
            t.is_composing() || t.pending_paste.is_some() || !t.pending_edits.is_empty()
        })
    {
        s.notice = "请先完成当前输入，再执行操作".into();
        return;
    }
    let name_field = fields
        .iter()
        .find_map(|(e, f, _)| (*f == Field::Id).then_some(e));
    let edit = !matches!(a, Action::Previous | Action::Next);
    let name_focus = name_field.filter(|_| matches!(a, Action::New | Action::Row(_)));
    if s.saving() {
        if let Some(selection) = s.sheet.as_deref().and_then(|m| destination(a, s.sel, m)) {
            s.files.next = Some((selection, edit, name_focus));
        }
        return;
    }
    if let Some(job) = s.job.as_mut() {
        if a == Action::Save && job.change.is_some() {
            job.save = true;
        }
        return;
    }
    focus.clear();
    s.notice.clear();
    if a == Action::DrawLocation {
        if s.editing_item && s.sel.tab == Tab::Locations {
            draw.write(super::location_draw::DrawLocation);
        }
        return;
    }
    let result = (|| -> Result<(), String> {
        let sheet = s.sheet.clone();
        let leave_hint = format!(
            "请先保存当前{}，或点「取消」放弃本次填写。",
            s.sel.tab.noun()
        );
        if a == Action::Source {
            if s.has_parked_input() {
                return Err("请先保存或取消各工具中的未保存填写。".into());
            } else if s.has_unsaved_input() {
                return Err(leave_hint.clone());
            }
            let path = active_document(&docs)
                .ok_or("工程规则尚未载入")?
                .path
                .clone();
            let workspace = workspace.as_deref_mut().ok_or("中央文档编辑区不可用")?;
            docs.expand(path, workspace)?;
            active_document_mut(&mut docs)
                .ok_or("工程规则尚未载入")?
                .mode = ViewMode::Edit;
            if let Ok(entity) = source.single() {
                focus.set(entity, FocusCause::Navigated);
            }
            return Ok(());
        }
        if matches!(a, Action::KeepDraft | Action::Reload) {
            let path = active_document(&docs)
                .ok_or("工程规则尚未载入")?
                .path
                .clone();
            if a == Action::KeepDraft {
                docs.keep_draft(&path);
            } else {
                docs.reload_document(&path, true);
            }
            return Ok(());
        }
        let doc = active_document_mut(&mut docs).ok_or("工程规则尚未载入")?;
        if a == Action::Cancel {
            s.cancel(doc);
            return Ok(());
        }
        if a == Action::Save {
            if doc.external.warning {
                return Err("磁盘文件已被修改，保存已停止。请先处理文件冲突。".into());
            }
            if sheet.is_none() {
                return Err("请先在文件浏览器打开并修正规则文件".into());
            }
            if s.editing_item
                && s.form.id.trim().is_empty()
                && (s.sel.item.is_none() || s.form != s.baseline)
            {
                if let Some(entity) = name_field {
                    focus.set(entity, FocusCause::Navigated);
                }
                return Err(format!("请输入{}名称", s.sel.tab.noun()));
            }
            if s.editing_item && (s.form != s.baseline || s.sel.item.is_none()) {
                submit(&mut s, &docs, Change::Apply, true)?;
            } else if doc.dirty() {
                let message = if s.editing_item {
                    format!("{}「{}」已保存", s.sel.tab.noun(), s.form.id)
                } else {
                    "工程规则已保存".into()
                };
                s.queue_write(doc, message, None);
            } else {
                s.editing_item = false;
                s.notice = format!("当前{}没有需要保存的修改", s.sel.tab.noun());
            }
            return Ok(());
        }
        if matches!(a, Action::Undo | Action::Redo) {
            // 还没应用的无效名称或取值也能撤回，避免导航校验把作者困在表单里。
            if s.form != s.baseline {
                if a == Action::Redo {
                    return Err("请先保存或撤销本次填写，再重做".into());
                }
                s.hydrate();
                s.notice = "已撤销本次填写".into();
                return Ok(());
            }
            if (a == Action::Redo && doc.history.can_redo())
                || (a == Action::Undo && doc.history.can_undo())
            {
                s.remember(doc);
                doc.travel(a == Action::Redo);
                s.track_edit(doc);
            }
            return Ok(());
        }
        let m = sheet
            .as_ref()
            .ok_or("请等待规则加载完成，或从文件浏览器打开并修复文件")?;
        if let Some(selection) = destination(a, s.sel, m) {
            if s.editing_item && a != Action::New && selection == s.sel {
                return Ok(());
            }
            let leaves_item =
                selection.tab != s.sel.tab || selection.item != s.sel.item || a == Action::New;
            if s.form != s.baseline || (leaves_item && s.has_unsaved_input()) {
                return Err(leave_hint);
            }
            s.select(selection, edit, name_focus, &mut focus);
            return Ok(());
        }
        let change = match a {
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
            if change == Change::Delete && doc.external.warning {
                return Err("磁盘文件已被修改，删除已停止。请先处理文件冲突。".into());
            }
            submit(&mut s, &docs, change, change == Change::Delete)?;
            return Ok(());
        }
        match a {
            Action::Type(kind) if kind != s.form.kind => {
                let s = &mut *s;
                s.alternate.change_type(&mut s.form, kind);
                s.suggest = None;
            }
            Action::BoolDefault(value) => s.form.default_value = value.to_string(),
            Action::Trigger(changed) if changed != s.form.changed => {
                let s = &mut *s;
                s.form.changed = changed;
                std::mem::swap(&mut s.form.trigger, &mut s.alternate.trigger);
                s.suggest = None;
            }
            Action::Once => s.form.once = !s.form.once,
            Action::From => {
                s.form.use_from = !s.form.use_from;
                s.suggest = None;
            }
            Action::To => {
                s.form.use_to = !s.form.use_to;
                s.suggest = None;
            }
            Action::ActionType(effect) => {
                let s = &mut *s;
                s.alternate.change_effect(&mut s.form, effect);
                s.suggest = None;
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
                    *s.form.field(f) = value.into_owned();
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
