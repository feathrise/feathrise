//! 自有工程环境验收。原控件修改全局和独立配方，引用与保存仍交给生产事务。

use bevy::{input::{ButtonState, keyboard::Key}, prelude::*, tasks::futures::check_ready, ui::RelativeCursorPosition};
use feathrise_atmosphere::{WeatherDebugOverride, WorldClock};
use feathrise_environment::vegetation::blade::GpuBladeInteraction;
use feathrise_scene_content::{
    ActiveEnvironmentSimulation, DEFAULT_ENVIRONMENT_PROFILE_ID, EnvironmentSimulation, SceneEnvironmentSource, SceneId,
    WorldEnvironmentRegistry, resolve_scene_environment,
};
use serde_json::{Value, json};

use super::{StartupScreenshot, click_control, pointer_context, request_screenshot, source_space_button, ui_visible};
use crate::{
    authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState},
    standalone::validation_fixture::key_state,
    ui::file_manager_validation::{mouse, move_cursor, pointer_click},
    world_environment_controls::WorldEnvironmentControlsSlot,
};

const SCREENSHOT: &str = "scene-tree-environment.png";
const LOCAL_SCREENSHOT: &str = "scene-tree-environment-local.png";
const GLOBAL_SLIDERS: [(&str, f32); 5] = [
    ("起始时间", 0.25), ("起始月份", 0.75), ("天气模式", 0.6),
    ("过渡进度", 0.5), ("草尖摆幅", 1.0),
];
const LOCAL_SLIDERS: [(&str, f32); 2] = [("起始时间", 0.75), ("草尖摆幅", 0.0)];

pub(in crate::standalone) fn enabled() -> bool {
    std::env::var("FEATHRISE_SCENE_TREE_STARTUP_VALIDATION").as_deref() == Ok("environment")
}

fn global_matches(value: &EnvironmentSimulation) -> bool {
    (value.hour - 6.0).abs() < 0.03 && value.month == 9 && value.day == 15 && value.paused
        && value.weather_mode_index == 3 && (value.weather_transition - 0.5).abs() < 0.01
        && (value.grass_wind_tip_strength - 0.35).abs() < 0.001
}

fn local_matches(value: &EnvironmentSimulation) -> bool {
    (value.hour - 18.0).abs() < 0.03 && value.month == 9 && value.day == 15 && value.paused
        && value.weather_mode_index == 3 && (value.weather_transition - 0.5).abs() < 0.01
        && value.grass_wind_tip_strength.abs() < 0.001
}

fn runtime_matches(world: &World, value: &EnvironmentSimulation) -> bool {
    // 解析结果之外再读正式消费者；暂停时刻可稳定核对，零摆幅必须抵达GPU风参数。
    world.get_resource::<ActiveEnvironmentSimulation>()
        .is_some_and(|active| active.configuration.as_ref() == Some(value))
        && world.get_resource::<WorldClock>().is_some_and(|clock|
            (clock.hour_for_display() - value.hour).abs() < 0.03
                && clock.month() == value.month && clock.day() == value.day)
        && world.get_resource::<WeatherDebugOverride>().is_some_and(|weather|
            weather.mode_index() == usize::from(value.weather_mode_index)
                && (weather.preview() - value.weather_transition).abs() < 0.01)
        && (value.grass_wind_tip_strength != 0.0
            || world.get_resource::<GpuBladeInteraction>()
                .is_some_and(|wind| wind.wind_tip_strength() == 0.0))
}

fn controls_hidden(world: &mut World) -> bool {
    world.query_filtered::<&ComputedNode, With<WorldEnvironmentControlsSlot>>().single(world)
        .is_ok_and(ComputedNode::is_empty)
}

pub(super) fn reopened(world: &mut World, scene: &SceneId) -> Option<Value> {
    let draft = world.resource::<EditorAuthoringSession>().draft.as_ref()?;
    if &draft.scene_id != scene || draft.environment_dirty { return None; }
    let environment = &draft.manifest.environment;
    let SceneEnvironmentSource::Shared(id) = &environment.source else { return None; };
    let registry = world.resource::<WorldEnvironmentRegistry>();
    let profile = registry.library.profiles.get(id)?;
    let resolved = resolve_scene_environment(environment, &registry.celestial_lighting, &registry.library);
    if registry.revision == 0 || registry.last_error.is_some()
        || &registry.library.default_profile_id != id || resolved.profile_id.as_ref() != Some(id)
        || resolved.missing_profile.is_some() || !global_matches(&profile.appearance.simulation)
        || resolved.environment.simulation.as_ref() != Some(&profile.appearance.simulation)
        || !environment.simulation.as_ref().is_some_and(local_matches)
    { return None; }
    let (id, name, simulation, local) = (id.clone(), profile.name.clone(),
        profile.appearance.simulation, environment.simulation);
    let source_text = format!("环境来源  共享方案 · {name}");
    let visible = world.query::<(Entity, &Text)>().iter(world).any(|(entity, text)| {
        text.0.lines().next() == Some(source_text.as_str()) && ui_visible(world, entity)
    });
    if !visible {
        if let Some(tab) = named_button(world, "场景环境").or_else(|| named_button(world, "环境")) {
            world.entity_mut(tab).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        }
        return None;
    }
    if !controls_hidden(world) || !runtime_matches(world, &simulation) { return None; }
    Some(json!({"profile_id": id, "profile_name": name, "scene_source": "shared",
        "visible_source": source_text, "simulation": simulation, "local_simulation": local,
        "shared_controls_hidden": true, "runtime_adopted": true}))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::standalone) struct Step { phase: Phase, frame: u8, button: Option<Entity> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Tool, Library, Create, SetDefault, Pause, Global(usize), GlobalDone, Scene, Choose,
    Bind, Shared, MakeLocal, LocalCopy, Local(usize), DragSave(DragSave), UndoLocal, UndoneLocal, RedoLocal, LocalDone,
    LocalScreenshot, Inherit, Default, RestoreLocal, RestoredLocal, InheritAgain, DefaultAgain,
    ChooseAgain, BindAgain, Capture, Screenshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DragSave { frame: u8, save_before: u64, revision_before: u64, undo_before: usize }

impl Step {
    pub(super) fn begin() -> Self { Self { phase: Phase::Tool, frame: 0, button: None } }
    fn next(&mut self, phase: Phase) { *self = Self { phase, frame: 0, button: None }; }
}

fn created_profile(world: &World) -> Option<(String, String)> {
    let global = world.resource::<GlobalEnvironmentAuthoringState>();
    let mut profiles = global.draft.library.profiles.iter()
        .filter(|(id, _)| id.as_str() != DEFAULT_ENVIRONMENT_PROFILE_ID);
    let (id, profile) = profiles.next()?;
    // 独占夹具只允许原按钮新建的一套方案，不能认领历史工程或已存在的配方。
    profiles.next().is_none().then(|| (id.clone(), profile.name.clone()))
}

fn profile_simulation(world: &World, id: &str) -> Option<EnvironmentSimulation> {
    world.resource::<GlobalEnvironmentAuthoringState>().draft.library.profiles.get(id)
        .map(|profile| profile.appearance.simulation)
}

fn scene_simulation(world: &World) -> Option<EnvironmentSimulation> {
    world.resource::<EditorAuthoringSession>().draft.as_ref()?.manifest.environment.simulation
}

fn project_default(world: &World, id: &str) -> bool {
    let global = world.resource::<GlobalEnvironmentAuthoringState>();
    global.draft.library.default_profile_id == id
}

fn scene_references(world: &World, id: &str) -> bool {
    world.resource::<EditorAuthoringSession>().draft.as_ref().is_some_and(|draft| {
        draft.environment_dirty && matches!(&draft.manifest.environment.source,
            SceneEnvironmentSource::Shared(active) if active == id)
    })
}

fn resolved_simulation(world: &World) -> Option<EnvironmentSimulation> {
    let session = world.resource::<EditorAuthoringSession>();
    let global = world.resource::<GlobalEnvironmentAuthoringState>();
    resolve_scene_environment(&session.draft.as_ref()?.manifest.environment,
        &global.draft.celestial_lighting, &global.draft.library).environment.simulation
}

pub(super) fn advance(world: &mut World, step: &mut Step) -> Result<Option<Value>, String> {
    let expected = created_profile(world);
    let (label, next) = match step.phase {
        Phase::Tool => ("环境", Phase::Library),
        Phase::Library => ("全局环境库", Phase::Create),
        Phase::Create => ("新建方案", Phase::SetDefault),
        Phase::SetDefault => {
            let Some((id, _)) = expected.as_ref() else { return Ok(None); };
            if step.frame == 0 && project_default(world, id) { step.next(Phase::Pause); }
            else if let Some(button) = named_button(world, "设为工程默认") {
                click(world, step, button, Phase::SetDefault)?;
            }
            return Ok(None);
        }
        Phase::Pause => {
            let Some((id, _)) = expected.as_ref() else { return Ok(None); };
            if step.frame == 0 && profile_simulation(world, id).is_some_and(|value| value.paused) {
                step.next(Phase::Global(0));
            } else if let Some(button) = named_button(world, "时间暂停：关")
                .or_else(|| named_button(world, "时间暂停：开"))
            { click(world, step, button, Phase::Pause)?; }
            return Ok(None);
        }
        Phase::Global(index) => {
            if index == GLOBAL_SLIDERS.len() { step.next(Phase::GlobalDone); }
            else { slider(world, step, GLOBAL_SLIDERS[index], Phase::Global(index + 1))?; }
            return Ok(None);
        }
        Phase::GlobalDone => {
            let Some((id, _)) = expected.as_ref() else { return Ok(None); };
            if !profile_simulation(world, id).as_ref().is_some_and(global_matches) {
                return Err("原滑块未将时间、月份、天气、过渡和草尖摆幅写入全局方案".into());
            }
            step.next(Phase::Scene);
            return Ok(None);
        }
        Phase::Scene => ("场景环境", Phase::Choose),
        Phase::Choose => ("选择全局方案", Phase::Bind),
        Phase::ChooseAgain => ("选择全局方案", Phase::BindAgain),
        Phase::Bind | Phase::BindAgain => {
            let Some((id, name)) = expected.as_ref() else { return Ok(None); };
            let next = if matches!(step.phase, Phase::BindAgain) { Phase::Capture } else { Phase::Shared };
            if scene_references(world, id) { step.next(next); return Ok(None); }
            if let Some(button) = named_button(world, &format!("{name} · 工程默认"))
                .or_else(|| named_button(world, name))
            { click(world, step, button, next)?; }
            return Ok(None);
        }
        Phase::Shared => {
            if !controls_hidden(world) { return Ok(None); }
            if !resolved_simulation(world).as_ref().is_some_and(global_matches) {
                return Err("共享场景没有采用全局方案的完整模拟配置".into());
            }
            if !resolved_simulation(world).as_ref().is_some_and(|value| runtime_matches(world, value)) {
                return Ok(None);
            }
            step.next(Phase::MakeLocal);
            return Ok(None);
        }
        Phase::MakeLocal => ("创建场景独立副本", Phase::LocalCopy),
        Phase::LocalCopy => {
            let local = world.resource::<EditorAuthoringSession>().draft.as_ref().is_some_and(|draft|
                draft.manifest.environment.source == SceneEnvironmentSource::Local);
            if !local { return Ok(None); }
            if !scene_simulation(world).as_ref().is_some_and(global_matches) {
                return Err("制作场景独立副本没有复制全局时间、天气和草尖摆幅".into());
            }
            step.next(Phase::Local(0));
            return Ok(None);
        }
        Phase::Local(index) => {
            if index == LOCAL_SLIDERS.len() { step.next(Phase::UndoLocal); }
            else if index == 0 {
                let session = world.resource::<EditorAuthoringSession>();
                let probe = DragSave { frame: 0, save_before: session.successful_save_sequence,
                    revision_before: session.revision, undo_before: session.undo.len() };
                step.next(Phase::DragSave(probe));
            }
            else { slider(world, step, LOCAL_SLIDERS[index], Phase::Local(index + 1))?; }
            return Ok(None);
        }
        Phase::DragSave(mut probe) => {
            if drag_save(world, &mut probe)? { step.next(Phase::Local(1)); }
            else { step.phase = Phase::DragSave(probe); }
            return Ok(None);
        }
        Phase::UndoLocal => ("撤销", Phase::UndoneLocal),
        Phase::UndoneLocal => {
            let Some(value) = scene_simulation(world) else { return Ok(None); };
            let expected = EnvironmentSimulation { grass_wind_tip_strength: 0.35, ..value };
            if value != expected || !local_matches(&EnvironmentSimulation {
                grass_wind_tip_strength: 0.0, ..value
            }) { return Err("撤销没有单独恢复最后一次草尖摆幅拖动".into()); }
            if !runtime_matches(world, &value) { return Ok(None); }
            step.next(Phase::RedoLocal);
            return Ok(None);
        }
        Phase::RedoLocal => ("重做", Phase::LocalDone),
        Phase::LocalDone => {
            let Some((id, _)) = expected.as_ref() else { return Ok(None); };
            if !scene_simulation(world).as_ref().is_some_and(local_matches)
                || !profile_simulation(world, id).as_ref().is_some_and(global_matches)
            { return Err("场景独立模拟配置未生效，或反向改写了全局方案".into()); }
            if !scene_simulation(world).as_ref().is_some_and(|value| runtime_matches(world, value)) {
                return Ok(None);
            }
            request_screenshot(world, LOCAL_SCREENSHOT)?;
            step.next(Phase::LocalScreenshot);
            return Ok(None);
        }
        Phase::LocalScreenshot => {
            if let Some(result) = world.get_resource_mut::<StartupScreenshot>()
                .and_then(|mut screenshot| check_ready(&mut screenshot.0))
            {
                result?;
                world.remove_resource::<StartupScreenshot>();
                step.next(Phase::Inherit);
            }
            return Ok(None);
        }
        Phase::Inherit => ("继承工程默认", Phase::Default),
        Phase::InheritAgain => ("继承工程默认", Phase::DefaultAgain),
        Phase::Default | Phase::DefaultAgain => {
            let inherited = world.resource::<EditorAuthoringSession>().draft.as_ref().is_some_and(|draft|
                draft.manifest.environment.source == SceneEnvironmentSource::InheritDefault);
            if !inherited || !controls_hidden(world) { return Ok(None); }
            if !resolved_simulation(world).as_ref().is_some_and(global_matches)
                || !scene_simulation(world).as_ref().is_some_and(local_matches)
            { return Err("切回工程默认没有恢复全局模拟配置，或丢失了场景独立副本".into()); }
            if !resolved_simulation(world).as_ref().is_some_and(|value| runtime_matches(world, value)) {
                return Ok(None);
            }
            step.next(if step.phase == Phase::Default { Phase::RestoreLocal } else { Phase::ChooseAgain });
            return Ok(None);
        }
        Phase::RestoreLocal => ("创建场景独立副本", Phase::RestoredLocal),
        Phase::RestoredLocal => {
            let local = world.resource::<EditorAuthoringSession>().draft.as_ref().is_some_and(|draft|
                draft.manifest.environment.source == SceneEnvironmentSource::Local);
            if !local { return Ok(None); }
            if !scene_simulation(world).as_ref().is_some_and(local_matches) {
                return Err("重新独立时没有恢复已保存的场景配方".into());
            }
            if !scene_simulation(world).as_ref().is_some_and(|value| runtime_matches(world, value)) {
                return Ok(None);
            }
            step.next(Phase::InheritAgain);
            return Ok(None);
        }
        Phase::Capture | Phase::Screenshot => {
            let Some((id, name)) = expected else { return Ok(None); };
            if !scene_references(world, &id) || !project_default(world, &id) || !controls_hidden(world) {
                return Ok(None);
            }
            let global = profile_simulation(world, &id).ok_or("环境方案已失效")?;
            let local = scene_simulation(world).ok_or("场景独立副本已丢失")?;
            if !global_matches(&global) || !local_matches(&local)
                || resolved_simulation(world) != Some(global)
            { return Err("保存前的共享引用没有恢复完整全局方案".into()); }
            if step.phase == Phase::Capture {
                if named_button(world, "场景环境").is_none() { return Ok(None); }
                request_screenshot(world, SCREENSHOT)?;
                step.next(Phase::Screenshot);
            } else if let Some(result) = world.get_resource_mut::<StartupScreenshot>()
                .and_then(|mut screenshot| check_ready(&mut screenshot.0))
            {
                result?;
                world.remove_resource::<StartupScreenshot>();
                return Ok(Some(json!({"startup": {"environment": {
                    "profile_id": id, "profile_name": name, "scene_source": "shared",
                    "simulation": global, "local_simulation": local, "inherited_default_verified": true,
                    "shared_controls_hidden": true, "runtime_adopted": true,
                    "local_zero_wind_adopted": true, "undo_redo_verified": true,
                    "drag_save_verified": true,
                    "local_restored": true, "local_screenshot": LOCAL_SCREENSHOT, "screenshot": SCREENSHOT
                }}})));
            }
            return Ok(None);
        }
    };
    // 重做在释放帧执行后会禁用原按钮，仍须用按下时的实体完成同一次释放协议。
    if let Some(button) = step.button.or_else(|| named_button(world, label)) {
        click(world, step, button, next)?;
    }
    Ok(None)
}

fn named_button(world: &mut World, label: &str) -> Option<Entity> {
    if let Some(button) = source_space_button(world, label) { return Some(button); }
    let candidate = world.query::<(Entity, &Text, &ComputedNode)>().iter(world)
        .find_map(|(entity, text, node)| (text.0 == label && !node.is_empty()).then_some(entity))?;
    let button = std::iter::successors(Some(candidate), |entity|
        world.get::<ChildOf>(*entity).map(ChildOf::parent))
        .find(|entity| world.get::<feathrise_editor_ui::EditorButtonClick>(*entity).is_some())?;
    crate::standalone::validation_fixture::reveal_property_control(world, button).ok()?;
    None
}

fn slider_pointer(world: &mut World, (label, ratio): (&str, f32)) -> Result<Option<(Entity, bevy::math::DVec2)>, String> {
    let label = world.query::<(Entity, &Text, &ComputedNode)>().iter(world)
        .find_map(|(entity, text, node)| (text.0 == label && !node.is_empty()).then_some(entity));
    let Some(label) = label else { return Ok(None); };
    let block = world.get::<ChildOf>(label).and_then(|row| world.get::<ChildOf>(row.parent()))
        .map(ChildOf::parent).ok_or("模拟参数标签没有对应的滑块块")?;
    let track = world.get::<Children>(block).into_iter().flat_map(|children| children.iter())
        .find(|entity| world.get::<RelativeCursorPosition>(*entity).is_some())
        .ok_or("模拟参数缺少原滑块轨道")?;
    if !ui_visible(world, track) {
        crate::standalone::validation_fixture::reveal_property_control(world, track)?;
        return Ok(None);
    }
    let (window, mut position) = pointer_context(world, track)?;
    let width = world.get::<ComputedNode>(track).ok_or("模拟滑块尚未布局")?.size().x;
    // 与正式控件相同的260/14轨道映射，通过真实按压和松开提交草稿及单条历史。
    position.x += f64::from(((7.0 + 246.0 * ratio) / 260.0 - 0.5) * width);
    Ok(Some((window, position)))
}

fn slider(world: &mut World, step: &mut Step, slider: (&str, f32), next: Phase) -> Result<(), String> {
    let Some((window, position)) = slider_pointer(world, slider)? else { return Ok(()); };
    let mut pending = Some((position, step.frame));
    pointer_click(world, &mut pending, window, position, MouseButton::Left);
    if let Some((_, frame)) = pending { step.frame = frame; } else { step.next(next); }
    Ok(())
}

fn drag_save(world: &mut World, probe: &mut DragSave) -> Result<bool, String> {
    let ratio = if probe.frame == 6 { 0.25 } else { 0.75 };
    let Some((window, position)) = slider_pointer(world, ("起始时间", ratio))? else { return Ok(false); };
    move_cursor(world, window, position);
    let session = world.resource::<EditorAuthoringSession>();
    if probe.frame >= 2 {
        let value = scene_simulation(world).ok_or("拖动保存验收缺少场景独立参数")?;
        let hour = if probe.frame == 7 { 6.0 } else { 18.0 };
        if (value.hour - hour).abs() > 0.03 || !global_matches(&EnvironmentSimulation { hour: 6.0, ..value }) {
            return Err("拖动中请求保存后，场景参数被回滚或未继续响应原手势".into());
        }
    }
    if (5..=8).contains(&probe.frame)
        && (session.saving || session.successful_save_sequence != probe.save_before
            || session.revision != probe.revision_before || session.undo.len() != probe.undo_before
            || !world.resource::<crate::resources::SceneEditorState>().save_requested)
    { return Err("环境滑块尚未松手时保存请求未保留，或提前冻结了未收口的草稿".into()); }
    match probe.frame {
        0 => {}
        1 => mouse(world, window, ButtonState::Pressed),
        2 => {
            if session.undo.len() != probe.undo_before { return Err("时间滑块尚未松手就提前生成历史".into()); }
            key_state(world, window, KeyCode::ControlLeft, Key::Control, ButtonState::Pressed);
        }
        3 => key_state(world, window, KeyCode::KeyS, Key::Character("s".into()), ButtonState::Pressed),
        4 => {
            key_state(world, window, KeyCode::KeyS, Key::Character("s".into()), ButtonState::Released);
            key_state(world, window, KeyCode::ControlLeft, Key::Control, ButtonState::Released);
        }
        5 | 6 | 7 => {} // 持续按住，移到六点再回十八点，证明保存等待期间仍是同一笔可编辑拖动。
        8 => mouse(world, window, ButtonState::Released),
        _ => {
            // 松手后只认原保存成功回执；等待帧数或输入注入都不能冒充落盘成功。
            if session.successful_save_sequence <= probe.save_before || session.saving { return Ok(false); }
            if session.revision != probe.revision_before.wrapping_add(1)
                || session.draft.as_ref().is_none_or(|draft| draft.environment_dirty)
                || world.resource::<GlobalEnvironmentAuthoringState>().dirty
            { return Err("拖动保存未形成单笔历史，或保存回执后仍有环境脏状态".into()); }
            return Ok(!world.resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        }
    }
    probe.frame += 1;
    Ok(false)
}

fn click(world: &mut World, step: &mut Step, button: Entity, next: Phase) -> Result<(), String> {
    step.button = Some(button);
    if let Some(frame) = click_control(world, button, step.frame)? { step.frame = frame; }
    else { step.next(next); }
    Ok(())
}
