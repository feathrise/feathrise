//! 界面语言属于作者个人偏好，保存值独立于游戏语言和当前系统语言。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorLocalePreference {
    #[default]
    System,
    Chinese,
    English,
}

impl EditorLocalePreference {
    pub fn from_saved(value: &str) -> Self {
        match value {
            "system" => Self::System,
            "zh-CN" => Self::Chinese,
            "en-US" => Self::English,
            // 无法识别的语言记录使用与未配置相同的系统档。
            _ => Self::default(),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Chinese => "zh-CN",
            Self::English => "en-US",
        }
    }
}
