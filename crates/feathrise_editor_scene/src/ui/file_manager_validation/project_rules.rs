//! 两个独立作者窗口从交互入口与工程文件列表打开表单，检验规则的正式编辑、保存与重开。
mod form;
use super::{click, evidence, fixture_focus, key_state, move_cursor, visible, window_event};
use crate::{
    asset_catalog::{EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::SceneEditorAssetRow,
    resources::SceneEditorState,
    ui::{
        asset_preview::DocumentState,
        gameplay_sidebar::project_rules::RuleButton,
        project_browser::{ProjectBrowserState, ProjectFileRow},
        sidebars::{
            EditorLeftSidebarTab, EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarTabButton,
        },
    },
};
use bevy::{
    app::AppExit,
    input::{ButtonState, keyboard::Key},
    input_focus::InputFocus,
    prelude::*,
    window::{Ime, PrimaryWindow},
};
use feathrise_editor_documents_model::{
    components::{DocumentAction, DocumentSource},
    project_rules::{EXAMPLE, PATH},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Sidebar,
    Open,
    Files,
    Loaded,
    Form(usize),
    Advanced,
    Edit(bool),
    Input(bool),
    Save(bool),
    Verify(bool),
    Capture,
    CaptureSource,
    Complete,
}

#[derive(Resource)]
struct Run {
    work: PathBuf,
    phase: String,
    stage: Stage,
    since: Instant,
    pointer: Option<(Entity, bool)>,
    keys: u8,
    saved_form: String,
    invalid: String,
    edited: String,
    diagnostic: Option<String>,
    capture: Option<evidence::CaptureTask>,
    form_shot: Value,
    source_shot: Value,
}

pub(super) fn install(app: &mut App, work: PathBuf, phase: String) -> Result<(), String> {
    app.insert_resource(Run {
        work,
        phase,
        stage: Stage::Sidebar,
        since: Instant::now(),
        pointer: None,
        keys: 0,
        saved_form: String::new(),
        invalid: String::new(),
        edited: String::new(),
        diagnostic: None,
        capture: None,
        form_shot: Value::Null,
        source_shot: Value::Null,
    })
    .add_systems(PreUpdate, hold_pointer.before(bevy::ui::UiSystems::Focus))
    .add_systems(
        Update,
        fixture_focus.before(crate::SceneEditorSystemSet::Input),
    )
    .add_systems(Last, drive);
    Ok(())
}

fn hold_pointer(world: &mut World) {
    let Some((entity, _)) = world.resource::<Run>().pointer else {
        return;
    };
    let Some(point) = world
        .get::<UiGlobalTransform>(entity)
        .map(|t| t.translation.as_dvec2())
    else {
        return;
    };
    if let Some(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
    {
        move_cursor(world, window, point);
    }
}

fn observe(world: &World) -> Option<(&str, bool, bool, &str)> {
    world
        .get_resource::<DocumentState>()?
        .project_rules_observed()
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        if run.stage == Stage::Complete {
            return;
        }
        let Some(window) = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .iter(world)
            .next()
        else {
            return;
        };
        let result = if run.since.elapsed() > Duration::from_secs(30) {
            let current =
                observe(world).map(|(text, dirty, edit, status)| (text.len(), dirty, edit, status));
            let form = if let Stage::Form(i) = run.stage {
                form::describe(run.phase == "reopen", i)
            } else {
                String::new()
            };
            let focus = world
                .get_resource::<InputFocus>()
                .and_then(|f| f.get())
                .and_then(|e| world.get::<Name>(e))
                .map(Name::as_str);
            Err(format!(
                "等待 {:?} {form} 超时，原生输入步骤 {}，焦点 {focus:?}，文档 {:?}，操作栏 {:?}",
                run.stage,
                run.keys,
                current,
                world.get_resource::<SceneEditorState>().map(|s| &s.status)
            ))
        } else {
            advance(world, &mut run, window)
        };
        match result {
            Ok(Some(next)) => {
                run.stage = next;
                run.since = Instant::now();
                run.keys = 0;
            }
            Ok(None) => {}
            Err(error) => finish(world, &mut run, Some(error)),
        }
    });
}

fn advance(world: &mut World, run: &mut Run, window: Entity) -> Result<Option<Stage>, String> {
    let next = match run.stage {
        Stage::Sidebar => {
            if !world
                .get_resource::<SceneEditorState>()
                .is_some_and(|e| e.open && e.pending_scene_transition.is_none())
            {
                return Ok(None);
            }
            let target = if run.phase == "reopen" {
                EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
            } else {
                EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay)
            };
            let tab = visible(world, |t: &SceneEditorSidebarTabButton| t.0 == target);
            if !click(world, &mut run.pointer, window, tab) {
                return Ok(None);
            }
            if run.phase == "reopen" {
                Stage::Files
            } else {
                Stage::Open
            }
        }
        Stage::Open => {
            let button = visible(world, |b: &RuleButton| b.0);
            if !click(world, &mut run.pointer, window, button) {
                return Ok(None);
            }
            Stage::Loaded
        }
        Stage::Files => {
            // 固定文件列表单击 assets 进入目录，再实际双击配置文件。这里不发送文档打开命令。
            if observe(world).is_some() {
                return Ok(Some(Stage::Loaded));
            }
            // 登记的 assets 目录采用后，文件列表改用资产行和相对资产根的路径。
            let row = if world.resource::<ProjectBrowserState>().assets_visible() {
                let assets = world.resource::<EditorAssetCatalog>();
                let index = assets.entries.iter().position(|item| item.asset_path == PATH);
                index.and_then(|i| visible(world, |r: &SceneEditorAssetRow| r.entry_index == Some(i)))
            } else {
                let files = world.resource::<EditorProjectFileCatalog>();
                let index = files.entries.iter().position(|item| item.asset_path == "assets");
                index.and_then(|i| visible(world, |r: &ProjectFileRow| r.entry_index == Some(i)))
            };
            click(world, &mut run.pointer, window, row);
            return Ok(None);
        }
        Stage::Loaded => {
            if form::entity(world, "RulesWorkbench").is_none() {
                return Ok(None);
            }
            let Some((source, dirty, ..)) = observe(world) else {
                return Ok(None);
            };
            let reopening = run.phase == "reopen";
            let expected = if reopening {
                fs::read_to_string(run.work.join("project/assets").join(PATH))
                    .map_err(|e| e.to_string())?
            } else {
                EXAMPLE.to_owned()
            };
            if dirty || source != expected.replace("\r\n", "\n") {
                return Err("中央文档首次读取的规则正文与磁盘预期不符".into());
            }
            Stage::Form(0)
        }
        Stage::Form(index) => return form::form(world, run, window, index),
        Stage::Advanced => {
            if !form::named_click(world, run, window, "RulesSource") {
                return Ok(None);
            }
            Stage::Input(false)
        }
        Stage::Edit(valid) => {
            // 保存按钮会接走正文焦点；点击正式编辑入口把焦点交还原生完整输入。
            let button = visible(world, |a: &DocumentAction| *a == DocumentAction::Edit);
            if !click(world, &mut run.pointer, window, button) {
                return Ok(None);
            }
            Stage::Input(valid)
        }
        Stage::Input(valid) => {
            let Some((_, _, true, _)) = observe(world) else {
                return Ok(None);
            };
            let Some(input) = visible(world, |source: &DocumentSource| source.key.is_some()) else {
                return Ok(None);
            };
            let desired = if valid { &run.edited } else { &run.invalid };
            if !form::replace(
                world,
                &mut run.pointer,
                &mut run.keys,
                window,
                input,
                desired,
            ) {
                return Ok(None);
            }
            if !observe(world).is_some_and(|(text, dirty, ..)| dirty && text == desired) {
                return Ok(None);
            }
            Stage::Save(valid)
        }
        Stage::Save(valid) => {
            let button = visible(world, |a: &DocumentAction| *a == DocumentAction::Save);
            if !click(world, &mut run.pointer, window, button) {
                return Ok(None);
            }
            Stage::Verify(valid)
        }
        Stage::Verify(valid) => {
            let Some((source, dirty, _, status)) = observe(world) else {
                return Ok(None);
            };
            if valid {
                if dirty || source != run.edited {
                    return Ok(None);
                }
                Stage::CaptureSource
            } else {
                if !dirty || !status.contains("未声明") {
                    return Ok(None);
                }
                if fs::read_to_string(run.work.join("project/assets").join(PATH))
                    .map_err(|e| e.to_string())?
                    != run.saved_form
                {
                    return Err("拒绝错误源码时磁盘上的表单保存结果已改变".into());
                }
                run.diagnostic = Some(status.to_owned());
                Stage::Edit(true)
            }
        }
        Stage::Capture => {
            let path = run.work.join(format!("rules-{}.png", run.phase));
            let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
                return Ok(None);
            };
            run.form_shot = shot;
            if run.phase == "reopen" {
                finish(world, run, None);
                Stage::Complete
            } else {
                Stage::Advanced
            }
        }
        Stage::CaptureSource => {
            let path = run.work.join("rules-source.png");
            let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
                return Ok(None);
            };
            run.source_shot = shot;
            finish(world, run, None);
            Stage::Complete
        }
        Stage::Complete => return Ok(None),
    };
    Ok(Some(next))
}

fn finish(world: &mut World, run: &mut Run, error: Option<String>) {
    let value = json!({"phase":run.phase,"passed":error.is_none(),"error":error,"stage":format!("{:?}",run.stage),
        "diagnostic":run.diagnostic,"screenshot":run.form_shot,"source_screenshot":run.source_shot,"saved_status":observe(world).map(|d|d.3),
        "entry":if run.phase=="reopen"{"文件 → assets → 双击 project_rules.json"}else{"交互 → 从示例创建"},
        "input":"Bevy WindowEvent（原生表单与源码输入、按钮、撤销和重做快捷键）","os_input_verified":false});
    let saved = evidence::report(&run.work.join(format!("rules-{}.json", run.phase)), &value);
    if let Err(error) = &saved {
        error!("工程规则验收报告写入失败：{error}");
    }
    run.stage = Stage::Complete;
    world.write_message(if error.is_none() && saved.is_ok() {
        AppExit::Success
    } else {
        AppExit::error()
    });
}
