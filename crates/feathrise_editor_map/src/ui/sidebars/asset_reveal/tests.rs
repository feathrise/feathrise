//! 一次性定位滚动的ECS回归；模拟布局输出，不创建原生窗口或GPU。

use super::*; // 测试生产滚动系统而非另写一套UI策略。
use crate::types::EditorAssetEntry;
use bevy::math::Affine2; // 直接构造已完成布局的物理位置。
use feathrise_map_content::MapAssetKind; // 使用正式资产分类。 // 真实目录条目。

/// 构造一个已滚到预览区、目标文件位于上方的资产页。
fn fixture(scale: f32) -> (App, Entity) {
    let mut catalog = EditorAssetCatalog::default(); // 目录IO槽保持空闲。
    catalog.entries.push(EditorAssetEntry {
        asset_path: "models/tree.glb".to_owned(), // 与selected_asset完整路径相同。
        display_name: "tree.glb".to_owned(),      // 文件名仅用于显示。
        is_directory: false,                      // 定位目标必须是文件。
        kind: MapAssetKind::Scene,                // 正式模型资产分类。
    });
    catalog.requested_revision = 3; // 后台已经完成本次定位。
    catalog.applied_revision = 3; // 请求与采用身份一致。
    catalog.revealed_entry = Some((3, 0)); // 定位第一条真实文件。
    let mut editor = MapEditorState::default(); // 不涉及地图草稿。
    editor.open = true; // F12可见。
    editor.selected_asset = Some("models/tree.glb".to_owned()); // 已采用的文件选择。
    let mut sidebar = MapEditorSidebarState::default(); // 其他页状态保留默认值。
    sidebar.navigator = EditorNavigatorTab::Assets; // 右侧当前属性不参与目录滚动归属。
    let mut app = App::new(); // 本测试只运行一次性滚动系统。
    app.insert_resource(catalog)
        .insert_resource(editor)
        .insert_resource(sidebar)
        .insert_resource(crate::ui::project_browser::ProjectBrowserState { view: crate::ui::project_browser::ProjectBrowserView::Assets, ..default() })
        .add_systems(Update, sync_asset_reveal_scroll); // 使用真实Query与Local去重。
    let page = app
        .world_mut()
        .spawn((
            crate::ui::project_browser::ProjectBrowserContent(crate::ui::project_browser::ProjectBrowserView::Assets), // 真实资产子视图拥有独立滚动。
            Node::default(), // 显示中的Flex页。
            ComputedNode {
                size: Vec2::new(300.0, 200.0) * scale,
                content_size: Vec2::new(300.0, 900.0) * scale,
                inverse_scale_factor: scale.recip(),
                ..default()
            }, // 模拟同一逻辑布局的不同DPI输出。
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(150.0, 200.0) * scale)), // 视口纵向范围100至300。
            ScrollPosition(Vec2::new(0.0, 250.0)), // 之前滚到了下面的预览区。
        ))
        .id(); // 暴露页面实体便于读取真实采用结果。
    app.world_mut().spawn((
        MapEditorAssetRow {
            slot: 0,
            entry_index: Some(0),
        }, // 当前页第一行。
        Node::default(), // 文件行已绑定且参与布局。
        ComputedNode {
            size: Vec2::new(280.0, 20.0) * scale,
            inverse_scale_factor: scale.recip(),
            ..default()
        }, // 行高20逻辑像素。
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(150.0, -140.0) * scale)), // 目标行顶端-150，需向上滚250。
    ));
    (app, page) // 不构造其他无关UI。
}

#[test] // 全屏UI倍率只影响物理像素，不能让滚动距离翻倍。
fn reveal_scroll_handles_ui_scale_and_runs_only_once() {
    for scale in [1.0, 1.5, 2.0, 2.5] {
        let (mut app, page) = fixture(scale); // 相同逻辑页面，不同物理尺寸。
        app.update(); // 首次消费已采用定位。
        assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 0.0); // 目标必须重新可见。
        app.world_mut().get_mut::<ScrollPosition>(page).unwrap().y = 140.0; // 作者自行滚回预览区。
        app.update(); // 同一revision不能抢回滚动。
        assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 140.0); // 新的手动操作优先。
    }
}

#[test] // 目录采用以后用户又选了别的资产，旧定位不再触发滚动。
fn reveal_scroll_does_not_override_a_later_file_selection() {
    let (mut app, page) = fixture(2.0); // 复用真实资源与Query夹具。
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = Some("other.glb".to_owned()); // 后来的作者选择。
    app.update(); // 旧定位身份不再满足当前选择。
    assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 250.0); // 原滚动位置不变。
}

#[test] // 窄矮窗口也只调整目标不可见的那一段。
fn reveal_scroll_moves_by_the_minimum_visible_delta() {
    let viewport = Rect::new(0.0, 100.0, 300.0, 300.0); // 200像素高的资产页视口。
    assert_eq!(
        visible_delta(viewport, Rect::new(0.0, 120.0, 280.0, 140.0)),
        0.0
    ); // 已在视口不滚动。
    assert_eq!(
        visible_delta(viewport, Rect::new(0.0, 320.0, 280.0, 340.0)),
        40.0
    ); // 最后一行在下方，仅露出差值。
    assert_eq!(
        visible_delta(viewport, Rect::new(0.0, 60.0, 280.0, 80.0)),
        -40.0
    ); // 上方行向上滚动。
}

#[test]
fn continuous_reveal_scrolls_to_a_deep_index_before_that_row_is_in_the_pool() {
    for scale in [1., 1.5, 2.] {
        let (mut app, page) = fixture(scale);
        let count = 96;
        let pitch = feathrise_editor_ui::metrics::ROW_HEIGHT + 2.;
        let height = crate::ui::continuous_list::content_height(count, 1, feathrise_editor_ui::metrics::ROW_HEIGHT, 2.);
        {
            let mut catalog = app.world_mut().resource_mut::<EditorAssetCatalog>();
            catalog.entries = (0..count).map(|index| EditorAssetEntry {
                asset_path: if index == count - 1 { "models/tree.glb".into() } else { format!("models/{index}.glb") },
                display_name: format!("模型{index}.glb"), is_directory: false, kind: MapAssetKind::Scene,
            }).collect();
            catalog.revealed_entry = Some((3, count - 1));
        }
        app.world_mut().get_mut::<ComputedNode>(page).unwrap().content_size.y = height * scale;
        app.world_mut().spawn((MapEditorAssetList, ComputedNode {
            size: Vec2::new(280., height) * scale, inverse_scale_factor: scale.recip(), ..default()
        }, UiGlobalTransform::from(Affine2::from_translation(Vec2::new(150., -150. + height * 0.5) * scale))));
        app.update();
        let expected = (count - 1) as f32 * pitch + feathrise_editor_ui::metrics::ROW_HEIGHT - 200.;
        assert!((app.world().get::<ScrollPosition>(page).unwrap().y - expected).abs() < 0.01,
            "完整内容高度决定远处目标位置，不能等目标先进入十二槽才滚动");
    }
}
