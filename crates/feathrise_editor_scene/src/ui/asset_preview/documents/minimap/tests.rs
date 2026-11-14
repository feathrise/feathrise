//! 真实滚动资源、手势所有权与后台换代回归；GPU完整排版由父层长JSON用例覆盖。

use feathrise_editor_ui::UiInteraction;
use super::super::{components::*, constants::*, resources::*, types::*};
use super::{
    control,
    math::{self, OverviewMetrics},
    raster, systems,
};
use bevy::{
    prelude::*,
    text::{EditableText, TextLayoutInfo},
    ui::RelativeCursorPosition,
    window::PrimaryWindow,
};
use std::{path::PathBuf, sync::Arc};

fn document() -> Document {
    Document {
        preview_scroll: Vec2::ZERO,
        path: "test.rs".into(),
        format: crate::asset_catalog::DocumentFormat::Code,
        generation: 1,
        revision: 0,
        saved_revision: 0,
        history: default(),
        disk: DiskIdentity {
            path: PathBuf::new(),
            digest: [0; 32],
            bom: false,
            crlf: false,
        },
        pages: vec![Arc::from("fn main() {}")],
        source: Arc::from("fn main() {}"),
        preview: Arc::new(Vec::new()),
        preview_revision: None,
        preview_clipped: false,
        mode: ViewMode::Edit,
        confirm_reload: false,
        external: default(),
        status: String::new(),
    }
}

fn fixture() -> (App, Entity, Entity, Entity) {
    use crate::{resources::*, ui::vegetation_preset_editor::*};
    let doc = document();
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .add_message::<bevy::input::mouse::MouseWheel>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<SceneBrowserDragState>()
        .init_resource::<Assets<Image>>()
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .init_resource::<VegetationPresetEditorState>()
        .insert_resource(DocumentState {
            active: Some(doc.path.clone()),
            visible: true,
            central_active: true,
            documents: vec![doc.clone()],
            ..default()
        })
        .insert_resource(DocumentMinimapState {
            shown: true,
            ..default()
        });
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Document;
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    let source = app
        .world_mut()
        .spawn((
            DocumentSource {
                key: Some(doc.source_key()),
            },
            ComputedNode {
                size: Vec2::new(500.0, 300.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            TextLayoutInfo {
                size: Vec2::new(500.0, 6_000.0),
                ..default()
            },
            EditableText::default(),
        ))
        .id();
    let rail = app
        .world_mut()
        .spawn((
            DocumentMinimapRail,
            ComputedNode {
                size: Vec2::new(96.0, 600.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
            UiInteraction::Hovered,
        ))
        .id();
    (app, source, rail, window)
}

#[test]
fn minimap_scenes_long_document_and_minimum_thumb_to_exact_endpoints() {
    let metrics = OverviewMetrics::new(
        Vec2::new(500.0, 100_000.0),
        Vec2::new(500.0, 300.0),
        Vec2::new(96.0, 600.0),
        99_700.0,
        2.0,
    )
    .unwrap();
    assert_eq!(metrics.thumb_height, 24.0);
    assert_eq!(metrics.thumb_top + metrics.thumb_height, 600.0);
    assert_eq!(metrics.scroll_for_top(-1.0), 0.0);
    assert_eq!(metrics.scroll_for_top(600.0), 99_700.0);
    assert!((metrics.scroll_for_top(288.0) - 49_850.0).abs() < 0.01);
}

#[test]
fn minimap_short_document_does_not_stretch_or_scroll() {
    let metrics = OverviewMetrics::new(
        Vec2::new(500.0, 100.0),
        Vec2::new(500.0, 300.0),
        Vec2::new(96.0, 600.0),
        0.0,
        1.0,
    )
    .unwrap();
    assert!((metrics.height - 19.2).abs() < 0.01);
    assert_eq!(metrics.thumb_height, metrics.height);
    assert_eq!(metrics.scroll_for_top(600.0), 0.0);
}

#[test]
fn minimap_rejects_missing_and_nonfinite_layout() {
    for extent in [
        Vec2::ZERO,
        Vec2::new(f32::NAN, 100.0),
        Vec2::new(100.0, f32::INFINITY),
    ] {
        assert!(
            OverviewMetrics::new(extent, Vec2::splat(100.0), Vec2::splat(100.0), 0.0, 1.0)
                .is_none()
        );
    }
    assert!(math::mark(Vec2::splat(10.0), Vec2::ZERO, Vec2::splat(100.0)).is_none());
}

#[test]
fn minimap_empty_document_finishes_with_a_transparent_outline() {
    let (mut app, source, _, _) = fixture();
    app.add_systems(Update, systems::update_outline);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("");
    app.world_mut()
        .get_mut::<TextLayoutInfo>(source)
        .unwrap()
        .size = Vec2::new(0.0, 18.0);
    let metrics = OverviewMetrics::new(
        Vec2::new(0.0, 18.0),
        Vec2::new(500.0, 300.0),
        Vec2::new(96.0, 600.0),
        0.0,
        1.0,
    )
    .unwrap();
    assert_eq!(metrics.maximum, 0.0);
    for _ in 0..100 {
        app.update();
        if app
            .world()
            .resource::<DocumentMinimapState>()
            .picture
            .is_some()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let picture = app
        .world()
        .resource::<DocumentMinimapState>()
        .picture
        .as_ref()
        .unwrap();
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&picture.image)
        .unwrap();
    assert!(image.data.as_ref().unwrap().iter().all(|value| *value == 0));
}

#[test]
fn minimap_raster_keeps_indentation_and_transparent_gaps() {
    let mark = math::mark(
        Vec2::new(250.0, 50.0),
        Vec2::new(100.0, 10.0),
        Vec2::new(500.0, 100.0),
    )
    .unwrap();
    assert!(mark.min[0] > 30);
    let key = MinimapKey {
        source: Entity::PLACEHOLDER,
        document: document().source_key(),
        revision: 0,
        extent: Vec2::new(500.0, 100.0),
        view_width: 500.0,
        glyphs: 1,
    };
    let result = raster::rasterize(key, vec![mark], [160, 150, 140, 255]);
    assert_eq!(
        result.pixels.len(),
        (MINIMAP_WIDTH * MINIMAP_HEIGHT * 4) as usize
    );
    assert_eq!(&result.pixels[..4], &[0, 0, 0, 0]);
    let offset = (512 * MINIMAP_WIDTH as usize + 48) * 4;
    assert_eq!(&result.pixels[offset..offset + 4], &[160, 150, 140, 255]);
}

#[test]
fn minimap_real_pointer_click_and_drag_use_centered_coordinates_at_all_scales() {
    use bevy::{
        app::HierarchyPropagatePlugin,
        camera::{ComputedCameraValues, RenderTargetInfo},
        ecs::system::RunSystemOnce,
        input::touch::Touches,
        ui::{FocusPolicy, UiStack, ui_focus_system, update::propagate_ui_target_cameras},
    };
    fn move_pointer(app: &mut App, window: Entity, point: Vec2) {
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(point));
        app.world_mut().write_message(bevy::window::WindowEvent::CursorMoved(bevy::window::CursorMoved {
            window, position: point, delta: None,
        }));
    }
    fn pointer_button(app: &mut App, window: Entity, state: bevy::input::ButtonState) {
        app.world_mut().write_message(bevy::window::WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left, state, window,
        }));
    }
    for scale in [1.0, 1.1, 2.0] {
        let (mut app, source, rail, window) = fixture();
        app.insert_resource(UiScale(scale))
            .init_resource::<Touches>()
            .add_message::<bevy::window::WindowEvent>()
            .add_message::<bevy::input::touch::TouchInput>()
            .add_plugins((bevy::picking::input::PointerInputPlugin, bevy::picking::PickingPlugin,
                bevy::picking::InteractionPlugin, bevy::ui::picking_backend::UiPickingPlugin,
                feathrise_editor_ui::UiInteractionPlugin))
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
            .add_systems(Update, propagate_ui_target_cameras);
        let camera = app.world_mut().spawn((Camera2d, Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(1920, 2000),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        })).id();
        app.world_mut().get_mut::<Window>(window).unwrap().resolution.set_physical_resolution(1920, 2000);
        let center = Vec2::new(300.0, 400.0) * scale;
        app.world_mut().entity_mut(rail).insert((
            Node::default(), UiTargetCamera(camera), FocusPolicy::Block,
            InheritedVisibility::VISIBLE, UiGlobalTransform::from_translation(center),
        ));
        app.world_mut().get_mut::<ComputedNode>(rail).unwrap().size *= scale;
        app.world_mut().get_mut::<ComputedNode>(source).unwrap().size *= scale;
        app.world_mut().get_mut::<ComputedNode>(source).unwrap().inverse_scale_factor = scale.recip();
        app.world_mut().get_mut::<TextLayoutInfo>(source).unwrap().size *= scale;
        app.insert_resource(UiStack { partition: vec![0..1], uinodes: vec![rail] });
        app.update(); // 先采用正式相机传播，再由真实命中系统计算坐标；不手工注入normalized。
        app.add_systems(Update, control::handle_input);
        let selection = app.world().get::<EditableText>(source).unwrap().editor.raw_selection().text_range();
        move_pointer(&mut app, window, center);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        pointer_button(&mut app, window, bevy::input::ButtonState::Pressed);
        app.world_mut().run_system_once(ui_focus_system).unwrap();
        // 旧focus只提供真实归一化坐标；三态须由正式现代拾取和共享投影生成。
        app.update();
        assert_eq!(*app.world().get::<UiInteraction>(rail).unwrap(), UiInteraction::Pressed);
        assert_eq!(app.world().get::<RelativeCursorPosition>(rail).unwrap().normalized, Some(Vec2::ZERO));
        assert!((app.world().get::<EditableText>(source).unwrap().viewport.offset.y - 2_850.0 * scale).abs() < 1.0,
            "真实轨道中心点击应导航到全文中间，UI倍率={scale}");
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        // 持续按住左键，从中心往上拖到四分之一位置，再越界到全文首尾。
        for (relative_y, expected) in [(-150.0, 1_350.0), (-320.0, 0.0), (320.0, 5_700.0)] {
            move_pointer(&mut app, window, center + Vec2::new(0.0, relative_y * scale));
            app.world_mut().run_system_once(ui_focus_system).unwrap();
            app.update();
            assert!((app.world().get::<EditableText>(source).unwrap().viewport.offset.y - expected * scale).abs() < 1.0,
                "真实拖动位置必须与全文视区一致，位置={relative_y}，UI倍率={scale}");
        }
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        pointer_button(&mut app, window, bevy::input::ButtonState::Released);
        app.world_mut().run_system_once(ui_focus_system).unwrap();
        app.update();
        assert!(app.world().resource::<DocumentMinimapState>().drag.is_none());
        assert!(app.world().resource::<DocumentMinimapState>().owns_mouse());
        assert_eq!(app.world().get::<EditableText>(source).unwrap().editor.raw_selection().text_range(), selection);
        let doc = app.world().resource::<DocumentState>().current().unwrap();
        assert!(!doc.dirty());
        assert_eq!(doc.source_text(), "fn main() {}");
    }
}

#[test]
fn minimap_click_drag_outside_and_release_preserve_caret_and_draft() {
    let (mut app, source, rail, _) = fixture();
    app.add_systems(Update, control::handle_input);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let selection = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    app.update();
    assert!((app.world().get::<EditableText>(source).unwrap().viewport.offset.y - 2_850.0).abs() < 1.0);
    assert!(app.world().resource::<DocumentMinimapState>().owns_mouse());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(rail)
        .unwrap()
        .normalized = Some(Vec2::new(2.0, 2.0));
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(rail)
        .unwrap()
        .cursor_over = false;
    app.update();
    assert_eq!(app.world().get::<EditableText>(source).unwrap().viewport.offset.y, 5_700.0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<DocumentMinimapState>()
            .drag
            .is_none()
    );
    assert!(app.world().resource::<DocumentMinimapState>().owns_mouse());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    assert!(!app.world().resource::<DocumentMinimapState>().owns_mouse());
    assert_eq!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        selection
    );
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(!doc.dirty());
    assert_eq!(doc.source_text(), "fn main() {}");
    assert!(
        !app.world()
            .resource::<crate::resources::SceneEditorState>()
            .dirty
    );
}

#[test]
fn minimap_drag_cancels_on_focus_loss_modal_hide_and_document_change() {
    for cancel in 0..4 {
        let (mut app, _, _, window) = fixture();
        app.add_systems(Update, control::handle_input);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(
            app.world()
                .resource::<DocumentMinimapState>()
                .drag
                .is_some()
        );
        match cancel {
            0 => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            1 => {
                app.world_mut()
                    .resource_mut::<crate::resources::SceneEditorState>()
                    .new_scene_dialog_open = true
            }
            2 => app.world_mut().resource_mut::<DocumentMinimapState>().shown = false,
            _ => {
                app.world_mut()
                    .resource_mut::<DocumentState>()
                    .current_mut()
                    .unwrap()
                    .revision += 1
            }
        }
        app.update();
        assert!(
            app.world()
                .resource::<DocumentMinimapState>()
                .drag
                .is_none()
        );
        assert!(app.world().resource::<DocumentMinimapState>().owns_mouse());
    }
}

#[test]
fn minimap_middle_chord_and_existing_owner_cannot_start_navigation() {
    for blocked in [true, false] {
        let (mut app, source, _, _) = fixture();
        app.add_systems(Update, control::handle_input);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        if blocked {
            app.world_mut()
                .resource_mut::<crate::resources::SceneEditorNumericScrubMouseGuard>()
                .advance(true, true, false);
        } else {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Middle);
        }
        app.update();
        assert!(
            app.world()
                .resource::<DocumentMinimapState>()
                .drag
                .is_none()
        );
        assert_eq!(app.world().get::<EditableText>(source).unwrap().viewport.offset.y, 0.0);
    }
}

#[test]
fn minimap_publishes_mouse_ownership_until_chord_and_release_clear() {
    use crate::resources::SceneEditorNumericScrubMouseGuard;
    let (mut app, _, _, _) = fixture();
    app.init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>();
    app.add_systems(
        Update,
        (
            control::handle_input,
            crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
        )
            .chain(),
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Middle);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<DocumentMinimapState>()
            .drag
            .is_none()
    );
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Middle);
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    assert!(
        !app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn minimap_wheel_consumes_once_including_document_end() {
    use bevy::{
        ecs::system::RunSystemOnce,
        input::mouse::{MouseScrollUnit, MouseWheel},
        input::touch::TouchPhase,
    };
    let (mut app, source, _, window) = fixture();
    app.add_systems(Update, control::handle_input);
    app.world_mut().get_mut::<EditableText>(source).unwrap().viewport.offset.y = 5_700.0;
    app.world_mut().write_message(MouseWheel {
        window,
        unit: MouseScrollUnit::Pixel,
        x: 0.0,
        y: -300.0,
        phase: TouchPhase::Moved,
    });
    app.update();
    assert!(app.world().resource::<DocumentMinimapState>().owns_wheel());
    app.world_mut().get_mut::<EditableText>(source).unwrap().viewport.offset.y = 3_000.0;
    app.world_mut().entity_mut(source).insert((
        crate::ui::vegetation_preset_editor::PresetNotesScroll,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app.world_mut()
        .run_system_once(crate::ui::systems::scroll_editor_side_panels)
        .unwrap();
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().viewport.offset.y,
        3_000.0,
        "外层不能重复消费小场景滚轮"
    );
    app.update();
    assert!(!app.world().resource::<DocumentMinimapState>().owns_wheel());
}

#[test]
fn minimap_snapshot_has_frame_budget_and_stable_outline_is_reused() {
    let (mut app, source, _, _) = fixture();
    app.add_systems(Update, systems::update_outline);
    let glyph = bevy::text::PositionedGlyph {
        position: Vec2::new(80.0, 25.0),
        section_index: 0,
        line_index: 0,
        atlas_info: bevy::text::GlyphAtlasInfo {
            texture: Handle::<Image>::default().id(),
            rect: Rect::new(0.0, 0.0, 8.0, 10.0),
            offset: Vec2::ZERO,
            is_alpha_mask: true,
        },
    };
    app.world_mut()
        .get_mut::<TextLayoutInfo>(source)
        .unwrap()
        .glyphs = vec![glyph; MINIMAP_GLYPHS_PER_FRAME + 5];
    app.update();
    let state = app.world().resource::<DocumentMinimapState>();
    assert!(state.task.is_none());
    assert_eq!(
        state.collecting.as_ref().unwrap().cursor,
        MINIMAP_GLYPHS_PER_FRAME
    );
    for _ in 0..100 {
        app.update();
        if app
            .world()
            .resource::<DocumentMinimapState>()
            .picture
            .is_some()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let image = app
        .world()
        .resource::<DocumentMinimapState>()
        .picture
        .as_ref()
        .unwrap()
        .image
        .clone();
    for _ in 0..5 {
        app.update();
    }
    let state = app.world().resource::<DocumentMinimapState>();
    assert_eq!(state.picture.as_ref().unwrap().image, image);
    assert!(state.collecting.is_none() && state.task.is_none());
}

#[test]
fn minimap_hidden_late_task_keeps_slot_and_rejects_old_document() {
    let (mut app, source, _, _) = fixture();
    app.add_systems(Update, systems::update_outline);
    let key = MinimapKey {
        source,
        document: document().source_key(),
        revision: 0,
        extent: Vec2::new(500.0, 6_000.0),
        view_width: 500.0,
        glyphs: 0,
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().resource_mut::<DocumentMinimapState>().task =
        Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
            receiver.recv().unwrap();
            raster::rasterize(key, vec![], [0; 4])
        }));
    app.world_mut().resource_mut::<DocumentMinimapState>().shown = false;
    app.update();
    assert!(
        app.world()
            .resource::<DocumentMinimapState>()
            .task
            .is_some()
    );
    assert!(
        app.world()
            .resource::<DocumentMinimapState>()
            .collecting
            .is_none()
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .generation += 1;
    sender.send(()).unwrap();
    for _ in 0..100 {
        app.update();
        if app
            .world()
            .resource::<DocumentMinimapState>()
            .task
            .is_none()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let state = app.world().resource::<DocumentMinimapState>();
    assert!(state.task.is_none() && state.picture.is_none());
}
