//! 独立作者窗口的唯一系统光标写侧；跟随刷圈隐藏指针，但绝不抓取或锁定鼠标。

use crate::{game_authoring::EditorCursorPresentation, resources::SceneEditorState};
use bevy::{prelude::*, window::{CursorGrabMode, CursorOptions, PrimaryWindow}};
use feathrise_scene_core::input::{EditorInputState, GameCursorSystemSet, GamePauseSystemSet, InputGateState};
use feathrise_scene_core::residency::SceneResidencyActivity;
use feathrise_editor_core::{EditorHostOverlay, SceneEditorSystemSet};

pub(super) fn install(app: &mut App) {
    app.add_systems(Update, apply_cursor_options
        .in_set(GameCursorSystemSet::Apply)
        .after(GamePauseSystemSet::Apply)
        .after(SceneEditorSystemSet::Cursor)
        .after(EditorCursorPresentation));
}

fn apply_cursor_options(
    editor: Option<Res<SceneEditorState>>,
    intent: Option<Res<EditorInputState>>,
    gate: Option<Res<InputGateState>>,
    overlay: Option<Res<EditorHostOverlay>>,
    residency: Option<Res<SceneResidencyActivity>>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    // 场景交接可暂停Cursor发布；关闭、浮层和失焦不能继续相信上一帧留下的刷圈意图。
    let custom = editor.as_deref().is_some_and(|editor| {
        editor.open && !editor.close_requested && !editor.new_scene_dialog_open
            && !editor.scene_transition_in_progress
    }) && intent.as_deref().is_some_and(|intent| {
        intent.scene_editor_open && intent.scene_editor_custom_cursor_visible
    }) && gate.as_deref().is_some_and(|gate| !gate.system_cursor_required)
        && !overlay.as_deref().is_some_and(|overlay| overlay.active)
        && !residency.as_deref().is_some_and(|residency| residency.switching);
    for (window, mut cursor) in &mut windows {
        let visible = !custom || !window.focused || !window.visible;
        if cursor.visible != visible { cursor.visible = visible; }
        // 作者视口使用绝对坐标；即使隐藏系统指针，也必须允许自然离开视口、Alt+Tab和关闭。
        if cursor.grab_mode != CursorGrabMode::None { cursor.grab_mode = CursorGrabMode::None; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        components::SceneEditorViewport,
        resources::{SceneBrowserDragState, SceneBrowserState, SceneDeletionTask},
        types::{EditorTool, EditorView},
    };
    use bevy::ui::RelativeCursorPosition;

    fn fixture() -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(SceneEditorState { open: true, tool: EditorTool::Water,
                view: EditorView::Top, brush_cursor_world: Some(Vec3::ZERO), ..default() })
            .insert_resource(EditorInputState { scene_editor_open: true, scene_editor_custom_cursor_visible: true })
            .init_resource::<InputGateState>()
            .init_resource::<EditorHostOverlay>()
            .init_resource::<SceneResidencyActivity>();
        install(&mut app);
        let window = app.world_mut().spawn((PrimaryWindow,
            Window { focused: true, ..default() },
            CursorOptions { visible: true, grab_mode: CursorGrabMode::Locked, ..default() })).id();
        (app, window)
    }

    fn assert_cursor(app: &App, window: Entity, visible: bool) {
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert_eq!(cursor.visible, visible);
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    }

    #[test]
    fn standalone_cursor_applies_the_real_brush_intent_in_the_same_frame() {
        let (mut app, window) = fixture();
        app.init_resource::<SceneDeletionTask>()
            .init_resource::<SceneBrowserState>()
            .init_resource::<SceneBrowserDragState>()
            .add_systems(Update, crate::ui::systems::sync_scene_editor_custom_cursor
                .in_set(EditorCursorPresentation));
        let viewport = app.world_mut().spawn((SceneEditorViewport,
            RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::ZERO) })).id();
        app.world_mut().resource_mut::<EditorInputState>().scene_editor_custom_cursor_visible = false;
        app.update();
        assert_cursor(&app, window, false);
        app.world_mut().get_mut::<RelativeCursorPosition>(viewport).unwrap().cursor_over = false;
        app.update();
        assert_cursor(&app, window, true);
        app.world_mut().get_mut::<RelativeCursorPosition>(viewport).unwrap().cursor_over = true;
        app.update();
        assert_cursor(&app, window, false);
    }

    #[test]
    fn standalone_cursor_restores_on_focus_modal_close_or_stale_scene_intent() {
        for guard in 0..10 {
            let (mut app, window) = fixture();
            app.update();
            assert_cursor(&app, window, false);
            // 不运行刷圈发布，模拟失焦或驻留期间保留true的旧意图；最终写侧仍须归还系统指针。
            match guard {
                0 => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
                1 => app.world_mut().resource_mut::<InputGateState>().system_cursor_required = true,
                2 => app.world_mut().resource_mut::<EditorHostOverlay>().active = true,
                3 => app.world_mut().resource_mut::<SceneEditorState>().close_requested = true,
                4 => app.world_mut().resource_mut::<SceneEditorState>().new_scene_dialog_open = true,
                5 => app.world_mut().resource_mut::<SceneEditorState>().open = false,
                6 => app.world_mut().resource_mut::<SceneEditorState>().scene_transition_in_progress = true,
                7 => app.world_mut().resource_mut::<SceneResidencyActivity>().switching = true,
                8 => app.world_mut().resource_mut::<EditorInputState>().scene_editor_open = false,
                _ => { app.world_mut().remove_resource::<InputGateState>(); }
            }
            app.update();
            assert_cursor(&app, window, true);
        }
    }
}
