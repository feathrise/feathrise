//! 作者用户配置的真实临时文件回归；所有文件均在测试专属根，不切cwd或修改真实用户配置。

use super::*;
use std::{
    fs,
    path::Path,
    sync::{Arc, Barrier},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    preferences: Preferences,
}
impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "feathrise-author-preferences-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let preferences =
            Preferences::load_at(root.join("用户配置/editor.json"), root.join("用户主目录"))
                .unwrap();
        Self { root, preferences }
    }
    fn file(&self) -> PathBuf {
        self.preferences.file().with_extension("author.json")
    }
    fn store(&self) -> Store {
        load(&self.preferences).0
    }
    fn app(&self) -> (App, Completion) {
        let store = self.store();
        let settings = store.editor.clone().into_runtime();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).insert_resource(settings);
        let completion = install(&mut app, store);
        (app, completion)
    }
    fn host_app(&self) -> (App, Completion) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let completion = install_host(&mut app, self.store());
        (app, completion)
    }
    fn install_close(&self, app: &mut App) {
        let root = self.root.join("关闭回归工程");
        let project = crate::project::Project {
            id: "preferences-close-fixture".into(),
            name: "偏好关闭回归".into(),
            manifest: root.join("Cargo.toml"),
            root: root.clone(),
            package: "unused-preferences-close".into(),
            context: crate::project::ProjectContext {
                root: root.clone(),
                id: "preferences-close-fixture".into(),
                name: "偏好关闭回归".into(),
                format: 1,
                asset_roots: vec![root.join("assets")],
                editor_data: root.join(".editor"),
                storage_key: "preferences-close-fixture".into(),
                startup_scene: None,
            },
        };
        app.add_message::<bevy::window::WindowCloseRequested>()
            .add_message::<AppExit>()
            .add_plugins(crate::native_play::NativePlayPlugin::new(
                project,
                self.root.join("unused-target"),
            ));
    }
}

fn wait_host_save(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready_to_exit(app.world_mut(), false).unwrap() {
        assert!(std::time::Instant::now() < deadline, "首页设置写槽没有真实收口");
        std::thread::yield_now();
    }
}

fn wait_host_reload(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !home_ready(app.world()) {
        assert!(poll_host(app.world_mut(), false).is_none());
        assert!(std::time::Instant::now() < deadline, "返首页读回槽没有真实收口");
        std::thread::yield_now();
    }
}

#[test]
fn home_settings_use_the_author_store_and_reopen_without_rewriting_unchanged_bytes() {
    let fixture = Fixture::new();
    let mut initial = fixture.store();
    initial.editor.ui_scale = 1.25;
    initial.save().unwrap();
    let original = fs::read(fixture.file()).unwrap();
    let game = fixture.root.join("游戏工程/settings.json");
    fs::create_dir_all(game.parent().unwrap()).unwrap();
    fs::write(&game, b"{game settings stay private}").unwrap();
    let (mut app, completion) = fixture.host_app();
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.25);
    for _ in 0..5 { assert!(poll_host(app.world_mut(), false).is_none()); }
    assert_eq!(fs::read(fixture.file()).unwrap(), original);
    {
        let mut settings = app.world_mut().resource_mut::<EditorSettings>();
        settings.editor_ui_scale = 1.5;
        settings.editor_ui_scale_auto = true;
        settings.editor_ui_scale_auto_offset = 0.05;
    }
    poll_host(app.world_mut(), false);
    wait_host_save(&mut app);
    let saved = fixture.store().editor;
    assert_eq!(saved.ui_scale, 1.5);
    assert!(saved.ui_scale_auto);
    assert_eq!(saved.ui_scale_auto_offset, 0.05);
    completion.finish().unwrap();
    drop(app);
    let (app, completion) = fixture.host_app();
    assert_eq!(EditorSettingsData::from(app.world().resource::<EditorSettings>()), saved);
    assert!(!fixture.preferences.file().exists(), "作者偏好不顺带改写最近工程文件");
    assert_eq!(fs::read(game).unwrap(), b"{game settings stay private}");
    completion.finish().unwrap();
}

#[test]
fn home_handoff_flushes_the_latest_value_and_never_competes_with_an_active_author() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.host_app();
    app.world_mut().resource_mut::<EditorSettings>().editor_ui_scale = 1.25;
    assert!(!yield_to_author(app.world_mut()).unwrap(), "仅派发IO不能让候选读回旧值");
    app.world_mut().resource_mut::<EditorSettings>().editor_ui_scale = 1.5;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !yield_to_author(app.world_mut()).unwrap() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!home_ready(app.world()));
    let mut author = fixture.store(); // 与实际候选相同：只有生产交接门返回true后才读取。
    assert_eq!(author.editor.ui_scale, 1.5);
    author.editor.ui_scale = 1.35;
    author.editor.color_theme = "Light".into();
    author.save().unwrap();
    let bytes = fs::read(fixture.file()).unwrap();
    app.world_mut().resource_mut::<EditorSettings>().editor_ui_scale = 2.0;
    for _ in 0..8 { assert!(poll_host(app.world_mut(), true).is_none()); }
    assert_eq!(fs::read(fixture.file()).unwrap(), bytes, "root旧快照不能争写作者最新设置");
    assert!(!ready_to_exit(app.world_mut(), false).unwrap());
    // 即便主App被异常退役，完成兜底也不能把暂停期间的root设置写回。
    drop(app);
    completion.finish().unwrap();
    assert_eq!(fs::read(fixture.file()).unwrap(), bytes);
}

#[test]
fn returning_home_asynchronously_adopts_the_author_latest_snapshot_and_new_cas_identity() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.host_app();
    assert!(yield_to_author(app.world_mut()).unwrap());
    let mut author = fixture.store();
    author.editor.ui_scale = 1.5;
    author.editor.ui_scale_auto = true;
    author.editor.ui_scale_auto_offset = -0.05;
    author.editor.render.fullscreen = true;
    author.save().unwrap();
    let original = fs::read(fixture.file()).unwrap();
    assert!(poll_host(app.world_mut(), false).is_none());
    assert!(!home_ready(app.world()), "读回任务刚开始不等于可以编辑或交接");
    assert!(!yield_to_author(app.world_mut()).unwrap());
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.0);
    wait_host_reload(&mut app);
    let latest = app.world().resource::<EditorSettings>();
    assert_eq!(latest.editor_ui_scale, 1.5);
    assert!(latest.editor_ui_scale_auto);
    assert_eq!(latest.editor_ui_scale_auto_offset, -0.05);
    assert!(latest.render.fullscreen);
    assert_eq!(fs::read(fixture.file()).unwrap(), original, "纯读回不应重新编码配置");
    app.world_mut().resource_mut::<EditorSettings>().editor_ui_scale = 1.25;
    wait_host_save(&mut app); // 使用作者刚写的新原文身份，不能与root启动时的旧身份冲突。
    let saved = fixture.store().editor;
    assert_eq!(saved.ui_scale, 1.25);
    assert!(saved.render.fullscreen);
    completion.finish().unwrap();
}

#[test]
fn late_home_reload_cannot_adopt_an_old_author_generation() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.host_app();
    assert!(yield_to_author(app.world_mut()).unwrap());
    poll_host(app.world_mut(), false); // 第一笔实际后台读取尚未由主帧采用。
    let generation = app.world().non_send_resource::<Runtime>().0.borrow().generation;
    poll_host(app.world_mut(), true); // 新作者重新持权，旧读回即使完成也不得恢复首页写权限。
    assert!(app.world().non_send_resource::<Runtime>().0.borrow().generation != generation);
    assert!(!home_ready(app.world()));
    let mut author = fixture.store();
    author.editor.ui_scale = 1.75;
    author.save().unwrap();
    poll_host(app.world_mut(), false);
    wait_host_reload(&mut app);
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.75);
    completion.finish().unwrap();
}

#[test]
fn bad_returned_preferences_remain_read_only_and_external_changes_block_handoff_without_spinning() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.host_app();
    assert!(yield_to_author(app.world_mut()).unwrap());
    fs::create_dir_all(fixture.file().parent().unwrap()).unwrap();
    fs::write(fixture.file(), b"{broken after author exit}").unwrap();
    poll_host(app.world_mut(), false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut received_failure = false;
    while !home_ready(app.world()) {
        received_failure |= poll_host(app.world_mut(), false).is_some();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(received_failure, "坏原文读回必须带诊断，不能冒充成功恢复");
    app.world_mut().resource_mut::<EditorSettings>().editor_ui_scale = 1.5;
    loop {
        if yield_to_author(app.world_mut()).is_err() { break; }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    for _ in 0..6 { poll_host(app.world_mut(), false); }
    assert!(app.world().non_send_resource::<Runtime>().0.borrow().job.is_none());
    assert_eq!(fs::read(fixture.file()).unwrap(), b"{broken after author exit}");
    assert!(completion.finish().is_err());

    // 正常读回也不能覆盖读回后被另一实例替换的原文；继续沿同一个publish_checked保护。
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.host_app();
    assert!(yield_to_author(app.world_mut()).unwrap());
    poll_host(app.world_mut(), false);
    wait_host_reload(&mut app);
    fs::create_dir_all(fixture.file().parent().unwrap()).unwrap();
    fs::write(fixture.file(), b"{external replacement after reload}").unwrap();
    app.world_mut().resource_mut::<EditorSettings>().editor_ui_scale = 1.25;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if yield_to_author(app.world_mut()).is_err() { break; }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(home_ready(app.world()), "失败不能把写权限交给尚未创建的作者");
    assert_eq!(fs::read(fixture.file()).unwrap(), b"{external replacement after reload}");
    assert!(completion.finish().is_err());
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // 只清理本测试创建并重新验证的精确临时根，不能递归删除计算出来的未知位置。
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        if self.root.parent() == Some(temporary.as_path())
            && self.root.canonicalize().ok().as_deref() == Some(self.root.as_path())
            && self.root.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("feathrise-author-preferences-")
            })
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[test]
fn legacy_author_preferences_follow_main_location_without_overwriting_new_settings() {
    let fixture = Fixture::new();
    let legacy = fixture.root.join("旧设置/editor.json");
    let legacy_author = legacy.with_extension("author.json");
    let preferences = Preferences::load_migrating_at(fixture.preferences.file(), fixture.preferences.home(), &legacy).unwrap();
    let mut original = Store::load(legacy_author.clone()).unwrap();
    original.editor.ui_scale = 1.35;
    original.save().unwrap();
    let bytes = fs::read(&legacy_author).unwrap();
    let (mut migrated, error) = load(&preferences);
    assert!(error.is_none());
    assert_eq!(migrated.editor.ui_scale, 1.35);
    assert_eq!(fs::read(fixture.file()).unwrap(), bytes);
    assert_eq!(fs::read(&legacy_author).unwrap(), bytes);
    assert!(!preferences.file().exists(), "作者偏好迁移不能顺带创建或改写最近工程文件");

    migrated.editor.ui_scale = 1.7;
    migrated.save().unwrap();
    assert_eq!(load(&preferences).0.editor.ui_scale, 1.7);
    assert_eq!(fs::read(&legacy_author).unwrap(), bytes);
}

#[test]
fn explicit_author_preferences_never_import_legacy_and_bad_legacy_stays_read_only() {
    let fixture = Fixture::new();
    let legacy = fixture.root.join("旧设置/editor.json");
    let legacy_author = legacy.with_extension("author.json");
    fs::create_dir_all(legacy_author.parent().unwrap()).unwrap();
    fs::write(&legacy_author, b"{broken legacy author preferences").unwrap();
    let (explicit, error) = load(&fixture.preferences);
    assert!(error.is_none());
    assert_eq!(explicit.editor, EditorSettingsData::default());
    assert!(!fixture.file().exists());

    let mut preferences = Preferences::load_migrating_at(fixture.preferences.file(), fixture.preferences.home(), &legacy).unwrap();
    let (mut store, error) = load(&preferences);
    assert!(error.is_some());
    store.editor.ui_scale = 1.6;
    assert!(store.save().unwrap_err().contains("拒绝覆盖"));
    assert_eq!(fs::read(fixture.file()).unwrap(), fs::read(&legacy_author).unwrap());
    preferences.save().unwrap(); // 作者文件损坏不能妨碍独立的主窗口偏好保存。
    assert!(preferences.file().is_file());
    assert_eq!(fs::read(&legacy_author).unwrap(), b"{broken legacy author preferences");
}

#[test]
fn unchanged_startup_and_corrupt_readonly_fallback_do_not_write_or_block_exit() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.app();
    app.update();
    assert!(!fixture.file().exists());
    assert!(ready_to_exit(app.world_mut(), false).unwrap());
    completion.finish().unwrap();
    drop(app);

    fs::create_dir_all(fixture.file().parent().unwrap()).unwrap();
    fs::write(fixture.file(), b"{broken author preferences").unwrap();
    assert!(load(&fixture.preferences).1.is_some());
    let (mut app, completion) = fixture.app();
    app.update();
    assert!(ready_to_exit(app.world_mut(), false).unwrap());
    completion.finish().unwrap();
    assert_eq!(
        fs::read(fixture.file()).unwrap(),
        b"{broken author preferences"
    );
}

#[test]
fn author_preference_writes_do_not_invalidate_concurrent_recent_project_updates() {
    let mut fixture = Fixture::new();
    fixture
        .preferences
        .record_opened_at("工程 A", &fixture.root.join("A"), 1)
        .unwrap();
    fixture.preferences.save().unwrap();
    let mut author = fixture.store();
    author.editor.ui_scale = 1.25;
    let barrier = Arc::new(Barrier::new(2));
    let author_barrier = Arc::clone(&barrier);
    let writer = std::thread::spawn(move || {
        author_barrier.wait();
        author.save().unwrap();
    });
    barrier.wait();
    fixture
        .preferences
        .record_opened_at("工程 B", &fixture.root.join("B"), 2)
        .unwrap();
    fixture.preferences.save().unwrap();
    writer.join().unwrap();
    // 子方作者设置写完后，父方持有的editor.json原文身份仍必须有效。
    fixture
        .preferences
        .record_opened_at("工程 C", &fixture.root.join("C"), 3)
        .unwrap();
    fixture.preferences.save().unwrap();
    let reopened =
        Preferences::load_at(fixture.preferences.file(), fixture.preferences.home()).unwrap();
    assert_eq!(
        reopened
            .recent_projects
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>(),
        ["工程 C", "工程 B", "工程 A"]
    );
    assert_eq!(fixture.store().editor.ui_scale, 1.25);
    assert_ne!(fixture.file(), fixture.preferences.file());
}

#[test]
fn both_project_game_settings_are_untouched_and_cannot_supply_editor_preferences() {
    let fixture = Fixture::new();
    let first = fixture.root.join("工程 A/settings.json");
    let second = fixture.root.join("工程 B/settings.json");
    for file in [&first, &second] {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
    }
    let first_bytes = br#"{"fullscreen":true,"editor":{"ui_scale":2.4},"game":"first"}"#;
    let second_bytes = br#"{"fullscreen":false,"editor":{"ui_scale":0.6},"game":"second"}"#;
    fs::write(&first, first_bytes).unwrap();
    fs::write(&second, second_bytes).unwrap();
    // 读取接口只接收用户偏好位置，不接收工程根，也不使用工程cwd；两工程均不能注入自己的设置。
    let mut store = fixture.store();
    assert_eq!(store.editor, EditorSettingsData::default());
    store.editor.ui_scale = 1.35;
    store.save().unwrap();
    for _ in [&first, &second] {
        assert_eq!(fixture.store().editor.ui_scale, 1.35);
    }
    assert_eq!(fs::read(first).unwrap(), first_bytes);
    assert_eq!(fs::read(second).unwrap(), second_bytes);
}

#[test]
fn latest_changes_coalesce_and_normal_exit_waits_without_blocking_the_frame() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.app();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.25;
    assert!(!ready_to_exit(app.world_mut(), false).unwrap()); // 只派发IO，本帧不等待结果。
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.35;
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .render
        .fullscreen = true;
    app.update(); // 在途写入尚未采用时，后续输入只收敛成同一份最新待存快照。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready_to_exit(app.world_mut(), false).unwrap() {
        assert!(std::time::Instant::now() < deadline, "作者偏好单槽没有收口");
        std::thread::yield_now();
    }
    completion.finish().unwrap();
    let saved = fixture.store().editor;
    assert_eq!(saved.ui_scale, 1.35);
    assert!(saved.render.fullscreen);
}

#[test]
fn app_exit_completion_flushes_the_last_coalesced_snapshot() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.app();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.25;
    app.update();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.35;
    app.update();
    // 模拟候选退役等直接AppExit；完成句柄在World之外存活，退出后才允许同步等最后IO。
    drop(app);
    completion.finish().unwrap();
    assert_eq!(fixture.store().editor.ui_scale, 1.35);
}

#[test]
fn corrupt_or_external_author_configuration_is_never_overwritten_and_failure_does_not_spin() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.app();
    fs::create_dir_all(fixture.file().parent().unwrap()).unwrap();
    fs::write(fixture.file(), b"{externally replaced").unwrap();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.25;
    app.update();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if ready_to_exit(app.world_mut(), false).is_err() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "未收到身份冲突结果");
        std::thread::yield_now();
    }
    for _ in 0..5 {
        app.update();
    }
    let runtime = app.world().non_send_resource::<Runtime>();
    assert!(runtime.0.borrow().job.is_none());
    assert!(runtime.0.borrow().error().is_some());
    assert_eq!(fs::read(fixture.file()).unwrap(), b"{externally replaced");
    assert!(completion.finish().is_err());
}

#[test]
fn apps_without_author_preferences_keep_the_existing_close_contract() {
    assert!(ready_to_exit(&mut World::new(), false).unwrap());
    assert!(Path::new("editor.json").with_extension("author.json") != Path::new("editor.json"));
}

#[test]
fn normal_close_publishes_app_exit_only_after_latest_editor_preferences_are_saved() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.app();
    fixture.install_close(&mut app);
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.25;
    app.world_mut()
        .write_message(bevy::window::WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        });
    app.update();
    assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    // 干净工程也须明确选择；本夹具没有作者文档，放弃不跳过作者个人偏好的真实写入。
    crate::native_project_close::tests::choose_discard(app.world_mut());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.world().resource::<Messages<AppExit>>().is_empty() {
        app.update();
        assert!(
            std::time::Instant::now() < deadline,
            "原关闭事务未等待作者偏好收口"
        );
        std::thread::yield_now();
    }
    assert_eq!(fixture.store().editor.ui_scale, 1.25);
    completion.finish().unwrap();
}

#[test]
fn failed_preference_write_keeps_original_close_state_and_cancel_restores_author_session() {
    let fixture = Fixture::new();
    let (mut app, completion) = fixture.app();
    fixture.install_close(&mut app);
    fs::create_dir_all(fixture.file().parent().unwrap()).unwrap();
    fs::write(fixture.file(), b"{protected broken preferences").unwrap();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.25;
    app.world_mut()
        .write_message(bevy::window::WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        });
    app.update();
    crate::native_project_close::tests::choose_discard(app.world_mut());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.update();
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        let mut feedback = bevy::ecs::message::MessageCursor::<
            feathrise_editor_map::standalone::SavedPlayFeedback,
        >::default();
        if feedback
            .read(
                app.world()
                    .resource::<Messages<feathrise_editor_map::standalone::SavedPlayFeedback>>(),
            )
            .any(|item| item.0.contains("作者个人设置未保存"))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "关闭失败未走原反馈与保留现场分支"
        );
        std::thread::yield_now();
    }
    crate::native_project_close::cancel_workspace_close(app.world_mut());
    app.update();
    assert!(
        !app.world()
            .resource::<crate::native_project_close::NativeProjectCloseState>()
            .active()
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_ui_scale,
        1.25
    );
    assert_eq!(
        fs::read(fixture.file()).unwrap(),
        b"{protected broken preferences"
    );
    // 用户撤回偏好修改后可正常关闭；损坏配置仍不应仅因存在就锁住退出。
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.0;
    app.world_mut()
        .write_message(bevy::window::WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        });
    app.update();
    crate::native_project_close::tests::choose_discard(app.world_mut());
    app.update();
    assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
    completion.finish().unwrap();
}
