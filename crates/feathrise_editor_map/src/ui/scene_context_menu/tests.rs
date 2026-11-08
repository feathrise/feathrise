//! 正式右键边沿、中文名称、目标换代和鼠标释放门回归，不访问系统剪贴板或地图文件。

use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::SceneContextMenuState, systems, types::*};
use crate::{
    authoring::{EditorAuthoringSession, test_heightfield_draft},
    components::MapEditorRoot,
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState},
    ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
};
use bevy::ui::RelativeCursorPosition;
use bevy::{input_focus::InputFocus, math::DVec2, prelude::*, window::PrimaryWindow};

struct Fixture {
    app: App,
    row: Entity,
    window: Entity,
    input: Entity,
    popup: Entity,
    backdrop: Entity,
}

fn target(session: &EditorAuthoringSession) -> SceneObjectTarget {
    let draft = session.draft.as_ref().unwrap();
    SceneObjectTarget {
        map: draft.map_id.clone(),
        revision: session.revision,
        key: SceneObjectKey::Placement {
            index: 0,
            id: draft.placements.placements[0].id.clone(),
            mesh: None,
        },
        name: String::new(),
        hidden: false,
        locked: false,
        copyable: true,
    }
}

fn fixture() -> Fixture {
    let mut draft = test_heightfield_draft(vec![0; 121].into());
    draft
        .placements
        .placements
        .push(crate::viewport::placement_move::interaction_tests::record());
    let map = draft.map_id.clone();
    let session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    };
    let object = target(&session);
    let mut app = App::new();
    app.add_plugins(feathrise_editor_ui::EditorButtonInputPlugin)
        .insert_resource(session)
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map),
            ..default()
        })
        .init_resource::<SceneContextMenuState>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<crate::ui::text_input::EditorTextInputGuard>()
        .add_systems(
            Update,
            (
                systems::control_menu,
                update_map_editor_numeric_scrub_mouse_guard,
                systems::dispatch,
                super::focus::select_target,
                super::scene::sync,
            )
                .chain(),
        );
    let mut window = Window {
        focused: true,
        ..default()
    };
    window.set_physical_cursor_position(Some(DVec2::new(1275.0, 715.0)));
    let window = app.world_mut().spawn((PrimaryWindow, window)).id();
    app.world_mut().spawn((
        MapEditorRoot,
        ComputedNode {
            size: Vec2::new(1280.0, 720.0),
            inverse_scale_factor: 1.0,
            ..default()
        },
        UiGlobalTransform::from_xy(640.0, 360.0),
    ));
    let row = app
        .world_mut()
        .spawn((
            SceneObjectRow(Some(object)),
            Node::default(),
            Interaction::Hovered,
        ))
        .id();
    let input = app
        .world_mut()
        .spawn((
            RenameInput,
            crate::ui::text_input::single_line_input("", 128),
        ))
        .id();
    app.world_mut().spawn((RenameField, Interaction::None));
    // 无窗口布局不产生命中几何；显式给出非零尺寸和可见性，让共享捕获执行真实资格检查。
    for action in SceneMenuAction::ALL {
        let child = app.world_mut().spawn(Text::new("")).id();
        app.world_mut()
            .spawn((
                InteractionButton,
                MenuButton(action),
                Interaction::None,
                Node::default(),
                ComputedNode {
                    size: Vec2::new(184.0, 28.0),
                    ..default()
                },
                InheritedVisibility::VISIBLE,
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
                BackgroundColor::default(),
                BorderColor::default(),
            ))
            .add_child(child);
    }
    app.world_mut().spawn((
        InteractionButton,
        RenameConfirm,
        Interaction::None,
        ComputedNode {
            size: Vec2::new(80.0, 28.0),
            ..default()
        },
        InheritedVisibility::VISIBLE,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
        BackgroundColor::default(),
        BorderColor::default(),
    ));
    app.world_mut().spawn((
        InteractionButton,
        RenameCancel,
        Interaction::None,
        ComputedNode {
            size: Vec2::new(80.0, 28.0),
            ..default()
        },
        InheritedVisibility::VISIBLE,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
        BackgroundColor::default(),
        BorderColor::default(),
    ));
    app.world_mut().spawn((RenamePanel, Node::default()));
    let popup = app.world_mut().spawn((Popup, Node::default())).id();
    let backdrop = app.world_mut().spawn((Backdrop, Node::default())).id();
    let mut fixture = Fixture {
        app,
        row,
        window,
        input,
        popup,
        backdrop,
    };
    step(&mut fixture.app);
    fixture
}

fn step(app: &mut App) {
    // 无渲染夹具手动提供官方命中的释放终态，实际捕获与单帧点击仍走生产插件。
    if app.world().resource::<ButtonInput<MouseButton>>().just_released(MouseButton::Left) {
        let world = app.world_mut();
        for (mut interaction, cursor) in world
            .query_filtered::<(&mut Interaction, &RelativeCursorPosition), With<InteractionButton>>()
            .iter_mut(world)
        {
            if *interaction == Interaction::Pressed {
                *interaction = if cursor.cursor_over() {
                    Interaction::Hovered
                } else {
                    Interaction::None
                };
            }
        }
    }
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

fn open(fixture: &mut Fixture) {
    fixture
        .app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    step(&mut fixture.app);
    assert!(
        fixture
            .app
            .world()
            .resource::<SceneContextMenuState>()
            .target
            .is_some()
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    step(&mut fixture.app);
}

fn choose(fixture: &mut Fixture, action: SceneMenuAction) {
    let world = fixture.app.world_mut();
    for (button, mut interaction) in world
        .query::<(&MenuButton, &mut Interaction)>()
        .iter_mut(world)
    {
        *interaction = if button.0 == action {
            Interaction::Pressed
        } else {
            Interaction::None
        };
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut fixture.app);
}

#[test]
fn scene_menu_freezes_right_clicked_object_and_waits_for_the_whole_mouse_release() {
    let mut test = fixture();
    open(&mut test);
    let target = test
        .app
        .world()
        .resource::<SceneContextMenuState>()
        .target
        .as_ref()
        .unwrap()
        .object
        .clone();
    choose(&mut test, SceneMenuAction::Copy);
    assert!(
        test.app.world().resource::<SceneContextMenuState>().target.is_some()
    );
    assert!(
        test.app.world().resource::<SceneContextMenuState>().queued.is_none()
    );
    assert!(
        test.app
            .world()
            .resource::<SceneContextMenuState>()
            .ready
            .is_none()
    );
    assert!(
        test.app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut test.app);
    assert!(
        test.app
            .world()
            .resource::<SceneContextMenuState>()
            .ready
            .is_none()
    );
    step(&mut test.app);
    let request = test
        .app
        .world()
        .resource::<SceneContextMenuState>()
        .ready
        .as_ref()
        .unwrap();
    assert_eq!(request.target, target);
    assert_eq!(request.action, SceneMenuAction::Copy);
    assert!(
        test.app
            .world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    step(&mut test.app);
    assert!(
        test.app
            .world()
            .resource::<SceneContextMenuState>()
            .ready
            .is_none()
    );
}

#[test]
fn scene_menu_rejects_rebound_rows_revision_changes_hidden_rows_and_unfocused_windows() {
    for invalid in 0..5 {
        let mut test = fixture();
        open(&mut test);
        match invalid {
            0 => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .revision += 1
            }
            1 => {
                test.app
                    .world_mut()
                    .get_mut::<SceneObjectRow>(test.row)
                    .unwrap()
                    .0 = None
            }
            2 => {
                test.app
                    .world_mut()
                    .get_mut::<Node>(test.row)
                    .unwrap()
                    .display = Display::None
            }
            3 => {
                test.app
                    .world_mut()
                    .get_mut::<Window>(test.window)
                    .unwrap()
                    .focused = false
            }
            _ => {
                test.app
                    .world_mut()
                    .resource_mut::<MapEditorState>()
                    .new_map_dialog_open = true
            }
        }
        choose(&mut test, SceneMenuAction::Delete);
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut test.app);
        step(&mut test.app);
        let menu = test.app.world().resource::<SceneContextMenuState>();
        assert!(menu.target.is_none() && menu.queued.is_none() && menu.ready.is_none());
        assert!(
            test.app
                .world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
}

#[test]
fn scene_menu_queued_command_is_cancelled_if_map_or_slot_changes_before_release() {
    for invalid in 0..2 {
        let mut test = fixture();
        open(&mut test);
        choose(&mut test, SceneMenuAction::Delete);
        if invalid == 0 {
            test.app
                .world_mut()
                .resource_mut::<MapEditorState>()
                .selected_map_id = None;
        } else {
            test.app
                .world_mut()
                .get_mut::<SceneObjectRow>(test.row)
                .unwrap()
                .0
                .as_mut()
                .unwrap()
                .name = "另一行".into();
        }
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut test.app);
        step(&mut test.app);
        assert!(
            test.app
                .world()
                .resource::<SceneContextMenuState>()
                .ready
                .is_none()
        );
    }
}

#[test]
fn scene_menu_chinese_rename_preserves_ime_enter_and_escape_then_confirms_once() {
    let mut test = fixture();
    open(&mut test);
    choose(&mut test, SceneMenuAction::Rename);
    assert!(!test.app.world().resource::<SceneContextMenuState>().renaming);
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut test.app);
    assert_eq!(
        test.app.world().resource::<InputFocus>().get(),
        Some(test.input)
    );
    *test
        .app
        .world_mut()
        .get_mut::<bevy::text::EditableText>(test.input)
        .unwrap() = crate::ui::text_input::single_line_input("村口 铜灯", 128);
    test.app
        .world_mut()
        .resource_mut::<crate::ui::text_input::EditorTextInputGuard>()
        .block_shortcuts = true;
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    step(&mut test.app);
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    step(&mut test.app);
    assert!(
        test.app
            .world()
            .resource::<SceneContextMenuState>()
            .renaming
    );
    assert!(
        test.app
            .world()
            .resource::<SceneContextMenuState>()
            .queued
            .is_none()
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    test.app
        .world_mut()
        .resource_mut::<crate::ui::text_input::EditorTextInputGuard>()
        .block_shortcuts = false;
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    step(&mut test.app);
    step(&mut test.app);
    let request = test
        .app
        .world()
        .resource::<SceneContextMenuState>()
        .ready
        .as_ref()
        .unwrap();
    assert_eq!(request.action, SceneMenuAction::Rename);
    assert_eq!(request.name, "村口 铜灯");
    assert!(test.app.world().resource::<InputFocus>().get().is_none());
}

#[test]
fn scene_menu_uses_reverse_labels_and_keeps_locked_objects_recoverable() {
    let mut test = fixture();
    {
        let mut row = test
            .app
            .world_mut()
            .get_mut::<SceneObjectRow>(test.row)
            .unwrap();
        let target = row.0.as_mut().unwrap();
        target.hidden = true;
        target.locked = true;
    }
    open(&mut test);
    let world = test.app.world_mut();
    for (button, children, disabled) in world
        .query::<(&MenuButton, &Children, Has<bevy::ui::InteractionDisabled>)>()
        .iter(world)
    {
        let label = &world.get::<Text>(children[0]).unwrap().0;
        match button.0 {
            SceneMenuAction::Visibility => {
                assert_eq!(label, "显示");
                assert!(!disabled);
            }
            SceneMenuAction::Lock => {
                assert_eq!(label, "解锁");
                assert!(!disabled);
            }
            SceneMenuAction::Rename | SceneMenuAction::Copy | SceneMenuAction::Delete => {
                assert!(disabled)
            }
            _ => {}
        }
    }
}

#[test]
fn scene_menu_escape_dismisses_without_closing_editor_and_anchor_stays_inside_scaled_root() {
    let mut test = fixture();
    let world = test.app.world_mut();
    for mut node in world
        .query_filtered::<&mut ComputedNode, With<MapEditorRoot>>()
        .iter_mut(world)
    {
        node.inverse_scale_factor = 0.5;
    }
    open(&mut test);
    let popup = test.app.world().get::<Node>(test.popup).unwrap();
    assert_eq!((popup.width, popup.height), (px(184), px(176)));
    assert_eq!(popup.left, px(452));
    assert_eq!(popup.top, px(180));
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    step(&mut test.app);
    assert_eq!(
        test.app.world().get::<Node>(test.backdrop).unwrap().display,
        Display::None
    );
    assert!(test.app.world().resource::<MapEditorState>().open);
    assert!(
        !test
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Escape)
    );
}

#[test]
fn scene_menu_types_offer_supported_actions_and_focus_the_clicked_target() {
    use crate::ui::{
        gltf_browser::EditorGltfBrowserState, sub_maps::SubMapEditorState,
        terrain_objects::TerrainObjectsEditorState,
    };
    use crate::viewport::{LayerCandidateState, LayerTarget};
    use feathrise_map_content::{MapId, MapInstanceId, MapInstancePlacement, TerrainId};
    for kind in 0..3 {
        let mut test = fixture();
        test.app
            .init_resource::<EditorGltfBrowserState>()
            .init_resource::<LayerCandidateState>()
            .init_resource::<TerrainObjectsEditorState>()
            .init_resource::<SubMapEditorState>();
        let key = match kind {
            0 => test
                .app
                .world()
                .get::<SceneObjectRow>(test.row)
                .unwrap()
                .0
                .as_ref()
                .unwrap()
                .key
                .clone(),
            1 => SceneObjectKey::Terrain {
                index: 0,
                id: TerrainId::legacy(),
            },
            _ => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .draft
                    .as_mut()
                    .unwrap()
                    .manifest
                    .sub_maps = vec![MapInstancePlacement {
                    id: MapInstanceId("village".into()),
                    source_map_id: MapId("source".into()),
                    label: "村庄引用".into(),
                    transform: default(),
                }]
                .into();
                SceneObjectKey::SubMap {
                    index: 0,
                    id: MapInstanceId("village".into()),
                }
            }
        };
        test.app
            .world_mut()
            .get_mut::<SceneObjectRow>(test.row)
            .unwrap()
            .0
            .as_mut()
            .unwrap()
            .key = key;
        open(&mut test);
        let world = test.app.world_mut();
        assert_eq!(
            world
                .query_filtered::<&Node, With<MenuButton>>()
                .iter(world)
                .filter(|node| node.display == Display::Flex)
                .count(),
            [6, 5, 4][kind]
        );
        choose(&mut test, SceneMenuAction::Focus);
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut test.app);
        step(&mut test.app);
        assert_eq!(
            test.app
                .world()
                .resource::<MapEditorState>()
                .viewport_framing_requested,
            Some(crate::types::EditorViewportFraming::Selected)
        );
        let selected = test
            .app
            .world()
            .resource::<LayerCandidateState>()
            .selected
            .as_ref()
            .unwrap();
        match kind {
            0 => assert_eq!(selected, &LayerTarget::Placement("move-test".into())),
            1 => assert_eq!(
                selected,
                &LayerTarget::TerrainObject(TerrainId::legacy().as_str().to_owned())
            ),
            _ => assert_eq!(selected, &LayerTarget::SubMap("village".into())),
        }
    }
}

#[test]
fn scene_menu_sub_map_chinese_name_respects_existing_utf8_byte_limit_before_confirming() {
    use feathrise_map_content::{MapId, MapInstanceId, MapInstancePlacement};
    for (count, valid) in [(85, true), (86, false)] {
        let mut test = fixture();
        test.app
            .world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft
            .as_mut()
            .unwrap()
            .manifest
            .sub_maps = vec![MapInstancePlacement {
            id: MapInstanceId("village".into()),
            source_map_id: MapId("source".into()),
            label: "原引用".into(),
            transform: default(),
        }]
        .into();
        {
            let mut row = test
                .app
                .world_mut()
                .get_mut::<SceneObjectRow>(test.row)
                .unwrap();
            let target = row.0.as_mut().unwrap();
            target.key = SceneObjectKey::SubMap {
                index: 0,
                id: MapInstanceId("village".into()),
            };
            target.name = "原引用".into();
        }
        open(&mut test);
        choose(&mut test, SceneMenuAction::Rename);
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut test.app);
        super::systems::set_name(
            &mut test
                .app
                .world_mut()
                .get_mut::<bevy::text::EditableText>(test.input)
                .unwrap(),
            &"灯".repeat(count),
        );
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        step(&mut test.app);
        step(&mut test.app);
        let menu = test.app.world().resource::<SceneContextMenuState>();
        if valid {
            assert_eq!(menu.ready.as_ref().unwrap().name.len(), 255);
        } else {
            assert!(menu.renaming && menu.queued.is_none() && menu.ready.is_none());
        }
    }
}
