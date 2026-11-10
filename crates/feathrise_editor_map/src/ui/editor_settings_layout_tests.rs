//! 设置弹窗的正式中文字体布局回归；缩放、DPI与窗口变化不滚走固定操作。

use super::components::*;
use super::*;
use crate::components::{
    MapEditorShortcutBindingText, MapEditorShortcutNoticeText, MapEditorShortcutSettingsBody,
};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::text::TextLayoutInfo;
use bevy::window::PrimaryWindow;
use feathrise_editor_core::EditorKeyBinding;
use feathrise_editor_core::EditorShortcutAction;

#[derive(Resource)]
struct TestFont(Handle<Font>);

fn layout_app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
            watch_for_changes_override: Some(false),
            ..default()
        },
        bevy::scene::ScenePlugin,
        bevy::input::InputPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        bevy::ui::UiPlugin,
        bevy::ui_widgets::ScrollbarPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<bevy::image::TextureAtlasLayout>()
    .insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .insert_resource(MapEditorSettingsState {
        open: true,
        ..default()
    })
    .init_resource::<EditorSettings>()
    .add_systems(Startup, (|| settings_panel(false)).spawn())
    .add_systems(
        Update,
        (
            scroll_settings_body,
            sync_settings_panel,
            crate::ui::shortcut_settings::scroll_shortcut_settings,
            crate::ui::shortcut_settings::sync_shortcut_settings_panel,
        )
            .chain(),
    );
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

fn entity<Marker: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Marker>>()
        .single(world)
        .unwrap()
}

fn bounds(app: &App, entity: Entity) -> Rect {
    Rect::from_center_size(
        app.world()
            .get::<UiGlobalTransform>(entity)
            .unwrap()
            .translation,
        app.world().get::<ComputedNode>(entity).unwrap().size(),
    )
}

#[test]
fn settings_fit_48_chinese_window_scale_dpi_combinations_and_keep_fixed_actions() {
    let (mut app, camera) = layout_app();
    for width in [320, 430, 640, 1280] {
        for height in [360, 720] {
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
                            (height as f32 * dpi) as u32,
                        ),
                        scale_factor: dpi,
                    });
                    // 最长的真实偏好名称也必须换行，不靠截短或假字体通过窄窗验收。
                    let mut settings = app.world_mut().resource_mut::<EditorSettings>();
                    settings.editor_ui_scale = scale;
                    settings.editor_color_theme = EditorColorTheme::GraphiteBlue;

                    settle(&mut app);
                    let factor = scale * dpi;
                    let frame = entity::<SettingsFrame>(&mut app);
                    let header = entity::<SettingsHeader>(&mut app);
                    let body = entity::<SettingsBody>(&mut app);
                    let footer = entity::<SettingsFooter>(&mut app);
                    let frame_bounds = bounds(&app, frame);
                    let body_bounds = bounds(&app, body);
                    let footer_bounds = bounds(&app, footer);
                    assert!(
                        frame_bounds.min.x >= 8.0 * factor - 1.1
                            && frame_bounds.max.x <= width as f32 * dpi - 8.0 * factor + 1.1,
                        "horizontal {width}/{height}/{scale}/{dpi}: {frame_bounds:?}"
                    );
                    assert!(
                        frame_bounds.min.y >= 8.0 * factor - 1.1
                            && frame_bounds.max.y <= height as f32 * dpi - 8.0 * factor + 1.1,
                        "vertical {width}/{height}/{scale}/{dpi}: {frame_bounds:?}"
                    );
                    assert!(body_bounds.height() > 0.0);
                    assert!(bounds(&app, header).max.y <= body_bounds.min.y + 1.1);
                    assert!(body_bounds.max.y <= footer_bounds.min.y + 1.1);
                    assert!(footer_bounds.max.y <= frame_bounds.max.y - factor + 1.1);
                    let percentage = entity::<MapEditorSettingsScaleText>(&mut app);
                    let percentage_row = app.world().get::<ChildOf>(percentage).unwrap().parent();
                    assert!(
                        (bounds(&app, percentage).center().y
                            - bounds(&app, percentage_row).center().y)
                            .abs()
                            <= 1.1
                    );
                    let world = app.world_mut();
                    let buttons = world
                        .query::<(Entity, &MapEditorSettingsButton, &Children)>()
                        .iter(world)
                        .map(|(entity, _, children)| (entity, children.to_vec()))
                        .collect::<Vec<_>>();
                    for (button, children) in buttons {
                        let button_bounds = bounds(&app, button);
                        if button_bounds.size().min_element() <= 0.0 {
                            continue;
                        }
                        assert!((button_bounds.height() - 28.0 * factor).abs() <= 1.1);
                        assert!(
                            button_bounds.min.x >= frame_bounds.min.x
                                && button_bounds.max.x <= frame_bounds.max.x + 1.1
                        );
                        for child in children {
                            let text_bounds = bounds(&app, child);
                            assert!(
                                (text_bounds.center().y - button_bounds.center().y).abs() <= 1.1
                            );
                            assert!(
                                (text_bounds.center().x - button_bounds.center().x).abs() <= 1.1
                            );
                            assert!(
                                text_bounds.width() <= button_bounds.width() - 2.0 * factor + 1.1
                            );
                            assert!(app.world().get::<TextLayoutInfo>(child).unwrap().size.y > 0.0);
                        }
                    }
                    app.world_mut().get_mut::<ScrollPosition>(body).unwrap().y = 100_000.0;
                    settle(&mut app);
                    assert_eq!(bounds(&app, footer), footer_bounds);
                    let computed = app.world().get::<ComputedNode>(body).unwrap();
                    let maximum = ((computed.content_size().y - computed.size().y)
                        * computed.inverse_scale_factor())
                    .max(0.0);
                    assert!(
                        (app.world().get::<ScrollPosition>(body).unwrap().y - maximum).abs() < 0.1
                    );
                    if height == 360 && scale == 1.5 {
                        assert!(maximum > 0.0);
                    }
                }
            }
        }
    }
}

#[test]
fn settings_scene_keeps_original_actions_and_six_sections_without_rebuilding() {
    let (mut app, _) = layout_app();
    settle(&mut app);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<SettingsGroup>>()
            .iter(world)
            .count(),
        6 // 游戏视角/操作分组已归游戏设置，编辑器只保留六组作者偏好。
    );
    let actions = world
        .query::<&MapEditorSettingsButton>()
        .iter(world)
        .map(|button| button.0)
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 46); // 移除两个玩法视角动作，其余作者设置、帮助和13组改键/清除入口保留。
    assert!(!world.query::<&Text>().iter(world).any(|text| {
        matches!(text.0.as_str(), "操作方式" | "第三人称" | "固定俯视")
    })); // 游戏相机配置不能重新混入作者设置。
    assert!(world.query::<&feathrise_editor_ui::context_help::ContextHelpButton>().iter(world).count() >= 7);
    // 新增视口显示分组，吸附与布局两条分支的既有恢复动作继续保留。
    for action in [
        EditorSettingsAction::ResetSnapping,
        EditorSettingsAction::ResetLayout,
        EditorSettingsAction::ToggleTerrainBackfaces,
        EditorSettingsAction::SetAutoScale,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::AddObjectMenu),
        EditorSettingsAction::UnbindShortcut(EditorShortcutAction::AddObjectMenu),
    ] {
        assert_eq!(
            actions.iter().filter(|candidate| **candidate == action).count(),
            1
        );
    }
    assert_eq!(
        actions
            .iter()
            .filter(|action| matches!(action, EditorSettingsAction::RebindShortcut(_)))
            .count(),
        13
    );
    assert_eq!(
        actions
            .iter()
            .filter(|action| matches!(action, EditorSettingsAction::UnbindShortcut(_)))
            .count(),
        13
    );
    assert_eq!(
        actions
            .iter()
            .filter(|action| **action == EditorSettingsAction::Close)
            .count(),
        3
    );
    assert_eq!(
        actions
            .iter()
            .filter(|action| **action == EditorSettingsAction::SetScale(100))
            .count(),
        2
    );
    let entities = world.query::<Entity>().iter(world).collect::<Vec<_>>();
    let body = entity::<SettingsBody>(&mut app);
    let footer = entity::<SettingsFooter>(&mut app);
    assert_eq!(
        app.world().get::<ChildOf>(footer).unwrap().parent(),
        entity::<SettingsFrame>(&mut app)
    );
    assert_eq!(
        app.world().get::<Node>(body).unwrap().overflow,
        Overflow::scroll_y()
    );
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_color_theme = EditorColorTheme::WarmCopper;
    settle(&mut app);
    assert!(
        entities
            .iter()
            .all(|entity| app.world().get_entity(*entity).is_ok())
    );
    let theme = entity::<SettingsThemeText>(&mut app);
    assert_eq!(
        app.world().get::<Text>(theme).unwrap().as_str(),
        "当前：暖墨铜金"
    );
}

#[test]
fn shortcut_page_fits_48_chinese_window_scale_dpi_combinations_with_full_chords_and_feedback() {
    let (mut app, camera) = layout_app();
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .shortcuts = true;
    let mut input = ButtonInput::<KeyCode>::default();
    for key in [KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::AltLeft] {
        input.press(key);
    }
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_keybindings
        .assign(
            EditorShortcutAction::FocusSelected,
            Some(EditorKeyBinding::from_input(KeyCode::KeyG, &input)),
        )
        .unwrap();
    {
        let mut state = app.world_mut().resource_mut::<MapEditorSettingsState>();
        state.shortcut_notice = "Ctrl+Shift+Alt+G 已用于“聚焦所选”，请换键或先清除原绑定".into();
        state.shortcut_notice_kind = EditorShortcutNoticeKind::Warning;
    }
    for width in [320, 430, 640, 1280] {
        for height in [360, 720] {
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
                            (height as f32 * dpi) as u32,
                        ),
                        scale_factor: dpi,
                    });
                    settle(&mut app);
                    let factor = scale * dpi;
                    let frame = entity::<SettingsFrame>(&mut app);
                    let tabs = entity::<SettingsTabs>(&mut app);
                    let body = entity::<MapEditorShortcutSettingsBody>(&mut app);
                    *app.world_mut().get_mut::<ScrollPosition>(body).unwrap() =
                        ScrollPosition::default();
                    settle(&mut app);
                    let notice = entity::<ShortcutNoticeCard>(&mut app);
                    let footer = entity::<SettingsFooter>(&mut app);
                    let frame_bounds = bounds(&app, frame);
                    let body_bounds = bounds(&app, body);
                    let notice_bounds = bounds(&app, notice);
                    let footer_bounds = bounds(&app, footer);
                    assert!(
                        frame_bounds.min.x >= 8.0 * factor - 1.1
                            && frame_bounds.max.x <= width as f32 * dpi - 8.0 * factor + 1.1
                    );
                    assert!(
                        frame_bounds.min.y >= 8.0 * factor - 1.1
                            && frame_bounds.max.y <= height as f32 * dpi - 8.0 * factor + 1.1
                    );
                    assert!(bounds(&app, tabs).max.y <= body_bounds.min.y + 1.1);
                    assert!(
                        body_bounds.height() >= 28.0 * factor - 1.1,
                        "body {width}/{height}/{scale}/{dpi}: {body_bounds:?}"
                    );
                    assert!(notice_bounds.min.y >= body_bounds.min.y - 1.1);
                    assert!(notice_bounds.max.y <= body_bounds.max.y + 1.1);
                    assert!(body_bounds.max.y <= footer_bounds.min.y + 1.1);
                    assert_eq!(app.world().get::<ChildOf>(notice).unwrap().parent(), body);
                    assert!(footer_bounds.max.y <= frame_bounds.max.y - factor + 1.1);
                    let world = app.world_mut();
                    let rows = world
                        .query::<(Entity, &ShortcutSettingsRow, &Children)>()
                        .iter(world)
                        .map(|(entity, action, children)| (entity, action.0, children.to_vec()))
                        .collect::<Vec<_>>();
                    assert_eq!(rows.len(), 13);
                    assert_eq!(rows.iter().filter(|(_, action, _)| *action == EditorShortcutAction::AddObjectMenu).count(), 1);
                    for (row, action, children) in rows {
                        let row_bounds = bounds(&app, row);
                        // 内容在三列内自适应，不能仅凭未越界放过动作/按键/清除分成三行。
                        let columns = children
                            .iter()
                            .map(|&child| bounds(&app, child))
                            .collect::<Vec<_>>();
                        assert_eq!(columns.len(), 3);
                        assert!(columns[0].max.x <= columns[1].min.x + 1.1);
                        assert!(columns[1].max.x <= columns[2].min.x + 1.1);
                        // 单行必须容纳完整真实字形，不能靠关闭折行掩盖名称裁切。
                        let name = app.world().get::<Children>(children[0]).unwrap()[0];
                        let name_size = app.world().get::<TextLayoutInfo>(name).unwrap().size;
                        assert!(name_size.x > 0.0 && name_size.x <= columns[0].width() + 1.1);
                        for column in &columns {
                            assert!((column.center().y - row_bounds.center().y).abs() <= 1.1);
                        }
                        assert!(
                            row_bounds.min.x >= body_bounds.min.x - 1.1
                                && row_bounds.max.x <= body_bounds.max.x + 1.1
                        );
                        for child in children {
                            let child_bounds = bounds(&app, child);
                            assert!(
                                child_bounds.min.x >= row_bounds.min.x - 1.1
                                    && child_bounds.max.x <= row_bounds.max.x + 1.1,
                                "row {action:?} {width}/{height}/{scale}/{dpi}: {row_bounds:?} / {child_bounds:?}"
                            );
                            assert!(
                                child_bounds.min.y >= row_bounds.min.y - 1.1
                                    && child_bounds.max.y <= row_bounds.max.y + 1.1
                            );
                            if app.world().get::<MapEditorSettingsButton>(child).is_some() {
                                assert!(child_bounds.height() >= 28.0 * factor - 1.1);
                                for text in app.world().get::<Children>(child).unwrap().iter() {
                                    let text_bounds = bounds(&app, text);
                                    assert!(text_bounds.width() > 0.0);
                                    assert!(
                                        text_bounds.min.x >= child_bounds.min.x - 1.1
                                            && text_bounds.max.x <= child_bounds.max.x + 1.1
                                    );
                                    assert!(
                                        text_bounds.min.y >= child_bounds.min.y - 1.1
                                            && text_bounds.max.y <= child_bounds.max.y + 1.1
                                    );
                                    assert!(
                                        (text_bounds.center().y - child_bounds.center().y).abs()
                                            <= 1.1
                                    );
                                    assert!(
                                        app.world().get::<TextLayoutInfo>(text).unwrap().size.y
                                            > 0.0
                                    );
                                }
                            }
                        }
                    }
                    let world = app.world_mut();
                    let binding = world
                        .query::<(&MapEditorShortcutBindingText, &Text)>()
                        .iter(world)
                        .find(|(action, _)| action.0 == EditorShortcutAction::FocusSelected)
                        .unwrap()
                        .1;
                    assert_eq!(binding.as_str(), "Ctrl+Shift+Alt+G");
                    app.world_mut().get_mut::<ScrollPosition>(body).unwrap().y = 100_000.0;
                    settle(&mut app);
                    assert_eq!(bounds(&app, footer), footer_bounds);
                    assert!(bounds(&app, notice).max.y < body_bounds.min.y);
                    let computed = app.world().get::<ComputedNode>(body).unwrap();
                    let maximum = ((computed.content_size().y - computed.size().y)
                        * computed.inverse_scale_factor())
                    .max(0.0);
                    assert!(
                        (app.world().get::<ScrollPosition>(body).unwrap().y - maximum).abs() < 0.1
                    );
                }
            }
        }
    }
}

#[test]
fn shortcut_compact_page_keeps_two_complete_rows_visible_in_60_window_scale_dpi_combinations() {
    let (mut app, camera) = layout_app();
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .shortcuts = true;
    settle(&mut app);
    let body = entity::<MapEditorShortcutSettingsBody>(&mut app);
    let track = entity::<ShortcutScrollbarTrack>(&mut app);
    let thumb = entity::<ShortcutScrollbarThumb>(&mut app);
    let footer = entity::<SettingsFooter>(&mut app);
    let notice = entity::<ShortcutNoticeCard>(&mut app);
    let last_row = {
        let world = app.world_mut();
        world.query::<(Entity, &ShortcutSettingsRow)>().iter(world)
            .find(|(_, row)| row.0 == EditorShortcutAction::ScaleSelected).unwrap().0
    };
    let first_rows = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ShortcutSettingsRow)>()
            .iter(world)
            .filter(|(_, action)| {
                matches!(
                    action.0,
                    EditorShortcutAction::FocusSelected | EditorShortcutAction::FrameMap
                )
            })
            .map(|(entity, _)| entity)
            .collect::<Vec<_>>()
    };
    assert_eq!(first_rows.len(), 2);
    assert_eq!(
        app.world()
            .get::<bevy::ui_widgets::Scrollbar>(track)
            .unwrap()
            .target,
        body
    );
    assert!(app.world().get::<Node>(thumb).is_none());
    for width in [320, 384, 430, 640, 1280] {
        for height in [360, 720] {
            for scale in [1.0, 1.25, 1.5] {
                for dpi in [1.0, 2.0] {
                    app.world_mut().resource_mut::<UiScale>().0 = scale;
                    app.world_mut()
                        .get_mut::<Camera>(camera)
                        .unwrap()
                        .computed
                        .target_info = Some(RenderTargetInfo {
                        physical_size: UVec2::new(
                            (width as f32 * dpi) as u32,
                            (height as f32 * dpi) as u32,
                        ),
                        scale_factor: dpi,
                    });
                    *app.world_mut().get_mut::<ScrollPosition>(body).unwrap() =
                        ScrollPosition::default();
                    settle(&mut app);
                    let viewport = bounds(&app, body);
                    let footer_bounds = bounds(&app, footer);
                    assert_eq!(
                        app.world().get::<Node>(notice).unwrap().display,
                        Display::None
                    );
                    // 必须同时看到动作名称、对应按键及清除，而非只保留一条名义上的空正文。
                    for &row in &first_rows {
                        let rect = bounds(&app, row);
                        assert!(
                            viewport.contains(rect.min) && viewport.contains(rect.max),
                            "首屏完整行 {width}/{height}/{scale}/{dpi}: {viewport:?} / {rect:?}"
                        );
                    }
                    let track_bounds = bounds(&app, track);
                    let thumb_bounds = bounds(&app, thumb);
                    assert!((track_bounds.min.y - viewport.min.y).abs() < 1.1);
                    assert!((track_bounds.max.y - viewport.max.y).abs() < 1.1);
                    assert!(thumb_bounds.height() > 0.0 && thumb_bounds.width() > 0.0);
                    assert!((thumb_bounds.min.y - track_bounds.min.y).abs() < 1.1);
                    app.world_mut().get_mut::<ScrollPosition>(body).unwrap().y = 100_000.0;
                    settle(&mut app);
                    assert_eq!(bounds(&app, footer), footer_bounds);
                    let help_bounds = bounds(&app, last_row);
                    assert!(
                        viewport.contains(help_bounds.min) && viewport.contains(help_bounds.max)
                    );
                    assert!((bounds(&app, thumb).max.y - track_bounds.max.y).abs() < 1.1);
                }
            }
        }
    }
}

#[test]
fn shortcut_native_scrollbar_pages_drags_clamps_and_cancels_on_page_switch() {
    use bevy::picking::backend::HitData;
    use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart, Pointer, PointerPress};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};
    use bevy::ui_widgets::ScrollbarDragState;
    let (mut app, camera) = layout_app();
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .shortcuts = true;
    settle(&mut app);
    let body = entity::<MapEditorShortcutSettingsBody>(&mut app);
    let track = entity::<ShortcutScrollbarTrack>(&mut app);
    let thumb = entity::<ShortcutScrollbarThumb>(&mut app);
    let window = entity::<PrimaryWindow>(&mut app);
    let settings_tick = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    for (scale, dpi) in [(1.0, 1.0), (1.25, 1.0), (1.5, 1.0), (1.5, 2.0)] {
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        app.world_mut()
            .get_mut::<Camera>(camera)
            .unwrap()
            .computed
            .target_info = Some(RenderTargetInfo {
            physical_size: UVec2::new((384.0 * dpi) as u32, (360.0 * dpi) as u32),
            scale_factor: dpi,
        });
        *app.world_mut().get_mut::<ScrollPosition>(body).unwrap() = ScrollPosition::default();
        settle(&mut app);
        let computed = app.world().get::<ComputedNode>(body).unwrap();
        let viewport = computed.size().y * computed.inverse_scale_factor();
        let content = computed.content_size().y * computed.inverse_scale_factor();
        let maximum = (content - viewport).max(0.0);
        let location = Location {
            target: bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window))
                .normalize(Some(window))
                .unwrap(),
            position: Vec2::new(
                bounds(&app, track).center().x,
                bounds(&app, track).max.y - 1.0,
            ) / dpi,
        };
        let hit = HitData::new(camera, 0.0, None, None);
        app.world_mut().trigger(PointerPress {
            entity: track,
            pointer: Pointer::new(PointerId::Mouse, location.clone()),
            button: PointerButton::Primary,
                hit: hit.clone(),
                count: 1,
        });
        assert!(
            (app.world().get::<ScrollPosition>(body).unwrap().y - viewport.min(maximum)).abs()
                < 1.1
        );
        app.world_mut().get_mut::<ScrollPosition>(body).unwrap().y = 0.0;
        app.world_mut().trigger(PointerDragStart {
            entity: thumb,
            pointer: Pointer::new(PointerId::Mouse, location.clone()),
            button: PointerButton::Primary,
                hit,
        });
        assert!(
            app.world()
                .get::<ScrollbarDragState>(thumb)
                .unwrap()
                .dragging
        );
        let distance = Vec2::new(0.0, 10.0 * scale);
        app.world_mut().trigger(PointerDrag {
            entity: thumb,
            pointer: Pointer::new(PointerId::Mouse, location.clone()),
            button: PointerButton::Primary,
                distance,
                delta: distance,
        });
        assert!(
            (app.world().get::<ScrollPosition>(body).unwrap().y
                - (10.0 * content / viewport).min(maximum))
            .abs()
                < 1.1
        );
        app.world_mut().trigger(PointerDrag {
            entity: thumb,
            pointer: Pointer::new(PointerId::Mouse, location.clone()),
            button: PointerButton::Primary,
                distance: Vec2::splat(100_000.0),
                delta: Vec2::splat(100_000.0),
        });
        assert!((app.world().get::<ScrollPosition>(body).unwrap().y - maximum).abs() < 1.1);
        app.world_mut().trigger(PointerDragEnd {
            entity: thumb,
            pointer: Pointer::new(PointerId::Mouse, location),
            button: PointerButton::Primary,
                distance,
        });
        assert!(
            !app.world()
                .get::<ScrollbarDragState>(thumb)
                .unwrap()
                .dragging
        );
    }
    app.world_mut()
        .get_mut::<ScrollbarDragState>(thumb)
        .unwrap()
        .dragging = true;
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .shortcuts = false;
    settle(&mut app);
    assert!(
        !app.world()
            .get::<ScrollbarDragState>(thumb)
            .unwrap()
            .dragging
    );
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 0.0);
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        settings_tick
    );
    assert!(!app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn settings_page_switch_keeps_header_tabs_footer_and_content_origin_fixed() {
    let (mut app, camera) = layout_app();
    settle(&mut app);
    let general = entity::<SettingsBody>(&mut app);
    let shortcuts_page = entity::<MapEditorShortcutSettingsPanel>(&mut app);
    let mut fixed = vec![
        entity::<SettingsFrame>(&mut app),
        entity::<SettingsHeader>(&mut app),
        entity::<SettingsTabs>(&mut app),
        entity::<SettingsFooter>(&mut app),
    ];
    let (general_reset, shortcuts_reset) = {
        let world = app.world_mut();
        fixed.extend(
            world
                .query::<(
                    Entity,
                    &MapEditorSettingsButton,
                    Option<&SettingsPageButton>,
                )>()
                .iter(world)
                .filter(|(_, button, page)| {
                    page.is_some() || button.0 == EditorSettingsAction::Close
                })
                .map(|(entity, _, _)| entity),
        );
        let mut buttons = world
            .query_filtered::<(Entity, &MapEditorSettingsButton), With<SettingsUtilityButton>>();
        (
            buttons
                .iter(world)
                .find(|(_, button)| button.0 == EditorSettingsAction::SetScale(100))
                .unwrap()
                .0,
            buttons
                .iter(world)
                .find(|(_, button)| button.0 == EditorSettingsAction::ResetShortcuts)
                .unwrap()
                .0,
        )
    };
    // 312px/150%恰好压到旧底栏两种重置按钮的折行分界，不能只测宽窗口。
    for width in [312, 320, 430, 640, 1280] {
        for height in [360, 720] {
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
                            (height as f32 * dpi) as u32,
                        ),
                        scale_factor: dpi,
                    });
                    app.world_mut()
                        .resource_mut::<MapEditorSettingsState>()
                        .shortcuts = false;
                    settle(&mut app);
                    let baseline = fixed
                        .iter()
                        .map(|&entity| bounds(&app, entity))
                        .chain([bounds(&app, general_reset)])
                        .collect::<Vec<_>>();
                    let content_origin = bounds(&app, general).min;
                    for shortcuts in [true, false, true, false] {
                        {
                            let mut state =
                                app.world_mut().resource_mut::<MapEditorSettingsState>();
                            state.shortcuts = shortcuts;
                            state.shortcut_notice = if shortcuts {
                                "Ctrl+Shift+Alt+G 已用于“聚焦所选”，请换键或先清除原绑定".into()
                            } else {
                                String::new()
                            };
                            state.shortcut_notice_kind = EditorShortcutNoticeKind::Warning;
                        }
                        // 首个布局帧就必须稳定，不能等待后续帧把跳动修正回去。
                        app.update();
                        let reset = if shortcuts {
                            shortcuts_reset
                        } else {
                            general_reset
                        };
                        let current = fixed
                            .iter()
                            .map(|&entity| bounds(&app, entity))
                            .chain([bounds(&app, reset)]);
                        for (index, (expected, actual)) in baseline.iter().zip(current).enumerate()
                        {
                            assert!(
                                (actual.min - expected.min).abs().max_element() < 0.1
                                    && (actual.max - expected.max).abs().max_element() < 0.1,
                                "切页移动公共控件{index}：{width}/{height}/{scale}/{dpi}/{shortcuts}：{expected:?} -> {actual:?}"
                            );
                        }
                        let content = if shortcuts { shortcuts_page } else { general };
                        assert!(
                            (bounds(&app, content).min - content_origin)
                                .abs()
                                .max_element()
                                < 0.1,
                            "切页移动正文起点：{width}/{height}/{scale}/{dpi}/{shortcuts}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn settings_pages_share_one_frame_backdrop_footer_and_keep_entities_when_switched() {
    let (mut app, _) = layout_app();
    settle(&mut app);
    let root = entity::<MapEditorSettingsPanel>(&mut app);
    let frame = entity::<SettingsFrame>(&mut app);
    let page = entity::<MapEditorShortcutSettingsPanel>(&mut app);
    let general = entity::<SettingsBody>(&mut app);
    let footer = entity::<SettingsFooter>(&mut app);
    let tabs = entity::<SettingsTabs>(&mut app);
    assert_eq!(app.world().get::<ChildOf>(page).unwrap().parent(), frame);
    assert_eq!(app.world().get::<ChildOf>(tabs).unwrap().parent(), frame);
    assert_eq!(app.world().get::<ChildOf>(footer).unwrap().parent(), frame);
    assert!(app.world().get::<GlobalZIndex>(page).is_none());
    let entities = {
        let world = app.world_mut();
        world.query::<Entity>().iter(world).collect::<Vec<_>>()
    };
    for shortcuts in [true, false, true, false] {
        app.world_mut()
            .resource_mut::<MapEditorSettingsState>()
            .shortcuts = shortcuts;
        settle(&mut app);
        assert!(
            entities
                .iter()
                .all(|entity| app.world().get_entity(*entity).is_ok())
        );
        assert_eq!(
            app.world().get::<Node>(root).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(general).unwrap().display,
            if shortcuts {
                Display::None
            } else {
                Display::Flex
            }
        );
        assert_eq!(
            app.world().get::<Node>(page).unwrap().display,
            if shortcuts {
                Display::Flex
            } else {
                Display::None
            }
        );
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<ShortcutSettingsGroup>>()
                .iter(world)
                .count(),
            4
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<MapEditorShortcutNoticeText>>()
                .iter(world)
                .count(),
            1
        );
        let buttons = world
            .query::<(Entity, &MapEditorSettingsButton)>()
            .iter(world)
            .filter(|(_, button)| {
                matches!(
                    button.0,
                    EditorSettingsAction::ResetShortcuts | EditorSettingsAction::SetScale(100)
                )
            })
            .map(|(entity, button)| (entity, button.0))
            .collect::<Vec<_>>();
        let visible = buttons
            .into_iter()
            .filter(|(entity, _)| {
                app.world()
                    .get::<ComputedNode>(*entity)
                    .unwrap()
                    .size()
                    .min_element()
                    > 0.0
            })
            .map(|(_, action)| action)
            .collect::<Vec<_>>();
        assert_eq!(
            visible
                .iter()
                .filter(|action| **action == EditorSettingsAction::ResetShortcuts)
                .count(),
            usize::from(shortcuts)
        );
        assert_eq!(
            visible
                .iter()
                .filter(|action| **action == EditorSettingsAction::SetScale(100))
                .count(),
            if shortcuts { 0 } else { 2 }
        );
    }
}
