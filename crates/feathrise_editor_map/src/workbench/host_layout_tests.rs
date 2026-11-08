//! 无工程布局偏好保护：初次装配与作者交接先恢复新来源，再允许原布局输入和记忆写回。
use feathrise_editor_ui::InteractionButton;
use super::*;
use crate::{
    components::MapEditorLayoutButton,
    resources::MapEditorLayoutState,
    types::{EditorLayoutAction, EditorPane, EditorPaneDrag},
    ui::layout::{handle_editor_layout_input, remember_editor_layout_preferences},
    workbench::{WorkbenchSettingsAccess, WorkbenchUiSet},
};
use bevy::ui::InteractionDisabled;
use feathrise_editor_core::{EditorLayoutPreferences, EditorSettings};
use feathrise_editor_ui::EditorButtonClick;

fn app(preferences: EditorLayoutPreferences) -> (App, Entity) {
    // 复用真实无工程插件夹具；只补WorkBenchUiPlugin原本注册的同一布局输入/记忆链。
    let (mut app, _) = super::scale_tests::app(false);
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_layout = preferences;
    app.init_resource::<ButtonInput<MouseButton>>().add_systems(
        Update,
        (
            handle_editor_layout_input,
            remember_editor_layout_preferences,
        )
            .chain()
            .in_set(WorkbenchUiSet::Update),
    );
    let button = app
        .world_mut()
        .spawn((
            InteractionButton,
            MapEditorLayoutButton(EditorLayoutAction::Content),
            EditorButtonClick::default(),
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id();
    (app, button)
}

fn saved_layout() -> EditorLayoutPreferences {
    EditorLayoutPreferences {
        content_width: 312.0,
        inspector_width: 440.0,
        content_visible: true,
        inspector_visible: false,
    }
}

#[test]
fn host_layout_first_frame_restores_nondefault_preferences_before_remember() {
    let saved = saved_layout();
    let (mut app, _) = app(saved);
    app.update();
    assert_eq!(
        app.world().resource::<MapEditorLayoutState>().preferences(),
        saved
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_layout,
        saved,
        "首帧不能把默认布局覆写进已载入的作者偏好"
    );
}

#[test]
fn host_layout_handoff_blocks_old_gestures_and_restores_latest_preferences() {
    let saved = saved_layout();
    let (mut app, button) = app(saved);
    app.update();
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = false;
    {
        let mut layout = app.world_mut().resource_mut::<MapEditorLayoutState>();
        layout.content_width = 333.0;
        layout.drag = Some(EditorPaneDrag {
            pane: EditorPane::Content,
            origin_x: 100.0,
            origin_width: saved.content_width,
            moved: true,
        });
    }
    app.world_mut()
        .entity_mut(button)
        .insert(EditorButtonClick::triggered());
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_layout,
        saved
    );
    assert!(
        app.world()
            .resource::<MapEditorLayoutState>()
            .drag
            .is_none()
    );
    assert!(app.world().get::<InteractionDisabled>(button).is_some());
    assert!(
        !app.world()
            .get::<EditorButtonClick>(button)
            .unwrap()
            .is_clicked()
    );

    let latest = EditorLayoutPreferences {
        content_width: 350.0,
        inspector_width: 460.0,
        content_visible: false,
        inspector_visible: true,
    };
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_layout = latest;
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_layout,
        latest,
        "交接期间旧LayoutState不能反写覆盖作者最新结果"
    );
    app.world_mut()
        .entity_mut(button)
        .insert(EditorButtonClick::triggered());
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = true;
    app.update();
    assert_eq!(
        app.world().resource::<MapEditorLayoutState>().preferences(),
        latest
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_layout,
        latest
    );
    assert!(app.world().get::<InteractionDisabled>(button).is_none());

    app.world_mut()
        .entity_mut(button)
        .insert(EditorButtonClick::triggered());
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_layout,
        EditorLayoutPreferences {
            content_visible: true,
            ..latest
        },
        "恢复后的新布局动作应正常发布"
    );
}

#[test]
fn host_layout_initially_blocked_keeps_saved_source_until_first_editable_frame() {
    let saved = saved_layout();
    let (mut app, _) = app(saved);
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = false;
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_layout,
        saved
    );
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = true;
    app.update();
    assert_eq!(
        app.world().resource::<MapEditorLayoutState>().preferences(),
        saved
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_layout,
        saved
    );
}
