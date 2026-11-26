//! 工程工具以各自的列表打开，变量行和规则、地点详情共用一套原生输入。
use super::*;

#[derive(Component)]
pub(super) struct ScrollArea;
pub(super) fn font(h: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: h.clone().into(),
        font_size: size.into(),
        ..default()
    }
}
pub(super) fn column(gap: f32) -> Node {
    Node {
        min_width: px(0),
        flex_shrink: 0.,
        flex_direction: FlexDirection::Column,
        row_gap: px(gap),
        ..default()
    }
}
pub(super) fn row() -> Node {
    Node {
        min_width: px(0),
        flex_wrap: FlexWrap::Wrap,
        align_items: AlignItems::Center,
        column_gap: px(5),
        row_gap: px(4),
        flex_shrink: 0.,
        ..default()
    }
}
pub(super) fn button(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    name: impl Into<String>,
    label: &str,
    a: Action,
) {
    button_with_icon(p, h, name, label, a, None);
}
pub(super) fn button_with_icon(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    name: impl Into<String>,
    label: &str,
    a: Action,
    icon: Option<&Handle<Image>>,
) {
    let mut e = p.spawn((
        a,
        AuthorButton,
        Name::new(name.into()),
        InteractionButton,
        FocusPolicy::Block,
        Node {
            min_height: px(metrics::CONTROL_HEIGHT),
            min_width: px(0),
            max_width: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(8)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(metrics::RADIUS)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(c::SLOT_BG),
        BorderColor::all(c::COPPER_BORDER_WEAK),
    ));
    // 保留通用文档命令身份，表单读取未确认的字段后再交给同一保存链。
    match a {
        Action::Save => {
            e.insert(super::super::super::components::DocumentAction::Save);
        }
        Action::Source => {
            e.insert(super::super::super::components::DocumentAction::Edit);
        }
        _ => {}
    }
    let text = (
        ButtonLabel(a),
        Text::new(label),
        font(h, metrics::FONT_CONTROL),
        TextLayout::no_wrap(),
        TextColor(c::TEXT_PRIMARY),
        Pickable::IGNORE,
    );
    if let Some(icon) = icon {
        use crate::ui::tooltips::{EditorTooltipTarget, TooltipAction};
        e.insert(EditorTooltipTarget(TooltipAction::EditRulesRecord));
        e.with_child((text, table::EditVisual::Text));
        e.with_child((
            Name::new("RulesRowEditIcon"),
            table::EditVisual::Icon,
            ImageNode {
                image: icon.clone(),
                color: c::TEXT_PRIMARY,
                ..default()
            },
            Node {
                display: Display::None,
                width: px(14),
                height: px(14),
                ..default()
            },
            Pickable::IGNORE,
        ));
    } else {
        e.with_child(text);
    }
}
fn buttons(p: &mut ChildSpawnerCommands, h: &Handle<Font>, items: &[(&str, &str, Action)]) {
    for &(name, label, a) in items {
        button(p, h, name, label, a);
    }
}
pub(super) fn info(p: &mut ChildSpawnerCommands, h: &Handle<Font>, role: Info, name: &str) {
    let heading = matches!(
        role,
        Info::PageTitle | Info::EditorTitle | Info::Conditions | Info::Actions
    );
    // 标题作为同行的完整词组参与测量，空间不足时整组按钮换行。
    let one_line = !matches!(role, Info::Yarn | Info::Status);
    let layout = if one_line {
        TextLayout::no_wrap()
    } else {
        TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter)
    };
    let minimum = if matches!(role, Info::Selection) {
        px(70)
    } else {
        px(0)
    };
    p.spawn((
        role,
        Name::new(name.to_owned()),
        Text::default(),
        layout,
        font(
            h,
            if matches!(role, Info::PageTitle) {
                18.
            } else if heading {
                metrics::FONT_BODY
            } else {
                metrics::FONT_CONTROL
            },
        ),
        TextColor(if heading {
            c::TEXT_PRIMARY
        } else {
            c::TEXT_MUTED
        }),
        Pickable::IGNORE,
        Node {
            // 弹窗标题和状态显式拿到实际行宽，避免文字先按字估高、再按宽行栅格化。
            width: if matches!(role, Info::EditorTitle | Info::Status) { percent(100) } else { Val::Auto },
            overflow: if matches!(role, Info::EditorTitle) { Overflow::clip_x() } else { Overflow::visible() },
            min_width: minimum,
            max_width: percent(100),
            flex_shrink: 0.,
            ..default()
        },
    ));
}
fn field(p: &mut ChildSpawnerCommands, h: &Handle<Font>, f: Field) {
    // 行外壳也是候选项的挂载点，打开选择器时在这个字段下面展开。
    p.spawn((FieldRow(f), column(4.))).with_children(|section| {
        section.spawn((
            FieldLabel(f),
            Text::default(),
            font(h, metrics::FONT_CONTROL),
            TextColor(c::TEXT_MUTED),
            Pickable::IGNORE,
        ));
        section.spawn(row()).with_children(|r| {
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
                font(h, metrics::FONT_BODY),
                LineHeight::Px(18.),
                TextColor(c::TEXT_VALUE),
                BackgroundColor(c::INPUT_BG),
                BorderColor::all(c::INPUT_BORDER),
                Node {
                    height: px(metrics::CONTROL_HEIGHT),
                    min_width: px(30),
                    flex_basis: px(0),
                    flex_grow: 1.,
                    padding: UiRect::horizontal(px(6)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(metrics::RADIUS)),
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
    });
}
fn choices(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    group: Group,
    caption: &str,
    items: &[(&str, &str, Action)],
) {
    p.spawn((group, column(4.))).with_children(|r| {
        r.spawn((
            Text::new(caption),
            font(h, metrics::FONT_CONTROL),
            TextColor(c::TEXT_MUTED),
            Pickable::IGNORE,
        ));
        r.spawn(row()).with_children(|r| buttons(r, h, items));
    });
}
fn rule_section(
    p: &mut ChildSpawnerCommands,
    h: &Handle<Font>,
    role: Info,
    name: &str,
    items: &[(&str, &str, Action)],
) {
    p.spawn((
        Group::Rule,
        Node {
            margin: UiRect::top(px(4)),
            ..row()
        },
    ))
    .with_children(|r| {
        info(r, h, role, name);
        buttons(r, h, items);
    });
}
pub(super) fn editor(p: &mut ChildSpawnerCommands, h: &Handle<Font>) {
    info(p, h, Info::EditorTitle, "RulesEditorTitle");
    p.spawn(column(6.)).with_children(|p| {
        field(p, h, Field::Id);
        choices(
            p,
            h,
            Group::Types,
            "变量类型",
            &[
                ("RulesTypeNumber", "数字", Action::Type(VariableKind::Number)),
                ("RulesTypeBool", "布尔", Action::Type(VariableKind::Bool)),
                ("RulesTypeText", "文本", Action::Type(VariableKind::Text)),
                ("RulesTypeEnum", "枚举", Action::Type(VariableKind::Enum)),
            ],
        );
        choices(
            p,
            h,
            Group::Boolean,
            "默认值",
            &[
                ("RulesBoolFalse", "假", Action::BoolDefault(false)),
                ("RulesBoolTrue", "真", Action::BoolDefault(true)),
            ],
        );
        field(p, h, Field::Choices);
        field(p, h, Field::Default);
    });
    info(p, h, Info::Yarn, "RulesYarnName");
    for f in [Field::Scene, Field::Min, Field::Max] {
        field(p, h, f);
    }
    p.spawn((Group::Location, row())).with_children(|r| {
        button(
            r,
            h,
            "RulesDrawLocation",
            "在视口绘制范围",
            Action::DrawLocation,
        );
    });
    choices(
        p,
        h,
        Group::Rule,
        "监听",
        &[
            ("RulesTriggerEvent", "游戏事件", Action::Trigger(false)),
            ("RulesTrigger", "变量变化", Action::Trigger(true)),
        ],
    );
    field(p, h, Field::Trigger);
    field(p, h, Field::EventTarget);
    choices(
        p,
        h,
        Group::Limits,
        "限定取值",
        &[
            ("RulesFrom", "限定旧值", Action::From),
            ("RulesTo", "限定新值", Action::To),
        ],
    );
    field(p, h, Field::From);
    field(p, h, Field::To);
    choices(
        p,
        h,
        Group::Rule,
        "重复执行",
        &[("RulesOnce", "每个存档仅一次", Action::Once)],
    );
    rule_section(
        p,
        h,
        Info::Conditions,
        "RulesConditions",
        &[
            ("RulesConditionAdd", "追加条件", Action::ConditionAdd),
            (
                "RulesConditionPrevious",
                "上一条件",
                Action::ConditionPrevious,
            ),
            ("RulesConditionNext", "下一条件", Action::ConditionNext),
            ("RulesConditionDelete", "删除条件", Action::ConditionDelete),
        ],
    );
    field(p, h, Field::ConditionVariable);
    field(p, h, Field::ConditionValue);
    rule_section(
        p,
        h,
        Info::Actions,
        "RulesActions",
        &[
            ("RulesActionAdd", "追加动作", Action::ActionAdd),
            ("RulesActionPrevious", "上一动作", Action::ActionPrevious),
            ("RulesActionNext", "下一动作", Action::ActionNext),
            ("RulesActionUp", "动作上移", Action::ActionUp),
            ("RulesActionDown", "动作下移", Action::ActionDown),
            ("RulesActionDelete", "删除动作", Action::ActionDelete),
        ],
    );
    choices(
        p,
        h,
        Group::Rule,
        "动作类型",
        &[
            (
                "RulesActionSet",
                "设置变量",
                Action::ActionType(Effect::Set),
            ),
            (
                "RulesActionPlayCinematic",
                "播放剧情演出",
                Action::ActionType(Effect::Play),
            ),
            (
                "RulesActionStartQuest",
                "开始任务",
                Action::ActionType(Effect::StartQuest),
            ),
            (
                "RulesActionType",
                "其他请求",
                Action::ActionType(Effect::Other),
            ),
        ],
    );
    field(p, h, Field::ActionName);
    field(p, h, Field::ActionValue);
    // 保存当前项直接写入工程。按钮放在末尾字段旁边，键盘也走同一个命令。
    p.spawn((super::editor_dialog::FormActions, Node {
        margin: UiRect::top(px(4)),
        ..row()
    }))
    .with_children(|r| {
        buttons(
            r,
            h,
            &[
                ("RulesSave", "保存变量", Action::Save),
                ("RulesCancel", "取消", Action::Cancel),
                ("RulesRuleUp", "规则上移", Action::RuleUp),
                ("RulesRuleDown", "规则下移", Action::RuleDown),
                ("RulesDelete", "删除变量", Action::Delete),
            ],
        )
    });
    p.spawn((
        Group::Picker,
        Name::new("RulesPicker"),
        Node {
            display: Display::None,
            padding: UiRect::all(px(5)),
            ..row()
        },
        BackgroundColor(c::PANEL_BG_SECTION),
    ))
    .with_children(|r| {
        button(
            r,
            h,
            "RulesOptionsPrevious",
            "上一组",
            Action::OptionsPrevious,
        );
        for i in 0..OPTIONS {
            button(r, h, format!("RulesOption{i}"), "", Action::Option(i));
        }
        button(r, h, "RulesOptionsNext", "下一组", Action::OptionsNext);
        button(r, h, "RulesOptionsClose", "收起", Action::OptionsClose);
    });
}
pub(super) fn mount(
    mut commands: Commands,
    sidebar: Query<Entity, Added<crate::ui::gameplay_sidebar::InteractionRulesHost>>,
    server: Res<AssetServer>,
) {
    let Ok(host) = sidebar.single() else { return };
    let h = feathrise_editor_core::editor_font(&server, feathrise_editor_core::FONT_PATH);
    let edit_icon = server.load("icons/editor/tool_edit.png");
    let workbench = commands
        .spawn((
            Panel,
            bevy::ui::RelativeCursorPosition::default(),
            Name::new("工程交互工作区"),
            ChildOf(host),
            Node {
                display: Display::None,
                width: percent(100),
                min_width: px(0),
                min_height: px(0),
                ..column(0.)
            },
            BackgroundColor(c::PANEL_BG_STRONG),
        ))
        .id();
    commands.spawn((
        SidebarStatus,
        ChildOf(workbench),
        Text::default(),
        font(&h, metrics::FONT_CONTROL),
        TextColor(c::TEXT_MUTED),
        Node {
            padding: UiRect::all(px(super::super::RULES_SHEET_PADDING)),
            ..default()
        },
    ));
    commands
        .spawn((
            PanelBody,
            Name::new("RulesWorkbench"),
            ChildOf(workbench),
            Node {
                min_height: px(0),
                padding: UiRect::all(px(super::super::RULES_SHEET_PADDING)),
                ..column(6.)
            },
        ))
        .with_children(|p| {
            p.spawn(column(5.)).with_children(|header| {
                info(header, &h, Info::PageTitle, "RulesPageTitle");
                header.spawn(row()).with_children(|r| {
                    // 列表上的撤销/重做也会生成文档草稿，在原位保留保存与取消入口。
                    r.spawn((Group::DraftActions, row()))
                        .with_children(|draft| {
                            button(draft, &h, "RulesListSave", "保存更改", Action::Save);
                            button(draft, &h, "RulesListCancel", "取消", Action::Cancel);
                        });
                    buttons(
                        r,
                        &h,
                        &[
                            ("RulesUndo", "撤销", Action::Undo),
                            ("RulesRedo", "重做", Action::Redo),
                            ("RulesSource", "高级源码", Action::Source),
                        ],
                    )
                });
            });
            info(p, &h, Info::Status, "RulesStatus");
            p.spawn((Group::External, row())).with_children(|r| {
                button(r, &h, "RulesKeepDraft", "保留当前修改", Action::KeepDraft);
                button(r, &h, "RulesReload", "重新载入磁盘文件", Action::Reload);
            });
            p.spawn((ScrollArea, Name::new("RulesFormScroll"), column(0.)))
                .with_children(|scroll| {
                    scroll
                        .spawn((
                            Name::new("RulesList"),
                            Node {
                                width: percent(100),
                                ..column(5.)
                            },
                        ))
                        .with_children(|items| table::list(items, &h, &edit_icon));
                });
        });
}
