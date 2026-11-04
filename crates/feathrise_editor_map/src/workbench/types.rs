//! 工作台目录记录与后台产物；大型投影只通过Arc交接。
use crate::types::MapBrowserEntry;
use std::{collections::HashMap, sync::Arc};

// 直接借用生产目录类型，后台打开工程后即可O(1)交接，不在宿主消息泵复制整张地图表。
pub use feathrise_map_content::MapCatalogEntry as WorkbenchMap;

/// 有界可见行和真实UI矩形，供宿主验证原目录交互；不复制完整资产目录。
#[derive(Clone, Default, serde::Serialize)]
pub struct AssetBrowserProbe {
    pub root: Option<std::path::PathBuf>,
    pub folder: String,
    pub selected: Option<String>,
    pub active: bool,
    pub ready: bool,
    /// 地图目录和资产导航按钮的物理矩形。
    pub tabs: [Option<[f32; 4]>; 2],
    pub entries: Vec<(String, bool, [f32; 4])>,
}

pub(super) struct CatalogResult {
    pub context: u64,
    pub source: Arc<Vec<WorkbenchMap>>,
    pub entries: Arc<Vec<MapBrowserEntry>>,
    pub indices: HashMap<String, usize>,
}
