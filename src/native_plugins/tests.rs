//! 加载器边界与有界队列回归；真DLL测试由显式安装包路径触发，缺包即报错而非假通过。
use super::{loader, resources::*};
use feathrise_plugin_api::{Request, SessionConfig, Snapshot};
use std::path::PathBuf;

fn temporary() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "feathrise-loader-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    root
}
#[test]
fn absent_plugins_are_empty_and_bad_manifests_are_rejected() {
    let root = temporary();
    assert!(loader::discover(&root).unwrap().packages.is_empty());
    let package = root.join("plugins/invalid");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("plugin.json"),
        br#"{"format":1,"id":"bad","tool":"bad","abi_major":999,"libraries":{}}"#,
    )
    .unwrap();
    let discovery = loader::discover(&root).unwrap();
    assert!(discovery.packages.is_empty());
    assert!(discovery.warnings.iter().any(|error| error.contains("旧plugin.json")));
}
#[test]
fn queues_are_bounded_and_faults_do_not_accept_new_mutations() {
    let (requests, _receiver) = async_channel::bounded(1);
    let mut tool = Tool {
        domain: "test",
        snapshot: Snapshot::default(),
        requests,
        queue: Default::default(),
        inflight: None,
        completed: 0,
        sequence: 0,
        fault: None,
        library_open: false,
        close_confirmation: None,
        choices: None,
        latest_activation: 0,
        map_pending: false,
    };
    for _ in 0..8 {
        assert!(tool.send(Request::Open, true).is_ok());
    }
    assert!(tool.send(Request::Open, true).is_err());
    assert_eq!(tool.sequence, 8);
    tool.queue.clear();
    tool.fault = Some("会话故障".into());
    assert!(tool.send(Request::Save { document: 1 }, false).is_err());
    assert_eq!(tool.sequence, 8);
}
#[test]
#[ignore = "需要FEATHRISE_PLUGIN_TEST_PACKAGE指定本轮已构建单文件插件；显式--ignored运行"]
fn actual_binary_creates_calls_and_destroys_session() {
    let binary =
        PathBuf::from(std::env::var_os("FEATHRISE_PLUGIN_TEST_PACKAGE").expect("本轮插件包路径"));
    let (manifest, library) = super::management::packages::inspect_package(&binary).unwrap();
    let origin = super::management::types::PluginOrigin {
        scope: super::management::types::PluginScope::Project,
        digest: super::management::packages::binary_digest(&library).unwrap(),
        path: library.clone(),
    };
    let package = loader::Package { manifest, library, origin };
    let root = temporary();
    std::fs::create_dir(root.join("assets")).unwrap();
    let config = SessionConfig {
        plugin_id: "rpg.equipment".into(),
        project_id: "fixture".into(),
        project_root: root.to_string_lossy().into(),
        asset_root: root.join("assets").to_string_lossy().into(),
    };
    let mut loaded = loader::Loaded::open(&package, config.clone()).unwrap();
    let snapshot = loaded.call(&Request::Open).unwrap();
    assert_eq!(snapshot.tool, "装备");
    assert!(snapshot.tabs.is_empty());
    let snapshot = loaded
        .call(&Request::Action {
            document: 0,
            action: "new".into(),
            value: String::new(),
        })
        .unwrap();
    assert!(snapshot.active > 0);
    assert!(snapshot.tabs[0].dirty);
    drop(loaded);
    let mut config = config;
    config.plugin_id = "wrong".into();
    assert!(loader::Loaded::open(&package, config).is_err());
}
