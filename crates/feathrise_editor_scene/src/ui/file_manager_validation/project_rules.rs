//! 两个真实窗口分别验证首次示例、错误保存、正式写入和关闭进程后重开。
use super::{
    click, evidence, fixture_focus, key_state, move_cursor, visible, window_event,
};
use crate::{
    resources::SceneEditorState,
    ui::{
        asset_preview::DocumentState,
        gameplay_sidebar::project_rules::RuleButton,
        sidebars::{EditorLeftSidebarTab, EditorSidebarTab, SceneEditorSidebarTabButton},
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
    Loaded,
    Edit(bool),
    Input(bool),
    Save(bool),
    Verify(bool),
    Capture,
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
    original: String,
    invalid: String,
    edited: String,
    diagnostic: Option<String>,
    capture: Option<evidence::CaptureTask>,
}

pub(super) fn install(app: &mut App, work: PathBuf, phase: String) -> Result<(), String> {
    // 文档的内存正文统一 LF，磁盘仍须逐字节保留本机检出的示例。
    let original = EXAMPLE.replace("\r\n", "\n");
    app.insert_resource(Run {
        work,
        phase,
        stage: Stage::Sidebar,
        since: Instant::now(),
        pointer: None,
        keys: 0,
        invalid: original.replace("\"set\": \"route\"", "\"set\": \"missing_rules_variable\""),
        edited: original.replace("\"smith_route\"", "\"smith_route_edited\""),
        original,
        diagnostic: None,
        capture: None,
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
            Err(format!(
                "等待 {:?} 超时，原生输入步骤 {}，文档 {:?}，操作栏 {:?}",
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
            Err(error) => finish(world, &mut run, Value::Null, Some(error)),
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
            let tab = visible(world, |t: &SceneEditorSidebarTabButton| {
                t.0 == EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay)
            });
            if !click(world, &mut run.pointer, window, tab) {
                return Ok(None);
            }
            Stage::Open
        }
        Stage::Open => {
            // 第二次仍点击「从示例创建」，验证它会保留作者刚保存的工程文件。
            let button = visible(world, |b: &RuleButton| b.0);
            if !click(world, &mut run.pointer, window, button) {
                return Ok(None);
            }
            Stage::Loaded
        }
        Stage::Loaded => {
            let Some((source, dirty, ..)) = observe(world) else {
                return Ok(None);
            };
            let reopening = run.phase == "reopen";
            let expected = if reopening {
                run.edited.as_str()
            } else {
                run.original.as_str()
            };
            if dirty || source != expected {
                return Err("中央文档首次读取的规则正文与磁盘预期不符".into());
            }
            if reopening {
                Stage::Capture
            } else {
                Stage::Edit(false)
            }
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
            if world.resource::<InputFocus>().get() != Some(input) {
                return Ok(None);
            }
            let desired = if valid { &run.edited } else { &run.invalid };
            match run.keys {
                0 => key_state(
                    world,
                    window,
                    KeyCode::ControlLeft,
                    Key::Control,
                    ButtonState::Pressed,
                ),
                1 => key_state(
                    world,
                    window,
                    KeyCode::KeyA,
                    Key::Character("a".into()),
                    ButtonState::Pressed,
                ),
                2 => {
                    key_state(
                        world,
                        window,
                        KeyCode::KeyA,
                        Key::Character("a".into()),
                        ButtonState::Released,
                    );
                    key_state(
                        world,
                        window,
                        KeyCode::ControlLeft,
                        Key::Control,
                        ButtonState::Released,
                    );
                }
                3 => window_event(
                    world,
                    Ime::Commit {
                        window,
                        value: desired.clone(),
                    },
                ),
                _ if !observe(world).is_some_and(|(text, dirty, ..)| dirty && text == desired) => {
                    return Ok(None);
                }
                _ => return Ok(Some(Stage::Save(valid))),
            }
            run.keys += 1;
            return Ok(None);
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
                Stage::Capture
            } else {
                if !dirty || !status.contains("未声明") {
                    return Ok(None);
                }
                if fs::read_to_string(run.work.join("project/assets").join(PATH))
                    .map_err(|e| e.to_string())?
                    != EXAMPLE
                {
                    return Err("拒绝错误保存时磁盘上的原示例已改变".into());
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
            finish(world, run, shot, None);
            Stage::Complete
        }
        Stage::Complete => return Ok(None),
    };
    Ok(Some(next))
}

fn finish(world: &mut World, run: &mut Run, screenshot: Value, error: Option<String>) {
    let value = json!({"phase":run.phase,"passed":error.is_none(),"error":error,"stage":format!("{:?}",run.stage),
        "diagnostic":run.diagnostic,"screenshot":screenshot,"saved_status":observe(world).map(|d|d.3),
        "input":"Bevy WindowEvent（侧栏、原生正文输入、保存按钮）","os_input_verified":false});
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
