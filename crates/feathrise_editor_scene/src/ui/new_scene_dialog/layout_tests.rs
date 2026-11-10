//! 正式中文字体、原生文字/数值树及缩放DPI的无窗口布局回归；不替代实机视觉验收。

use super::components::*;
use super::workbench_tests::{entity, target};
use super::*;
use crate::components::{
    SceneEditorActionButton, NewSceneDialogPanel, NewSceneNameText, NewSceneNotesText,
};
use crate::types::EditorAction;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::text::TextLayoutInfo;
use bevy::window::PrimaryWindow;

#[derive(Resource)]
struct TestFont(Handle<Font>);

fn dialog() -> (App, Entity) {
    let mut app = super::tests::app();
    app.add_plugins((
        bevy::input::InputPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::input_focus::InputDispatchPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        bevy::ui::UiPlugin,
        bevy::ui_widgets::TextInputPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<bevy::image::TextureAtlasLayout>()
    .add_message::<bevy::window::Ime>()
    .init_resource::<feathrise_editor_core::EditorSettings>()
    .add_systems(
        Update,
        (
            sync_new_scene_dialog_status,
        ),
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .new_scene_name = "含中文名称的场景".into();
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(1280, 720),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
        ))
        .id();
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.insert_resource(TestFont(font));
    app.update();
    let root = entity::<NewSceneDialog>(&mut app);
    let host = app
        .world_mut()
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    app.world_mut().entity_mut(host).add_child(root);
    app.world_mut().get_mut::<Node>(root).unwrap().display = Display::Flex;
    settle(&mut app);
    (app, camera)
}

fn settle(app: &mut App) {
    for _ in 0..6 {
        app.update();
        let font = app.world().resource::<TestFont>().0.clone();
        let world = app.world_mut();
        for mut text in world.query::<&mut TextFont>().iter_mut(world) {
            if text.font != font.clone().into() {
                text.font = font.clone().into();
            }
        }
    }
    app.update();
}

fn bounds(app: &App, item: Entity) -> Rect {
    let size = app.world().get::<ComputedNode>(item).unwrap().size();
    let center = app
        .world()
        .get::<UiGlobalTransform>(item)
        .unwrap()
        .translation;
    Rect::from_center_size(center, size)
}

#[test]
fn create_and_edit_modal_fit_48_window_scale_dpi_combinations_with_long_feedback() {
    let (mut app, camera) = dialog();
    for editing in [false, true] {
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.new_scene_edit_target =
                editing.then(|| target("MAP-complete-chinese"));
            editor.status = format!(
                "保存失败：{}\n尚未写入，完整诊断可滚动查看。",
                "完整中文路径/".repeat(50)
            );
        }
        for width in [320, 430, 640, 1280] {
            for scale in [0.6, 1.0, 1.5] {
                for dpi in [1.0, 2.0] {
                    app.world_mut().resource_mut::<UiScale>().0 = scale;
                    app.world_mut()
                        .get_mut::<Camera>(camera)
                        .unwrap()
                        .computed
                        .target_info = Some(RenderTargetInfo {
                        physical_size: UVec2::new(
                            (width as f32 * dpi) as u32,
                            (360.0 * dpi) as u32,
                        ),
                        scale_factor: dpi,
                    });
                    settle(&mut app);
                    let factor = scale * dpi;
                    let panel = entity::<NewSceneDialogPanel>(&mut app);
                    let panel_bounds = bounds(&app, panel);
                    assert!(
                        panel_bounds.min.x >= 8.0 * factor - 1.1
                            && panel_bounds.max.x <= width as f32 * dpi - 8.0 * factor + 1.1,
                        "panel {panel_bounds:?}/{width}/{scale}/{dpi}"
                    );
                    assert!(
                        panel_bounds.min.y >= 8.0 * factor - 1.1
                            && panel_bounds.max.y <= 360.0 * dpi - 8.0 * factor + 1.1,
                        "panel height {panel_bounds:?}/{width}/{scale}/{dpi}"
                    );
                    let footer = entity::<DialogFooter>(&mut app);
                    let feedback = entity::<DialogFeedbackScroll>(&mut app);
                    let feedback_size = app.world().get::<ComputedNode>(feedback).unwrap().size();
                    assert!(feedback_size.y <= 60.0 * factor + 1.1);
                    assert!(bounds(&app, footer).max.y <= panel_bounds.max.y - 12.0 * factor + 1.1);
                    let status = entity::<NewSceneStatusText>(&mut app);
                    assert_eq!(
                        app.world().get::<Text>(status).unwrap().0,
                        app.world().resource::<SceneEditorState>().status
                    );
                    let actions: Vec<_> = {
                        let world = app.world_mut();
                        world
                            .query::<(Entity, &SceneEditorActionButton)>()
                            .iter(world)
                            .filter_map(|(entity, action)| {
                                matches!(
                                    action.0,
                                    EditorAction::ConfirmNewScene | EditorAction::CancelNewScene
                                )
                                .then_some(entity)
                            })
                            .collect()
                    };
                    assert_eq!(actions.len(), 2);
                    for button in actions {
                        let button_bounds = bounds(&app, button);
                        assert!(
                            button_bounds.min.x >= panel_bounds.min.x
                                && button_bounds.max.x <= panel_bounds.max.x + 1.1
                        );
                        assert!((button_bounds.height() - 26.0 * factor).abs() <= 1.1);
                        for child in app.world().get::<Children>(button).unwrap() {
                            let text_bounds = bounds(&app, *child);
                            assert!(
                                (text_bounds.center().y - button_bounds.center().y).abs() <= 1.1
                            );
                            assert!(
                                (text_bounds.center().x - button_bounds.center().x).abs() <= 1.1
                            );
                            assert!(
                                text_bounds.width() <= button_bounds.width() - 2.0 * factor + 1.1
                            );
                        }
                    }
                    let name = entity::<NewSceneNameText>(&mut app);
                    let notes = entity::<NewSceneNotesText>(&mut app);
                    assert!((bounds(&app, name).height() - 26.0 * factor).abs() <= 1.1);
                    assert_eq!(
                        *app.world().get::<bevy::text::LineHeight>(name).unwrap(),
                        bevy::text::LineHeight::Px(18.0)
                    );
                    assert!(
                        app.world().get::<TextLayoutInfo>(name).unwrap().size.y
                            <= bounds(&app, name).height() - 8.0 * factor + 1.1,
                        "name text {:?}, node {:?}/{width}/{scale}/{dpi}",
                        app.world().get::<TextLayoutInfo>(name).unwrap().size,
                        bounds(&app, name)
                    );
                    assert!(bounds(&app, notes).width() <= panel_bounds.width());
                    let fields: Vec<_> = {
                        let world = app.world_mut();
                        world.query_filtered::<Entity, With<feathrise_editor_ui::NumericInputValue>>().iter(world).collect()
                    };
                    assert!(fields.is_empty()); // 名称/备注之外不再保留隐形尺寸输入。
                }
            }
        }
    }
}
