//! 项目环境库与场景环境来源的作者事务。地域气候身份始终留在场景清单。

use bevy::prelude::*;
use feathrise_scene_content::{
    EnvironmentAppearance, EnvironmentSimulation, SceneEnvironmentDescriptor, SceneEnvironmentSource, SceneSkyMode,
    WorldEnvironmentProfile, resolve_scene_environment,
};

use super::environment::{commit_environment_change, edited_environment};
use super::{EditorAuthoringSession, EditorCommand, GlobalEnvironmentAuthoringState};
use crate::{resources::SceneEditorState, types::EnvironmentEditAction};

#[cfg(test)]
#[path = "environment_library_tests.rs"]
mod tests;

#[derive(Message, Clone)]
pub(crate) enum EnvironmentLibraryEditRequest {
    Select(String),
    Create,
    Duplicate(String),
    Rename(String, String),
    SetDefault(String),
    UseProjectDefault,
    UseShared(String),
    MakeLocal,
    SetGlobalSkyMode(SceneSkyMode),
    SetGlobalSelectedSkybox,
    ClearGlobalSkybox,
}

#[derive(Clone)]
pub enum EnvironmentLibraryChange {
    Profile {
        id: String,
        before: Option<WorldEnvironmentProfile>,
        after: Option<WorldEnvironmentProfile>,
    },
    Default {
        before: String,
        after: String,
    },
}

impl EnvironmentLibraryChange {
    pub(crate) fn apply(
        &self,
        global: &mut GlobalEnvironmentAuthoringState,
        undoing: bool,
    ) -> Result<(), String> {
        let listing_changed = match self {
            Self::Profile { id, before, after } => {
                if let Some(profile) = if undoing { before } else { after } {
                    profile.appearance.validate()?;
                    global
                        .draft
                        .library
                        .profiles_mut()
                        .insert(id.clone(), profile.clone());
                } else {
                    global.draft.library.profiles_mut().remove(id);
                }
                before.as_ref().map(|profile| &profile.name)
                    != after.as_ref().map(|profile| &profile.name)
            }
            Self::Default { before, after } => {
                let id = if undoing { before } else { after };
                if !global.draft.library.profiles.contains_key(id) {
                    return Err("项目默认环境对应的套装已经不存在".into());
                }
                global.draft.library.default_profile_id.clone_from(id);
                true
            }
        };
        if listing_changed {
            global.library_listing_revision = global.library_listing_revision.wrapping_add(1);
            global.maintain_environment_selection();
        }
        Ok(())
    }
}

impl GlobalEnvironmentAuthoringState {
    pub(crate) fn maintain_environment_selection(&mut self) {
        if self.selected_environment().is_none() {
            self.selected_environment_id = Some(self.draft.library.default_profile_id.clone())
                .filter(|id| self.draft.library.profiles.contains_key(id));
        }
    }

    pub(crate) fn selected_environment(&self) -> Option<&WorldEnvironmentProfile> {
        self.draft
            .library
            .profiles
            .get(self.selected_environment_id.as_deref()?)
    }

    pub(crate) fn selected_celestial(&self) -> &feathrise_scene_content::CelestialLightingProfile {
        self.selected_environment()
            .map_or(&self.draft.celestial_lighting, |profile| {
                &profile.appearance.celestial_lighting
            })
    }

    pub(crate) fn selected_celestial_mut(
        &mut self,
    ) -> &mut feathrise_scene_content::CelestialLightingProfile {
        // 旧文件的基础天体参数给遗留场景使用。库套装修改只写选中的完整方案。
        if let Some(id) = self.selected_environment_id.as_deref()
            && let Some(profile) = self.draft.library.profiles_mut().get_mut(id)
        {
            &mut profile.appearance.celestial_lighting
        } else {
            &mut self.draft.celestial_lighting
        }
    }
}

pub(super) fn scene_environment_editable(environment: &SceneEnvironmentDescriptor) -> bool {
    matches!(
        environment.source,
        SceneEnvironmentSource::Legacy | SceneEnvironmentSource::Local
    )
}

pub(super) fn profile_sky(profile: &WorldEnvironmentProfile) -> SceneEnvironmentDescriptor {
    let mut environment = SceneEnvironmentDescriptor::default();
    profile.appearance.apply_to(&mut environment);
    environment
}

pub(super) fn set_profile_sky(
    profile: &mut WorldEnvironmentProfile,
    environment: &SceneEnvironmentDescriptor,
) {
    // 原天空字段校验和规范化继续由场景作者入口完成，库只复制其外观字段。
    let appearance = &mut profile.appearance;
    appearance.sky_mode = environment.sky_mode;
    appearance.skybox_path.clone_from(&environment.skybox_path);
    appearance.skybox_brightness = environment.skybox_brightness;
    appearance.environment_diffuse_intensity = environment.environment_diffuse_intensity;
    appearance.environment_specular_intensity = environment.environment_specular_intensity;
    appearance.skybox_yaw_degrees = environment.skybox_yaw_degrees;
}

fn commit_library_change(
    change: EnvironmentLibraryChange,
    global: &mut GlobalEnvironmentAuthoringState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
) -> Result<(), String> {
    change.apply(global, false)?;
    session
        .undo
        .push(EditorCommand::GlobalEnvironmentLibraryChanged {
            sequence: global.revision.wrapping_add(1),
            change: Box::new(change),
        });
    session.redo.clear();
    global.mark_changed(session, editor);
    editor.status = "全局环境库草稿已更新".into();
    Ok(())
}

pub(crate) fn commit_profile_change(
    id: String,
    before: Option<WorldEnvironmentProfile>,
    after: WorldEnvironmentProfile,
    global: &mut GlobalEnvironmentAuthoringState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
) -> Result<(), String> {
    if before.as_ref() == Some(&after) {
        return Ok(());
    }
    commit_library_change(
        EnvironmentLibraryChange::Profile {
            id,
            before,
            after: Some(after),
        },
        global,
        session,
        editor,
    )
}

fn change_scene_source(
    source: SceneEnvironmentSource,
    global: &GlobalEnvironmentAuthoringState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    legacy_simulation: EnvironmentSimulation,
) -> Result<(), String> {
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| draft.manifest.is_data_driven())
        .ok_or("当前场景没有可编辑的环境")?;
    let before = draft.manifest.environment.clone();
    if before.source == source {
        return Ok(());
    }
    if let SceneEnvironmentSource::Shared(id) = &source
        && !global.draft.library.profiles.contains_key(id)
    {
        return Err("所选全局环境已经不存在".into());
    }
    let mut after = before.clone();
    if matches!(before.source, SceneEnvironmentSource::Local) {
        after.local_environment_initialized = true;
    }
    if matches!(source, SceneEnvironmentSource::Local) && !after.local_environment_initialized {
        // 第一次制作副本冻结当前完整生效值。往返切换时恢复这份独立配置。
        let mut resolved = resolve_scene_environment(
            &before,
            &global.draft.celestial_lighting,
            &global.draft.library,
        );
        resolved.environment.simulation.get_or_insert(legacy_simulation);
        EnvironmentAppearance::from_resolved(&resolved).apply_to(&mut after);
        after.local_environment_initialized = true;
    }
    after.source = source;
    after.validate()?;
    commit_environment_change(session, editor, before, after, "场景环境来源已更新");
    Ok(())
}

fn library_action(
    request: EnvironmentLibraryEditRequest,
    global: &mut GlobalEnvironmentAuthoringState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    legacy_simulation: EnvironmentSimulation,
) -> Result<(), String> {
    use EnvironmentLibraryEditRequest::*;
    match request {
        Select(id) => {
            if global.draft.library.profiles.contains_key(&id) {
                global.selected_environment_id = Some(id);
            }
        }
        request @ (Create | Duplicate(_)) => {
            let profile = if let Duplicate(id) = request {
                let mut profile = global
                    .draft
                    .library
                    .profiles
                    .get(&id)
                    .cloned()
                    .ok_or("要复制的环境已经不存在")?;
                profile.name.push_str(" · 副本");
                profile
            } else {
                let environment = session
                    .draft
                    .as_ref()
                    .map(|draft| &draft.manifest.environment);
                let mut resolved = resolve_scene_environment(
                    environment.unwrap_or(&SceneEnvironmentDescriptor::inheriting_default()),
                    &global.draft.celestial_lighting,
                    &global.draft.library,
                );
                resolved.environment.simulation.get_or_insert(legacy_simulation);
                WorldEnvironmentProfile {
                    name: format!("环境 {}", global.draft.library.profiles.len() + 1),
                    appearance: EnvironmentAppearance::from_resolved(&resolved),
                }
            };
            let id = format!("env-{}", bevy::asset::uuid::Uuid::new_v4().simple());
            commit_profile_change(id.clone(), None, profile, global, session, editor)?;
            global.selected_environment_id = Some(id);
        }
        Rename(id, name) => {
            let before = global
                .draft
                .library
                .profiles
                .get(&id)
                .cloned()
                .ok_or("要改名的环境已经不存在")?;
            let name = name.trim();
            if name.is_empty() || name.chars().any(char::is_control) {
                return Err("请输入有效的环境名称".into());
            }
            let mut after = before.clone();
            after.name = name.to_owned();
            commit_profile_change(id, Some(before), after, global, session, editor)?;
        }
        SetDefault(after) => {
            let before = global.draft.library.default_profile_id.clone();
            if before != after {
                commit_library_change(
                    EnvironmentLibraryChange::Default { before, after },
                    global,
                    session,
                    editor,
                )?;
            }
        }
        UseProjectDefault => change_scene_source(
            SceneEnvironmentSource::InheritDefault,
            global,
            session,
            editor,
            legacy_simulation,
        )?,
        UseShared(id) => {
            change_scene_source(SceneEnvironmentSource::Shared(id), global, session, editor, legacy_simulation)?
        }
        MakeLocal => change_scene_source(SceneEnvironmentSource::Local, global, session, editor, legacy_simulation)?,
        action => {
            let action = match action {
                SetGlobalSkyMode(mode) => EnvironmentEditAction::SetSkyMode(mode),
                SetGlobalSelectedSkybox => EnvironmentEditAction::SetSelectedSkybox,
                ClearGlobalSkybox => EnvironmentEditAction::ClearSkybox,
                _ => unreachable!("已处理所有环境来源与目录操作"),
            };
            let id = global
                .selected_environment_id
                .clone()
                .ok_or("请先选中一套全局环境")?;
            let before = global
                .selected_environment()
                .cloned()
                .ok_or("所选环境已经不存在")?;
            let mut after = before.clone();
            let environment = edited_environment(
                &profile_sky(&before),
                action,
                editor.selected_asset.as_deref(),
            )?;
            set_profile_sky(&mut after, &environment);
            commit_profile_change(id, Some(before), after, global, session, editor)?;
        }
    }
    Ok(())
}

pub(crate) fn apply_environment_library_requests(
    mut requests: MessageReader<EnvironmentLibraryEditRequest>,
    mut global: ResMut<GlobalEnvironmentAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    global_save: Res<super::GlobalEnvironmentSaveTask>,
    clock: Res<feathrise_atmosphere::WorldClock>,
    weather: Res<feathrise_atmosphere::WeatherDebugOverride>,
    settings: Res<feathrise_editor_core::EditorSettings>,
) {
    for request in requests.read() {
        if !editor.open || session.saving || global_save.task.is_some() || editor.close_requested {
            continue;
        }
        if matches!(
            request,
            EnvironmentLibraryEditRequest::UseProjectDefault
                | EnvironmentLibraryEditRequest::UseShared(_)
                | EnvironmentLibraryEditRequest::MakeLocal
        ) && (editor.scene_transition_in_progress
            || session.draft.as_ref().map(|draft| &draft.scene_id)
                != editor.selected_scene_id.as_ref())
        {
            continue; // 切场景空窗不把上一场景的来源按钮应用到另一份草稿。
        }
        let request = request.clone();
        let legacy_simulation = crate::world_environment_controls::simulation_from_runtime(&clock, &weather, &settings);
        if let Err(error) = library_action(request, &mut global, &mut session, &mut editor, legacy_simulation) {
            editor.status = error;
        }
    }
}
