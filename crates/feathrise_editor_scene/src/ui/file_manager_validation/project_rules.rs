//! 空工程走首次添加流程，示例工程通过交互入口与文件列表验证表单编辑、布局、保存和重开。
mod directory;
mod form;
mod layout;
mod location;
mod table;
use super::{click, evidence, fixture_focus, key_state, move_cursor, visible, window_event};
use crate::{
    asset_catalog::{EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::{SceneEditorAssetRow, SceneEditorViewport},
    resources::SceneEditorState,
    ui::{
        asset_preview::DocumentState,
        project_browser::{ProjectBrowserState, ProjectFileRow},
        sidebars::{
            EditorLeftSidebarTab, EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarTabButton,
        },
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
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
    components::{
        DocumentAction, DocumentCenterHost, DocumentPathText, DocumentPreviewBody, DocumentSource,
    },
    project_rules::{EMPTY, EXAMPLE, PATH},
};
pub(crate) use layout::reveal_rules_control;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    PrepareScene,
    Sidebar,
    Directory(usize),
    Open,
    OpenContext,
    ContextFile,
    ContextSidebar,
    ContextTools(usize),
    Loaded,
    Form(usize),
    Advanced,
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
    layout: serde_json::Map<String, Value>,
    direct_saves: serde_json::Map<String, Value>,
    deletion: Option<form::DeletionCheck>,
    layout_wait: String,
    wheel_point: Option<Vec2>,
    splitter_target: Option<Vec2>,
    pixel_tolerance: f32,
    center: Value,
}

const CONTEXT: &str = "inline_context.md";

pub(super) fn install(app: &mut App, work: PathBuf, phase: String) -> Result<(), String> {
    app.insert_resource(Run {
        work,
        stage: if phase == "empty" {
            Stage::PrepareScene
        } else {
            Stage::Sidebar
        },
        phase,
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
        layout: default(),
        direct_saves: default(),
        deletion: None,
        layout_wait: String::new(),
        wheel_point: None,
        splitter_target: None,
        pixel_tolerance: 0.,
        center: Value::Null,
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
    let run = world.resource::<Run>();
    let Some(point) = run
        .pointer
        .and_then(|(e, _)| world.get::<UiGlobalTransform>(e).map(|t| t.translation))
        .or(run.wheel_point)
    else {
        return;
    };
    if let Some(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
    {
        move_cursor(world, window, point.as_dvec2());
    }
}

fn observe(world: &World) -> Option<(&str, bool, bool, &str)> {
    world
        .get_resource::<DocumentState>()?
        .project_rules_observed()
}

/// 核对当前中央标签、可见面板实体及真实边界，防止“短暂调过去再调回来”也通过。
fn central(world: &mut World) -> Option<Value> {
    let tab = world.resource::<VegetationPresetEditorState>().active_tab;
    let (id, doc, preview) = match tab {
        PresetWorkspaceTab::Scene => (visible(world, |_: &SceneEditorViewport| true)?, None, None),
        PresetWorkspaceTab::Document => {
            let host = visible(world, |_: &DocumentCenterHost| true)?;
            let path = visible(world, |_: &DocumentPathText| true)?;
            let body = visible(world, |_: &DocumentPreviewBody| true)?;
            if !directory::ancestor(world, path, host) || !directory::ancestor(world, body, host) {
                return None;
            }
            (
                host,
                Some(world.get::<Text>(path)?.as_str().to_owned()),
                Some(body.to_bits()),
            )
        }
        _ => return None,
    };
    Some(
        json!({"tab":format!("{tab:?}"),"entity":id.to_bits(),"document":doc, "preview":preview,
        "bounds":layout::coords(layout::bounds(world, id)?)}),
    )
}

fn check_center(world: &mut World, run: &Run) -> Result<(), String> {
    if run.center.is_null() || central(world).as_ref() == Some(&run.center) {
        return Ok(());
    }
    Err(format!(
        "点击右侧交互工具改变了中央页面，原 {}，现 {:?}",
        run.center,
        central(world)
    ))
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
                form::describe(&run.phase, i)
            } else {
                String::new()
            };
            let focus = world
                .get_resource::<InputFocus>()
                .and_then(|f| f.get())
                .and_then(|e| world.get::<Name>(e))
                .map(Name::as_str);
            Err(format!(
                "等待 {:?} {form} 超时，原生输入步骤 {}，焦点 {focus:?}，布局 {}，文档 {:?}，操作栏 {:?}",
                run.stage,
                run.keys,
                run.layout_wait,
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
                run.layout_wait.clear();
                run.wheel_point = None;
            }
            Ok(None) => {}
            Err(error) => finish(world, &mut run, Some(error)),
        }
    });
}

fn advance(world: &mut World, run: &mut Run, window: Entity) -> Result<Option<Stage>, String> {
    check_center(world, run)?;
    let next = match run.stage {
        Stage::PrepareScene => {
            let Some(mut editor) = world.get_resource_mut::<SceneEditorState>() else {
                return Ok(None);
            };
            if !editor.open || editor.scene_transition_in_progress {
                return Ok(None);
            }
            editor.queue_scene_transition(
                feathrise_scene_content::SceneId::parse("M1-SAMPLE")?,
                "打开本轮交互工具目录夹具场景",
            );
            Stage::Sidebar
        }
        Stage::Sidebar => {
            if !world
                .get_resource::<SceneEditorState>()
                .is_some_and(|e| e.open && e.pending_scene_transition.is_none())
            {
                return Ok(None);
            }
            if run.phase == "empty"
                && !world
                    .resource::<crate::authoring::EditorAuthoringSession>()
                    .draft
                    .as_ref()
                    .is_some_and(|draft| draft.scene_id.as_str() == "M1-SAMPLE")
            {
                return Ok(None);
            }
            if run.phase == "empty" {
                let Some(center) = central(world).filter(|value| value["tab"] == "Scene") else {
                    return Ok(None);
                };
                run.center = center;
            }
            let target = EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay);
            let tab = visible(world, |t: &SceneEditorSidebarTabButton| t.0 == target);
            if !click(world, &mut run.pointer, window, tab) {
                return Ok(None);
            }
            if run.phase == "empty" {
                Stage::Directory(0)
            } else {
                Stage::Open
            }
        }
        Stage::Directory(index) => return directory::before_open(world, run, window, index),
        Stage::Open => {
            let name = match run.phase.as_str() {
                "draw-reopen" => "交互目录条目地点区域",
                "reopen" => "交互目录条目触发规则",
                _ => "交互目录条目工程变量",
            };
            if !form::named_click(world, run, window, name) {
                run.layout_wait = button_diagnostic(world, name);
                return Ok(None);
            }
            Stage::Loaded
        }
        Stage::OpenContext => {
            let tab = visible(world, |button: &SceneEditorSidebarTabButton| {
                button.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
            });
            if !click(world, &mut run.pointer, window, tab) {
                return Ok(None);
            }
            Stage::ContextFile
        }
        Stage::ContextFile => {
            // 用正式文件列表双击，给中央打开独立文档，再核对每项右栏工具都保持它。
            if let Some(center) = central(world).filter(|value| {
                value["tab"] == "Document"
                    && value["document"]
                        .as_str()
                        .is_some_and(|path| path.contains(CONTEXT))
            }) {
                run.center = center;
                return Ok(Some(Stage::ContextSidebar));
            }
            let row = if world.resource::<ProjectBrowserState>().assets_visible() {
                let assets = world.resource::<EditorAssetCatalog>();
                let index = assets
                    .entries
                    .iter()
                    .position(|item| item.asset_path == CONTEXT);
                index.and_then(|i| {
                    visible(world, |r: &SceneEditorAssetRow| r.entry_index == Some(i))
                })
            } else {
                let files = world.resource::<EditorProjectFileCatalog>();
                let index = files
                    .entries
                    .iter()
                    .position(|item| item.asset_path == "assets");
                index.and_then(|i| visible(world, |r: &ProjectFileRow| r.entry_index == Some(i)))
            };
            click(world, &mut run.pointer, window, row);
            return Ok(None);
        }
        Stage::ContextSidebar => {
            // 双击资产会进入资产检查器；在中央文档仍打开时显式回到右侧交互。
            let tab = visible(world, |button: &SceneEditorSidebarTabButton| {
                button.0 == EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay)
            });
            if !click(world, &mut run.pointer, window, tab) {
                return Ok(None);
            }
            Stage::ContextTools(0)
        }
        Stage::ContextTools(index) => return directory::on_document(world, run, window, index),
        Stage::Loaded => {
            if form::entity(world, "RulesWorkbench").is_none() {
                return Ok(None);
            }
            let Some((source, dirty, ..)) = observe(world) else {
                return Ok(None);
            };
            let reopening = matches!(run.phase.as_str(), "reopen" | "draw-reopen");
            let expected = if reopening {
                fs::read_to_string(run.work.join("project/assets").join(PATH))
                    .map_err(|e| e.to_string())?
            } else if run.phase == "empty" {
                EMPTY.to_owned()
            } else {
                EXAMPLE.to_owned()
            };
            if dirty || source != expected.replace("\r\n", "\n") {
                return Err("右栏表单首次读取的规则正文与磁盘预期不符".into());
            }
            Stage::Form(0)
        }
        Stage::Form(index) => return form::form(world, run, window, index),
        Stage::Advanced => {
            // 触发规则目录不提供高级源码。显式切到仍保留文件入口的地点区域页。
            if layout::caption(world, "RulesPageTitle").as_deref() != Some("地点区域") {
                form::named_click(world, run, window, "交互目录条目地点区域");
                return Ok(None);
            }
            if !form::named_click(world, run, window, "RulesSource") {
                return Ok(None);
            }
            Stage::Input(false)
        }
        Stage::Input(valid) => {
            let Some((_, _, true, _)) = observe(world) else {
                return Ok(None);
            };
            let Some(input) = visible(world, |source: &DocumentSource| source.key.is_some()) else {
                run.layout_wait = format!(
                    "源码控件 {:?}",
                    world.query::<(Entity, &DocumentSource, &ComputedNode, &InheritedVisibility)>()
                        .iter(world).map(|(id, source, node, visible)|
                            (id, source.key.is_some(), node.size(), visible.get())
                        ).collect::<Vec<_>>()
                );
                return Ok(None);
            };
            if valid {
                let focused = world.resource::<InputFocus>().get();
                run.layout_wait = format!(
                    "源码 {input:?} 边界 {:?} 裁剪 {:?} 焦点 {focused:?} 命中 {:?}",
                    layout::bounds(world, input),
                    world.get::<bevy::ui::CalculatedClip>(input),
                    world.get::<bevy::ui::RelativeCursorPosition>(input),
                );
            }
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
                let rejected = run.work.join("rules-source-rejected.png");
                let Some(shot) = evidence::capture(world, &mut run.capture, &rejected)? else {
                    return Ok(None);
                };
                run.layout.insert("source-rejected".into(), shot);
                // 保存校验失败后源码仍处于编辑态。原生输入步骤直接点击编辑器取焦点。
                Stage::Input(true)
            }
        }
        Stage::Capture => {
            let path = run.work.join(format!("rules-{}.png", run.phase));
            let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
                return Ok(None);
            };
            run.form_shot = shot;
            if run.phase != "edit" {
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

fn button_diagnostic(world: &mut World, name: &str) -> String {
    let Some(button) = form::entity(world, name) else {
        return format!("{name}尚未可见");
    };
    let bounds = layout::bounds(world, button);
    let interaction = world.get::<feathrise_editor_ui::UiInteraction>(button);
    format!("{name} 边界 {bounds:?}，交互 {interaction:?}")
}

fn finish(world: &mut World, run: &mut Run, error: Option<String>) {
    let value = json!({"phase":run.phase,"passed":error.is_none(),"error":error,"stage":format!("{:?}",run.stage),
        "diagnostic":run.diagnostic,"screenshot":run.form_shot,"source_screenshot":run.source_shot,"saved_status":observe(world).map(|d|d.3),
        "layout":run.layout,"direct_saves":run.direct_saves,
        "entry":match run.phase.as_str(){"reopen"=>"交互 → 触发规则","draw-reopen"=>"交互 → 地点区域","empty"=>"交互 → 六项直达",_=>"交互 → 工程变量"},
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
