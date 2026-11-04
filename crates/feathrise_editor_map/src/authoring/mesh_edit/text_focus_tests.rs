//! 普通文字焦点与IME分开仲裁；正式网格状态机和对象共用输入门都必须保护原生文字控件。

use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    input_focus::{FocusCause, InputFocus},
    text::EditableText,
};
use crate::ui::text_input::{EditorTextInput, EditorTextInputGuard};

fn focus_field(app: &mut App, native_only: bool) -> Entity {
    app.init_resource::<InputFocus>();
    let field = if native_only {
        app.world_mut().spawn(EditableText::new("普通文字")).id()
    } else {
        app.world_mut().spawn(EditorTextInput).id()
    };
    app.world_mut().resource_mut::<InputFocus>().set(field, FocusCause::Navigated);
    assert!(!app.world().resource::<EditorTextInputGuard>().block_shortcuts,
        "本回归保护普通文本焦点，不能依赖IME候选门代替它");
    field
}

#[test]
fn marked_and_native_text_focus_block_shared_map_commands_but_keep_document_properties_available() {
    fn gates(
        context: crate::viewport::placement_move::MoveInputContext,
        editor: Res<MapEditorState>,
    ) -> (bool, bool) {
        (context.map_commands_available(&editor), context.document_available(&editor))
    }
    for native_only in [false, true] {
        let (mut app, _, _) = app();
        let field = focus_field(&mut app, native_only);
        assert_eq!(app.world_mut().run_system_once(gates).unwrap(), (false, true),
            "对象与建模快捷键都要让出文本输入；文档级数字属性仍需保持有效，native_only={native_only}");
        app.world_mut().resource_mut::<feathrise_editor_ui::ActiveNumericInput>().entity = Some(field);
        assert_eq!(app.world_mut().run_system_once(gates).unwrap(), (false, true),
            "属性框不能因持有自己的数字焦点而失去文档资格");
        app.world_mut().resource_mut::<feathrise_editor_ui::ActiveNumericInput>().entity = None;
        app.world_mut().resource_mut::<InputFocus>().clear();
        assert_eq!(app.world_mut().run_system_once(gates).unwrap(), (true, true));
    }
}

#[test]
fn marked_and_native_text_focus_reject_mesh_entry_and_new_modal_gestures_then_restore_after_clear() {
    for native_only in [false, true] {
        let (mut app, _, node) = app();
        let before = current(&app).clone();
        focus_field(&mut app, native_only);
        press(&mut app, KeyCode::Tab);
        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(!app.world().resource::<MeshEditState>().active(),
            "文本框中的Tab不能进入网格编辑，native_only={native_only}");

        app.world_mut().resource_mut::<InputFocus>().clear();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Tab);
        enter(&mut app, node);
        focus_field(&mut app, native_only);
        press(&mut app, KeyCode::KeyG);
        fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
        assert!(!app.world().resource::<MeshEditState>().active(),
            "文本获得焦点后结束旧网格会话，字符G不能启动移动，native_only={native_only}");
        assert!(!app.world().resource::<MeshEditState>().moving());
        assert_eq!(current(&app), &before);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(!app.world().resource::<MapEditorState>().dirty);

        app.world_mut().resource_mut::<InputFocus>().clear();
        fixture::step(&mut app, node, Vec2::ZERO);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Tab);
        enter(&mut app, node);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyG);
        press(&mut app, KeyCode::KeyG);
        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(app.world().resource::<MeshEditState>().moving(), "清焦后的新手势恢复");
    }
}

#[test]
fn marked_and_native_text_focus_cancel_unconfirmed_mesh_preview_without_publishing_history() {
    for native_only in [false, true] {
        let (mut app, root, node) = app();
        enter(&mut app, node);
        let before = current(&app).clone();
        request(&mut app, MeshEditAction::Move);
        fixture::step(&mut app, node, Vec2::ZERO);
        fixture::step(&mut app, node, Vec2::new(0.03, 0.01));
        let state = app.world().resource::<MeshEditState>();
        let ghost = state.ghost.unwrap();
        assert_ne!(state.movement.as_ref().unwrap().preview, before);
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));

        focus_field(&mut app, native_only);
        press(&mut app, KeyCode::Enter);
        fixture::step(&mut app, node, Vec2::new(0.04, 0.01));
        assert!(!app.world().resource::<MeshEditState>().active(),
            "文本框的Enter只能归还文字域，不能采用旧几何预览，native_only={native_only}");
        assert_eq!(current(&app), &before);
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
        assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden));
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(!app.world().resource::<MapEditorState>().dirty);
    }
}
