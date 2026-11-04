//! 导出生命周期回归：关闭只取消自己的后台槽，真正收口前保留窗口和工作状态。

use super::{resources::*, types::ExportReceipt};
use bevy::prelude::*;
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc}, time::{Duration, Instant}};

fn state() -> ExportState {
    ExportState { open: true, destination: "unused/output".into(), status: String::new(),
        failed: None, receipt: None, revision: 1, working: false }
}

#[test]
fn input_lease_rejects_changes_even_when_content_is_later_restored() {
    let valid = Arc::new(AtomicBool::new(true));
    let checked = valid.clone();
    let control = crate::build_process::BuildControl::with_input_guard(None,
        Some(crate::build_process::InputGuard(Arc::new(move || checked.load(Ordering::Acquire)))));
    assert!(control.check().is_ok());
    valid.store(false, Ordering::Release);
    assert!(control.check().unwrap_err().contains("源码或构建配置已变化"));
    assert!(!control.is_cancelled());
}

#[test]
fn close_waits_for_owned_worker_and_does_not_drop_the_active_slot() {
    let mut app = App::new();
    app.insert_resource(state()).init_non_send::<ExportRuntime>();
    super::ui::install(&mut app);
    let control = crate::build_process::BuildControl::default();
    let worker_control = control.clone();
    let (release, released) = mpsc::sync_channel::<()>(1);
    let (_progress, progress) = mpsc::sync_channel(8);
    let (result_tx, result) = mpsc::sync_channel::<Result<ExportReceipt, String>>(1);
    let handle = std::thread::spawn(move || {
        let _ = released.recv_timeout(Duration::from_secs(2));
        let _ = result_tx.send(worker_control.check().map(|_| unreachable!("关闭应先取消")));
    });
    app.world_mut().non_send_mut::<ExportRuntime>().active = Some(ActiveExport { control: control.clone(), progress, result, handle });
    app.world_mut().resource_mut::<ExportState>().working = true;
    assert!(!super::ready_to_exit(app.world_mut()).unwrap());
    assert!(control.is_cancelled());
    assert!(app.world().resource::<ExportState>().busy());
    assert!(app.world().non_send::<ExportRuntime>().active.is_some());
    assert!(app.world().non_send::<ExportRuntime>().stopping,
        "未安装关闭事务的退出调用不能自行撤回停止意图");
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !super::ready_to_exit(app.world_mut()).unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!app.world().resource::<ExportState>().busy());
    assert!(app.world().resource::<ExportState>().failed.is_none());
}

#[test]
fn cancelling_authorized_close_restores_export_gate_after_the_owned_worker_drains() {
    use crate::native_project_close::{NativeProjectCloseState, authorize_exit};
    let mut app = App::new();
    app.insert_resource(state()).init_non_send::<ExportRuntime>()
        .init_resource::<NativeProjectCloseState>()
        .add_message::<feathrise_editor_map::standalone::SavedPlayFeedback>();
    super::ui::install(&mut app);
    let control = crate::build_process::BuildControl::default();
    let worker_control = control.clone();
    let (release, released) = mpsc::sync_channel::<()>(1);
    let (_progress, progress) = mpsc::sync_channel(8);
    let (result_tx, result) = mpsc::sync_channel::<Result<ExportReceipt, String>>(1);
    let handle = std::thread::spawn(move || {
        let _ = released.recv_timeout(Duration::from_secs(2));
        let _ = result_tx.send(worker_control.check().map(|_| unreachable!("已授权关闭应取消本次任务")));
    });
    app.world_mut().non_send_mut::<ExportRuntime>().active = Some(ActiveExport { control: control.clone(), progress, result, handle });
    app.world_mut().resource_mut::<ExportState>().working = true;

    assert!(!authorize_exit(app.world_mut(), true));
    assert!(!control.is_cancelled(), "仅展示关闭确认不能取消在途导出");
    crate::native_project_close::tests::choose_discard(app.world_mut());
    assert!(authorize_exit(app.world_mut(), false));
    assert!(!super::ready_to_exit(app.world_mut()).unwrap());
    assert!(control.is_cancelled());
    assert!(app.world().non_send::<ExportRuntime>().stopping);

    // 通过原关闭协调器撤销已授权的Draining，而不是直接把导出状态写成成功或空闲。
    crate::native_project_close::tests::choose_cancel(app.world_mut());
    assert!(!authorize_exit(app.world_mut(), false));
    assert!(!app.world().resource::<NativeProjectCloseState>().active());
    super::systems::poll(app.world_mut());
    assert!(!app.world().non_send::<ExportRuntime>().stopping);
    assert!(app.world().non_send::<ExportRuntime>().active.is_some(), "原任务仍须实际收口");
    assert!(control.is_cancelled(), "取消关闭不能复活已取消的导出");

    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.world().resource::<ExportState>().busy() {
        super::systems::poll(app.world_mut());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!app.world().non_send::<ExportRuntime>().stopping);
    assert!(app.world().non_send::<ExportRuntime>().active.is_none());
    assert!(app.world().resource::<ExportState>().failed.is_none());
    app.world_mut().non_send_mut::<ExportRuntime>().failure = Some("收口失败仍需处理".into());
    super::systems::poll(app.world_mut());
    assert_eq!(app.world().non_send::<ExportRuntime>().failure.as_deref(), Some("收口失败仍需处理"));
}

#[test]
fn completed_worker_receipt_is_the_only_success_state() {
    let mut app = App::new();
    app.insert_resource(state()).init_non_send::<ExportRuntime>();
    let control = crate::build_process::BuildControl::default();
    let (_progress, progress) = mpsc::sync_channel(8);
    let (result_tx, result) = mpsc::sync_channel(1);
    let handle = std::thread::spawn(move || {
        result_tx.send(Ok(ExportReceipt { directory: PathBuf::from("owned/output"),
            executable: PathBuf::from("owned/output/scene.exe"), files: 7, bytes: 128, warning: None })).unwrap();
    });
    app.world_mut().non_send_mut::<ExportRuntime>().active = Some(ActiveExport { control, progress, result, handle });
    app.world_mut().resource_mut::<ExportState>().working = true;
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.world().resource::<ExportState>().busy() {
        super::systems::poll(app.world_mut());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let state = app.world().resource::<ExportState>();
    assert_eq!(state.receipt.as_ref().unwrap().files, 7);
    assert!(state.status.starts_with("导出完成"));
    assert!(state.failed.is_none());
}
