//! 文件菜单真实ECS开关、混合关闭、模态/输入保护和鼠标静默释放回归。
use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::super::{
    AssetPreviewWorkspace, DocumentState, documents::DocumentTabRoot, types::ExpandedAssetPreview,
};
use super::{components::*, resources::AssetTabMenuState, systems};
use crate::ui::{
    numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
    vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
};
use crate::{components::SceneEditorRoot, resources::*};
use bevy::{prelude::*, ui::RelativeCursorPosition, window::PrimaryWindow};

struct Fixture {
    app: App,
    window: Entity,
    tab: Entity,
    button: Entity,
}

fn fixture() -> Fixture {
    let mut workspace = VegetationPresetEditorState::default();
    let mut documents = DocumentState::default();
    documents
        .expand("第一.json".into(), &mut workspace)
        .unwrap();
    documents.expand("第二.rs".into(), &mut workspace).unwrap();
    let media = AssetPreviewWorkspace {
        expanded: Some(ExpandedAssetPreview {
            id: 2,
            path: "第二.png".into(),
            return_tab: PresetWorkspaceTab::Document,
            view: default(),
            target: default(),
            restore_cursor: 0,
            audio_position: 0.0,
        }),
        parked: vec![ExpandedAssetPreview {
            id: 1,
            path: "第一.glb".into(),
            return_tab: PresetWorkspaceTab::Scene,
            view: default(),
            target: default(),
            restore_cursor: 0,
            audio_position: 0.0,
        }],
        central_active: true,
        ..default()
    };
    workspace.active_tab = PresetWorkspaceTab::AssetPreview;
    let mut app = App::new();
    // 复用正式注册入口，同时提供资产与工程文件消息，避免独立夹具落后于生产系统参数。
    crate::ui::external_files::register_requests(&mut app);
    app.insert_resource(workspace)
        .insert_resource(documents)
        .insert_resource(media)
        .insert_resource(SceneEditorState {
            open: true,
            selected_asset: Some("其他.txt".into()),
            ..default()
        })
        .init_resource::<crate::ui::sidebars::SceneEditorSidebarState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<SceneEditorSettingsState>()
        .init_resource::<AssetTabMenuState>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_message::<super::messages::CloseAllAssetTabs>()
        .add_message::<super::messages::CloseUnmodifiedAssetTabs>()
        .add_message::<super::super::documents::SaveAllDocuments>()
        .add_message::<crate::ui::asset_context_menu::CopyAssetPath>()
        .insert_resource(crate::ui::external_files::ExternalEditors::for_tests(&[crate::ui::external_files::ExternalEditor::VsCode]))
        .add_systems(
            Update,
            (
                systems::control_menu,
                systems::close_all_tabs,
                update_scene_editor_numeric_scrub_mouse_guard,
                super::scene::sync,
            )
                .chain(),
        );
    let mut window = Window {
        focused: true,
        ..default()
    };
    window.set_physical_cursor_position(Some(bevy::math::DVec2::new(350.0, 18.0)));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    app.world_mut().spawn((
        SceneEditorRoot,
        ComputedNode {
            size: Vec2::new(1920.0, 1080.0),
            inverse_scale_factor: 1.0,
            ..default()
        },
        UiGlobalTransform::from_xy(960.0, 540.0),
    ));
    let tab = app
        .world_mut()
        .spawn((
            DocumentTabRoot,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ))
        .id();
    let button = app
        .world_mut()
        .spawn((
            CloseAllTabsButton,
            UiInteraction::None,
            BackgroundColor::default(),
        ))
        .id();
    app.world_mut().spawn((TabMenuBackdrop, Node::default()));
    app.world_mut().spawn((TabMenuPopup, Node::default()));
    Fixture {
        app,
        window,
        tab,
        button,
    }
}

fn step(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

fn open(test: &mut Fixture) {
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    step(&mut test.app);
    assert!(test.app.world().resource::<AssetTabMenuState>().open);
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    step(&mut test.app);
}

fn click_close(test: &mut Fixture) {
    *test
        .app
        .world_mut()
        .get_mut::<UiInteraction>(test.button)
        .unwrap() = UiInteraction::Pressed;
    test.app.world_mut().entity_mut(test.button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut test.app);
}

#[test]
fn tab_menu_close_waits_for_inside_release_and_outside_release_keeps_tabs() {
    for outside in [false, true] {
        let mut test = fixture();
        test.app.add_plugins(feathrise_editor_ui::EditorButtonInputPlugin);
        test.app.world_mut().entity_mut(test.button).insert((
            InteractionButton, Node::default(),
            InheritedVisibility::VISIBLE, // 无窗口夹具显式提供可见性传播结果。
            RelativeCursorPosition { cursor_over: true, ..default() },
            ComputedNode { size: Vec2::new(100.0, 24.0), ..default() },
        ));
        open(&mut test);
        *test.app.world_mut().get_mut::<UiInteraction>(test.button).unwrap() = UiInteraction::Pressed;
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut test.app);
        step(&mut test.app);
        assert!(test.app.world().resource::<AssetTabMenuState>().open);
        assert!(test.app.world().resource::<DocumentState>().expanded());
        assert!(test.app.world().resource::<AssetPreviewWorkspace>().has_tabs());
        *test.app.world_mut().get_mut::<UiInteraction>(test.button).unwrap() =
            if outside { UiInteraction::None } else { UiInteraction::Hovered };
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut test.app);
        step(&mut test.app);
        assert_eq!(test.app.world().resource::<AssetTabMenuState>().open, outside);
        assert_eq!(test.app.world().resource::<DocumentState>().expanded(), outside);
        assert_eq!(test.app.world().resource::<AssetPreviewWorkspace>().has_tabs(), outside);
    }
}

#[test]
fn file_tab_menu_closes_all_document_and_media_tabs_and_releases_only_after_mouse_up() {
    let mut test = fixture();
    open(&mut test);
    assert_eq!(
        test.app.world().resource::<AssetTabMenuState>().anchor,
        Vec2::new(350.0, 18.0)
    );
    assert!(test.app.world().resource::<DocumentState>().expanded()); // 右击只打开菜单。
    click_close(&mut test);
    assert!(!test.app.world().resource::<DocumentState>().expanded());
    assert!(
        !test
            .app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .has_tabs()
    );
    assert_eq!(
        test.app
            .world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Scene
    );
    assert_eq!(
        test.app
            .world()
            .resource::<SceneEditorState>()
            .selected_asset
            .as_deref(),
        Some("第二.png")
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
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    step(&mut test.app);
    assert!(
        !test
            .app
            .world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn background_close_all_preserves_active_author_page_selection_and_hidden_inspector() {
    let mut test = fixture();
    test.app.init_resource::<SceneEditorLayoutState>();
    test.app
        .world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Domain("test.author", 0);
    test.app
        .world_mut()
        .resource_mut::<SceneEditorLayoutState>()
        .inspector_visible = false;
    open(&mut test);
    click_close(&mut test);
    assert_eq!(
        test.app
            .world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Domain("test.author", 0)
    );
    assert_eq!(
        test.app
            .world()
            .resource::<SceneEditorState>()
            .selected_asset
            .as_deref(),
        Some("其他.txt")
    );
    assert!(
        !test
            .app
            .world()
            .resource::<SceneEditorLayoutState>()
            .inspector_visible
    );
    assert!(!test.app.world().resource::<DocumentState>().expanded());
    assert!(
        !test
            .app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .has_tabs()
    );
}

/// 用实际Bevy命中打开菜单；RenderTarget临时绑定窗口仅用于UI鼠标输入，不创建额外View。
pub(in super::super) fn gpu_open_menu(
    app: &mut App,
    window: Entity,
    camera: Entity,
    position: Vec2,
) {
    gpu_click(app, window, camera, position, MouseButton::Right);
    let diagnostic = {
        let world = app.world_mut();
        let mut tabs = world.query_filtered::<(&RelativeCursorPosition, &ComputedNode, &UiGlobalTransform), With<DocumentTabRoot>>();
        let rows: Vec<_> = tabs
            .iter(world)
            .map(|(cursor, node, transform)| (*cursor, node.size(), transform.affine().translation))
            .collect();
        format!(
            "光标{position:?}，标签{rows:?}，鼠标门{}，布局所有权{}，IME{}，小场景所有权{}",
            world
                .resource::<SceneEditorNumericScrubMouseGuard>()
                .blocked(),
            world
                .resource::<SceneEditorLayoutState>()
                .owns_mouse_this_frame,
            world.resource::<EditorTextInputGuard>().block_shortcuts,
            world
                .resource::<super::super::DocumentMinimapState>()
                .owns_mouse()
        )
    };
    assert!(
        app.world().resource::<AssetTabMenuState>().open,
        "{diagnostic}"
    );
}

pub(in super::super) fn gpu_click_close_all(app: &mut App, window: Entity, camera: Entity) {
    let position = {
        let world = app.world_mut();
        let mut buttons = world.query_filtered::<&UiGlobalTransform, With<CloseAllTabsButton>>();
        buttons.single(world).unwrap().affine().translation
    };
    gpu_click(app, window, camera, position, MouseButton::Left);
    assert!(!app.world().resource::<AssetTabMenuState>().open);
    assert!(!app.world().resource::<DocumentState>().expanded());
    assert!(!app.world().resource::<AssetPreviewWorkspace>().has_tabs());
}

pub(in super::super) fn gpu_click_close_unmodified(app: &mut App, window: Entity, camera: Entity) {
    let position = {
        let world = app.world_mut();
        let mut buttons =
            world.query_filtered::<&UiGlobalTransform, With<CloseUnmodifiedTabsButton>>();
        buttons.single(world).unwrap().affine().translation
    };
    gpu_click(app, window, camera, position, MouseButton::Left);
    assert!(!app.world().resource::<AssetTabMenuState>().open);
    assert!(!app.world().resource::<AssetPreviewWorkspace>().has_tabs());
}

pub(in super::super) fn gpu_click_save_all(app: &mut App, window: Entity, camera: Entity) {
    let tabs = app.world().resource::<DocumentState>().tab_count();
    let position = {
        let world = app.world_mut();
        world.query_filtered::<&UiGlobalTransform, With<SaveAllDraftsButton>>()
            .single(world).unwrap().affine().translation
    };
    gpu_click(app, window, camera, position, MouseButton::Left);
    assert!(!app.world().resource::<AssetTabMenuState>().open);
    assert_eq!(app.world().resource::<DocumentState>().tab_count(), tabs);
}

pub(in super::super) fn gpu_click_external(app: &mut App, window: Entity, camera: Entity, action: crate::ui::external_files::ExternalFileAction) {
    let position = {
        let world = app.world_mut();
        world.query::<(&ExternalFileButton, &UiGlobalTransform)>().iter(world)
            .find(|(button, _)| button.0 == action).unwrap().1.affine().translation
    };
    gpu_click(app, window, camera, position, MouseButton::Left);
    assert!(!app.world().resource::<AssetTabMenuState>().open);
}

pub(in super::super) fn gpu_click_copy_path(app: &mut App, window: Entity, camera: Entity, kind: crate::ui::asset_context_menu::AssetPathCopyKind) {
    let position = {
        let world = app.world_mut();
        world.query::<(&CopyFilePathButton, &UiGlobalTransform)>().iter(world)
            .find(|(button, _)| button.0 == kind).unwrap().1.affine().translation
    };
    gpu_click(app, window, camera, position, MouseButton::Left);
    assert!(!app.world().resource::<AssetTabMenuState>().open);
}

pub(in super::super) fn gpu_assert_external_visibility(app: &mut App, editors: &[crate::ui::external_files::ExternalEditor]) {
    let world = app.world_mut();
    for (action, node) in world.query::<(&ExternalFileButton, &Node)>().iter(world) {
        let expected = action.0.editor().is_none_or(|editor| editors.contains(&editor));
        assert_eq!(node.display != Display::None, expected); // 私有组件在所属域内验证，隐藏IDE真实退出布局。
    }
}

fn gpu_click(app: &mut App, window: Entity, camera: Entity, position: Vec2, button: MouseButton) {
    use bevy::ecs::system::RunSystemOnce;
    let target = app
        .world()
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(button);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    if button == MouseButton::Left {
        let world = app.world_mut();
        let mut buttons = world.query_filtered::<&UiInteraction, Or<(With<CloseAllTabsButton>, With<CloseUnmodifiedTabsButton>, With<SaveAllDraftsButton>, With<ExternalFileButton>, With<CopyFilePathButton>)>>();
        assert_eq!(
            buttons
                .iter(world)
                .filter(|interaction| **interaction == UiInteraction::Pressed)
                .count(),
            1,
            "真实鼠标命中必须到达菜单按钮"
        );
    }
    app.world_mut()
        .run_system_once(systems::control_menu)
        .unwrap();
    app.world_mut()
        .run_system_once(systems::close_all_tabs)
        .unwrap();
    app.world_mut()
        .run_system_once(update_scene_editor_numeric_scrub_mouse_guard)
        .unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear(); // 松键属于下一输入帧，不能重用右键按下边沿。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(button);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    app.world_mut()
        .run_system_once(systems::control_menu)
        .unwrap();
    app.world_mut().run_system_once(systems::close_all_tabs).unwrap();
    app.world_mut()
        .run_system_once(update_scene_editor_numeric_scrub_mouse_guard)
        .unwrap();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

#[test]
fn outside_click_escape_and_secondary_buttons_only_dismiss_file_tab_menu() {
    for cancel in 0..4 {
        let mut test = fixture();
        open(&mut test);
        if cancel == 0 {
            test.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        } else {
            test.app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press([MouseButton::Left, MouseButton::Right, MouseButton::Middle][cancel - 1]);
        }
        step(&mut test.app);
        assert!(!test.app.world().resource::<AssetTabMenuState>().open);
        assert!(test.app.world().resource::<DocumentState>().expanded());
        assert!(
            test.app
                .world()
                .resource::<AssetPreviewWorkspace>()
                .has_tabs()
        );
        assert!(
            test.app
                .world()
                .resource::<SceneEditorNumericScrubMouseGuard>()
                .blocked()
        );
    }
}

#[test]
fn hidden_unfocused_modal_and_ime_states_cancel_instead_of_replaying_close_all() {
    for blocked in 0..4 {
        let mut test = fixture();
        open(&mut test);
        match blocked {
            0 => test.app.world_mut().resource_mut::<SceneEditorState>().open = false,
            1 => {
                test.app
                    .world_mut()
                    .get_mut::<Window>(test.window)
                    .unwrap()
                    .focused = false
            }
            2 => {
                test.app
                    .world_mut()
                    .resource_mut::<SceneEditorSettingsState>()
                    .open = true
            }
            _ => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorTextInputGuard>()
                    .block_shortcuts = true
            }
        }
        click_close(&mut test);
        assert!(!test.app.world().resource::<AssetTabMenuState>().open);
        assert!(test.app.world().resource::<DocumentState>().expanded());
        test.app.world_mut().resource_mut::<SceneEditorState>().open = true;
        test.app
            .world_mut()
            .get_mut::<Window>(test.window)
            .unwrap()
            .focused = true;
        test.app
            .world_mut()
            .resource_mut::<SceneEditorSettingsState>()
            .open = false;
        test.app
            .world_mut()
            .resource_mut::<EditorTextInputGuard>()
            .block_shortcuts = false;
        step(&mut test.app);
        assert!(test.app.world().resource::<DocumentState>().expanded());
    }
}

#[test]
fn right_click_outside_visible_file_tabs_or_with_numeric_ownership_does_not_open() {
    for blocked in 0..3 {
        let mut test = fixture();
        match blocked {
            0 => {
                test.app
                    .world_mut()
                    .get_mut::<RelativeCursorPosition>(test.tab)
                    .unwrap()
                    .cursor_over = false
            }
            1 => {
                test.app
                    .world_mut()
                    .resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>()
                    .entity = Some(test.tab)
            }
            _ => {
                test.app
                    .world_mut()
                    .resource_mut::<DocumentState>()
                    .close_all_tabs();
                test.app
                    .world_mut()
                    .resource_mut::<AssetPreviewWorkspace>()
                    .close_all_tabs();
            }
        }
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        step(&mut test.app);
        assert!(!test.app.world().resource::<AssetTabMenuState>().open);
    }
}

#[test]
fn close_all_files_returns_to_a_still_open_author_tab_and_falls_back_when_it_was_closed() {
    for library_open in [true, false] {
        let mut test = fixture();

        test.app.insert_resource(feathrise_editor_core::WorkspaceDomainState {open_tabs:if library_open {vec![PresetWorkspaceTab::Domain("test.author",0)]} else {vec![]},..default()});
        let mut documents = test
            .app
            .world_mut()
            .remove_resource::<DocumentState>()
            .unwrap();
        {
            let mut workspace = test
                .app
                .world_mut()
                .resource_mut::<VegetationPresetEditorState>();
            workspace.active_tab = PresetWorkspaceTab::Domain("test.author", 0);
            documents.expand("第二.rs".into(), &mut workspace).unwrap();
        }
        test.app.insert_resource(documents);
        open(&mut test);
        click_close(&mut test);
        assert_eq!(
            test.app
                .world()
                .resource::<VegetationPresetEditorState>()
                .active_tab,
            if library_open {
                PresetWorkspaceTab::Domain("test.author", 0)
            } else {
                PresetWorkspaceTab::Scene
            }
        );
        assert_eq!(
            test.app
                .world()
                .resource::<SceneEditorState>()
                .selected_asset
                .as_deref(),
            Some("第二.rs")
        );
    }
}

#[test]
fn switching_scene_cancels_an_existing_menu_before_close_all_can_run() {
    let mut test = fixture();
    open(&mut test);
    test.app
        .world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(feathrise_scene_content::SceneId("MAP-next".into()));
    click_close(&mut test);
    assert!(!test.app.world().resource::<AssetTabMenuState>().open);
    assert!(test.app.world().resource::<DocumentState>().expanded());
    assert!(
        test.app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .has_tabs()
    );
}

#[test]
fn pending_native_text_edits_block_file_menu_without_replaying_the_old_click() {
    use bevy::{
        input_focus::{FocusCause, InputFocus},
        text::{EditableText, TextEdit},
    };
    let mut test = fixture();
    test.app.init_resource::<InputFocus>();
    let source = test.app.world_mut().spawn(EditableText::default()).id();
    test.app
        .world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::Insert("已确认但待采用".into()));
    test.app
        .world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    step(&mut test.app);
    assert!(!test.app.world().resource::<AssetTabMenuState>().open);
    test.app
        .world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .pending_edits
        .clear();
    step(&mut test.app);
    assert!(!test.app.world().resource::<AssetTabMenuState>().open);
}

#[test]
fn unfocused_document_source_pending_edits_still_block_the_file_menu() {
    use bevy::text::{EditableText, TextEdit};
    let mut test = fixture();
    let source = test
        .app
        .world_mut()
        .spawn((
            EditableText::default(),
            super::super::documents::DocumentSource::default(),
        ))
        .id();
    test.app
        .world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::Insert("待采用的粘贴".into()));
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    step(&mut test.app);
    assert!(!test.app.world().resource::<AssetTabMenuState>().open);
}

#[test]
fn close_unmodified_menu_item_closes_read_only_and_unloaded_file_tabs() {
    let mut test = fixture();
    let button = test
        .app
        .world_mut()
        .spawn((
            CloseUnmodifiedTabsButton,
            UiInteraction::None,
            BackgroundColor::default(),
        ))
        .id();
    open(&mut test);
    *test.app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    test.app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut test.app);
    assert!(!test.app.world().resource::<DocumentState>().expanded());
    assert!(
        !test
            .app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .has_tabs()
    );
    assert!(
        test.app
            .world()
            .resource::<SceneEditorState>()
            .status
            .contains("未修改")
    );
}

#[test]
fn external_background_file_tab_menu_freezes_the_clicked_file_and_preserves_active_tab() {
    use crate::ui::external_files::{ExternalFileTab, OpenExternalFile};
    for (_, action) in crate::ui::external_files::EXTERNAL_MENU_ITEMS {
        let mut test = fixture();
        test.app.insert_resource(crate::ui::external_files::ExternalEditors::for_tests(&crate::ui::external_files::ExternalEditor::ALL));
        test.app.world_mut().entity_mut(test.tab).insert(ExternalFileTab("第一.json".into()));
        let active = test.app.world().resource::<VegetationPresetEditorState>().active_tab;
        open(&mut test);
        assert_eq!(test.app.world().resource::<AssetTabMenuState>().path.as_deref(), Some("第一.json"));
        test.app.world_mut().spawn((ExternalFileButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut test.app);
        let requests: Vec<_> = test.app.world_mut().resource_mut::<Messages<OpenExternalFile>>().drain().collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "第一.json");
        assert_eq!(requests[0].action, action);
        assert_eq!(test.app.world().resource::<VegetationPresetEditorState>().active_tab, active);
        assert!(test.app.world().resource::<DocumentState>().expanded());
    }
}

#[test]
fn copied_file_tab_paths_use_the_clicked_file_and_blank_tab_area_cannot_copy() {
    use crate::ui::asset_context_menu::{AssetPathCopyKind, CopyAssetPath};
    for kind in AssetPathCopyKind::ALL {
        for file in [false, true] {
            let mut test = fixture();
            test.app.insert_resource(crate::ui::external_files::ExternalEditors::default());
            if file { test.app.world_mut().entity_mut(test.tab).insert(crate::ui::external_files::ExternalFileTab("第一.json".into())); }
            let active = test.app.world().resource::<VegetationPresetEditorState>().active_tab;
            let button = test.app.world_mut().spawn((CopyFilePathButton(kind), UiInteraction::None, Node::default(), BackgroundColor::default())).id();
            open(&mut test);
            assert_eq!(test.app.world().get::<Node>(button).unwrap().display, if file { Display::Flex } else { Display::None });
            *test.app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
            test.app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
            test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut test.app);
            step(&mut test.app);
            let requests: Vec<_> = test.app.world_mut().resource_mut::<Messages<CopyAssetPath>>().drain().collect();
            assert_eq!(requests.len(), usize::from(file));
            if file { assert_eq!(requests[0].path, "第一.json"); assert_eq!(requests[0].kind, kind); }
            assert_eq!(test.app.world().resource::<VegetationPresetEditorState>().active_tab, active);
            assert!(test.app.world().resource::<DocumentState>().expanded());
            assert!(test.app.world().resource::<Messages<super::super::documents::SaveAllDocuments>>().is_empty());
            assert!(test.app.world().resource::<Messages<crate::ui::external_files::OpenExternalFile>>().is_empty());
        }
    }
}

#[test]
fn external_file_tab_menu_rejects_old_pressed_items_after_an_editor_is_removed() {
    use crate::ui::external_files::{ExternalEditors, ExternalEditor, ExternalFileTab, OpenExternalFile};
    for (_, action) in crate::ui::external_files::EXTERNAL_MENU_ITEMS.into_iter().filter(|(_, action)| action.editor().is_some()) {
        let mut test = fixture();
        test.app.insert_resource(ExternalEditors::for_tests(&ExternalEditor::ALL));
        test.app.world_mut().entity_mut(test.tab).insert(ExternalFileTab("第一.json".into()));
        open(&mut test);
        test.app.insert_resource(ExternalEditors::default());
        test.app.world_mut().spawn((ExternalFileButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut test.app);
        assert!(test.app.world().resource::<Messages<OpenExternalFile>>().is_empty());
        assert!(test.app.world().resource::<DocumentState>().expanded());
    }
}

#[test]
fn external_file_tab_menu_cancels_when_its_frozen_target_disappears() {
    use crate::ui::external_files::{ExternalFileAction, ExternalFileTab, OpenExternalFile};
    let mut test = fixture();
    test.app.world_mut().entity_mut(test.tab).insert(ExternalFileTab("第一.json".into()));
    open(&mut test);
    test.app.world_mut().entity_mut(test.tab).remove::<ExternalFileTab>();
    test.app.world_mut().spawn((ExternalFileButton(ExternalFileAction::VsCode(None)), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut test.app);
    assert!(!test.app.world().resource::<AssetTabMenuState>().open);
    assert!(test.app.world().resource::<Messages<OpenExternalFile>>().is_empty());
}

#[test]
fn twelve_item_tab_menu_stays_inside_scaled_root_at_bottom_right() {
    let rect = crate::ui::asset_context_menu::helpers::sized_menu_rect(
        Vec2::new(959.0, 539.0),
        Vec2::new(960.0, 540.0),
        Vec2::new(240.0, 320.0),
    );
    assert_eq!(rect.size(), Vec2::new(240.0, 320.0));
    assert!(rect.max.cmple(Vec2::new(960.0, 540.0)).all());
}

#[test]
fn file_tab_save_all_dispatches_once_without_closing_or_switching_tabs() {
    let mut test = fixture();
    let button = test.app.world_mut().spawn((SaveAllDraftsButton, UiInteraction::None, BackgroundColor::default())).id();
    let active = test.app.world().resource::<VegetationPresetEditorState>().active_tab;
    open(&mut test);
    *test.app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    test.app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut test.app);
    assert!(test.app.world().resource::<DocumentState>().expanded());
    assert!(test.app.world().resource::<AssetPreviewWorkspace>().has_tabs());
    assert_eq!(test.app.world().resource::<VegetationPresetEditorState>().active_tab, active);
    assert_eq!(test.app.world().resource::<Messages<super::super::documents::SaveAllDocuments>>().len(), 1);
    assert!(test.app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
}
