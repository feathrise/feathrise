//! 菜单冻结的目录与场景身份；条目换绑后绝不按同一下标猜测新文件。

use feathrise_scene_content::SceneId; // 切换场景立即使上一菜单失效。
use std::path::PathBuf;

/// 两种复制都取右键冻结路径；相对路径按当前来源根计算，绝对路径只做词法拼接。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetPathCopyKind { Relative, Absolute }

impl AssetPathCopyKind {
    pub const ALL: [Self; 2] = [Self::Relative, Self::Absolute];
    pub fn description(self) -> &'static str {
        match self { Self::Relative => "相对路径", Self::Absolute => "绝对路径" }
    }
    pub fn label(self) -> &'static str {
        match self { Self::Relative => "复制相对路径", Self::Absolute => "复制绝对路径" }
    }
}

/// 仅包含点击时的一条记录和轻量版本，不复制或扫描整个目录。
pub(super) enum AssetMenuSource {
    Assets,
    Project { root: PathBuf, context: u64 }, // 工程根由浏览来源冻结，不能从assets父目录推测。
}

pub(super) struct AssetMenuTarget {
    pub(super) path: String,          // 复制内容始终来自实际条目，而非显示名。
    pub(super) index: usize,          // 全目录绝对索引，不由可复用行槽或旧页码换算。
    pub(super) revision: u64,         // 当前已经采用的目录版本。
    pub(super) viewport_revision: u64, // 目录滚动换绑立即取消旧菜单，不能把同槽另一文件当成原目标。
    pub(super) scene_id: Option<SceneId>, // 多张动态场景使用相同场景枚举，必须绑定真实SceneId。
    pub(super) source: AssetMenuSource, // 同一菜单在工程目录与登记资产目录间共用鼠标所有权。
}
