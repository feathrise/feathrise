//! 校验随 EXE 编译的词表和现有文字节点的实时切换。

use super::*;
use std::collections::BTreeSet;

fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split('{').skip(1).filter_map(|part| part.split_once('}').map(|(name, _)| name)).collect()
}

#[test]
fn editor_catalog_has_both_locales_and_matching_parameters() {
    let mut keys = BTreeSet::new();
    for line in CATALOG.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
        assert_eq!(line.split('\t').count(), 3, "{line}");
    }
    for Entry { key, zh, en } in entries() {
        assert!(keys.insert(key), "duplicate editor message: {key}");
        assert!(!key.is_empty() && !zh.trim().is_empty() && !en.trim().is_empty(), "{key}");
        assert_eq!(placeholders(&zh), placeholders(&en), "{key}");
    }
    assert_eq!(
        EditorLocale::English.format("project.close.unsaved", &[("count", "{path}".into())]),
        "Save {path} unsaved changes before closing?"
    );
}

#[test]
fn language_refreshes_current_and_late_labels_with_user_content_untouched() {
    let mut app = App::new();
    install(&mut app);
    app.init_resource::<EditorSettings>();
    let label = app.world_mut().spawn(LocalizedText::new("project.open")).id();
    let user_value = app.world_mut().spawn(Text::new("打开工程")).id();
    app.update();
    assert_eq!(app.world().get::<Text>(label).unwrap().0, "打开工程");
    app.world_mut().resource_mut::<EditorSettings>().ui_locale = EditorLocalePreference::English;
    app.update();
    assert_eq!(app.world().get::<Text>(label).unwrap().0, "Open Project");
    assert_eq!(app.world().get::<Text>(user_value).unwrap().0, "打开工程");
    let late = app.world_mut().spawn(editor_label("设置")).id();
    app.update();
    assert_eq!(app.world().get::<Text>(late).unwrap().0, "Settings");
    app.world_mut().resource_mut::<EditorSettings>().ui_locale = EditorLocalePreference::Chinese;
    app.update();
    assert_eq!(app.world().get::<Text>(late).unwrap().0, "设置");
}
