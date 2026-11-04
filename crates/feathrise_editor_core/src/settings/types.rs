//! 作者工具配色和资产视图的稳定偏好。

/// F12三套已确认配色；只选择工具外观，不参与游戏界面或作者文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorColorTheme {
    #[default]
    NeutralGray,
    GraphiteBlue,
    WarmCopper,
}

impl EditorColorTheme {
    pub const ALL: [Self; 3] = [Self::NeutralGray, Self::GraphiteBlue, Self::WarmCopper];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NeutralGray => "NeutralGray",
            Self::GraphiteBlue => "GraphiteBlue",
            Self::WarmCopper => "WarmCopper",
        }
    }

    pub fn from_saved(value: &str) -> Self {
        match value {
            "GraphiteBlue" => Self::GraphiteBlue,
            "WarmCopper" => Self::WarmCopper,
            _ => Self::NeutralGray, // 缺项和未来未知身份不影响其他设置加载。
        }
    }
}

/// 资产浏览器的显示偏好；两种模式共享目录、分页和选择身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AssetBrowserViewMode {
    #[default]
    List,
    Thumbnails,
}

impl AssetBrowserViewMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::Thumbnails => "Thumbnails",
        }
    }

    /// 旧文件及未知值都保留原列表，不使设置加载失败。
    pub fn from_saved(value: &str) -> Self {
        match value {
            "Thumbnails" => Self::Thumbnails,
            _ => Self::List,
        }
    }
}
