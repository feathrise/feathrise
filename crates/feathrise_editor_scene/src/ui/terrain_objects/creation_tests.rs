//! 创建后端边界、即时禁用和公共数值框真实输入回归，不写入项目场景资产。
use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    ui::terrain_objects::{
        components::*, page_tests, resources::TerrainObjectsEditorState, systems, types::Mutation,
    },
};
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    prelude::*,
};
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputConfig, NumericInputScrubConfig, NumericInputValue,
    NumericValue,
};

fn values(spacing: f32) -> [f32; 8] {
    [0.0, 0.0, 0.0, 64.0, 16.0, 64.0, 0.0, spacing]
}

#[test]
fn creation_spacing_retains_backend_ranges_and_real_source_contracts() {
    use SceneTerrainKind::{Gltf, SparseClipmap, Voxel};
    for (spacing, clipmap, voxel) in [
        (0.01, false, true),
        (0.124, false, true),
        (0.125, true, true),
        (0.5, true, true),
        (0.501, true, false),
        (64.0, true, false),
        (64.001, false, false),
    ] {
        assert_eq!(
            validate(SparseClipmap, &values(spacing)).is_ok(),
            clipmap,
            "Clipmap {spacing}"
        );
        assert_eq!(
            validate(Voxel, &values(spacing)).is_ok(),
            voxel,
            "体素 {spacing}"
        );
        assert!(validate(Gltf, &values(spacing)).is_ok());
    }
    // 小于一格的地形仍保留两个端点，不恢复已经退休的完整GLB高度网格。
    for extent in [0.01, 1.0, 63.999] {
        let mut fine = values(0.125); fine[3] = extent; fine[5] = extent;
        let spec = validate(SparseClipmap, &fine).unwrap();
        assert_eq!(spec.resolution, [((extent / 0.125).ceil() as u32 + 1); 2]);
    }
    feathrise_scene_content::SceneVoxelTerrainSource {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: feathrise_scene_content::SceneId::parse("MAP-creation-spacing").unwrap(),
        terrain_id: feathrise_scene_content::TerrainId::parse("terrain-fine").unwrap(),
        cell_size_meters: 0.01,
        min: [-0.5, -16.0, -0.5],
        max: [0.5, 0.0, 0.5],
        blocks: Default::default(),
        surface_fields_path: None,
    }
    .validate()
    .unwrap();
    assert!(
        validate(Gltf, &values(f32::NAN)).is_ok(),
        "原始资产导入不使用生成格距"
    );
    assert!(validate(SparseClipmap, &values(f32::NAN)).is_err());
    let mut too_large = values(0.01);
    too_large[3] = 1_000_000.0;
    too_large[5] = 1_000_000.0;
    assert!(
        validate(SparseClipmap, &too_large).is_err(),
        "分页源尺寸与格距约束仍参与预检"
    );
    for extent in [0.009, 8192.001] {
        let mut invalid = values(0.125);
        invalid[3] = extent;
        assert!(validate(SparseClipmap, &invalid).is_err());
    }
}

fn field(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &Field)>()
        .iter(world)
        .find(|(_, field)| **field == Field::Creation(7))
        .unwrap()
        .0
}
fn action(app: &mut App, target: Action) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ActionButton)>()
        .iter(world)
        .find(|(_, action)| action.0 == target)
        .unwrap()
        .0
}
fn point(app: &mut App, window: Entity, position: Vec2) {
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(position));
    app.world_mut()
        .write_message(bevy::window::WindowEvent::CursorMoved(
            bevy::window::CursorMoved {
                window,
                position,
                delta: None,
            },
        ));
    app.update();
}
fn mouse(app: &mut App, window: Entity, button: MouseButton, state: ButtonState) {
    let message = MouseButtonInput {
        button,
        state,
        window,
    };
    app.world_mut().write_message(message);
    app.world_mut()
        .write_message(bevy::window::WindowEvent::MouseButtonInput(message));
    app.update();
}
fn key(app: &mut App, window: Entity, code: KeyCode, logical: Key, text: Option<&str>) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical.clone(),
            state,
            text: if state == ButtonState::Pressed {
                text.map(Into::into)
            } else {
                None
            },
            repeat: false,
            window,
        });
        app.update();
    }
}
fn click_action(app: &mut App, window: Entity, target: Action) {
    let button = action(app, target);
    assert!(app.world().get::<ComputedNode>(button).unwrap().size().min_element() > 0.0,
        "回归必须点击可见的正式按钮");
    let position = app.world().get::<UiGlobalTransform>(button).unwrap().affine().translation;
    point(app, window, position);
    mouse(app, window, MouseButton::Left, ButtonState::Pressed);
    mouse(app, window, MouseButton::Left, ButtonState::Released);
    app.update();
}
fn open(app: &mut App, window: Entity) {
    click_action(app, window, Action::ToggleCreation);
    assert!(app.world().resource::<TerrainObjectsEditorState>().creation_open);
    // 来源按钮现在只选择规格；数字框必须通过真实来源和放置方式才取得可编辑身份。
    click_action(app, window, Action::Clipmap);
    click_action(app, window, Action::PlacementCoordinates);
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().creation_source, super::CreationSource::Clipmap);
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().placement_mode, super::CreationPlacementMode::Coordinates);
    assert!(app.world().resource::<TerrainObjectsEditorState>().pending.is_none());
}

#[test]
fn empty_scene_accepts_screenshot_rectangular_clipmap_at_minimum_requested_spacing() {
    let screenshot = [0.0, 0.0, 0.0, 97.1, 92.69, 95.4, 0.0, 0.125];
    let spec = validate_input(SceneTerrainKind::SparseClipmap, &screenshot)
        .expect("非整除矩形的0.125米请求仍合法，不能把实际均分格距当作请求下限");
    assert_eq!(spec.resolution, [778, 765]);
    let source = SparseHeightSpec::new(spec.min, spec.max, screenshot[7], -screenshot[4] * 0.5, screenshot[4]).unwrap();
    assert!(source.actual_spacing_meters[0] < 0.125);
    assert_eq!(source.cells.map(|count| count as u32 + 1), spec.resolution);

    let (mut app, window) = page_tests::creation_page();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrains = Some(Vec::new().into());
        draft.manifest.world_min = [-0.5; 2];
        draft.manifest.world_max = [0.5; 2];
        draft.terrain_documents = Default::default();
    }
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().creation = screenshot;
    open(&mut app, window);
    click_action(&mut app, window, Action::Clipmap);
    click_action(&mut app, window, Action::PlacementViewport);
    let start = action(&mut app, Action::StartCreation);
    assert!(!app.world().entity(start).contains::<bevy::ui::InteractionDisabled>(),
        "新建空场景不能依赖已有网格或地面；该规格开始按钮应立即可用");
    click_action(&mut app, window, Action::StartCreation);
    assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_some(),
        "真实开始按钮必须武装统一点放，选择来源本身不能提前创建");
    assert!(app.world().resource::<TerrainObjectsEditorState>().pending.is_none());
    assert!(app.world().resource::<TerrainObjectsEditorState>().selected.is_none());
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    key(&mut app, window, KeyCode::Escape, Key::Escape, None);
    assert!(app.world().resource::<TerrainObjectsEditorState>().point_placement.is_none());
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().creation, screenshot);
}

#[test]
fn creation_spacing_buttons_explain_unavailable_backends_without_changing_input() {
    let (mut app, window) = page_tests::creation_page();
    open(&mut app, window);
    app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some("gltf/creation-feedback.glb".into());
    let spacing_root = field(&mut app);
    for (spacing, clipmap, voxel) in [
        (0.125, true, true),
        (0.501, true, false),
        (64.001, false, false),
    ] {
        app.world_mut()
            .resource_mut::<TerrainObjectsEditorState>()
            .creation[7] = spacing;
        app.update();
        for (target, allowed) in [
            (Action::Clipmap, clipmap),
            (Action::Voxel, voxel),
            (Action::ImportGlb, true),
        ] {
            let label = match target {
                Action::Clipmap => "高度地形",
                Action::Voxel => "体素", _ => "原始 GLB",
            };
            let button = action(&mut app, target);
            assert!(!app.world().entity(button).contains::<bevy::ui::InteractionDisabled>(),
                "无效规格仍允许选择来源以阅读原因：{label}");
            click_action(&mut app, window, target);
            assert_eq!(field(&mut app), spacing_root, "切换来源只改变能力显隐，不重建数值输入根");
            assert_eq!(app.world().get::<ComputedNode>(spacing_root).unwrap().size().y > 0.0,
                target != Action::ImportGlb, "原始GLB不暴露未使用的生成格距");
            let start = action(&mut app, Action::StartCreation);
            assert_eq!(
                !app.world()
                    .entity(start)
                    .contains::<bevy::ui::InteractionDisabled>(),
                allowed,
                "格距 {spacing}，来源 {label}"
            );
            let feedback = {
                let world = app.world_mut();
                world.query_filtered::<&Text, With<CreationFeedback>>().single(world).unwrap().0.clone()
            };
            if allowed {
                assert!(feedback.contains("可创建"));
                if target == Action::ImportGlb { assert!(feedback.contains("gltf/creation-feedback.glb")); }
            } else {
                assert!(!feedback.contains("可创建"));
                assert!(feedback.contains(if target == Action::Voxel {
                    "本入口体素源格距必须在 0.125 至 0.500 米之间"
                } else { "0.125" }), "正文直接解释当前来源失败原因：{feedback}");
                let serial = app.world().resource::<EditorAuthoringSession>().next_serial;
                click_action(&mut app, window, Action::StartCreation);
                assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, serial);
                assert!(app.world().resource::<TerrainObjectsEditorState>().pending.is_none());
            }
        }
        let world = app.world_mut();
        let description = &world
            .query_filtered::<&Text, With<CreationSummary>>()
            .single(world)
            .unwrap()
            .0;
        assert!(description.contains(if clipmap {
            "高度地形：可创建"
        } else {
            "高度地形：不可创建"
        }));
        assert!(description.contains(if voxel {
            "体素地形：可创建"
        } else {
            "体素地形：不可创建"
        }));
        assert!(description.contains("公共源格距最小0.125m"));
        assert!(!description.contains("0.010"), "说明只呈现公共入口的可用范围");
        if !voxel {
            assert!(description.contains("本入口体素源格距必须在 0.125 至 0.500 米之间"),
                "不能把既有作者源格式的更低下限当成本入口的创建范围");
        }
        assert_eq!(
            world.resource::<TerrainObjectsEditorState>().creation[7],
            spacing
        );
        assert!(world.resource::<EditorAuthoringSession>().undo.is_empty(), "选择来源与无效开始不能产生作者历史");
    }
}

#[test]
fn creation_spacing_ui_reentry_normalizes_old_centimeter_state_before_creation() {
    let (mut app, window) = page_tests::creation_page();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .creation[7] = 0.01;
    app.update();
    assert_eq!(
        app.world().resource::<TerrainObjectsEditorState>().creation[7],
        0.01,
        "关闭状态不重写保留的创建记忆"
    );
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.update();
    assert_eq!(
        app.world().resource::<TerrainObjectsEditorState>().creation[7],
        0.125,
        "正式界面进入即归一，不依赖点击创建或数值提交"
    );
    open(&mut app, window);
    let root = field(&mut app);
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(0.125)
    );
    click_action(&mut app, window, Action::Clipmap);
    let start = action(&mut app, Action::StartCreation);
    assert!(!app.world().entity(start).contains::<bevy::ui::InteractionDisabled>());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision);
    assert!(session.undo.is_empty());
    assert!(app.world().resource::<TerrainObjectsEditorState>().pending.is_none());
}

#[test]
fn creation_spacing_precise_input_clamps_below_minimum_and_preserves_millimeters() {
    let (mut app, window) = page_tests::creation_page();
    open(&mut app, window);
    let root = field(&mut app);
    let config = app.world().get::<NumericInputConfig>(root).unwrap().0;
    assert_eq!(config.min, Some(NumericValue::F32(0.125)));
    assert_eq!(config.decimals, 3);
    assert_eq!(config.step, Some(NumericValue::F32(0.001)));
    let scrub = app.world().get::<NumericInputScrubConfig>(root).unwrap();
    assert_eq!(scrub.drag_step, 0.001);
    assert_eq!(scrub.minimum, 0.125);
    // 所选源格距仍反映原文件精度，不能因公共创建范围而把既有0.010m源显示成0.125m。
    let world = app.world_mut();
    let selected = world.query::<(Entity, &Field)>().iter(world)
        .find(|(_, field)| **field == Field::Selected(7)).unwrap().0;
    assert_eq!(world.get::<NumericInputConfig>(selected).unwrap().0.min, Some(NumericValue::F32(0.01)));
    for (text, value) in [("0.125", 0.125), ("0.126", 0.126), ("0.010", 0.125), ("0.124", 0.125)] {
        let position = app
            .world()
            .get::<UiGlobalTransform>(root)
            .unwrap()
            .affine()
            .translation;
        point(&mut app, window, position);
        mouse(&mut app, window, MouseButton::Middle, ButtonState::Pressed);
        mouse(&mut app, window, MouseButton::Middle, ButtonState::Released);
        assert_eq!(
            app.world().resource::<ActiveNumericInput>().entity,
            Some(root)
        );
        key(
            &mut app,
            window,
            KeyCode::Digit0,
            Key::Character(text.into()),
            Some(text),
        );
        key(&mut app, window, KeyCode::Enter, Key::Enter, None);
        assert_eq!(
            app.world().get::<NumericInputValue>(root).unwrap().0,
            NumericValue::F32(value)
        );
        assert_eq!(
            app.world().resource::<TerrainObjectsEditorState>().creation[7],
            value
        );
    }
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn creation_spacing_drag_reaches_and_stops_at_clipmap_minimum() {
    let (mut app, window) = page_tests::creation_page();
    open(&mut app, window);
    let root = field(&mut app);
    let position = app
        .world()
        .get::<UiGlobalTransform>(root)
        .unwrap()
        .affine()
        .translation;
    for distance in [12.5, 32.0] {
        point(&mut app, window, position);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
        point(&mut app, window, position - Vec2::X * distance);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
        assert_eq!(
            app.world().get::<NumericInputValue>(root).unwrap().0,
            NumericValue::F32(0.125)
        );
        assert_eq!(
            app.world().resource::<TerrainObjectsEditorState>().creation[7],
            0.125
        );
    }
    click_action(&mut app, window, Action::Clipmap);
    let start = action(&mut app, Action::StartCreation);
    assert!(
        !app.world()
            .entity(start)
            .contains::<bevy::ui::InteractionDisabled>()
    );
}

#[test]
fn creation_spacing_invalid_direct_request_never_starts_background_or_history() {
    let (mut app, window) = page_tests::creation_page();
    open(&mut app, window);
    app.world_mut()
        .resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
            world.resource_scope(|world, mut state: Mut<TerrainObjectsEditorState>| {
                state.creation[7] = 0.01;
                let serial = session.next_serial;
                let mut editor = world.resource_mut::<SceneEditorState>();
                systems::start(
                    &mut session,
                    &mut editor,
                    &mut state,
                    Mutation::Add(SceneTerrainKind::SparseClipmap),
                );
                assert!(state.pending.is_none());
                assert_eq!(session.next_serial, serial);
                assert!(session.undo.is_empty());
                assert!(editor.status.contains("0.125"));
                assert_eq!(state.creation[7], 0.01);
            });
        });
}
