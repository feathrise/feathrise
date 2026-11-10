//! 真实模式按钮与同步门回归：五态互斥、工具恢复、重复点击及鼠标释放隔离。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::types::EditorTool;
use crate::ui::courtyard::CourtyardUiState;
use crate::ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState};
use bevy::asset::AssetPlugin;

fn app() -> App {
    app_with_toolbar(false)
}

fn app_with_toolbar(global: bool) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::text::TextPlugin,
        bevy::scene::ScenePlugin,
    ))
    .insert_resource(SceneEditorState {
        open: true,
        tool: EditorTool::Smooth,
        ..default()
    })
    .init_resource::<SceneEditorSidebarState>()
    .init_resource::<ViewportModeState>()
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
    .init_resource::<CourtyardUiState>()
    .init_asset::<Image>() // 竖向模式按钮通过现有AssetServer加载透明图片；夹具不需要渲染。
    .add_systems(
        Update,
        (setup, input, super::super::sync_mode, style).chain(),
    );
    app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Terrain;
    if global { app.world_mut().spawn_scene(global_mode_toolbar()).unwrap(); }
    app.world_mut().spawn((SceneEditorViewport, Node::default()));
    app.update();
    app
}

#[test]
fn validation_select_after_brush_uses_original_button_before_numeric_and_f12() {
    let mut app = app();
    app.init_resource::<crate::standalone::StandaloneEditorMode>();
    press(&mut app, ViewportMode::Edit);
    assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Smooth);
    // 重现live9的错误前置：仅写Inspect仍会被Edit模式恢复为原画笔，徽标暂不显示。
    app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Inspect;
    app.update();
    assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Smooth);

    let select = super::super::validation_press_select(app.world_mut()).unwrap();
    assert!(matches!(app.world().get::<ModeButton>(select), Some(ModeButton::Mode(ViewportMode::Select))));
    app.update();
    assert_mode(&mut app, ViewportMode::Select);
    assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Inspect);
    // 真正模式回执仍执行同一原模式门；基线先完成选择操作，无需放宽原生留孔校验。
    crate::standalone::set_embedded_playing(app.world_mut(), true);
    app.update();
    crate::standalone::set_embedded_playing(app.world_mut(), false);
    app.update();
    assert_mode(&mut app, ViewportMode::Select);
    assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Inspect);
    assert_eq!(mode_button(&mut app, ViewportMode::Select), select);
}

#[test]
fn vertical_mode_tools_own_the_only_modes_and_header_reveals_context_controls_explicitly() {
    let mut app = app_with_toolbar(true);
    // 扩展筛选只对已安装作者域可见；先发布中性夹具，再验证全部七类真实按钮的动作和填色。
    app.insert_resource(crate::game_authoring::scene_interaction::fixtures::state("filter-fixture", 1));
    app.update();
    let world = app.world_mut();
    assert_eq!(world.query::<&ModeButton>().iter(world).filter(|button| matches!(button, ModeButton::Mode(_))).count(), 5);
    for (button, tooltip) in world.query::<(&ModeButton, &crate::ui::tooltips::EditorTooltipTarget)>().iter(world) {
        let expected = match button {
            ModeButton::Mode(value) => crate::ui::tooltips::TooltipAction::ViewportMode(*value),
            ModeButton::Options => crate::ui::tooltips::TooltipAction::ViewportOptions,
            _ => continue,
        };
        assert_eq!(tooltip.0, expected);
    } // 只迁移控件位置，名称与实际键位仍由同一个只读悬停提示模块解释。
    let (root, node) = world.query_filtered::<(Entity, &Node), With<ContextControls>>().single(world).unwrap();
    assert_eq!(node.display, Display::None);
    let options = world.query::<(Entity, &ModeButton)>().iter(world).find(|(_, button)| matches!(button, ModeButton::Options)).unwrap().0;
    world.entity_mut(options).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert_eq!(app.world().get::<Node>(root).unwrap().display, Display::Flex);
    *app.world_mut().get_mut::<UiInteraction>(options).unwrap() = UiInteraction::None;
    for filter in [SelectionFilter::All, SelectionFilter::Surface, SelectionFilter::Vegetation,
        SelectionFilter::Domain, SelectionFilter::Water, SelectionFilter::Placements, SelectionFilter::Gameplay] {
        let world = app.world_mut();
        let target = world.query::<(Entity, &ModeButton)>().iter(world)
            .find(|(_, button)| matches!(button, ModeButton::Filter(value) if *value == filter)).unwrap().0;
        world.entity_mut(target).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update();
        *app.world_mut().get_mut::<UiInteraction>(target).unwrap() = UiInteraction::None;
        app.update();
        let world = app.world_mut();
        assert_eq!(world.resource::<ViewportModeState>().filter, filter);
        for (button, background, border) in world.query::<(&ModeButton, &BackgroundColor, &BorderColor)>().iter(world) {
            if let ModeButton::Filter(value) = button {
                assert_eq!(background.0, if *value == filter { colors::SELECT_BG } else { colors::SLOT_BG });
                assert_eq!(*border, BorderColor::all(colors::COPPER_BORDER_WEAK));
            }
        } // 七类筛选实际切换后保持导航填色，不能由动态同步恢复亮白边框。
    }
    press(&mut app, ViewportMode::Rotate);
    assert_mode(&mut app, ViewportMode::Rotate);
    let entity = mode_button(&mut app, ViewportMode::Rotate);
    let parent = app.world().get::<ChildOf>(entity).unwrap().parent();
    assert!(app.world().get::<ModeToolsRoot>(parent).is_some());
    let world = app.world_mut();
    let global = world.query_filtered::<Entity, With<GlobalModeToolbar>>().single(world).unwrap();
    assert_eq!(world.get::<Children>(global).unwrap().len(), 1); // 顶栏仅保留筛选菜单，不再复制五种模式。
    assert!(world.get::<ControlsRoot>(global).is_none()); // 空间笔刷只挂到唯一的视口菜单宿主。
    app.init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    app.world_mut().resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>().active_tab =
        crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Domain("fixture.domain", 1);
    press(&mut app, ViewportMode::Move);
    assert_mode(&mut app, ViewportMode::Rotate); // 全局位置不能让预设文档借隐藏场景模式取得操作权。
}

fn mode_button(app: &mut App, value: ViewportMode) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ModeButton)>()
        .iter(world)
        .find_map(|(entity, button)| {
            matches!(button, ModeButton::Mode(mode) if *mode == value).then_some(entity)
        })
        .unwrap()
}

fn press(app: &mut App, value: ViewportMode) {
    let target = mode_button(app, value);
    let world = app.world_mut();
    for mut interaction in world
        .query_filtered::<&mut UiInteraction, With<ModeButton>>()
        .iter_mut(world)
    {
        *interaction = UiInteraction::None;
    }
    world.entity_mut(target).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
}

fn assert_mode(app: &mut App, value: ViewportMode) {
    let world = app.world_mut();
    let modes = world
        .query::<(&ModeButton, &UiInteraction, &BackgroundColor, &BorderColor)>()
        .iter(world)
        .filter_map(|(button, interaction, background, border)| match button {
            ModeButton::Mode(mode) => Some((*mode, *interaction, background.0, *border)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(modes.len(), 5);
    for (mode, interaction, background, border) in modes {
        let fill = match (interaction, mode == value) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::SLOT_BG,
        };
        assert_eq!(background, fill, "模式选中与交互填色：{mode:?}");
        assert_eq!(border, BorderColor::all(colors::COPPER_BORDER_WEAK));
    } // 持续选中态由填色区分，模式切换和被阻塞按下均不能恢复亮白描边。
    assert_eq!(world.resource::<ViewportModeState>().mode, value);
    assert_eq!(
        world
            .resource::<SceneEditorState>()
            .placement_transform_enabled,
        value.transforms_placement()
    );
    assert!(!world.resource::<SceneEditorState>().dirty);
}

#[test]
fn five_modes_share_one_vertical_rail_and_restore_the_previous_edit_tool() {
    let mut app = app();
    let select = mode_button(&mut app, ViewportMode::Select);
    let row = app.world().get::<ChildOf>(select).unwrap().parent();
    let modes = app
        .world()
        .get::<Children>(row)
        .unwrap()
        .iter()
        .filter_map(|entity| match app.world().get::<ModeButton>(entity) {
            Some(ModeButton::Mode(mode)) => Some(*mode),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        modes,
        vec![
            ViewportMode::Select,
            ViewportMode::Move,
            ViewportMode::Rotate,
            ViewportMode::Scale,
            ViewportMode::Edit
        ]
    );
    assert_mode(&mut app, ViewportMode::Select);
    app.world_mut().resource_mut::<ViewportModeState>().filter = SelectionFilter::Water;
    press(&mut app, ViewportMode::Edit);
    assert_mode(&mut app, ViewportMode::Edit);
    assert_eq!(
        app.world().resource::<SceneEditorState>().tool,
        EditorTool::Smooth
    );
    press(&mut app, ViewportMode::Move);
    assert_mode(&mut app, ViewportMode::Move);
    assert_eq!(
        app.world().resource::<SceneEditorState>().tool,
        EditorTool::Inspect
    );
    assert_eq!(
        app.world()
            .resource::<ViewportModeState>()
            .selection_filter(app.world().resource::<SceneEditorState>()),
        SelectionFilter::Water
    );
    assert_eq!(
        app.world().resource::<ViewportModeState>().filter,
        SelectionFilter::Water
    );
    press(&mut app, ViewportMode::Move); // 已选模式再次按下不执行W的关闭动作。
    assert_mode(&mut app, ViewportMode::Move);
    for value in [ViewportMode::Rotate, ViewportMode::Scale] {
        press(&mut app, value);
        assert_mode(&mut app, value);
        press(&mut app, value);
        assert_mode(&mut app, value);
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        );
    }
    press(&mut app, ViewportMode::Edit);
    assert_mode(&mut app, ViewportMode::Edit);
    assert_eq!(
        app.world().resource::<SceneEditorState>().tool,
        EditorTool::Smooth
    );
    press(&mut app, ViewportMode::Select);
    assert_mode(&mut app, ViewportMode::Select);
    assert_eq!(
        app.world().resource::<SceneEditorState>().tool,
        EditorTool::Inspect
    );
    assert_eq!(
        app.world()
            .resource::<ViewportModeState>()
            .selection_filter(app.world().resource::<SceneEditorState>()),
        SelectionFilter::Water
    );
}

#[test]
fn hidden_scene_toolbar_rejects_stale_presses_and_does_not_own_document_mouse() {
    let mut app = app();
    let world = app.world_mut();
    let viewport = world.query_filtered::<Entity, With<SceneEditorViewport>>().single(world).unwrap();
    let rail = world.query_filtered::<Entity, With<ModeToolsRoot>>().single(world).unwrap();
    world.get_mut::<Node>(viewport).unwrap().display = Display::None;
    world.entity_mut(rail).insert(RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::ZERO) });
    press(&mut app, ViewportMode::Move);
    assert_mode(&mut app, ViewportMode::Select);
    assert!(!app.world().resource::<ViewportModeState>().pointer_over_controls);
    app.world_mut().get_mut::<Node>(viewport).unwrap().display = Display::Flex;
    app.update();
    assert_mode(&mut app, ViewportMode::Select); // 隐藏页按下已被消费，不会在返回场景后补放。
    press(&mut app, ViewportMode::Move);
    assert_mode(&mut app, ViewportMode::Move);
}

#[test]
fn mode_buttons_wait_for_mouse_release_and_w_shares_the_mode_highlight() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<SceneEditorNumericScrubMouseGuard>()
        .advance(true, true, false);
    press(&mut app, ViewportMode::Move);
    assert_mode(&mut app, ViewportMode::Select);
    app.world_mut()
        .resource_mut::<SceneEditorNumericScrubMouseGuard>()
        .advance(false, false, false);
    app.update(); // 原按下边沿已经消费，静默释放后不能补切模式。
    assert_mode(&mut app, ViewportMode::Select);
    for expected in [ViewportMode::Move, ViewportMode::Select] {
        app.world_mut()
            .resource_scope(|world, mut editor: Mut<SceneEditorState>| {
                let mut mode = world.resource_mut::<ViewportModeState>();
                crate::viewport::placement_move::toggle_placement_move(
                    &mut editor,
                    Some(&mut mode),
                );
            });
        app.update();
        assert_mode(&mut app, expected);
    }
}

#[test]
fn mesh_edit_reuses_the_rail_without_activating_object_transforms() {
    let mut app = app();
    app.init_resource::<MeshEditState>()
        .add_systems(Update, mesh_tool_input.before(input));
    app.world_mut().resource_mut::<MeshEditState>().activate_for_test();
    app.update();
    {
        let world = app.world_mut();
        assert_eq!(world.query_filtered::<&Node, With<ModeToolsRoot>>().single(world).unwrap().display, Display::Flex);
        for (button, node) in world.query::<(&ModeButton, &Node)>().iter(world) {
            if let ModeButton::Mode(mode) = *button {
                assert_eq!(node.display, if mode == ViewportMode::Edit { Display::None } else { Display::Flex });
            }
        }
    }
    for (mode, tool) in [
        (ViewportMode::Move, MeshEditTool::Move),
        (ViewportMode::Rotate, MeshEditTool::Rotate),
        (ViewportMode::Scale, MeshEditTool::Scale),
        (ViewportMode::Select, MeshEditTool::Select),
    ] {
        press(&mut app, mode);
        assert_eq!(app.world().resource::<MeshEditState>().request, Some(MeshEditAction::Tool(tool)));
        assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Select);
        let editor = app.world().resource::<SceneEditorState>();
        assert!(!editor.placement_transform_enabled);
        assert!(!editor.dirty);
        // 此夹具只测按钮分发；模拟核心采用意图后松开，确认持续高亮读取元素工具而非全局 Select。
        {
            let mut state = app.world_mut().resource_mut::<MeshEditState>();
            state.request = None;
            state.tool = tool;
        }
        let world = app.world_mut();
        for mut interaction in world.query_filtered::<&mut UiInteraction, With<ModeButton>>().iter_mut(world) {
            *interaction = UiInteraction::None;
        }
        app.update();
        let world = app.world_mut();
        for (button, background) in world.query::<(&ModeButton, &BackgroundColor)>().iter(world) {
            if let ModeButton::Mode(value) = *button {
                assert_eq!(background.0, if value == mode { colors::SELECT_BG } else { colors::SLOT_BG });
            }
        }
    }
    app.world_mut().resource_mut::<SceneEditorNumericScrubMouseGuard>().advance(true, true, false);
    press(&mut app, ViewportMode::Move);
    assert!(app.world().resource::<MeshEditState>().request.is_none());
    app.world_mut().resource_mut::<SceneEditorNumericScrubMouseGuard>().advance(false, false, false);
    app.update();
    assert!(app.world().resource::<MeshEditState>().request.is_none(), "被守卫拒绝的完成点击不能延后补放");
    app.world_mut().resource_mut::<MeshEditState>().request = Some(MeshEditAction::SelectAll);
    press(&mut app, ViewportMode::Rotate);
    assert_eq!(app.world().resource::<MeshEditState>().request, Some(MeshEditAction::SelectAll), "竖栏不得覆盖同帧已发布的元素意图");
    // 退出后恢复同一组五个节点，普通模式按钮继续经过原对象变换入口。
    *app.world_mut().resource_mut::<MeshEditState>() = MeshEditState::default();
    app.update();
    {
        let world = app.world_mut();
        let mut visible = 0;
        for (button, node) in world.query::<(&ModeButton, &Node)>().iter(world) {
            if matches!(button, ModeButton::Mode(_)) {
                assert_eq!(node.display, Display::Flex);
                visible += 1;
            }
        }
        assert_eq!(visible, 5);
    }
    press(&mut app, ViewportMode::Move);
    assert_mode(&mut app, ViewportMode::Move);
    assert!(app.world().resource::<MeshEditState>().request.is_none());
}
