//! 正式中文字体与原生输入树的管理模态布局回归；无窗口检查不代替实机验收。

use super::DialogKind;
use super::components::*;
use super::tests::{deletion, entity, folder};
use crate::authoring::TerrainExternalChangeState;
use crate::components::{MapFolderDialogNameText, MapFolderDialogStatusText};
use crate::resources::{MapBrowserState, MapDeletionTask, MapEditorState};
use crate::types::MapFolderDialogMode;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::prelude::*;
use bevy::text::TextLayoutInfo;
use bevy::window::PrimaryWindow;
use feathrise_map_content::MapId;

#[derive(Resource)]
struct TestFont(Handle<Font>);

fn layout_app() -> (App, Entity) {
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
    .add_message::<bevy::window::Ime>();
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
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    app.update();
    let name = entity::<MapFolderDialogNameText>(&mut app);
    app.world_mut()
        .get_mut::<bevy::text::EditableText>(name)
        .unwrap()
        .editor
        .set_text("含中文名字的文件夹");
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
fn five_modes_fit_120_actual_chinese_window_scale_dpi_combinations() {
    let (mut app, camera) = layout_app();
    for mode in 0..5 {
        app.world_mut()
            .resource_mut::<MapDeletionTask>()
            .confirmation = None;
        app.world_mut().resource_mut::<MapBrowserState>().dialog = None;
        app.world_mut()
            .resource_mut::<TerrainExternalChangeState>()
            .conflict = None;
        let kind = match mode {
            0 => {
                let mut request = deletion("MAP-long-chinese-name");
                request.display_name = "长中文地图名称".repeat(16);
                app.world_mut()
                    .resource_mut::<MapDeletionTask>()
                    .confirmation = Some(request);
                DialogKind::MapDeletion
            }
            1..=3 => {
                let folder_mode = match mode {
                    1 => MapFolderDialogMode::Create,
                    2 => MapFolderDialogMode::Rename,
                    _ => MapFolderDialogMode::Delete,
                };
                let mut request = folder(folder_mode);
                request.name = "长中文文件夹".repeat(8);
                let mut browser = app.world_mut().resource_mut::<MapBrowserState>();
                browser.dialog = Some(request);
                browser.last_error = Some(format!(
                    "真实写入失败：{}\n完整诊断结尾",
                    "完整中文路径/".repeat(160)
                ));
                DialogKind::Folder
            }
            _ => {
                app.world_mut()
                    .resource_mut::<TerrainExternalChangeState>()
                    .register_save_conflict(MapId::parse("MAP-external-terrain").unwrap(), [8; 32]);
                DialogKind::TerrainConflict
            }
        };
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
                    let panel = {
                        let world = app.world_mut();
                        world
                            .query::<(Entity, &ManagementPanel)>()
                            .iter(world)
                            .find(|(_, marker)| marker.0 == kind)
                            .unwrap()
                            .0
                    };
                    let panel_bounds = bounds(&app, panel);
                    assert!(
                        panel_bounds.min.x >= 8.0 * factor - 1.1
                            && panel_bounds.max.x <= width as f32 * dpi - 8.0 * factor + 1.1,
                        "horizontal {mode}/{width}/{scale}/{dpi}: {panel_bounds:?}"
                    );
                    assert!(
                        panel_bounds.min.y >= 8.0 * factor - 1.1
                            && panel_bounds.max.y <= 360.0 * dpi - 8.0 * factor + 1.1,
                        "vertical {mode}/{width}/{scale}/{dpi}: {panel_bounds:?}"
                    );
                    let footer = {
                        let world = app.world_mut();
                        world
                            .query::<(Entity, &ManagementFooter)>()
                            .iter(world)
                            .find(|(_, marker)| marker.0 == kind)
                            .unwrap()
                            .0
                    };
                    let feedback = {
                        let world = app.world_mut();
                        world
                            .query::<(Entity, &ManagementFeedback)>()
                            .iter(world)
                            .find(|(_, marker)| marker.0 == kind)
                            .unwrap()
                            .0
                    };
                    let footer_bounds = bounds(&app, footer);
                    let feedback_bounds = bounds(&app, feedback);
                    assert!(feedback_bounds.height() <= 64.0 * factor + 1.1);
                    assert!(feedback_bounds.max.y <= footer_bounds.min.y + 1.1);
                    assert!(footer_bounds.max.y <= panel_bounds.max.y - factor + 1.1);
                    let buttons = app.world().get::<Children>(footer).unwrap().to_vec();
                    assert_eq!(buttons.len(), if kind == DialogKind::MapDeletion { 3 } else { 2 });
                    for button in buttons {
                        let button_bounds = bounds(&app, button);
                        assert!((button_bounds.height() - 26.0 * factor).abs() <= 1.1);
                        assert!(
                            button_bounds.min.x >= panel_bounds.min.x
                                && button_bounds.max.x <= panel_bounds.max.x + 1.1
                        );
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
                    let body = {
                        let world = app.world_mut();
                        world.query_filtered::<(Entity, &ManagementScroll), Without<ManagementFeedback>>().iter(world).find(|(_, marker)| marker.0 == kind).unwrap().0
                    };
                    assert!(bounds(&app, body).max.y <= feedback_bounds.min.y + 1.1);
                    if mode == 1 || mode == 2 {
                        let name = entity::<MapFolderDialogNameText>(&mut app);
                        let wrapper = app.world().get::<ChildOf>(name).unwrap().parent();
                        assert!((bounds(&app, wrapper).height() - 26.0 * factor).abs() <= 1.1);
                        assert!(
                            (bounds(&app, name).center().y - bounds(&app, wrapper).center().y)
                                .abs()
                                <= 1.1
                        );
                        assert!(
                            app.world().get::<TextLayoutInfo>(name).unwrap().size.y
                                <= 16.0 * factor + 1.1
                        );
                        assert!(bounds(&app, name).width() <= panel_bounds.width());
                    }
                    if kind == DialogKind::Folder {
                        let status = entity::<MapFolderDialogStatusText>(&mut app);
                        assert!(
                            app.world()
                                .get::<Text>(status)
                                .unwrap()
                                .ends_with("完整诊断结尾")
                        );
                        assert!(
                            app.world()
                                .get::<ComputedNode>(feedback)
                                .unwrap()
                                .content_size()
                                .y
                                > feedback_bounds.height()
                        );
                    }
                }
            }
        }
    }
}
