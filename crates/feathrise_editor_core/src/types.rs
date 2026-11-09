//! 跨地图、预设和文档共享的中央标签身份；不承载具体草稿。

/// 工程预览和独立启动共用的显式构建选择。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectBuildProfile {
    #[default]
    Debug,
    Release,
}

impl ProjectBuildProfile {
    pub fn label(self) -> &'static str {
        match self { Self::Debug => "Debug", Self::Release => "Release" }
    }

    /// DX 与导出使用的 Cargo profile。原生 Debug 由编辑器宿主映射到 `dev`。
    pub fn cargo_profile(self) -> &'static str {
        match self { Self::Debug => "render-dev", Self::Release => "release" }
    }
}

/// 中央工作区的顶层标签身份。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresetWorkspaceTab {
    /// 永久存在且不可关闭的地图三维视口。
    #[default]
    Map,
    /// 文本文档宿主；多个文件标签及当前路径由文档会话持有。
    Document,
    /// 图片/GLTF宿主；多个文件标签复用唯一画布与离屏相机。
    AssetPreview,
    /// 一个独立植被预设草稿标签；数字只在本次 F12 会话内稳定。
    Vegetation(u64),
    /// 一个独立 PBR 地表预设草稿标签。
    Surface(u64),
    /// 一个独立静水外观预设草稿标签。
    Water(u64),
    /// 一个独立建筑空间语义预设草稿标签。
    Building(u64),
    /// 应用已安装作者域的标签；含义与草稿由该应用拥有，编辑器不绑定具体游戏类型。
    Domain(&'static str, u64),
}
