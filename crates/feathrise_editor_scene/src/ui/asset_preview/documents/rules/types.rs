//! 工作台只保存当前表单与选择位置，文档正文、历史和磁盘身份仍由原会话持有。
use feathrise_editor_documents_model::project_rules::{
    ActionForm, ConditionForm, LocationForm, RuleForm, RulesForm, TriggerForm, ValueKind,
    VariableForm,
};
use std::collections::{BTreeMap, BTreeSet};

/// 枚举沿用规则版本一的文本与 choices，已有工程直接显示为枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VariableKind {
    Bool,
    Number,
    Text,
    Enum,
}
impl VariableKind {
    pub fn of(variable: &VariableForm) -> Self {
        match variable.kind {
            ValueKind::Bool => Self::Bool,
            ValueKind::Number => Self::Number,
            ValueKind::Text if variable.choices.is_some() => Self::Enum,
            ValueKind::Text => Self::Text,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Bool => "布尔",
            Self::Number => "数字",
            Self::Text => "文本",
            Self::Enum => "枚举",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Variables,
    Rules,
    Locations,
}
impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Variables => "工程变量",
            Self::Rules => "触发规则",
            Self::Locations => "地点区域",
        }
    }
    pub fn noun(self) -> &'static str {
        match self {
            Self::Variables => "变量",
            Self::Rules => "规则",
            Self::Locations => "地点",
        }
    }
}

/// 类型切换时暂存当前输入；只把作者最终选中的类型写入工程文件。
#[derive(Default)]
pub(super) struct AlternateInputs {
    defaults: Vec<(VariableKind, String)>,
    pub trigger: String,
    pub action_name: String,
    pub action_value: String,
}
impl AlternateInputs {
    /// 快捷动作和自定义请求共用同一份请求字段；切回变量时恢复之前填写的变量和值。
    pub fn change_effect(&mut self, form: &mut Form, effect: Effect) {
        let request = effect != Effect::Set;
        if request != form.request {
            form.request = request;
            std::mem::swap(&mut form.action_name, &mut self.action_name);
            std::mem::swap(&mut form.action_value, &mut self.action_value);
        }
        if request && Effect::of(form) != effect {
            form.action_name = match effect {
                Effect::Play => "cinematic.play",
                Effect::StartQuest => "quest.start",
                _ => "",
            }
            .into();
            form.action_value.clear();
        }
    }
    pub fn change_type(&mut self, form: &mut Form, kind: VariableKind) {
        let next = self
            .defaults
            .iter()
            .position(|(stored, _)| *stored == kind)
            .map(|index| self.defaults.swap_remove(index).1)
            .unwrap_or_else(|| match kind {
                VariableKind::Bool => "false".into(),
                VariableKind::Number => "0".into(),
                VariableKind::Text | VariableKind::Enum => String::new(),
            });
        let previous = std::mem::replace(&mut form.default_value, next);
        self.defaults.push((form.kind, previous));
        form.kind = kind;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Effect {
    Set,
    Play,
    StartQuest,
    Other,
}
impl Effect {
    pub fn of(form: &Form) -> Self {
        if !form.request {
            return Self::Set;
        }
        match form.action_name.as_str() {
            "cinematic.play" => Self::Play,
            "quest.start" => Self::StartQuest,
            _ => Self::Other,
        }
    }
}

#[derive(bevy::prelude::Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Id,
    Default,
    Choices,
    Trigger,
    From,
    To,
    EventTarget,
    ConditionVariable,
    ConditionValue,
    ActionName,
    ActionValue,
    Scene,
    Min,
    Max,
}

#[derive(Clone, PartialEq)]
pub(super) struct Form {
    pub id: String,
    pub kind: VariableKind,
    pub default_value: String,
    pub choices: String,
    pub changed: bool,
    pub trigger: String,
    pub from: String,
    pub to: String,
    pub use_from: bool,
    pub use_to: bool,
    pub event_target: String,
    pub once: bool,
    pub condition_variable: String,
    pub condition_value: String,
    pub request: bool,
    pub action_name: String,
    pub action_value: String,
    pub scene: String,
    pub min: String,
    pub max: String,
}
impl Default for Form {
    fn default() -> Self {
        Self {
            id: String::new(),
            // 数值行可直接填写 a 和 3；切换类型再采用该类型自己的初值。
            kind: VariableKind::Number,
            default_value: "0".into(),
            choices: String::new(),
            changed: false,
            trigger: String::new(),
            from: String::new(),
            to: String::new(),
            use_from: false,
            use_to: false,
            event_target: String::new(),
            once: false,
            condition_variable: String::new(),
            condition_value: String::new(),
            request: false,
            action_name: String::new(),
            action_value: String::new(),
            scene: String::new(),
            min: "0, 0, 0".into(),
            max: "1, 1, 1".into(),
        }
    }
}
impl Form {
    pub fn field(&mut self, k: Field) -> &mut String {
        match k {
            Field::Id => &mut self.id,
            Field::Default => &mut self.default_value,
            Field::Choices => &mut self.choices,
            Field::Trigger => &mut self.trigger,
            Field::From => &mut self.from,
            Field::To => &mut self.to,
            Field::EventTarget => &mut self.event_target,
            Field::ConditionVariable => &mut self.condition_variable,
            Field::ConditionValue => &mut self.condition_value,
            Field::ActionName => &mut self.action_name,
            Field::ActionValue => &mut self.action_value,
            Field::Scene => &mut self.scene,
            Field::Min => &mut self.min,
            Field::Max => &mut self.max,
        }
    }
    pub fn set_condition(&mut self, c: Option<&ConditionForm>) {
        self.condition_variable = c.map(|c| c.variable.clone()).unwrap_or_default();
        self.condition_value = c.map(|c| c.equals.clone()).unwrap_or_default();
    }
    pub fn set_action(&mut self, a: Option<&ActionForm>) {
        let (request, name, value) = match a {
            Some(ActionForm::Set { variable, value }) => (false, variable.as_str(), value.as_str()),
            Some(ActionForm::Request { request, target }) => {
                (true, request.as_str(), target.as_str())
            }
            None => (false, "", ""),
        };
        self.request = request;
        self.action_name = name.to_owned();
        self.action_value = value.to_owned();
    }
    pub fn load(model: &RulesForm, sel: &Selection) -> Self {
        let mut f = Self::default();
        let Some(i) = sel.item else { return f };
        match sel.tab {
            Tab::Variables => {
                if let Some(v) = model.variables.get(i) {
                    f.id = v.id.clone();
                    f.kind = VariableKind::of(v);
                    f.default_value = v.default.clone();
                    f.choices = v
                        .choices
                        .as_ref()
                        .map_or_else(String::new, |x| super::values::show_choices(x));
                }
            }
            Tab::Locations => {
                if let Some(v) = model.locations.get(i) {
                    f.id = v.id.clone();
                    f.scene = v.scene.clone();
                    f.min = v.min.join(", ");
                    f.max = v.max.join(", ");
                }
            }
            Tab::Rules => {
                if let Some(r) = model.rules.get(i) {
                    f.id = r.id.clone();
                    f.once = r.once;
                    match &r.when {
                        TriggerForm::Event { event, target } => {
                            f.trigger = event.clone();
                            f.event_target = target.clone().unwrap_or_default();
                        }
                        TriggerForm::Changed { variable, from, to } => {
                            f.changed = true;
                            f.trigger = variable.clone();
                            f.use_from = from.is_some();
                            f.use_to = to.is_some();
                            f.from = from.clone().unwrap_or_default();
                            f.to = to.clone().unwrap_or_default();
                        }
                    }
                    f.set_condition(r.conditions.get(sel.condition));
                    f.set_action(r.actions.get(sel.action));
                }
            }
        }
        f
    }
    pub fn condition(&self) -> ConditionForm {
        ConditionForm {
            variable: self.condition_variable.trim().into(),
            equals: self.condition_value.clone(),
        }
    }
    pub fn action(&self) -> ActionForm {
        if self.request {
            ActionForm::Request {
                request: self.action_name.trim().into(),
                target: self.action_value.clone(),
            }
        } else {
            ActionForm::Set {
                variable: self.action_name.trim().into(),
                value: self.action_value.clone(),
            }
        }
    }
    pub fn trigger(&self) -> TriggerForm {
        if self.changed {
            TriggerForm::Changed {
                variable: self.trigger.trim().into(),
                from: self.use_from.then(|| self.from.clone()),
                to: self.use_to.then(|| self.to.clone()),
            }
        } else {
            TriggerForm::Event {
                event: self.trigger.trim().into(),
                target: (!self.event_target.is_empty()).then(|| self.event_target.clone()),
            }
        }
    }
    pub fn variable(&self) -> Result<VariableForm, String> {
        if self.id.trim().is_empty() {
            return Err("请输入变量名称".into());
        }
        let choices = if self.kind == VariableKind::Enum {
            let choices = super::values::parse_choices(&self.choices)?.ok_or("请输入枚举选项")?;
            if !choices.contains(&self.default_value) {
                return Err("默认值必须是枚举选项之一".into());
            }
            Some(choices)
        } else {
            None
        };
        Ok(VariableForm {
            id: self.id.trim().into(),
            kind: match self.kind {
                VariableKind::Bool => ValueKind::Bool,
                VariableKind::Number => ValueKind::Number,
                VariableKind::Text | VariableKind::Enum => ValueKind::Text,
            },
            default: self.default_value.clone(),
            choices,
        })
    }
    pub fn location(&self) -> Result<LocationForm, String> {
        Ok(LocationForm {
            id: self.id.trim().into(),
            scene: self.scene.clone(),
            min: super::values::xyz(&self.min)?,
            max: super::values::xyz(&self.max)?,
        })
    }
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Selection {
    pub tab: Tab,
    pub item: Option<usize>,
    pub condition: usize,
    pub action: usize,
}
impl Selection {
    pub fn rule<'a>(&self, m: &'a RulesForm) -> Option<&'a RuleForm> {
        self.item
            .and_then(|i| m.rules.get(i))
            .filter(|_| self.tab == Tab::Rules)
    }
}
/// 按名称的索引与选项在同一后台任务中生成，绘制时只截取可见的几行。
pub(super) struct Sheet {
    pub model: RulesForm,
    pub events: Vec<String>,
    pub requests: Vec<String>,
    indices: [BTreeMap<String, usize>; 3],
    pub event_targets: BTreeMap<String, Vec<String>>,
    pub request_targets: BTreeMap<String, Vec<String>>,
}
// 表单展示玩法名称，选择后仍写工程规则协议中的事件或请求 ID。自定义名称照常可以输入。
const EVENTS: &[(&str, &str)] = &[
    ("location.entered", "进入地点"),
    ("location.exited", "离开地点"),
    ("quest.accepted", "接取任务"),
    ("quest.stage_changed", "任务阶段改变"),
    ("quest.completed", "完成任务"),
    ("enemy.defeated", "击败敌人"),
    ("item.acquired", "获得物品"),
    ("item.picked_up", "从场景拾取物品"),
    ("cinematic.finished", "剧情演出结束"),
];
const REQUESTS: &[(&str, &str)] = &[
    ("cinematic.play", "播放剧情演出"),
    ("quest.start", "开始任务"),
];

pub(super) fn choice_label<'a>(field: Field, form: &Form, id: &'a str) -> &'a str {
    match field {
        Field::Trigger if !form.changed => event_label(id),
        Field::ActionName if form.request => request_label(id),
        _ => id,
    }
}
pub(super) fn event_label(id: &str) -> &str {
    known_label(EVENTS, id)
}
pub(super) fn request_label(id: &str) -> &str {
    known_label(REQUESTS, id)
}
fn known_label<'a>(entries: &'static [(&str, &str)], id: &'a str) -> &'a str {
    entries
        .iter()
        .find(|(key, _)| *key == id)
        .map_or(id, |(_, label)| *label)
}

impl Sheet {
    pub fn new(model: RulesForm) -> Self {
        let mut events: BTreeSet<String> = EVENTS.iter().map(|(id, _)| (*id).to_owned()).collect();
        let mut requests: BTreeSet<String> =
            REQUESTS.iter().map(|(id, _)| (*id).to_owned()).collect();
        let mut event_targets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut request_targets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut rule_indices = BTreeMap::new();
        for (index, rule) in model.rules.iter().enumerate() {
            rule_indices.insert(rule.id.clone(), index);
            if let TriggerForm::Event { event, target } = &rule.when {
                events.insert(event.clone());
                if let Some(t) = target {
                    event_targets
                        .entry(event.clone())
                        .or_default()
                        .insert(t.clone());
                }
            }
            for action in &rule.actions {
                if let ActionForm::Request { request, target } = action {
                    requests.insert(request.clone());
                    request_targets
                        .entry(request.clone())
                        .or_default()
                        .insert(target.clone());
                }
            }
        }
        let lists = |m: BTreeMap<String, BTreeSet<String>>| {
            m.into_iter()
                .map(|(k, v)| (k, v.into_iter().collect()))
                .collect()
        };
        let indices = [
            model
                .variables
                .iter()
                .enumerate()
                .map(|(i, v)| (v.id.clone(), i))
                .collect(),
            rule_indices,
            model
                .locations
                .iter()
                .enumerate()
                .map(|(i, v)| (v.id.clone(), i))
                .collect(),
        ];
        Self {
            model,
            events: events.into_iter().collect(),
            requests: requests.into_iter().collect(),
            indices,
            event_targets: lists(event_targets),
            request_targets: lists(request_targets),
        }
    }
    pub fn position(&self, tab: Tab, id: &str) -> Option<usize> {
        self.indices[tab as usize].get(id).copied()
    }
    pub fn len(&self, tab: Tab) -> usize {
        match tab {
            Tab::Variables => self.model.variables.len(),
            Tab::Rules => self.model.rules.len(),
            Tab::Locations => self.model.locations.len(),
        }
    }
    pub fn id(&self, tab: Tab, i: usize) -> Option<&str> {
        match tab {
            Tab::Variables => self.model.variables.get(i).map(|v| v.id.as_str()),
            Tab::Rules => self.model.rules.get(i).map(|v| v.id.as_str()),
            Tab::Locations => self.model.locations.get(i).map(|v| v.id.as_str()),
        }
    }
}
