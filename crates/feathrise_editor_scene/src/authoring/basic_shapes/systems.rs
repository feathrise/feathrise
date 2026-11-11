//! 统一网格入口消费：打开面板不切文档，坐标创建直接采用，点放才切回场景。

use super::{CUBOID_SELECTION, placement::is_cuboid_selection, resources::BasicShapeState};
use crate::{
    authoring::{EditorAuthoringSession, edit},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState, SceneEditorState},
    types::EditorTool,
    ui::{
        gltf_browser::EditorGltfBrowserState,
        sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState},
        terrain_objects::TerrainObjectsEditorState,
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
    },
    viewport::modes::{ViewportMode, ViewportModeState},
};
use bevy::{prelude::*, window::PrimaryWindow};

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct CreationContext<'w, 's> {
    commands: Commands<'w, 's>,
    asset_server: Res<'w, AssetServer>,
    browser: ResMut<'w, EditorGltfBrowserState>,
    selection: Option<ResMut<'w, crate::resources::EditorViewportSelectionState>>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn control_menu(
    mut state: ResMut<BasicShapeState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    settings: Option<Res<SceneEditorSettingsState>>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut mode: ResMut<ViewportModeState>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    mut meshes: ResMut<TerrainObjectsEditorState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut context: CreationContext,
) {
    state.captured_this_frame = false;
    let open_requested = std::mem::take(&mut state.request_menu);
    let request = state.creation_request.take();
    let request_identity = state.creation_identity.take();
    let cancel_requested = std::mem::take(&mut state.cancel_requested);
    if !is_cuboid_selection(&editor) {
        state.placement_target = None;
        state.placement_identity = None; // 完成一次创建后的修订不应阻塞下一次添加。
    }
    let available = editor.open
        && !editor.close_requested && !editor.save_requested
        && !editor.scene_transition_in_progress && editor.pending_scene_transition.is_none()
        && !editor.new_scene_dialog_open && !editor.suppress_world_pick_this_frame
        && editor.scene_switch_confirmation.is_none() && !session.saving
        && !settings.as_ref().is_some_and(|state| state.open || state.owns_mouse_this_frame)
        && !windows.single().is_ok_and(|window| !window.focused)
        && session.draft.as_ref().is_some_and(|draft| editor.selected_scene_id.as_ref() == Some(&draft.scene_id));
    let stale = state.placement_identity.is_some() && (state.placement_target(&session).is_none()
        || meshes.creation_source != crate::ui::terrain_objects::CreationSource::Cuboid
        || meshes.placement_mode != crate::ui::terrain_objects::CreationPlacementMode::Viewport
        || workspace.active_tab != PresetWorkspaceTab::Scene);
    if !available || stale {
        if is_cuboid_selection(&editor) {
            cancel(&mut state, &mut editor, &mut mode);
        }
        return; // 请求和点放身份都不能跨场景/作者换代或模态保护延迟采用。
    }
    if (cancel_requested || keyboard.just_pressed(KeyCode::Escape)) && is_cuboid_selection(&editor) {
        cancel(&mut state, &mut editor, &mut mode);
        keyboard.clear_just_pressed(KeyCode::Escape);
        return; // 这次Esc只取消点放，不继续关闭中央文档或F12。
    }
    if open_requested && !guard.blocked() {
        meshes.creation_open = true;
        sidebar.left = EditorLeftSidebarTab::Meshes;
        sidebar.right = EditorLeftSidebarTab::Meshes.work_page();
        state.captured_this_frame = true;
    }
    let Some(request) = request else { return; };
    // 面板已经核验按钮资格；释放帧的公共鼠标门可能正由该按钮自身持有，不能二次拒收。
    if !request.target.is_builtin() || request.target.validate().is_err()
        || request.position.is_some_and(|point| !point.is_finite())
        || !session.draft.as_ref().is_some_and(|draft| request_identity.as_ref() == Some(&(draft.scene_id.clone(), session.revision)))
    {
        return;
    }
    state.captured_this_frame = true; // 创建按钮的释放不能同时落到视口。
    state.work_plane = None;
    if let Some(position) = request.position {
        if let Some(id) = edit::create_cuboid_at(&mut context.commands, &context.asset_server,
            &mut editor, &mut session, request.target, position)
        {
            context.browser.select_primitive_placement_from_viewport(id);
            if let Some(selection) = context.selection.as_deref_mut() { selection.clear_vegetation_selection(); }
            sidebar.reveal_scene_properties();
            mode.mode = ViewportMode::Select;
            state.placement_target = None;
            state.placement_identity = None;
        }
        return; // 世界中心坐标直接采用，保留当前中央文档与未保存文本。
    }
    let draft = session.draft.as_ref().expect("上方资格已经确认草稿");
    state.placement_identity = Some((draft.scene_id.clone(), session.revision));
    state.placement_target = Some(request.target); // 后续面板调整不得改写已武装点放的规格。
    editor.selected_asset = Some(CUBOID_SELECTION.into());
    editor.tool = EditorTool::PlaceAsset;
    editor.domain_brush_active = false;
    editor.placement_transform_enabled = false;
    mode.mode = ViewportMode::Edit;
    mode.remember_tool(EditorTool::PlaceAsset);
    sidebar.left = EditorLeftSidebarTab::Meshes;
    sidebar.observed_tool = EditorTool::PlaceAsset;
    sidebar.right = EditorLeftSidebarTab::Meshes.work_page();
    workspace.active_tab = PresetWorkspaceTab::Scene;
    workspace.touch_ui();
    editor.status = if super::placement::work_plane_is_available(&session) {
        "添加立方体：当前为空场景，底面贴Y=0工作平面；左键放置，Esc取消".into()
    } else {
        "添加立方体：按当前规格预览，底面贴地；左键放置，Esc取消".into()
    };
}

fn cancel(state: &mut BasicShapeState, editor: &mut SceneEditorState, mode: &mut ViewportModeState) {
    state.placement_target = None;
    state.placement_identity = None;
    state.work_plane = None;
    state.captured_this_frame = true;
    editor.tool = EditorTool::Inspect;
    editor.selected_asset = None;
    editor.placement_transform_enabled = false;
    mode.mode = ViewportMode::Select;
    editor.status = "已取消添加立方体".into();
}
