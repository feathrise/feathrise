//! 当前工程的共享资料页。表单使用中央工作区原标签，读写交给项目清单后台任务。

mod components;
mod resources;
mod scene;
mod systems;
#[cfg(all(feature = "validation", windows))]
pub(crate) mod validation;

use crate::project::Project;
use bevy::prelude::*;
use feathrise_editor_core::{PresetWorkspaceTab, SceneEditorSystemSet};
use resources::{IconPicker, ProjectSettingsState};

const TAB: PresetWorkspaceTab = PresetWorkspaceTab::Domain("feathrise.project-settings", 0);
const TITLE: &str = "工程设置";

#[derive(Message)]
pub(crate) struct OpenProjectSettings;

/// 只有清单已成功发布才通知工程菜单、窗口和工程管理器。
#[derive(Message)]
pub(crate) struct ProjectSettingsSaved {
    pub(crate) project: Project,
}

pub(crate) use resources::ProjectSettingsPresentation;

pub(crate) struct ProjectSettingsPlugin(Project);

impl ProjectSettingsPlugin {
    pub(crate) fn new(project: Project) -> Self {
        Self(project)
    }
}

impl Plugin for ProjectSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ProjectSettingsState::new(self.0.clone()))
            .insert_non_send(IconPicker::default())
            .init_resource::<ProjectSettingsPresentation>()
            .add_message::<OpenProjectSettings>()
            .add_message::<ProjectSettingsSaved>()
            .add_systems(Startup, systems::begin_load)
            .add_systems(
                Update,
                (systems::poll, systems::capture_fields, systems::input)
                    .chain()
                    .in_set(SceneEditorSystemSet::Input),
            )
            .add_systems(
                Update,
                (scene::mount, scene::present)
                    .chain()
                    .after(SceneEditorSystemSet::Presentation),
            );
        systems::install_save_adapter(app);
        #[cfg(all(feature = "validation", windows))]
        validation::install(app, &self.0);
    }
}
