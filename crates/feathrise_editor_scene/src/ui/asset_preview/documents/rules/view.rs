//! 固定行池的列表与属性表单。长清单通过分页浏览，原生输入实体保留字体、选区与输入法状态。
use super::{types::*, *};
use bevy::{
    input_focus::tab_navigation::TabIndex,
    text::{EditableText, LineHeight},
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::{InteractionButton, colors as c};
pub(super) const ROWS: usize = 8;
pub(super) const OPTIONS: usize = 4;
pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        (mount, display, form)
            .chain()
            .after(super::super::presentation::sync)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
}
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Tab(Tab),
    Row(usize),
    Previous,
    Next,
    New,
    Apply,
    Delete,
    Save,
    Source,
    Undo,
    Redo,
    Type,
    Trigger,
    Once,
    From,
    To,
    RuleUp,
    RuleDown,
    ConditionPrevious,
    ConditionNext,
    ConditionAdd,
    ConditionDelete,
    ActionType,
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
struct Panel;
#[derive(Component)]
struct FieldRow(Field);
#[derive(Component)]
struct ButtonLabel(Action);
#[derive(Component)]
struct FieldLabel(Field);
#[derive(Component)]
enum Info {
    Selection,
    Conditions,
    Actions,
    Status,
}
#[derive(Component, Clone, Copy)]
enum Group {
    Rule,
    Condition,
    Actions,
    Picker,
    External,
}
fn font(h: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: h.clone().into(),
        font_size: size.into(),
        ..default()
    }
}
fn row() -> Node {
    Node {
        width: percent(100),
        min_width: px(0),
        flex_wrap: FlexWrap::Wrap,
        align_items: AlignItems::Center,
        column_gap: px(5),
        row_gap: px(3),
        flex_shrink: 0.,
        ..default()
    }
}
fn button(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    name: impl Into<String>,
    label: &str,
    a: Action,
) {
    let mut e = p.spawn((
        a,
        AuthorButton,
        Name::new(name.into()),
        InteractionButton,
        FocusPolicy::Block,
        Node {
            min_height: px(25),
            min_width: px(0),
            max_width: percent(100),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(c::SLOT_BG),
        BorderColor::all(c::COPPER_BORDER_WEAK),
    ));
    // 共用身份让原有保存/编辑入口和窗口验收仍可定位；业务按工作台当前待提交字段处理。
    match a {
        Action::Save => {
            e.insert(super::super::components::DocumentAction::Save);
        }
        Action::Source => {
            e.insert(super::super::components::DocumentAction::Edit);
        }
        _ => {}
    }
    e.with_child((
        ButtonLabel(a),
        Text::new(label),
        font(h, 12.),
        TextLayout::no_wrap(),
        TextColor(c::TEXT_PRIMARY),
        Pickable::IGNORE,
    ));
}
fn field(p: &mut ChildSpawnerCommands, h: &Handle<Font>, f: Field) {
    p.spawn((FieldRow(f), row())).with_children(|r| {
        r.spawn((
            FieldLabel(f),
            Text::default(),
            font(h, 12.),
            TextColor(c::TEXT_MUTED),
            Node {
                width: px(102),
                ..default()
            },
        ));
        r.spawn((
            f,
            Name::new(format!("RulesField{f:?}")),
            crate::ui::text_input::EditorTextInput,
            TabIndex(0),
            FocusPolicy::Block,
            bevy::text::TextCursorStyle::default(),
            EditableText {
                allow_newlines: false,
                ..default()
            },
            TextLayout::no_wrap(),
            font(h, 13.),
            LineHeight::Px(18.),
            TextColor(c::TEXT_VALUE),
            BackgroundColor(c::INPUT_BG),
            BorderColor::all(c::COPPER_BORDER_WEAK),
            Node {
                height: px(26),
                min_width: px(25),
                flex_basis: px(0),
                flex_grow: 1.,
                padding: UiRect::horizontal(px(5)),
                border: UiRect::all(px(1)),
                overflow: Overflow::clip(),
                ..default()
            },
        ));
        if !matches!(
            f,
            Field::Id | Field::Min | Field::Max | Field::Scene | Field::Choices
        ) {
            button(
                r,
                h,
                format!("RulesSuggest{f:?}"),
                "选择…",
                Action::Suggest(f),
            );
        }
    });
}
fn info(p: &mut ChildSpawnerCommands, h: &Handle<Font>, role: Info, name: &str) {
    p.spawn((
        role,
        Name::new(name.to_owned()),
        Text::default(),
        font(h, 12.),
        TextColor(c::TEXT_MUTED),
        Node {
            flex_shrink: 0.,
            ..default()
        },
    ));
}
fn group(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    g: Group,
    buttons: &[(&str, &str, Action)],
) {
    p.spawn((g, row())).with_children(|r| {
        for (n, l, a) in buttons {
            button(r, h, *n, l, *a);
        }
    });
}
fn mount(
    mut commands: Commands,
    hosts: Query<Entity, Added<super::super::components::DocumentCenterHost>>,
    server: Res<AssetServer>,
) {
    let h = feathrise_editor_core::editor_font(&server, feathrise_editor_core::FONT_PATH);
    for host in &hosts {
        commands.entity(host).with_children(|host| {
            host.spawn((
                Panel,
                Name::new("RulesWorkbench"),
                Node {
                    display: Display::None,
                    width: percent(100),
                    height: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(8)),
                    row_gap: px(6),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(c::PANEL_BG_STRONG),
            ))
            .with_children(|p| {
                p.spawn(row()).with_children(|r| {
                    r.spawn((
                        Text::new("工程变量与触发规则"),
                        font(&h, 17.),
                        TextColor(c::TEXT_PRIMARY),
                    ));
                    for (n, l, a) in [
                        ("RulesSave", "保存", Action::Save),
                        ("RulesUndo", "撤销", Action::Undo),
                        ("RulesRedo", "重做", Action::Redo),
                        ("RulesSource", "高级源码", Action::Source),
                    ] {
                        button(r, &h, n, l, a);
                    }
                });
                p.spawn(row()).with_children(|r| {
                    for (n, l, t) in [
                        ("RulesTabVariables", "变量", Tab::Variables),
                        ("RulesTabRules", "触发规则", Tab::Rules),
                        ("RulesTabLocations", "地点范围", Tab::Locations),
                    ] {
                        button(r, &h, n, l, Action::Tab(t));
                    }
                });
                p.spawn(Node {
                    width: percent(100),
                    flex_grow: 1.,
                    min_height: px(0),
                    min_width: px(0),
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|body| {
                    body.spawn(Node {
                        width: percent(28),
                        min_width: px(88),
                        max_width: px(255),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        min_height: px(0),
                        overflow: Overflow::clip(),
                        ..default()
                    })
                    .with_children(|list| {
                        list.spawn(row()).with_children(|r| {
                            for (n, l, a) in [
                                ("RulesNew", "新建", Action::New),
                                ("RulesPrevious", "上一个", Action::Previous),
                                ("RulesNext", "下一个", Action::Next),
                            ] {
                                button(r, &h, n, l, a);
                            }
                        });
                        info(list, &h, Info::Selection, "RulesSelection");
                        for i in 0..ROWS {
                            button(list, &h, format!("RulesRow{i}"), "", Action::Row(i));
                        }
                    });
                    body.spawn((
                        crate::components::SceneEditorRightPanelScroll,
                        ScrollPosition::default(),
                        RelativeCursorPosition::default(),
                        Node {
                            flex_grow: 1.,
                            flex_basis: px(0),
                            flex_direction: FlexDirection::Column,
                            min_width: px(0),
                            min_height: px(0),
                            height: percent(100),
                            overflow: Overflow::scroll_y(),
                            padding: UiRect::right(px(8)),
                            row_gap: px(4),
                            ..default()
                        },
                    ))
                    .with_children(|form| {
                        field(form, &h, Field::Id);
                        button(form, &h, "RulesType", "变量类型", Action::Type);
                        for f in [
                            Field::Default,
                            Field::Choices,
                            Field::Scene,
                            Field::Min,
                            Field::Max,
                        ] {
                            field(form, &h, f);
                        }
                        group(
                            form,
                            &h,
                            Group::Rule,
                            &[
                                ("RulesTrigger", "触发方式", Action::Trigger),
                                ("RulesOnce", "仅执行一次", Action::Once),
                                ("RulesRuleUp", "规则上移", Action::RuleUp),
                                ("RulesRuleDown", "规则下移", Action::RuleDown),
                            ],
                        );
                        field(form, &h, Field::Trigger);
                        field(form, &h, Field::EventTarget);
                        form.spawn(row()).with_children(|r| {
                            button(r, &h, "RulesFrom", "限定旧值", Action::From);
                            button(r, &h, "RulesTo", "限定新值", Action::To);
                        });
                        field(form, &h, Field::From);
                        field(form, &h, Field::To);
                        info(form, &h, Info::Conditions, "RulesConditions");
                        group(
                            form,
                            &h,
                            Group::Condition,
                            &[
                                (
                                    "RulesConditionPrevious",
                                    "上一条件",
                                    Action::ConditionPrevious,
                                ),
                                ("RulesConditionNext", "下一条件", Action::ConditionNext),
                                ("RulesConditionAdd", "追加条件", Action::ConditionAdd),
                                (
                                    "RulesConditionDelete",
                                    "删除此条件",
                                    Action::ConditionDelete,
                                ),
                            ],
                        );
                        field(form, &h, Field::ConditionVariable);
                        field(form, &h, Field::ConditionValue);
                        info(form, &h, Info::Actions, "RulesActions");
                        group(
                            form,
                            &h,
                            Group::Actions,
                            &[
                                ("RulesActionType", "动作类型", Action::ActionType),
                                ("RulesActionPrevious", "上一动作", Action::ActionPrevious),
                                ("RulesActionNext", "下一动作", Action::ActionNext),
                                ("RulesActionAdd", "追加动作", Action::ActionAdd),
                                ("RulesActionDelete", "删除此动作", Action::ActionDelete),
                                ("RulesActionUp", "动作上移", Action::ActionUp),
                                ("RulesActionDown", "动作下移", Action::ActionDown),
                            ],
                        );
                        field(form, &h, Field::ActionName);
                        field(form, &h, Field::ActionValue);
                    });
                });
                p.spawn((Group::Picker, row())).with_children(|r| {
                    button(
                        r,
                        &h,
                        "RulesOptionsPrevious",
                        "上一组",
                        Action::OptionsPrevious,
                    );
                    for i in 0..OPTIONS {
                        button(r, &h, format!("RulesOption{i}"), "", Action::Option(i));
                    }
                    button(r, &h, "RulesOptionsNext", "下一组", Action::OptionsNext);
                    button(r, &h, "RulesOptionsClose", "收起", Action::OptionsClose);
                });
                p.spawn(row()).with_children(|r| {
                    button(r, &h, "RulesApply", "应用到草稿", Action::Apply);
                    button(r, &h, "RulesDelete", "删除所选项", Action::Delete);
                    info(r, &h, Info::Status, "RulesStatus");
                });
                p.spawn((Group::External, row())).with_children(|r| {
                    for (label, a) in [
                        (
                            "保留草稿",
                            super::super::components::DocumentAction::KeepDraft,
                        ),
                        (
                            "从磁盘重载（草稿需确认）",
                            super::super::components::DocumentAction::Reload,
                        ),
                    ] {
                        r.spawn((
                            a,
                            InteractionButton,
                            Node {
                                padding: UiRect::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(c::SLOT_BG),
                        ))
                        .with_child((
                            Text::new(label),
                            font(&h, 12.),
                            TextColor(c::TEXT_PRIMARY),
                            Pickable::IGNORE,
                        ));
                    }
                });
            });
        });
    }
}

pub(super) fn field_shown(tab: Tab, f: &Form, k: Field) -> bool {
    match k {
        Field::Id => true,
        Field::Default => tab == Tab::Variables,
        Field::Choices => {
            tab == Tab::Variables
                && f.kind == feathrise_editor_documents_model::project_rules::ValueKind::Text
        }
        Field::From | Field::To => tab == Tab::Rules && f.changed,
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
        Field::Default => "默认值",
        Field::Choices => "固定选项（逗号）",
        Field::Trigger => {
            if form.changed {
                "变化变量"
            } else {
                "事件名称"
            }
        }
        Field::From => "旧值（可选）",
        Field::To => "新值（可选）",
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
                "请求目标"
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
) {
    if active(&docs).is_none() || *last == Some(s.revision) {
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
fn display(
    s: Res<Studio>,
    docs: Res<DocumentState>,
    mut panel: Query<&mut Node, With<Panel>>,
    mut groups: Query<(&Group, &mut Node), (Without<Panel>, Without<FieldRow>, Without<Action>)>,
    mut fields: Query<(&FieldRow, &mut Node), (Without<Panel>, Without<Action>)>,
    mut buttons: Query<(&Action, &mut Node, &mut BackgroundColor), Without<Panel>>,
    mut labels: Query<(&FieldLabel, &mut Text), (Without<ButtonLabel>, Without<Info>)>,
    mut captions: Query<(&ButtonLabel, &mut Text), Without<Info>>,
    mut infos: Query<
        (&Info, &mut Text, &mut Node),
        (
            Without<Panel>,
            Without<Group>,
            Without<FieldRow>,
            Without<Action>,
        ),
    >,
) {
    if !s.is_changed() && !docs.is_changed() {
        return;
    }
    let visible = active(&docs).is_some();
    for mut n in &mut panel {
        show(&mut n, visible);
    }
    if !visible {
        return;
    }
    let tab = s.sel.tab;
    let sheet = s.sheet.as_deref();
    let doc = docs.current().unwrap();
    let count = sheet.map_or(0, |m| m.len(tab));
    let start = s.sel.item.unwrap_or(0) / ROWS * ROWS;
    let rule = sheet.and_then(|m| s.sel.rule(&m.model));
    let conditions = rule.map_or(0, |r| r.conditions.len());
    let actions = rule.map_or(0, |r| r.actions.len());
    for (g, mut n) in &mut groups {
        show(
            &mut n,
            match g {
                Group::Rule | Group::Condition | Group::Actions => tab == Tab::Rules,
                Group::Picker => s.suggest.is_some(),
                Group::External => doc.external.warning || doc.confirm_reload,
            },
        );
    }
    for (FieldRow(f), mut n) in &mut fields {
        show(&mut n, field_shown(tab, &s.form, *f));
    }
    for (FieldLabel(k), mut t) in &mut labels {
        set(&mut t, field_label(tab, &s.form, *k));
    }
    for (a, mut n, mut bg) in &mut buttons {
        let shown = match a {
            Action::Row(i) => start + i < count,
            Action::Type => tab == Tab::Variables,
            Action::From | Action::To => tab == Tab::Rules && s.form.changed,
            Action::RuleUp | Action::RuleDown => s.sel.item.is_some() && count > 1,
            Action::Option(i) => s
                .suggest
                .zip(sheet)
                .and_then(|((f, base), m)| controls::suggest(m, &s.form, f, base + i))
                .is_some(),
            Action::Suggest(f) => sheet
                .and_then(|m| controls::suggest(m, &s.form, *f, 0))
                .is_some(),
            _ => true,
        };
        show(&mut n, shown);
        let selected = match a {
            Action::Tab(t) => *t == tab,
            Action::Row(i) => Some(start + i) == s.sel.item,
            Action::Once => s.form.once,
            Action::From => s.form.use_from,
            Action::To => s.form.use_to,
            _ => false,
        };
        bg.set_if_neq(BackgroundColor(if selected {
            c::SELECT_BG
        } else {
            c::SLOT_BG
        }));
    }
    use feathrise_editor_documents_model::project_rules::ValueKind;
    for (ButtonLabel(a), mut t) in &mut captions {
        let label = match a {
            Action::Row(i) => sheet.and_then(|m| m.id(tab, start + i)).unwrap_or(""),
            Action::Option(i) => s
                .suggest
                .zip(sheet)
                .and_then(|((f, base), m)| {
                    controls::suggest(m, &s.form, f, base + i)
                        .map(|id| choice_label(f, &s.form, id))
                })
                .unwrap_or(""),
            Action::Type => match s.form.kind {
                ValueKind::Bool => "类型 · 布尔",
                ValueKind::Number => "类型 · 数字",
                ValueKind::Text => "类型 · 文本",
            },
            Action::Trigger => {
                if s.form.changed {
                    "监听 · 变量变化"
                } else {
                    "监听 · 游戏事件"
                }
            }
            Action::ActionType => {
                if s.form.request {
                    "动作 · 发送请求"
                } else {
                    "动作 · 设置变量"
                }
            }
            Action::Once => {
                if s.form.once {
                    "每个存档仅一次 · 开"
                } else {
                    "每个存档仅一次 · 关"
                }
            }
            Action::From => {
                if s.form.use_from {
                    "限定旧值 · 开"
                } else {
                    "限定旧值 · 关"
                }
            }
            Action::To => {
                if s.form.use_to {
                    "限定新值 · 开"
                } else {
                    "限定新值 · 关"
                }
            }
            _ => continue,
        };
        set(&mut t, label);
    }
    for (role, mut t, mut n) in &mut infos {
        show(
            &mut n,
            !matches!(role, Info::Actions | Info::Conditions) || tab == Tab::Rules,
        );
        let value = match role {
            Info::Selection => {
                if let Some(i) = s.sel.item {
                    format!("{} / {count}", i + 1)
                } else {
                    format!("新建 · 现有 {count} 项")
                }
            }
            Info::Conditions => {
                if conditions > 0 {
                    format!(
                        "附加条件（全部满足）   {} / {conditions}",
                        s.sel.condition + 1
                    )
                } else {
                    "附加条件（可选）".into()
                }
            }
            Info::Actions => {
                if actions > 0 {
                    format!("执行动作（按顺序）   {} / {actions}", s.sel.action + 1)
                } else {
                    "第一项执行动作".into()
                }
            }
            Info::Status => {
                if !s.notice.is_empty() {
                    s.notice.clone()
                } else if s.job.is_some() {
                    "正在处理工程规则…".into()
                } else if doc.external.warning {
                    "磁盘版本已变化，可保留草稿或重新载入".into()
                } else if doc.status.contains("保存") && !doc.status.starts_with("原文预览") {
                    doc.status.clone()
                } else {
                    "填写后点「应用到草稿」，Ctrl+S 保存。".into()
                }
            }
        };
        set(&mut t, &value);
    }
}
