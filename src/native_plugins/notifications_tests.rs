//! 通知生命周期回归：真实错误资源驱动去重、关闭、模态隔离和既有管理器入口。
use super::*;
use crate::native_plugins::management::{resources::PluginManagerState, types::{PackageEntry, PluginScope}};

fn native(errors: Vec<String>) -> NativeTools {
    let (_, receiver) = async_channel::bounded(1);
    NativeTools {
        tools: default(), receiver, discovery_pending: false, loading: 0, errors,
        management_errors: default(), load_states: [("fixture.bad".into(), "加载失败".into())].into(),
        load_origins: default(), ui_revision: 1, diagnostics: default(), input_pending: false, save_tickets: default(),
    }
}

fn app(errors: Vec<String>) -> App {
    let mut app = App::new();
    app.insert_resource(native(errors)).init_resource::<NotificationState>().add_systems(Update, (sync, control).chain());
    app.insert_resource(PluginManagerState::with_roots(Some(std::env::temp_dir().join("unused-notice-project")),
        std::env::temp_dir().join("unused-notice-global")));
    app.update();
    app
}

fn send(app: &mut App, action: NotificationAction) {
    let entity = app.world_mut().spawn((action, EditorButtonClick::triggered())).id();
    app.update();
    app.world_mut().despawn(entity);
}

fn finish_scan(app: &mut App) {
    for _ in 0..20_000 {
        if app.world().resource::<NotificationState>().scan.is_none() { return; }
        app.update();
    }
    panic!("固定预算诊断扫描未能完成");
}

fn error_row(id: &str) -> PackageEntry {
    PackageEntry {
        id: id.into(), tool: "坏插件".into(), version: None,
        directory: "unused-notice-project/plugins/bad".into(), path: "unused-notice-project/plugins/bad.dll".into(),
        scope: PluginScope::Project, inherited: false, effective: true, overridden_by_project: false,
        global_available: false, legacy: false, origin: None, enabled: false,
        error: Some("实际发现错误".into()), pending: None, runtime_dependencies: vec![], runtime_missing: vec![],
    }
}

#[test]
fn dismiss_keeps_errors_and_same_content_does_not_reopen_until_a_new_batch() {
    let message = "fixture.bad：动态库损坏".to_string();
    let mut app = app(vec![message.clone(), message.clone()]);
    assert_eq!(app.world().resource::<NotificationState>().count, 1);
    assert!(app.world().resource::<NotificationState>().visible);
    send(&mut app, NotificationAction::Dismiss);
    assert!(!app.world().resource::<NotificationState>().visible);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.ui_revision += 1;
        native.errors.push(message.clone());
        native.diagnostics.advance();
    }
    app.update();
    assert!(!app.world().resource::<NotificationState>().visible, "重复错误和普通快照刷新不能重新弹出");
    assert_eq!(app.world().resource::<NativeTools>().errors.len(), 3);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.ui_revision += 1;
        native.errors.push("fixture.other：入口缺失".into());
        native.diagnostics.advance();
    }
    app.update();
    assert!(app.world().resource::<NotificationState>().visible);
    assert_eq!(app.world().resource::<NotificationState>().count, 2);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.errors.clear();
        native.diagnostics.advance();
        native.ui_revision += 1;
    }
    app.update();
    assert!(!app.world().resource::<NotificationState>().visible);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.errors.push(message);
        native.diagnostics.advance();
        native.ui_revision += 1;
    }
    app.update();
    assert!(app.world().resource::<NotificationState>().visible, "清空后再次失败属于新的诊断批次");
}

#[test]
fn inspect_opens_the_original_manager_and_selects_the_actual_failed_plugin() {
    let mut app = app(vec!["fixture.bad：动态库损坏".into()]);
    app.world_mut().resource_mut::<PluginManagerState>().rows =
        vec![error_row("fixture.other"), error_row("fixture.bad")];
    send(&mut app, NotificationAction::Inspect);
    let manager = app.world().resource::<PluginManagerState>();
    assert!(manager.open);
    assert_eq!(manager.selected.as_deref(), Some("fixture.bad"));
    assert!(!app.world().resource::<NotificationState>().visible);
    assert_eq!(app.world().resource::<NativeTools>().errors.len(), 1);
}

#[test]
fn modal_blocks_notification_actions_and_does_not_replay_them_after_closing() {
    let mut app = app(vec!["fixture.bad：动态库损坏".into()]);
    app.world_mut().resource_mut::<PluginManagerState>().open = true;
    send(&mut app, NotificationAction::Dismiss);
    assert!(app.world().resource::<NotificationState>().visible);
    app.world_mut().resource_mut::<PluginManagerState>().open = false;
    app.update();
    assert!(app.world().resource::<NotificationState>().visible);
}

#[test]
fn summary_is_bounded_while_full_diagnostics_remain_in_the_source_resource() {
    let errors = (0..400).map(|index| format!("fixture.{index}：{}", "超长错误正文".repeat(500))).collect();
    let mut app = app(errors);
    finish_scan(&mut app);
    let state = app.world().resource::<NotificationState>();
    assert!(state.summary.len() < 512);
    assert!(state.count <= MAX_ERRORS);
    assert_eq!(app.world().resource::<NativeTools>().errors.len(), 400);
    app.world_mut().resource_mut::<NativeTools>().ui_revision += 1;
    app.update();
    assert!(app.world().resource::<NotificationState>().visible);
}

#[test]
fn a_change_after_the_old_1024_byte_prefix_still_opens_a_new_notice() {
    let prefix = format!("fixture.bad：{}", "长路径".repeat(500));
    let mut app = app(vec![format!("{prefix}原因A")]);
    finish_scan(&mut app);
    let summary = app.world().resource::<NotificationState>().summary.clone();
    send(&mut app, NotificationAction::Dismiss);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.errors[0] = format!("{prefix}原因B");
        native.diagnostics.advance();
    }
    app.update();
    finish_scan(&mut app);
    assert!(app.world().resource::<NotificationState>().visible);
    assert_eq!(app.world().resource::<NotificationState>().summary, summary,
        "摘要可以相同，但完整正文身份不同仍须重新提示");
}

#[test]
fn the_257th_error_participates_in_the_full_collection_identity() {
    let mut app = app((0..257).map(|index| format!("fixture.{index}：原错误")).collect());
    finish_scan(&mut app);
    assert_eq!(app.world().resource::<NotificationState>().count, 257);
    send(&mut app, NotificationAction::Dismiss);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.errors[256] = "fixture.256：新的最后一条错误".into();
        native.diagnostics.advance();
    }
    app.update();
    finish_scan(&mut app);
    assert!(app.world().resource::<NotificationState>().visible);
    assert_eq!(app.world().resource::<NotificationState>().count, 257);
}

#[test]
fn scanning_obeys_byte_and_entry_budgets_and_rejects_a_stale_source() {
    let mut app = app(vec!["长".repeat(SCAN_BYTES_PER_FRAME)]);
    {
        let state = app.world().resource::<NotificationState>();
        let scan = state.scan.as_ref().unwrap();
        assert_eq!(scan.offset, SCAN_BYTES_PER_FRAME);
        assert!(!state.visible);
    }
    app.update();
    assert_eq!(app.world().resource::<NotificationState>().scan.as_ref().unwrap().offset,
        2 * SCAN_BYTES_PER_FRAME);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.errors = vec!["fixture.bad：新的来源".into()];
        native.diagnostics = default();
    }
    app.update();
    let state = app.world().resource::<NotificationState>();
    assert!(state.scan.is_none() && state.visible);
    assert!(state.summary.contains("新的来源"));
    assert!(!state.summary.contains("长长"));
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.errors = (0..40).map(|index| format!("fixture.{index}：错误")).collect();
        native.diagnostics.advance();
    }
    app.update();
    assert_eq!(app.world().resource::<NotificationState>().scan.as_ref().unwrap().index,
        SCAN_ENTRIES_PER_FRAME);
    assert!(!app.world().resource::<NotificationState>().visible);
    finish_scan(&mut app);
    assert_eq!(app.world().resource::<NotificationState>().count, 40);
}

#[test]
fn identical_reordered_and_repeated_errors_do_not_reopen_a_dismissed_notice() {
    let mut app = app(vec!["fixture.a：错误A".into(), "fixture.b：错误B".into()]);
    send(&mut app, NotificationAction::Dismiss);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.errors = vec!["fixture.b：错误B".into(), "fixture.a：错误A".into(), "fixture.b：错误B".into()];
        native.diagnostics.advance();
    }
    app.update();
    assert!(!app.world().resource::<NotificationState>().visible);
    assert_eq!(app.world().resource::<NotificationState>().count, 2);
}

#[test]
fn oversized_collection_reports_an_explicit_budget_error_without_discarding_source_records() {
    let mut app = app((0..=MAX_ERRORS).map(|index| format!("fixture.{index}：错误")).collect());
    let state = app.world().resource::<NotificationState>();
    assert!(state.visible && state.scan.is_none());
    assert!(state.summary.contains("超出1024条分析预算"));
    assert_eq!(app.world().resource::<NativeTools>().errors.len(), MAX_ERRORS + 1);
    send(&mut app, NotificationAction::Dismiss);
    app.world_mut().resource_mut::<NativeTools>().ui_revision += 1;
    app.update();
    assert!(!app.world().resource::<NotificationState>().visible);
}

#[test]
fn ordinary_snapshots_only_use_constant_time_gate_and_equal_length_replacement_is_detected() {
    let mut app = app(vec!["fixture.bad：旧错误".into()]);
    send(&mut app, NotificationAction::Dismiss);
    let batches = app.world().resource::<NotificationState>().scanned_batches;
    for _ in 0..20 {
        app.world_mut().resource_mut::<NativeTools>().ui_revision += 1;
        app.update();
    }
    assert_eq!(app.world().resource::<NotificationState>().scanned_batches, batches);
    assert!(!app.world().resource::<NotificationState>().visible);
    {
        let mut native = app.world_mut().resource_mut::<NativeTools>();
        native.errors[0] = "fixture.bad：新错误".into();
        native.diagnostics.advance();
    }
    app.update();
    assert_eq!(app.world().resource::<NotificationState>().scanned_batches, batches + 1);
    assert!(app.world().resource::<NotificationState>().visible);
    assert!(app.world().resource::<NotificationState>().summary.contains("新错误"));
}

#[test]
fn reconstructed_source_starts_a_new_notice_even_with_identical_diagnostics() {
    let mut app = app(vec!["fixture.bad：重建来源".into()]);
    send(&mut app, NotificationAction::Dismiss);
    let batches = app.world().resource::<NotificationState>().scanned_batches;
    // 新作者来源按构造合同取得新instance，不借错误条数或Vec分配地址猜测生命周期。
    app.world_mut().resource_mut::<NativeTools>().diagnostics = default();
    app.update();
    assert_eq!(app.world().resource::<NotificationState>().scanned_batches, batches + 1);
    assert!(app.world().resource::<NotificationState>().visible);
}

#[test]
fn inspect_waits_for_busy_global_refresh_then_locates_the_project_failure() {
    use crate::native_plugins::management::{packages, types::{ManagementCommand, PackageManifest, PluginRoots}};
    use std::{fs, time::{Duration, Instant}};
    let root = std::env::temp_dir().join(format!("feathrise-notice-scope-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    fs::write(root.join("owner"), b"notification-scope-fixture").unwrap();
    let project = root.join("project");
    fs::create_dir_all(project.join("plugins")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname='notification_fixture'\nversion='0.1.0'\nedition='2024'\n").unwrap();
    fs::write(project.join("plugins").join(packages::binary_filename("fixture.bad")), b"invalid project binary").unwrap();
    let roots = PluginRoots { project: Some(project.clone()), global: root.join("global") };
    let source = root.join(packages::binary_filename("fixture.bad"));
    feathrise_plugin_api::metadata::test_support::write_metadata_fixture(&source, &PackageManifest {
        format: 1, id: "fixture.bad".into(), tool: "健康全局副本".into(), version: "1.0.0".into(),
        target_os: std::env::consts::OS.into(), target_arch: std::env::consts::ARCH.into(),
        abi_major: feathrise_plugin_api::ABI_MAJOR, runtime_dependencies: vec![], runtime: None,
    }).unwrap();
    packages::stage_at(&roots, PluginScope::Global, ManagementCommand::Install { source, replace: false }).unwrap();
    assert!(packages::apply_pending_with_roots(&roots).is_empty());
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, bevy::asset::AssetPlugin {
        file_path: root.to_string_lossy().into(), watch_for_changes_override: Some(false), ..default()
    })).init_asset::<Font>();
    management::install(&mut app, project);
    install(&mut app);
    app.insert_resource(native(vec!["fixture.bad：工程副本损坏".into()]));
    let mut manager = PluginManagerState::with_roots(roots.project.clone(), roots.global.clone());
    manager.scope = PluginScope::Global;
    manager.rows = packages::catalog_at(&roots, PluginScope::Global).unwrap().entries;
    assert!(manager.rows.iter().any(|row| row.id == "fixture.bad" && row.scope == PluginScope::Global && row.error.is_none()));
    app.insert_resource(manager);
    app.update();
    app.world_mut().resource_mut::<PluginManagerState>().refresh().unwrap();
    let pulse = app.world_mut().spawn((NotificationAction::Inspect, EditorButtonClick::triggered())).id();
    control(app.world_mut());
    app.world_mut().despawn(pulse);
    assert!(app.world().resource::<PluginManagerState>().open);
    assert!(app.world().resource::<PluginManagerState>().busy);
    assert_eq!(app.world().resource::<PluginManagerState>().scope, PluginScope::Global);
    assert!(app.world().resource::<NotificationState>().select_after_open);
    let started = Instant::now();
    loop {
        app.update();
        let manager = app.world().resource::<PluginManagerState>();
        if manager.scope == PluginScope::Project && !manager.busy
            && manager.selected.as_deref() == Some("fixture.bad")
            && !app.world().resource::<NotificationState>().select_after_open {
            let selected = manager.rows.iter().find(|row| row.id == "fixture.bad").unwrap();
            assert_eq!(selected.scope, PluginScope::Project);
            assert!(selected.error.is_some());
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "通知没有完成工程错误定位");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!app.world().resource::<NotificationState>().select_after_open);
    drop(app);
    assert_eq!(root.parent().unwrap(), std::env::temp_dir().canonicalize().unwrap());
    assert_eq!(fs::read(root.join("owner")).unwrap(), b"notification-scope-fixture");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn closing_the_manager_cancels_a_waiting_inspect_instead_of_replaying_later() {
    let mut app = app(vec!["fixture.bad：工程副本损坏".into()]);
    {
        let mut manager = app.world_mut().resource_mut::<PluginManagerState>();
        manager.scope = PluginScope::Global;
        manager.busy = true;
    }
    send(&mut app, NotificationAction::Inspect);
    assert!(app.world().resource::<NotificationState>().select_after_open);
    app.world_mut().resource_mut::<PluginManagerState>().open = false;
    app.update();
    assert!(!app.world().resource::<NotificationState>().select_after_open);
    {
        let mut manager = app.world_mut().resource_mut::<PluginManagerState>();
        manager.busy = false;
        manager.open = true;
    }
    app.update();
    assert_eq!(app.world().resource::<PluginManagerState>().scope, PluginScope::Global);
    assert!(!app.world().resource::<NotificationState>().select_after_open);
}
