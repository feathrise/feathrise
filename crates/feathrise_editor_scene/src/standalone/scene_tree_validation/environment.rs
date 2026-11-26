//! 自有项目原生环境验收。使用原按钮修改场景和套装，交给父夹具统一保存。

use bevy::{prelude::*, tasks::futures::check_ready};
use feathrise_scene_content::{
    DEFAULT_ENVIRONMENT_PROFILE_ID, SceneEnvironmentSource, SceneId, WorldEnvironmentRegistry,
    resolve_scene_environment,
};
use serde_json::{Value, json};

use super::{
    StartupScreenshot, click_control, request_screenshot, source_space_button, ui_visible,
};
use crate::authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState};

const SCREENSHOT: &str = "scene-tree-environment.png";

pub(in crate::standalone) fn enabled() -> bool {
    std::env::var("FEATHRISE_SCENE_TREE_STARTUP_VALIDATION").as_deref() == Ok("environment")
}

pub(super) fn reopened(world: &mut World, scene: &SceneId) -> Option<Value> {
    let draft = world.resource::<EditorAuthoringSession>().draft.as_ref()?;
    if &draft.scene_id != scene || draft.environment_dirty {
        return None;
    }
    let environment = &draft.manifest.environment;
    let SceneEnvironmentSource::Shared(id) = &environment.source else {
        return None;
    };
    let registry = world.resource::<WorldEnvironmentRegistry>();
    let profile = registry.library.profiles.get(id)?;
    let resolved =
        resolve_scene_environment(environment, &registry.celestial_lighting, &registry.library);
    if registry.revision == 0
        || registry.last_error.is_some()
        || &registry.library.default_profile_id != id
        || resolved.profile_id.as_ref() != Some(id)
        || resolved.missing_profile.is_some()
    {
        return None;
    }
    let (id, name) = (id.clone(), profile.name.clone());
    let source_text = format!("环境来源  共享方案 · {name}");
    let visible = world
        .query::<(Entity, &Text)>()
        .iter(world)
        .any(|(entity, text)| {
            text.0.lines().next() == Some(source_text.as_str()) && ui_visible(world, entity)
        });
    if !visible {
        // 重开只进入原环境页，引用和默认方案都由已保存文件恢复。
        if let Some(tab) =
            source_space_button(world, "场景环境").or_else(|| source_space_button(world, "环境"))
        {
            world
                .entity_mut(tab)
                .insert(feathrise_editor_ui::EditorButtonClick::triggered());
        }
        return None;
    }
    Some(
        json!({"profile_id": id, "profile_name": name, "scene_source": "shared",
        "visible_source": source_text}),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::standalone) struct Step {
    phase: Phase,
    frame: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Tool,
    Library,
    Create,
    SetDefault,
    Scene,
    Choose,
    Bind,
    Capture,
    Screenshot,
}

impl Step {
    pub(super) fn begin() -> Self {
        Self {
            phase: Phase::Tool,
            frame: 0,
        }
    }
}

fn created_profile(world: &World) -> Option<(String, String)> {
    let global = world.resource::<GlobalEnvironmentAuthoringState>();
    let mut profiles = global
        .draft
        .library
        .profiles
        .iter()
        .filter(|(id, _)| id.as_str() != DEFAULT_ENVIRONMENT_PROFILE_ID);
    let (id, profile) = profiles.next()?;
    // 隔离夹具只有一份隐式默认。验收必须点击创建一份新方案后才取得其实际身份。
    profiles
        .next()
        .is_none()
        .then(|| (id.clone(), profile.name.clone()))
}

fn project_default(world: &World, id: &str) -> bool {
    let global = world.resource::<GlobalEnvironmentAuthoringState>();
    global.dirty && global.draft.library.default_profile_id == id
}

fn scene_references(world: &World, id: &str) -> bool {
    let session = world.resource::<EditorAuthoringSession>();
    session.draft.as_ref().is_some_and(|draft| {
        draft.environment_dirty
            && matches!(&draft.manifest.environment.source, SceneEnvironmentSource::Shared(active) if active == id)
    })
}

fn screenshot_ready(world: &mut World) -> Option<Result<(), String>> {
    world
        .get_resource_mut::<StartupScreenshot>()
        .and_then(|mut screenshot| check_ready(&mut screenshot.0))
}

pub(super) fn advance(world: &mut World, step: &mut Step) -> Result<Option<Value>, String> {
    let expected = created_profile(world);
    let (label, next) = match step.phase {
        Phase::Tool => ("环境", Phase::Library),
        Phase::Library => ("全局环境库", Phase::Create),
        Phase::Create => ("新建方案", Phase::SetDefault),
        Phase::SetDefault => {
            let Some((id, name)) = expected.as_ref() else {
                return Ok(None);
            };
            if step.frame == 0 && project_default(world, id) {
                step.phase = Phase::Scene;
            } else if (step.frame != 0 || source_space_button(world, name).is_some())
                && let Some(button) = source_space_button(world, "设为工程默认")
            {
                // 新方案行布局完成后再点击，等默认方案确实更新才继续验收。
                click(world, step, button, Phase::SetDefault)?;
            }
            return Ok(None);
        }
        Phase::Scene => ("场景环境", Phase::Choose),
        Phase::Choose => ("选择全局方案", Phase::Bind),
        Phase::Bind => {
            let Some((id, name)) = expected.as_ref() else {
                return Ok(None);
            };
            // 松手成功后界面会立即关闭选择列表，这时应按真实场景回执推进。
            if scene_references(world, id) {
                step.phase = Phase::Capture;
                return Ok(None);
            }
            let default_label = format!("{name} · 工程默认");
            let Some(button) = source_space_button(world, &default_label)
                .or_else(|| source_space_button(world, name))
            else {
                return Ok(None);
            };
            click(world, step, button, Phase::Capture)?;
            return Ok(None);
        }
        Phase::Capture | Phase::Screenshot => {
            let Some((id, name)) = expected else {
                return Ok(None);
            };
            if !scene_references(world, &id) || !project_default(world, &id) {
                return Ok(None);
            }
            if step.phase == Phase::Capture {
                request_screenshot(world, SCREENSHOT)?;
                step.phase = Phase::Screenshot;
            } else if let Some(result) = screenshot_ready(world) {
                result?;
                world.remove_resource::<StartupScreenshot>();
                return Ok(Some(json!({"startup": {"environment": {
                    "profile_id": id, "profile_name": name, "scene_source": "shared", "screenshot": SCREENSHOT
                }}})));
            }
            return Ok(None);
        }
    };
    if let Some(button) = source_space_button(world, label) {
        click(world, step, button, next)?;
    }
    Ok(None)
}

fn click(world: &mut World, step: &mut Step, button: Entity, next: Phase) -> Result<(), String> {
    if let Some(frame) = click_control(world, button, step.frame)? {
        step.frame = frame;
    } else {
        *step = Step {
            phase: next,
            frame: 0,
        };
    }
    Ok(())
}
