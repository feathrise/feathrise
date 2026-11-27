//! 独占工程内通过正式文件菜单、源码和画布验证原生 Rust 界面，重启后双击重开。

use super::super::{components::{DocumentAction, DocumentSource}, types::ViewMode};
use super::{Action, UiLayouts, canvas::CanvasItem, fields::Field};
use crate::{
    authoring::EditorAuthoringSession,
    asset_catalog::{EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::{SceneEditorActionButton, SceneEditorAssetRow},
    resources::SceneEditorState,
    types::EditorAction,
    ui::{
        external_files::platform_path,
        file_manager::{
            FileAction, FileManagerButton, FileManagerState, FileNameInput, FixedFileSelection,
        },
        file_manager_validation::{
            click, evidence, fixture_focus, mouse, move_cursor, pointer_click, right_click, visible,
        },
        project_browser::{ProjectBrowserState, ProjectBrowserView, ProjectFileRow},
        sidebars::{
            EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarState,
            SceneEditorSidebarTabButton,
        },
    },
    standalone::validation_fixture::{key_state, window_event},
    workbench::WorkbenchModel,
};
use bevy::{
    input::{ButtonState, keyboard::{Key, KeyboardInput, NativeKeyCode}, mouse::MouseButton},
    input_focus::InputFocus,
    picking::{hover::HoverMap, pointer::PointerId},
    prelude::*,
    text::EditableText,
    window::PrimaryWindow,
};
use feathrise_editor_documents_model::history::components::HistoryAction;
use feathrise_ui_bsn::{UiLayout, WidgetKind};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const FILE: &str = "character_stats.rs";
const USER_CODE: &str = "\npub const EDITOR_USER_CODE: &str = \"manual Rust is retained\";\n";

#[derive(Debug, PartialEq, Eq)]
struct SceneSnapshot {
    revision: u64,
    successful_saves: u64,
    history: (usize, usize),
    dirty: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct DocumentSnapshot {
    source: Arc<str>,
    revision: u64,
    history: (usize, usize),
    dirty: bool,
}

#[derive(Resource)]
struct Run {
    work: PathBuf,
    folder: PathBuf,
    phase: String,
    stage: usize,
    since: Instant,
    click: Option<(Entity, bool)>,
    pointer_click: Option<(bevy::math::DVec2, u8)>,
    typing: Option<(Entity, u8)>,
    keyboard_inputs: usize,
    public_actions: [usize; 3],
    scene: Option<SceneSnapshot>,
    scene_route_error: Option<String>,
    preview_document: Option<DocumentSnapshot>,
    modes_verified: bool,
    pointer: Option<bevy::math::DVec2>,
    capture: Option<evidence::CaptureTask>,
}

pub(crate) fn install(app: &mut App, work: PathBuf, phase: String) -> Result<(), String> {
    if !matches!(phase.as_str(), "edit" | "reopen")
        || std::fs::read(work.join(".ui-bsn-fixture")).ok().as_deref()
            != Some(b"feathrise-ui-bsn-v1")
    {
        return Err("界面布局验收拒绝未核验的夹具或阶段".into());
    }
    let folder = platform_path(&work.join("project/assets/ui"));
    app.insert_resource(Run {
        work,
        folder,
        phase,
        stage: 0,
        since: Instant::now(),
        click: None,
        pointer_click: None,
        typing: None,
        keyboard_inputs: 0,
        public_actions: [0; 3],
        scene: None,
        scene_route_error: None,
        preview_document: None,
        modes_verified: false,
        pointer: None,
        capture: None,
    })
    .add_systems(PreUpdate, hold.before(bevy::ui::UiSystems::Focus))
    .add_systems(
        Update,
        fixture_focus.before(crate::SceneEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        observe_scene_route
            .after(crate::ui::actions::handle_editor_shortcuts)
            .before(crate::authoring::apply_sparse_terrain_undo_redo)
            .before(crate::authoring::apply_undo_redo),
    )
    .add_systems(Last, drive);
    Ok(())
}

fn hold(world: &mut World) {
    let run = world.resource::<Run>();
    let at = run
        .pointer
        .or(run.pointer_click.map(|(position, _)| position))
        .or_else(|| {
            run.click
                .and_then(|(entity, _)| world.get::<UiGlobalTransform>(entity))
                .map(|transform| transform.translation.as_dvec2())
        });
    if let Some(at) = at
        && let Some(window) = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .iter(world)
            .next()
    {
        move_cursor(world, window, at);
    }
}

fn scene_snapshot(world: &World) -> SceneSnapshot {
    let session = world.resource::<EditorAuthoringSession>();
    SceneSnapshot {
        revision: session.revision,
        successful_saves: session.successful_save_sequence,
        history: (session.undo.len(), session.redo.len()),
        dirty: world.resource::<SceneEditorState>().dirty,
    }
}

fn document_snapshot(world: &World) -> Option<DocumentSnapshot> {
    let doc = world.resource::<super::DocumentState>().current()?;
    Some(DocumentSnapshot {
        source: doc.source.clone(),
        revision: doc.revision,
        history: super::super::history::validation_lengths(&doc.history),
        dirty: doc.dirty(),
    })
}

/// 在场景历史消费前观察公共分派结果；空场景也必须证明没有收到文档的撤销请求。
fn observe_scene_route(
    editor: Res<SceneEditorState>,
    documents: Res<super::DocumentState>,
    mut run: ResMut<Run>,
) {
    if run.scene.is_some() && documents.in_center()
        && documents.current().is_some_and(|doc| doc.path.ends_with(FILE))
        && (editor.save_requested || editor.undo_requested || editor.redo_requested)
    {
        run.scene_route_error = Some("界面文档的公共保存或历史操作串入了隐藏场景".into());
    }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        let Some(window) = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .iter(world)
            .next()
        else {
            return;
        };
        let result = if let Some(error) = &run.scene_route_error {
            Err(error.clone())
        } else if run.since.elapsed() > Duration::from_secs(75) {
            let source_inputs = world.query_filtered::<(Entity, &ComputedNode, &InheritedVisibility,
                Option<&bevy::input_focus::tab_navigation::TabIndex>, &EditableText), With<DocumentSource>>()
                .iter(world).map(|(entity, node, visibility, tab, input)| {
                    (entity, node.size(), visibility.get(), tab.is_some(), input.value().to_string().len())
                }).collect::<Vec<_>>();
            let files = world.resource::<FileManagerState>();
            let fixed = world.resource::<FixedFileSelection>();
            let ui = world.resource::<UiLayouts>();
            let doc = world.resource::<super::DocumentState>().current();
            Err(format!(
                "阶段 {} 超时，固定栏目录 {:?}（读取中 {}），文件菜单 {}，文件状态 {:?}，当前文档 {:?}，界面状态 {:?}，按压 {:?}，录入 {:?}，焦点 {:?}，源码框 {:?}",
                run.stage,
                fixed.directory,
                fixed.loading,
                files.context_menu_open(),
                files.notice,
                doc.map(|doc| (&doc.path, &doc.status)),
                ui.notice,
                run.click.map(|(entity, down)| (entity, down, world.get::<feathrise_editor_ui::UiInteraction>(entity))),
                run.typing,
                world.resource::<InputFocus>().get(),
                source_inputs,
            ))
        } else {
            step(world, &mut run, window)
        };
        match result {
            Ok(Progress::Wait) => {}
            Ok(Progress::Next) => {
                run.stage += 1;
                run.since = Instant::now();
                eprintln!("独立界面布局 {} 阶段 {}", run.phase, run.stage);
            }
            final_result => {
                let mut value = match final_result {
                    Ok(Progress::Done(shot)) => {
                        json!({"passed":true,"phase":run.phase,"screenshot":shot})
                    }
                    Err(error) => {
                        json!({"passed":false,"phase":run.phase,"stage":run.stage,"error":error})
                    }
                    _ => unreachable!(),
                };
                value["input"] = json!({
                    "source":"Bevy WindowEvent 鼠标聚焦、Ctrl+A 与键盘文字事件",
                    "completed_fields":run.keyboard_inputs,
                    "os_input_verified":false,
                });
                value["controls"] = json!({
                    "public_save_clicks":run.public_actions[0],
                    "public_undo_clicks":run.public_actions[1],
                    "public_redo_clicks":run.public_actions[2],
                    "adjacent_modes_and_no_preview_draft":run.modes_verified,
                    "hidden_scene_unchanged":run.scene.as_ref().is_some_and(|before| *before == scene_snapshot(world)),
                });
                let output = run.work.join(format!("ui-bsn-{}.json", run.phase));
                let ok = value["passed"] == true && evidence::report(&output, &value).is_ok();
                if !ok {
                    let _ = evidence::report(&output, &value);
                }
                world.write_message(if ok {
                    AppExit::Success
                } else {
                    AppExit::error()
                });
            }
        }
    });
}

enum Progress {
    Wait,
    Next,
    Done(serde_json::Value),
}

fn action(world: &mut World, run: &mut Run, window: Entity, action: Action) -> bool {
    let target = visible(world, |button: &Action| *button == action);
    click(world, &mut run.click, window, target)
}

fn public_action(world: &mut World, run: &mut Run, window: Entity, action: EditorAction) -> bool {
    let target = visible(world, |button: &SceneEditorActionButton| button.0 == action);
    let clicked = click(world, &mut run.click, window, target);
    if clicked {
        let index = match action {
            EditorAction::Save => 0,
            EditorAction::Undo => 1,
            EditorAction::Redo => 2,
            _ => unreachable!(),
        };
        run.public_actions[index] += 1;
    }
    clicked
}

fn inside_layout_panel(world: &World, mut entity: Entity) -> bool {
    loop {
        if world.get::<super::view::Panel>(entity).is_some() { return true; }
        let Some(parent) = world.get::<ChildOf>(entity) else { return false; };
        entity = parent.parent();
    }
}

/// 只查正式可见节点，防止验收仍点击文档内的重复按钮而绕过顶部公共路由。
fn common_toolbar(world: &mut World) -> Result<bool, String> {
    for action in [EditorAction::Save, EditorAction::Undo, EditorAction::Redo] {
        let buttons: Vec<_> = world.query::<(Entity, &SceneEditorActionButton, &ComputedNode, &InheritedVisibility)>()
            .iter(world).filter(|(_, button, node, visibility)| button.0 == action
                && node.size().min_element() > 1.0 && visibility.get())
            .map(|(entity, ..)| entity).collect();
        if buttons.is_empty() { return Ok(false); }
        if buttons.len() != 1 || inside_layout_panel(world, buttons[0]) {
            return Err(format!("{action:?} 应只有一个顶部公共按钮，界面内部不能重复"));
        }
    }
    if visible(world, |action: &DocumentAction| matches!(action, DocumentAction::Save | DocumentAction::SaveAll)).is_some()
        || visible(world, |_: &HistoryAction| true).is_some()
    {
        return Err("界面文档仍显示局部保存、撤销或重做按钮".into());
    }
    Ok(true)
}

fn file_button(world: &mut World, run: &mut Run, window: Entity, action: FileAction) -> bool {
    let target = visible(world, |button: &FileManagerButton| button.action == action);
    click(world, &mut run.click, window, target)
}

fn click_files_tab(world: &mut World, run: &mut Run, window: Entity) -> bool {
    if world.resource::<SceneEditorSidebarState>().navigator == EditorNavigatorTab::Assets {
        return true;
    }
    let target = visible(world, |tab: &SceneEditorSidebarTabButton| {
        tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
    });
    click(world, &mut run.click, window, target);
    false
}

fn open_assets_folder(world: &mut World, run: &mut Run, window: Entity) -> bool {
    // 发布点击和目录实际采用是不同的帧，启动中的点击被输入门拒绝后继续等候重试。
    let fixed = world.resource::<FixedFileSelection>();
    if fixed.asset_view
        && !fixed.loading
        && world.resource::<ProjectBrowserState>().view == ProjectBrowserView::Assets
    {
        return true;
    }
    let catalog = world.resource::<EditorProjectFileCatalog>();
    if catalog.is_loading() {
        return false;
    }
    let index = catalog
        .entries
        .iter()
        .position(|entry| entry.asset_path == "assets");
    let target = index
        .and_then(|index| visible(world, |row: &ProjectFileRow| row.entry_index == Some(index)));
    click(world, &mut run.click, window, target);
    false
}

fn asset_row(world: &mut World, path: &str) -> Option<Entity> {
    let fixed = world.resource::<FixedFileSelection>();
    if !fixed.asset_view || fixed.loading {
        return None;
    }
    let index = world
        .resource::<EditorAssetCatalog>()
        .entries
        .iter()
        .position(|entry| entry.asset_path == path)?;
    visible(world, |row: &SceneEditorAssetRow| {
        row.entry_index == Some(index)
    })
}

fn context_for_ui_folder(world: &mut World, run: &mut Run, window: Entity) -> bool {
    let Some(position) = asset_row(world, "ui")
        .and_then(|row| world.get::<UiGlobalTransform>(row))
        .map(|node| node.translation.as_dvec2())
    else {
        return false;
    };
    right_click(world, &mut run.pointer_click, window, position)
        && world.resource::<FileManagerState>().context_menu_open()
}

fn type_in<C: Component>(
    world: &mut World, run: &mut Run, window: Entity,
    find: impl Fn(&C) -> bool, value: &str,
) -> bool {
    let Some(entity) = visible(world, find) else {
        return false;
    };
    let matches = world.get::<EditableText>(entity).is_some_and(|text| text.value() == value);
    if matches && run.typing.is_none_or(|(target, _)| target != entity) {
        return true;
    }
    let (target, stage) = *run.typing.get_or_insert((entity, 0));
    if target != entity {
        return false;
    }
    if stage == 0 {
        let Some(position) = world.get::<UiGlobalTransform>(entity)
            .map(|node| node.translation.as_dvec2()) else { return false; };
        if pointer_click(world, &mut run.pointer_click, window, position, MouseButton::Left) {
            run.typing = Some((entity, 1));
        }
        return false;
    }
    // 必须由前面的正式指针流程取得焦点；直接写文字队列会漏掉不可点击输入的回归。
    if world.resource::<InputFocus>().get() != Some(entity) {
        return false;
    }
    match stage {
        1 | 2 => {
            let state = if stage == 1 { ButtonState::Pressed } else { ButtonState::Released };
            for (code, key) in [(KeyCode::ControlLeft, Key::Control), (KeyCode::KeyA, Key::Character("a".into()))] {
                key_state(world, window, code, key, state);
            }
        }
        3 => {
            let event = KeyboardInput {
                window,
                key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                logical_key: Key::Character(value.into()),
                state: ButtonState::Pressed,
                text: Some(value.into()),
                repeat: false,
            };
            window_event(world, event.clone());
            window_event(world, KeyboardInput { state: ButtonState::Released, text: None, ..event });
        }
        _ if matches => {
            run.typing = None;
            run.keyboard_inputs += 1;
            return true;
        }
        _ => return false,
    }
    // 组合键的按下、释放和文字输入跨帧，交给正式输入系统依次处理。
    run.typing = Some((entity, stage + 1));
    false
}

fn binding_fields(world: &mut World, run: &mut Run, window: Entity, id: &str, height: &str) -> bool {
    [(Field::Id, id), (Field::Content, id), (Field::Height, height)]
        .into_iter()
        .all(|(expected, value)| type_in(world, run, window, |field: &Field| *field == expected, value))
}

fn widget(world: &World, id: &str) -> Option<WidgetKind> {
    world
        .resource::<UiLayouts>()
        .layout
        .as_ref()?
        .root
        .find(id)
        .map(|node| node.kind)
}

fn edited(layout: &UiLayout) -> bool {
    [("physical_attack", 31.0), ("health", 24.0)]
        .into_iter()
        .all(|(key, height)| {
            layout.root.find(key).is_some_and(|node| {
                node.kind == WidgetKind::Binding
                    && node.content == key
                    && node.height == Val::Px(height)
            })
        })
        && layout.root.find("widget_1").is_some_and(|group| {
            group.children.iter().any(|text| {
                text.kind == WidgetKind::Text
                    && text
                        .position
                        .is_some_and(|position| position.x > 20.0 && position.y > 14.0)
            })
        })
}

fn step(world: &mut World, run: &mut Run, window: Entity) -> Result<Progress, String> {
    let path = run.folder.join(FILE);
    let editor = world.resource::<SceneEditorState>();
    if !editor.open
        || editor.pending_scene_transition.is_some()
        || world
            .get_resource::<WorkbenchModel>()
            .is_some_and(|host| !host.visible || host.busy || host.modal)
    {
        return Ok(Progress::Wait);
    }
    if world.resource::<super::DocumentState>().in_center()
        && world.resource::<super::DocumentState>().current().is_some_and(|doc| doc.path.ends_with(FILE))
    {
        let current = scene_snapshot(world);
        if run.scene.as_ref().is_some_and(|before| *before != current) {
            return Err("编辑界面文档改变了隐藏场景的修订、保存回执或历史".into());
        }
        run.scene.get_or_insert(current);
    }
    let advance = if run.phase == "reopen" {
        match run.stage {
            0 => click_files_tab(world, run, window),
            1 => open_assets_folder(world, run, window),
            2 => {
                let fixed = world.resource::<FixedFileSelection>();
                if !fixed.loading && platform_path(&fixed.directory) == run.folder {
                    true
                } else {
                    let row = asset_row(world, "ui");
                    click(world, &mut run.click, window, row);
                    false
                }
            }
            3 | 4 => {
                let fixed = world.resource::<FixedFileSelection>();
                if platform_path(&fixed.directory) != run.folder {
                    return Ok(Progress::Wait);
                }
                let row = asset_row(world, "ui/character_stats.rs");
                click(world, &mut run.click, window, row)
            }
            5 => {
                world
                    .resource::<UiLayouts>()
                    .layout
                    .as_deref()
                    .is_some_and(edited)
                    && common_toolbar(world)?
                    && action(world, run, window, Action::Source)
            }
            6 => {
                let document = world.resource::<super::DocumentState>().current();
                if !document.is_some_and(|doc| doc.mode == ViewMode::Edit && !doc.dirty()) {
                    return Ok(Progress::Wait);
                }
                if !common_toolbar(world)? { return Ok(Progress::Wait); }
                let preview = visible(world, |action: &DocumentAction| {
                    *action == DocumentAction::Preview
                });
                click(world, &mut run.click, window, preview)
            }
            7 => {
                world
                    .resource::<UiLayouts>()
                    .layout
                    .as_deref()
                    .is_some_and(edited)
                    && action(world, run, window, Action::Properties)
            }
            8 => {
                if world.resource::<UiLayouts>().edit_pending() {
                    return Ok(Progress::Wait);
                }
                if world
                    .resource::<super::DocumentState>()
                    .current()
                    .is_some_and(|doc| doc.dirty())
                {
                    return Err("未修改属性却使重开的 Rust 界面产生草稿".into());
                }
                true
            }
            _ => return modes_round_trip(world, run, window, 9),
        }
    } else {
        match run.stage {
            0 => {
                if path.exists() {
                    return Err("新建前验收资产已存在".into());
                }
                click_files_tab(world, run, window)
            }
            1 => open_assets_folder(world, run, window),
            2 => context_for_ui_folder(world, run, window),
            3 => file_button(world, run, window, FileAction::NewMenu),
            4 => file_button(world, run, window, FileAction::NewUiLayout),
            5 => type_in(world, run, window, |_: &FileNameInput| true, FILE),
            6 => file_button(world, run, window, FileAction::Confirm),
            7 => path.is_file() && common_toolbar(world)?
                && action(world, run, window, Action::Insert(WidgetKind::Binding)),
            8 => {
                widget(world, "widget_1") == Some(WidgetKind::Binding)
                    && binding_fields(world, run, window, "physical_attack", "bad-size")
            }
            9 => action(world, run, window, Action::Properties),
            10 => {
                if world.resource::<UiLayouts>().notice != "请输入有效的尺寸或坐标数值"
                {
                    return Ok(Progress::Wait);
                }
                let fields: std::collections::HashMap<_, _> = world
                    .query::<(&Field, &EditableText)>()
                    .iter(world)
                    .map(|(field, input)| (*field, input.value().to_string()))
                    .collect();
                if fields.get(&Field::Content).map(String::as_str) != Some("physical_attack")
                    || fields.get(&Field::Id).map(String::as_str) != Some("physical_attack")
                {
                    return Err("属性校验失败后，原有输入被清空".into());
                }
                type_in(world, run, window, |field: &Field| *field == Field::Height, "31")
            }
            11 => action(world, run, window, Action::Properties),
            12 => {
                widget(world, "physical_attack").is_some()
                    && action(world, run, window, Action::Insert(WidgetKind::Binding))
            }
            13 => {
                widget(world, "widget_1") == Some(WidgetKind::Binding)
                    && binding_fields(world, run, window, "health", "24")
            }
            14 => action(world, run, window, Action::Properties),
            15 => {
                widget(world, "health").is_some()
                    && action(world, run, window, Action::Insert(WidgetKind::Container))
            }
            16 => {
                widget(world, "widget_1") == Some(WidgetKind::Container)
                    && action(world, run, window, Action::Insert(WidgetKind::Text))
            }
            17 => {
                let Some(entity) = visible(world, |widget: &CanvasItem| widget.0 == "widget_2")
                else {
                    return Ok(Progress::Wait);
                };
                let at = world
                    .get::<UiGlobalTransform>(entity)
                    .unwrap()
                    .affine()
                    .translation
                    .as_dvec2();
                let hovered = world
                    .resource::<HoverMap>()
                    .0
                    .get(&PointerId::Mouse)
                    .is_some_and(|hits| hits.contains_key(&entity));
                run.pointer = Some(at);
                move_cursor(world, window, at);
                if hovered {
                    mouse(world, window, ButtonState::Pressed);
                }
                hovered
            }
            18 => {
                let at = run.pointer.ok_or("未捕获画布控件")? + bevy::math::DVec2::new(60.0, 45.0);
                run.pointer = Some(at);
                move_cursor(world, window, at);
                true
            }
            19 => {
                mouse(world, window, ButtonState::Released);
                run.pointer = None;
                true
            }
            20 => {
                world
                    .resource::<UiLayouts>()
                    .layout
                    .as_deref()
                    .is_some_and(edited)
                    && public_action(world, run, window, EditorAction::Undo)
            }
            21 => {
                // 撤销只移除这次拖动，容器和文本仍在同一个草稿里。
                let unpositioned = world
                    .resource::<UiLayouts>()
                    .layout
                    .as_deref()
                    .and_then(|layout| layout.root.find("widget_2"))
                    .is_some_and(|text| text.position.is_none());
                unpositioned && public_action(world, run, window, EditorAction::Redo)
            }
            22 => {
                world
                    .resource::<UiLayouts>()
                    .layout
                    .as_deref()
                    .is_some_and(edited)
                    && public_action(world, run, window, EditorAction::Save)
            }
            23 => {
                let doc = world.resource::<super::DocumentState>().current();
                let saved = doc.is_some_and(|doc| !doc.dirty() && doc.path.ends_with(FILE));
                let source = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                saved && UiLayout::parse_document(&source).is_ok_and(|layout| edited(&layout))
            }
            24 => action(world, run, window, Action::Source),
            25 => {
                let Some(doc) = world.resource::<super::DocumentState>().current() else {
                    return Ok(Progress::Wait);
                };
                if doc.mode != ViewMode::Edit {
                    return Ok(Progress::Wait);
                }
                if !common_toolbar(world)? { return Ok(Progress::Wait); }
                let doc = world.resource::<super::DocumentState>().current().unwrap();
                // 只准备键盘输入文本；实际修订仍由中央源码输入系统采用，不能直接写文档结果。
                let mut layout = UiLayout::parse_document(&doc.source)?;
                layout.root.width = Val::Percent(85.0);
                let mut source = layout.replace_document(&doc.source)?;
                if !source.ends_with(USER_CODE) {
                    source.push_str(USER_CODE);
                }
                type_in(world, run, window, |_: &DocumentSource| true, &source)
            }
            26 => {
                if !world.resource::<super::DocumentState>().current()
                    .is_some_and(|doc| doc.source.ends_with(USER_CODE)) {
                    return Ok(Progress::Wait);
                }
                let preview = visible(world, |action: &DocumentAction| *action == DocumentAction::Preview);
                click(world, &mut run.click, window, preview)
            }
            27 => {
                world.resource::<UiLayouts>().layout.as_ref()
                    .is_some_and(|layout| layout.root.width == Val::Percent(85.0))
                    && action(world, run, window, Action::Select("root".into()))
            }
            28 => type_in(world, run, window, |field: &Field| *field == Field::Width, "90%"),
            29 => action(world, run, window, Action::Properties),
            30 => {
                let docs = world.resource::<super::DocumentState>();
                if !docs.current().is_some_and(|doc| doc.source.ends_with(USER_CODE)) {
                    return Err("画布应用覆盖了宏外手写 Rust 代码".into());
                }
                world.resource::<UiLayouts>().layout.as_ref()
                    .is_some_and(|layout| layout.root.width == Val::Percent(90.0))
                    && public_action(world, run, window, EditorAction::Undo)
            }
            31 => {
                world.resource::<UiLayouts>().layout.as_ref()
                    .is_some_and(|layout| layout.root.width == Val::Percent(85.0))
                    && public_action(world, run, window, EditorAction::Save)
            }
            32 => {
                let doc = world.resource::<super::DocumentState>().current();
                if !doc.is_some_and(|doc| !doc.dirty() && doc.source.ends_with(USER_CODE)) {
                    return Ok(Progress::Wait);
                }
                let source = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
                source.ends_with(USER_CODE) && UiLayout::parse_document(&source).is_ok_and(|layout| {
                    edited(&layout) && layout.root.width == Val::Percent(85.0)
                })
            }
            _ => return modes_round_trip(world, run, window, 33),
        }
    };
    Ok(if advance {
        Progress::Next
    } else {
        Progress::Wait
    })
}

fn modes_round_trip(world: &mut World, run: &mut Run, window: Entity, first_stage: usize) -> Result<Progress, String> {
    let stage = run.stage - first_stage;
    let advance = match stage {
        0 => {
            if !common_toolbar(world)? { return Ok(Progress::Wait); }
            let Some(edit) = visible(world, |action: &Action| *action == Action::Preview(false))
                else { return Ok(Progress::Wait); };
            let Some(preview) = visible(world, |action: &Action| *action == Action::Preview(true))
                else { return Ok(Progress::Wait); };
            let parent = world.get::<ChildOf>(edit).map(|parent| parent.parent());
            if parent != world.get::<ChildOf>(preview).map(|parent| parent.parent())
                || !parent.and_then(|parent| world.get::<Children>(parent))
                    .is_some_and(|children| children.len() == 2 && children[0] == edit && children[1] == preview)
            {
                return Err("编辑与交互预览必须相邻，且位于同一个独立按钮组".into());
            }
            run.preview_document = document_snapshot(world);
            run.preview_document.is_some()
        }
        1 => action(world, run, window, Action::Preview(true)),
        2 | 4 => {
            if world.resource::<UiLayouts>().interactive != (stage == 2) {
                return Ok(Progress::Wait);
            }
            // 预览只改变交互模式，不能修改源码、内容修订、脏位或加入撤销步骤。
            if document_snapshot(world) != run.preview_document {
                return Err("编辑与交互预览切换产生了文档草稿或改变了历史".into());
            }
            if stage == 4 { run.modes_verified = true; }
            true
        }
        3 => action(world, run, window, Action::Preview(false)),
        _ => return photograph(world, run),
    };
    Ok(if advance { Progress::Next } else { Progress::Wait })
}

fn photograph(world: &mut World, run: &mut Run) -> Result<Progress, String> {
    if world
        .resource::<UiLayouts>()
        .layout
        .as_deref()
        .is_none_or(|layout| !edited(layout))
    {
        return Ok(Progress::Wait);
    }
    let name = run.work.join(format!("ui-bsn-{}.png", run.phase));
    Ok(evidence::capture(world, &mut run.capture, &name)?.map_or(Progress::Wait, Progress::Done))
}
