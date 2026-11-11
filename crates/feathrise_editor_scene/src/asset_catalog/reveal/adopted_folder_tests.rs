//! 连续目录采用的首项锚点与刷新位置回归；用正式完成值采用，不触碰真实工程或滚动组件。

use super::*;
use crate::asset_catalog::scan::finish_asset_scan;
use crate::types::EditorAssetEntry;
use feathrise_scene_content::SceneAssetKind;

fn entries(folder: &str) -> Vec<EditorAssetEntry> {
    (0..96).map(|index| EditorAssetEntry { asset_path: format!("{folder}/{index}.bin"),
        display_name: format!("{index}.bin"), is_directory: false, kind: SceneAssetKind::Data }).collect()
}

fn context() -> (EditorAssetCatalog, SceneEditorState, SceneEditorSidebarState) {
    let catalog = EditorAssetCatalog { current_folder: "a".into(), adopted_folder: Some("a".into()),
        entries: entries("a"), requested_revision: 7, applied_revision: 7,
        visible_start: 64, visible_count: 20, ..Default::default() };
    let editor = SceneEditorState { open: true, selected_asset: Some("a/64.bin".into()), ..Default::default() };
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.navigator = EditorNavigatorTab::Assets;
    (catalog, editor, sidebar)
}

#[test]
fn ordinary_equal_height_folder_change_anchors_first_item_even_when_navigation_path_is_already_changed() {
    let (mut catalog, mut editor, sidebar) = context();
    // 正式目录行与“上级”会先公布导航路径，再等后台完成；此前不能据current_folder推断目录没变。
    catalog.current_folder = "b".into();
    catalog.requested_revision = 8;
    let completed = finish_asset_scan(AssetScanRequest { revision: 8, folder: "b".into(), ..Default::default() }, Ok(entries("b")));
    assert!(!adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed));
    assert_eq!(catalog.adopted_folder.as_deref(), Some("b"));
    let anchor = catalog.scroll_anchor.expect("等高的新目录也必须请求首项滚动");
    assert_eq!(anchor.index, 0);
    assert_eq!(anchor.revision, 8);
    assert!(!anchor.require_selection);
    assert_eq!(catalog.entries.len(), 96, "新旧高度完全相同，不能依赖滚动范围缩短来碰巧归位");
    assert_eq!(editor.selected_asset.as_deref(), Some("a/64.bin"));
}

#[test]
fn refreshing_the_same_adopted_folder_does_not_publish_a_new_scroll_anchor() {
    let (mut catalog, mut editor, sidebar) = context();
    catalog.requested_revision = 8;
    let completed = finish_asset_scan(AssetScanRequest { revision: 8, folder: "a".into(), ..Default::default() }, Ok(entries("a")));
    assert!(!adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed));
    assert_eq!(catalog.adopted_folder.as_deref(), Some("a"));
    assert!(catalog.scroll_anchor.is_none(), "同目录刷新不向原唯一滚动容器发自动归零意图");
    assert_eq!(editor.selected_asset.as_deref(), Some("a/64.bin"));
}

#[test]
fn failed_folder_request_keeps_last_adoption_and_source_rebinding_clears_it() {
    let (mut catalog, mut editor, sidebar) = context();
    catalog.request_scroll_anchor(64);
    let ticket = catalog.scroll_anchor.unwrap().ticket;
    catalog.current_folder = "b".into();
    catalog.requested_revision = 8;
    let completed = finish_asset_scan(AssetScanRequest { revision: 8, folder: "b".into(), ..Default::default() }, Err("目录暂不可读".into()));
    assert!(!adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed));
    assert_eq!(catalog.adopted_folder.as_deref(), Some("a"));
    assert_eq!(catalog.current_folder, "a");
    assert_eq!(catalog.entries[0].asset_path, "a/0.bin");
    assert_eq!(catalog.scroll_anchor.unwrap().ticket, ticket);
    catalog.bind_project_source(2, None);
    assert!(catalog.adopted_folder.is_none());
    assert!(catalog.scroll_anchor.is_none());
    assert!(catalog.entries.is_empty());
}
