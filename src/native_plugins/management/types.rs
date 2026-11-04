//! 单文件作者插件的数据合同；安装状态与当前实例装载分离，已有会话不热卸载。
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) use feathrise_plugin_api::PluginMetadata as PackageManifest;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PluginScope {
    #[default]
    Project,
    Global,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginRoots {
    pub project: Option<PathBuf>,
    /// 此字段直接指向 ~/.feathrise/plugins，不能再隐式追加一层 plugins。
    pub global: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginOrigin {
    pub scope: PluginScope,
    pub path: PathBuf,
    pub digest: String,
}

/// 旧目录归档的实际完成来源；诊断只能按原路径及原清单摘要精确消解。
#[derive(Clone, Debug)]
pub(crate) struct ArchivedLegacy {
    pub id: String,
    pub origin: PluginOrigin,
    pub error: String,
    pub archived: PathBuf,
    /// 后台生成目录快照后核对，主帧不得补做文件存在性检查。
    pub original_vacant: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Catalog {
    pub entries: Vec<PackageEntry>,
    pub warnings: Vec<String>,
    /// 不影响加载的迁移说明单独保留，不能投影为作者工具的加载失败。
    pub notices: Vec<String>,
    /// 只有后台文件/配置事务确实完成才提供回执，不能用待生效计划伪造成功。
    pub completion: Option<String>,
    pub archived_legacy: Option<ArchivedLegacy>,
}

#[derive(Clone, Debug)]
pub(crate) struct PackageEntry {
    pub id: String,
    pub tool: String,
    pub version: Option<String>,
    /// 保留原视图字段名，值现在是唯一原生二进制路径，而不是包目录。
    pub directory: PathBuf,
    pub path: PathBuf,
    pub scope: PluginScope,
    pub inherited: bool,
    pub effective: bool,
    pub overridden_by_project: bool,
    pub global_available: bool,
    pub legacy: bool,
    pub origin: Option<PluginOrigin>,
    pub enabled: bool,
    pub error: Option<String>,
    pub pending: Option<PendingChange>,
    pub runtime_dependencies: Vec<String>,
    pub runtime_missing: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PendingChange {
    pub kind: PendingKind,
    pub version: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PendingKind {
    Install,
    Update,
    Enable,
    Disable,
    Remove,
}

#[derive(Clone, Debug)]
pub(crate) enum ManagementCommand {
    OpenDirectory,
    Install { source: PathBuf, replace: bool },
    SetEnabled { id: String, enabled: bool },
    Remove { id: String },
    Cancel { id: String },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    pub format: u32,
    /// 全局总开关或本工程直接安装副本的偏好；移除本地副本不改变继承全局的选择。
    pub disabled: std::collections::BTreeSet<String>,
    /// 本工程对继承全局副本的独立偏好；旧工程状态缺省为空，不将旧本地停用误套给全局。
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub inherited_disabled: std::collections::BTreeSet<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    pub format: u32,
    pub scope: PluginScope,
    pub changes: BTreeMap<String, PendingRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingRecord {
    pub manifest: PackageManifest,
    pub kind: PendingKind,
    pub ticket: String,
    /// 正式文件必须是 plugins 的直属文件，绝不把清单 ID 解释成路径。
    pub destination: String,
    pub before_digest: Option<String>,
    /// 全局索引身份与实际文件摘要分别冻结；坏对象仍可显式移除，不能借机覆盖新索引。
    pub before_index: Option<GlobalRecord>,
    pub after_digest: Option<String>,
    /// 工程可以禁用继承的全局副本；不能借此移除或改写全局对象。
    pub inherited: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GlobalIndex {
    pub format: u32,
    pub entries: BTreeMap<String, GlobalRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GlobalRecord {
    pub digest: String,
    pub filename: String,
}
