//! 仅显式验收启动，用独占工程驱动原生菜单、表单与保存事务，再开新进程复核。

use super::{ProjectSettingsSaved, components::{Action, Field, SettingsIcon, SettingsStatus}};
use crate::{
    build_process::{self, BuildControl},
    project::{self, Project, metadata::{self, Edit, IconEdit}},
    project_browser_validation::write_new,
    window_chrome::ChromeSlot,
};
use bevy::{
    ecs::message::MessageCursor, prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    text::{EditableText, TextReadWriteMode}, ui::InteractionDisabled,
    window::PrimaryWindow,
};
use feathrise_editor_core::{EditorLocalePreference, EditorSettings};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File}, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf},
    process::Command, time::{Duration, Instant},
};

const ENV: &str = "FEATHRISE_PROJECT_SETTINGS_VALIDATION";
const OWNER: &str = ".project-settings-validation-owner.json";
const INITIAL: &str = "工程设置验收";
const NAME: &str = "羽立 · 正式工程名称";
const DESCRIPTION: &str = "从工程设置保存的简介\n保留中文和换行。";
const SCREENSHOT: &str = "project-settings-saved.png";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Edit, Reopen }

impl Mode {
    fn directory(self) -> &'static str {
        match self { Self::Edit => "edit", Self::Reopen => "reopen" }
    }
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    write_new(path, &serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?)
}

fn owner(work: &Path, project: &Project) -> Value {
    json!({"schema":1,"purpose":"owned-project-settings-validation","work":work,
        "root":project.root,"id":project.id})
}

pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    if !report.is_absolute() || report.exists() { return Err("工程设置验收须使用本轮新的绝对报告路径".into()); }
    fs::create_dir_all(report.parent().ok_or("工程设置验收报告缺少父目录")?)
        .map_err(|error| error.to_string())?;
    let work = report.with_extension("work");
    fs::create_dir(&work).map_err(|error| format!("工程设置验收工作目录须独占：{error}"))?;
    let work = work.canonicalize().map_err(|error| error.to_string())?;
    let mut proof = json!({"schema":1,"work":work,"os_keyboard_and_file_dialog_verified":false});
    let result = exercise(&target, &work, &mut proof);
    proof["passed"] = json!(result.is_ok());
    proof["error"] = json!(result.as_ref().err());
    write_json(&report, &proof)?;
    result
}

fn exercise(target: &Path, work: &Path, proof: &mut Value) -> Result<(), String> {
    let root = crate::project_template::create_with_control(
        &work.join("project"), INITIAL, &crate::project_template_root(), &BuildControl::default(),
    )?;
    let project = project::open(&root)?;
    let seed = work.join("preexisting-icon.png");
    write_new(&seed, include_bytes!("../../assets/icons/editor/tree_water.png"))?;
    let initial = metadata::read(&project)?;
    let (_, seeded) = metadata::save(&project, &initial, Edit {
        name: INITIAL.into(), description: String::new(), icon: IconEdit::Import(seed),
    })?;
    let icon = seeded.metadata.icon.ok_or("夹具没有已保存的工程图标")?;
    write_json(&work.join(OWNER), &owner(work, &project))?;
    child(target, work, &project, Mode::Edit, proof)?;

    let reopened = project::open(&root)?;
    let disk = metadata::read(&reopened)?.metadata;
    if reopened.id != project.id || reopened.context.asset_roots != project.context.asset_roots
        || reopened.name != NAME || disk.name != NAME || disk.description != DESCRIPTION || disk.icon.is_some()
        || !root.join(&icon).is_file()
    {
        return Err("重新读取工程清单时，名称、简介、图标或原工程身份与验收手势不一致".into());
    }
    let screenshot = work.join(Mode::Edit.directory()).join(SCREENSHOT);
    let png = fs::read(&screenshot).map_err(|error| error.to_string())?;
    proof["disk"] = json!({"root":root,"id":reopened.id,"name":disk.name,"description":disk.description,
        "default_icon":true,"retained_icon_asset":icon});
    let sha256: String = Sha256::digest(&png).iter().map(|byte| format!("{byte:02x}")).collect();
    proof["screenshot"] = json!({"path":screenshot,"sha256":sha256});
    child(target, work, &reopened, Mode::Reopen, proof)
}

fn child(target: &Path, work: &Path, project: &Project, mode: Mode, proof: &mut Value) -> Result<(), String> {
    let output = work.join(mode.directory());
    fs::create_dir(&output).map_err(|error| error.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.contains("_VALIDATION") || name.contains("_SMOKE_") || name.starts_with("FEATHRISE_WORKSPACE_") {
            command.env_remove(key);
        }
    }
    command.args(["--native-editor", "--project"]).arg(&project.root)
        .arg("--target-dir").arg(target).env(ENV, &output);
    crate::native_validation::isolate_author_command(&mut command, &output)?;
    let result = build_process::run(&command, &BuildControl::default(), Some(Duration::from_secs(135)), tail)?;
    write_new(&output.join("process.log"), format!("{}\n{}", result.stdout, result.stderr).as_bytes())?;
    let mut window = read_json(&output.join("window.json"))?;
    let passed = result.exit_code == 0 && window["passed"] == true;
    window["exit_code"] = json!(result.exit_code);
    proof[mode.directory()] = window;
    if !passed { return Err(format!("工程设置原生窗口阶段 {} 失败", mode.directory())); }
    Ok(())
}

fn tail(mut file: File) -> Result<String, String> {
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(128 * 1024))).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let mut bytes = Vec::new();
    File::open(path).and_then(|file| file.take(16 * 1024).read_to_end(&mut bytes))
        .map_err(|error| format!("无法读取工程设置验收证据 {}：{error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

pub(super) fn install(app: &mut App, project: &Project) {
    let Some(output) = std::env::var_os(ENV).map(PathBuf::from) else { return; };
    match preflight(project, output) {
        Ok(plan) => {
            app.insert_resource(bevy::winit::WinitSettings::continuous())
                .insert_resource(plan).add_systems(Last, drive);
        }
        Err(error) => {
            eprintln!("{error}");
            app.world_mut().write_message(AppExit::error());
        }
    }
}

fn preflight(project: &Project, output: PathBuf) -> Result<Plan, String> {
    let work = output.parent().ok_or("工程设置验收没有工作目录")?;
    let mode = match output.file_name().and_then(|name| name.to_str()) {
        Some("edit") => Mode::Edit,
        Some("reopen") => Mode::Reopen,
        _ => return Err("工程设置验收阶段不明".into()),
    };
    if !output.is_absolute() || output.canonicalize().ok().as_ref() != Some(&output)
        || project.root != work.join("project") || project.context.asset_roots != [project.root.join("assets")]
        || read_json(&work.join(OWNER))? != owner(work, project)
        || output.join("window.json").exists() || output.join(SCREENSHOT).exists()
        || std::env::var_os("FEATHRISE_PREFERENCES_FILE").as_deref()
            != Some(output.join("isolated-preferences.json").as_os_str())
        || std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").as_deref()
            != Some(output.join("isolated-global-plugins").as_os_str())
        || fs::read(output.join("isolated-global-plugins/.feathrise-plugin-fixture-owner")).ok().as_deref()
            != Some(b"feathrise-single-binary-validation-v1")
    {
        return Err("工程设置验收工程或偏好与插件隔离未通过归属检查".into());
    }
    Ok(Plan { output, mode, project: project.clone(), stage: Stage::Window, pulse: None,
        started: Instant::now(), receipt: false, done: false })
}

#[derive(Clone, Copy, Debug)]
enum Stage { Window, Menu, Fields, Save, Receipt, Capture }

#[derive(Resource)]
struct Plan {
    output: PathBuf,
    mode: Mode,
    project: Project,
    stage: Stage,
    pulse: Option<Entity>,
    started: Instant,
    receipt: bool,
    done: bool,
}

fn drive(world: &mut World, mut saves: Local<MessageCursor<ProjectSettingsSaved>>) {
    world.resource_scope(|world, mut plan: Mut<Plan>| {
        if plan.done { return; }
        if let Some(entity) = plan.pulse.take() && let Ok(mut button) = world.get_entity_mut(entity) {
            button.insert(EditorButtonClick::default());
        }
        let saved = saves.read(world.resource::<Messages<ProjectSettingsSaved>>()).any(|event| {
            event.project.id == plan.project.id && event.project.root == plan.project.root
                && event.project.name == NAME
        });
        plan.receipt |= saved;
        let result = if plan.started.elapsed() > Duration::from_secs(100) {
            Err(format!("工程设置窗口等待 {:?} 超时", plan.stage))
        } else { step(world, &mut plan) };
        if matches!(result, Ok(false)) { return; }
        let window = json!({"passed":result.is_ok(),"stage":format!("{:?}",plan.stage),
            "error":result.as_ref().err(),"saved_receipt":plan.receipt,"mode":plan.mode.directory()});
        let written = write_json(&plan.output.join("window.json"), &window);
        plan.done = true;
        if let Err(error) = &result { eprintln!("{error}"); }
        if let Err(error) = &written { eprintln!("{error}"); }
        world.write_message(if result.is_ok() && written.is_ok() { AppExit::Success } else { AppExit::error() });
    });
}

fn step(world: &mut World, plan: &mut Plan) -> Result<bool, String> {
    plan.stage = match plan.stage {
        Stage::Window => {
            let mut locale = world.resource_mut::<EditorSettings>();
            if locale.ui_locale != EditorLocalePreference::Chinese {
                locale.ui_locale = EditorLocalePreference::Chinese;
                return Ok(false);
            }
            let menu = world.query::<(Entity, &ChromeSlot)>().iter(world)
                .find_map(|(entity, slot)| (matches!(slot, ChromeSlot::Project) && available(world, entity)).then_some(entity));
            let Some(menu) = menu else { return Ok(false); };
            pulse(world, plan, menu);
            Stage::Menu
        }
        Stage::Menu => {
            let (Some(settings), Some(export)) = (menu_button(world, "工程设置"), menu_button(world, "导出工程…"))
                else { return Ok(false); };
            let parent = world.get::<ChildOf>(settings).map(ChildOf::parent).and_then(|parent| world.get::<Children>(parent))
                .ok_or("工程设置缺少菜单容器")?;
            if !parent.windows(2).any(|pair| pair == [settings, export]) {
                return Err("工程设置应位于同一工程菜单的导出工程正上方".into());
            }
            pulse(world, plan, settings);
            Stage::Fields
        }
        Stage::Fields => {
            let [Some(name), Some(description)] = editable_fields(world) else { return Ok(false); };
            let text = |entity| world.get::<EditableText>(entity).map(|text| text.value().to_string());
            if plan.mode == Mode::Reopen {
                if text(name).as_deref() != Some(NAME) || text(description).as_deref() != Some(DESCRIPTION)
                    || icon_visible(world) || !updated_title(world)
                {
                    return Err("新进程打开工程设置时未还原已保存的名称、简介和默认图标".into());
                }
                return Ok(true);
            }
            if text(name).as_deref() != Some(INITIAL) { return Err("原工程名称未加载到表单".into()); }
            let Some(reset) = action(world, Action::ResetIcon).filter(|_| icon_visible(world)) else { return Ok(false); };
            // 输入写入真实表单组件，后续采用、校验和写入都由生产系统处理。
            world.get_mut::<EditableText>(name).unwrap().editor.set_text(NAME);
            world.get_mut::<EditableText>(description).unwrap().editor.set_text(DESCRIPTION);
            pulse(world, plan, reset);
            Stage::Save
        }
        Stage::Save => {
            let Some(save) = action(world, Action::Save).filter(|_| !icon_visible(world)) else { return Ok(false); };
            pulse(world, plan, save);
            Stage::Receipt
        }
        Stage::Receipt => {
            let status = world.query_filtered::<&Text, With<SettingsStatus>>().iter(world)
                .any(|status| status.as_str() == "工程设置已保存");
            if !plan.receipt || !status || !updated_title(world) { return Ok(false); }
            world.spawn(Screenshot::primary_window()).observe(save_to_disk(plan.output.join(SCREENSHOT)));
            Stage::Capture
        }
        Stage::Capture => {
            let Ok(png) = fs::read(plan.output.join(SCREENSHOT)) else { return Ok(false); };
            return Ok(png.starts_with(b"\x89PNG\r\n\x1a\n") && png.ends_with(b"\0\0\0\0IEND\xaeB\x60\x82"));
        }
    };
    Ok(false)
}

fn pulse(world: &mut World, plan: &mut Plan, button: Entity) {
    world.entity_mut(button).insert(EditorButtonClick::triggered());
    plan.pulse = Some(button);
}

fn visible(world: &World, entity: Entity) -> bool {
    let mut current = Some(entity);
    while let Some(entity) = current {
        if world.get::<Node>(entity).is_some_and(|node| node.display == Display::None)
            || world.get::<InheritedVisibility>(entity).is_some_and(|visibility| !visibility.get()) { return false; }
        current = world.get::<ChildOf>(entity).map(ChildOf::parent);
    }
    world.get::<ComputedNode>(entity).is_some_and(|node| node.size().min_element() > 2.)
}

fn available(world: &World, entity: Entity) -> bool {
    visible(world, entity) && world.get::<InteractionDisabled>(entity).is_none()
}

fn menu_button(world: &mut World, label: &str) -> Option<Entity> {
    let mut query = world.query::<(Entity, &Text)>();
    query.iter(world).find_map(|(mut entity, text)| {
        if text.as_str() != label { return None; }
        while world.get::<InteractionButton>(entity).is_none() {
            entity = world.get::<ChildOf>(entity)?.parent();
        }
        available(world, entity).then_some(entity)
    })
}

fn action(world: &mut World, wanted: Action) -> Option<Entity> {
    world.query::<(Entity, &Action)>().iter(world)
        .find_map(|(entity, action)| (*action == wanted && available(world, entity)).then_some(entity))
}

fn editable_fields(world: &mut World) -> [Option<Entity>; 2] {
    let mut fields = [None; 2];
    for (entity, field, mode) in world.query::<(Entity, &Field, &TextReadWriteMode)>().iter(world) {
        if matches!(mode, TextReadWriteMode::Editable) && visible(world, entity) {
            fields[match field { Field::Name => 0, Field::Description => 1 }] = Some(entity);
        }
    }
    fields
}

fn icon_visible(world: &mut World) -> bool {
    world.query_filtered::<Entity, With<SettingsIcon>>().iter(world).any(|entity| visible(world, entity))
}

fn updated_title(world: &mut World) -> bool {
    world.query_filtered::<&Window, With<PrimaryWindow>>().iter(world)
        .any(|window| window.title == format!("{NAME} · Feathrise（羽立）"))
}
