//! 管理器风险分支回归：执行生产输入与后台轮询，覆盖旧点击、模态收口和单槽失败保护。
use super::{components::*, packages, resources::PluginManagerState, systems, types::*};
use bevy::{input_focus::InputFocus, prelude::*};
use feathrise_editor_core::EditorTextInputGuard;
use feathrise_editor_ui::EditorButtonClick;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    directory: PathBuf,
    project: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "feathrise-plugin-control-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        fs::write(directory.join("owner"), b"plugin-control-fixture").unwrap();
        let project = directory.join("project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(project.join("assets")).unwrap();
        fs::write(
            project.join("assets/author-data.txt"),
            b"author-must-survive",
        )
        .unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname='control_fixture'\nversion='0.1.0'\n[dependencies]\n",
        )
        .unwrap();
        Self { directory, project }
    }

    fn package(&self, name: &str, id: &str) -> PathBuf {
        let source = self.directory.join(format!(
            "{name}.{}",
            match std::env::consts::OS {
                "windows" => "dll",
                "macos" => "dylib",
                _ => "so",
            }
        ));
        let manifest = PackageManifest {
            format: 1,
            id: id.into(),
            tool: "定向夹具插件".into(),
            abi_major: feathrise_plugin_api::ABI_MAJOR,
            version: "1.0.0".into(),
            target_os: std::env::consts::OS.into(),
            target_arch: std::env::consts::ARCH.into(),
            runtime_dependencies: Vec::new(),
            runtime: None,
        };
        // 真实共享头与静态元数据节只用于事务和输入门，绝不冒充动态装载成功。
        feathrise_plugin_api::metadata::test_support::write_metadata_fixture(&source, &manifest)
            .unwrap();
        source
    }

    fn install(&self, name: &str, id: &str) {
        packages::stage(
            &self.project,
            ManagementCommand::Install {
                source: self.package(name, id),
                replace: false,
            },
        )
        .unwrap();
        assert!(packages::apply_pending(&self.project).is_empty());
    }

    fn app(&self) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<EditorTextInputGuard>();
        let mut manager = PluginManagerState::new(self.project.clone());
        manager.open = true;
        manager.was_open = true;
        manager.refreshed = true;
        manager.rows = packages::catalog(&self.project).unwrap().entries;
        manager.selected = manager.rows.first().map(|row| row.id.clone());
        app.insert_resource(manager);
        app
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // 只删除本测试成功create_dir并再次核对owner、绝对根和父目录归属的确切目录。
        let expected_parent = std::env::temp_dir().canonicalize().unwrap();
        if self.directory.is_absolute()
            && self.directory.parent() == Some(expected_parent.as_path())
            && self
                .directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("feathrise-plugin-control-")
            && fs::read(self.directory.join("owner")).ok().as_deref()
                == Some(b"plugin-control-fixture")
        {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

fn click(world: &mut World, action: ManagerAction, revision: u64) {
    let entity = world
        .spawn((
            action,
            ManagerBinding(revision),
            EditorButtonClick::triggered(),
        ))
        .id();
    systems::control(world);
    world.despawn(entity);
}

fn poll_until_idle(world: &mut World) {
    let started = Instant::now();
    while world.resource::<PluginManagerState>().busy {
        systems::poll(world);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "生产管理后台任务未返回"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn no_pending(root: &Path) {
    assert!(
        packages::catalog(root)
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.pending.is_none())
    );
}

#[test]
fn stale_row_binding_cannot_change_selection_or_disable_current_package() {
    let fixture = Fixture::new();
    fixture.install("first", "fixture.first");
    fixture.install("second", "fixture.second");
    let mut app = fixture.app();
    {
        let mut state = app.world_mut().resource_mut::<PluginManagerState>();
        state.revision = 20;
        state.selected = Some("fixture.first".into());
    }
    click(
        app.world_mut(),
        ManagerAction::Select("fixture.second".into()),
        19,
    );
    assert_eq!(
        app.world()
            .resource::<PluginManagerState>()
            .selected
            .as_deref(),
        Some("fixture.first")
    );
    click(
        app.world_mut(),
        ManagerAction::SetEnabled {
            id: "fixture.first".into(),
            enabled: false,
        },
        19,
    );
    assert!(!app.world().resource::<PluginManagerState>().busy);
    assert!(packages::disabled_ids(&fixture.project).unwrap().is_empty());
    no_pending(&fixture.project);
}

#[test]
fn escape_cancels_remove_confirmation_before_closing_and_keeps_disk_plan_empty() {
    let fixture = Fixture::new();
    fixture.install("first", "fixture.first");
    let mut app = fixture.app();
    app.world_mut()
        .resource_mut::<PluginManagerState>()
        .confirm_remove = Some("fixture.first".into());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    systems::control(app.world_mut());
    assert!(app.world().resource::<PluginManagerState>().open);
    assert!(
        app.world()
            .resource::<PluginManagerState>()
            .confirm_remove
            .is_none()
    );
    assert!(!app.world().resource::<PluginManagerState>().busy);
    no_pending(&fixture.project);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Escape);
        keys.clear();
        keys.press(KeyCode::Escape);
    }
    systems::control(app.world_mut());
    assert!(!app.world().resource::<PluginManagerState>().open);
    assert!(
        app.world()
            .resource::<PluginManagerState>()
            .captures_input(),
        "关闭Esc的释放帧仍须持有背景输入"
    );
    systems::control(app.world_mut());
    assert!(
        !app.world()
            .resource::<PluginManagerState>()
            .captures_input()
    );
    no_pending(&fixture.project);
    assert!(
        fixture
            .project
            .join("plugins")
            .join(packages::binary_filename("fixture.first"))
            .is_file()
    );
}

#[test]
fn close_button_holds_its_release_frame_then_returns_input_on_the_next_quiet_frame() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(app.world_mut(), ManagerAction::Close, revision);
    let state = app.world().resource::<PluginManagerState>();
    assert!(!state.open && state.captured);
    assert!(
        app.world()
            .resource::<EditorTextInputGuard>()
            .block_shortcuts
    );
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    systems::control(app.world_mut());
    assert!(
        !app.world()
            .resource::<PluginManagerState>()
            .captures_input()
    );
    assert!(
        !app.world()
            .resource::<EditorTextInputGuard>()
            .block_shortcuts
    );
}

#[test]
fn active_project_close_gate_rejects_management_clicks_without_starting_background_io() {
    let fixture = Fixture::new();
    fixture.install("first", "fixture.first");
    let mut app = fixture.app();
    let mut closing = crate::native_project_close::NativeProjectCloseState::default();
    closing.hold("关闭事务正在等待实际回执".into());
    app.insert_resource(closing);
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(
        app.world_mut(),
        ManagerAction::SetEnabled {
            id: "fixture.first".into(),
            enabled: false,
        },
        revision,
    );
    let state = app.world().resource::<PluginManagerState>();
    assert!(!state.open && !state.busy && state.task.is_none());
    assert_eq!(state.sequence, 0);
    no_pending(&fixture.project);
    assert_eq!(
        fs::read(fixture.project.join("assets/author-data.txt")).unwrap(),
        b"author-must-survive"
    );
}

#[test]
fn single_slot_rejects_second_request_and_failed_task_keeps_last_catalog() {
    let fixture = Fixture::new();
    fixture.install("first", "fixture.first");
    let mut app = fixture.app();
    let baseline = app
        .world()
        .resource::<PluginManagerState>()
        .rows
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let baseline_bytes = fs::read(
        fixture
            .project
            .join("plugins")
            .join(packages::binary_filename("fixture.first")),
    )
    .unwrap();
    {
        let mut state = app.world_mut().resource_mut::<PluginManagerState>();
        state
            .request(ManagementCommand::Install {
                source: fixture.directory.join("missing-source"),
                replace: false,
            })
            .unwrap();
        assert!(state.busy);
        assert!(
            state
                .request(ManagementCommand::Remove {
                    id: "fixture.first".into()
                })
                .is_err()
        );
    }
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert_eq!(state.sequence, 1, "拒绝的第二请求不得挤掉第一个在途票据");
    assert_eq!(
        state
            .rows
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        baseline
    );
    assert!(!state.status.is_empty());
    assert_eq!(
        fs::read(
            fixture
                .project
                .join("plugins")
                .join(packages::binary_filename("fixture.first"))
        )
        .unwrap(),
        baseline_bytes
    );
    no_pending(&fixture.project);
}

#[test]
fn selected_update_id_rejects_a_different_valid_source_package_and_preserves_installation() {
    let fixture = Fixture::new();
    fixture.install("first", "fixture.first");
    let other = fixture.package("other", "fixture.other");
    let mut app = fixture.app();
    let before = fs::read(
        fixture
            .project
            .join("plugins")
            .join(packages::binary_filename("fixture.first")),
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<PluginManagerState>()
        .install_package(other, Some("fixture.first".into()))
        .unwrap();
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert!(
        state.status.contains("fixture.other") && state.status.contains("fixture.first"),
        "必须说明来源ID与更新目标不匹配"
    );
    assert_eq!(state.rows.len(), 1);
    no_pending(&fixture.project);
    assert_eq!(
        fs::read(
            fixture
                .project
                .join("plugins")
                .join(packages::binary_filename("fixture.first"))
        )
        .unwrap(),
        before
    );
    assert!(
        !fixture
            .project
            .join("plugins")
            .join(packages::binary_filename("fixture.other"))
            .exists()
    );
}

#[test]
fn scope_buttons_freeze_the_background_scope_and_keep_project_and_global_rows_separate() {
    let fixture = Fixture::new();
    fixture.install("project", "fixture.project");
    let roots = packages::roots_for_project(&fixture.project).unwrap();
    packages::stage_at(&roots, PluginScope::Global, ManagementCommand::Install {
        source: fixture.package("global", "fixture.global"), replace: false,
    }).unwrap();
    assert!(packages::apply_pending_with_roots(&roots).is_empty());
    let mut app = fixture.app();
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(app.world_mut(), ManagerAction::SetScope(PluginScope::Global), revision);
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(app.world_mut(), ManagerAction::SetScope(PluginScope::Project), revision);
    assert_eq!(app.world().resource::<PluginManagerState>().scope, PluginScope::Global);
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert_eq!(state.rows.len(), 1);
    assert_eq!(state.rows[0].id, "fixture.global");
    assert_eq!(state.rows[0].scope, PluginScope::Global);
    assert_eq!(state.sequence, 1, "忙碌时的切换不能启动第二个后台槽");
}

#[test]
fn no_project_state_rejects_project_actions_without_starting_a_task() {
    let fixture = Fixture::new();
    let mut state = PluginManagerState::with_roots(None, fixture.directory.join("owned-global"));
    assert!(!state.has_project());
    assert!(state.project_root().is_none());
    assert!(state.request_at(PluginScope::Project, ManagementCommand::Remove { id:"fixture.tool".into() }).is_err());
    assert!(state.switch_scope(PluginScope::Project).is_err());
    assert_eq!(state.scope, PluginScope::Global);
    assert!(!state.busy && state.task.is_none());
    assert_eq!(state.sequence, 0);
    assert!(!fixture.directory.join("owned-global").exists());
}

#[test]
fn migration_notice_keeps_refresh_and_pending_change_receipts() {
    let fixture = Fixture::new();
    let roots = PluginRoots {
        project: Some(fixture.project.clone()),
        global: fixture.directory.join("owned-global"),
    };
    let legacy = fixture.project.join("plugins/old-tool");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(
        legacy.join("plugin.json"),
        br#"{"format":1,"id":"fixture.tool","tool":"old"}"#,
    ).unwrap();
    packages::stage_at(&roots, PluginScope::Project, ManagementCommand::Install {
        source: fixture.package("new", "fixture.tool"),
        replace: false,
    }).unwrap();
    assert!(packages::apply_pending_with_roots(&roots).is_empty());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut manager = PluginManagerState::with_roots(roots.project, roots.global);
    manager.refresh().unwrap();
    app.insert_resource(manager);
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert!(state.status.starts_with("已读取当前工程"));
    assert!(state.status.contains("迁移提示：旧目录包fixture.tool"));
    assert_eq!(state.rows.len(), 1);
    assert!(state.rows[0].error.is_none());

    // 同一迁移说明常驻时，后台操作的成功回执和重开要求仍必须清晰保留。
    app.world_mut().resource_mut::<PluginManagerState>().request(
        ManagementCommand::SetEnabled { id: "fixture.tool".into(), enabled: false },
    ).unwrap();
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert!(state.status.starts_with("变更已记录，重开工程后生效"));
    assert!(state.status.contains("迁移提示：旧目录包fixture.tool"));
    assert!(state.rows[0].pending.is_some());
    assert!(legacy.join("plugin.json").is_file());
}

fn global_app(fixture: &Fixture) -> App {
    let mut app = fixture.app();
    let mut manager = PluginManagerState::with_roots(None, fixture.directory.join("owned-global"));
    manager.open = true;
    manager.was_open = true;
    manager.refreshed = true;
    app.insert_resource(manager);
    app
}

#[test]
fn home_global_transactions_commit_without_project_and_preserve_immutable_versions() {
    let fixture = Fixture::new();
    let source = fixture.package("home-first", "fixture.home");
    let mut app = global_app(&fixture);
    app.world_mut().resource_mut::<PluginManagerState>()
        .install_package(source.clone(), None).unwrap();
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert!(state.status.starts_with("全局变更已生效"), "{}", state.status);
    assert!(state.load_states.is_empty(), "未打开工程不能假报DLL已加载");
    assert!(state.rows[0].pending.is_none() && state.rows[0].enabled);
    let original = state.rows[0].path.clone();
    let original_bytes = fs::read(&original).unwrap();
    let global = state.roots.global.clone();

    // 重建管理器仅刷新，不能再次显示已经完成的安装计划。
    let mut replacement = PluginManagerState::with_roots(None, global.clone());
    replacement.refresh().unwrap();
    app.insert_resource(replacement);
    poll_until_idle(app.world_mut());
    assert!(app.world().resource::<PluginManagerState>().rows[0].pending.is_none());
    for enabled in [false, true] {
        app.world_mut().resource_mut::<PluginManagerState>()
            .request(ManagementCommand::SetEnabled { id: "fixture.home".into(), enabled }).unwrap();
        poll_until_idle(app.world_mut());
        let state = app.world().resource::<PluginManagerState>();
        assert!(state.status.starts_with("全局变更已生效"), "{}", state.status);
        assert_eq!(state.rows[0].enabled, enabled);
        assert!(state.rows[0].pending.is_none());
    }
    let updated = fixture.directory.join(packages::binary_filename("updated-home"));
    let mut metadata = feathrise_plugin_api::read_plugin_metadata(&source).unwrap();
    metadata.version = "2.0.0".into();
    feathrise_plugin_api::metadata::test_support::write_metadata_fixture(&updated, &metadata).unwrap();
    app.world_mut().resource_mut::<PluginManagerState>()
        .install_package(updated, Some("fixture.home".into())).unwrap();
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert_eq!(state.rows[0].version.as_deref(), Some("2.0.0"));
    assert!(state.rows[0].pending.is_none());
    assert_ne!(state.rows[0].path, original);
    assert_eq!(fs::read(&original).unwrap(), original_bytes, "旧会话可能仍映射的对象不得被改写");
    app.world_mut().resource_mut::<PluginManagerState>()
        .request(ManagementCommand::Remove { id: "fixture.home".into() }).unwrap();
    poll_until_idle(app.world_mut());
    assert!(app.world().resource::<PluginManagerState>().rows.is_empty());
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
    let journal: Journal = serde_json::from_slice(&fs::read(global.join(".management/pending.json")).unwrap()).unwrap();
    assert!(journal.changes.is_empty());
    assert_eq!(fs::read(fixture.project.join("assets/author-data.txt")).unwrap(), b"author-must-survive");
}

#[test]
fn home_refresh_recovers_a_previous_global_install_plan() {
    let fixture = Fixture::new();
    let mut app = global_app(&fixture);
    let roots = app.world().resource::<PluginManagerState>().roots.clone();
    packages::stage_at(&roots, PluginScope::Global, ManagementCommand::Install {
        source: fixture.package("old-plan", "fixture.old-plan"), replace: false,
    }).unwrap();
    app.world_mut().resource_mut::<PluginManagerState>().refresh().unwrap();
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert_eq!(state.rows[0].id, "fixture.old-plan");
    assert!(state.rows[0].pending.is_none() && state.rows[0].path.is_file());
}

#[test]
fn home_commit_failure_preserves_pending_and_does_not_claim_effective() {
    let fixture = Fixture::new();
    let mut app = global_app(&fixture);
    let global = app.world().resource::<PluginManagerState>().roots.global.clone();
    fs::create_dir_all(global.join(".management")).unwrap();
    // 真实回执落盘失败必须触发原索引回滚，不能仅清掉“待安装”文案。
    fs::write(global.join(".management/receipts"), b"receipt-path-obstacle").unwrap();
    app.world_mut().resource_mut::<PluginManagerState>()
        .install_package(fixture.package("cannot-commit", "fixture.failed"), None).unwrap();
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert!(!state.status.contains("全局变更已生效"));
    assert!(state.status.contains("未应用"), "{}", state.status);
    assert!(state.rows[0].pending.is_some());
    let journal: Journal = serde_json::from_slice(&fs::read(global.join(".management/pending.json")).unwrap()).unwrap();
    assert!(journal.changes.contains_key("fixture.failed"));
}

#[test]
fn an_open_project_keeps_global_changes_pending_for_a_new_author_session() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.world_mut().resource_mut::<PluginManagerState>().switch_scope(PluginScope::Global).unwrap();
    poll_until_idle(app.world_mut());
    app.world_mut().resource_mut::<PluginManagerState>()
        .install_package(fixture.package("project-session", "fixture.deferred"), None).unwrap();
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert!(state.rows[0].pending.is_some());
    assert!(state.status.contains("重开工程后生效"));
    assert!(!state.status.contains("全局变更已生效"));
}

#[test]
fn confirmed_legacy_remove_archives_every_original_byte_without_a_restart() {
    let fixture = Fixture::new();
    let legacy = fixture.project.join("plugins/legacy-tool");
    fs::create_dir_all(legacy.join("empty")).unwrap();
    fs::write(legacy.join("plugin.json"), br#"{"format":1,"id":"fixture.legacy","tool":"legacy"}"#).unwrap();
    fs::write(legacy.join("old.dll"), b"preserve-old-dll").unwrap();
    let mut app = fixture.app();
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(app.world_mut(), ManagerAction::RequestRemove("fixture.legacy".into()), revision);
    assert!(legacy.is_dir(), "进入确认状态不能提前归档");
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(app.world_mut(), ManagerAction::ConfirmRemove, revision);
    poll_until_idle(app.world_mut());
    let state = app.world().resource::<PluginManagerState>();
    assert!(state.rows.is_empty());
    assert!(state.status.starts_with("旧格式插件已归档移除"), "{}", state.status);
    assert!(!legacy.exists());
    let receipt = fs::read_dir(fixture.project.join(".editor/plugin-management/receipts")).unwrap().next().unwrap().unwrap().path();
    let record: serde_json::Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
    let archived = PathBuf::from(record["archived"].as_str().unwrap());
    assert!(archived.starts_with(fixture.project.join(".editor/plugin-management/backups")));
    assert_eq!(fs::read(archived.join("old.dll")).unwrap(), b"preserve-old-dll");
    assert!(archived.join("empty").is_dir());
    assert_eq!(fs::read(fixture.project.join("assets/author-data.txt")).unwrap(), b"author-must-survive");
}

fn diagnostic_native(
    errors: Vec<String>,
    states: std::collections::BTreeMap<String, String>,
    origins: std::collections::BTreeMap<String, PluginOrigin>,
) -> crate::native_plugins::resources::NativeTools {
    let (_, receiver) = async_channel::bounded(1);
    crate::native_plugins::resources::NativeTools {
        tools: Default::default(), receiver, discovery_pending: false, loading: 0,
        errors, management_errors: Vec::new(), load_states: states, load_origins: origins,
        ui_revision: 1, diagnostics: Default::default(), input_pending: false,
        save_tickets: Default::default(),
    }
}

fn legacy_with_diagnostics(fixture: &Fixture) -> (PathBuf, PluginRoots, crate::native_plugins::loader::Discovery) {
    let legacy = fixture.project.join("plugins/legacy-diagnostics");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("plugin.json"), br#"{"format":1,"id":"fixture.legacy","tool":"legacy"}"#).unwrap();
    fs::write(legacy.join("old.dll"), b"legacy-source").unwrap();
    for name in ["bad-one", "bad-two"] {
        fs::write(fixture.project.join("plugins").join(packages::binary_filename(name)), b"invalid-binary").unwrap();
    }
    let roots = packages::roots_for_project(&fixture.project).unwrap();
    let discovery = crate::native_plugins::loader::discover_with_roots(&roots).unwrap();
    assert!(discovery.packages.is_empty(), "本夹具不伪造DLL装载成功");
    assert_eq!(discovery.warnings.len(), 3);
    assert!(discovery.origins.contains_key("fixture.legacy"));
    (legacy, roots, discovery)
}

#[test]
fn utf8_bom_legacy_archive_preserves_bytes_and_resolves_its_own_diagnostic() {
    use crate::native_plugins::resources::NativeTools;
    let fixture = Fixture::new();
    let (legacy, roots, _) = legacy_with_diagnostics(&fixture);
    let manifest = legacy.join("plugin.json");
    let mut original = vec![0xef, 0xbb, 0xbf];
    original.extend(fs::read(&manifest).unwrap());
    fs::write(&manifest, &original).unwrap();
    // Windows旧工具可能保留UTF-8 BOM；列表、诊断来源和归档必须识别同一份清单原字节。
    let catalog = packages::catalog_at(&roots, PluginScope::Project).unwrap();
    let entry = catalog.entries.iter().find(|entry| entry.id == "fixture.legacy").unwrap();
    assert!(entry.legacy);
    let discovery = crate::native_plugins::loader::discover_with_roots(&roots).unwrap();
    assert!(discovery.origins.contains_key("fixture.legacy"), "已识别的BOM旧包仍须保留精确诊断来源");
    let remaining: Vec<_> = discovery.warnings.iter()
        .filter(|error| !error.starts_with("fixture.legacy：")).cloned().collect();
    let mut app = fixture.app();
    app.insert_resource(diagnostic_native(discovery.warnings, discovery.states, discovery.origins));
    systems::poll(app.world_mut());
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(app.world_mut(), ManagerAction::Select("fixture.legacy".into()), revision);
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(app.world_mut(), ManagerAction::RequestRemove("fixture.legacy".into()), revision);
    assert!(legacy.is_dir(), "确认前不得移动旧包");
    let revision = app.world().resource::<PluginManagerState>().revision;
    click(app.world_mut(), ManagerAction::ConfirmRemove, revision);
    poll_until_idle(app.world_mut());
    assert!(!legacy.exists(), "确认移除须完成真实归档事务");
    assert_eq!(app.world().resource::<NativeTools>().errors, remaining);
    let receipt = fs::read_dir(fixture.project.join(".editor/plugin-management/receipts"))
        .unwrap().next().unwrap().unwrap().path();
    let record: serde_json::Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
    let archived = PathBuf::from(record["archived"].as_str().unwrap());
    assert_eq!(fs::read(archived.join("plugin.json")).unwrap(), original, "归档须保留BOM及完整原字节");
    assert_eq!(fs::read(archived.join("old.dll")).unwrap(), b"legacy-source");
    assert_eq!(fs::read(fixture.project.join("assets/author-data.txt")).unwrap(), b"author-must-survive");
}

#[test]
fn archive_resolves_only_its_unloaded_legacy_diagnostic_and_keeps_other_failures_blocking() {
    use crate::native_plugins::resources::NativeTools;
    let fixture = Fixture::new();
    let (_, _, discovery) = legacy_with_diagnostics(&fixture);
    let mut app = fixture.app();
    app.insert_resource(diagnostic_native(discovery.warnings, discovery.states, discovery.origins));
    crate::native_plugins::adapter::install(&mut app);
    systems::poll(app.world_mut()); // 冻结实际发现产生的诊断实例，而非只按同ID删除字符串。
    let before = app.world().resource::<NativeTools>().diagnostics;
    let remaining: Vec<_> = app.world().resource::<NativeTools>().errors.iter()
        .filter(|error| !error.starts_with("fixture.legacy：")).cloned().collect();
    app.world_mut().resource_mut::<PluginManagerState>()
        .request(ManagementCommand::Remove { id: "fixture.legacy".into() }).unwrap();
    poll_until_idle(app.world_mut());
    let native = app.world().resource::<NativeTools>();
    assert_eq!(native.errors, remaining);
    assert!(!native.load_states.contains_key("fixture.legacy"));
    assert!(!native.load_origins.contains_key("fixture.legacy"));
    assert_ne!(native.diagnostics, before);
    let blocker = feathrise_editor_map::standalone::saved_play_blocker(app.world()).unwrap();
    let original_error = remaining.first().expect("两个真实坏包错误必须仍然保留");
    assert!(blocker.starts_with(original_error), "F12仍须展示剩余坏包的原始错误");
    assert_eq!(blocker, format!("{original_error}，完成后请重新启动工程；草稿和历史已保留"));
}

#[test]
fn archive_keeps_diagnostics_if_the_request_diagnostic_generation_changed() {
    use crate::native_plugins::resources::NativeTools;
    let fixture = Fixture::new();
    let (_, _, discovery) = legacy_with_diagnostics(&fixture);
    let mut app = fixture.app();
    app.insert_resource(diagnostic_native(discovery.warnings, discovery.states, discovery.origins));
    systems::poll(app.world_mut());
    app.world_mut().resource_mut::<PluginManagerState>()
        .request(ManagementCommand::Remove { id: "fixture.legacy".into() }).unwrap();
    // 模拟该操作在途时新的真实诊断批次被采用；旧回执不能清理新代次中的同ID来源。
    app.world_mut().resource_mut::<NativeTools>().diagnostics.advance();
    let before = app.world().resource::<NativeTools>().errors.clone();
    poll_until_idle(app.world_mut());
    assert_eq!(app.world().resource::<NativeTools>().errors, before);
    assert!(app.world().resource::<PluginManagerState>().status.contains("重开工程后重新检查"));
}

#[test]
fn archive_resolution_rejects_missing_origin_duplicate_diagnostics_and_reused_source() {
    let fixture = Fixture::new();
    let (legacy, roots, discovery) = legacy_with_diagnostics(&fixture);
    let result = packages::stage_at(&roots, PluginScope::Project,
        ManagementCommand::Remove { id: "fixture.legacy".into() }).unwrap();
    let archived = result.archived_legacy.unwrap();
    assert!(archived.original_vacant);
    let native = || diagnostic_native(discovery.warnings.clone(), discovery.states.clone(), discovery.origins.clone());
    let mut missing = native();
    missing.load_origins.remove("fixture.legacy");
    assert!(missing.clear_archived_legacy(&archived, &result.entries).is_err());
    assert_eq!(missing.errors, discovery.warnings);
    let mut duplicate = native();
    let warning = duplicate.errors.iter().find(|error| error.starts_with("fixture.legacy：")).unwrap().clone();
    duplicate.errors.push(warning);
    assert!(duplicate.clear_archived_legacy(&archived, &result.entries).is_err());
    assert_eq!(duplicate.errors.len(), 4);
    let mut other_origin = native();
    other_origin.load_origins.get_mut("fixture.legacy").unwrap().path = fixture.directory.join("different-source");
    assert!(other_origin.clear_archived_legacy(&archived, &result.entries).is_err());
    let mut loaded = native();
    let (requests, _) = async_channel::bounded(1);
    loaded.tools.insert(0, crate::native_plugins::types::Tool {
        domain: "fixture.legacy",
        snapshot: feathrise_plugin_api::Snapshot::default(),
        requests, queue: Default::default(), inflight: None, completed: 0, sequence: 0,
        fault: Some("live-tool-fault".into()), library_open: false, close_confirmation: None,
        choices: None, latest_activation: 0, map_pending: false,
    });
    assert!(loaded.clear_archived_legacy(&archived, &result.entries).is_err());
    assert_eq!(loaded.tools[&0].fault.as_deref(), Some("live-tool-fault"));
    assert_eq!(loaded.errors, discovery.warnings);
    // 移除工程旧目录后出现同ID全局副本，仍须新作者实例重新检查，不能按旧诊断冒报健康。
    packages::stage_at(&roots, PluginScope::Global, ManagementCommand::Install {
        source: fixture.package("same-id-global", "fixture.legacy"), replace: false,
    }).unwrap();
    assert!(packages::apply_pending_with_roots(&roots).is_empty());
    let same_id_catalog = packages::catalog_at(&roots, PluginScope::Project).unwrap();
    assert!(same_id_catalog.entries.iter().any(|entry| entry.id == "fixture.legacy" && entry.inherited));
    let mut same_id = native();
    assert!(same_id.clear_archived_legacy(&archived, &same_id_catalog.entries).is_err());
    assert_eq!(same_id.errors, discovery.warnings);
    // 真实复用原位置，哪怕同ID、相同清单SHA也不能被当作刚归档的旧对象。
    fs::create_dir(&legacy).unwrap();
    fs::write(legacy.join("plugin.json"), br#"{"format":1,"id":"fixture.legacy","tool":"legacy"}"#).unwrap();
    let current = packages::catalog_at(&roots, PluginScope::Project).unwrap();
    let mut reused = native();
    assert!(reused.clear_archived_legacy(&archived, &current.entries).is_err());
    assert_eq!(reused.errors, discovery.warnings);
}

#[test]
fn archive_resolution_rejects_a_manifest_changed_since_discovery() {
    let fixture = Fixture::new();
    let (legacy, roots, discovery) = legacy_with_diagnostics(&fixture);
    fs::write(legacy.join("plugin.json"), br#"{"format":1,"id":"fixture.legacy","tool":"changed"}"#).unwrap();
    let result = packages::stage_at(&roots, PluginScope::Project,
        ManagementCommand::Remove { id: "fixture.legacy".into() }).unwrap();
    let mut native = diagnostic_native(discovery.warnings.clone(), discovery.states, discovery.origins);
    assert!(native.clear_archived_legacy(result.archived_legacy.as_ref().unwrap(), &result.entries).is_err());
    assert_eq!(native.errors, discovery.warnings);
}
