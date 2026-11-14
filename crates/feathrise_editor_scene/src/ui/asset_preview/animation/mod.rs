//! 原预览宿主向独立动画插件发布工程身份和输入门，原场景资源继续拥有生命周期。

use super::{AssetPreviewState, AssetPreviewWorkspace, DocumentState, preview_shading};
use crate::{
    SceneEditorSystemSet,
    resources::*,
    ui::{
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_editor_animation_model::{
    assembly::{AnimationOperations, AnimationSystems},
    host::AnimationHost,
};

pub(crate) mod project_close;
pub(crate) use feathrise_editor_animation_model::resources::AnimationEditorState;
pub(super) use feathrise_editor_animation_model::{
    preview::PreviewRuntime as AnimationPreviewRuntime,
    request::{PreviewRigAnimation, checked_clip_path},
    resources::{BindingState as WeightBinding, WeightFrame},
};

#[cfg(feature = "validation")]
pub(crate) use feathrise_editor_animation::{
    active_project_clip, project_clip_properties, properties_panel_visible,
    rig_animation_diagnostic, rig_animation_identity, rig_animation_probe, timeline_dock_probe,
};

#[cfg(any(test, feature = "validation"))]
pub(crate) use feathrise_editor_animation::workflow_validation;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod timeline_scene_shortcut_tests;
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod workflow_validation_tests;

#[derive(SystemParam)]
struct HostInputs<'w, 's> {
    editor: Res<'w, SceneEditorState>,
    sidebar: Res<'w, SceneEditorSidebarState>,
    workspace: Res<'w, VegetationPresetEditorState>,
    documents: Res<'w, DocumentState>,
    layout: Option<Res<'w, SceneEditorLayoutState>>,
    settings: Option<Res<'w, SceneEditorSettingsState>>,
    browser: Option<Res<'w, SceneBrowserState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    external: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    scene_drag: Option<Res<'w, SceneBrowserDragState>>,
    shading: preview_shading::Pointer<'w, 's>,
}

fn publish_host(source: HostInputs, mut host: ResMut<AnimationHost>) {
    // 各原阶段就地借用宿主权威状态。只在路径改变时更新副本，预览、草稿和任务仍共用原资源。
    let root = source.documents.asset_root();
    if host.asset_root != root {
        host.asset_root = root.to_owned();
    }
    if host.selected_asset != source.editor.selected_asset {
        host.selected_asset
            .clone_from(&source.editor.selected_asset);
    }
    host.tab = source.workspace.active_tab;
    host.open = source.editor.open;
    host.closing = source.editor.close_requested;
    host.switching_scene = source.editor.pending_scene_transition.is_some()
        || source.editor.new_scene_dialog_open
        || source.editor.scene_switch_confirmation.is_some();
    host.modal_open = source.settings.as_ref().is_some_and(|s| s.open)
        || source.browser.as_ref().is_some_and(|s| s.dialog.is_some())
        || source
            .deletion
            .as_ref()
            .is_some_and(|s| s.confirmation.is_some())
        || source.external.as_ref().is_some_and(|s| s.modal_open());
    host.sidebar_assets_visible = source.sidebar.right == EditorRightSidebarTab::Assets
        && source
            .layout
            .as_ref()
            .is_none_or(|layout| layout.inspector_shown());
    host.layout_dragging = source
        .layout
        .as_ref()
        .is_some_and(|state| state.drag.is_some() || state.owns_mouse_this_frame);
    host.scene_dragging = source
        .scene_drag
        .as_ref()
        .is_some_and(|state| state.blocks_other_mouse() || state.press.is_some());
    host.shading_hovered = source.shading.cursor_over();
}

pub(super) fn install(app: &mut App) {
    #[cfg(test)]
    if !app.is_plugin_added::<feathrise_editor_animation::AnimationEditorPlugin>() {
        app.add_plugins(feathrise_editor_animation::AnimationEditorPlugin);
    }

    install_input_boundary(app);
    app.add_message::<PreviewRigAnimation>()
        .configure_sets(
            Update,
            AnimationSystems::Drive.before(super::loading::poll_asset_preview_loading),
        )
        .configure_sets(
            Update,
            AnimationSystems::Preview
                .after(super::loading::poll_asset_preview_loading)
                .before(super::navigation::sync_asset_preview_view),
        )
        .add_systems(
            Update,
            receive_rig_animation
                .after(super::workspace::actions)
                .after(super::ProjectAnimationPickSet)
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            publish_host
                .after(super::subassets::invalidate_model_hot_reload)
                .before(AnimationSystems::Drive)
                .in_set(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            publish_host
                .after(super::subasset_presentation::sync_asset_preview_subassets)
                .before(AnimationSystems::Presentation)
                .in_set(SceneEditorSystemSet::Presentation),
        );
}

// 独立动画与场景建模同屏时，必须先发布时间轴按键和鼠标所有权。
fn install_input_boundary(app: &mut App) {
    app.init_resource::<AnimationHost>()
        .configure_sets(
            Update,
            AnimationSystems::Input
                .after(super::workspace::actions)
                .after(super::interaction::handle_asset_preview_actions)
                .before(super::navigation::control_asset_preview)
                .before(crate::authoring::mesh_edit::MeshEditInput)
                .before(crate::authoring::placement_batch::handle_placement_batch)
                .before(crate::ui::actions::handle_editor_shortcuts)
                .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard),
        )
        .add_systems(
            Update,
            publish_host.after(super::interaction::handle_asset_preview_actions)
                .before(AnimationSystems::Input)
                .in_set(SceneEditorSystemSet::Input),
        );
}

pub(super) fn receive_rig_animation(
    mut requests: MessageReader<PreviewRigAnimation>,
    mut editor: ResMut<SceneEditorState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    documents: Res<DocumentState>,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    workspace: Res<VegetationPresetEditorState>,
    mut animation: ResMut<AnimationEditorState>,
    operations: Res<AnimationOperations>,
    files: Option<Res<crate::ui::file_manager::browser::BrowserSessions>>,
) {
    let Some(request) = requests.read().last() else {
        return;
    };
    let root = documents.asset_root();
    // 文件窗已核验前景、行修订和本帧片段确认；该手势仍占总鼠标门，不能挡住它自己的业务接收。
    let file_pick = files.as_ref().is_some_and(|files| files.accepts_clip(&request.animation_path, request.clip_index));
    if !editor.open
        || editor.close_requested
        || editor.pending_scene_transition.is_some()
        || editor.new_scene_dialog_open
        || editor.scene_switch_confirmation.is_some()
        || (guard.blocked() && !file_pick)
        || root.as_os_str().is_empty()
        || !request.matches(&preview, &media, workspace.active_tab, root)
    {
        return;
    }
    let source = match checked_clip_path(root, &request.animation_path).and_then(|source| {
        checked_clip_path(root, &request.model_path)?;
        Ok(source)
    }) {
        Ok(source) => source,
        Err(error) => {
            editor.status = error;
            return;
        }
    };
    (operations.select)(&mut animation, request.clone(), source);
}
