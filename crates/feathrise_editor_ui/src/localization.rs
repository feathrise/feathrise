//! 编辑器自有文案的语言选择与 Bevy 文字绑定。作者内容通过普通 Text 显示。

use bevy::prelude::*;
use feathrise_editor_core::{EditorLocalePreference, EditorSettings};
use std::{borrow::Cow, collections::HashMap, sync::OnceLock};

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorLocale {
    #[default]
    Chinese,
    English,
}

impl EditorLocale {
    pub fn resolve(preference: EditorLocalePreference, system: Self) -> Self {
        match preference {
            EditorLocalePreference::System => system,
            EditorLocalePreference::Chinese => Self::Chinese,
            EditorLocalePreference::English => Self::English,
        }
    }

    pub fn from_system_name(name: &str) -> Self {
        if name.split(['-', '_', '.', '@']).next().is_some_and(|language| language.eq_ignore_ascii_case("zh")) {
            Self::Chinese
        } else {
            Self::English
        }
    }

    pub fn text(self, key: &str) -> &str {
        match catalog().keys.get(key) {
            Some(entry) if self == Self::English && !entry.en.is_empty() => entry.en.as_ref(),
            Some(entry) => {
                if self == Self::English {
                    warn_once(entry.key);
                }
                entry.zh.as_ref()
            }
            None => key,
        }
    }

    /// 迁移现有共用控件的固定编辑器标签。调用方负责区分产品文案和用户数据。
    pub fn editor_label(self, source: &str) -> &str {
        catalog().labels.get(source).map_or(source, |key| self.text(key))
    }

    /// 旧状态通道已有完整中文回执。只匹配本程序定义的模板，捕获的参数直接交给英文模板。
    pub fn editor_notice(self, source: &str) -> Cow<'_, str> {
        if self == Self::Chinese || source.is_empty() {
            return Cow::Borrowed(source);
        }
        let label = self.editor_label(source);
        if label != source {
            return Cow::Borrowed(label);
        }
        if let Some(patterns) = source.chars().next().and_then(|initial| catalog().patterns.get(&initial)) {
            for key in patterns {
                let pattern = &catalog().keys[key].zh;
                if let Some(args) = notice_args(pattern, source) {
                    return Cow::Owned(self.format(key, &args));
                }
            }
        }
        Cow::Borrowed(source)
    }

    /// 带动作或按键名的旧回执由所属面板明确选择模板和可翻译参数。
    pub fn editor_notice_with(
        self,
        key: &str,
        source: &str,
        localize_arg: impl Fn(&str, &str) -> String,
    ) -> Option<String> {
        if self == Self::Chinese {
            return None;
        }
        let template = &catalog().keys.get(key)?.zh;
        let mut args = notice_args(template, source)?;
        for (name, value) in &mut args {
            *value = localize_arg(name, value);
        }
        Some(self.format(key, &args))
    }

    /// 插值按模板顺序单次执行，路径和原始诊断中的花括号只是值。
    pub fn format(self, key: &str, args: &[(&str, String)]) -> String {
        let mut pattern = self.text(key);
        let mut result = String::with_capacity(pattern.len());
        while let Some((prefix, argument)) = pattern.split_once('{') {
            let Some((name, rest)) = argument.split_once('}') else { break };
            result.push_str(prefix);
            if let Some((_, value)) = args.iter().find(|(arg, _)| *arg == name) {
                result.push_str(value);
            } else {
                result.push('{');
                result.push_str(name);
                result.push('}');
            }
            pattern = rest;
        }
        result.push_str(pattern);
        result
    }
}

/// 由独立宿主在装配期识别，绑定系统只读取固定大小的语言值。
#[derive(Resource, Clone, Copy, Default)]
pub struct EditorSystemLocale(pub EditorLocale);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Message {
    Key(&'static str, Vec<(&'static str, String)>),
    Literal(String),
}

/// 语言改变时更新原节点；布局和按钮身份仍由既有场景管理。
#[derive(Component, Clone, Debug, PartialEq, Eq)]
#[require(Text)]
pub struct LocalizedText(Message);

impl Default for LocalizedText {
    fn default() -> Self {
        Self(Message::Literal(String::new()))
    }
}

impl LocalizedText {
    pub fn new(key: &'static str) -> Self {
        Self(Message::Key(key, Vec::new()))
    }

    pub fn with_args(key: &'static str, args: Vec<(&'static str, String)>) -> Self {
        Self(Message::Key(key, args))
    }

    pub fn label(label: &str) -> Self {
        catalog().labels.get(label).map_or_else(
            || Self(Message::Literal(label.into())),
            |key| Self::new(key),
        )
    }

    pub fn render(&self, locale: EditorLocale) -> Cow<'_, str> {
        match &self.0 {
            Message::Key(key, args) if args.is_empty() => Cow::Borrowed(locale.text(key)),
            Message::Key(key, args) => Cow::Owned(locale.format(key, args)),
            Message::Literal(label) => Cow::Borrowed(label),
        }
    }
}

/// 仍在创建时提供原文，便于不安装 UI 插件的场景测试检查初始树。
pub fn editor_label(source: &str) -> (Text, LocalizedText) {
    (Text::new(source), LocalizedText::label(source))
}

/// BSN 保留原 Text 节点及其布局组件，只在同一实体上声明编辑器文案来源。
pub fn label_scene(source: &str) -> impl Scene {
    template_value(LocalizedText::label(source))
}

pub(super) fn install(app: &mut App) {
    // 词表只解析一次，且在装配期构造索引，首帧只处理真实可见文案。
    let _ = catalog();
    app.init_resource::<EditorLocale>()
        .init_resource::<EditorSystemLocale>()
        .add_systems(PreUpdate, sync_locale)
        .add_systems(
            PostUpdate,
            (sync_locale, refresh)
                .chain()
                .before(bevy::ui::UiSystems::Prepare),
        );
}

fn sync_locale(
    settings: Option<Res<EditorSettings>>,
    system: Res<EditorSystemLocale>,
    mut active: ResMut<EditorLocale>,
) {
    let Some(settings) = settings else { return };
    if !settings.is_changed() && !system.is_changed() {
        return;
    }
    active.set_if_neq(EditorLocale::resolve(settings.ui_locale, system.0));
}

fn refresh(
    locale: Res<EditorLocale>,
    mut previous: Local<EditorLocale>,
    mut texts: Query<(Ref<LocalizedText>, &mut Text)>,
    changed: Query<Entity, Changed<LocalizedText>>,
) {
    let apply = |message: Ref<LocalizedText>, mut text: Mut<Text>| {
        if matches!(message.0, Message::Literal(_)) {
            return;
        }
        // 有些旧场景节点使用静态初始文字，实际值随后由业务状态系统接管。
        // 切换只刷新本绑定此前拥有的文字，不用初始标签覆盖当前回执。
        if !message.is_changed() && text.as_str() != message.render(*previous).as_ref() {
            return;
        }
        let rendered = message.render(*locale);
        if text.0 != rendered {
            text.0 = rendered.into_owned();
        }
    };
    if locale.is_changed() {
        for (message, text) in &mut texts {
            apply(message, text);
        }
    } else {
        for entity in &changed {
            if let Ok((message, text)) = texts.get_mut(entity) {
                apply(message, text);
            }
        }
    }
    *previous = *locale;
}

struct Entry {
    key: &'static str,
    zh: Cow<'static, str>,
    en: Cow<'static, str>,
}

#[derive(Default)]
struct Catalog {
    keys: HashMap<&'static str, Entry>,
    labels: HashMap<Cow<'static, str>, &'static str>,
    patterns: HashMap<char, Vec<&'static str>>,
}

const CATALOG: &str = include_str!("localization/editor.tsv");

fn entries() -> impl Iterator<Item = Entry> {
    CATALOG.lines().filter_map(|line| {
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut columns = line.split('\t');
        let key = columns.next()?;
        let zh = catalog_text(columns.next()?);
        let en = catalog_text(columns.next()?);
        Some(Entry { key, zh, en })
    })
}

fn catalog_text(value: &'static str) -> Cow<'static, str> {
    if value.contains("\\n") {
        Cow::Owned(value.replace("\\n", "\n"))
    } else {
        Cow::Borrowed(value)
    }
}

fn catalog() -> &'static Catalog {
    static CATALOG_INDEX: OnceLock<Catalog> = OnceLock::new();
    CATALOG_INDEX.get_or_init(|| {
        let mut index = Catalog::default();
        for entry in entries() {
            if let Some((prefix, _)) = entry.zh.split_once('{') {
                if let Some(initial) = prefix.chars().next().filter(|initial| !initial.is_control()) {
                    index.patterns.entry(initial).or_default().push(entry.key);
                }
            } else {
                index.labels.entry(entry.zh.clone()).or_insert(entry.key);
            }
            index.keys.insert(entry.key, entry);
        }
        for patterns in index.patterns.values_mut() {
            // 优先采用带具体语义的较长固定前缀。
            patterns.sort_by_key(|key| std::cmp::Reverse(index.keys[key].zh.find('{').unwrap_or(0)));
        }
        index
    })
}

fn notice_args<'a>(pattern: &'a str, source: &str) -> Option<Vec<(&'a str, String)>> {
    let (prefix, mut pattern) = pattern.split_once('{')?;
    let mut source = source.strip_prefix(prefix)?;
    let mut args = Vec::new();
    loop {
        let (name, next) = pattern.split_once('}')?;
        if let Some((separator, rest)) = next.split_once('{') {
            if separator.is_empty() {
                return None;
            }
            let (value, remaining) = source.split_once(separator)?;
            args.push((name, value.to_owned()));
            source = remaining;
            pattern = rest;
        } else {
            args.push((name, source.strip_suffix(next)?.to_owned()));
            return Some(args);
        }
    }
}

fn warn_once(key: &'static str) {
    static WARNED: OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> = OnceLock::new();
    if let Ok(mut warned) = WARNED.get_or_init(Default::default).lock()
        && warned.insert(key)
    {
        warn!("Missing English editor translation for {key}");
    }
}

#[cfg(test)]
mod tests;
