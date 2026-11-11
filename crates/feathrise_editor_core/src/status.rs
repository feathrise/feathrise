//! 作者操作反馈的稳定值契约；不拥有场景会话或反馈槽。

/// 显式操作回执的表现语义；普通预览文字不能推断或覆盖保存结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorStatusKind {
    Success,
    Warning,
    Error,
}

/// 各类操作使用固定回执槽；不同操作及不同对象的成功不能互相清除失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorStatusOperation {
    SceneSave,
    GlobalEnvironmentSave,
    SceneCreate,
    SceneDelete,
    SurfacePresetSave,
    WaterPresetSave,
    VegetationPresetSave,
    /// 外部领域工具的真实操作结果；不要求公共编辑器认识装备等业务类型。
    NativePlugin,
}

impl EditorStatusOperation {
    pub const COUNT: usize = 8;

    pub fn slot(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SceneSave => "保存场景",
            Self::GlobalEnvironmentSave => "保存全局环境",
            Self::SceneCreate => "新建场景",
            Self::SceneDelete => "删除场景",
            Self::SurfacePresetSave => "保存地表预设",
            Self::WaterPresetSave => "保存水体预设",
            Self::VegetationPresetSave => "保存植被预设",
            Self::NativePlugin => "插件工具",
        }
    }
}

/// 只冻结反馈用的对象身份与名称，不参与文件票据、作者修订或采用资格。
#[derive(Clone, Debug)]
pub struct EditorStatusTarget {
    pub id: std::sync::Arc<str>,
    pub label: std::sync::Arc<str>,
}

impl EditorStatusTarget {
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: std::sync::Arc::from(id),
            label: std::sync::Arc::from(label),
        }
    }
}
