//! 环境页的固定回显字段及小型语义快照；不复制场景数据或建立新运行时状态机。

use feathrise_atmosphere::AuthoredSkyboxStatus;
use feathrise_scene_content::{SceneEnvironmentDescriptor, SceneId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum EnvironmentTextField {
    #[default]
    SceneContext,
    SceneSource,
    SceneHint,
    CurrentValues,
    SceneFeedback,
    GlobalFeedback,
}

/// 与正式光照共享曲线的计算值；加载阶段另行回显，计算完成不能冒充资源已采用。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CurrentIblValues {
    pub(super) day_night_factor: f32,
    pub(super) diffuse: f32,
    pub(super) specular: f32,
    pub(super) skybox: f32,
}

#[derive(Default)]
pub(crate) struct EnvironmentObservation {
    pub(super) locale: feathrise_editor_ui::localization::EditorLocale,
    pub(super) scene_id: Option<SceneId>,
    pub(super) draft_scene_id: Option<SceneId>,
    pub(super) scene_name: Option<String>,
    pub(super) runtime_scene: Option<String>,
    pub(super) environment: Option<SceneEnvironmentDescriptor>,
    pub(super) loading: bool,
    pub(super) environment_revision: u64,
    pub(super) environment_dirty: bool,
    pub(super) saving: bool,
    pub(super) ibl: Option<AuthoredSkyboxStatus>,
    pub(super) current_values: Option<CurrentIblValues>,
    pub(super) global_initialized: bool,
    pub(super) global_dirty: bool,
    pub(super) global_saving: bool,
    pub(super) global_error: Option<String>,
}
