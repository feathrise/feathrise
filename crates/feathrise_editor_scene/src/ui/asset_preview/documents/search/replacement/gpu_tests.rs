//! 正式中央替换预览、中文输入与真实鼠标回归；共享已有离屏文档夹具。

use feathrise_editor_ui::UiInteraction;
use super::super::super::resources::DocumentState;
use super::super::{
    components::{SearchAction, SearchField},
    gpu_tests as search_gpu,
};
use super::{components::*, constants::*, control, resources::AssetReplaceState};
use bevy::{
    ecs::system::RunSystemOnce,
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, TextEdit},
};
use std::path::Path;

pub(in super::super::super) fn gpu_assert_cross_replacement(
    app: &mut App,
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
    width: u32,
    root: &Path,
    snapshot: fn(&mut App, Handle<Image>, &str, u32),
) {
    let source = search_gpu::source(app);
    let originals: Vec<_> = ["config/武器.json", "scripts/剑.rs", "另一个.JSON"]
        .iter()
        .map(|path| (path.to_string(), std::fs::read(root.join(path)).unwrap()))
        .collect();
    search_gpu::chord(app, window, KeyCode::KeyF, true);
    let query = search_gpu::field(app, SearchField::Query);
    search_gpu::set_field(app, query, "长剑");
    search_gpu::chord(app, window, KeyCode::Enter, false);
    search_gpu::settle(app);
    let field = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<ReplaceField>>()
            .single(world)
            .unwrap()
    };
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(field, FocusCause::Navigated);
    app.world_mut().write_message(bevy::window::Ime::Preedit {
        window,
        value: "短剑".into(),
        cursor: Some((6, 6)),
    });
    search_gpu::frames(app, 2);
    search_gpu::chord(app, window, KeyCode::Enter, false);
    assert!(
        !app.world().resource::<AssetReplaceState>().tab_open,
        "候选Enter不能打开或应用预览"
    );
    app.world_mut().write_message(bevy::window::Ime::Commit {
        window,
        value: "短剑".into(),
    });
    search_gpu::frames(app, 3);
    assert_eq!(
        app.world().get::<EditableText>(field).unwrap().value(),
        "短剑"
    );
    search_gpu::chord(app, window, KeyCode::KeyH, true);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<AssetReplaceState>()
            .current()
            .unwrap()
            .files
            .len(),
        3
    );
    assert_layout(app);
    snapshot(
        app,
        target.clone(),
        "asset-browser-cross-replace-diff.png",
        width,
    );

    // 长差异使用真正滚轮消息，行槽换身份后仍能点击末端匹配；外栏不跟着滚。
    let ticket = app.world().resource::<AssetReplaceState>().preview_ticket;
    click(
        app,
        window,
        camera,
        ReplaceAction::File { ticket, index: 1 },
    );
    let view = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<ReplaceDiff>>()
            .single(world)
            .unwrap()
    };
    let outer = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, (
                With<crate::components::SceneEditorRightPanelScroll>,
                With<crate::ui::sidebars::SceneEditorSidebarPage>,
            )>()
            .iter(world)
            .find(|entity| world.get::<Node>(*entity).unwrap().display != Display::None)
            .unwrap()
    };
    let outer_before = app.world().get::<ScrollPosition>(outer).unwrap().y;
    app.world_mut()
        .get_mut::<bevy::ui::RelativeCursorPosition>(view)
        .unwrap()
        .cursor_over = true;
    app.world_mut()
        .write_message(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Pixel,
            x: 0.0,
            y: -240.0,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    app.world_mut().run_system_once(control::scroll).unwrap();
    assert!(app.world().resource::<AssetReplaceState>().owns_wheel());
    app.world_mut()
        .init_resource::<crate::resources::SceneBrowserDragState>();
    app.world_mut()
        .run_system_once(crate::ui::systems::scroll_editor_side_panels)
        .unwrap();
    assert_eq!(
        app.world().get::<ScrollPosition>(outer).unwrap().y,
        outer_before
    );
    search_gpu::frames(app, 3);
    click(
        app,
        window,
        camera,
        ReplaceAction::Hit { ticket, index: 35 },
    );
    assert_eq!(app.world().resource::<AssetReplaceState>().hit, 35);
    click(
        app,
        window,
        camera,
        ReplaceAction::File { ticket, index: 0 },
    );
    assert_layout(app);

    // 勾选只改变预览请求；候选与差异始终复用固定行槽。
    let ticket = app.world().resource::<AssetReplaceState>().preview_ticket;
    click(
        app,
        window,
        camera,
        ReplaceAction::Check { ticket, index: 1 },
    );
    click(app, window, camera, ReplaceAction::Generate);
    settle(app);
    assert_eq!(app.world().resource::<AssetReplaceState>().checked, [0, 2]);
    let ticket = app.world().resource::<AssetReplaceState>().preview_ticket;
    click(app, window, camera, ReplaceAction::Hit { ticket, index: 1 });
    click(app, window, camera, ReplaceAction::ApplyHit);
    settle(app);
    {
        let docs = app.world().resource::<DocumentState>();
        let doc = docs
            .documents
            .iter()
            .find(|doc| doc.path == "config/武器.json")
            .unwrap();
        assert!(doc.source_text().contains("\"名称\": \"长剑\""));
        assert!(doc.source_text().contains("\"说明\": \"短剑\""));
        assert_eq!(docs.documents.iter().filter(|doc| doc.dirty()).count(), 1);
    }
    assert_disk(root, &originals);
    click(app, window, camera, ReplaceAction::OpenFile);
    settle(app);
    let selected = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    assert_eq!(
        &app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source[selected],
        "短剑"
    );
    click(app, window, camera, ReplaceAction::Activate);
    click(app, window, camera, ReplaceAction::Undo);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .all(|doc| !doc.dirty())
    );
    click(app, window, camera, ReplaceAction::Redo);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .any(|doc| doc.dirty())
    );
    click(app, window, camera, ReplaceAction::Undo);
    refresh(app, window, camera);
    click(app, window, camera, ReplaceAction::ApplyFile);
    settle(app);
    {
        let docs = app.world().resource::<DocumentState>();
        assert_eq!(docs.documents.iter().filter(|doc| doc.dirty()).count(), 1);
        assert!(
            !docs
                .documents
                .iter()
                .find(|doc| doc.path == "config/武器.json")
                .unwrap()
                .source_text()
                .contains("长剑")
        );
    }
    click(app, window, camera, ReplaceAction::Undo);
    refresh(app, window, camera);
    click(app, window, camera, ReplaceAction::ApplyChecked);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .filter(|doc| doc.dirty())
            .count(),
        3
    );
    assert_disk(root, &originals);
    let ticket = app.world().resource::<AssetReplaceState>().preview_ticket;
    click(
        app,
        window,
        camera,
        ReplaceAction::File { ticket, index: 1 },
    );
    click(app, window, camera, ReplaceAction::OpenFile);
    settle(app);
    {
        let doc = app.world().resource::<DocumentState>().current().unwrap();
        assert_eq!(doc.path, "scripts/剑.rs");
        assert_eq!(
            app.world().get::<EditableText>(source).unwrap().value(),
            doc.source_text()
        );
    }
    snapshot(
        app,
        target.clone(),
        "asset-browser-cross-replace-drafts.png",
        width,
    );
    assert_eq!(search_gpu::source(app), source);
    // 后续手工编辑必须停止整批撤销，不能只恢复其他文件或丢掉新草稿。
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::Insert("草稿".into()));
    search_gpu::frames(app, 4);
    let revisions = revisions(app);
    click(app, window, camera, ReplaceAction::Activate);
    click(app, window, camera, ReplaceAction::Undo);
    assert!(
        app.world()
            .resource::<AssetReplaceState>()
            .hint
            .contains("后续编辑")
    );
    assert_eq!(revisions, self::revisions(app));
    search_gpu::set_field(app, query, "短剑");
    search_gpu::click(app, window, camera, SearchAction::Submit);
    search_gpu::settle(app);
    search_gpu::set_field(app, field, "新剑");
    click(app, window, camera, ReplaceAction::Open);
    settle(app);
    std::fs::write(root.join("另一个.JSON"), "{\"外部\":\"短剑\"}").unwrap();
    click(app, window, camera, ReplaceAction::ApplyChecked);
    settle(app);
    assert!(
        app.world()
            .resource::<AssetReplaceState>()
            .hint
            .contains("已变化")
    );
    assert_eq!(revisions, self::revisions(app));
    assert_eq!(
        std::fs::read(root.join("config/武器.json")).unwrap(),
        originals[0].1
    );
    assert_eq!(
        std::fs::read(root.join("scripts/剑.rs")).unwrap(),
        originals[1].1
    );
    assert_eq!(search_gpu::source(app), source);
    snapshot(
        app,
        target.clone(),
        "asset-browser-cross-replace-conflict.png",
        width,
    );

    // 重现用户“b换成c”的上限场景：真正达到1024条后，按钮仍可点击，只改列出项。
    let maximum = super::super::constants::MAX_HITS;
    let original = "b\n".repeat(maximum + 5);
    std::fs::write(root.join("超出显示上限.txt"), &original).unwrap();
    search_gpu::set_field(app, query, "b");
    search_gpu::click(app, window, camera, SearchAction::Submit);
    search_gpu::settle(app);
    assert!(
        app.world()
            .resource::<super::super::resources::AssetSearchState>()
            .current()
            .unwrap()
            .limited
    );
    search_gpu::set_field(app, field, "c");
    search_gpu::chord(app, window, KeyCode::KeyH, true);
    settle(app);
    {
        let state = app.world().resource::<AssetReplaceState>();
        assert!(state.current().unwrap().search.limited);
        assert_eq!(state.current().unwrap().files[0].after.len(), maximum);
        assert!(state.hint.contains("仅处理已列出匹配"));
    }
    assert_layout(app);
    snapshot(
        app,
        target,
        "asset-browser-cross-replace-limited.png",
        width,
    );
    click(app, window, camera, ReplaceAction::ApplyHit);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .find(|doc| doc.path == "超出显示上限.txt")
            .unwrap()
            .source_text(),
        format!("c\n{}", "b\n".repeat(maximum + 4))
    );
    click(app, window, camera, ReplaceAction::Undo);
    for action in [ReplaceAction::ApplyFile, ReplaceAction::ApplyChecked] {
        refresh(app, window, camera);
        click(app, window, camera, action);
        settle(app);
        assert_eq!(
            app.world()
                .resource::<DocumentState>()
                .documents
                .iter()
                .find(|doc| doc.path == "超出显示上限.txt")
                .unwrap()
                .source_text(),
            format!("{}{}", "c\n".repeat(maximum), "b\n".repeat(5))
        );
        assert_eq!(
            std::fs::read_to_string(root.join("超出显示上限.txt")).unwrap(),
            original
        );
        click(app, window, camera, ReplaceAction::Undo);
    }
    assert_eq!(search_gpu::source(app), source);
}

fn revisions(app: &App) -> Vec<u64> {
    app.world()
        .resource::<DocumentState>()
        .documents
        .iter()
        .map(|doc| doc.revision)
        .collect()
}

/// 批量保存夹具从正式搜索与差异按钮产生真实最近一批，不能伪造保存范围。
pub(in super::super::super) fn gpu_prepare_save_batch(app: &mut App, window: Entity, camera: Entity) {
    search_gpu::chord(app, window, KeyCode::KeyF, true);
    let query = search_gpu::field(app, SearchField::Query);
    search_gpu::set_field(app, query, "长剑");
    search_gpu::chord(app, window, KeyCode::Enter, false);
    search_gpu::settle(app);
    let field = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<ReplaceField>>().single(world).unwrap()
    };
    search_gpu::set_field(app, field, "短剑");
    click(app, window, camera, ReplaceAction::Open);
    settle(app);
    click(app, window, camera, ReplaceAction::ApplyChecked);
    settle(app);
}

pub(in super::super::super) fn gpu_click_save_batch(app: &mut App, window: Entity, camera: Entity) {
    click(app, window, camera, ReplaceAction::SaveBatch);
    settle(app);
}
fn refresh(app: &mut App, window: Entity, camera: Entity) {
    search_gpu::click(app, window, camera, SearchAction::Submit);
    search_gpu::settle(app);
    click(app, window, camera, ReplaceAction::Open);
    settle(app);
}
fn assert_disk(root: &Path, originals: &[(String, Vec<u8>)]) {
    for (path, bytes) in originals {
        assert_eq!(&std::fs::read(root.join(path)).unwrap(), bytes);
    }
}
fn settle(app: &mut App) {
    for _ in 0..200 {
        search_gpu::frames(app, 1);
        let state = app.world().resource::<AssetReplaceState>();
        let docs = app.world().resource::<DocumentState>();
        if state.task.is_none()
            && state.pending.is_none()
            && docs.io.is_none()
            && docs.pending_load.is_none()
        {
            search_gpu::frames(app, 4);
            return;
        }
    }
    panic!("中央替换预览未完成");
}
fn assert_layout(app: &mut App) {
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<ReplaceFileRow>>()
            .iter(world)
            .count(),
        FILE_SLOTS
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<ReplaceDiffRow>>()
            .iter(world)
            .count(),
        DIFF_SLOTS
    );
    let panel = world
        .query_filtered::<&ComputedNode, With<ReplacePanel>>()
        .single(world)
        .unwrap();
    assert!(panel.size().x > 800.0 && panel.size().y > 600.0);
    for (action, node, transform) in world
        .query::<(&ReplaceAction, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
        .filter(|(action, _, _)| {
            matches!(
                action,
                ReplaceAction::ApplyHit
                    | ReplaceAction::ApplyFile
                    | ReplaceAction::ApplyChecked
                    | ReplaceAction::Undo
                    | ReplaceAction::Redo
            )
        })
    {
        assert!(
            node.size().x > 20.0 && node.size().y > 0.0,
            "按钮必须可见：{action:?}"
        );
        let center = transform.affine().translation;
        assert!(center.x >= 0.0 && center.y >= 0.0 && center.x <= 1920.0 && center.y <= 1080.0);
    }
}
fn click(app: &mut App, window: Entity, camera: Entity, action: ReplaceAction) {
    let (entity, position) = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ReplaceAction, &UiGlobalTransform, &ComputedNode)>()
            .iter(world)
            .find(|(_, candidate, _, node)| {
                **candidate == action && node.size().cmpgt(Vec2::ZERO).all()
            })
            .map(|(entity, _, transform, _)| (entity, transform.affine().translation))
            .unwrap_or_else(|| panic!("未找到按钮：{action:?}"))
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
        *app.world().get::<UiInteraction>(entity).unwrap(),
        UiInteraction::Pressed,
        "真实鼠标未命中：{action:?}"
    );
    app.world_mut().run_system_once(control::input).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    app.world_mut().run_system_once(super::control::input).unwrap();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    search_gpu::frames(app, 3);
}
