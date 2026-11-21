//! 正式右键边沿、中文名称、目标换代和鼠标释放门回归，不访问系统剪贴板或场景文件。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::SceneContextMenuState, systems, types::*};
use crate::{
    authoring::{EditorAuthoringSession, test_heightfield_draft},
    components::SceneEditorRoot,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
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

#[test]
fn scene_tree_double_click_rejects_rebound_slots_modifiers_and_expired_first_click() {
    use bevy::time::Real;

    let Fixture { mut app, row, .. } = fixture();
    app.init_resource::<Time<Real>>()
        .init_resource::<crate::ui::gltf_browser::EditorGltfBrowserState>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(Update, systems::queue_tree_focus
            .after(update_scene_editor_numeric_scrub_mouse_guard)
            .before(systems::dispatch));
    let replacement = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let mut alternate = target(&session);
        let placements = &mut session.draft.as_mut().unwrap().placements.placements;
        let mut record = placements[0].clone();
        record.id = "scroll-rebound-row".into();
        alternate.name = "滚动后可见组".into();
        std::sync::Arc::make_mut(&mut record.editor).group = alternate.name.clone();
        alternate.key = SceneObjectKey::Group { first_member: placements.len(), id: record.id.clone() };
        placements.push(record);
        alternate
    };
    let release = |app: &mut App| {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Left);
        mouse.release(MouseButton::Left);
        app.world_mut().entity_mut(row).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.update(); // 静默帧即使允许派发也只能采用尚有效的同一行双击。
    };
    let focused = |app: &App| app.world().resource::<SceneContextMenuState>().ready.as_ref()
        .is_some_and(|request| request.action == SceneMenuAction::Focus);

    release(&mut app);
    assert!(!focused(&app));
    app.world_mut().entity_mut(row).insert(SceneObjectRow(Some(replacement)));
    release(&mut app);
    assert!(!focused(&app), "同一复用槽换绑为另一对象时仍是两次单击");
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ShiftLeft);
    release(&mut app);
    assert!(!focused(&app), "Shift 的增量选择不得聚焦");
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::ShiftLeft);
    release(&mut app);
    assert!(!focused(&app), "修饰键手势不得留下首击");
    app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_millis(400));
    release(&mut app);
    assert!(!focused(&app), "超时再次点击只选择");
    release(&mut app);
    assert!(focused(&app), "当前行的有效二击仍能正常派发");
    assert_eq!(app.world().resource::<crate::ui::gltf_browser::EditorGltfBrowserState>().selected_group.as_deref(), Some("滚动后可见组"));
}

fn target(session: &EditorAuthoringSession) -> SceneObjectTarget {
    let draft = session.draft.as_ref().unwrap();
    SceneObjectTarget {
        scene_id: draft.scene_id.clone(),
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
    let map = draft.scene_id.clone();
    let session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    };
    let object = target(&session);
    let mut app = App::new();
    app.add_plugins(feathrise_editor_ui::EditorButtonInputPlugin)
        .insert_resource(session)
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(map),
            ..default()
        })
        .init_resource::<SceneContextMenuState>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<crate::ui::text_input::EditorTextInputGuard>()
        .add_systems(
            Update,
            (
                systems::control_menu,
                update_scene_editor_numeric_scrub_mouse_guard,
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
        SceneEditorRoot,
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
            UiInteraction::Hovered,
        ))
        .id();
    let input = app
        .world_mut()
        .spawn((
            RenameInput,
            crate::ui::text_input::single_line_input("", 128),
        ))
        .id();
    app.world_mut().spawn((RenameField, UiInteraction::None));
    // 无窗口布局不产生命中几何；显式给出非零尺寸和可见性，让共享捕获执行真实资格检查。
    for action in SceneMenuAction::ALL {
        let child = app.world_mut().spawn(Text::new("")).id();
        app.world_mut()
            .spawn((
                InteractionButton,
                MenuButton(action),
                UiInteraction::None,
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
        UiInteraction::None,
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
        UiInteraction::None,
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
            .query_filtered::<(&mut UiInteraction, &RelativeCursorPosition), With<InteractionButton>>()
            .iter_mut(world)
        {
            if *interaction == UiInteraction::Pressed {
                *interaction = if cursor.cursor_over() {
                    UiInteraction::Hovered
                } else {
                    UiInteraction::None
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
        .query::<(&MenuButton, &mut UiInteraction)>()
        .iter_mut(world)
    {
        *interaction = if button.0 == action {
            UiInteraction::Pressed
        } else {
            UiInteraction::None
        };
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut fixture.app);
}

#[test]
fn blender_menu_opens_builtin_and_whole_glb_placements() {
    use crate::authoring::blender_mesh::BlenderMeshAction;
    use feathrise_editor_ui::localization::EditorLocale;
    use feathrise_scene_content::ScenePlacementAssetTarget;

    for (asset_path, asset_target, allowed) in [
        ("", ScenePlacementAssetTarget::DEFAULT_CUBOID, true),
        ("gltf/model.glb", ScenePlacementAssetTarget::Scene0, true),
        ("gltf/model.gltf", ScenePlacementAssetTarget::Scene0, false),
    ] {
        let mut test = fixture();
        test.app
            .add_message::<BlenderMeshAction>()
            .add_systems(Update, systems::send_blender_action.after(super::focus::select_target))
            .insert_resource(EditorLocale::English);
        {
            let mut session = test.app.world_mut().resource_mut::<EditorAuthoringSession>();
            let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
            record.asset_path = asset_path.into();
            record.asset_target = asset_target;
            record.water_surfaces.clear(); // 通用移动夹具带水绑定，本用例验证普通模型的 Blender 入口。
        }
        open(&mut test);
        let world = test.app.world_mut();
        let (_, node, children) = world.query::<(&MenuButton, &Node, &Children)>()
            .iter(world)
            .find(|(button, _, _)| button.0 == SceneMenuAction::EditInBlender)
            .unwrap();
        assert_eq!(node.display == Display::Flex, allowed);
        if allowed {
            assert_eq!(world.get::<Text>(children[0]).unwrap().as_str(), "Edit in Blender");
        }
        choose(&mut test, SceneMenuAction::EditInBlender);
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut test.app);
        step(&mut test.app);
        let world = test.app.world();
        let messages = world.resource::<Messages<BlenderMeshAction>>();
        let mut reader = messages.get_cursor();
        let command = reader.read(messages).next();
        if allowed {
            let draft = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
            assert!(matches!(command, Some(BlenderMeshAction { scene_id, placement_id })
                if *scene_id == draft.scene_id && *placement_id == draft.placements.placements[0].id));
        } else {
            assert!(command.is_none());
        }
    }
}

#[test]
fn scene_menu_reveals_glb_from_frozen_row_and_ignores_builtin_models() {
    use crate::ui::{file_manager::{FileManagerState, browser::BrowserNavigate}, gltf_browser::EditorGltfBrowserState};
    use feathrise_scene_content::ScenePlacementAssetTarget;

    const MODEL: &str = "scenes/测试场景/models/blender-abcd.GLB";
    for (asset_path, asset_target) in [
        (MODEL, ScenePlacementAssetTarget::Scene0),
        ("", ScenePlacementAssetTarget::DEFAULT_CUBOID),
    ] {
        let mut test = fixture();
        let mut browser = EditorGltfBrowserState::default();
        browser.selected_placement_id = Some("previous-selection".into());
        test.app.init_resource::<FileManagerState>().insert_resource(browser)
            .add_message::<BrowserNavigate>()
            .add_systems(Update, systems::reveal_glb_file.after(super::focus::select_target));
        {
            let mut session = test.app.world_mut().resource_mut::<EditorAuthoringSession>();
            let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
            record.asset_path = asset_path.into();
            record.asset_target = asset_target;
        }
        open(&mut test);
        let world = test.app.world_mut();
        let (_, node, children) = world.query::<(&MenuButton, &Node, &Children)>()
            .iter(world).find(|(button, _, _)| button.0 == SceneMenuAction::RevealGlb).unwrap();
        assert_eq!(node.display == Display::Flex, asset_path == MODEL);
        if asset_path == MODEL {
            assert_eq!(world.get::<Text>(children[0]).unwrap().as_str(), "定位 GLB 文件");
        }
        choose(&mut test, SceneMenuAction::RevealGlb);
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut test.app);
        assert!(test.app.world().resource::<Messages<BrowserNavigate>>().is_empty());
        step(&mut test.app);
        let world = test.app.world();
        let messages = world.resource::<Messages<BrowserNavigate>>();
        let mut reader = messages.get_cursor();
        let request = reader.read(messages).next();
        if asset_path == MODEL {
            assert!(matches!(request, Some(BrowserNavigate::RevealFile { path, .. }) if path == MODEL));
        } else {
            assert!(request.is_none());
        }
        assert_eq!(world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref(), Some("previous-selection"));
    }
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
            .resource::<SceneEditorNumericScrubMouseGuard>()
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
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
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
fn scene_menu_queued_command_is_cancelled_if_scene_or_slot_changes_before_release() {
    for invalid in 0..2 {
        let mut test = fixture();
        open(&mut test);
        choose(&mut test, SceneMenuAction::Delete);
        if invalid == 0 {
            test.app
                .world_mut()
                .resource_mut::<SceneEditorState>()
                .selected_scene_id = None;
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
        .query_filtered::<&mut ComputedNode, With<SceneEditorRoot>>()
        .iter_mut(world)
    {
        node.inverse_scale_factor = 0.5;
    }
    open(&mut test);
    let popup = test.app.world().get::<Node>(test.popup).unwrap();
    assert_eq!((popup.width, popup.height), (px(184), px(204)));
    assert_eq!(popup.left, px(452));
    assert_eq!(popup.top, px(152));
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    step(&mut test.app);
    assert_eq!(
        test.app.world().get::<Node>(test.backdrop).unwrap().display,
        Display::None
    );
    assert!(test.app.world().resource::<SceneEditorState>().open);
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
        gltf_browser::EditorGltfBrowserState, subscenes::SubsceneEditorState,
        terrain_objects::TerrainObjectsEditorState,
    };
    use crate::viewport::{LayerCandidateState, LayerTarget};
    use feathrise_scene_content::{SceneId, SceneInstanceId, SceneInstancePlacement, TerrainId};
    for kind in 0..4 {
        let mut test = fixture();
        test.app
            .init_resource::<EditorGltfBrowserState>()
            .init_resource::<LayerCandidateState>()
            .init_resource::<TerrainObjectsEditorState>()
            .init_resource::<SubsceneEditorState>();
        if kind == 0 {
            test.app.world_mut().resource_mut::<SceneEditorState>().tool = crate::types::EditorTool::Water;
        }
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
            2 => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .draft
                    .as_mut()
                    .unwrap()
                    .manifest
                    .subscenes = vec![SceneInstancePlacement {
                    id: SceneInstanceId("village".into()),
                    source_scene_id: SceneId("source".into()),
                    label: "村庄引用".into(),
                    transform: default(),
                }]
                .into();
                SceneObjectKey::Subscene {
                    index: 0,
                    id: SceneInstanceId("village".into()),
                }
            }
            _ => {
                let world = test.app.world_mut();
                let id = {
                    let mut session = world.resource_mut::<EditorAuthoringSession>();
                    let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
                    std::sync::Arc::make_mut(&mut record.editor).group = "村口组".into();
                    record.id.clone()
                };
                world.get_mut::<SceneObjectRow>(test.row).unwrap().0.as_mut().unwrap().name = "村口组".into();
                SceneObjectKey::Group { first_member: 0, id }
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
            [7, 5, 4, 1][kind]
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
                .resource::<SceneEditorState>()
                .viewport_framing_requested,
            Some(if kind == 0 { crate::types::EditorViewportFraming::TreePlacement }
                else { crate::types::EditorViewportFraming::Selected })
        );
        let selected = test
            .app
            .world()
            .resource::<LayerCandidateState>()
            .selected
            .as_ref();
        match kind {
            0 => assert_eq!(selected, Some(&LayerTarget::Placement("move-test".into()))),
            1 => assert_eq!(
                selected,
                Some(&LayerTarget::TerrainObject(TerrainId::legacy().as_str().to_owned()))
            ),
            2 => assert_eq!(selected, Some(&LayerTarget::Subscene("village".into()))),
            _ => {
                assert!(selected.is_none()); // 只聚焦整组，不把成员强行改成多选。
                assert_eq!(test.app.world().resource::<EditorGltfBrowserState>().selected_group.as_deref(), Some("村口组"));
            }
        }
    }
}

#[test]
fn scene_menu_subscene_chinese_name_respects_existing_utf8_byte_limit_before_confirming() {
    use feathrise_scene_content::{SceneId, SceneInstanceId, SceneInstancePlacement};
    for (count, valid) in [(85, true), (86, false)] {
        let mut test = fixture();
        test.app
            .world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft
            .as_mut()
            .unwrap()
            .manifest
            .subscenes = vec![SceneInstancePlacement {
            id: SceneInstanceId("village".into()),
            source_scene_id: SceneId("source".into()),
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
            target.key = SceneObjectKey::Subscene {
                index: 0,
                id: SceneInstanceId("village".into()),
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
