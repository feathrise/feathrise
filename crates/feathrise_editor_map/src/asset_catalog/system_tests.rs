//! 最小 ECS 异步生命周期回归；可控延迟只属于测试夹具，生产帧始终使用非阻塞 check_ready。

use std::sync::mpsc::{SyncSender, sync_channel}; // 夹具模拟尚未结束的单个 IO，不访问实际作者资产。
use std::time::Duration; // 所有测试等待都有明确截止，不允许挂起整个测试进程。

use bevy::app::TaskPoolPlugin; // 初始化正式 IoTaskPool，全程不创建原生窗口。
use bevy::prelude::*; // App、World 和资源允许执行正式 Tasks 系统链。
use bevy::tasks::IoTaskPool; // 使用与生产相同的唯一后台任务类型。
use feathrise_map_content::MapAssetKind; // 夹具条目仍是正式 Scene 类型。

use super::resources::EditorAssetCatalog; // 目录域内部测试可以检查任务、定位与退役槽。
use super::reveal::request_asset_reveal; // 新请求必须走正式入口，不手改定位身份。
use super::scan::finish_asset_scan; // 构造已经完成后台排序的可控结果。
use super::systems::{poll_asset_scan, start_asset_scan}; // 测试正式单航班启动和非阻塞采用系统。
use super::types::AssetScanRequest; // 冻结旧任务的完整输入身份。
use crate::resources::MapEditorState; // F12 关闭和所选文件参与取消门。
use crate::types::EditorAssetEntry; // 无需加载真正 GLB 即可观察目录/选择副作用。
use crate::ui::sidebars::MapEditorSidebarState; // 右侧资产页状态由正式请求入口设置。

/// 夹具失败或提前返回时也释放模拟 IO，避免遗留占用后台线程的测试任务。
struct ReleaseIo(Option<SyncSender<()>>); // 容量一且最多发一次，测试主线程发送不会阻塞。

impl Drop for ReleaseIo {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(()); // 任务已经完成或超时时不二次 panic。
        }
    }
}

/// 构造已有完整目录和选中预览，仅在私有夹具中安装一个可延迟任务。
fn setup() -> (App, ReleaseIo) {
    let mut app = App::new(); // 不依赖 GPU 或 AssetServer 的正式系统初始化回归。
    app.add_plugins(TaskPoolPlugin::default()); // 测试所有后台句柄使用公共任务池。
    let file = |path: &str| EditorAssetEntry {
        asset_path: path.to_owned(), // 完整路径是唯一选择身份。
        display_name: path.rsplit('/').next().unwrap().to_owned(), // 仅用于读者可辨的夹具名称。
        is_directory: false,         // 选择文件会触发现有预览链。
        kind: MapAssetKind::Scene,   // 不需要真的加载 GLB 来检测选择变化。
    };
    let mut catalog = EditorAssetCatalog {
        current_folder: "old".to_owned(), // 当前显示目录与后台目标不同。
        entries: vec![file("old/previous.glb")], // 旧预览有对应的可见列表项。
        requested_revision: 4,            // 模拟一个已完整应用的目录代次。
        applied_revision: 4,              // 起始没有普通扫描责任。
        ..default()                       // 尚未安装后台夹具任务。
    };
    let mut editor = MapEditorState {
        open: true,                                          // 正在编辑器内发起定位。
        selected_asset: Some("old/previous.glb".to_owned()), // 完成前必须保持旧选择。
        ..default()                                          // 不存在地图作者写入或关闭请求。
    };
    let mut sidebar = MapEditorSidebarState::default(); // 请求本身负责显示资产页。
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "first/a.glb").unwrap(); // 产生真正的定位票据。
    let reveal = catalog.reveal.as_ref().unwrap(); // 安装任务时冻结此刻完整身份。
    let completed = finish_asset_scan(
        AssetScanRequest {
            revision: reveal.revision,                   // 旧任务将在新版请求之后过期。
            folder: reveal.folder.clone(),               // 目标只属于 first 目录。
            reveal_file: Some(reveal.file_path.clone()), // 不是纯文件名匹配。
        },
        Ok(vec![file("first/a.glb")]), // 候选自身合法，丢弃必须是身份门而不是文件错误。
    );
    let (sender, receiver) = sync_channel(1); // 主线程只负责一次非阻塞释放信号。
    catalog.task = Some(IoTaskPool::get().spawn(async move {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("测试夹具必须及时释放模拟 IO"); // 仅测试模拟外部延迟，绝不用于生产帧。
        Some(completed) // 非阻塞轮询实际接收 Task 完成值。
    }));
    app.insert_resource(catalog) // 与正式插件相同的三个资源。
        .insert_resource(editor) // 主线程始终拥有编辑器会话。
        .insert_resource(sidebar) // 右侧页签可以随时变化并取消定位。
        .add_systems(Update, (start_asset_scan, poll_asset_scan).chain()); // 与正式插件的 Tasks 顺序一致。
    (app, ReleaseIo(Some(sender))) // Drop 释放，不依赖测试成功路径才能停止模拟等待。
}

/// 测试允许有界等待后台；生产系统每次只轮询一次，绝不调用此夹具函数。
fn advance_until(app: &mut App, ready: impl Fn(&EditorAssetCatalog) -> bool) {
    for _ in 0..5_000 {
        app.update(); // 执行一次真实主线程 Update；任务未就绪时必须立即返回。
        if ready(app.world().resource::<EditorAssetCatalog>()) {
            return; // 只等到测试所需的明确状态边界。
        }
        std::thread::sleep(Duration::from_millis(1)); // 仅测试让出 CPU，最多五秒截止。
    }
    panic!("资产目录后台状态未在测试截止内收敛"); // 不能用无限轮询掩盖任务槽或取消缺陷。
}

#[test]
fn newer_reveal_keeps_the_old_task_in_flight_until_it_really_finishes() {
    let (mut app, release) = setup(); // first/a.glb 的任务正在夹具中等待释放。
    app.world_mut()
        .resource_scope(|world, mut catalog: Mut<EditorAssetCatalog>| {
            world.resource_scope(|world, mut editor: Mut<MapEditorState>| {
                let mut sidebar = world.resource_mut::<MapEditorSidebarState>(); // 三个资源按正式 API 分别借用。
                request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "second/b.glb")
                    .unwrap(); // 新意图不替换旧 Task。
                request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "latest/c.glb")
                    .unwrap(); // 未启动请求合并为最新一个。
            });
        });
    for _ in 0..4 {
        app.update(); // 模拟连续多帧，没有完成就绪信号。
        let catalog = app.world().resource::<EditorAssetCatalog>(); // 只观察固定任务槽，不触碰 IO。
        assert!(catalog.task.is_some()); // 旧任务仍然真实占槽，不能伪取消。
        assert_eq!(catalog.applied_revision, 4); // 未发生任何错误的新扫描采用。
        assert_eq!(catalog.reveal.as_ref().unwrap().file_path, "latest/c.glb"); // 最新待启动意图没有被旧任务吞掉。
    }
    drop(release); // 唯一旧任务现在可以真正完成。
    advance_until(&mut app, |catalog| catalog.retired_entries.is_some()); // 旧合法结果必须作为迟到结果退役。
    let catalog = app.world().resource::<EditorAssetCatalog>(); // 停在旧任务结束、新任务尚未启动的边界。
    assert!(catalog.task.is_none()); // 只有到此时才允许下一任务取得槽位。
    assert_eq!(catalog.reveal.as_ref().unwrap().file_path, "latest/c.glb"); // 新任务继续保留。
    assert_eq!(catalog.current_folder, "old"); // 迟到结果没有短暂导航到 first 目录。
    assert_eq!(
        app.world()
            .resource::<MapEditorState>()
            .selected_asset
            .as_deref(),
        Some("old/previous.glb")
    ); // 原预览保持稳定。
}

#[test]
fn close_retires_late_results_in_the_same_single_flight_slot_without_reopening_selection() {
    let (mut app, release) = setup(); // 模拟关闭时仍在运行的 IO。
    app.world_mut().resource_mut::<MapEditorState>().open = false; // 用户已关闭 F12。
    app.update(); // Tasks 仍运行取消检查，但不得等待或丢弃旧句柄。
    let catalog = app.world().resource::<EditorAssetCatalog>(); // 取消与任务完成是两个独立状态。
    assert!(catalog.reveal.is_none()); // 关闭立即撤销旧定位资格。
    assert!(catalog.task.is_some()); // IO 仍真实占槽直到被夹具释放。
    drop(release); // 旧 IO 完成后才允许释放数据。
    advance_until(&mut app, |catalog| catalog.retired_entries.is_some()); // 迟到结果只成为后台析构候选。
    advance_until(&mut app, |catalog| {
        catalog.retired_entries.is_none() && catalog.task.is_none()
    }); // 关闭期间也必须清掉唯一退役责任。
    app.world_mut().resource_mut::<MapEditorState>().open = true; // 用户后来重新打开编辑器。
    app.update(); // 已取消代次不应重新启动旧扫描。
    let catalog = app.world().resource::<EditorAssetCatalog>(); // 最终保持旧完整可用目录。
    assert!(catalog.task.is_none()); // 稳定重开不创建多余 IO。
    assert_eq!(catalog.current_folder, "old"); // 不出现关闭前目录回跳。
    assert_eq!(catalog.applied_revision, catalog.requested_revision); // 已取消请求责任正确收口。
    assert_eq!(
        app.world()
            .resource::<MapEditorState>()
            .selected_asset
            .as_deref(),
        Some("old/previous.glb")
    ); // 重开仍显示原文件。
}

#[test]
fn project_source_change_rejects_delayed_same_name_and_keeps_one_io_slot() {
    let (mut app, release) = setup();
    let root = std::env::temp_dir().join(format!("editor-asset-project-{}-{}",
        std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(root.join("first")).unwrap();
    std::fs::write(root.join("first/a.glb"), b"project B").unwrap();
    let root = root.canonicalize().unwrap();
    // 旧任务的完成值也含first/a.glb；归属只能由换根修订判断，不能依靠同名路径。
    app.world_mut().resource_mut::<EditorAssetCatalog>().bind_project_source(2, Some(root.clone()));
    app.world_mut().resource_mut::<MapEditorState>().selected_asset = None;
    app.world_mut().resource_mut::<EditorAssetCatalog>().current_folder = "first".into();
    for _ in 0..4 {
        app.update();
        let catalog = app.world().resource::<EditorAssetCatalog>();
        assert!(catalog.task.is_some());
        assert!(catalog.entries.is_empty(), "换工程后不能显示旧目录条目");
        assert!(catalog.retired_source.is_some());
    }
    drop(release);
    advance_until(&mut app, |catalog| catalog.retired_entries.is_some());
    assert!(app.world().resource::<EditorAssetCatalog>().entries.is_empty());
    advance_until(&mut app, |catalog| catalog.feedback.as_ref().is_some_and(|feedback|
        feedback.result.is_ok() && feedback.revision == catalog.requested_revision));
    let catalog = app.world().resource::<EditorAssetCatalog>();
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(catalog.entries[0].asset_path, "first/a.glb");
    assert!(app.world().resource::<MapEditorState>().selected_asset.is_none());
    // IO句柄已完成，测试只清理自己创建且已核实的临时目录。
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn hosted_without_project_never_falls_back_to_editor_repository_assets() {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default())
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<MapEditorState>()
        .init_resource::<MapEditorSidebarState>()
        .add_systems(Update, (start_asset_scan, poll_asset_scan).chain());
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    app.world_mut().resource_mut::<EditorAssetCatalog>().bind_project_source(1, None);
    for _ in 0..5 { app.update(); }
    let catalog = app.world().resource::<EditorAssetCatalog>();
    assert!(catalog.task.is_none());
    assert!(catalog.entries.is_empty());
    assert!(catalog.feedback.is_none());
}
