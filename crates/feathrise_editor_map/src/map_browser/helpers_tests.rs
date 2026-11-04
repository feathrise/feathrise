//! 虚拟地图文件夹格式、树排序、展开投影与拖拽阈值的纯单元测试。

use std::collections::{BTreeMap, BTreeSet}; // 构造确定性归属和会话展开集合。

use bevy::math::Vec2; // 拖拽阈值测试使用物理像素向量。
use feathrise_map_content::MapId; // 测试地图必须经过正式稳定身份解析。

use super::helpers::*; // 直接覆盖同功能域全部纯规则。
use crate::constants::MAP_FOLDER_MAX_DEPTH; // 构造恰好越界的十七层父链。
use crate::types::{MapBrowserEntry, MapBrowserItem, MapFolderDocument, MapFolderRecord}; // 构造真实树输入形状。

/// 构造最小文件夹记录，便于层级和查重用例表达意图。
fn folder(id: &str, name: &str, parent_id: Option<&str>) -> MapFolderRecord {
    MapFolderRecord {
        id: id.to_owned(),                       // 测试决定稳定ID。
        display_name: name.to_owned(),           // 测试决定作者名称。
        parent_id: parent_id.map(str::to_owned), // 可选父级保持真实Serde形状。
    }
}

/// 构造主线程已经冻结、但尚待后台写入深度的地图树记录。
fn map_entry(id: &str, name: &str, parent_id: Option<&str>) -> MapBrowserEntry {
    MapBrowserEntry {
        item: MapBrowserItem::Map(MapId::parse(id).expect("测试MapId合法")), // 保持正式稳定身份。
        display_name: name.to_owned(), // 后台只能读取冻结名称。
        published: true,               // 测试地图都视为已发布，不影响树排序。
        depth: 0,                      // `build_browser_tree`必须根据父级覆盖占位深度。
        parent_folder_id: parent_id.map(str::to_owned), // 指定根目录或某个稳定父级。
    }
}

#[test]
fn valid_nested_document_and_root_assignment_round_trip() {
    let mut document = MapFolderDocument::default(); // 从缺失文件合法空分类开始。
    document.folders.push(folder("FOLDER-1", "河南区域", None)); // 建立第一层分类。
    document
        .folders
        .push(folder("FOLDER-2", "开封周边", Some("FOLDER-1"))); // 建立第二层分类。
    let map_id = MapId::parse("HN-01").expect("测试MapId合法"); // 使用正式身份解析器。
    assign_map_to_folder(&mut document, &map_id, Some("FOLDER-2")).expect("合法归类应成功"); // 地图进入第二层文件夹。
    let bytes = encode_folder_document(&document).expect("合法文档应可编码"); // 写入格式含终止换行。
    let decoded = decode_folder_document(&bytes).expect("编码结果应可重新加载"); // 加载复用同一校验门。
    assert_eq!(decoded, document); // 往返不得改变稳定ID、名称或归属。
    assert_eq!(
        selected_container_folder_id(&decoded, Some(&MapBrowserItem::Map(map_id.clone()))),
        Some("FOLDER-2".to_owned()),
    ); // 选择地图时新建上下文沿用其直接父级。
    assign_map_to_folder(&mut document, &map_id, None).expect("移回根目录应成功"); // 根目录通过移除键表达。
    assert!(!document.map_folders.contains_key("HN-01")); // 不保存冗余根目录哨兵。
}

#[test]
fn duplicate_names_cycles_missing_parents_and_excess_depth_are_rejected() {
    let duplicate = MapFolderDocument {
        folders: vec![
            folder("FOLDER-1", "区域", None),  // 第一条根目录名称。
            folder("FOLDER-2", "区 域", None), // 空格不同仍是不同名称，先保留合法对照。
            folder("FOLDER-3", "区域", None),  // 完全同名必须拒绝。
        ],
        ..MapFolderDocument::default()
    };
    assert!(validate_folder_document(&duplicate).is_err()); // 同级重复不可采用。

    let cycle = MapFolderDocument {
        folders: vec![
            folder("FOLDER-1", "一", Some("FOLDER-2")), // 一指向二。
            folder("FOLDER-2", "二", Some("FOLDER-1")), // 二反向指向一形成循环。
        ],
        ..MapFolderDocument::default()
    };
    assert!(validate_folder_document(&cycle).is_err()); // 任意父链循环都拒绝。

    let missing = MapFolderDocument {
        folders: vec![folder("FOLDER-1", "孤儿", Some("FOLDER-missing"))], // 引用不存在父级。
        ..MapFolderDocument::default()
    };
    assert!(validate_folder_document(&missing).is_err()); // 不猜测回根目录。

    let mut deep = MapFolderDocument::default(); // 构造十七层父链。
    for index in 0..=MAP_FOLDER_MAX_DEPTH {
        deep.folders.push(folder(
            &format!("FOLDER-{index}"), // 每层使用唯一稳定ID。
            &format!("层{index}"),      // 每层名称保持同父级唯一。
            index
                .checked_sub(1)
                .map(|parent| format!("FOLDER-{parent}"))
                .as_deref(), // 第二层起引用前一层。
        ));
    }
    assert!(validate_folder_document(&deep).is_err()); // 第十七层越过硬上限。
}

#[test]
fn folder_names_and_assignment_targets_are_strictly_validated() {
    assert!(normalize_folder_name("  河南区域  ").is_ok()); // UI输入允许首尾空格并统一修剪。
    assert!(normalize_folder_name(".").is_err()); // 文件系统保留名称拒绝。
    assert!(normalize_folder_name("甲/乙").is_err()); // 虚拟名称不能冒充路径。
    let document = MapFolderDocument {
        map_folders: BTreeMap::from([("HN-01".to_owned(), "FOLDER-missing".to_owned())]), // 构造失效归属。
        ..MapFolderDocument::default()
    };
    assert!(validate_folder_document(&document).is_err()); // 失效目标不回退根目录。
}

#[test]
fn tree_is_depth_first_folder_first_and_keeps_catalog_map_order() {
    let entries = build_browser_tree(
        vec![
            folder("FOLDER-B", "B分类", None), // 根同级输入故意逆序。
            folder("FOLDER-A-CHILD", "子分类", Some("FOLDER-A")), // A目录的直接子目录。
            folder("FOLDER-A", "A分类", None), // 名称排序后必须成为第一棵根子树。
        ],
        vec![
            map_entry("HN-02", "A内地图", Some("FOLDER-A")), // A子目录之后显示。
            map_entry("HN-01", "根地图一", None),            // 根地图保持catalog第一顺位。
            map_entry("MAP-1", "根地图二", None),            // 名称不参与地图排序。
        ],
    );
    let labels = entries
        .iter()
        .map(|entry| entry.display_name.as_str())
        .collect::<Vec<_>>(); // 断言只关心最终可读顺序。
    assert_eq!(
        labels,
        vec![
            "A分类",
            "子分类",
            "A内地图",
            "B分类",
            "根地图一",
            "根地图二"
        ],
    ); // 每个父级先目录子树后地图，根地图仍保持catalog顺序。
    assert_eq!(entries[0].depth, 1); // 根目录直属文件夹缩进一级。
    assert_eq!(entries[1].depth, 2); // 嵌套子目录缩进两级。
    assert_eq!(entries[2].depth, 2); // A内地图与其同父级子目录深度一致。
}

#[test]
fn projection_only_reveals_descendants_of_expanded_folders() {
    let full = build_browser_tree(
        vec![
            folder("FOLDER-A", "A分类", None),             // 根目录直属文件夹。
            folder("FOLDER-B", "B分类", Some("FOLDER-A")), // A的嵌套文件夹。
        ],
        vec![
            map_entry("HN-01", "A内地图", Some("FOLDER-A")), // A展开后可见。
            map_entry("HN-02", "B内地图", Some("FOLDER-B")), // A与B同时展开后可见。
        ],
    );
    let collapsed = project_visible_browser_tree(&full, &BTreeSet::new()); // 默认全部文件夹折叠。
    assert_eq!(collapsed.len(), 1); // 只显示根目录直属A目录。
    let a_expanded = project_visible_browser_tree(&full, &BTreeSet::from(["FOLDER-A".to_owned()]));
    assert_eq!(
        a_expanded
            .iter()
            .map(|entry| entry.display_name.as_str())
            .collect::<Vec<_>>(),
        vec!["A分类", "B分类", "A内地图"],
    ); // B仍折叠，但A的直接地图在全部子目录之后显示。
    let all_expanded = project_visible_browser_tree(
        &full,
        &BTreeSet::from(["FOLDER-A".to_owned(), "FOLDER-B".to_owned()]),
    );
    assert_eq!(all_expanded.len(), full.len()); // 完整父链展开后所有地图可见。
}

#[test]
fn drag_requires_six_physical_pixels() {
    assert!(!drag_threshold_reached(Vec2::ZERO, Vec2::new(5.99, 0.0))); // 阈值内仍是普通单击。
    assert!(drag_threshold_reached(Vec2::ZERO, Vec2::new(6.0, 0.0))); // 精确边界进入拖拽。
}
