//! 固定行池的列表与属性表单。长清单通过分页浏览，原生输入实体保留字体、选区与输入法状态。
use super::{types::*, *};
use bevy::{
    input_focus::tab_navigation::TabIndex,
    text::{EditableText, LineHeight},
    ui::FocusPolicy,
};
use feathrise_editor_ui::{InteractionButton, UiInteraction, colors as c, metrics};

mod layout;
mod table;
mod editor_dialog;
pub(crate) use editor_dialog::RulesEditorDialogOverlay;
pub(super) use editor_dialog::is_open as editor_dialog_open;
pub(super) const ROWS: usize = 8;
pub(super) const OPTIONS: usize = 4;
pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        (
            layout::mount,
            editor_dialog::mount,
            display,
            table::fit,
            table::sync,
            anchor_options,
            editor_dialog::sync,
            form,
        )
            .chain()
            .after(super::super::presentation::sync)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
    app.add_systems(Update, editor_dialog::scroll.in_set(crate::SceneEditorSystemSet::Input));
}
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Row(usize),
    DeleteRow(usize),
    Previous,
    Next,
    New,
    Cancel,
    Delete,
    Save,
    Source,
    Undo,
    Redo,
    KeepDraft,
    Reload,
    Type(VariableKind),
    BoolDefault(bool),
    DrawLocation,
    Trigger(bool),
    Once,
    From,
    To,
    RuleUp,
    RuleDown,
    ConditionPrevious,
    ConditionNext,
    ConditionAdd,
    ConditionDelete,
    ActionType(Effect),
    ActionPrevious,
    ActionNext,
    ActionAdd,
    ActionDelete,
    ActionUp,
    ActionDown,
    Suggest(Field),
    Option(usize),
    OptionsPrevious,
    OptionsNext,
    OptionsClose,
}
#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
struct PanelBody;
#[derive(Component)]
struct SidebarStatus;
#[derive(Component)]
struct FieldRow(Field);
#[derive(Component)]
struct ButtonLabel(Action);
#[derive(Component)]
struct FieldLabel(Field);
#[derive(Component, Clone, Copy)]
enum Info {
    PageTitle,
    Selection,
    EditorTitle,
    Yarn,
    Conditions,
    Actions,
    Status,
}
#[derive(Component, Clone, Copy)]
enum Group {
    Editor,
    DraftActions,
    Types,
    Boolean,
    Location,
    Rule,
    Limits,
    Picker,
    External,
}
pub(super) fn field_shown(tab: Tab, f: &Form, k: Field) -> bool {
    match k {
        Field::Id => true,
        Field::Default => tab == Tab::Variables && f.kind != VariableKind::Bool,
        Field::Choices => tab == Tab::Variables && f.kind == VariableKind::Enum,
        Field::From => tab == Tab::Rules && f.changed && f.use_from,
        Field::To => tab == Tab::Rules && f.changed && f.use_to,
        Field::EventTarget => tab == Tab::Rules && !f.changed,
        Field::Scene | Field::Min | Field::Max => tab == Tab::Locations,
        _ => tab == Tab::Rules,
    }
}
fn field_label(tab: Tab, form: &Form, k: Field) -> &'static str {
    match k {
        Field::Id => match tab {
            Tab::Variables => "变量名称",
            Tab::Rules => "规则 ID",
            Tab::Locations => "地点 ID",
        },
        Field::Default => "值",
        Field::Choices => "候选值（用逗号分隔）",
        Field::Trigger => {
            if form.changed {
                "变化变量"
            } else {
                "事件名称"
            }
        }
        Field::From => "旧值",
        Field::To => "新值",
        Field::EventTarget => "事件目标（可选）",
        Field::ConditionVariable => "条件变量",
        Field::ConditionValue => "等于",
        Field::ActionName => {
            if form.request {
                "请求名称"
            } else {
                "写入变量"
            }
        }
        Field::ActionValue => {
            if form.request {
                match form.action_name.as_str() {
                    "cinematic.play" => "演出 ID",
                    "quest.start" => "任务 ID",
                    _ => "请求目标",
                }
            } else {
                "写入值"
            }
        }
        Field::Scene => "场景 ID",
        Field::Min => "最小坐标 XYZ",
        Field::Max => "最大坐标 XYZ",
    }
}

fn form(
    s: Res<Studio>,
    docs: Res<DocumentState>,
    mut last: Local<Option<u64>>,
    mut inputs: Query<(&Field, &mut EditableText)>,
    added: Query<(), Added<Field>>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
) {
    if active(&docs).is_none() {
        if focus.get().is_some_and(|e| inputs.contains(e)) {
            focus.clear();
        }
        return;
    }
    // 保存回执收起原生行时只释放其文本焦点，保留作者已转向的其他面板。
    if !s.editing_item && focus.get().is_some_and(|e| inputs.contains(e)) {
        focus.clear();
    }
    if *last == Some(s.revision) && added.is_empty() {
        return;
    }
    *last = Some(s.revision);
    let mut form = s.form.clone();
    for (&f, mut input) in &mut inputs {
        let value = form.field(f);
        if input.value() != value.as_str() {
            crate::ui::text_input::replace_single_line_value(&mut input, value, None);
        }
    }
}
fn show(node: &mut Node, yes: bool) {
    let next = if yes { Display::Flex } else { Display::None };
    if node.display != next {
        node.display = next;
    }
}
fn set(text: &mut Text, s: &str) {
    if text.as_str() != s {
        **text = s.into();
    }
}
/// 只移动一个候选列表，选项贴着正在填写的字段展开，滚轮仍由正文接收。
fn anchor_options(
    s: Res<Studio>,
    sections: Query<(Entity, &FieldRow)>,
    groups: Query<(Entity, &Group, &ChildOf)>,
    mut commands: Commands,
) {
    if !s.is_changed() {
        return;
    }
    let Some((field, _)) = s.suggest else { return };
    let Some((destination, _)) = sections.iter().find(|(_, f)| f.0 == field) else {
        return;
    };
    if let Some((list, _, parent)) = groups.iter().find(|(_, g, _)| matches!(g, Group::Picker))
        && parent.parent() != destination
    {
        commands.entity(destination).add_child(list);
    }
}

fn display(
    s: Res<Studio>,
    docs: Res<DocumentState>,
    mut editor: ResMut<crate::resources::SceneEditorState>,
    mut panel: Query<(&mut Node, &mut Visibility, Ref<Panel>)>,
    mut body: Query<&mut Node, (With<PanelBody>, Without<Panel>, Without<SidebarStatus>)>,
    mut sidebar_status: Query<(&mut Text, &mut Node), (With<SidebarStatus>, Without<Panel>)>,
    mut groups: Query<
        (&Group, &mut Node),
        (
            Without<Panel>,
            Without<PanelBody>,
            Without<SidebarStatus>,
            Without<FieldRow>,
            Without<Action>,
        ),
    >,
    mut fields: Query<
        (&FieldRow, &mut Node, &mut Visibility),
        (Without<Panel>, Without<PanelBody>, Without<SidebarStatus>),
    >,
    mut buttons: Query<
        (
            &Action,
            &mut Node,
            &UiInteraction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (
            Without<Panel>,
            Without<PanelBody>,
            Without<SidebarStatus>,
            Without<FieldRow>,
        ),
    >,
    hover: Query<(), (With<AuthorButton>, Changed<UiInteraction>)>,
    mut labels: Query<
        (&FieldLabel, &mut Text),
        (Without<SidebarStatus>, Without<ButtonLabel>, Without<Info>),
    >,
    mut captions: Query<(&ButtonLabel, &mut Text), (Without<SidebarStatus>, Without<Info>)>,
    mut infos: Query<
        (&Info, &mut Text, &mut Node),
        (
            Without<Panel>,
            Without<PanelBody>,
            Without<SidebarStatus>,
            Without<Group>,
            Without<FieldRow>,
            Without<Action>,
        ),
    >,
) {
    if !s.is_changed()
        && !docs.is_changed()
        && hover.is_empty()
        && !panel.iter().any(|(_, _, p)| p.is_added())
    {
        return;
    }
    let ready_document = shown_document(&s, &docs);
    let document = ready_document.or_else(|| listed_document(&s, &docs));
    let visible = document.is_some();
    let sidebar = docs.rules_in_sidebar();
    for (mut node, mut visibility, _) in &mut panel {
        show(&mut node, visible || sidebar);
        visibility.set_if_neq(if visible || sidebar {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for mut node in &mut body {
        show(&mut node, visible);
    }
    for (mut caption, mut node) in &mut sidebar_status {
        show(&mut node, sidebar && !visible);
        set(
            &mut caption,
            if !docs.sidebar_rules.error.is_empty() {
                &docs.sidebar_rules.error
            } else if active_document(&docs).is_none() && docs.rules_sidebar_document().is_some() {
                "完成中央源码编辑后可继续使用表单。"
            } else {
                "正在读取工程规则…"
            },
        );
    }
    let Some(doc) = document else { return };
    let tab = s.sel.tab;
    let sheet = s.sheet.as_deref();
    let available = ready_document.is_some()
        && !docs.is_loading(&doc.path)
        && !docs.file_change_pending(&doc.path);
    let modal = editor_dialog_open(&s, &docs);
    let (list, selection) = s.list_projection();
    let count = list.map_or(0, |m| m.len(tab));
    let start = selection.item.unwrap_or(0) / ROWS * ROWS;
    let rule = sheet.and_then(|m| s.sel.rule(&m.model));
    let conditions = rule.map_or(0, |r| r.conditions.len());
    let actions = rule.map_or(0, |r| r.actions.len());
    let new = s.sel.item.is_none();
    for (g, mut n) in &mut groups {
        show(
            &mut n,
            match g {
                Group::Editor => s.editing_item && available,
                Group::DraftActions => available && !s.editing_item && !s.saving() && doc.dirty(),
                Group::Types => tab == Tab::Variables,
                Group::Boolean => tab == Tab::Variables && s.form.kind == VariableKind::Bool,
                Group::Location => tab == Tab::Locations,
                Group::Rule => tab == Tab::Rules,
                Group::Limits => tab == Tab::Rules && s.form.changed,
                Group::Picker => s.suggest.is_some_and(|(f, _)| field_shown(tab, &s.form, f)),
                Group::External => doc.external.warning || doc.confirm_reload,
            },
        );
    }
    for (FieldRow(f), mut n, mut visible) in &mut fields {
        let shown = s.editing_item && available && field_shown(tab, &s.form, *f);
        show(&mut n, shown);
        // 原生 Tab 导航以继承显隐过滤输入，折叠行也须离开焦点候选。
        visible.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (FieldLabel(k), mut t) in &mut labels {
        set(&mut t, field_label(tab, &s.form, *k));
    }
    for (a, mut n, interaction, mut bg, mut border) in &mut buttons {
        let shown = match a {
            Action::Source => tab == Tab::Locations,
            Action::Row(i) => start + i < count,
            Action::DeleteRow(i) => tab != Tab::Locations && start + i < count,
            Action::New => sheet.is_some() && (modal || !(new && s.editing_item)),
            Action::Previous => start > 0,
            Action::Next => start + ROWS < count,
            Action::Delete => tab == Tab::Locations && !new,
            Action::Cancel => s.editing_item || s.has_unsaved_input(),
            Action::Undo => tab == Tab::Locations && (s.has_unsaved_input() || doc.history.can_undo()),
            Action::Redo => doc.history.can_redo(),
            Action::KeepDraft => doc.external.warning,
            Action::RuleUp => tab == Tab::Rules && s.sel.item.is_some_and(|i| i > 0),
            Action::RuleDown => tab == Tab::Rules && s.sel.item.is_some_and(|i| i + 1 < count),
            Action::ConditionPrevious | Action::ConditionNext => conditions > 1,
            Action::ConditionDelete => conditions > 0,
            Action::ActionPrevious | Action::ActionNext => actions > 1,
            Action::ActionUp => actions > 1 && s.sel.action > 0,
            Action::ActionDown => s.sel.action + 1 < actions,
            Action::ActionDelete => actions > 1,
            Action::OptionsPrevious => s.suggest.is_some_and(|(_, base)| base > 0),
            Action::OptionsNext => s.suggest.zip(sheet).is_some_and(|((f, base), m)| {
                controls::suggest(m, &s.form, f, base + OPTIONS).is_some()
            }),
            Action::Option(i) => s
                .suggest
                .zip(sheet)
                .is_some_and(|((f, base), m)| controls::suggest(m, &s.form, f, base + i).is_some()),
            Action::Suggest(f) => {
                sheet.is_some_and(|m| controls::suggest(m, &s.form, *f, 0).is_some())
            }
            _ => true,
        };
        show(&mut n, shown);
        let selected = match a {
            Action::Type(t) => *t == s.form.kind,
            Action::BoolDefault(value) => {
                s.form.default_value == if *value { "true" } else { "false" }
            }
            Action::Trigger(changed) => *changed == s.form.changed,
            Action::ActionType(effect) => *effect == Effect::of(&s.form),
            Action::Once => s.form.once,
            Action::From => s.form.use_from,
            Action::To => s.form.use_to,
            _ => false,
        };
        let primary = *a == Action::Save || (*a == Action::New && !s.editing_item);
        let color = match interaction {
            UiInteraction::Pressed => c::SELECT_BG_PRESSED,
            UiInteraction::Hovered => c::SELECT_BG_HOVER,
            _ if selected || primary => c::SELECT_BG,
            _ => c::SLOT_BG,
        };
        bg.set_if_neq(BackgroundColor(color));
        border.set_if_neq(BorderColor::all(if selected || primary {
            c::COPPER_BORDER
        } else {
            c::COPPER_BORDER_WEAK
        }));
    }
    for (ButtonLabel(a), mut t) in &mut captions {
        let label = match a {
            Action::New => format!("新建{}", tab.noun()),
            Action::Save if !s.editing_item => "保存更改".into(),
            Action::Save => format!("保存{}", tab.noun()),
            Action::Delete => format!("删除{}", tab.noun()),
            Action::Reload if doc.confirm_reload => "确认丢弃".into(),
            Action::Option(i) => s
                .suggest
                .zip(sheet)
                .and_then(|((f, base), m)| {
                    controls::suggest(m, &s.form, f, base + i)
                        .map(|id| choice_label(f, &s.form, &id).to_owned())
                })
                .unwrap_or_default(),
            Action::Once => if s.form.once {
                "每个存档仅一次 · 开"
            } else {
                "每个存档仅一次 · 关"
            }
            .into(),
            Action::From => if s.form.use_from {
                "限定旧值 · 开"
            } else {
                "限定旧值 · 关"
            }
            .into(),
            Action::To => if s.form.use_to {
                "限定新值 · 开"
            } else {
                "限定新值 · 关"
            }
            .into(),
            _ => continue,
        };
        set(&mut t, &label);
    }
    for (role, mut t, mut n) in &mut infos {
        let shown = match role {
            Info::Selection => sheet.is_some(),
            Info::Actions | Info::Conditions => tab == Tab::Rules,
            Info::Yarn => tab == Tab::Variables && !s.form.id.trim().is_empty(),
            _ => true,
        };
        if !shown {
            show(&mut n, false);
            continue;
        }
        let value = match role {
            Info::PageTitle => tab.title().into(),
            Info::Selection if count == 0 => format!("还没有{}", tab.noun()),
            Info::Selection => format!("共 {count} 个{}", tab.noun()),
            Info::EditorTitle if new => format!("新建{}", tab.noun()),
            Info::EditorTitle => format!(
                "编辑{} · {}",
                tab.noun(),
                sheet
                    .and_then(|m| m.id(tab, s.sel.item?))
                    .unwrap_or_default()
            ),
            Info::Yarn => format!("Yarn 读取名  $project_{}", s.form.id.trim()),
            Info::Conditions if conditions > 0 => format!(
                "附加条件（全部满足）   {} / {conditions}",
                s.sel.condition + 1
            ),
            Info::Conditions => "附加条件（可选）".into(),
            Info::Actions if actions > 0 => {
                format!("执行动作（按顺序）   {} / {actions}", s.sel.action + 1)
            }
            Info::Actions => "执行动作".into(),
            Info::Status if doc.confirm_reload => {
                "再次点「确认丢弃」会载入磁盘版本并放弃当前填写。".into()
            }
            Info::Status if !docs.sidebar_rules.error.is_empty() => docs.sidebar_rules.error.clone(),
            Info::Status if active_document(&docs).is_none() => {
                "完成中央源码编辑后可继续使用表单。".into()
            }
            Info::Status if !available => "正在更新文件，完成后可继续编辑。".into(),
            Info::Status if !s.notice.is_empty() => s.notice.clone(),
            Info::Status if doc.external.warning => "文件已在外部修改，请选择如何处理。".into(),
            Info::Status if s.saving() => "正在保存…".into(),
            Info::Status if s.job.is_some() => "正在读取规则…".into(),
            Info::Status if doc.status.contains("失败") => doc.status.clone(),
            Info::Status if s.has_unsaved_input() || doc.dirty() => "有未保存的修改。".into(),
            Info::Status => s.saved_current().unwrap_or("").to_owned(),
        };
        if matches!(role, Info::Status) {
            // 普通回执只刷新底栏，在侧栏不占位置。弹窗仍就地提示输入校验。
            if !modal && !value.is_empty() && t.as_str() != value.as_str() {
                editor.status.clone_from(&value);
            }
            show(&mut n, modal && !value.is_empty());
        } else {
            show(&mut n, true);
        }
        set(&mut t, &value);
    }
}
