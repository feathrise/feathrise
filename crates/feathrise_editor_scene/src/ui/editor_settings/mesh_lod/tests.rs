//! 正式全局LOD控件的提交、取消、隐藏拒收与中文布局回归；不加载场景或写settings.json。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, systems::*};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use crate::resources::{SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState, SceneEditorState};
use crate::ui::editor_settings::handle_settings_input;
use feathrise_editor_core::EditorSettings;
use feathrise_settings::{ MeshLodSettings};
use feathrise_editor_ui::*;

fn app() -> (App, [Entity; 4]) {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<EditorSettings>().init_resource::<SceneEditorLayoutState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .insert_resource(SceneEditorState { open: true, ..default() })
        .insert_resource(SceneEditorSettingsState { open: true, ..default() })
        .add_plugins(EditorUiPlugin)
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn_empty().with_children(|parent| {
                for field in [MeshLodField::ScreenError, MeshLodField::Hysteresis, MeshLodField::SafeNear, MeshLodField::CollisionError] {
                    let (label, unit, min, max, step) = field.config();
                    spawn_numeric_scrub_field(parent, field, default(), NumericScrubFieldOptions::new(
                        label, unit, NumericValue::F32(field.value(default())),
                        NumericInputSpec::f32(4).with_min(NumericValue::F32(min)).with_max(NumericValue::F32(max)),
                        NumericInputScrubConfig::linear(step, step, min, max, 1.0)));
                }
            });
        })
        .add_systems(Update, (handle_settings_input, handle_mesh_lod_inputs).chain().after(EditorUiSystemSet::NumericInput))
        .add_systems(PostUpdate, sync_mesh_lod_inputs);
    app.update();
    let mut fields = [Entity::PLACEHOLDER; 4];
    let world = app.world_mut();
    for (entity, field) in world.query::<(Entity, &MeshLodField)>().iter(world) {
        fields[match field { MeshLodField::ScreenError => 0, MeshLodField::Hysteresis => 1, MeshLodField::SafeNear => 2, MeshLodField::CollisionError => 3 }] = entity;
    }
    (app, fields)
}

fn commit(app: &mut App, entity: Entity, value: f32, outcome: NumericInputCommitOutcome) {
    app.world_mut().write_message(NumericInputCommitted { entity, value: NumericValue::F32(value), outcome,
        source: NumericInputCommitSource::Text, changed: true, edited: true });
    app.update();
}

#[test]
fn global_mesh_lod_controls_update_only_preferences_and_reject_invalid_or_hidden_commits() {
    let (mut app, fields) = app();
    app.world_mut().resource_mut::<EditorSettings>().editor_ui_scale = 1.25;
    commit(&mut app, fields[0], 0.5, NumericInputCommitOutcome::Accepted);
    commit(&mut app, fields[2], 64.0, NumericInputCommitOutcome::Accepted);
    commit(&mut app, fields[3], 0.005, NumericInputCommitOutcome::Accepted);
    let expected = MeshLodSettings { max_error_pixels: 0.5, collision_safe_near_meters: 64.0,
        collision_max_error_meters: 0.005, ..default() };
    assert_eq!(app.world().resource::<EditorSettings>().render.mesh_lod, expected);
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.25);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    for (entity, value) in [(fields[0], f32::NAN), (fields[0], 17.0), (fields[2], 31.0), (fields[3], -1.0)] {
        commit(&mut app, entity, value, NumericInputCommitOutcome::Accepted);
    }
    commit(&mut app, fields[0], 8.0, NumericInputCommitOutcome::Canceled);
    commit(&mut app, fields[0], 9.0, NumericInputCommitOutcome::Rejected);
    assert_eq!(app.world().resource::<EditorSettings>().render.mesh_lod, expected);
    app.world_mut().resource_mut::<SceneEditorSettingsState>().shortcuts = true;
    commit(&mut app, fields[0], 7.0, NumericInputCommitOutcome::Accepted);
    assert_eq!(app.world().resource::<EditorSettings>().render.mesh_lod, expected);
    app.world_mut().resource_mut::<SceneEditorSettingsState>().shortcuts = false;
    let toggle = app.world_mut().spawn((ActionButton(Action::Collision), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)).id();
    app.update();
    assert!(app.world().resource::<EditorSettings>().render.mesh_lod.collision_enabled);
    *app.world_mut().get_mut::<UiInteraction>(toggle).unwrap() = UiInteraction::None;
    app.world_mut().spawn((ActionButton(Action::Reset), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.update();
    assert_eq!(app.world().resource::<EditorSettings>().render.mesh_lod, MeshLodSettings::default());
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.25);
}

#[test]
fn global_lod_precision_escape_only_cancels_number_and_next_escape_closes_modal() {
    let (mut app, fields) = app();
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(fields[0]);
    {
        let mut edit = app.world_mut().get_mut::<NumericInputEditState>(fields[0]).unwrap();
        edit.editing = true;
        edit.buffer = "5".into();
    }
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    app.update();
    assert!(app.world().resource::<SceneEditorSettingsState>().open);
    assert_eq!(app.world().resource::<ActiveNumericInput>().entity, None);
    assert_eq!(app.world().resource::<EditorSettings>().render.mesh_lod, MeshLodSettings::default());
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Escape); keys.clear(); keys.press(KeyCode::Escape);
    }
    app.update();
    assert!(!app.world().resource::<SceneEditorSettingsState>().open);
}

#[test]
fn identical_or_canceled_global_lod_inputs_do_not_mark_preferences_changed_for_autosave() {
    let (mut app, fields) = app();
    let before = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
    commit(&mut app, fields[0], 1.0, NumericInputCommitOutcome::Accepted);
    assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), before);
    commit(&mut app, fields[2], 128.0, NumericInputCommitOutcome::Canceled);
    assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), before);
    commit(&mut app, fields[0], 0.75, NumericInputCommitOutcome::Accepted);
    assert_ne!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), before);
}

#[test]
fn global_lod_scrub_escape_restores_safe_near_without_closing_or_saving() {
    let (mut app, fields) = app();
    app.world_mut().resource_mut::<ActiveNumericInputScrub>().entity = Some(fields[2]);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    app.world_mut().run_system_once(handle_settings_input).unwrap();
    assert!(app.world().resource::<SceneEditorSettingsState>().open);
    let mut cursor = bevy::ecs::message::MessageCursor::<NumericInputSetValue>::default();
    assert!(cursor.read(app.world().resource::<Messages<NumericInputSetValue>>()).any(|event|
        event.entity == fields[2] && event.cancel_edit && event.value == NumericValue::F32(32.0)));
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn closing_or_switching_settings_page_cancels_existing_lod_buffer_and_refuses_late_commit() {
    for shortcuts in [false, true] {
        let (mut app, fields) = app();
        app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(fields[0]);
        {
            let mut edit = app.world_mut().get_mut::<NumericInputEditState>(fields[0]).unwrap();
            edit.editing = true; edit.buffer = "9".into();
        }
        if shortcuts { app.world_mut().resource_mut::<SceneEditorSettingsState>().shortcuts = true; }
        else { app.world_mut().resource_mut::<SceneEditorState>().open = false; }
        app.update(); app.update();
        assert_eq!(app.world().resource::<ActiveNumericInput>().entity, None);
        commit(&mut app, fields[0], 9.0, NumericInputCommitOutcome::Accepted);
        assert_eq!(app.world().resource::<EditorSettings>().render.mesh_lod, MeshLodSettings::default());
    }
}

#[test]
fn global_lod_card_fits_real_chinese_font_and_keeps_low_windows_scrollable() {
    use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
    use bevy::ui::{ComputedUiRenderTargetInfo, ComputedUiTargetCamera,
        ui_layout_system, ui_surface::UiSurface, update::propagate_ui_target_cameras};
    for (width, height, scale, dpi, must_scroll) in [(256.0, 360.0, 1.0, 1.0, false), (360.0, 480.0, 1.0, 1.0, false),
        (480.0, 540.0, 1.5, 1.0, false), (640.0, 720.0, 1.5, 2.0, false),
        // 仅倍率不能推断内容必然溢出；明确压缩正文高度，验证真正需要滚动的低窗口。
        (256.0, 120.0, 1.0, 1.0, true)] {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin { file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(), ..default() },
            bevy::text::TextPlugin, bevy::scene::ScenePlugin, bevy::transform::TransformPlugin,
            bevy::mesh::MeshPlugin, bevy::camera::visibility::VisibilityPlugin))
            .init_resource::<EditorSettings>().init_resource::<UiSurface>().insert_resource(UiScale(scale))
            .insert_resource(SceneEditorState { open: true, ..default() })
            .insert_resource(SceneEditorSettingsState { open: true, ..default() })
            .add_message::<MouseWheel>()
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate));
        app.configure_sets(PostUpdate, PropagateSet::<ComputedUiTargetCamera>::default()
            .after(propagate_ui_target_cameras).before(bevy::ui::widget::measure_text_system));
        app.configure_sets(PostUpdate, PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras).before(bevy::ui::widget::measure_text_system));
        app.add_systems(PostUpdate, (propagate_ui_target_cameras,
            bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender), ui_layout_system).chain());
        app.add_systems(Update, (setup_mesh_lod_inputs, crate::ui::editor_settings::scroll_settings_body));
        let logical_width = width / (scale * dpi);
        let logical_height = height / (scale * dpi);
        let scroll_cursor = bevy::ui::RelativeCursorPosition {
            cursor_over: true, normalized: Some(Vec2::splat(0.5)),
        };
        app.add_systems(Startup, SpawnSystem::spawn(move || bsn! {
            crate::ui::editor_settings::components::SettingsBody
            bevy::ui::RelativeCursorPosition ~{scroll_cursor}
            Node { width: px(logical_width), height: px(logical_height), min_width: px(0), min_height: px(0),
                flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y() } ScrollPosition::default()
            Children [@super::scene::panel()]
        }));
        app.world_mut().spawn((Camera2d, Camera { computed: ComputedCameraValues {
            target_info: Some(RenderTargetInfo { physical_size: UVec2::new(width as u32, height as u32), scale_factor: dpi }), ..default() },
            viewport: Some(Viewport { physical_size: UVec2::new(width as u32, height as u32), ..default() }), ..default() }));
        let font = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec()));
        for _ in 0..5 {
            app.update();
            let world = app.world_mut();
            for mut text_font in world.query::<&mut TextFont>().iter_mut(world) { text_font.font = font.clone().into(); }
        }
        app.update();
        let world = app.world_mut();
        let mut fields = 0;
        for (_, node, transform) in world.query::<(&MeshLodField, &ComputedNode, &UiGlobalTransform)>().iter(world) {
            let left = transform.translation.x - node.size().x * 0.5;
            let right = transform.translation.x + node.size().x * 0.5;
            assert!(left >= -1.1 && right <= width + 1.1, "LOD数值框越出侧栏：{left}..{right}/{width}/{scale}/{dpi}");
            assert!(node.size().y > 0.0);
            fields += 1;
        }
        assert_eq!(fields, 4);
        for (_, node, children) in world.query::<(&ActionButton, &ComputedNode, &Children)>().iter(world) {
            for child in children.iter() {
                if let Some(text) = world.get::<bevy::text::TextLayoutInfo>(child) {
                    assert!(text.size.x <= node.size().x + 1.1, "全局LOD按钮中文越出：{:?}/{:?}", text.size, node.size());
                }
            }
        }
        // Node自动带ScrollPosition；只验收常规正文宿主，不把数值框内部节点当作滚动页。
        let scroll = world.query_filtered::<(&ComputedNode, &ScrollPosition),
            With<crate::ui::editor_settings::components::SettingsBody>>().single(world).unwrap().0;
        let viewport = scroll.size();
        let content = scroll.content_size();
        eprintln!("全局LOD中文布局 width={width} height={height} scale={scale} dpi={dpi} viewport={viewport:?} content={content:?}");
        assert!((viewport.y - height).abs() <= 1.1, "正文视口与窗口高度不一致：{width}/{height}/{scale}/{dpi} viewport={viewport:?} content={content:?}");
        if must_scroll {
            assert!(content.y > viewport.y + 1.1,
                "低窗口须保留完整内容并允许滚动：{width}/{height}/{scale}/{dpi} viewport={viewport:?} content={content:?}");
            let (node, _) = world.query_filtered::<(&Node, &ScrollPosition),
                With<crate::ui::editor_settings::components::SettingsBody>>().single(world).unwrap();
            assert_eq!(node.overflow, Overflow::scroll_y());
            let body = world.query_filtered::<Entity,
                With<crate::ui::editor_settings::components::SettingsBody>>().single(world).unwrap();
            app.world_mut().write_message(MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y: -3.0, window: Entity::PLACEHOLDER,
                phase: bevy::input::touch::TouchPhase::Moved });
            app.update();
            let position = app.world().get::<ScrollPosition>(body).unwrap();
            assert!(position.y > 0.0 && position.y <= (content.y - viewport.y) / (scale * dpi) + 1.1,
                "低窗口实际滚轮须移动正文且不越界：{width}/{height}/{scale}/{dpi} position={position:?} viewport={viewport:?} content={content:?}");
        }
    }
}
