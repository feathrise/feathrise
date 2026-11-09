//! 正式字体/GPU/鼠标命中验证外部联动，不打开用户的 VS Code 或资源管理器。

use feathrise_editor_ui::UiInteraction;
use super::*;
use bevy::{ecs::system::RunSystemOnce, input_focus::{FocusCause, InputFocus}, text::TextEdit};
use crate::ui::external_files::{ExternalFileAction, ExternalFileTab, ExternalRequestProbe};
use crate::ui::asset_context_menu::{AssetPathCopyKind, CopyAssetPath};

#[derive(Resource, Default)]
struct CopiedPathProbe(Vec<(String, AssetPathCopyKind)>);
fn collect_paths(mut requests: MessageReader<CopyAssetPath>, mut copied: ResMut<CopiedPathProbe>) {
    copied.0.extend(requests.read().map(|request| (request.path.clone(), request.kind))); // 只观察真实点击意图，不修改系统剪贴板。
}

pub(super) fn gpu_assert_external(
    app: &mut App, window: Entity, camera: Entity, target: Handle<Image>, width: u32,
    root: &std::path::Path, path: &str, snapshot: fn(&mut App, Handle<Image>, &str, u32),
) {
    crate::ui::external_files::install_probe(app);
    app.init_resource::<CopiedPathProbe>().add_systems(Update, collect_paths.after(crate::MapEditorSystemSet::Input));
    let source = {
        let world = app.world_mut(); world.query_filtered::<Entity, With<DocumentSource>>().single(world).unwrap()
    };
    click_action(app, window, camera, DocumentAction::Edit);
    let previous_key = app.world().get::<DocumentSource>(source).unwrap().key.clone();
    std::fs::write(root.join(path), "// 外部干净修改\nfn main() { let 值 = 2; }\n").unwrap();
    return_to_editor(app, window);
    assert_eq!(app.world().get::<EditableText>(source).unwrap().value(), "// 外部干净修改\nfn main() { let 值 = 2; }\n");
    assert_ne!(app.world().get::<DocumentSource>(source).unwrap().key, previous_key);
    app.world_mut().resource_mut::<InputFocus>().set(source, FocusCause::Navigated);
    {
        let mut input = app.world_mut().get_mut::<EditableText>(source).unwrap();
        input.queue_edit(TextEdit::SelectAll); input.queue_edit(TextEdit::Insert("// 内置草稿\nfn main() {}\n".into()));
    }
    frames(app, 4);
    assert!(app.world().resource::<DocumentState>().current().unwrap().dirty());
    std::fs::write(root.join(path), "// 外部第二版\nfn main() { }\n").unwrap();
    return_to_editor(app, window);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(doc.external.warning && doc.dirty());
    assert_eq!(doc.source_text(), "// 内置草稿\nfn main() {}\n");
    let notice = {
        let world = app.world_mut(); world.query_filtered::<&Node, With<DocumentExternalNotice>>().single(world).unwrap().display
    };
    assert_eq!(notice, Display::Flex);
    snapshot(app, target.clone(), "asset-browser-external-draft-conflict.png", width);
    click_action(app, window, camera, DocumentAction::KeepDraft);
    assert!(!app.world().resource::<DocumentState>().current().unwrap().external.warning);
    assert!(app.world().resource::<DocumentState>().current().unwrap().dirty());
    std::fs::write(root.join(path), "// 外部第三版\nfn main() { let 值 = 3; }\n").unwrap();
    return_to_editor(app, window);
    assert!(app.world().resource::<DocumentState>().current().unwrap().external.warning);
    click_action(app, window, camera, DocumentAction::Reload);
    assert!(app.world().resource::<DocumentState>().current().unwrap().confirm_reload);
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "// 内置草稿\nfn main() {}\n");
    click_action(app, window, camera, DocumentAction::Reload);
    settle(app);
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "// 外部第三版\nfn main() { let 值 = 3; }\n");
    assert!(!app.world().resource::<DocumentState>().current().unwrap().dirty());

    let position = {
        let world = app.world_mut(); world.query::<(&ExternalFileTab, &DocumentAction, &UiGlobalTransform)>().iter(world)
            .find(|(file, action, _)| file.0 == path && matches!(action, DocumentAction::ActivateTab(_)))
            .unwrap().2.affine().translation
    };
    super::super::super::tab_menu::gpu_open_menu(app, window, camera, position);
    frames(app, 3);
    snapshot(app, target.clone(), "asset-browser-external-file-menu.png", width);
    for kind in AssetPathCopyKind::ALL {
        super::super::super::tab_menu::gpu_click_copy_path(app, window, camera, kind);
        frames(app, 3);
        assert_eq!(app.world().resource::<CopiedPathProbe>().0.last().unwrap(), &(path.to_owned(), kind));
        assert!(!app.world().resource::<DocumentState>().current().unwrap().dirty());
        super::super::super::tab_menu::gpu_open_menu(app, window, camera, position);
        frames(app, 3);
    }
    assert_eq!(app.world().resource::<CopiedPathProbe>().0.len(), 2);
    super::super::super::tab_menu::gpu_click_external(app, window, camera, ExternalFileAction::VsCode(None));
    frames(app, 3);
    assert_eq!(app.world().resource::<ExternalRequestProbe>().0.len(), 1);
    for action in [ExternalFileAction::VsCodeProject(None), ExternalFileAction::Zed(None), ExternalFileAction::ZedProject(None),
        ExternalFileAction::RustRover(None), ExternalFileAction::RustRoverProject(None)] {
        super::super::super::tab_menu::gpu_open_menu(app, window, camera, position);
        frames(app, 3);
        super::super::super::tab_menu::gpu_click_external(app, window, camera, action);
        frames(app, 3);
        let probe = app.world().resource::<ExternalRequestProbe>();
        let request = probe.0.last().unwrap();
        assert_eq!(request.path, path);
        assert_eq!(request.action, action);
        assert!(!request.has_draft);
    }
    let probe = app.world().resource::<ExternalRequestProbe>();
    assert_eq!(probe.0.len(), 6);
    app.insert_resource(crate::ui::external_files::ExternalEditors::for_tests(&[crate::ui::external_files::ExternalEditor::Zed]));
    super::super::super::tab_menu::gpu_open_menu(app, window, camera, position);
    frames(app, 3);
    super::super::super::tab_menu::gpu_assert_external_visibility(app, &[crate::ui::external_files::ExternalEditor::Zed]);
    snapshot(app, target.clone(), "asset-browser-external-installed-only.png", width);
    super::super::super::tab_menu::gpu_click_external(app, window, camera, ExternalFileAction::Zed(None));
    frames(app, 3);
    let count = { let world = app.world_mut(); world.query_filtered::<Entity, With<DocumentSource>>().iter(world).count() };
    assert_eq!(count, 1);
    snapshot(app, target, "asset-browser-external-reloaded.png", width);
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count { app.update(); std::thread::sleep(std::time::Duration::from_millis(16)); }
}
fn settle(app: &mut App) {
    for _ in 0..200 {
        frames(app, 1);
        let state = app.world().resource::<DocumentState>();
        if state.io.is_none() && state.parser.is_none() && state.pending_load.is_none()
            && state.external.ready.is_none() && !state.external.requested { frames(app, 3); return; }
    }
    panic!("外部内容核对未完成");
}
fn return_to_editor(app: &mut App, window: Entity) {
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false; frames(app, 1);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true; settle(app);
}
fn click_action(app: &mut App, window: Entity, camera: Entity, action: DocumentAction) {
    frames(app, 3);
    let (button, position) = {
        let world = app.world_mut(); world.query::<(Entity, &DocumentAction, &UiGlobalTransform, &ComputedNode)>().iter(world)
            .find(|(_, candidate, _, node)| **candidate == action && node.size().cmpgt(Vec2::ZERO).all())
            .map(|(entity, _, transform, _)| (entity, transform.affine().translation)).unwrap()
    };
    let target = app.world().get::<bevy::camera::RenderTarget>(camera).unwrap().clone();
    app.world_mut().get_mut::<Window>(window).unwrap().set_physical_cursor_position(Some(position.as_dvec2()));
    *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert_eq!(*app.world().get::<UiInteraction>(button).unwrap(), UiInteraction::Pressed);
    app.world_mut().run_system_once(super::super::interaction::actions).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    app.world_mut().run_system_once(super::super::interaction::actions).unwrap();
    *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = target;
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    frames(app, 3);
}
