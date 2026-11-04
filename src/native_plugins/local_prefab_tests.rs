//! 本地预览真实驱动回归：缓存返回、播放控制、绑定工程与迟到后台结果，不安装构建或游戏进程。
use super::*;
use feathrise_map_content::author_visuals::{AuthorPrefab, AuthorPose};
use feathrise_plugin_api::{Preview, RuntimePreview};
use std::time::{Duration, Instant};

fn source() -> AuthorPrefabPreview {
    AuthorPrefabPreview {
        prefab: AuthorPrefab { model: None, transform: AuthorPose {
            translation: [0.; 3], rotation: [0., 0., 0., 1.], scale: [1.; 3],
        }, attachments: vec![], idle: None },
        playing: false, speed: 0.5,
    }
}
fn select(app: &mut App, document: u32, revision: u64) {
    let mut tools = app.world_mut().resource_mut::<NativeTools>();
    let tool = tools.tools.get_mut(&0).unwrap();
    tool.snapshot.active = document;
    tool.snapshot.preview = Some(Preview { revision, model: None, icon: None, details: String::new(),
        runtime: Some(RuntimePreview { revision, kind: AUTHOR_PREFAB_PREVIEW_KIND.into(), payload: serde_json::to_value(source()).unwrap() }),
    });
}
fn fixture() -> App {
    let (mut app, _, _) = super::super::sidebar_tests::fixture();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), render::AuthorVisualsPlugin))
        .init_asset::<Mesh>().init_asset::<StandardMaterial>()
        .init_asset::<Gltf>().init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<AnimationClip>().init_asset::<AnimationGraph>()
        .init_resource::<AssetPreviewState>().init_resource::<State>();
    let root = std::env::temp_dir().join("feathrise-local-preview-no-io-fixture");
    let assets = root.join("assets");
    app.insert_resource(Settings { project: "fixture".into(), root: root.clone(), assets: assets.clone() })
        .insert_resource(feathrise_map_content::RuntimeAssetRoot::new(assets).unwrap());
    let tab = PresetWorkspaceTab::Domain("fixture.equipment", 1);
    app.world_mut().resource_mut::<MapEditorSidebarState>().left = EditorLeftSidebarTab::Domain("fixture.equipment");
    app.world_mut().resource_mut::<MapEditorSidebarState>().right = EditorRightSidebarTab::Tools;
    app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = tab;
    select(&mut app, 1, 1);
    app
}
fn wait_root(app: &mut App) -> Entity {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        drive(app.world_mut());
        if let Some(root) = app.world().resource::<State>().root { return root; }
        assert!(Instant::now() < deadline, "本地预览后台准备未在测试期限内完成");
        std::thread::yield_now();
    }
}
fn check_playback(app: &mut App, root: Entity) {
    let body = app.world().get::<Children>(root).unwrap()[0];
    let (_, node) = AnimationGraph::from_clip(Handle::default());
    let mut player = AnimationPlayer::default();
    player.play(node).repeat();
    app.world_mut().entity_mut(body).insert(player);
    app.update(); // 真实SDK分帧准备产生AuthorPrefabReady，然后本地驱动应用会话播放参数。
    drive(app.world_mut());
    assert!(app.world().get::<AuthorPrefabReady>(root).is_some());
    let active = app.world().get::<AnimationPlayer>(body).unwrap().playing_animations().next().unwrap().1;
    assert!(active.is_paused());
    assert_eq!(active.speed(), 0.5);
}
#[test]
fn browsing_and_lost_root_restore_cached_prefab_and_playback_after_payload_is_moved() {
    let mut app = fixture();
    let first = wait_root(&mut app);
    check_playback(&mut app, first);
    assert!(app.world().resource::<NativeTools>().tools[&0].snapshot.preview.as_ref().unwrap().runtime.as_ref().unwrap().payload.is_null());
    let tab = app.world().resource::<VegetationPresetEditorState>().active_tab;
    app.world_mut().resource_mut::<AssetPreviewFocus>().browse(tab);
    drive(app.world_mut());
    assert!(app.world().resource::<State>().root.is_none());
    assert_eq!(*app.world().get::<Visibility>(first).unwrap(), Visibility::Hidden);
    assert!(app.world().get::<AssetPreviewRetiredRoot>(first).is_some());
    assert!(ready_to_exit(app.world()));
    app.world_mut().resource_mut::<AssetPreviewFocus>().show_workspace();
    let second = wait_root(&mut app);
    assert_ne!(first, second);
    check_playback(&mut app, second);
    app.world_mut().despawn(second);
    let third = wait_root(&mut app);
    assert_ne!(second, third);
    check_playback(&mut app, third);
    let wrong = std::env::temp_dir().join("feathrise-different-project-no-io-fixture");
    app.insert_resource(feathrise_map_content::RuntimeAssetRoot::new(wrong.join("assets")).unwrap());
    drive(app.world_mut());
    assert!(app.world().resource::<State>().root.is_none());
    assert!(app.world().get::<AssetPreviewRetiredRoot>(third).is_some());
}
#[test]
fn obsolete_worker_finishes_without_publishing_then_latest_document_uses_the_same_single_flight_slot() {
    let mut app = fixture();
    let old = wanted(app.world()).unwrap();
    let (release, wait) = std::sync::mpsc::sync_channel::<()>(1);
    let task = IoTaskPool::get().spawn(async move {
        let result = wait.recv().map(|_| Arc::new(source())).map_err(|error| error.to_string());
        (old, result)
    });
    app.world_mut().resource_mut::<State>().task = Some(task);
    select(&mut app, 2, 2);
    drive(app.world_mut());
    assert!(app.world().resource::<State>().root.is_none());
    assert!(!ready_to_exit(app.world())); // 在途验证必须真实结束，关闭门不能仅撤销身份就放行。
    release.send(()).unwrap();
    let root = wait_root(&mut app);
    let state = app.world().resource::<State>();
    assert_eq!(state.root(0, 2, 2), Some(root));
    assert_eq!(state.root(0, 1, 1), None);
    assert_eq!(state.cached.as_ref().unwrap().0.document, 2);
}
