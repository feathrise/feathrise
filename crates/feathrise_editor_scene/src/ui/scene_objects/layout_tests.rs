//! 真实中文字体的对象管理排版；窄侧栏、缩放和DPI下按钮折行、字段裁剪。

use super::components::{ObjectAction, ObjectButton, ObjectField, ObjectInput};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::asset::AssetPlugin;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::prelude::*;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};

#[test]
fn controls_fit_real_chinese_font_in_narrow_scaled_sidebars() {
    for (width, scale, dpi) in [
        (256.0, 1.0, 1.0),
        (360.0, 1.0, 1.0),
        (480.0, 1.5, 1.0),
        (640.0, 1.0, 2.0),
    ] {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                ..default()
            },
            bevy::text::TextPlugin,
            bevy::scene::ScenePlugin,
            bevy::transform::TransformPlugin,
            bevy::mesh::MeshPlugin,
            bevy::camera::visibility::VisibilityPlugin,
        ))
        .init_resource::<UiSurface>()
        .insert_resource(UiScale(scale))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
            PostUpdate,
        ))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
            PostUpdate,
        ));
        app.configure_sets(
            PostUpdate,
            PropagateSet::<ComputedUiTargetCamera>::default()
                .after(propagate_ui_target_cameras)
                .before(bevy::ui::widget::measure_text_system),
        );
        app.configure_sets(
            PostUpdate,
            PropagateSet::<ComputedUiRenderTargetInfo>::default()
                .after(propagate_ui_target_cameras)
                .before(bevy::ui::widget::measure_text_system),
        );
        app.add_systems(
            PostUpdate,
            (
                propagate_ui_target_cameras,
                bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
                ui_layout_system,
            )
                .chain(),
        );
        let logical_width = width / (scale * dpi);
        app.add_systems(Startup, bevy::prelude::SpawnSystem::spawn(move || bsn! {
            Node { width: px(logical_width), min_width: px(0), flex_direction: FlexDirection::Column }
            Children [@super::scene::properties_panel()]
        }));
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(width as u32, 800),
                        scale_factor: dpi,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: UVec2::new(width as u32, 800),
                    ..default()
                }),
                ..default()
            },
        ));
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(
                feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
            ));
        app.update();
        let world = app.world_mut();
        for mut node in world.query_filtered::<&mut Node, With<crate::ui::scene_inspector::InspectorSection>>().iter_mut(world) { node.display = Display::Flex; }
        for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
            text_font.font = font.clone().into();
        }
        for mut input in world
            .query_filtered::<&mut bevy::text::EditableText, With<ObjectInput>>()
            .iter_mut(world)
        {
            *input = crate::ui::text_input::single_line_input(&"灯".repeat(128), 128);
        }
        for _ in 0..4 {
            app.update();
        }
        let world = app.world_mut();
        let mut count = 0;
        for (node, transform, children) in world
            .query_filtered::<
                (&ComputedNode, &UiGlobalTransform, Option<&Children>),
                Or<(With<ObjectButton>, With<ObjectInput>)>,
            >()
            .iter(world)
        {
            let left = transform.translation.x - node.size().x * 0.5;
            let right = transform.translation.x + node.size().x * 0.5;
            assert!(
                left >= -1.0 && right <= width + 1.0,
                "管理控件越出侧栏：{left}..{right}, {width}"
            );
            assert!(node.size().y > 0.0);
            for child in children.into_iter().flat_map(|children| children.iter()) {
                if let Some(text) = world.get::<bevy::text::TextLayoutInfo>(child) {
                    assert!(text.size.x <= node.size().x + 1.0);
                }
            }
            count += 1;
        }
        assert_eq!(count, 6);
        let (name_size, name_center) = world.query::<(&ObjectInput, &ComputedNode, &UiGlobalTransform)>()
            .iter(world).find(|(input, ..)| input.0 == ObjectField::Name)
            .map(|(_, node, transform)| (node.size(), transform.translation)).unwrap();
        let (confirm_size, confirm_center) = world.query::<(&ObjectButton, &ComputedNode, &UiGlobalTransform)>()
            .iter(world).find(|(button, ..)| button.0 == ObjectAction::Rename)
            .map(|(_, node, transform)| (node.size(), transform.translation)).unwrap();
        assert!((name_center.y - confirm_center.y).abs() <= 1.1);
        assert!(name_center.x + name_size.x / 2.0 < confirm_center.x - confirm_size.x / 2.0);
        assert!(name_size.x > confirm_size.x); // 长名称占剩余一行，确认按钮只占标签所需尺寸。
        assert!(world.query::<&ObjectButton>().iter(world).all(|button|
            !matches!(button.0, ObjectAction::ToggleHidden | ObjectAction::ToggleLocked)));
    }
}

#[test]
fn placement_name_responds_to_pointer_and_renders_keyboard_and_ime_text() {
    use crate::authoring::EditorAuthoringSession;
    use crate::ui::{gltf_browser::EditorGltfBrowserState, terrain_objects::page_tests::{click, key, page}};
    use crate::viewport::{LayerCandidateState, multi_select::PlacementSelectionSet};
    use bevy::input::keyboard::Key;
    use bevy::input_focus::{InputFocus, pointer_focus::PointerFocusPlugin};
    use bevy::text::{EditableText, TextLayoutInfo};
    use bevy::window::{Ime, WindowEvent};

    let (mut app, window) = page();
    app.add_plugins(PointerFocusPlugin)
        .init_resource::<PlacementSelectionSet>()
        .init_resource::<crate::viewport::placement_move::PlacementMoveState>()
        .init_resource::<crate::authoring::placement_batch::PlacementBatchState>()
        .add_systems(Update, bevy::text::apply_text_edits.before(crate::SceneEditorSystemSet::Input));
    super::install(&mut app);
    let world = app.world_mut();
    world.spawn_scene(bsn! {
        Node { position_type: PositionType::Absolute, left: px(640), width: px(320) }
        Children [@super::scene::properties_panel()]
    }).unwrap();
    let font = world.resource_mut::<Assets<Font>>().add(Font::from_bytes(
        feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
    ));
    for mut text_font in world.query_filtered::<&mut TextFont, With<ObjectInput>>().iter_mut(world) {
        text_font.font = font.clone().into();
    }
    app.update();

    let world = app.world_mut();
    let mut record = crate::viewport::placement_move::interaction_tests::record();
    record.asset_path = "scenes/test/models/blender-roundtrip.glb".into();
    let id = record.id.clone();
    // 控件先完成初始化，再绑定带自定义名称的 GLB，覆盖回填丢失字体样式的路径。
    std::sync::Arc::make_mut(&mut record.editor).name = "铜灯".into();
    let mut session = world.resource_mut::<EditorAuthoringSession>();
    let revision = session.placement_structure_revision;
    session.draft.as_mut().unwrap().placements.placements.push(record);
    world.resource_mut::<EditorGltfBrowserState>().select_indexed_test_placement(&id, 0, revision);
    world.resource_mut::<LayerCandidateState>().clear_for_scene_tree();
    app.update();

    let world = app.world_mut();
    let input = world.query::<(Entity, &ObjectInput)>().iter(world)
        .find_map(|(entity, field)| (field.0 == ObjectField::Name).then_some(entity)).unwrap();
    let confirm = world.query::<(Entity, &ObjectButton)>().iter(world)
        .find_map(|(entity, button)| (button.0 == ObjectAction::Rename).then_some(entity)).unwrap();
    let layout = world.get::<TextLayoutInfo>(input).unwrap();
    assert_eq!(layout.glyphs.len(), 2);
    assert_eq!(layout.size.y, 18.0);

    click(&mut app, window, input, MouseButton::Left);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(input));
    key(&mut app, window, KeyCode::End, Key::End, None);
    key(&mut app, window, KeyCode::Digit2, Key::Character("2".into()), Some("2"));
    let ime = Ime::Commit { window, value: "号".into() };
    app.world_mut().write_message(ime.clone());
    app.world_mut().write_message(WindowEvent::Ime(ime));
    app.update();
    assert_eq!(app.world().get::<EditableText>(input).unwrap().value().to_string(), "铜灯2号");
    assert_eq!(app.world().get::<TextLayoutInfo>(input).unwrap().glyphs.len(), 4);

    click(&mut app, window, confirm, MouseButton::Left);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements[0].editor.name, "铜灯2号");
    assert_eq!(session.undo.len(), 1);
}
