//! 时间、天气和草尖摆幅复用场景与方案库的保存、预览和撤销事务。

use feathrise_atmosphere::{WeatherDebugOverride, WorldClock};
use feathrise_editor_core::EditorSettings;
use feathrise_scene_content::{EnvironmentSimulation, SceneEnvironmentSource};

use super::types::{SimulationDrag, SimulationTarget};
use crate::authoring::{
    EditorAuthoringSession, GlobalEnvironmentAuthoringState, GlobalEnvironmentSaveTask,
    commit_environment_change, environment_library::commit_profile_change,
};
use crate::resources::SceneEditorState;
use crate::ui::{environment::{EnvironmentPage, EnvironmentPageState}, sidebars::{EditorRightSidebarTab, SceneEditorSidebarState}};

pub(crate) fn simulation_from_runtime(
    clock: &WorldClock,
    weather: &WeatherDebugOverride,
    settings: &EditorSettings,
) -> EnvironmentSimulation {
    EnvironmentSimulation {
        hour: clock.hour_for_display(), month: clock.month(), day: clock.day(),
        paused: settings.environment.world_time_paused,
        weather_mode_index: weather.mode_index() as u8,
        weather_transition: weather.preview(),
        grass_wind_tip_strength: settings.environment.grass_wind_tip_strength,
    }
}

pub(super) fn editing_target(
    editor: &SceneEditorState,
    sidebar: &SceneEditorSidebarState,
    page: &EnvironmentPageState,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
    save: &GlobalEnvironmentSaveTask,
) -> Option<SimulationTarget> {
    if !editor.open || editor.close_requested || editor.new_scene_dialog_open || session.saving
        || save.task.is_some() || sidebar.right != EditorRightSidebarTab::Environment {
        return None;
    }
    match page.page {
        EnvironmentPage::Library => global.selected_environment_id.clone()
            .filter(|_| global.selected_environment().is_some()).map(SimulationTarget::Profile),
        EnvironmentPage::Scene => session.draft.as_ref().filter(|draft| {
            !editor.scene_transition_in_progress && draft.manifest.is_data_driven()
                && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
                && draft.manifest.environment.source == SceneEnvironmentSource::Local
        }).map(|draft| SimulationTarget::Scene(draft.scene_id.clone())),
    }
}

impl SimulationTarget {
    pub(super) fn version(&self, session: &EditorAuthoringSession, global: &GlobalEnvironmentAuthoringState) -> (u64, u64) {
        (match self { Self::Scene(_) => session.revision, Self::Profile(_) => global.revision }, global.source_revision)
    }

    pub(super) fn value(&self, session: &EditorAuthoringSession, global: &GlobalEnvironmentAuthoringState) -> Option<EnvironmentSimulation> {
        match self {
            Self::Scene(id) => session.draft.as_ref().filter(|draft| &draft.scene_id == id)
                .and_then(|draft| draft.manifest.environment.simulation),
            Self::Profile(id) => global.draft.library.profiles.get(id).map(|profile| profile.appearance.simulation),
        }
    }

    pub(super) fn dirty(&self, session: &EditorAuthoringSession, global: &GlobalEnvironmentAuthoringState) -> bool {
        match self {
            Self::Scene(_) => session.draft.as_ref().is_some_and(|draft| draft.environment_dirty),
            Self::Profile(_) => global.dirty,
        }
    }

    pub(super) fn write(&self, value: Option<EnvironmentSimulation>, session: &mut EditorAuthoringSession, global: &mut GlobalEnvironmentAuthoringState) {
        match self {
            Self::Scene(id) => {
                if let Some(draft) = session.draft.as_mut().filter(|draft| &draft.scene_id == id) {
                    draft.manifest.environment.simulation = value;
                    draft.environment_dirty = true;
                }
            }
            Self::Profile(id) => {
                if let Some(profile) = global.draft.library.profiles_mut().get_mut(id) {
                    profile.appearance.simulation = value.unwrap_or_default();
                    global.dirty = true;
                }
            }
        }
    }
}

impl SimulationDrag {
    pub(super) fn finish(self, cancel: bool, session: &mut EditorAuthoringSession, global: &mut GlobalEnvironmentAuthoringState, editor: &mut SceneEditorState) {
        // 身份和最后写入值共同约束所有权。换图、热更或外部改写后不覆盖新值。
        if self.target.value(session, global) != Some(self.latest) { return; }
        let unchanged = self.before == Some(self.latest);
        if cancel || unchanged {
            self.target.write(self.before, session, global);
            if self.target.version(session, global) == self.version {
                match &self.target {
                    SimulationTarget::Scene(_) => {
                        if let Some(draft) = session.draft.as_mut() { draft.environment_dirty = self.dirty_before; }
                    }
                    SimulationTarget::Profile(_) => global.dirty = self.dirty_before,
                }
            }
            editor.dirty = global.dirty || session.has_scene_changes();
            return;
        }
        match self.target {
            SimulationTarget::Scene(_) => {
                let Some(after) = session.draft.as_ref().map(|draft| draft.manifest.environment.clone()) else { return };
                let mut before = after.clone();
                before.simulation = self.before;
                commit_environment_change(session, editor, before, after, "场景时间、天气与草尖摆幅已更新");
            }
            SimulationTarget::Profile(id) => {
                let Some(after) = global.draft.library.profiles.get(&id).cloned() else { return };
                let mut before = after.clone();
                before.appearance.simulation = self.before.unwrap_or_default();
                if let Err(error) = commit_profile_change(id, Some(before), after, global, session, editor) {
                    editor.status = error;
                }
            }
        }
    }
}
