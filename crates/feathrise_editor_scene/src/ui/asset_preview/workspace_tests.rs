//! 中央图片/模型标签的真实ECS交互、父关系和切页回归；GPU画面另由离屏用例验证。

use feathrise_editor_ui::UiInteraction;
use super::*;
use bevy::asset::AssetPlugin;
use bevy::ecs::system::RunSystemOnce;

fn fixture() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_resource::<SceneEditorState>()
        .init_resource::<SceneEditorSidebarState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<AssetPreviewWorkspace>()
        .init_resource::<AssetPreviewFocus>()
        .init_resource::<AssetPreviewNavigation>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<crate::game_authoring::ExternalAssetPreviewRequest>()
        .add_systems(
            Update,
            (
                actions,
                sync_activity,
                super::super::mounting::mount_asset_preview_panel,
            )
                .chain(),
        );
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("textures/preview.png".into());
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    let mut state = app.world_mut().resource_mut::<AssetPreviewState>();
    state.selected_path = Some("textures/preview.png".into());
    state.content = AssetPreviewContent::Image {
        handle: Handle::default(),
        phase: AssetPreviewPhase::Ready,
    };
    let sidebar = app.world_mut().spawn(Node::default()).id();
    let center = app
        .world_mut()
        .spawn((AssetPreviewCenterHost, Node::default()))
        .id();
    let panel = app
        .world_mut()
        .spawn((AssetPreviewPanel, Node::default(), ChildOf(sidebar)))
        .id();
    app.update(); // 挂载系统先记录侧栏原父级，与正式静态壳一致。
    (app, panel, sidebar, center)
}

fn press(app: &mut App, action: AssetPreviewWorkspaceAction) {
    let button = app
        .world_mut()
        .spawn((
            feathrise_editor_ui::EditorButtonClick::triggered(),
            UiInteraction::Pressed,
            action,
        ))
        .id();
    app.update();
    app.world_mut().entity_mut(button).despawn();
}

#[test]
fn asset_center_expand_restore_keeps_panel_view_and_file_selection() {
    let (mut app, panel, sidebar, center) = fixture();
    let view = super::super::view_math::AssetPreviewView {
        zoom: 2.5,
        image_pan: Vec2::new(0.2, -0.3),
        ..default()
    };
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view = view;
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), center);
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::AssetPreview
    );
    assert_eq!(app.world().resource::<AssetPreviewNavigation>().view, view);
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .view,
        view
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("textures/other.png".into());
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .reveal_scene_properties();
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .path,
        "textures/preview.png"
    );
    press(&mut app, AssetPreviewWorkspaceAction::RestoreSidebar);
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), sidebar);
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Scene
    );
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_asset
            .as_deref(),
        Some("textures/preview.png")
    );
    assert_eq!(app.world().resource::<AssetPreviewNavigation>().view, view);
    {
        let mut state = app.world_mut().resource_mut::<SceneEditorSidebarState>();
        assert_eq!(
            state.inspector,
            crate::ui::sidebars::EditorInspectorTarget::Asset
        );
        state.right = EditorRightSidebarTab::Tools;
        state.reveal_properties();
        assert_eq!(state.right, EditorRightSidebarTab::Assets);
    }
}

#[test]
fn asset_center_closed_return_tab_and_hidden_sidebar_fall_back_safely() {
    let (mut app, panel, sidebar, _) = fixture();
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Vegetation(999);
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    app.insert_resource(SceneEditorLayoutState::default());
    app.world_mut()
        .resource_mut::<SceneEditorLayoutState>()
        .inspector_visible = false;
    press(&mut app, AssetPreviewWorkspaceAction::RestoreSidebar);
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Scene
    );
    assert!(
        app.world()
            .resource::<SceneEditorLayoutState>()
            .inspector_visible
    );
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), sidebar);
}

#[test]
fn asset_center_inactive_tab_close_does_not_steal_author_page_or_selection() {
    let (mut app, panel, sidebar, _) = fixture();
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    app.world_mut()
        .resource_mut::<AssetPreviewWorkspace>()
        .restore_view = false;
    let view = super::super::view_math::AssetPreviewView {
        zoom: 3.0,
        ..default()
    };
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view = view;
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Scene;
    app.update();
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .view,
        view
    );
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), sidebar);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("config/another.json".into());
    press(&mut app, AssetPreviewWorkspaceAction::RestoreSidebar);
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Scene
    );
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_asset
            .as_deref(),
        Some("config/another.json")
    );
    assert!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .is_none()
    );
}

#[test]
fn asset_center_modal_and_document_selection_cannot_replace_media_tab() {
    let (mut app, _, _, _) = fixture();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .new_scene_dialog_open = true;
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    assert!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .is_none()
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .new_scene_dialog_open = false;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("config/test.json".into());
    press(&mut app, AssetPreviewWorkspaceAction::OpenSelected);
    assert!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .is_none()
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("gltf/tree.glb".into());
    press(&mut app, AssetPreviewWorkspaceAction::OpenSelected);
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .path,
        "gltf/tree.glb"
    );
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .view
            .zoom,
        1.0
    );
}

#[test]
fn asset_center_reopening_same_sidebar_file_uses_latest_view() {
    let (mut app, _, _, _) = fixture();
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Scene;
    app.update();
    let view = super::super::view_math::AssetPreviewView {
        zoom: 4.0,
        ..default()
    };
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view = view;
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .view,
        view
    );
}

#[test] // 目录随源文件规模增长，恢复不能在一帧搜索全部候选；热更缩短也不能越界。
fn asset_center_primitive_restore_obeys_budget_and_missing_target_fallback() {
    let (mut app, panel, _, _) = fixture();
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    let missing = AssetPreviewPrimitiveId {
        mesh: 999,
        primitive: 0,
    };
    {
        let mut media = app.world_mut().resource_mut::<AssetPreviewWorkspace>();
        let entry = media.expanded.as_mut().unwrap();
        entry.target = AssetPreviewModelTarget::Primitive(missing);
        entry.view.zoom = 5.0;
        media.restore_view = true;
    }
    let mut model = AssetPreviewModelSession::new(1, Handle::default());
    model.catalog.phase = AssetPreviewCatalogPhase::Ready;
    model.catalog.choices = (0..130)
        .map(|mesh| AssetPreviewPrimitiveChoice {
            id: AssetPreviewPrimitiveId { mesh, primitive: 0 },
            material: None,
            requires_skinning: false,
        })
        .collect();
    {
        let mut state = app.world_mut().resource_mut::<AssetPreviewState>();
        state.model = Some(model);
        state.content = AssetPreviewContent::Model {
            root: panel,
            handle: Handle::default(),
            phase: AssetPreviewPhase::Ready,
        };
    }
    app.world_mut().run_system_once(restore_navigation).unwrap();
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .restore_cursor,
        64
    );
    assert!(app.world().resource::<AssetPreviewWorkspace>().restore_view);
    app.world_mut().run_system_once(restore_navigation).unwrap();
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .restore_cursor,
        128
    );
    app.world_mut()
        .resource_mut::<AssetPreviewState>()
        .model
        .as_mut()
        .unwrap()
        .catalog
        .choices
        .truncate(3);
    app.world_mut().run_system_once(restore_navigation).unwrap();
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .target,
        AssetPreviewModelTarget::Scene0
    );
    app.world_mut().run_system_once(restore_navigation).unwrap();
    assert!(!app.world().resource::<AssetPreviewWorkspace>().restore_view);
    assert_eq!(
        app.world().resource::<AssetPreviewNavigation>().view.zoom,
        1.0
    );
}

#[test] // 源文件失败仍保留精确诊断，只把唯一文字回显移到底栏，不保留空白卡。
fn asset_center_load_failure_keeps_bottom_diagnostic_while_restore_is_pending() {
    let (mut app, _, _, _) = fixture();
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    let error = "图片加载失败：文件不存在";
    {
        let mut state = app.world_mut().resource_mut::<AssetPreviewState>();
        state.content = AssetPreviewContent::Image {
            handle: Handle::default(),
            phase: AssetPreviewPhase::Failed,
        };
        state.status = error.into();
    }
    let placeholder = app
        .world_mut()
        .spawn((AssetPreviewPlaceholder, Node::default(), Text::new("")))
        .id();
    let status = app
        .world_mut()
        .spawn((
            AssetPreviewStatusText,
            crate::ui::status_bar::PanelStatusText("资源预览"),
            Text::new(""),
            Node {
                display: Display::None,
                ..default()
            },
        ))
        .id();
    let canvas = app
        .world_mut()
        .spawn((AssetPreviewCanvas, Node::default()))
        .id();
    app.world_mut()
        .run_system_once(super::super::presentation::sync_asset_preview_presentation)
        .unwrap();
    assert_eq!(app.world().get::<Text>(status).unwrap().as_str(), error);
    assert_eq!(
        app.world().get::<Node>(placeholder).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Node>(canvas).unwrap().display,
        Display::None
    );
    assert!(
        app.world()
            .get::<crate::ui::status_bar::PanelStatusText>(status)
            .is_some()
    );
    assert!(app.world().resource::<AssetPreviewWorkspace>().restore_view);
}

#[test] // 尺寸来自已采用Image，复制回执不覆盖属性；选择换代也不能借用旧文件尺寸。
fn asset_image_metadata_keeps_real_dimensions_and_rejects_another_selected_file() {
    let (mut app, _, _, _) = fixture();
    let mut image = Image::default();
    image.resize(bevy::render::render_resource::Extent3d {
        width: 64,
        height: 32,
        depth_or_array_layers: 1,
    });
    let mut images = Assets::<Image>::default();
    let handle = images.add(image);
    app.insert_resource(images);
    {
        let mut state = app.world_mut().resource_mut::<AssetPreviewState>();
        state.content = AssetPreviewContent::Image {
            handle,
            phase: AssetPreviewPhase::Ready,
        };
        state.status = "已复制引用".into();
    }
    let metadata = app
        .world_mut()
        .spawn((AssetPreviewMetadataText, Text::new(""), Node::default()))
        .id();
    app.world_mut()
        .run_system_once(super::super::presentation::sync_asset_preview_presentation)
        .unwrap();
    assert!(
        app.world()
            .get::<Text>(metadata)
            .unwrap()
            .contains("64 × 32 px")
    );
    assert_eq!(
        app.world().resource::<AssetPreviewState>().status,
        "已复制引用"
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("other.png".into());
    app.world_mut()
        .run_system_once(super::super::presentation::sync_asset_preview_presentation)
        .unwrap();
    assert!(app.world().get::<Text>(metadata).unwrap().is_empty());
    assert_eq!(
        app.world().get::<Node>(metadata).unwrap().display,
        Display::None
    );
}

#[test]
fn asset_multiple_media_tabs_keep_views_and_close_only_the_requested_file() {
    let (mut app, _, _, _) = fixture();
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    let first = app
        .world()
        .resource::<AssetPreviewWorkspace>()
        .expanded
        .as_ref()
        .unwrap()
        .id;
    app.world_mut()
        .resource_mut::<AssetPreviewWorkspace>()
        .restore_view = false;
    let first_snap = feathrise_editor_core::EditorSnapSettings {
        enabled: true,
        show_grid: true,
        mode: feathrise_editor_core::EditorSnapMode::Relative,
        translation_meters: 0.025,
        ..default()
    };
    {
        let mut navigation = app.world_mut().resource_mut::<AssetPreviewNavigation>();
        navigation.view.zoom = 2.5;
        navigation.view.snapping = first_snap;
    }
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("textures/second.png".into());
    press(&mut app, AssetPreviewWorkspaceAction::OpenSelected);
    let second = app
        .world()
        .resource::<AssetPreviewWorkspace>()
        .expanded
        .as_ref()
        .unwrap()
        .id;
    let new_tab_snap = super::super::view_math::AssetPreviewView::default().snapping;
    assert_eq!(
        app.world().resource::<AssetPreviewWorkspace>().expanded.as_ref().unwrap().view.snapping,
        new_tab_snap
    ); // 新标签不能继承上一文件的吸附和参考网格。
    assert_eq!(
        app.world().resource::<AssetPreviewWorkspace>().parked[0]
            .view
            .zoom,
        2.5
    );
    app.world_mut()
        .resource_mut::<AssetPreviewState>()
        .selected_path = Some("textures/second.png".into());
    app.world_mut()
        .resource_mut::<AssetPreviewWorkspace>()
        .restore_view = false;
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view
        .zoom = 4.0;
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view
        .snapping = new_tab_snap;
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Document;
    app.world_mut().run_system_once(sync_activity).unwrap();
    press(&mut app, AssetPreviewWorkspaceAction::ActivateTab(first));
    {
        let media = app.world().resource::<AssetPreviewWorkspace>();
        assert_eq!(media.tabs().count(), 2);
        assert_eq!(media.expanded.as_ref().unwrap().id, first);
        assert_eq!(media.expanded.as_ref().unwrap().view.zoom, 2.5);
        assert_eq!(media.expanded.as_ref().unwrap().view.snapping, first_snap);
        assert_eq!(
            media.expanded.as_ref().unwrap().return_tab,
            PresetWorkspaceTab::Document,
            "从文档页激活旧媒体标签应返回本次来源，不能沿用首次打开时的来源"
        );
        assert_eq!(media.parked[0].view.zoom, 4.0);
        assert_eq!(media.parked[0].view.snapping, new_tab_snap);
    }
    app.world_mut()
        .resource_mut::<AssetPreviewState>()
        .selected_path = Some("textures/preview.png".into());
    app.world_mut().run_system_once(restore_navigation).unwrap();
    assert_eq!(app.world().resource::<AssetPreviewNavigation>().snapping(), first_snap);
    assert_eq!(
        app.world().resource::<AssetPreviewNavigation>().view.zoom,
        2.5
    );
    press(&mut app, AssetPreviewWorkspaceAction::CloseTab(second));
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .tabs()
            .count(),
        1
    );
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .id,
        first
    );
    assert_eq!(
        app.world().resource::<AssetPreviewNavigation>().view.zoom,
        2.5
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("textures/third.png".into());
    press(&mut app, AssetPreviewWorkspaceAction::OpenSelected);
    let third = app
        .world()
        .resource::<AssetPreviewWorkspace>()
        .expanded
        .as_ref()
        .unwrap()
        .id;
    press(&mut app, AssetPreviewWorkspaceAction::CloseTab(third));
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .id,
        first
    );
    assert!(app.world().resource::<AssetPreviewWorkspace>().restore_view);
}

#[test]
fn asset_multiple_media_tabs_have_a_fixed_metadata_budget_and_reuse_at_capacity() {
    let (mut app, _, _, _) = fixture();
    let mut media = app
        .world_mut()
        .remove_resource::<AssetPreviewWorkspace>()
        .unwrap();
    let mut workspace = VegetationPresetEditorState::default();
    let state = AssetPreviewState::default();
    let navigation = AssetPreviewNavigation::default();
    for index in 0..MAX_MEDIA_TABS {
        expand(
            &mut media,
            &mut workspace,
            &format!("image-{index}.png"),
            &state,
            &navigation,
        )
        .unwrap();
    }
    assert_eq!(media.tabs().count(), MAX_MEDIA_TABS);
    assert!(
        expand(
            &mut media,
            &mut workspace,
            "overflow.png",
            &state,
            &navigation
        )
        .is_err()
    );
    expand(
        &mut media,
        &mut workspace,
        "image-0.png",
        &state,
        &navigation,
    )
    .unwrap();
    assert_eq!(media.tabs().count(), MAX_MEDIA_TABS);
    assert_eq!(media.expanded.as_ref().unwrap().path, "image-0.png");
}

#[test]
fn asset_audio_media_tabs_preserve_independent_positions_and_use_the_existing_panel() {
    let (mut app, panel, _, center) = fixture();
    assert!(media_path("audio/剑声.OGG"));
    assert!(media_path("audio/music.mp3"));
    assert!(!media_path("config/music.json"));
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("audio/剑声.OGG".into());
    press(&mut app, AssetPreviewWorkspaceAction::OpenSelected);
    let first = app
        .world()
        .resource::<AssetPreviewWorkspace>()
        .expanded
        .as_ref()
        .unwrap()
        .id;
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .audio_position,
        0.0
    );
    app.world_mut()
        .resource_mut::<AssetPreviewWorkspace>()
        .expanded
        .as_mut()
        .unwrap()
        .audio_position = 12.25;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("audio/music.mp3".into());
    press(&mut app, AssetPreviewWorkspaceAction::OpenSelected);
    let second = app
        .world()
        .resource::<AssetPreviewWorkspace>()
        .expanded
        .as_ref()
        .unwrap()
        .id;
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .audio_position,
        0.0
    );
    app.world_mut()
        .resource_mut::<AssetPreviewWorkspace>()
        .expanded
        .as_mut()
        .unwrap()
        .audio_position = 3.5;
    press(&mut app, AssetPreviewWorkspaceAction::ActivateTab(first));
    let media = app.world().resource::<AssetPreviewWorkspace>();
    assert_eq!(media.expanded.as_ref().unwrap().audio_position, 12.25);
    assert_eq!(
        media
            .parked
            .iter()
            .find(|tab| tab.id == second)
            .unwrap()
            .audio_position,
        3.5
    );
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), center);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<AssetPreviewPanel>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<AssetPreviewCamera>>()
            .iter(world)
            .count(),
        0
    );
}

#[test]
fn asset_audio_restore_waits_for_complete_state_then_exposes_ready_or_failed_player() {
    let (mut app, _, _, _) = fixture();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("audio/剑声.wav".into());
    press(&mut app, AssetPreviewWorkspaceAction::OpenSelected);
    {
        let mut state = app.world_mut().resource_mut::<AssetPreviewState>();
        state.selected_path = Some("audio/剑声.wav".into());
        state.content = AssetPreviewContent::Audio {
            phase: AssetPreviewPhase::Loading,
        };
    }
    app.world_mut().run_system_once(restore_navigation).unwrap();
    assert!(app.world().resource::<AssetPreviewWorkspace>().restore_view);
    for phase in [AssetPreviewPhase::Ready, AssetPreviewPhase::Failed] {
        app.world_mut()
            .resource_mut::<AssetPreviewWorkspace>()
            .restore_view = true;
        app.world_mut().resource_mut::<AssetPreviewState>().content =
            AssetPreviewContent::Audio { phase };
        app.world_mut().run_system_once(restore_navigation).unwrap();
        assert!(!app.world().resource::<AssetPreviewWorkspace>().restore_view);
        assert_eq!(
            super::super::presentation::phase_label(
                app.world().resource::<AssetPreviewState>(),
                phase == AssetPreviewPhase::Failed
            ),
            if phase == AssetPreviewPhase::Failed {
                "加载失败"
            } else {
                "已就绪"
            }
        );
    }
}

#[test]
fn asset_audio_hides_image_model_controls_and_generic_placeholder() {
    let (mut app, _, _, _) = fixture();
    app.world_mut().resource_mut::<AssetPreviewState>().content = AssetPreviewContent::Audio {
        phase: AssetPreviewPhase::Ready,
    };
    let placeholder = app
        .world_mut()
        .spawn((
            AssetPreviewPlaceholder,
            Node::default(),
            Text::new("旧模型"),
            TextColor::default(),
        ))
        .id();
    let controls = app
        .world_mut()
        .spawn((AssetPreviewControls, Node::default()))
        .id();
    let hint = app
        .world_mut()
        .spawn((
            AssetPreviewNavigationHint,
            Node::default(),
            Text::new("旧模型操作"),
        ))
        .id();
    app.world_mut()
        .run_system_once(super::super::presentation::sync_asset_preview_presentation)
        .unwrap();
    app.world_mut()
        .run_system_once(super::super::presentation::sync_asset_preview_controls)
        .unwrap();
    for entity in [placeholder, controls, hint] {
        assert_eq!(
            app.world().get::<Node>(entity).unwrap().display,
            Display::None
        );
    }
    assert!(app.world().get::<Text>(hint).unwrap().0.is_empty());
}
