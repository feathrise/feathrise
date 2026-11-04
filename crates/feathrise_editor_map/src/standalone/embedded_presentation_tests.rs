//! 执行原F12、壳体与相机系统，验证原生预览期间工作台、输入现场和作者历史保持。

use super::*;
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand, WaterNetworkEditState},
    components::{MapEditorCamera, MapEditorRoot},
    resources::{
        EditorAssetCatalog, MapEditorLayoutState, MapEditorNumericScrubMouseGuard,
        MapEditorViewportState, MapEditorState,
    },
    standalone::{RunSavedProject, systems::publish_saved_play},
    ui::{
        gltf_browser::EditorGltfBrowserState,
        systems::{sync_editor_shell, sync_map_editor_blocker, toggle_map_editor},
    },
};
use bevy::{
    ecs::message::MessageCursor,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{FocusCause, InputFocus},
    window::PrimaryWindow,
};
use feathrise_map_content::{
    ActiveDynamicMap, MapCatalogAsset, MapId, MapManifestAsset, MapPlacementRecord,
};
use feathrise_scene_core::state::{EditorStartup, ScenePhase};

fn fixture() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin)
        .init_resource::<StandaloneEditorMode>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<feathrise_scene_core::input::InputGateState>()
        .init_resource::<feathrise_scene_core::input::EditorInputState>()
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<Assets<MapCatalogAsset>>()
        .init_resource::<Assets<MapManifestAsset>>()
        .init_resource::<MapEditorLayoutState>()
        .init_resource::<InputFocus>()
        .insert_resource(EditorStartup)
        .insert_resource(State::new(ScenePhase::Idle))
        .insert_resource(ActiveDynamicMap {
            map_id: Some(MapId::parse("MAP-local-game").unwrap()),
        })
        .add_message::<RunSavedProject>()
        .add_systems(
            Update,
            (
                toggle_map_editor,
                publish_saved_play,
                sync_map_editor_blocker,
                sync_editor_shell,
                crate::viewport::sync_editor_camera_activity,
            )
                .chain(),
        );
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let map = draft.map_id.clone();
    let record: MapPlacementRecord = serde_json::from_value(serde_json::json!({
        "id": "selection-kept", "asset_path": "fixture.glb", "translation": [1.0, 2.0, 3.0],
        "yaw_degrees": 0.0, "scale": [1.0, 1.0, 1.0]
    }))
    .unwrap();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        revision: 17,
        undo: vec![EditorCommand::PlacementAdded(record.clone())],
        redo: vec![EditorCommand::PlacementAdded(record)],
        ..default()
    })
    .insert_resource(MapEditorState {
        open: true,
        dirty: true,
        selected_map_id: Some(map.clone()),
        map_list_selection_id: Some(map.clone()),
        ..default()
    })
    .init_resource::<EditorGltfBrowserState>();
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selected_placement_id = Some("selection-kept".into());
    let root = app
        .world_mut()
        .spawn((MapEditorRoot, Node::default(), Visibility::Visible))
        .id();
    let pose = Transform::from_xyz(3.0, 7.0, 11.0).looking_at(Vec3::ZERO, Vec3::Y);
    let camera = app
        .world_mut()
        .spawn((MapEditorCamera, Camera::default(), pose))
        .id();
    let focus = app.world_mut().spawn(Node::default()).id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(focus, FocusCause::Navigated);
    app.world_mut().spawn((
        PrimaryWindow,
        Window {
            ime_enabled: true,
            ..default()
        },
    ));
    app.insert_resource(MapEditorViewportState {
        _image: Handle::default(),
        camera,
        center: Vec3::new(4.0, 1.0, 2.0),
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::ONE,
        extent: 91.0,
        yaw: 0.37,
        pitch: 0.42,
        framed_map_id: Some(map),
        render_size: UVec2::new(900, 600),
    });
    super::super::embedded_input::install(&mut app);
    (app, root, camera, focus)
}

fn f12(app: &mut App) {
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::F12,
        logical_key: Key::F12,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::F12,
        logical_key: Key::F12,
        state: ButtonState::Released,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    app.update();
}

fn assert_shell(app: &App, root: Entity, camera: Entity, visible: bool) {
    assert_eq!(
        app.world().get::<Node>(root).unwrap().display,
        if visible {
            Display::Flex
        } else {
            Display::None
        }
    );
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    );
    assert_eq!(
        app.world().get::<Camera>(camera).unwrap().is_active,
        visible
    );
}

#[test]
fn project_preview_f12_cannot_enter_or_return_through_the_original_editor_input_system() {
    let (mut app, root, camera, _) = fixture();
    app.insert_resource(feathrise_editor_core::ProjectLaunchSettings { preview: true, ..default() });
    app.update();
    f12(&mut app);
    assert!(app.world().resource::<Messages<RunSavedProject>>().is_empty());
    assert_shell(&app, root, camera, true);
    set_embedded_playing(app.world_mut(), true);
    app.world_mut().resource_mut::<StandaloneEditorMode>().set_external_play_active(true);
    app.update();
    f12(&mut app);
    assert!(app.world().resource::<Messages<RunSavedProject>>().is_empty());
    assert_shell(&app, root, camera, true);
    assert!(app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn native_preview_keeps_original_shell_camera_focus_and_f12_return_without_resetting_author() {
    let (mut app, root, camera, focus) = fixture();
    app.update();
    let map = app
        .world()
        .resource::<MapEditorState>()
        .selected_map_id
        .clone();
    let pose = *app.world().get::<Transform>(camera).unwrap();
    let layout = app.world().resource::<MapEditorLayoutState>().preferences();
    let mut requests = MessageCursor::<RunSavedProject>::default();
    f12(&mut app);
    assert_eq!(
        requests
            .read(app.world().resource::<Messages<RunSavedProject>>())
            .filter(|event| !event.stop_only)
            .count(),
        1
    );
    assert_shell(&app, root, camera, true); // 发送请求不等于游戏确认进入试玩。

    app.world_mut()
        .resource_mut::<StandaloneEditorMode>()
        .set_external_play_active(true);
    set_embedded_playing(app.world_mut(), true);
    for _ in 0..4 {
        set_embedded_playing(app.world_mut(), true); // 逐帧模式事实不能修改作者现场。
        app.update();
        assert_shell(&app, root, camera, true);
        assert!(app.world().contains_resource::<EditorStartup>());
        assert!(
            app.world()
                .resource::<feathrise_scene_core::input::EditorInputState>()
                .map_editor_open
        );
    }
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(focus));
    assert!(
        app.world_mut()
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap()
            .ime_enabled
    );
    f12(&mut app);
    assert_eq!(
        requests
            .read(app.world().resource::<Messages<RunSavedProject>>())
            .filter(|event| event.stop_only)
            .count(),
        1
    );
    assert_shell(&app, root, camera, true); // 返回请求不改变界面，模式仍等待实际回执。
    assert_eq!(
        app.world().resource::<MapEditorState>().selected_map_id,
        map
    );

    set_embedded_playing(app.world_mut(), false);
    app.world_mut()
        .resource_mut::<StandaloneEditorMode>()
        .set_external_play_active(false);
    app.update();
    assert_shell(&app, root, camera, true);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(focus));
    let editor = app.world().resource::<MapEditorState>();
    assert_eq!(editor.selected_map_id, map);
    assert_eq!(editor.map_list_selection_id, map);
    assert!(editor.dirty && !editor.save_requested && !editor.close_requested);
    assert_eq!(
        app.world().resource::<MapEditorLayoutState>().preferences(),
        layout
    );
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), pose);
    let viewport = app.world().resource::<MapEditorViewportState>();
    assert_eq!(
        (
            viewport.center,
            viewport.extent,
            viewport.yaw,
            viewport.pitch
        ),
        (Vec3::new(4.0, 1.0, 2.0), 91.0, 0.37, 0.42)
    );
    assert_eq!(
        app.world()
            .resource::<EditorGltfBrowserState>()
            .selected_placement_id
            .as_deref(),
        Some("selection-kept")
    );
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        (session.revision, session.undo.len(), session.redo.len()),
        (17, 1, 1)
    );
    assert_eq!(
        session.draft.as_ref().map(|draft| draft.map_id.clone()),
        map
    );
    assert!(!session.saving);
}

#[test]
fn saved_world_f12_keeps_author_shell_and_embedded_failure_restores_it() {
    let (mut app, root, camera, _) = fixture();
    f12(&mut app);
    app.world_mut()
        .resource_mut::<StandaloneEditorMode>()
        .set_external_play_active(true);
    set_embedded_playing(app.world_mut(), false); // SavedWorld普通逐帧更新没有界面副作用。
    f12(&mut app);
    assert_shell(&app, root, camera, true);
    assert!(
        !app.world()
            .resource::<StandaloneEditorMode>()
            .embedded_playing()
    );
    set_embedded_playing(app.world_mut(), true);
    app.update();
    assert_shell(&app, root, camera, true);
    set_embedded_playing(app.world_mut(), false); // 进程失败仍归还现有作者现场，不要求另一份游戏回执。
    app.update();
    assert_shell(&app, root, camera, true);
}

#[test]
fn returning_from_native_preview_keeps_the_latest_editor_focus_and_view_instead_of_old_snapshot() {
    let (mut app, root, camera, _) = fixture();
    app.update();
    set_embedded_playing(app.world_mut(), true);
    let next_focus = app.world_mut().spawn(Node::default()).id();
    app.world_mut().resource_mut::<InputFocus>().set(next_focus, FocusCause::Navigated);
    let next_pose = Transform::from_xyz(12., 8., 4.);
    *app.world_mut().get_mut::<Transform>(camera).unwrap() = next_pose;
    app.world_mut().resource_mut::<MapEditorViewportState>().yaw = 1.2;
    set_embedded_playing(app.world_mut(), true);
    app.update();
    assert_shell(&app, root, camera, true);
    set_embedded_playing(app.world_mut(), false);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(next_focus));
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), next_pose);
    assert_eq!(app.world().resource::<MapEditorViewportState>().yaw, 1.2);
    assert_shell(&app, root, camera, true);
}

#[test]
fn embedded_play_real_multi_selection_survives_f12_until_actual_close() {
    use crate::viewport::{
        modes::{ViewportMode, ViewportModeState, sync_mode},
        multi_select::{PlacementSelectionSet, advance_multi_selection},
        placement_move::{handle_placement_move, interaction_tests::batch_preview_fixture},
    };
    use std::sync::Arc;

    let (mut app, _, _, _) = batch_preview_fixture();
    let map = app.world().resource::<MapEditorState>().selected_map_id.clone();
    app.add_plugins(bevy::input::InputPlugin)
        .init_resource::<StandaloneEditorMode>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<feathrise_scene_core::input::InputGateState>()
        .init_resource::<crate::MapEditorSidebarState>()
        .insert_resource(State::new(ScenePhase::Idle))
        .insert_resource(ActiveDynamicMap { map_id: map.clone() })
        .add_message::<RunSavedProject>()
        .add_systems(Update, (toggle_map_editor, publish_saved_play).chain().before(handle_placement_move))
        .add_systems(Update, (sync_mode, advance_multi_selection).chain().after(handle_placement_move));
    super::super::embedded_input::install(&mut app);
    app.update();
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Move;
    app.update();
    assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Move);
    let original = app.world().resource::<PlacementSelectionSet>();
    assert_eq!(original.members.len(), 2);
    let members = original.members.clone();
    let identity = (original.map.clone(), original.revision, original.center);
    let assert_group = |world: &World| {
        let selection = world.resource::<PlacementSelectionSet>();
        assert!(Arc::ptr_eq(&selection.members, &members), "呈现往返不能清空或重建原完整组选择");
        assert_eq!((selection.map.clone(), selection.revision, selection.center), identity);
        assert!(selection.ready_for(world.resource::<EditorAuthoringSession>()));
        assert_eq!(world.resource::<MapEditorState>().selected_map_id, map);
        assert!(world.resource::<EditorAuthoringSession>().undo.is_empty());
    };
    assert_group(app.world());

    let mut requests = MessageCursor::<RunSavedProject>::default();
    f12(&mut app); // 真实原入口只发请求；没有游戏回执前仍保留原作者呈现。
    assert_eq!(requests.read(app.world().resource::<Messages<RunSavedProject>>())
        .filter(|event| !event.stop_only).count(), 1);
    app.world_mut().resource_mut::<StandaloneEditorMode>().set_external_play_active(true);
    set_embedded_playing(app.world_mut(), true);
    for _ in 0..4 {
        app.update(); // 原多选系统实际运行，不能用停调度掩盖错误的关闭清理。
        assert!(app.world().resource::<MapEditorState>().open);
        assert_group(app.world());
    }
    f12(&mut app);
    assert_eq!(requests.read(app.world().resource::<Messages<RunSavedProject>>())
        .filter(|event| event.stop_only).count(), 1);
    assert_group(app.world());
    set_embedded_playing(app.world_mut(), false);
    app.world_mut().resource_mut::<StandaloneEditorMode>().set_external_play_active(false);
    app.update();
    assert!(app.world().resource::<MapEditorState>().open);
    assert_group(app.world());
    // 没有关闭/重开作者会话，返回不能擅自切换作者原来选择的变换工具。
    assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Move);
    assert!(app.world().resource::<MapEditorState>().placement_transform_enabled);

    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(selection.members.is_empty() && selection.map.is_none() && !selection.ready);
}

#[test]
fn embedded_play_real_browser_cache_preserves_selection_expansion_and_index_until_actual_close() {
    use crate::ui::gltf_browser::{
        EditorGltfBrowserEntryKind, EditorGltfMeshSelection, refresh_gltf_browser_cache,
    };
    use bevy::{
        asset::{
            AssetApp, AssetLoader, AssetPlugin, LoadContext,
            io::{
                AssetSourceBuilder, AssetSourceId, Reader,
                memory::{Dir, MemoryAssetReader},
            },
        },
        world_serialization::WorldAsset,
    };
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    #[derive(TypePath)]
    struct BrowserSceneFixtureLoader;
    impl AssetLoader for BrowserSceneFixtureLoader {
        type Asset = WorldAsset;
        type Settings = ();
        type Error = std::io::Error;
        async fn load(
            &self,
            _reader: &mut dyn Reader,
            _: &(),
            context: &mut LoadContext<'_>,
        ) -> Result<WorldAsset, Self::Error> {
            // 真正交给AssetServer加载的最小Scene0；稳定节点/网格身份由原缓存DFS自行验证。
            let mesh = context.add_labeled_asset("Mesh0/Primitive0", Mesh::from(Cuboid::default()));
            let mut world = World::new();
            let root = world.spawn(Name::new("Scene0")).id();
            let node = world
                .spawn((Name::new("original-node"), ChildOf(root)))
                .id();
            world.spawn((
                ChildOf(node),
                Mesh3d(mesh),
                bevy::gltf::GltfMeshName("original-mesh".into()),
            ));
            context.add_labeled_asset("Scene0", WorldAsset::new(world));
            Ok(WorldAsset::new(World::new()))
        }
        fn extensions(&self) -> &[&str] {
            &["scene_fixture"]
        }
    }

    let (mut app, root, camera, _) = fixture();
    let directory = Dir::default();
    directory.insert_asset_text(
        Path::new("fixture.scene_fixture"),
        "owned browser scene fixture",
    );
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || {
            Box::new(MemoryAssetReader {
                root: directory.clone(),
            })
        }),
    )
    .add_plugins((MinimalPlugins, AssetPlugin::default()))
    .init_asset::<WorldAsset>()
    .init_asset::<Mesh>()
    .register_asset_loader(BrowserSceneFixtureLoader)
    .add_systems(Update, refresh_gltf_browser_cache.after(sync_editor_shell));
    let mut record = match &app.world().resource::<EditorAuthoringSession>().undo[0] {
        EditorCommand::PlacementAdded(record) => record.clone(),
        _ => unreachable!(),
    };
    record.asset_path = "fixture.scene_fixture".into();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements
        .push(record);
    for _ in 0..4 {
        app.update();
    }
    let mesh = EditorGltfMeshSelection {
        placement_id: "selection-kept".into(),
        node_path: vec!["original-node".into()],
        mesh_name: "original-mesh".into(),
        mesh_index: Some(0),
        valid: true,
        orphaned_water_binding: false,
    };
    assert!(
        app.world_mut()
            .resource_mut::<EditorGltfBrowserState>()
            .select_mesh_from_viewport(mesh.clone())
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let browser = app.world().resource::<EditorGltfBrowserState>();
        if browser.entries.iter().any(|entry| {
            matches!(entry.kind, EditorGltfBrowserEntryKind::Mesh)
                && entry.selection.as_ref() == Some(&mesh)
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "原缓存未发布真实Scene0网格：{}",
            browser.status
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    let assert_browser = |world: &World| {
        let browser = world.resource::<EditorGltfBrowserState>();
        let revision = world
            .resource::<EditorAuthoringSession>()
            .placement_structure_revision;
        assert_eq!(
            browser.selected_placement_id.as_deref(),
            Some("selection-kept")
        );
        assert_eq!(browser.selection.as_ref(), Some(&mesh));
        assert_eq!(
            browser.placement_record_index("selection-kept", revision),
            Some(0)
        );
        assert!(
            browser.entries.iter().any(|entry| matches!(&entry.kind,
            EditorGltfBrowserEntryKind::Placement { placement_id, collapsed: false, .. }
                if placement_id == "selection-kept")),
            "原展开根不得因呈现隐藏重建为折叠态"
        );
    };
    assert_browser(app.world());
    f12(&mut app); // 真实原F12入口先发出试玩请求，显隐等待宿主实际回执。
    app.world_mut()
        .resource_mut::<StandaloneEditorMode>()
        .set_external_play_active(true);
    set_embedded_playing(app.world_mut(), true);
    for _ in 0..4 {
        app.update();
        assert_shell(&app, root, camera, true);
        assert_browser(app.world());
    }
    f12(&mut app);
    set_embedded_playing(app.world_mut(), false);
    app.world_mut()
        .resource_mut::<StandaloneEditorMode>()
        .set_external_play_active(false);
    app.update();
    assert_shell(&app, root, camera, true);
    assert_browser(app.world());

    // 真正退出作者会话仍立即撤下公开选择/索引；保留内存只适用于已确认的嵌入试玩。
    app.world_mut().remove_resource::<EditorStartup>();
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    let browser = app.world().resource::<EditorGltfBrowserState>();
    assert!(browser.selected_placement_id.is_none() && browser.selection.is_none());
    assert!(browser.entries.is_empty());
    assert_eq!(browser.placement_record_index("selection-kept", 0), None);
}
