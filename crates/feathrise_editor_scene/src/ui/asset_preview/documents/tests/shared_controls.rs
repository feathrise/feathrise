//! 在独占文件上验证公共保存、无场景可见性和文件身份换绑。

use super::*;
use super::super::{commands, interaction};
use crate::{components::SceneEditorActionButton, resources::SceneEditorState, types::EditorAction};
use crate::ui::{sidebars::SceneEditorSidebarState, vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState}};
use feathrise_editor_ui::EditorButtonClick;

fn shared_app(fixture: &Fixture) -> (App, Entity) {
    let mut app = app(fixture);
    {
        let mut docs = app.world_mut().resource_mut::<DocumentState>();
        docs.documents = vec![fixture.document_at("layout.rs")];
        docs.active = Some("layout.rs".into());
        docs.expanded = Some(ExpandedDocument { id: 1, path: "layout.rs".into(), return_tab: PresetWorkspaceTab::Scene });
        docs.central_active = true;
        docs.visible = true;
        docs.current_mut().unwrap().mode = ViewMode::Preview;
        docs.current_mut().unwrap().edit("// feathrise-ui: editable\n// 新内容\n");
    }
    let mut workspace = VegetationPresetEditorState::default();
    workspace.active_tab = PresetWorkspaceTab::Document;
    app.insert_resource(SceneEditorState { open: true, ..default() })
        .insert_resource(workspace)
        .init_resource::<SceneEditorSidebarState>()
        .init_resource::<crate::resources::EditorTextInputGuard>()
        .init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_systems(Update, (commands::bind, interaction::actions).chain().before(session::drive_tasks))
        .add_systems(Update, crate::ui::world_session::sync_editor_world_controls.after(session::drive_tasks));
    let save = app.world_mut().spawn((SceneEditorActionButton(EditorAction::Save), EditorButtonClick::default(), Node::default())).id();
    app.update();
    (app, save)
}

#[test]
fn shared_ui_save_uses_existing_document_io_and_stays_visible_without_scene() {
    let fixture = Fixture::new();
    fixture.write("unused");
    std::fs::write(fixture.root.join("layout.rs"), "// feathrise-ui: editable\n// 原文\n").unwrap();
    let (mut app, save) = shared_app(&fixture);
    assert_eq!(app.world().get::<Node>(save).unwrap().display, Display::Flex);
    app.world_mut().entity_mut(save).insert(EditorButtonClick::triggered());
    app.update();
    settle(&mut app);
    assert_eq!(std::fs::read_to_string(fixture.root.join("layout.rs")).unwrap(), "// feathrise-ui: editable\n// 新内容\n");
    assert!(!app.world().resource::<DocumentState>().current().unwrap().dirty());
    let scene = app.world().resource::<SceneEditorState>();
    assert!(!scene.save_requested && !scene.undo_requested && !scene.redo_requested);
}

#[test]
fn shared_ui_save_discards_old_tab_click_instead_of_saving_new_document() {
    let fixture = Fixture::new();
    fixture.write("unused");
    let original = "// feathrise-ui: editable\n// 原文\n";
    for path in ["layout.rs", "second.rs"] { std::fs::write(fixture.root.join(path), original).unwrap(); }
    let (mut app, save) = shared_app(&fixture);
    {
        let mut docs = app.world_mut().resource_mut::<DocumentState>();
        let mut second = fixture.document_at("second.rs");
        second.edit("// feathrise-ui: editable\n// 第二份草稿\n");
        docs.documents.push(second);
        docs.active = Some("second.rs".into());
        docs.expanded.as_mut().unwrap().path = "second.rs".into();
    }
    app.world_mut().entity_mut(save).insert(EditorButtonClick::triggered());
    app.update();
    assert!(!app.world().get::<EditorButtonClick>(save).unwrap().is_clicked());
    assert!(app.world().resource::<DocumentState>().io.is_none());
    assert_eq!(std::fs::read_to_string(fixture.root.join("second.rs")).unwrap(), original);
    app.world_mut().entity_mut(save).insert(EditorButtonClick::triggered());
    app.update();
    settle(&mut app);
    assert_eq!(std::fs::read_to_string(fixture.root.join("second.rs")).unwrap(), "// feathrise-ui: editable\n// 第二份草稿\n");
    assert_eq!(std::fs::read_to_string(fixture.root.join("layout.rs")).unwrap(), original);
}

#[test]
fn shared_ui_save_waits_for_property_paste_and_queued_edits_in_both_modes() {
    for mode in [ViewMode::Preview, ViewMode::Edit] {
        for paste in [false, true] {
            let fixture = Fixture::new();
            fixture.write("unused");
            let original = "// feathrise-ui: editable\n// 原文\n";
            std::fs::write(fixture.root.join("layout.rs"), original).unwrap();
            let (mut app, save) = shared_app(&fixture);
            app.world_mut().resource_mut::<DocumentState>().current_mut().unwrap().mode = mode;
            let before_status = app.world().resource::<DocumentState>().current().unwrap().status.clone();
            let mut text = EditableText::new("100%");
            if paste {
                text.pending_paste = Some(bevy::clipboard::ClipboardRead::Ready(Ok("80%".into())));
            } else {
                text.queue_edit(bevy::text::TextEdit::Insert("80%".into()));
            }
            let field = app.world_mut().spawn((super::super::ui_layout::Field::Width, text)).id();
            app.world_mut().entity_mut(save).insert(EditorButtonClick::triggered());
            app.update();
            assert!(app.world().resource::<DocumentState>().io.is_none());
            assert_eq!(app.world().resource::<DocumentState>().current().unwrap().status, before_status,
                "拒收属性输入期间的保存不得用文档状态变更触发面板重建");
            assert_eq!(std::fs::read_to_string(fixture.root.join("layout.rs")).unwrap(), original);
            {
                let mut text = app.world_mut().get_mut::<EditableText>(field).unwrap();
                assert!(text.pending_paste.is_some() || !text.pending_edits.is_empty());
                text.pending_paste = None;
                text.pending_edits.clear();
            }
            app.update();
            assert!(app.world().resource::<DocumentState>().io.is_none());
            assert_eq!(std::fs::read_to_string(fixture.root.join("layout.rs")).unwrap(), original,
                "输入完成后不能补发已经拒收的旧保存");
            app.world_mut().entity_mut(save).insert(EditorButtonClick::triggered());
            app.update();
            settle(&mut app);
            assert!(!app.world().resource::<DocumentState>().current().unwrap().dirty());
            assert_eq!(std::fs::read_to_string(fixture.root.join("layout.rs")).unwrap(),
                "// feathrise-ui: editable\n// 新内容\n");
        }
    }
}
