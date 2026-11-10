//! 作者设置持久化、旧值回退和界面倍率回归，不触碰原游戏配置文件。
use super::*;

#[test]
fn editor_ui_scale_has_a_desktop_default_and_safe_bounds() {
    assert_eq!(EditorSettings::default().editor_ui_scale, 1.0);
    assert!(!EditorSettings::default().editor_ui_scale_auto);
    assert_eq!(EditorSettings::default().editor_ui_scale_auto_offset, 0.0);
    for (value, expected) in [
        (f32::NAN, 1.0),
        (f32::INFINITY, 1.0),
        (-1.0, 0.6),
        (5.0, 2.0),
        (1.25, 1.25),
    ] {
        assert_eq!(sanitize_editor_ui_scale(value), expected);
    }
}

#[test]
fn editor_auto_scale_fits_logical_window_without_changing_manual_preference() {
    for (width, height, expected) in [
        (1920.0, 1080.0, 1.0),
        (1440.0, 810.0, 0.75),
        (2560.0, 1440.0, 1.33),
        (800.0, 600.0, 0.6),
        (320.0, 180.0, 0.6),
        (3840.0, 2160.0, 2.0),
        (7680.0, 4320.0, 2.0),
    ] {
        assert_eq!(
            effective_editor_ui_scale(1.25, true, 0.0, width, height),
            expected
        );
    }
    assert_eq!(
        effective_editor_ui_scale(1.25, false, 0.05, 800.0, 600.0),
        1.25
    );
    assert_eq!(
        effective_editor_ui_scale(1.25, true, 0.0, 0.0, 1080.0),
        1.25
    );
    assert_eq!(
        effective_editor_ui_scale(1.25, true, 0.05, 1440.0, 810.0),
        0.8
    );
    assert_eq!(
        effective_editor_ui_scale(1.25, true, 0.05, 1920.0, 1080.0),
        1.05
    );
    assert_eq!(
        effective_editor_ui_scale(1.25, true, -0.05, 1920.0, 1080.0),
        0.95
    );
    for (offset, expected) in [(f32::NAN, 1.0), (5.0, 2.0), (-5.0, 0.6)] {
        assert_eq!(
            effective_editor_ui_scale(1.25, true, offset, 1920.0, 1080.0),
            expected
        );
    }
}

#[test] // 新F12字段在没有settings.json时必须从互不耦合的一倍手感开始。
fn scene_editor_scrub_sensitivities_default_independently_to_one() {
    let sensitivities = EditorSettings::default().scene_editor_scrub_sensitivities; // 使用正式新游戏默认资源。
    for field in SceneEditorScrubField::ALL {
        // 通过稳定字段全集覆盖当前全部三十个持久灵敏度槽位。
        assert_eq!(sensitivities.get(field), 1.0); // 任何字段都不得偷用其他数值框的旧灵敏度。
    } // 结束全字段默认值验证。
}

#[test] // 强类型访问器必须使三十个逻辑字段互不串槽。
fn scene_editor_scrub_field_accessors_keep_slots_independent() {
    // 用唯一数值检测误写到相邻配置槽位。
    let mut sensitivities = SceneEditorScrubSensitivities::default(); // 从统一一倍默认值开始。
    for (index, field) in SceneEditorScrubField::ALL.into_iter().enumerate() {
        // 给每个字段写入不同的测试倍率。
        sensitivities.set(field, index as f32 + 2.0); // 避开默认一倍，防止未写入时假阳性。
    } // 完成全部槽位写入后再统一核对。
    for (index, field) in SceneEditorScrubField::ALL.into_iter().enumerate() {
        // 二次遍历可以暴露多个键指向同一槽位的错误。
        assert_eq!(sensitivities.get(field), index as f32 + 2.0); // 每个键必须读回自己的唯一倍率。
    } // 结束强类型访问器独立性验证。
}

#[test]
fn author_preferences_round_trip_without_any_game_fields() {
    let mut settings = EditorSettings::default();
    settings.editor_ui_scale = 1.25;
    settings.editor_ui_scale_auto = true;
    settings.editor_ui_scale_auto_offset = 0.05;
    settings.editor_color_theme = EditorColorTheme::WarmCopper;
    settings.editor_layout.content_width = 340.0;
    settings.editor_snapping.mode = EditorSnapMode::Relative;
    settings.asset_browser_view_mode = AssetBrowserViewMode::Thumbnails;
    settings.render.mesh_lod.max_error_pixels = 2.5;
    settings.environment.world_time_paused = true;
    for (i, field) in SceneEditorScrubField::ALL.into_iter().enumerate() {
        settings
            .scene_editor_scrub_sensitivities
            .set(field, 1.0 + i as f32 * 0.25);
    }
    settings
        .editor_keybindings
        .assign(EditorShortcutAction::FrameScene, None)
        .unwrap();
    let json = serde_json::to_string(&EditorSettingsData::from(&settings)).unwrap();
    for forbidden in [
        "language",
        "control_mode",
        "mouse_sensitivity",
        "player_skill_cooldown",
        "auto_pickup",
        "minimap",
    ] {
        assert!(
            !json.contains(forbidden),
            "作者配置不得含游戏字段 {forbidden}"
        );
    }
    let restored: EditorSettingsData = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.into_runtime(), settings);
}
#[test]
fn missing_and_bad_author_values_recover_independently() {
    let data: EditorSettingsData = serde_json::from_str("{}").unwrap();
    assert_eq!(data.into_runtime(), EditorSettings::default());
    let data: EditorSettingsData = serde_json::from_str(r#"{"ui_scale":0.1,"ui_scale_auto":true,"ui_scale_auto_offset":8.0,"color_theme":"future","asset_browser_view_mode":"future","layout":{"content_width":900,"inspector_width":-2},"snapping":{"translation_meters":0,"rotation_degrees":30}}"#).unwrap();
    let restored = data.into_runtime();
    assert_eq!(restored.editor_ui_scale, MIN_EDITOR_UI_SCALE);
    assert_eq!(restored.editor_color_theme, EditorColorTheme::NeutralGray);
    assert_eq!(restored.asset_browser_view_mode, AssetBrowserViewMode::List);
    assert_eq!(
        restored.editor_layout.content_width,
        EditorLayoutPreferences::CONTENT_MAX_WIDTH
    );
    assert_eq!(
        restored.editor_layout.inspector_width,
        EditorLayoutPreferences::INSPECTOR_MIN_WIDTH
    );
    assert_eq!(restored.editor_snapping.translation_meters, 1.0);
    assert_eq!(restored.editor_snapping.rotation_degrees, 30.0);
}
#[test]
fn legacy_environment_scrub_scenes_to_both_explicit_channels_only_when_missing() {
    let data: EditorSettingsData = serde_json::from_str(
        r#"{"scrub":{"environment_light_intensity":3.0,"environment_diffuse_intensity":2.0}}"#,
    )
    .unwrap();
    let restored = data.into_runtime();
    assert_eq!(
        restored
            .scene_editor_scrub_sensitivities
            .environment_diffuse_intensity,
        2.0
    );
    assert_eq!(
        restored
            .scene_editor_scrub_sensitivities
            .environment_specular_intensity,
        3.0
    );
    assert_eq!(restored.scene_editor_scrub_sensitivities.brush_radius, 1.0);
}
