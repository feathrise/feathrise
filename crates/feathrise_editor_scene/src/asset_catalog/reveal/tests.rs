//! 反向定位的纯数据和最小 ECS 回归：跨页、子资产、失败托底与用户意图优先。

use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*; // 最小 ECS 只验证真实资产行点击，不启动窗口或渲染。
use feathrise_scene_content::{SceneAssetKind, SceneId}; // 测试使用正式文件类型与场景身份。

use super::*; // 直接检验请求、取消与完整完成值采用边界。
use crate::asset_catalog::scan::finish_asset_scan; // 用纯条目模拟后台排序之后的完整输入。
use crate::asset_catalog::systems::handle_asset_row_clicks; // 文件选择必须实际取消未完成定位。
use crate::components::SceneEditorAssetRow; // 固定行池与真实点击系统使用相同 marker。
use crate::resources::SceneEditorNumericScrubMouseGuard; // 与正式输入共享数值拖动互斥门。
use crate::types::EditorTool; // 定位不应切换场景画笔或放置工具。
use crate::ui::sidebars::EditorRightSidebarTab;

/// 失败、迟到与完整采用均保持原目录事务，只发布自己的真实反馈。
#[test]
fn favorite_folder_is_adopted_atomically_without_replacing_selected_file() {
    let (mut catalog, mut editor, mut sidebar) = context();
    request_asset_folder(&mut catalog, &mut editor, &mut sidebar, "new/常用目录").unwrap();
    assert_eq!(catalog.current_folder, "old");
    let request = AssetScanRequest { revision: catalog.requested_revision, folder: "new/常用目录".into(), reveal_file: None };
    assert!(!adopt_asset_scan(&mut catalog, &mut editor, &sidebar,
        finish_asset_scan(request, Ok(vec![file("new/常用目录/a.glb")]))));
    assert_eq!(catalog.current_folder, "new/常用目录");
    assert_eq!(catalog.page, 0);
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb"));
    assert!(catalog.reveal.is_none());
    assert!(!editor.dirty && !editor.save_requested);
}

#[test]
fn empty_directory_reference_opens_asset_root_but_empty_file_reference_stays_invalid() {
    let (mut catalog, mut editor, mut sidebar) = context();
    request_asset_folder(&mut catalog, &mut editor, &mut sidebar, "").unwrap();
    let revision = catalog.requested_revision;
    assert!(request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "").is_err());
    assert_eq!(catalog.requested_revision, revision, "坏文件请求不能覆盖有效目录意图");
    let completed = finish_asset_scan(AssetScanRequest { revision, folder: String::new(), reveal_file: None },
        Ok(vec![file("root.glb")]));
    assert!(!adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed));
    assert!(catalog.current_folder.is_empty());
    assert_eq!(catalog.revealed_revision(), Some(revision), "目录成功也必须保留实际采用意图");
    assert!(catalog.revealed_entry.is_none(), "目录根不得伪造文件选择");
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb"));
}

#[test]
fn favorite_folder_failure_and_close_preserve_original_browsing_identity() {
    for close in [false, true] {
        let (mut catalog, mut editor, mut sidebar) = context();
        request_asset_folder(&mut catalog, &mut editor, &mut sidebar, "missing").unwrap();
        let request = AssetScanRequest { revision: catalog.requested_revision, folder: "missing".into(), reveal_file: None };
        editor.open = !close;
        assert!(!adopt_asset_scan(&mut catalog, &mut editor, &sidebar, finish_asset_scan(request, Err("目录不存在".into()))));
        assert_eq!(catalog.current_folder, "old");
        assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb"));
        assert!(!editor.dirty && !editor.save_requested);
    }
}

#[test]
fn ordinary_folder_completion_is_recorded_only_in_the_visible_live_session() {
    for mode in 0..5 {
        let (mut catalog, mut editor, mut sidebar) = context();
        sidebar.navigator = EditorNavigatorTab::Assets;
        match mode {
            1 => editor.open = false,
            2 => editor.close_requested = true,
            3 => editor.pending_scene_transition = Some(SceneId("next-map".into())),
            4 => sidebar.navigator = EditorNavigatorTab::SceneTree,
            _ => {}
        }
        let mut app = App::new();
        app.add_plugins(bevy::app::TaskPoolPlugin::default());
        let request = AssetScanRequest { revision: catalog.requested_revision, folder: catalog.current_folder.clone(), reveal_file: None };
        catalog.task = Some(bevy::tasks::IoTaskPool::get().spawn(async move {
            Some(finish_asset_scan(request, Ok(vec![file("old/current.glb")])))
        }));
        app.insert_resource(catalog).insert_resource(editor).insert_resource(sidebar)
            .add_message::<crate::ui::asset_shortcuts::AssetVisited>()
            .add_systems(Update, crate::asset_catalog::poll_asset_scan);
        for _ in 0..1000 {
            app.update();
            if app.world().resource::<EditorAssetCatalog>().feedback.is_some() { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.world().resource::<EditorAssetCatalog>().feedback.is_some());
        let visits: Vec<_> = app.world_mut().resource_mut::<Messages<crate::ui::asset_shortcuts::AssetVisited>>().drain().collect();
        assert_eq!(visits.len(), usize::from(mode == 0));
        if let Some(visit) = visits.first() { assert!(visit.is_directory); assert_eq!(visit.path, "old"); }
    }
}

#[test]
fn directory_feedback_accepts_only_current_failure_and_complete_success() {
    let (mut catalog, mut editor, sidebar) = context();
    let request = |revision| AssetScanRequest {
        revision,
        folder: "old".into(),
        reveal_file: None,
    };
    let failed = finish_asset_scan(request(7), Err("读取目录失败：真实完整原因".into()));
    assert!(!adopt_asset_scan(
        &mut catalog,
        &mut editor,
        &sidebar,
        failed
    ));
    let feedback = catalog.feedback.clone().unwrap();
    assert_eq!(feedback.revision, 7);
    assert_eq!(feedback.result, Err("读取目录失败：真实完整原因".into()));
    assert_eq!(catalog.entries[0].asset_path, "old/previous.glb");
    catalog.requested_revision = 9;
    let late = finish_asset_scan(request(8), Err("迟到的错误不能覆盖当前反馈".into()));
    assert!(!adopt_asset_scan(&mut catalog, &mut editor, &sidebar, late));
    assert_eq!(catalog.feedback.as_ref(), Some(&feedback));
    assert_eq!(editor.status, "读取目录失败：真实完整原因");
    let complete = finish_asset_scan(request(9), Ok(vec![file("old/new.glb")]));
    assert!(!adopt_asset_scan(
        &mut catalog,
        &mut editor,
        &sidebar,
        complete
    ));
    assert_eq!(catalog.feedback.as_ref().unwrap().revision, 9);
    assert_eq!(catalog.feedback.as_ref().unwrap().result, Ok(()));
    assert_eq!(catalog.entries[0].asset_path, "old/new.glb");
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb"));
}

#[test]
fn invalid_reveal_index_reports_failure_without_publishing_private_directory() {
    let (mut catalog, mut editor, mut sidebar) = context();
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "new/target.glb").unwrap();
    let completed = AssetScanResult {
        request: frozen_scan(&catalog),
        result: Ok(crate::asset_catalog::types::AssetScanContents {
            entries: vec![file("new/target.glb")],
            reveal_index: Some(99),
        }),
    };
    assert!(!adopt_asset_scan(
        &mut catalog,
        &mut editor,
        &sidebar,
        completed
    ));
    assert!(
        catalog
            .feedback
            .as_ref()
            .unwrap()
            .result
            .as_ref()
            .unwrap_err()
            .contains("有效文件位置")
    );
    assert_eq!(catalog.current_folder, "old");
    assert_eq!(catalog.entries[0].asset_path, "old/previous.glb");
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb"));
}

/// 构造真实完整路径的目录项，显示名不参与定位身份。
fn file(path: &str) -> EditorAssetEntry {
    EditorAssetEntry {
        asset_path: path.to_owned(), // 跨目录测试不能仅比较最后文件名。
        display_name: path.rsplit('/').next().unwrap().to_owned(), // UI 可读文件名与实际目录相符。
        is_directory: false,         // 文件目标才允许成为预览选择。
        kind: SceneAssetKind::Scene,   // glTF 模型会沿已有预览链加载。
    }
}

/// 保留一份已有可用目录与预览，便于确认异步定位不是破坏性的导航。
fn context() -> (EditorAssetCatalog, SceneEditorState, SceneEditorSidebarState) {
    let catalog = EditorAssetCatalog {
        current_folder: "old".to_owned(), // 定位期间公开路径仍属于原目录。
        entries: vec![file("old/previous.glb")], // 完成前固定行池继续显示旧内容。
        requested_revision: 7,            // 基线目录已经成功应用。
        applied_revision: 7,              // 没有隐含普通扫描。
        ..default()                       // 页码零、唯一任务空闲。
    };
    let editor = SceneEditorState {
        open: true,                                               // 用户正在 F12 编辑会话中操作。
        selected_scene_id: Some(SceneId::parse("MAP-test").unwrap()), // 使用明确场景身份检测迟到跨场景。
        selected_asset: Some("old/previous.glb".to_owned()),      // 当前有完整可用预览。
        tool: EditorTool::Vegetation,                             // 定位不能擅自切换工具。
        ..default() // 作者 dirty、保存与撤销请求均为 false。
    };
    (catalog, editor, SceneEditorSidebarState::default()) // 初始右侧为场景页，可检验一次性切页。
}

/// 复制的只有轻量请求，完整目录条目不在主线程深拷贝。
fn frozen_scan(catalog: &EditorAssetCatalog) -> AssetScanRequest {
    let reveal = catalog.reveal.as_ref().expect("夹具应已有定位请求"); // 对照真正的扫描启动快照。
    AssetScanRequest {
        revision: reveal.revision,                   // 保留任务实际启动票据。
        folder: reveal.folder.clone(),               // 保留直接父目录。
        reveal_file: Some(reveal.file_path.clone()), // 精确匹配所属文件而不携带 # 标签。
    }
}

#[test]
fn cross_folder_and_cross_page_reveal_atomically_selects_the_owner_file() {
    let (mut catalog, mut editor, mut sidebar) = context(); // 从已有场景页和预览开始。
    request_asset_reveal(
        &mut catalog,                         // 不预先清空旧目录。
        &mut editor,                          // 不预先清空旧预览。
        &mut sidebar,                         // 立即让左侧资产页可见。
        "new/tree-0026.glb#Mesh2/Primitive3", // 任意子资产只定位其真实所属文件。
    )
    .unwrap();
    assert_eq!(sidebar.navigator, EditorNavigatorTab::Assets); // 相关浏览器已经显示。
    assert_eq!(sidebar.right, EditorRightSidebarTab::Scene); // 采用前不抢当前右侧属性。
    assert_eq!(catalog.current_folder, "old"); // 后台尚未验证目标时不跳目录。
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb")); // 旧预览继续保留。
    let entries = (0..40)
        .map(|index| file(&format!("new/tree-{index:04}.glb")))
        .collect(); // 模拟最终排序超过两页。
    let completed = finish_asset_scan(frozen_scan(&catalog), Ok(entries)); // 后台搜索索引与完整目录共同返回。
    assert!(adopt_asset_scan(
        &mut catalog,
        &mut editor,
        &sidebar,
        completed
    )); // 一次采用后各个UI读到同一状态，并明确请求右侧预览。
    assert_eq!(catalog.current_folder, "new"); // 只有成功后才打开正确父目录。
    assert_eq!(catalog.page, 0); // 兼容字段不再影响连续滚动窗口。
    assert_eq!(catalog.scroll_anchor.unwrap().index, 26); // 深处目标按完整目录索引滚入可见区。
    assert_eq!(catalog.entries.len(), 40); // 完整目录不因为 UI 行池而截断。
    assert_eq!(editor.selected_asset.as_deref(), Some("new/tree-0026.glb")); // 现有预览只接收可加载的所属文件。
    assert!(editor.status.contains("#Mesh2/Primitive3")); // 作者仍能核对请求中的子网格身份。
    assert!(catalog.reveal.is_none()); // 已完成意图不会逐帧重复定位。
    assert!(catalog.retired_entries.is_some()); // 旧目录只移入后台释放责任，采用帧不整表析构。
    assert!(
        !editor.dirty && !editor.save_requested && !editor.undo_requested && !editor.redo_requested
    ); // 浏览绝不成为作者命令。
    assert_eq!(editor.tool, EditorTool::Vegetation); // 场景工具保持原值。
    assert_eq!(editor.selected_scene_id.unwrap().as_str(), "MAP-test"); // 定位不切换作者场景。
}

#[test]
fn root_file_and_material_label_work_without_changing_existing_dirty_state() {
    let (mut catalog, mut editor, mut sidebar) = context(); // 场景已有未保存内容时也能只读定位。
    editor.dirty = true; // 定位不得清除未保存标记。
    request_asset_reveal(
        &mut catalog,
        &mut editor,
        &mut sidebar,
        "tree.glb#Material0/std",
    )
    .unwrap(); // 根目录文件没有父路径前缀。
    let completed = finish_asset_scan(frozen_scan(&catalog), Ok(vec![file("tree.glb")])); // 成功结果给出唯一根目录目标。
    adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed); // 不涉及任何预设或场景事务。
    assert!(catalog.current_folder.is_empty()); // 根目录必须表示为空字符串，不能是 '.'。
    assert_eq!(editor.selected_asset.as_deref(), Some("tree.glb")); // 预览使用根文件。
    assert!(editor.status.contains("#Material0/std")); // 材质子资产信息完整保留。
    assert!(editor.dirty); // 未保存作者状态原样保留。
}

#[test]
fn missing_file_or_directory_preserves_the_previous_directory_page_and_selection() {
    for input in [
        Ok(vec![file("new/not-the-requested-file.glb")]), // 目录存在但没有目标文件。
        Err("无法打开资产目录 new：目录不存在".to_owned()), // 目录 IO 明确失败。
        Ok(vec![file("other/target.glb")]),               // 即使文件名相同也不能选择其他完整路径。
    ] {
        let (mut catalog, mut editor, mut sidebar) = context(); // 每个失败使用独立基线。
        request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "new/target.glb").unwrap(); // 未验证前不改原目录。
        let completed = finish_asset_scan(frozen_scan(&catalog), input); // 错误在后台完成阶段产生。
        assert!(!adopt_asset_scan(
            &mut catalog,
            &mut editor,
            &sidebar,
            completed
        )); // 失败只能更新具体诊断，不取得预览焦点。
        assert_eq!(catalog.current_folder, "old"); // 不跳进不存在的目录。
        assert_eq!(catalog.entries[0].asset_path, "old/previous.glb"); // 旧列表仍完整可用。
        assert_eq!(catalog.page, 0); // 失败不改变浏览位置。
        assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb")); // 旧预览不变。
        assert!(editor.status.contains("定位失败")); // 作者能直接看到失败而不是无声没反应。
        assert_eq!(catalog.applied_revision, catalog.requested_revision); // 失败不会启动每帧自动重试。
        assert!(catalog.reveal.is_none()); // 失败责任结束，等待显式再定位。
    }
}

#[test]
fn malformed_result_is_rejected_before_any_directory_or_selection_is_published() {
    let (mut catalog, mut editor, mut sidebar) = context(); // 保留旧完整结果作为失败托底。
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "new/target.glb").unwrap(); // 真实目标明确。
    let mut completed = finish_asset_scan(frozen_scan(&catalog), Ok(vec![file("new/target.glb")])); // 构造合法后台结果。
    completed.result.as_mut().unwrap().reveal_index = Some(8); // 模拟损坏完成值不能用于 get_unchecked 或错误索引。
    adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed); // 原子采用前再次复核索引身份。
    assert_eq!(catalog.current_folder, "old"); // 非法索引不发布半个目录。
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb")); // 非法索引不破坏预览。
    assert!(editor.status.contains("有效文件位置")); // 失败原因可诊断。
}

#[test]
fn a_late_result_cannot_override_a_newer_reveal_request() {
    let (mut catalog, mut editor, mut sidebar) = context(); // 只保留一份正在显示的目录。
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "first/a.glb").unwrap(); // 第一个请求可以已在后台运行。
    let old = finish_asset_scan(frozen_scan(&catalog), Ok(vec![file("first/a.glb")])); // 冻结第一任务完成值。
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "second/b.glb").unwrap(); // 新意图替换旧待采用目标。
    let latest_revision = catalog.requested_revision; // 第二请求必须保持其完整身份。
    let latest_status = editor.status.clone(); // 迟到结果不得覆盖新请求提示。
    assert!(!adopt_asset_scan(&mut catalog, &mut editor, &sidebar, old)); // 第一结果已过期，不能切换预览来源。
    assert_eq!(catalog.current_folder, "old"); // 不出现第一文件短暂跳回。
    assert_eq!(catalog.reveal.as_ref().unwrap().revision, latest_revision); // 第二请求未被消费或误取消。
    assert_eq!(editor.status, latest_status); // 不显示第一请求成功或失败。
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb")); // 新结果之前保持旧预览。
}

#[test]
fn user_navigation_refresh_selection_and_scene_changes_cancel_late_reveals() {
    for change in 0..6 {
        let (mut catalog, mut editor, mut sidebar) = context(); // 六种后续用户意图各自独立。
        request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "new/target.glb").unwrap(); // 定位仍在等待。
        let completed = finish_asset_scan(frozen_scan(&catalog), Ok(vec![file("new/target.glb")])); // 提前冻结旧任务完成值。
        match change {
            0 => catalog.requested_revision += 1, // 与现有“刷新”按钮相同，只提升目录修订。
            1 => catalog.current_folder = "chosen-folder".to_owned(), // 防御只改目录未提升修订的外部导航。
            2 => catalog.scroll_intent_revision += 1, // 实际滚动意图优先于等待中的旧定位。
            3 => editor.selected_asset = Some("old/user-selected.glb".to_owned()), // 另一文件选择不被覆盖。
            4 => sidebar.navigator = EditorNavigatorTab::SceneTree, // 离开左侧浏览器后，旧定位不能抢回。
            _ => editor.selected_scene_id = Some(SceneId::parse("MAP-other").unwrap()), // 另一个场景会话不接收旧定位。
        }
        let folder = catalog.current_folder.clone(); // 冻结用户最终明确选择。
        let selection = editor.selected_asset.clone(); // 迟到任务不得覆盖这份选择。
        let viewport = catalog.viewport_revision; // 用户滚动后的窗口身份保持不变。
        adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed); // 采用前须先检查请求是否过期。
        assert!(catalog.reveal.is_none()); // 旧请求失去采用资格。
        assert_eq!(catalog.current_folder, folder); // 不抢回父目录。
        assert_eq!(catalog.viewport_revision, viewport); // 不抢回目标窗口。
        assert_eq!(editor.selected_asset, selection); // 不重置当前预览选择。
        if change == 0 {
            assert_ne!(catalog.applied_revision, catalog.requested_revision); // 主动刷新仍需下一次普通扫描，取消不能吞掉它。
        }
    }
}

#[test]
fn pending_reveal_survives_layout_and_pool_changes_without_a_new_scroll_intent() {
    let (mut catalog, mut editor, mut sidebar) = context();
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "new/target.glb").unwrap();
    let completed = finish_asset_scan(frozen_scan(&catalog), Ok(vec![file("new/target.glb")]));
    // 读取提示换行或窗口变高只改变呈现窗口，不能冒充作者主动离开旧定位的意图。
    catalog.viewport_revision += 1;
    catalog.visible_start = 1;
    catalog.visible_count = 36;
    assert!(adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed));
    assert_eq!(catalog.current_folder, "new");
    assert_eq!(editor.selected_asset.as_deref(), Some("new/target.glb"));
    assert_eq!(catalog.scroll_anchor.unwrap().index, 0);
}

#[test]
fn changing_right_tools_does_not_cancel_the_left_browser_reveal() {
    let (mut catalog, mut editor, mut sidebar) = context();
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "new/target.glb").unwrap();
    sidebar.right = EditorRightSidebarTab::Tools;
    let completed = finish_asset_scan(frozen_scan(&catalog), Ok(vec![file("new/target.glb")]));
    assert!(adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed));
    assert_eq!(editor.selected_asset.as_deref(), Some("new/target.glb"));
    assert_eq!(sidebar.navigator, EditorNavigatorTab::Assets);
    assert_eq!(sidebar.right, EditorRightSidebarTab::Tools); // 纯采用不抢页，正式轮询在成功后揭示文件属性。
}

#[test]
fn file_click_reveals_asset_properties_and_folder_navigation_keeps_tools() {
    for directory in [false, true] {
        let (mut catalog, editor, mut sidebar) = context();
        catalog.entries[0].is_directory = directory;
        sidebar.navigator = EditorNavigatorTab::Assets;
        sidebar.right = EditorRightSidebarTab::Tools;
        let mut app = App::new();
        app.insert_resource(catalog).insert_resource(editor).insert_resource(sidebar)
            .init_resource::<SceneEditorNumericScrubMouseGuard>()
            .add_systems(Update, handle_asset_row_clicks);
        app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, SceneEditorAssetRow { slot: 0, entry_index: Some(0) }));
        app.update();
        let sidebar = app.world().resource::<SceneEditorSidebarState>();
        assert_eq!(sidebar.navigator, EditorNavigatorTab::Assets);
        assert_eq!(sidebar.right, if directory { EditorRightSidebarTab::Tools } else { EditorRightSidebarTab::Assets });
        let editor = app.world().resource::<SceneEditorState>();
        assert_eq!(editor.tool, EditorTool::Vegetation);
        assert!(!editor.dirty && !editor.save_requested);
    }
}

#[test]
fn hidden_left_browser_discards_stale_file_press_instead_of_replaying_it() {
    for hidden_pane in [false, true] {
        let (catalog, mut editor, mut sidebar) = context();
        editor.selected_asset = None;
        sidebar.navigator = if hidden_pane { EditorNavigatorTab::Assets } else { EditorNavigatorTab::Scenes };
        sidebar.right = EditorRightSidebarTab::Assets; // 右侧文件属性可见也不能让隐藏列表接收按下。
        let mut layout = crate::resources::SceneEditorLayoutState::default();
        layout.content_visible = !hidden_pane;
        let mut app = App::new();
        app.insert_resource(catalog).insert_resource(editor).insert_resource(sidebar).insert_resource(layout)
            .init_resource::<SceneEditorNumericScrubMouseGuard>().add_systems(Update, handle_asset_row_clicks);
        let row = app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, SceneEditorAssetRow { slot: 0, entry_index: Some(0) })).id();
        app.update();
        assert!(app.world().resource::<SceneEditorState>().selected_asset.is_none());
        app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = EditorNavigatorTab::Assets;
        app.world_mut().resource_mut::<crate::resources::SceneEditorLayoutState>().content_visible = true;
        app.update();
        assert!(app.world().resource::<SceneEditorState>().selected_asset.is_none());
        *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
        app.update();
        *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::Pressed; app.world_mut().entity_mut(row).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        app.update();
        assert_eq!(app.world().resource::<SceneEditorState>().selected_asset.as_deref(), Some("old/previous.glb"));
    }
}

#[test]
fn closing_then_reopening_f12_does_not_revive_an_old_reveal() {
    let (mut catalog, mut editor, mut sidebar) = context(); // 关闭前已有正在等待的定位。
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "new/target.glb").unwrap(); // 后台工作可以继续但不能在重开后跳回。
    let completed = finish_asset_scan(frozen_scan(&catalog), Ok(vec![file("new/target.glb")])); // 固定关闭之前的任务身份。
    editor.open = false; // 真正关闭会话。
    expire_asset_reveal(&mut catalog, &editor, &sidebar); // Tasks 阶段即使任务未完成也要处理关闭。
    editor.open = true; // 用户立即重新打开 F12。
    adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed); // 关闭前任务此时才完成。
    assert_eq!(catalog.current_folder, "old"); // 旧定位不能复活。
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb")); // 重开后的预览仍是用户原选择。
    assert!(catalog.reveal.is_none()); // 请求必须已不可恢复。
}

#[test]
fn real_asset_row_click_cancels_pending_reveal_even_when_reselecting_the_same_file() {
    let (mut catalog, mut editor, mut sidebar) = context(); // 当前旧文件已经选中。
    request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, "new/target.glb").unwrap(); // 点击新引用只排入定位意图。
    let completed = finish_asset_scan(frozen_scan(&catalog), Ok(vec![file("new/target.glb")])); // 模拟稍后返回的旧结果。
    let mut app = App::new(); // 不依赖渲染或文件 IO 的真实 ECS 输入回归。
    app.insert_resource(catalog) // 使用正式目录资源。
        .insert_resource(editor) // 使用真实编辑会话。
        .init_resource::<SceneEditorNumericScrubMouseGuard>() // 鼠标当前由普通资产列表拥有。
        .add_systems(Update, handle_asset_row_clicks); // 执行正式点击逻辑。
    app.world_mut().spawn((
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, // 用户明确重新单击旧文件。
        SceneEditorAssetRow {
            slot: 0,              // 原列表首行。
            entry_index: Some(0), // 指向已选中的原资产。
        },
    ));
    app.update(); // 即使 selected_asset 值相同，这一显式意图也取消反向定位。
    let mut catalog = app
        .world_mut()
        .remove_resource::<EditorAssetCatalog>()
        .unwrap(); // 取出真实输入后的目录状态。
    let mut editor = app.world_mut().remove_resource::<SceneEditorState>().unwrap(); // 不额外模拟取消逻辑。
    assert!(catalog.reveal.is_none()); // 确认实际点击系统已取消旧请求。
    adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed); // 迟到结果现在必须被拒绝。
    assert_eq!(editor.selected_asset.as_deref(), Some("old/previous.glb")); // 用户重新确认的原资产不被换走。
    assert_eq!(catalog.current_folder, "old"); // 原文件所在目录也不变。
}
