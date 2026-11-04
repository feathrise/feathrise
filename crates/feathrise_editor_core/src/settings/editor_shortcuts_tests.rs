//! 编辑器组合键与损坏设置回归：裸键、左右修饰键、冲突、解绑及独立往返。

use super::*;

#[test]
fn add_object_menu_defaults_to_shift_a_and_preserves_old_conflicts() {
    let mut input = ButtonInput::default();
    input.press(KeyCode::ShiftRight);
    input.press(KeyCode::KeyA);
    let defaults = EditorKeyBindings::default();
    assert!(defaults.pressed(EditorShortcutAction::AddObjectMenu, &input));
    input.press(KeyCode::ControlLeft);
    assert!(!defaults.pressed(EditorShortcutAction::AddObjectMenu, &input));
    let old = BTreeMap::from([("focus_selected".into(), Some("Shift+KeyA".into()))]);
    let mut bindings = EditorKeyBindings::from_saved(&old);
    assert_eq!(bindings.get(EditorShortcutAction::AddObjectMenu), None);
    bindings.assign(EditorShortcutAction::AddObjectMenu, Some(EditorKeyBinding::plain(KeyCode::KeyA))).unwrap();
    assert_eq!(EditorKeyBindings::from_saved(&bindings.to_saved()), bindings);
}

#[test]
fn rotation_and_scale_shortcuts_preserve_old_custom_bindings_and_round_trip() {
    let defaults = EditorKeyBindings::default();
    assert_eq!(defaults.get(EditorShortcutAction::RotateSelected), Some(EditorKeyBinding::plain(KeyCode::KeyE)));
    assert_eq!(defaults.get(EditorShortcutAction::ScaleSelected), Some(EditorKeyBinding::plain(KeyCode::KeyR)));
    let old = BTreeMap::from([("focus_selected".into(), Some("KeyE".into())), ("frame_map".into(), Some("KeyR".into()))]);
    let mut bindings = EditorKeyBindings::from_saved(&old);
    assert_eq!(bindings.get(EditorShortcutAction::RotateSelected), None);
    assert_eq!(bindings.get(EditorShortcutAction::ScaleSelected), None);
    bindings.assign(EditorShortcutAction::RotateSelected, Some(EditorKeyBinding { control: true, ..EditorKeyBinding::plain(KeyCode::KeyT) })).unwrap();
    bindings.assign(EditorShortcutAction::ScaleSelected, None).unwrap();
    assert_eq!(EditorKeyBindings::from_saved(&bindings.to_saved()), bindings);
}

#[test]
fn new_move_shortcut_preserves_old_custom_w_and_supports_unbinding() {
    let defaults = EditorKeyBindings::from_saved(&BTreeMap::new());
    assert_eq!(defaults.get(EditorShortcutAction::MoveSelected), Some(EditorKeyBinding::plain(KeyCode::KeyW)));
    let data = BTreeMap::from([("focus_selected".into(), Some("KeyW".into()))]);
    let bindings = EditorKeyBindings::from_saved(&data);
    assert_eq!(bindings.get(EditorShortcutAction::FocusSelected), Some(EditorKeyBinding::plain(KeyCode::KeyW)));
    assert_eq!(bindings.get(EditorShortcutAction::MoveSelected), None); // 新默认不得抢走用户以前绑定的 W。
    let mut data = data;
    data.insert("move_selected".into(), Some("Ctrl+Shift+KeyM".into()));
    let bindings = EditorKeyBindings::from_saved(&data);
    assert_eq!(bindings.get(EditorShortcutAction::MoveSelected).unwrap().display(), "Ctrl+Shift+M");
    assert_eq!(EditorKeyBindings::from_saved(&bindings.to_saved()), bindings);
    data.insert("move_selected".into(), None);
    assert_eq!(EditorKeyBindings::from_saved(&data).get(EditorShortcutAction::MoveSelected), None);
}

#[test]
fn matching_requires_exact_modifiers_and_accepts_both_sides() {
    let bindings = EditorKeyBindings::default();
    let mut keys = ButtonInput::default();
    keys.press(KeyCode::KeyF);
    assert!(bindings.pressed(EditorShortcutAction::FocusSelected, &keys));
    keys.press(KeyCode::ControlRight);
    assert!(!bindings.pressed(EditorShortcutAction::FocusSelected, &keys));
    keys.reset_all();
    keys.press(KeyCode::ControlRight);
    keys.press(KeyCode::KeyS);
    assert!(bindings.pressed(EditorShortcutAction::Save, &keys));
    keys.press(KeyCode::ShiftLeft);
    assert!(!bindings.pressed(EditorShortcutAction::Save, &keys));
    keys.reset(KeyCode::ShiftLeft);
    keys.press(KeyCode::SuperLeft);
    assert!(!bindings.pressed(EditorShortcutAction::Save, &keys));
}

#[test]
fn assignment_rejects_conflicts_and_reserved_keys_without_partial_changes() {
    let mut bindings = EditorKeyBindings::default();
    let before = bindings.clone();
    assert_eq!(
        bindings.assign(
            EditorShortcutAction::FocusSelected,
            Some(EditorKeyBinding::plain(KeyCode::Home))
        ),
        Err(EditorShortcutConflict::InUse(
            EditorShortcutAction::FrameMap
        ))
    );
    for key in [
        KeyCode::F12,
        KeyCode::F4,
        KeyCode::Escape,
        KeyCode::Delete,
        KeyCode::Enter,
        KeyCode::ShiftLeft,
    ] {
        assert_eq!(
            bindings.assign(
                EditorShortcutAction::Save,
                Some(EditorKeyBinding::plain(key))
            ),
            Err(EditorShortcutConflict::ReservedKey)
        );
    }
    assert_eq!(bindings, before);
    bindings
        .assign(EditorShortcutAction::FrameMap, None)
        .unwrap();
    bindings
        .assign(
            EditorShortcutAction::FocusSelected,
            Some(EditorKeyBinding::plain(KeyCode::Home)),
        )
        .unwrap();
    assert_eq!(
        EditorKeyBindings::from_saved(&bindings.to_saved()),
        bindings
    );
}

#[test]
fn all_supported_keys_and_modifier_combinations_round_trip() {
    for &(key, _, _) in SUPPORTED_KEYS {
        for mask in 0..8 {
            let binding = EditorKeyBinding {
                key,
                control: mask & 1 != 0,
                shift: mask & 2 != 0,
                alt: mask & 4 != 0,
            };
            assert_eq!(
                EditorKeyBinding::from_saved(&binding.formatted(true)),
                Some(binding)
            );
            assert!(!binding.display().is_empty());
        }
    }
}

#[test]
fn partial_or_bad_saved_bindings_preserve_valid_overrides_and_never_duplicate() {
    let data = BTreeMap::from([
        ("focus_selected".into(), Some("Ctrl+Shift+KeyG".into())),
        ("frame_map".into(), None),
        ("save_map".into(), Some("F12".into())),
        ("undo".into(), Some("Ctrl+Ctrl+KeyZ".into())),
        ("redo".into(), Some("Ctrl+KeyZ".into())),
        ("future_action".into(), Some("KeyH".into())),
    ]);
    let bindings = EditorKeyBindings::from_saved(&data);
    assert_eq!(
        bindings
            .get(EditorShortcutAction::FocusSelected)
            .unwrap()
            .display(),
        "Ctrl+Shift+G"
    );
    assert_eq!(bindings.get(EditorShortcutAction::FrameMap), None);
    assert_eq!(
        bindings.get(EditorShortcutAction::Save),
        EditorKeyBindings::default().get(EditorShortcutAction::Save)
    );
    assert_eq!(
        bindings.get(EditorShortcutAction::Undo),
        EditorKeyBindings::default().get(EditorShortcutAction::Undo)
    );
    assert_eq!(
        bindings.get(EditorShortcutAction::Redo),
        EditorKeyBindings::default().get(EditorShortcutAction::Redo)
    );
    assert_eq!(
        EditorKeyBindings::from_saved(&BTreeMap::new()),
        EditorKeyBindings::default()
    );
    for (index, action) in EditorShortcutAction::ALL.into_iter().enumerate() {
        if let Some(binding) = bindings.get(action) {
            assert!(
                !EditorShortcutAction::ALL[..index]
                    .iter()
                    .any(|&other| bindings.get(other) == Some(binding))
            );
        }
    }
}
