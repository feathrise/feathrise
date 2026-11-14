//! 统一创建面板的真实按钮、动态字段、冻结数值身份和点放取消回归；不生成正式场景资产。

use super::*;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};

fn form(source: Action, mode: Action) -> (App, Entity) {
    let (mut app, window) = creation_page();
    app.init_resource::<VegetationPresetEditorState>();
    app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document;
    let toggle = action(&mut app, Action::ToggleCreation);
    click(&mut app, window, toggle, MouseButton::Left);
    let source = action(&mut app, source);
    click(&mut app, window, source, MouseButton::Left);
    let mode = action(&mut app, mode);
    click(&mut app, window, mode, MouseButton::Left);
    (app, window)
}

fn displayed(app: &mut App, target: Field) -> bool {
    let world = app.world_mut();
    world.query::<(&Slot, &Node)>().iter(world)
        .find(|(slot, _)| slot.0 == target).unwrap().1.display != Display::None
}

fn feedback(app: &mut App) -> String {
    let world = app.world_mut();
    world.query_filtered::<&Text, With<CreationFeedback>>().single(world).unwrap().0.clone()
}

fn unchanged(app: &App) {
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    let state = app.world().resource::<TerrainObjectsEditorState>();
    assert!(state.pending.is_none() && state.point_query.is_none());
}

#[test]
fn unified_creation_real_sources_and_modes_show_only_applicable_fields_without_switching_document() {
    let (mut app, window) = form(Action::Cuboid, Action::PlacementViewport);
    for (source, field, hidden) in [
        (Action::Cuboid, Field::CuboidCreation(0), Field::Creation(3)),
        (Action::Clipmap, Field::Creation(7), Field::Creation(4)),
        (Action::Voxel, Field::Creation(4), Field::CuboidCreation(0)),
        (Action::ImportGlb, Field::Creation(6), Field::Creation(7)),
    ] {
        let button = action(&mut app, source);
        click(&mut app, window, button, MouseButton::Left);
        assert!(displayed(&mut app, field), "适用字段必须显示：{source:?}");
        assert!(!displayed(&mut app, hidden), "无效参数必须隐藏：{source:?}");
        assert!(!displayed(&mut app, Field::Creation(0)));
        assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Document);
        unchanged(&app);
    }
    let coordinates = action(&mut app, Action::PlacementCoordinates);
    click(&mut app, window, coordinates, MouseButton::Left);
    assert!(displayed(&mut app, Field::Creation(0)));
    assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Document);
    let start = action(&mut app, Action::StartCreation);
    assert!(app.world().entity(start).contains::<bevy::ui::InteractionDisabled>());
    assert!(feedback(&mut app).contains("请先选择GLB资产"));
    app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some("fixtures/参考模型.glb".into());
    app.update();
    assert!(!app.world().entity(start).contains::<bevy::ui::InteractionDisabled>());
    assert!(feedback(&mut app).contains("资产：fixtures/参考模型.glb"));
    unchanged(&app);
}

#[test]
fn clipmap_ignores_hidden_height_memory_for_validation_and_viewport_intent_identity() {
    let (mut app, window) = form(Action::Clipmap, Action::PlacementViewport);
    for old_height in [f32::NAN, -3.0, 0.0, f32::INFINITY] {
        app.world_mut().resource_mut::<TerrainObjectsEditorState>().creation[4] = old_height;
        app.update();
        assert!(!displayed(&mut app, Field::Creation(4)));
        assert!(feedback(&mut app).contains("高度地形：可创建"));
        let state = app.world().resource::<TerrainObjectsEditorState>();
        assert!(creation::validate_selected(state).is_ok());
        assert_eq!(creation::normalized(SceneTerrainKind::SparseClipmap, state.creation)[4], 16.0);
    }
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().creation[4] = f32::NAN;
    let start = action(&mut app, Action::StartCreation);
    click(&mut app, window, start, MouseButton::Left);
    for _ in 0..3 { app.update(); }
    assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_some(),
        "隐藏NaN不参与规格，冻结身份必须按保留位比较，不能使有效Clipmap点放立即失效");
    unchanged(&app);
}

#[test]
fn creation_numeric_late_commit_cannot_follow_source_or_placement_purpose() {
    for switch in [Action::Voxel, Action::PlacementViewport] {
        let (mut app, window) = form(Action::Clipmap, Action::PlacementCoordinates);
        let input = field(&mut app, Field::Creation(0));
        let old = app.world().resource::<TerrainObjectsEditorState>().creation[0];
        click(&mut app, window, input, MouseButton::Middle);
        assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(input));
        let button = action(&mut app, switch);
        click(&mut app, window, button, MouseButton::Left);
        late_commit(&mut app, input, 712.0);
        assert_eq!(app.world().resource::<TerrainObjectsEditorState>().creation[0], old);
        unchanged(&app);
    }
}

#[test]
fn real_start_release_arms_terrain_point_placement_while_hovering_controls_then_cancel_keeps_history_empty() {
    for cancel_with_escape in [false, true] {
        let (mut app, window) = form(Action::Clipmap, Action::PlacementViewport);
        app.world_mut().resource_mut::<ViewportModeState>().pointer_over_controls = true;
        let start = action(&mut app, Action::StartCreation);
        point_at(&mut app, window, start);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
        assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_none());
        mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
        assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_some());
        assert!(app.world().resource::<TerrainObjectsEditorState>().owns_mouse());
        for _ in 0..4 { app.update(); }
        assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_some(),
            "开始后仍悬停面板只阻止新点击，不能撤销已武装的点放");
        assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Scene);
        unchanged(&app);
        if cancel_with_escape { key(&mut app, window, KeyCode::Escape, Key::Escape, None); }
        else {
            let cancel = action(&mut app, Action::CancelPlacement);
            assert!(!app.world().entity(cancel).contains::<bevy::ui::InteractionDisabled>());
            click(&mut app, window, cancel, MouseButton::Left);
        }
        assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_none());
        assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Select);
        unchanged(&app);
    }
}

#[test]
fn terrain_point_placement_rejects_changed_author_modal_document_tool_and_window_contexts() {
    for case in 0..6 {
        let (mut app, window) = form(Action::Clipmap, Action::PlacementViewport);
        let start = action(&mut app, Action::StartCreation);
        click(&mut app, window, start, MouseButton::Left);
        assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_some());
        match case {
            0 => app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1,
            1 => app.world_mut().resource_mut::<SceneEditorState>().new_scene_dialog_open = true,
            2 => app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document,
            3 => app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Move,
            4 => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            _ => app.world_mut().resource_mut::<SceneEditorState>().selected_scene_id = Some(SceneId::parse("MAP-foreign-create").unwrap()),
        }
        app.update();
        assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_none(), "失效场景{case}");
        if case == 3 { assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Move); }
        unchanged(&app);
    }
}
