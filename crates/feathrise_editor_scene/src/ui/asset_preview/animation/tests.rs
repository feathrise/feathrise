//! 跨 crate 交互夹具：仍由正式场景边界发布输入门。

use super::super::{
    AssetPreviewWorkspace, DocumentState,
    types::{AssetPreviewContent, AssetPreviewPhase, AssetPreviewState},
};
use crate::{
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{input::mouse::MouseWheel, prelude::*};
use feathrise_editor_animation::test_support::AnimationTrack;
use feathrise_editor_animation_model::{
    data::tests::test_document,
    resources::{AnimationEditorState, Session},
};
use feathrise_editor_ui::{NumericInputCommitted, UiInteraction};
use std::path::PathBuf;

pub(super) fn install_controls(app: &mut App) {
    super::install_input_boundary(app);
    feathrise_editor_animation::test_support::install_controls(app);
}

pub(super) fn app_context() -> (App, PathBuf, PathBuf) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<DocumentState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<AssetPreviewWorkspace>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<MouseWheel>()
        .add_message::<NumericInputCommitted>();
    let root = app
        .world()
        .resource::<DocumentState>()
        .asset_root()
        .to_owned();
    let alpha = root.join("animation-regression-alpha.glb");
    let beta = root.join("animation-regression-beta.glb");
    let mut editor = SceneEditorState::default();
    editor.open = true;
    editor.selected_asset = Some("animation-regression-alpha.glb".into());
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    let mut state = AnimationEditorState::default();
    state.active = Some(alpha.clone());
    state.visible = true;
    state
        .sessions
        .insert(alpha.clone(), Session::new(test_document()));
    state
        .sessions
        .insert(beta.clone(), Session::new(test_document()));
    app.insert_resource(editor)
        .insert_resource(sidebar)
        .insert_resource(state)
        .insert_resource(AssetPreviewState {
            selected_path: Some("animation-regression-alpha.glb".into()),
            content: AssetPreviewContent::Model {
                handle: Handle::default(),
                root: Entity::PLACEHOLDER,
                phase: AssetPreviewPhase::Ready,
            },
            ..default()
        });
    (app, alpha, beta)
}

pub(crate) fn install_timeline_input_for_tests(app: &mut App) -> Entity {
    app.init_resource::<DocumentState>()
        .init_resource::<AssetPreviewWorkspace>()
        .init_resource::<SceneEditorSidebarState>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_message::<feathrise_editor_ui::NumericInputCommitted>();
    let path = "animation-mesh-input.glb";
    let source = app
        .world()
        .resource::<DocumentState>()
        .asset_root()
        .join(path);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some(path.into());
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .reveal_asset_properties();
    let mut state = AnimationEditorState::default();
    state.active = Some(source.clone());
    state.visible = true;
    state.sessions.insert(source, Session::new(test_document()));
    let binding = state.revision;
    app.insert_resource(state)
        .insert_resource(AssetPreviewState {
            selected_path: Some(path.into()),
            content: AssetPreviewContent::Model {
                handle: Handle::default(),
                root: Entity::PLACEHOLDER,
                phase: AssetPreviewPhase::Ready,
            },
            ..default()
        });
    install_controls(app);
    app.world_mut()
        .spawn((
            AnimationTrack { binding, row: None },
            ComputedNode {
                size: Vec2::new(200.0, 20.0),
                ..default()
            },
            bevy::ui::RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
            UiInteraction::Hovered,
        ))
        .id()
}
