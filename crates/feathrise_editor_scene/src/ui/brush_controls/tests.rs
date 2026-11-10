use super::*; // 验证字段边界、显示换算与独立配置，公共拖动数学由editor_ui测试覆盖。

#[test]
fn brush_specs_share_exact_input_and_drag_ranges() {
    for (kind, minimum, maximum) in [
        (SceneEditorBrushSliderKind::Spacing, 0.01, f32::MAX),
        (
            SceneEditorBrushSliderKind::Radius,
            BRUSH_RADIUS_MIN_METERS,
            BRUSH_RADIUS_MAX_METERS,
        ),
        (
            SceneEditorBrushSliderKind::Strength,
            BRUSH_STRENGTH_MIN * 100.0,
            BRUSH_STRENGTH_MAX * 100.0,
        ),
        (
            SceneEditorBrushSliderKind::TerrainSpeed,
            TERRAIN_SPEED_MIN_METERS_PER_SECOND,
            TERRAIN_SPEED_MAX_METERS_PER_SECOND,
        ),
    ] {
        let spec = numeric_spec(kind);
        let scrub = scrub_config(kind, 1.0);
        assert_eq!(spec.min.and_then(NumericValue::as_f32), Some(minimum));
        assert_eq!(spec.max.and_then(NumericValue::as_f32), Some(maximum));
        assert_eq!(scrub.minimum, minimum);
        assert_eq!(scrub.maximum, maximum);
    }
}

#[test]
fn displayed_strength_round_trips_through_percent_boundary() {
    let mut editor = SceneEditorState::default();
    apply_numeric_brush_value(SceneEditorBrushSliderKind::Strength, 63.0, &mut editor)
        .expect("有效百分比应更新笔刷");
    assert_eq!(editor.brush_strength, 0.63);
    assert_eq!(
        displayed_value(SceneEditorBrushSliderKind::Strength, &editor),
        63.0
    );
}

#[test]
fn invalid_saved_sensitivity_falls_back_without_polluting_config() {
    let mut settings = EditorSettings::default();
    settings.scene_editor_scrub_sensitivities.brush_radius = f32::NAN;
    assert_eq!(
        scrub_sensitivity(SceneEditorBrushSliderKind::Radius, &settings),
        1.0
    );
    settings.scene_editor_scrub_sensitivities.brush_radius = 10_000.0;
    assert_eq!(
        scrub_sensitivity(SceneEditorBrushSliderKind::Radius, &settings),
        SCRUB_SENSITIVITY_MAX
    );
}

#[test]
fn display_origin_restores_raw_precision_for_all_brush_fields() {
    let cases = [
        (SceneEditorBrushSliderKind::Spacing, 3.125_f32, 3.13_f32),
        (SceneEditorBrushSliderKind::Radius, 46.875_f32, 46.9_f32), // 快捷键可产生高于一位显示精度的半径。
        (SceneEditorBrushSliderKind::Strength, 0.356_789_f32, 36.0_f32), // 归一化强度高于整数百分比显示精度。
        (SceneEditorBrushSliderKind::TerrainSpeed, 24.375_f32, 24.4_f32), // 雕刻速度raw高于一位显示精度。
    ];
    for (kind, raw_start, display_origin) in cases {
        let mut editor = SceneEditorState::default(); // 每个字段独立模拟一次左拖。
        match kind {
            SceneEditorBrushSliderKind::Spacing => editor.brush_spacing = raw_start,
            SceneEditorBrushSliderKind::Radius => editor.brush_radius = raw_start,
            SceneEditorBrushSliderKind::Strength => editor.brush_strength = raw_start,
            SceneEditorBrushSliderKind::TerrainSpeed => editor.terrain_sculpt_speed = raw_start,
        } // 第一条Changed前业务raw仍保留完整尾数。
        let mut transaction = BrushNumericScrub {
            entity: Entity::PLACEHOLDER, // 纯值测试无需真实UI根。
            kind,                        // 稳定字段身份。
            raw_start,                   // 首个Changed冻结的业务raw起点。
            latest_raw: raw_start,       // 应用显示档前字段仍归起点所有。
        };
        apply_numeric_brush_value(kind, display_origin, &mut editor)
            .expect("显示起点吸附档必须是合法笔刷值"); // 模拟拖离后回到共享 UI 控件按下时显示值。
        transaction.latest_raw = raw_brush_value(kind, &editor); // 记录本事务最后实际写入值。
        assert!(restore_brush_scrub_if_owned(&mut editor, transaction)); // Committed.changed=false走owned-field恢复。
        assert_eq!(
            raw_brush_value(kind, &editor).to_bits(),
            raw_start.to_bits()
        ); // 位级证明隐藏raw尾数没有量化。
    }
}

#[test]
fn raw_restore_never_overwrites_a_later_brush_value() {
    let mut editor = SceneEditorState {
        brush_radius: 80.0, // 后来快捷键在release前明确接管同一字段。
        ..Default::default()
    };
    let transaction = BrushNumericScrub {
        entity: Entity::PLACEHOLDER,            // 纯owned-field判定无需UI根。
        kind: SceneEditorBrushSliderKind::Radius, // 旧事务属于半径。
        raw_start: 46.875,                      // 旧拖动进入前raw起点。
        latest_raw: 46.9,                       // 旧拖动最后拥有的显示吸附档。
    };
    assert!(!restore_brush_scrub_if_owned(&mut editor, transaction)); // 当前80不再等于旧latest，恢复必须拒绝。
    assert_eq!(editor.brush_radius, 80.0); // 后来字段值保持权威。
}

#[test]
fn domain_brush_spacing_uses_common_numeric_bridge() {
    let mut editor = SceneEditorState::default();
    let radius = editor.brush_radius;
    apply_numeric_brush_value(SceneEditorBrushSliderKind::Spacing, 2.35, &mut editor).unwrap();
    assert_eq!(editor.brush_spacing, 2.35);
    assert_eq!(editor.brush_radius, radius);
    assert_eq!(
        displayed_value(SceneEditorBrushSliderKind::Spacing, &editor),
        2.35
    );
    assert!(
        apply_numeric_brush_value(SceneEditorBrushSliderKind::Spacing, f32::NAN, &mut editor)
            .is_none()
    );
    let mut settings = EditorSettings::default();
    settings
        .scene_editor_scrub_sensitivities
        .set(SceneEditorScrubField::BrushSpacing, 2.0);
    assert_eq!(
        scrub_sensitivity(SceneEditorBrushSliderKind::Spacing, &settings),
        2.0
    );
    assert_eq!(
        scrub_sensitivity(SceneEditorBrushSliderKind::Radius, &settings),
        1.0
    );
}

#[test]
fn domain_and_surface_brush_fields_remain_distinct_after_tool_switch() {
    let mut editor = SceneEditorState::default(); // 模拟同一工具栏从扩展散布刷子切换到地表工具。
    editor.tool = EditorTool::Domain("fixture.domain"); // 扩展散布专用工具。
    editor.domain_brush_active = true; // 已选择扩展散布圆刷。
    assert!(brush_field_applies(
        SceneEditorBrushSliderKind::Radius,
        &editor
    )); // 半径必须可编辑。
    assert!(brush_field_applies(
        SceneEditorBrushSliderKind::Spacing,
        &editor
    )); // 最小间距必须可编辑。
    assert!(!brush_field_applies(
        SceneEditorBrushSliderKind::Strength,
        &editor
    )); // 扩展散布散布不消费强度。
    editor.domain_brush_active = false; // 结束扩展散布笔刷上下文。
    editor.tool = EditorTool::Surface; // 切换地表绘制。
    assert!(brush_field_applies(
        SceneEditorBrushSliderKind::Radius,
        &editor
    )); // 地表仍显示圆刷半径。
    assert!(brush_field_applies(
        SceneEditorBrushSliderKind::Strength,
        &editor
    )); // 地表混合使用强度。
    assert!(!brush_field_applies(
        SceneEditorBrushSliderKind::Spacing,
        &editor
    )); // 扩展散布间距不得泄漏到地表。
    assert!(!brush_field_applies(
        SceneEditorBrushSliderKind::TerrainSpeed,
        &editor
    )); // 地表绘制不升降高度。
}
