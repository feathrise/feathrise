//! 首页偏好的真实事务门回归；只使用本轮临时配置，候选未获授权前绝不启动真实作者进程。

use super::*;
use crate::{
    project::{Project, ProjectContext},
    user_preferences::Preferences,
};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_scene::workbench::WorkbenchSettingsAccess;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    app: App,
    host: Host,
    completion: Option<author_preferences::Completion>,
}

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "feathrise-home-preferences-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let preferences =
            Preferences::load_at(root.join("preferences/editor.json"), root.join("home")).unwrap();
        let (store, error) = author_preferences::load(&preferences);
        assert!(error.is_none());
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<NativeProjectCloseState>()
            .init_resource::<WorkbenchSettingsAccess>()
            .add_message::<AppExit>();
        let completion = Some(author_preferences::install_host(&mut app, store));
        let host = Host {
            target_override: None,
            preferences,
            preferences_job: None,
            job: None,
            prepared: None,
            picker: None,
            current: None,
            candidate: None,
            retiring: None,
            want_close: false,
            close_sent: false,
            exit_app: false,
            notice: String::new(),
            changed: false,
            initial: None,
        };
        Self {
            root,
            app,
            host,
            completion,
        }
    }

    fn file(&self) -> PathBuf {
        self.host.preferences.file().with_extension("author.json")
    }

    fn project(&self) -> Project {
        let root = self.root.join("owned-project");
        Project {
            id: "home-preferences-fixture".into(),
            name: "首页偏好夹具".into(),
            manifest: root.join("Cargo.toml"),
            package: "unused-home-preferences".into(),
            root: root.clone(),
            context: ProjectContext {
                root: root.clone(),
                id: "home-preferences-fixture".into(),
                name: "首页偏好夹具".into(),
                format: 1,
                asset_roots: vec![root.join("assets")],
                editor_data: root.join(".editor"),
                storage_key: "home-preferences-fixture".into(),
                startup_scene: None,
            },
        }
    }

    fn poll(&mut self) {
        poll_author_preferences(&mut self.host, self.app.world_mut());
    }

    fn has_exit(&self) -> bool {
        !self.app.world().resource::<Messages<AppExit>>().is_empty()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(completion) = self.completion.take() {
            let _ = completion.finish();
        }
        // 只清理测试创建且绝对身份仍一致的临时根，不触碰target或真实用户配置。
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        if self.root.parent() == Some(temporary.as_path())
            && self.root.canonicalize().ok().as_deref() == Some(self.root.as_path())
            && self.root.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("feathrise-home-preferences-")
            })
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[test]
fn authorized_home_exit_waits_for_the_actual_preference_write() {
    let mut fixture = Fixture::new();
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.25;
    fixture.host.exit_app = true; // 本用例只检验已由原确认事务授权后的最终门，不替代确认层回归。
    finish_app_exit(&mut fixture.host, fixture.app.world_mut());
    assert!(!fixture.has_exit(), "派发后台写入不能冒充已落盘");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fixture.has_exit() {
        fixture.poll();
        finish_app_exit(&mut fixture.host, fixture.app.world_mut());
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        author_preferences::load(&fixture.host.preferences)
            .0
            .editor
            .ui_scale,
        1.25
    );
    assert!(
        !fixture.host.preferences.file().exists(),
        "只写作者偏好文件"
    );
}

#[test]
fn failed_home_write_keeps_the_exit_confirmation_and_original_bytes() {
    let mut fixture = Fixture::new();
    fs::create_dir_all(fixture.file().parent().unwrap()).unwrap();
    fs::write(fixture.file(), b"{external preferences replacement}").unwrap();
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.5;
    fixture.host.exit_app = true;
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.host.exit_app {
        fixture.poll();
        finish_app_exit(&mut fixture.host, fixture.app.world_mut());
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!fixture.has_exit());
    assert!(
        fixture
            .app
            .world()
            .resource::<NativeProjectCloseState>()
            .active()
    );
    assert!(fixture.host.notice.contains("未保存"));
    assert_eq!(
        fs::read(fixture.file()).unwrap(),
        b"{external preferences replacement}"
    );
}

#[test]
fn prepared_project_is_retained_until_flush_and_failure_cannot_start_a_candidate() {
    let mut fixture = Fixture::new();
    fixture.host.prepared = Some(fixture.project());
    fs::create_dir_all(fixture.file().parent().unwrap()).unwrap();
    fs::write(fixture.file(), b"{protected external preferences}").unwrap();
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.25;
    poll_prepared(&mut fixture.host, fixture.app.world_mut(), 0);
    assert!(fixture.host.prepared.is_some() && fixture.host.busy());
    assert!(fixture.host.candidate.is_none());
    assert!(
        fixture
            .app
            .world()
            .resource::<WorkbenchSettingsAccess>()
            .editable,
        "写入排空前仍接受末次设置；只有成功交权限后才封输入"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.host.prepared.is_some() {
        poll_prepared(&mut fixture.host, fixture.app.world_mut(), 0);
        assert!(
            fixture.host.candidate.is_none(),
            "受保护原文写入失败不能创建读取旧设置的候选"
        );
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(fixture.host.notice.contains("工程未交接"));
    assert_eq!(
        fs::read(fixture.file()).unwrap(),
        b"{protected external preferences}"
    );
}

#[test]
fn settings_gate_stays_closed_between_author_handoff_and_complete_home_reload() {
    let mut fixture = Fixture::new();
    assert!(author_preferences::yield_to_author(fixture.app.world_mut()).unwrap());
    sync_preferences_access(fixture.app.world_mut());
    assert!(
        !fixture
            .app
            .world()
            .resource::<WorkbenchSettingsAccess>()
            .editable
    );
    // 作者拥有期间的真实保存不归root执行。以另一Store模拟另一进程持有的新原文身份。
    let author = author_preferences::load(&fixture.host.preferences).0;
    // Store保存算法本身保持私有；由相同原作者Runtime完成真实保存，而非测试写成功状态。
    let mut author_app = App::new();
    author_app
        .add_plugins(MinimalPlugins)
        .insert_resource(EditorSettings::default());
    let author_completion = author_preferences::install_host(&mut author_app, author);
    // 读取完成后的这次修改才构成作者实际输入。
    author_app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.75;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !author_preferences::ready_to_exit(author_app.world_mut(), false).unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    author_completion.finish().unwrap();
    fixture.poll(); // 全部作者退役后，先进入真实异步读回；这一帧不能开放设置输入。
    assert!(
        !fixture
            .app
            .world()
            .resource::<WorkbenchSettingsAccess>()
            .editable
    );
    while !fixture
        .app
        .world()
        .resource::<WorkbenchSettingsAccess>()
        .editable
    {
        fixture.poll();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<EditorSettings>()
            .editor_ui_scale,
        1.75
    );
}
