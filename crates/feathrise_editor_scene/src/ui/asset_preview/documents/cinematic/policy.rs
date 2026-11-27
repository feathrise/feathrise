//! 演出播放次数、条件和收尾动作的结构化表单。固定控件编辑当前项，保存仍走原文档事务。

use super::{model::number, view, *};
use feathrise_cinematic::{FinishAction, PlaybackCondition, RepeatPolicy};
use feathrise_editor_ui::colors as c;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Repeat {
    #[default]
    Every,
    Save,
    Quest,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ConditionKind {
    #[default]
    Quest,
    Item,
    Seen,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FinishKind {
    #[default]
    Variable,
    Quest,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ValueKind {
    #[default]
    Number,
    Bool,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    RepeatQuest,
    ConditionTarget,
    Status,
    Stage,
    FinishTarget,
    Value,
    From,
    To,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Repeat(Repeat),
    Condition(ConditionKind),
    Finish(FinishKind),
    Value(ValueKind),
    ConditionAdd,
    ConditionDelete,
    ConditionPrevious,
    ConditionNext,
    Seen,
    FinishAdd,
    FinishDelete,
    FinishPrevious,
    FinishNext,
    Bool,
}

#[derive(Clone, Default)]
struct Condition {
    kind: ConditionKind,
    target: String,
    status: String,
    stage: String,
    seen: bool,
}
#[derive(Clone, Default)]
struct Finish {
    kind: FinishKind,
    target: String,
    value: String,
    value_kind: ValueKind,
    from: String,
    to: String,
}
#[derive(Clone, Default)]
pub(super) struct Policy {
    repeat: Repeat,
    quest: String,
    conditions: Vec<Condition>,
    finishes: Vec<Finish>,
    condition: usize,
    finish: usize,
}
impl Policy {
    pub fn load(film: &feathrise_cinematic::Cinematic) -> Self {
        let (repeat, quest) = match &film.repeat {
            RepeatPolicy::EveryTime => (Repeat::Every, String::new()),
            RepeatPolicy::OncePerSave => (Repeat::Save, String::new()),
            RepeatPolicy::OncePerQuest { quest } => (Repeat::Quest, quest.clone()),
        };
        Self {
            repeat,
            quest,
            conditions: film
                .conditions
                .iter()
                .map(|condition| match condition {
                    PlaybackCondition::Quest {
                        quest,
                        status,
                        stage,
                    } => Condition {
                        target: quest.clone(),
                        status: status.clone().unwrap_or_default(),
                        stage: stage.map(|stage| stage.to_string()).unwrap_or_default(),
                        ..default()
                    },
                    PlaybackCondition::HasItem { item } => Condition {
                        kind: ConditionKind::Item,
                        target: item.clone(),
                        ..default()
                    },
                    PlaybackCondition::Seen { cinematic, seen } => Condition {
                        kind: ConditionKind::Seen,
                        target: cinematic.clone(),
                        seen: *seen,
                        ..default()
                    },
                })
                .collect(),
            finishes: film
                .on_finish
                .iter()
                .map(|action| match action {
                    FinishAction::SetVariable { key, value } => Finish {
                        target: key.clone(),
                        value: value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string()),
                        value_kind: if value.is_boolean() {
                            ValueKind::Bool
                        } else if value.is_string() {
                            ValueKind::Text
                        } else {
                            ValueKind::Number
                        },
                        ..default()
                    },
                    FinishAction::QuestObjective { quest, from, to } => Finish {
                        kind: FinishKind::Quest,
                        target: quest.clone(),
                        from: from.to_string(),
                        to: to.to_string(),
                        ..default()
                    },
                })
                .collect(),
            ..default()
        }
    }
    /// 字符串只在后台应用时转换，切换当前项可以保留尚未填写完整的输入。
    pub fn apply(self, film: &mut feathrise_cinematic::Cinematic) -> Result<(), String> {
        film.repeat = match self.repeat {
            Repeat::Every => RepeatPolicy::EveryTime,
            Repeat::Save => RepeatPolicy::OncePerSave,
            Repeat::Quest => RepeatPolicy::OncePerQuest {
                quest: self.quest.trim().into(),
            },
        };
        film.conditions = self
            .conditions
            .into_iter()
            .map(|condition| {
                Ok(match condition.kind {
                    ConditionKind::Quest => PlaybackCondition::Quest {
                        quest: condition.target.trim().into(),
                        status: (!condition.status.trim().is_empty())
                            .then(|| condition.status.trim().into()),
                        stage: optional_stage(&condition.stage)?,
                    },
                    ConditionKind::Item => PlaybackCondition::HasItem {
                        item: condition.target.trim().into(),
                    },
                    ConditionKind::Seen => PlaybackCondition::Seen {
                        cinematic: condition.target.trim().into(),
                        seen: condition.seen,
                    },
                })
            })
            .collect::<Result<_, String>>()?;
        film.on_finish = self
            .finishes
            .into_iter()
            .map(|finish| {
                Ok(match finish.kind {
                    FinishKind::Variable => FinishAction::SetVariable {
                        key: finish.target.trim().into(),
                        value: match finish.value_kind {
                            ValueKind::Bool => serde_json::Value::Bool(finish.value == "true"),
                            ValueKind::Number => serde_json::json!(number(&finish.value)?),
                            ValueKind::Text => serde_json::Value::String(finish.value),
                        },
                    },
                    FinishKind::Quest => FinishAction::QuestObjective {
                        quest: finish.target.trim().into(),
                        from: stage(&finish.from)?,
                        to: stage(&finish.to)?,
                    },
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(())
    }
    pub fn field(&mut self, field: Field) -> Option<&mut String> {
        match field {
            Field::RepeatQuest => Some(&mut self.quest),
            Field::ConditionTarget => Some(&mut self.conditions.get_mut(self.condition)?.target),
            Field::Status => Some(&mut self.conditions.get_mut(self.condition)?.status),
            Field::Stage => Some(&mut self.conditions.get_mut(self.condition)?.stage),
            Field::FinishTarget => Some(&mut self.finishes.get_mut(self.finish)?.target),
            Field::Value => Some(&mut self.finishes.get_mut(self.finish)?.value),
            Field::From => Some(&mut self.finishes.get_mut(self.finish)?.from),
            Field::To => Some(&mut self.finishes.get_mut(self.finish)?.to),
        }
    }
    pub fn value(&self, field: Field) -> &str {
        match field {
            Field::RepeatQuest => &self.quest,
            Field::ConditionTarget => self
                .conditions
                .get(self.condition)
                .map_or("", |c| &c.target),
            Field::Status => self
                .conditions
                .get(self.condition)
                .map_or("", |c| &c.status),
            Field::Stage => self.conditions.get(self.condition).map_or("", |c| &c.stage),
            Field::FinishTarget => self.finishes.get(self.finish).map_or("", |f| &f.target),
            Field::Value => self.finishes.get(self.finish).map_or("", |f| &f.value),
            Field::From => self.finishes.get(self.finish).map_or("", |f| &f.from),
            Field::To => self.finishes.get(self.finish).map_or("", |f| &f.to),
        }
    }
    pub fn label(&self, field: Field) -> Option<&'static str> {
        let condition = self.conditions.get(self.condition);
        let finish = self.finishes.get(self.finish);
        match field {
            Field::RepeatQuest => (self.repeat == Repeat::Quest).then_some("关联任务 ID"),
            Field::ConditionTarget => condition.map(|c| match c.kind {
                ConditionKind::Quest => "任务 ID",
                ConditionKind::Item => "物品标识",
                ConditionKind::Seen => "演出 ID",
            }),
            Field::Status => condition
                .filter(|c| c.kind == ConditionKind::Quest)
                .map(|_| "任务状态"),
            Field::Stage => condition
                .filter(|c| c.kind == ConditionKind::Quest)
                .map(|_| "阶段（可空）"),
            Field::FinishTarget => finish.map(|f| {
                if f.kind == FinishKind::Quest {
                    "任务 ID"
                } else {
                    "变量名称"
                }
            }),
            Field::Value => finish
                .filter(|f| f.kind == FinishKind::Variable && f.value_kind != ValueKind::Bool)
                .map(|_| "值"),
            Field::From => finish
                .filter(|f| f.kind == FinishKind::Quest)
                .map(|_| "原目标序号"),
            Field::To => finish
                .filter(|f| f.kind == FinishKind::Quest)
                .map(|_| "新目标序号"),
        }
    }
    pub fn shown(&self, action: Action) -> bool {
        match action {
            Action::Condition(_) | Action::ConditionDelete => !self.conditions.is_empty(),
            Action::ConditionPrevious => self.condition > 0,
            Action::ConditionNext => self.condition + 1 < self.conditions.len(),
            Action::Seen => self
                .conditions
                .get(self.condition)
                .is_some_and(|c| c.kind == ConditionKind::Seen),
            Action::Finish(_) | Action::FinishDelete => !self.finishes.is_empty(),
            Action::FinishPrevious => self.finish > 0,
            Action::FinishNext => self.finish + 1 < self.finishes.len(),
            Action::Value(_) => self
                .finishes
                .get(self.finish)
                .is_some_and(|f| f.kind == FinishKind::Variable),
            Action::Bool => self
                .finishes
                .get(self.finish)
                .is_some_and(|f| f.kind == FinishKind::Variable && f.value_kind == ValueKind::Bool),
            _ => true,
        }
    }
    pub fn selected(&self, action: Action) -> bool {
        match action {
            Action::Repeat(repeat) => self.repeat == repeat,
            Action::Condition(kind) => self
                .conditions
                .get(self.condition)
                .is_some_and(|c| c.kind == kind),
            Action::Finish(kind) => self
                .finishes
                .get(self.finish)
                .is_some_and(|f| f.kind == kind),
            Action::Value(kind) => self
                .finishes
                .get(self.finish)
                .is_some_and(|f| f.value_kind == kind),
            _ => false,
        }
    }
    pub fn act(&mut self, action: Action) {
        match action {
            Action::Repeat(repeat) => self.repeat = repeat,
            Action::ConditionAdd => {
                self.condition = self.conditions.len();
                self.conditions.push(Condition::default());
            }
            Action::ConditionDelete => {
                if !self.conditions.is_empty() {
                    self.conditions.remove(self.condition);
                    self.condition = self.condition.saturating_sub(1);
                }
            }
            Action::ConditionPrevious => self.condition = self.condition.saturating_sub(1),
            Action::ConditionNext => {
                self.condition = (self.condition + 1).min(self.conditions.len().saturating_sub(1))
            }
            Action::Condition(kind) => {
                if let Some(c) = self.conditions.get_mut(self.condition) {
                    c.kind = kind;
                }
            }
            Action::Seen => {
                if let Some(c) = self.conditions.get_mut(self.condition) {
                    c.seen = !c.seen;
                }
            }
            Action::FinishAdd => {
                self.finish = self.finishes.len();
                self.finishes.push(Finish {
                    value: "0".into(),
                    from: "0".into(),
                    to: "1".into(),
                    ..default()
                });
            }
            Action::FinishDelete => {
                if !self.finishes.is_empty() {
                    self.finishes.remove(self.finish);
                    self.finish = self.finish.saturating_sub(1);
                }
            }
            Action::FinishPrevious => self.finish = self.finish.saturating_sub(1),
            Action::FinishNext => {
                self.finish = (self.finish + 1).min(self.finishes.len().saturating_sub(1))
            }
            Action::Finish(kind) => {
                if let Some(f) = self.finishes.get_mut(self.finish) {
                    f.kind = kind;
                }
            }
            Action::Value(kind) => {
                if let Some(f) = self.finishes.get_mut(self.finish) {
                    if f.value_kind != kind {
                        f.value = match kind {
                            ValueKind::Number => "0",
                            ValueKind::Bool => "false",
                            ValueKind::Text => "",
                        }
                        .into();
                        f.value_kind = kind;
                    }
                }
            }
            Action::Bool => {
                if let Some(f) = self.finishes.get_mut(self.finish) {
                    f.value = if f.value == "true" { "false" } else { "true" }.into();
                }
            }
        }
    }
}
fn stage(value: &str) -> Result<u32, String> {
    value
        .trim()
        .parse()
        .map_err(|_| "任务阶段或目标序号需要非负整数".into())
}
fn optional_stage(value: &str) -> Result<Option<u32>, String> {
    if value.trim().is_empty() {
        Ok(None)
    } else {
        stage(value).map(Some)
    }
}

#[derive(Component)]
pub(super) struct Root;
#[derive(Component)]
pub(super) struct Count(bool);

pub(super) fn mount(p: &mut ChildSpawnerCommands, h: &Handle<Font>) {
    p.spawn((
        Root,
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            flex_shrink: 0.,
            ..default()
        },
    ))
    .with_children(|p| {
        caption(p, h, "播放次数");
        choices(
            p,
            h,
            &[
                ("每次", Action::Repeat(Repeat::Every)),
                ("每存档一次", Action::Repeat(Repeat::Save)),
                ("每次任务一次", Action::Repeat(Repeat::Quest)),
            ],
        );
        view::field(p, h, view::Field::Policy(Field::RepeatQuest));
        p.spawn((
            Count(true),
            Text::default(),
            TextLayout::no_wrap(),
            view::font(h, 11.),
            TextColor(c::TEXT_MUTED),
            Node {
                width: percent(100),
                ..default()
            },
        ));
        choices(
            p,
            h,
            &[
                ("上一条件", Action::ConditionPrevious),
                ("下一条件", Action::ConditionNext),
                ("新增条件", Action::ConditionAdd),
                ("删除条件", Action::ConditionDelete),
            ],
        );
        choices(
            p,
            h,
            &[
                ("任务", Action::Condition(ConditionKind::Quest)),
                ("持有物品", Action::Condition(ConditionKind::Item)),
                ("已看记录", Action::Condition(ConditionKind::Seen)),
            ],
        );
        for field in [Field::ConditionTarget, Field::Status, Field::Stage] {
            view::field(p, h, view::Field::Policy(field));
        }
        choices(p, h, &[("尚未看过", Action::Seen)]);
        p.spawn((
            Count(false),
            Text::default(),
            TextLayout::no_wrap(),
            view::font(h, 11.),
            TextColor(c::TEXT_MUTED),
            Node {
                width: percent(100),
                ..default()
            },
        ));
        choices(
            p,
            h,
            &[
                ("上一动作", Action::FinishPrevious),
                ("下一动作", Action::FinishNext),
                ("新增动作", Action::FinishAdd),
                ("删除动作", Action::FinishDelete),
            ],
        );
        choices(
            p,
            h,
            &[
                ("设置变量", Action::Finish(FinishKind::Variable)),
                ("推进任务", Action::Finish(FinishKind::Quest)),
            ],
        );
        view::field(p, h, view::Field::Policy(Field::FinishTarget));
        choices(
            p,
            h,
            &[
                ("number", Action::Value(ValueKind::Number)),
                ("bool", Action::Value(ValueKind::Bool)),
                ("text", Action::Value(ValueKind::Text)),
            ],
        );
        for field in [Field::Value, Field::From, Field::To] {
            view::field(p, h, view::Field::Policy(field));
        }
        choices(p, h, &[("false", Action::Bool)]);
    });
}
fn caption(p: &mut ChildSpawnerCommands, h: &Handle<Font>, text: &str) {
    p.spawn((
        Text::new(text),
        TextLayout::no_wrap(),
        view::font(h, 11.),
        TextColor(c::TEXT_MUTED),
        Node {
            width: percent(100),
            ..default()
        },
    ));
}
fn choices(p: &mut ChildSpawnerCommands, h: &Handle<Font>, choices: &[(&str, Action)]) {
    p.spawn(view::row()).with_children(|p| {
        for &(label, action) in choices {
            view::button(p, h, label, view::Action::Policy(action));
        }
    });
}
pub(super) fn display(
    s: Res<Studio>,
    mut root: Query<&mut Node, With<Root>>,
    mut counts: Query<(&Count, &mut Text), Without<view::OptionLabel>>,
    mut buttons: Query<(&view::Action, &mut BackgroundColor, &mut BorderColor)>,
    mut captions: Query<(&view::OptionLabel, &mut Text)>,
) {
    if !s.is_changed() {
        return;
    }
    for mut node in &mut root {
        node.display = if s.settings {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (Count(condition), mut text) in &mut counts {
        let (noun, index, len) = if *condition {
            (
                "播放条件 · 全部满足",
                s.policy.condition,
                s.policy.conditions.len(),
            )
        } else {
            ("收尾动作", s.policy.finish, s.policy.finishes.len())
        };
        **text = if len == 0 {
            format!("{noun} · 0 项")
        } else {
            format!("{noun} · {} / {len}", index + 1)
        };
    }
    for (action, mut background, mut border) in &mut buttons {
        if let view::Action::Policy(action) = action {
            let selected = s.policy.selected(*action);
            background.set_if_neq(BackgroundColor(if selected {
                c::SELECT_BG
            } else {
                c::SLOT_BG
            }));
            border.set_if_neq(BorderColor::all(if selected {
                c::COPPER_BORDER
            } else {
                c::COPPER_BORDER_WEAK
            }));
        }
    }
    for (label, mut text) in &mut captions {
        match label.0 {
            view::Action::Policy(Action::Seen) => {
                **text = if s
                    .policy
                    .conditions
                    .get(s.policy.condition)
                    .is_some_and(|c| c.seen)
                {
                    "已经看过"
                } else {
                    "尚未看过"
                }
                .into()
            }
            view::Action::Policy(Action::Bool) => {
                **text = s
                    .policy
                    .finishes
                    .get(s.policy.finish)
                    .map_or("false", |f| f.value.as_str())
                    .into()
            }
            _ => {}
        }
    }
}
