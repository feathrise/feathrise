//! 积水真实UI系统接线回归，覆盖控件显隐、公共数值消息和按钮反馈。
use feathrise_editor_ui::UiInteraction;
use super::*; // 直接运行生产笔刷控件系统。
use crate::authoring::EditorAuthoringSession; // 工具样式读取当前作者草稿。
use crate::components::MapEditorActionButton; // 与BSN按钮相同的动作标记。
use crate::types::EditorAction; // 稳定积水工具按钮身份。
#[test] // 不能仅证明纯画笔算法，而遗漏用户实际编辑入口。
fn puddles_ui_exposes_numeric_controls_and_updates_real_brush_values() {
    let mut app = App::new(); // 无窗口ECS足以验证UI节点和消息链。
    app.insert_resource(MapEditorState {
        open: true,
        tool: EditorTool::Puddles,
        ..Default::default()
    }) // 用户已选择独立积水工具。
    .init_resource::<EditorSettings>() // 公共笔刷灵敏度来源。
    .init_resource::<ActiveNumericInput>() // 当前没有精确输入焦点。
    .init_resource::<ActiveNumericInputScrub>() // 当前没有相对拖动捕获。
    .init_resource::<BrushNumericInputState>() // 笔刷数值提交状态。
    .init_resource::<EditorAuthoringSession>() // 样式系统不需要真实地图像素。
    .add_message::<NumericInputChanged>() // 公共左拖实时消息。
    .add_message::<NumericInputCommitted>() // 公共精确输入提交。
    .add_message::<NumericInputScrubSensitivityChanged>() // 灵敏度实时消息。
    .add_message::<NumericInputScrubSensitivityCommitted>() // 灵敏度提交消息。
    .add_message::<NumericInputSetValue>() // 外部快捷键回写公共数值模型。
    .add_systems(
        Update,
        (
            handle_brush_numeric_inputs,
            sync_brush_sliders,
            crate::ui::actions::sync_editor_action_button_styles,
        )
            .chain(),
    ); // 与生产相同的有序业务桥。
    let mut controls = Vec::new(); // 固定四个现有控件，不依赖地图规模。
    for kind in [
        MapEditorBrushSliderKind::Radius,
        MapEditorBrushSliderKind::Strength,
        MapEditorBrushSliderKind::TerrainSpeed,
        MapEditorBrushSliderKind::Spacing,
    ] {
        let slot = app
            .world_mut()
            .spawn((BrushNumericInputSlot(kind), Node::default()))
            .id(); // 生产BSN相同的显隐容器。
        let field = app
            .world_mut()
            .spawn((
                MapEditorBrushSlider(kind),
                NumericInputValue(NumericValue::F32(0.0)),
                NumericInputConfig(numeric_spec(kind)),
                scrub_config(kind, 1.0),
                NumericInputDisabled,
            ))
            .id(); // 模拟已挂载的公共输入根。
        controls.push((kind, slot, field)); // 保留测试所需实体身份。
    }
    let button = app
        .world_mut()
        .spawn((
            MapEditorActionButton(EditorAction::ToolPuddles),
            UiInteraction::None,
            BackgroundColor::default(),
        ))
        .id(); // 鼠标移开后仍应保持选中反馈。
    app.update(); // 运行真实UI系统而非直接调用纯可见性函数。
    for (kind, slot, field) in &controls {
        let visible = matches!(
            kind,
            MapEditorBrushSliderKind::Radius | MapEditorBrushSliderKind::Strength
        ); // 积水只消费两个参数。
        assert_eq!(
            app.world().get::<Node>(*slot).unwrap().display,
            if visible {
                Display::Flex
            } else {
                Display::None
            }
        ); // 参数槽位确实显示或隐藏。
        assert_eq!(
            app.world().get::<NumericInputDisabled>(*field).is_none(),
            visible
        ); // 显示字段可以实际接收专业数值输入。
    }
    assert_eq!(
        app.world().get::<BackgroundColor>(button).unwrap().0,
        colors::SELECT_BG
    ); // 验证积水工具分支进入正式高亮系统。
    for (kind, _, field) in &controls {
        let value = match kind {
            MapEditorBrushSliderKind::Radius => 72.0,
            MapEditorBrushSliderKind::Strength => 63.0,
            _ => continue,
        }; // 模拟作者分别提交米数及百分比。
        app.world_mut().write_message(NumericInputCommitted {
            entity: *field,
            value: NumericValue::F32(value),
            outcome: NumericInputCommitOutcome::Accepted,
            source: NumericInputCommitSource::Text,
            changed: true,
            edited: true,
        }); // 与中键精确输入的真实消息相同。
    }
    app.update(); // 消息必须实际写入笔刷读取的MapEditorState。
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 72.0); // 下一笔消费精确半径。
    assert_eq!(
        app.world().resource::<MapEditorState>().brush_strength,
        0.63
    ); // 百分比转换为归一化强度。
    app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::Inspect; // 用户退出积水工具。
    app.update(); // 原工具参数与按钮反馈应共同撤销。
    assert_eq!(
        app.world().get::<Node>(controls[0].1).unwrap().display,
        Display::None
    ); // 检查模式没有无效圆刷控件。
    assert_eq!(
        app.world().get::<BackgroundColor>(button).unwrap().0,
        colors::BUTTON_DEV_SMALL_NORMAL
    ); // 工具高亮不会残留。
} // 结束实际UI业务桥回归。

#[test]
fn volume_radius_updates_live_field_limits_and_accepts_sub_two_meter_input() {
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        tool: EditorTool::Raise,
        brush_radius: 72.0,
        ..default()
    })
    .init_resource::<EditorSettings>()
    .init_resource::<ActiveNumericInput>()
    .init_resource::<ActiveNumericInputScrub>()
    .init_resource::<BrushNumericInputState>()
    .add_message::<NumericInputChanged>()
    .add_message::<NumericInputCommitted>()
    .add_message::<NumericInputScrubSensitivityChanged>()
    .add_message::<NumericInputScrubSensitivityCommitted>()
    .add_message::<NumericInputSetValue>()
    .add_systems(
        Update,
        (handle_brush_numeric_inputs, sync_brush_sliders).chain(),
    );
    let kind = MapEditorBrushSliderKind::Radius;
    let field = app
        .world_mut()
        .spawn((
            MapEditorBrushSlider(kind),
            NumericInputValue(NumericValue::F32(72.0)),
            NumericInputConfig(numeric_spec(kind)),
            scrub_config(kind, 1.0),
        ))
        .id();
    for action in [
        EditorAction::ToolVolumeDig,
        EditorAction::ToolVolumeFill,
        EditorAction::ToolVolumeSmooth,
        EditorAction::ToolVolumeFlatten,
    ] {
        crate::ui::actions::request_terrain_tool(
            action,
            &mut app.world_mut().resource_mut::<MapEditorState>(),
        );
        app.update();
        let spec = app.world().get::<NumericInputConfig>(field).unwrap().0;
        let scrub = app.world().get::<NumericInputScrubConfig>(field).unwrap();
        assert_eq!(spec.min, Some(NumericValue::F32(0.5)));
        assert_eq!(spec.max, Some(NumericValue::F32(8.0)));
        assert_eq!(
            (scrub.minimum, scrub.maximum, scrub.drag_step),
            (0.5, 8.0, 0.1)
        );
        app.world_mut().write_message(NumericInputCommitted {
            entity: field,
            value: NumericValue::F32(0.7),
            outcome: NumericInputCommitOutcome::Accepted,
            source: NumericInputCommitSource::Text,
            changed: true,
            edited: true,
        });
        app.update();
        assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 0.7);
        app.world_mut()
            .resource_mut::<ActiveNumericInputScrub>()
            .entity = Some(field);
        app.world_mut().write_message(NumericInputChanged {
            entity: field,
            value: NumericValue::F32(0.6),
        });
        app.update();
        assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 0.6);
        app.world_mut()
            .resource_mut::<ActiveNumericInputScrub>()
            .entity = None;
        app.world_mut().write_message(NumericInputCommitted {
            entity: field,
            value: NumericValue::F32(0.6),
            outcome: NumericInputCommitOutcome::Accepted,
            source: NumericInputCommitSource::Scrub,
            changed: true,
            edited: true,
        });
        app.update();
        assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 0.6);
    }
    crate::ui::actions::request_terrain_tool(
        EditorAction::ToolRaise,
        &mut app.world_mut().resource_mut::<MapEditorState>(),
    );
    app.update();
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 2.0);
    assert_eq!(
        app.world().get::<NumericInputConfig>(field).unwrap().0.min,
        Some(NumericValue::F32(2.0))
    );
    assert_eq!(
        app.world()
            .get::<NumericInputScrubConfig>(field)
            .unwrap()
            .maximum,
        BRUSH_RADIUS_MAX_METERS
    );
}
