//! 正式GPU布局、中文候选、保存快捷键/工具条/右键菜单与实际磁盘结果；复用唯一原生正文。

use super::super::{
    components::{DocumentAction, DocumentSource, DocumentTabRoot},
    io,
    resources::DocumentState,
    types::*,
};
use super::{components::*, control, resources::DocumentBatchSaveState, types::FileOutcome};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, TextEdit},
};
use std::{path::Path, sync::Arc, time::Duration};

pub(in super::super) fn gpu_assert_batch_save(
    app: &mut App,
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
    width: u32,
    root: &Path,
    snapshot: fn(&mut App, Handle<Image>, &str, u32),
) {
    let source = source(app);
    click(app, window, camera, DocumentAction::Edit);
    set_source(app, source, "// 与替换无关的独立草稿\n");
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    app.world_mut().write_message(bevy::window::Ime::Preedit {
        window,
        value: "中文".into(),
        cursor: Some((6, 6)),
    });
    frames(app, 2);
    chord(app, window);
    assert_eq!(
        app.world().resource::<DocumentBatchSaveState>().ticket,
        0,
        "中文候选不能触发批量保存"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("基础文件.rs")).unwrap(),
        "fn main() {}\n"
    );
    app.world_mut().write_message(bevy::window::Ime::Commit {
        window,
        value: "中文".into(),
    });
    frames(app, 3);
    let independent = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .source
        .clone();

    // 范围由真实差异应用产生；单击保存本批应保留旁边无关草稿。
    super::super::search::replacement::gpu_prepare_save_batch(app, window, camera);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .filter(|doc| doc.dirty())
            .count(),
        4
    );
    super::super::search::replacement::gpu_click_save_batch(app, window, camera);
    settle(app);
    let report = app.world().resource::<DocumentBatchSaveState>();
    assert_eq!(report.files.len(), 3);
    assert!(
        report
            .files
            .iter()
            .all(|file| file.outcome == FileOutcome::Saved)
    );
    for path in ["config/武器.json", "scripts/剑.rs", "另一个.JSON"] {
        let bytes = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(bytes.contains("短剑"));
        assert!(!bytes.contains("长剑"));
    }
    assert!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .find(|doc| doc.path == "基础文件.rs")
            .unwrap()
            .dirty()
    );
    snapshot(
        app,
        target.clone(),
        "asset-browser-batch-save-scope.png",
        width,
    );

    // 后台草稿无需占标签；真实Ctrl+Shift+S在替换预览也保存全部，逐项失败后继续。
    let bad = document(root, "无效配置.json", "{}", "{\"中文\":}");
    let external = document(root, "外部修改.txt", "磁盘原文", "本地草稿");
    let last = document(root, "后续有效.md", "原文", "正常保存的后续文件");
    std::fs::write(root.join("外部修改.txt"), "外部新内容").unwrap();
    app.world_mut()
        .resource_mut::<DocumentState>()
        .documents
        .extend([bad, external, last]);
    chord(app, window);
    settle(app);
    let report = app.world().resource::<DocumentBatchSaveState>();
    assert_eq!(report.files.len(), 4);
    assert_eq!(
        report
            .files
            .iter()
            .filter(|file| file.outcome == FileOutcome::Saved)
            .count(),
        2
    );
    assert_eq!(
        report
            .files
            .iter()
            .filter(|file| file.outcome == FileOutcome::Failed)
            .count(),
        2
    );
    assert_eq!(
        std::fs::read_to_string(root.join("基础文件.rs")).unwrap(),
        independent.as_ref()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("无效配置.json")).unwrap(),
        "{}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("外部修改.txt")).unwrap(),
        "外部新内容"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("后续有效.md")).unwrap(),
        "正常保存的后续文件"
    );
    snapshot(
        app,
        target.clone(),
        "asset-browser-batch-save-failures.png",
        width,
    );
    assert_report_layout(app);

    // 修正使用同一官方输入：保存全部工具条必须采用本帧确认后的完整草稿。
    app.world_mut().resource_scope(|world, mut docs: Mut<DocumentState>| {
        docs.expand("无效配置.json".into(), &mut world.resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()).unwrap();
        docs.documents.iter_mut().find(|doc| doc.path == "无效配置.json").unwrap().mode = ViewMode::Edit;
    });
    frames(app, 5);
    assert_eq!(source, self::source(app));
    set_source(app, source, "{\"中文\":\"已修正\"}");
    click(app, window, camera, DocumentAction::SaveAll);
    settle(app);
    assert_eq!(
        std::fs::read_to_string(root.join("无效配置.json")).unwrap(),
        "{\"中文\":\"已修正\"}"
    );
    assert_eq!(
        app.world().resource::<DocumentBatchSaveState>().files.len(),
        2
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .history
            .can_undo()
    );

    // 菜单保存同帧独占鼠标，意图仍能抵达保存槽，标签与源码实体保持。
    set_source(app, source, "{\"中文\":\"菜单保存\"}");
    let position = {
        let world = app.world_mut();
        world
            .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<DocumentTabRoot>>()
            .iter(world)
            .find(|(node, _)| node.size().x > 0.0)
            .unwrap()
            .1
            .affine()
            .translation
    };
    super::super::super::tab_menu::gpu_open_menu(app, window, camera, position);
    frames(app, 2);
    snapshot(
        app,
        target.clone(),
        "asset-browser-batch-save-menu.png",
        width,
    );
    super::super::super::tab_menu::gpu_click_save_all(app, window, camera);
    settle(app);
    assert_eq!(
        std::fs::read_to_string(root.join("无效配置.json")).unwrap(),
        "{\"中文\":\"菜单保存\"}"
    );
    assert_eq!(self::source(app), source);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .find(|doc| doc.path == "外部修改.txt")
            .unwrap()
            .dirty()
    );
    snapshot(app, target, "asset-browser-batch-save-document.png", width);
}

fn document(root: &Path, path: &str, original: &str, draft: &str) -> Document {
    std::fs::write(root.join(path), original).unwrap();
    let (disk, text) = io::load(root, path).unwrap();
    let mut doc = Document {
        path: path.into(),
        format: crate::asset_catalog::document_format(path).unwrap(),
        generation: 1,
        disk,
        pages: text.pages,
        source: text.text,
        revision: 0,
        saved_revision: 0,
        history: default(),
        preview: Arc::new(Vec::new()),
        preview_revision: None,
        preview_clipped: false,
        mode: ViewMode::Edit,
        preview_scroll: Vec2::ZERO,
        confirm_reload: false,
        external: default(),
        status: String::new(),
    };
    doc.edit(draft);
    doc
}
fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
}
fn settle(app: &mut App) {
    for _ in 0..200 {
        frames(app, 1);
        if app.world().resource::<DocumentState>().io.is_none()
            && !app.world().resource::<DocumentBatchSaveState>().running
        {
            frames(app, 4);
            return;
        }
    }
    panic!("批量保存未结清");
}
fn source(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<DocumentSource>>()
        .single(world)
        .unwrap()
}
fn set_source(app: &mut App, source: Entity, value: &str) {
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    let mut input = app.world_mut().get_mut::<EditableText>(source).unwrap();
    input.queue_edit(TextEdit::SelectAll);
    input.queue_edit(TextEdit::Insert(value.into()));
    frames(app, 3);
}
fn chord(app: &mut App, window: Entity) {
    let keys = [
        (KeyCode::ControlLeft, Key::Control),
        (KeyCode::ShiftLeft, Key::Shift),
        (KeyCode::KeyS, Key::Character("s".into())),
    ];
    for state in [ButtonState::Pressed, ButtonState::Released] {
        for (code, logical) in &keys {
            app.world_mut().write_message(KeyboardInput {
                key_code: *code,
                logical_key: logical.clone(),
                state,
                text: None,
                repeat: false,
                window,
            });
        }
        frames(app, 2);
    }
}
fn click(app: &mut App, window: Entity, camera: Entity, action: DocumentAction) {
    let (entity, position) = {
        let world = app.world_mut();
        world
            .query::<(Entity, &DocumentAction, &UiGlobalTransform, &ComputedNode)>()
            .iter(world)
            .find(|(_, candidate, _, node)| {
                **candidate == action && node.size().cmpgt(Vec2::ZERO).all()
            })
            .map(|(entity, _, transform, _)| (entity, transform.affine().translation))
            .unwrap()
    };
    let target = app
        .world()
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert_eq!(
        *app.world().get::<Interaction>(entity).unwrap(),
        Interaction::Pressed,
        "实际鼠标命中：{action:?}"
    );
    app.world_mut()
        .run_system_once(super::super::interaction::actions)
        .unwrap();
    app.world_mut().run_system_once(control::input).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    app.world_mut().run_system_once(super::super::interaction::actions).unwrap();
    app.world_mut().run_system_once(control::input).unwrap();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    frames(app, 3);
}
fn assert_report_layout(app: &mut App) {
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<SaveReport>>()
            .iter(world)
            .count(),
        2
    );
    let visible: Vec<_> = world
        .query_filtered::<&ComputedNode, With<SaveReportScroll>>()
        .iter(world)
        .filter(|node| node.size().y > 0.0)
        .collect();
    assert!(visible.is_empty(), "保存报告统一进入底栏，正文不应残留旧卡片或空滚动框");
    assert!(
        world
            .query_filtered::<(&Text, &feathrise_editor_ui::PanelStatusText), With<SaveReportBody>>()
            .iter(world)
            .all(|(text, feedback)| text.as_str().contains("[失败]") && text.as_str().contains("[成功]")
                && feedback.0 == "批量保存详情")
    );
}
