//! 使用原传送列表、名称绑定和单墙事务验证导航；真实中文字体布局不读写用户场景或设置。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::*, types::*};
use crate::SceneEditorSystemSet;
use crate::authoring::{BoundaryEditState, EditorAuthoringSession, EditorPreviewAssets};
use crate::components::*;
use crate::resources::{
    EditorStaticTextInputState, SceneBrowserDragState, SceneBrowserState, SceneDeletionTask,
    SceneEditorNumericScrubMouseGuard, SceneEditorState,
};
use crate::types::{EditorTool, SceneBrowserItem};
use crate::ui::courtyard::CourtyardUiState;
use crate::ui::portal_list::*;
use crate::ui::sidebars::{EditorLeftSidebarTab, EditorRightSidebarTab, SceneEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::viewport::SelectedPortalViewportState;
use crate::viewport::content_selection::{LayerCandidateState, LayerTarget};
use crate::viewport::modes::{ViewportMode, ViewportModeState};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_ui::{ActiveNumericInput, NumericInputRoot, NumericInputSetValue};
use feathrise_scene_content::{
    SceneBoundariesAsset, SceneManifestAsset, ScenePortalRecord, ScenePortalTarget, ScenePortalsAsset,
};

#[derive(Resource)]
struct TestFont(Handle<Font>);

fn page(shell: bool) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
    ));
    app.init_resource::<UiSurface>()
        .init_resource::<InputFocus>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<EditorStaticTextInputState>()
        .init_resource::<SceneBrowserState>()
        .init_resource::<SceneBrowserDragState>()
        .init_resource::<SceneDeletionTask>()
        .init_resource::<BoundaryEditState>()
        .init_resource::<LayerCandidateState>()
        .init_resource::<SelectedPortalViewportState>()
        .init_resource::<PortalRowEntityRevision>()
        .init_resource::<ViewportModeState>()
        .init_resource::<CourtyardUiState>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_asset::<SceneManifestAsset>()
        .init_asset::<ScenePortalsAsset>()
        .add_message::<KeyboardInput>()
        .add_message::<NumericInputSetValue>()
        .insert_resource(UiScale(1.0));
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    draft.manifest.display_name = "用于检查中文名称与完整传送列表的场景".into();
    draft.boundaries = Some(
        SceneBoundariesAsset::rectangular_default(&draft.scene_id, [0.0; 2], [10.0; 2], 0.0, 20.0)
            .unwrap(),
    );
    for index in 0..43 {
        draft.portals.portals.push(ScenePortalRecord {
            terrain_binding: None,
            arrival_terrain_binding: None,
            id: format!("portal-{index}"),
            label: format!("已有源点{index}"),
            label_customized: true,
            position: [1.0, 0.0, 1.0],
            arrival: [2.0, 0.0, 2.0],
            target: ScenePortalTarget::default_spawn(draft.scene_id.clone()),
            color_srgba: [1.0, 0.5, 0.1, 1.0],
            arrow_degrees: 0.0,
        });
    }
    let scene_id = draft.scene_id.clone();
    let boundary_id = draft.boundaries.as_ref().unwrap().paths[0].id.clone();
    app.world_mut()
        .resource_mut::<SceneBrowserState>()
        .selected_item = Some(SceneBrowserItem::Scene(scene_id.clone()));
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(scene_id.clone()),
        ..default()
    });
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        selected_boundary_path: Some((scene_id, boundary_id)),
        ..default()
    });
    app.insert_resource(EditorPreviewAssets {
        portal_base_mesh: default(),
        portal_arrow_head_mesh: default(),
        portal_arrow_shaft_mesh: default(),
        portal_edge_beam_mesh: default(),
        portal_edge_ring_mesh: default(),
        portal_base_material: default(),
        portal_arrow_material: default(),
        portal_edge_beam_material: default(),
        portal_edge_ring_material: default(),
        portal_label_font: default(),
    }); // 测试不创建世界预览实体，名称事务仍走原刷新入口。
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
    sidebar.left = EditorLeftSidebarTab::Gameplay;
    app.insert_resource(sidebar);
    app.configure_sets(
        Update,
        (
            SceneEditorSystemSet::Input,
            SceneEditorSystemSet::Tasks,
            SceneEditorSystemSet::Presentation,
        )
            .chain(),
    );
    super::install(&mut app);
    crate::authoring::boundary::objects::install(&mut app);
    app.add_systems(
        Update,
        (
            crate::ui::static_text_input::read_values,
            handle_portal_list_row_clicks,
            handle_portal_name_editor,
            handle_portal_delete_button,
        )
            .chain()
            .in_set(SceneEditorSystemSet::Input),
    );
    app.add_systems(
        Update,
        crate::viewport::modes::sync_mode
            .after(SceneEditorSystemSet::Input)
            .before(SceneEditorSystemSet::Tasks),
    );
    app.add_systems(
        Update,
        (
            crate::authoring::setup_boundary_thickness_input,
            spawn_portal_list_rows,
            sync_portal_selection_panel,
            sync_current_portal_rows,
            sync_target_portal_rows,
            sync_portal_name_editor,
            crate::ui::static_text_input::sync_focus,
            crate::authoring::sync_boundary_text,
        )
            .chain()
            .in_set(SceneEditorSystemSet::Presentation),
    );
    app.add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
        PostUpdate,
    ));
    app.add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
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
    if shell {
        app.add_systems(Startup, crate::ui::sidebars::test_right_sidebar.spawn());
        app.add_systems(
            Update,
            crate::ui::sidebars::sync_editor_sidebar_presentation
                .in_set(SceneEditorSystemSet::Presentation),
        );
    } else {
        app.add_systems(Startup, super::scene::gameplay_tools_panel.spawn());
    }
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.insert_resource(TestFont(font));
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::splat(4096),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::splat(4096),
                ..default()
            }),
            ..default()
        },
    ));
    app.update();
    let world = app.world_mut();
    let root = if shell {
        let root = world
            .query_filtered::<Entity, With<SceneEditorPane>>()
            .single(world)
            .unwrap();
        world.get_mut::<Node>(root).unwrap().width = percent(100);
        root
    } else {
        world
            .query_filtered::<Entity, With<GameplaySidebarRoot>>()
            .single(world)
            .unwrap()
    };
    let host = world
        .spawn(Node {
            width: px(240),
            height: px(350),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    world.entity_mut(root).insert(ChildOf(host));
    settle(&mut app);
    (app, host)
}

fn settle(app: &mut App) {
    for _ in 0..6 {
        app.update();
        let font = app.world().resource::<TestFont>().0.clone();
        let world = app.world_mut();
        for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
            if text_font.font != font.clone().into() {
                text_font.font = font.clone().into();
            }
        }
    }
    app.update(); // 后台补齐列表的新行也使用真实字体完成布局。
}

fn button(app: &mut App, label: &str) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &Children, &UiInteraction)>()
        .iter(world)
        .find_map(|(entity, children, _)| {
            (world
                .get::<ComputedNode>(entity)
                .is_some_and(|node| node.size().x > 0.0)
                && children.iter().any(|child| {
                    world
                        .get::<Text>(child)
                        .is_some_and(|text| text.as_str() == label)
                }))
            .then_some(entity)
        })
        .unwrap_or_else(|| panic!("缺少操作：{label}"))
}

fn click(app: &mut App, entity: Entity) {
    app.world_mut()
        .entity_mut(entity)
        .insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    app.world_mut().entity_mut(entity).insert(UiInteraction::None);
    settle(app);
}

fn press(app: &mut App, label: &str) {
    let entity = button(app, label);
    click(app, entity);
}

fn author(app: &App) -> String {
    let draft = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap();
    serde_json::to_string(&(&draft.placements, &draft.portals, &draft.boundaries)).unwrap()
}

fn feedback(app: &mut App, kind: GameplaySidebarFeedbackKind) -> String {
    let world = app.world_mut();
    world
        .query::<(&GameplaySidebarFeedbackText, &Text)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == kind)
        .map(|(_, text)| text.to_string())
        .unwrap()
}

#[test]
fn tabs_prepare_original_tools_without_entering_edit_or_changing_authors() {
    let (mut app, _) = page(false);
    let before = author(&app);
    for target in [
        GameplaySidebarPage::Portal,
        GameplaySidebarPage::Boundary,
        GameplaySidebarPage::Asset,
    ] {
        press(&mut app, target.label());
        assert_eq!(app.world().resource::<GameplaySidebarState>().page, target);
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        );
        assert_eq!(
            app.world().resource::<ViewportModeState>().mode,
            ViewportMode::Select
        );
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
        app.update();
        assert_eq!(app.world().resource::<SceneEditorState>().tool, target.tool());
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
        app.update();
        assert_eq!(author(&app), before);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.undo.is_empty());
        assert_eq!(session.revision, 0);
    }
    press(&mut app, "打开资产浏览");
    assert_eq!(
        app.world().resource::<SceneEditorSidebarState>().navigator,
        crate::ui::sidebars::EditorNavigatorTab::Assets
    );
    assert_eq!(app.world().resource::<SceneEditorSidebarState>().right, EditorRightSidebarTab::Tools); // 浏览左侧资产不抢右侧工具。
    press(&mut app, "查看实例属性");
    assert_eq!(
        app.world().resource::<SceneEditorSidebarState>().right,
        EditorRightSidebarTab::Scene
    );
}

#[test]
fn selection_follows_stable_objects_but_does_not_reclaim_manual_tabs() {
    let (mut app, _) = page(false);
    let before = author(&app);
    for (target, expected) in [
        (
            LayerTarget::Portal("portal-2".into()),
            GameplaySidebarPage::Portal,
        ),
        (
            LayerTarget::Boundary("wall-1".into()),
            GameplaySidebarPage::Boundary,
        ),
        (
            LayerTarget::Placement("placement-1".into()),
            GameplaySidebarPage::Asset,
        ),
    ] {
        app.world_mut()
            .resource_mut::<LayerCandidateState>()
            .selected = Some(target);
        app.update();
        assert_eq!(
            app.world().resource::<GameplaySidebarState>().page,
            expected
        );
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        );
        press(&mut app, "空气墙");
        app.update();
        assert_eq!(
            app.world().resource::<GameplaySidebarState>().page,
            GameplaySidebarPage::Boundary
        );
    }
    assert_eq!(author(&app), before);
    {
        let mut selection = app.world_mut().resource_mut::<LayerCandidateState>();
        selection.selected = Some(LayerTarget::Portal("portal-2".into()));
        selection.selection_revision += 1;
    }
    app.update();
    press(&mut app, "空气墙");
    app.world_mut()
        .resource_mut::<LayerCandidateState>()
        .selection_revision += 1;
    app.update();
    assert_eq!(
        app.world().resource::<GameplaySidebarState>().page,
        GameplaySidebarPage::Portal
    ); // 同稳定ID的新一次确认能回来，悬停和旧选择不能抢页。
}

#[test]
fn select_mode_keeps_every_portal_row_and_reuses_name_and_delete_transactions() {
    let (mut app, _) = page(false);
    press(&mut app, "传送点");
    let world = app.world_mut();
    let current = world
        .query::<(Entity, &SceneEditorCurrentPortalRow)>()
        .iter(world)
        .map(|(entity, row)| (entity, row.portal_id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(current.len(), 43);
    assert_eq!(
        world
            .query::<&SceneEditorTargetPortalRow>()
            .iter(world)
            .count(),
        44
    );
    let source = current
        .iter()
        .find(|(_, id)| id.as_deref() == Some("portal-17"))
        .unwrap()
        .0;
    click(&mut app, source);
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_current_portal_id
            .as_deref(),
        Some("portal-17")
    );
    press(&mut app, "应用名称"); // 未聚焦不能把只读名称当成新提交。
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    let world = app.world_mut();
    let input = world
        .query_filtered::<Entity, With<SceneEditorPortalNameInput>>()
        .single(world)
        .unwrap();
    click(&mut app, input);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .portal_name_edit
            .active,
        "{}",
        app.world().resource::<SceneEditorState>().status
    );
    let world = app.world_mut();
    let input_text = world
        .query_filtered::<Entity, With<SceneEditorPortalNameInputText>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(input_text)
        .insert(crate::ui::text_input::single_line_input("王府南门", 48));
    app.update();
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .portal_name_edit
            .value,
        "王府南门",
        "官方名称必须先读回原业务缓冲"
    );
    press(&mut app, "应用名称");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.draft.as_ref().unwrap().portals.portals[17].label,
        "王府南门"
    );
    assert_eq!(session.undo.len(), 1);
    assert_eq!(
        app.world().resource::<SceneEditorState>().tool,
        EditorTool::Inspect
    );
    let before = author(&app);
    let world = app.world_mut();
    let target = world
        .query::<(Entity, &SceneEditorTargetPortalRow)>()
        .iter(world)
        .find(|(_, row)| row.slot == 36)
        .unwrap()
        .0;
    click(&mut app, target);
    assert_eq!(author(&app), before); // 选择新点的目标绝不改写已有源点连接。
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .portal_target
            .is_some()
    );
    press(&mut app, "删除源点");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 2);
    assert_eq!(session.draft.as_ref().unwrap().portals.portals.len(), 42);
    assert!(
        !session
            .draft
            .as_ref()
            .unwrap()
            .portals
            .portals
            .iter()
            .any(|portal| portal.id == "portal-17")
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&SceneEditorCurrentPortalRow>()
            .iter(world)
            .count(),
        42
    );
    assert_eq!(
        world
            .query::<&SceneEditorTargetPortalRow>()
            .iter(world)
            .count(),
        43
    );
    let delete = button(&mut app, "删除源点");
    press(&mut app, "资产摆放");
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    click(&mut app, delete); // 模拟隐藏页迟到的Interaction，不能跨页执行删除。
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().revision,
        revision
    );
}

#[test]
fn wall_creation_still_uses_single_wall_history_and_original_fixed_pool() {
    let (mut app, _) = page(false);
    let before = author(&app);
    press(&mut app, "空气墙");
    assert_eq!(author(&app), before);
    press(&mut app, "新建内部墙");
    let session = app.world().resource::<EditorAuthoringSession>();
    let walls = &session
        .draft
        .as_ref()
        .unwrap()
        .boundaries
        .as_ref()
        .unwrap()
        .paths;
    assert_eq!(walls.len(), 2);
    assert_eq!(walls[1].role, feathrise_scene_content::SceneBoundaryRole::Internal);
    assert_eq!(session.undo.len(), 1);
    let world = app.world_mut();
    let session = world.resource_mut::<EditorAuthoringSession>().into_inner();
    let document = session.draft.as_mut().unwrap().boundaries.as_mut().unwrap();
    for index in 2..43 {
        let mut wall = document.paths[1].clone();
        wall.id = format!("wall-test-{index}");
        wall.label = format!("固定行池墙{index}");
        document.paths.push(wall);
    }
    settle(&mut app);
    let world = app.world_mut();
    let count = world
        .query::<&Text>()
        .iter(world)
        .filter(|text| text.contains("碰撞") && text.contains("辅助线"))
        .count();
    assert_eq!(count, 8); // 内容增至43面墙也不增加原八行池。
    assert_eq!(
        world
            .query_filtered::<Entity, With<NumericInputRoot>>()
            .iter(world)
            .count(),
        3
    );
}

#[test]
fn blocked_tab_clicks_are_consumed_instead_of_replayed_after_unlock() {
    let (mut app, _) = page(false);
    let boundary = button(&mut app, "空气墙");
    for blocker in 0..10 {
        match blocker {
            0 => {
                app.world_mut()
                    .resource_mut::<EditorTextInputGuard>()
                    .block_shortcuts = true
            }
            1 => app
                .world_mut()
                .resource_mut::<SceneEditorNumericScrubMouseGuard>()
                .advance(true, true, false),
            2 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .close_requested = true
            }
            3 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
            }
            4 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .scene_transition_in_progress = true
            }
            5 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .saving = true
            }
            6 => {
                let wall = app
                    .world()
                    .resource::<EditorAuthoringSession>()
                    .draft
                    .as_ref()
                    .unwrap()
                    .boundaries
                    .as_ref()
                    .unwrap()
                    .paths[0]
                    .clone();
                let mut drag = app.world_mut().resource_mut::<BoundaryEditState>();
                drag.path_index = Some(0);
                drag.point_index = Some(0);
                drag.before = Some(wall);
            }
            7 => {
                app.world_mut().resource_mut::<SceneEditorSidebarState>().left =
                    EditorLeftSidebarTab::Terrain
            }
            8 => app.world_mut().resource_mut::<SceneEditorState>().open = false,
            9 => app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(boundary),
            _ => unreachable!(),
        }
        app.world_mut()
            .entity_mut(boundary)
            .insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update();
        assert_eq!(
            app.world().resource::<GameplaySidebarState>().page,
            GameplaySidebarPage::Asset,
            "阻塞{blocker}"
        );
        app.world_mut()
            .resource_mut::<EditorTextInputGuard>()
            .block_shortcuts = false;
        app.world_mut()
            .insert_resource(SceneEditorNumericScrubMouseGuard::default());
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.close_requested = false;
            editor.new_scene_dialog_open = false;
            editor.scene_transition_in_progress = false;
            editor.open = true;
        }
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .saving = false;
        app.world_mut()
            .resource_mut::<BoundaryEditState>()
            .clear_drag();
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left =
            EditorLeftSidebarTab::Gameplay;
        app.world_mut().resource_mut::<ActiveNumericInput>().entity = None;
        app.update(); // Interaction仍是Pressed，解锁帧不得延迟切页。
        assert_eq!(
            app.world().resource::<GameplaySidebarState>().page,
            GameplaySidebarPage::Asset,
            "旧点击重放{blocker}"
        );
        app.world_mut()
            .entity_mut(boundary)
            .insert(UiInteraction::None);
        app.update();
    }
    press(&mut app, "空气墙");
    assert_eq!(
        app.world().resource::<GameplaySidebarState>().page,
        GameplaySidebarPage::Boundary
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[path = "layout_tests.rs"]
mod layout;
