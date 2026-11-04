//! 资产浏览后台扫描与反向定位的纯数据身份；不保存任何地图或预设修改。

use feathrise_map_content::MapId; // 定位结果不得跨地图会话回跳。

use crate::types::EditorAssetEntry; // 一层目录的完整拥有型结果只在后台扫描和排序。

/// 连续目录的一次性可见锚点；目录定位、列表/缩略图切换与列数变化共用实际布局滚动。
#[derive(Clone, Copy, Debug)]
pub(crate) struct AssetScrollAnchor {
    pub(crate) ticket: u64,
    pub(crate) revision: u64,
    pub(crate) index: usize,
    pub(crate) require_selection: bool,
}

/// 仅当前身份通过核验的完成值可发布；不以底栏提示或空列表推断扫描成功。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetCatalogFeedback {
    pub(crate) revision: u64,
    pub(crate) result: Result<(), String>,
}

/// 同一完整路径、目录修订和页内的两次按下才算双击；固定行池换内容不能误打开。
pub(super) struct TimedAssetFileClick {
    pub path: String,
    pub revision: u64,
    pub viewport_revision: u64,
    pub seconds: f64,
}

pub(crate) use feathrise_editor_documents_model::types::DocumentFormat;

/// 一个后台任务冻结的目录修订与完整目标，不能只靠文件名匹配。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AssetScanRequest {
    pub(super) revision: u64,  // 刷新、换目录和新定位均推进请求身份。
    pub(super) folder: String, // 相对 assets 根的待扫描父目录。
    pub(super) reveal_file: Option<String>, // 定位时精确匹配完整文件路径，普通浏览为 None。
}

/// 后台完成排序和定位后的目录；主线程只读取目标索引，绝不再扫描全表。
pub(super) struct AssetScanContents {
    pub(super) entries: Vec<EditorAssetEntry>, // 完整目录，不按产品常量截断。
    pub(super) reveal_index: Option<usize>,    // 后台按最终排序计算的绝对行号。
}

/// 后台完成值同时携带输入身份，主线程必须复核后才能原子发布。
pub(super) struct AssetScanResult {
    pub(super) request: AssetScanRequest, // 冻结目录、修订和可选定位目标。
    pub(super) result: Result<AssetScanContents, String>, // 失败不覆盖上一完整目录或选择。
}

/// 点击引用行“定位”冻结的浏览意图；等待期间继续显示旧目录与旧预览。
#[derive(Clone, Debug)]
pub(super) struct AssetRevealRequest {
    pub(super) is_directory: bool,     // 目录定位扫描目录本身，不把它冒充预览文件。
    pub(super) revision: u64,           // 与唯一目录扫描任务共享同一修订。
    pub(super) reference: String,       // 保留 #Mesh/Material 等子资产信息供完成提示。
    pub(super) file_path: String,       // 浏览器只选所属文件，不把子资产标签冒充文件名。
    pub(super) folder: String,          // 文件的直接父目录，根目录用空字符串。
    pub(super) original_folder: String, // 用户主动导航后旧结果不得抢回目录。
    pub(super) original_scroll_intent_revision: u64, // 只追踪用户滚动，不混入布局或池容量变化。
    pub(super) original_selection: Option<String>, // 外部选择变化也能撤销旧定位。
    pub(super) map_id: Option<MapId>,   // 换图后不能采用上一个作者会话的请求。
}
