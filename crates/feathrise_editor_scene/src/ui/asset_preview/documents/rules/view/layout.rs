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
    let delete = matches!(a, Action::DeleteRow(_));
    let selector = matches!(a, Action::ListTagPicker | Action::ScopeScenePicker);
    let mut e = p.spawn((
        a,
        AuthorButton,
        Name::new(name.into()),
        InteractionButton,
        FocusPolicy::Block,
        Node {
            width: if delete { px(table::ROW_ACTION_WIDTH) } else { Val::Auto },
            min_height: px(metrics::CONTROL_HEIGHT),
            min_width: px(0),
            max_width: percent(100),
            flex_basis: if selector { px(0) } else { Val::Auto },
            flex_grow: if selector { 1. } else { 0. },
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
    if selector {
        // Bevy 的溢出裁剪作用于后代，标签放进独立裁剪框，避免字形盖住同级箭头。
        e.with_children(|p| {
            p.spawn(Node {
                min_width: px(0),
                flex_basis: px(0),
                flex_grow: 1.,
                overflow: Overflow::clip_x(),
                ..default()
            })
            .with_child(text);
        });
        e.with_child((
            Text::new("▼"),
            font(h, metrics::FONT_CONTROL),
            TextLayout::no_wrap(),
            TextColor(c::TEXT_MUTED),
            Pickable::IGNORE,
            Node {
                margin: UiRect::left(px(4)),
                flex_shrink: 0.,
                ..default()
            },
        ));
    } else if let Some(icon) = icon {
        use crate::ui::tooltips::{EditorTooltipTarget, TooltipAction};
        e.insert(EditorTooltipTarget(if delete {
            TooltipAction::DeleteRulesRecord
        } else {
            TooltipAction::EditRulesRecord
        }));
        let glyph = (
            Name::new(if delete { "RulesRowDeleteIcon" } else { "RulesRowEditIcon" }),
            ImageNode {
                image: icon.clone(),
                color: c::TEXT_PRIMARY,
                ..default()
            },
            Node {
                display: if delete { Display::Flex } else { Display::None },
                width: px(14),
                height: px(14),
                ..default()
            },
            Pickable::IGNORE,
        );
        if delete {
            e.with_child(glyph);
        } else {
            e.insert(table::EditVisual::Button);
            e.with_child((text, table::EditVisual::Text));
            e.with_child((glyph, table::EditVisual::Icon));
        }
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
        field(p, h, Field::Tag);
        choices(
            p,
            h,
            Group::Types,
            "变量类型",
            &[
                ("RulesTypeNumber", VariableKind::Number.label(), Action::Type(VariableKind::Number)),
                ("RulesTypeBool", VariableKind::Bool.label(), Action::Type(VariableKind::Bool)),
                ("RulesTypeText", VariableKind::Text.label(), Action::Type(VariableKind::Text)),
                ("RulesTypeEnum", VariableKind::Enum.label(), Action::Type(VariableKind::Enum)),
            ],
        );
        choices(
            p,
            h,
            Group::Boolean,
            "值",
            &[
                ("RulesBoolFalse", "false", Action::BoolDefault(false)),
                ("RulesBoolTrue", "true", Action::BoolDefault(true)),
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
                ("RulesDelete", "删除地点", Action::Delete),
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
fn list_scope(p: &mut ChildSpawnerCommands, h: &Handle<Font>) {
    p.spawn((Group::ListScope, column(5.))).with_children(|p| {
        p.spawn(row()).with_children(|r| {
            buttons(r, h, &[
                ("RulesScopeProject", "工程级", Action::ScopeProject),
                ("RulesScopeScene", "场景级", Action::ScopeScene),
            ]);
        });
        p.spawn(Node { flex_wrap: FlexWrap::NoWrap, ..row() }).with_children(|r| {
            buttons(r, h, &[
                ("RulesListTagPicker", "标签 · 全部", Action::ListTagPicker),
                ("RulesScopeScenePicker", "场景", Action::ScopeScenePicker),
            ]);
        });
        // 选择面板覆盖列表，展开、翻选项或关闭都不会重新排列现有记录。
        p.spawn((
            Group::ListOptions,
            Name::new("RulesListOptions"),
            GlobalZIndex(422), // 高于工作台 420，低于独立文件浮窗。
            FocusPolicy::Block,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: percent(100),
                left: px(0),
                right: px(0),
                padding: UiRect::all(px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(metrics::RADIUS)),
                ..column(4.)
            },
            BackgroundColor(c::DIALOG_BG),
            BorderColor::all(c::DIALOG_BORDER),
        )).with_children(|popup| {
            for i in 0..OPTIONS {
                button(popup, h, format!("RulesListOption{i}"), "", Action::ListOption(i));
            }
            popup.spawn(row()).with_children(|r| {
                buttons(r, h, &[
                    ("RulesListOptionsPrevious", "上一组", Action::ListOptionsPrevious),
                    ("RulesListOptionsNext", "下一组", Action::ListOptionsNext),
                    ("RulesListOptionsClose", "收起", Action::ListOptionsClose),
                ]);
            });
        });
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
    let delete_icon = server.load("icons/editor/tool_delete.png");
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
                    buttons(
                        r,
                        &h,
                        &[
                            ("RulesUndo", "撤销", Action::Undo),
                            ("RulesSource", "高级源码", Action::Source),
                        ],
                    )
                });
            });
            info(p, &h, Info::Status, "RulesStatus");
            list_scope(p, &h);
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
                        .with_children(|items| table::list(items, &h, &edit_icon, &delete_icon));
                });
            // 草稿和文件冲突有各自的操作，放在表格之后，显隐不会推移记录。
            p.spawn(row()).with_children(|r| {
                button(r, &h, "RulesRedo", "重做", Action::Redo);
                r.spawn((Group::DraftActions, row())).with_children(|draft| {
                    button(draft, &h, "RulesListSave", "保存更改", Action::Save);
                    button(draft, &h, "RulesListCancel", "取消", Action::Cancel);
                });
            });
            p.spawn((Group::External, row())).with_children(|r| {
                button(r, &h, "RulesKeepDraft", "保留当前修改", Action::KeepDraft);
                button(r, &h, "RulesReload", "重新载入磁盘文件", Action::Reload);
            });
        });
}
