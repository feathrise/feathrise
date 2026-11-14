//! 统一创建入口的坐标请求和视口点放；地形射线仅在单飞后台任务查询，空图显式使用工作平面。

use super::{creation, resources::TerrainObjectsEditorState, systems, types::{CreationSource, CreationPlacementMode, Mutation}};
use crate::{authoring::{EditorAuthoringSession, terrain_ground::TerrainGroundSnapshot}, components::SceneEditorViewport,
    resources::{SceneEditorState, SceneEditorViewportState, SceneEditorNumericScrubMouseGuard},
    viewport::{placement_move::MoveCamera, modes::{ViewportMode, ViewportModeState}},
    ui::vegetation_preset_editor::{VegetationPresetEditorState, PresetWorkspaceTab}};
use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, Task, futures::check_ready}, ui::RelativeCursorPosition};
use feathrise_scene_content::{SceneId, TerrainId};
use feathrise_scene::ClipmapGroundHit;
use std::{collections::BTreeSet, sync::{Arc, Weak}};

/// 开始按钮冻结整份规格，后续切换来源、坐标用途或作者版本都使旧点放意图失效。
pub(super) struct PointPlacement {
    scene_id: SceneId, revision: u64, terrain_revision: u64, ticket: u64,
    source: CreationSource, values: [f32; 8], asset: Option<String>,
    model_target: Option<feathrise_scene_content::ScenePlacementAssetTarget>,
}

/// 模型创建复用普通摆放记录和原预览；不为导入模型分配地形ID或私有地形画布。
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct ModelCreationContext<'w, 's> {
    commands: Commands<'w, 's>,
    assets: Res<'w, AssetServer>,
    preview: Option<Res<'w, crate::ui::asset_preview::AssetPreviewState>>,
    browser: Option<ResMut<'w, crate::ui::gltf_browser::EditorGltfBrowserState>>,
}
/// 取消只撤销采用权限；仍保留唯一任务槽直到后台真正结清，防止连续取消制造并发。
/// 主线程只保留Weak分配身份，避免任务结清时在主线程析构过期大快照。
pub(super) struct PointQuery {
    ticket: u64, snapshot: Weak<TerrainGroundSnapshot>, hidden: Weak<BTreeSet<TerrainId>>,
    task: Task<Result<Option<Vec3>, ClipmapGroundHit>>,
}

fn mutation(source: CreationSource) -> Option<Mutation> {
    match source {
        CreationSource::ImportedGlb => None,
        _ => source.kind().map(Mutation::Add),
    }
}
fn current(intent: &PointPlacement, editor: &SceneEditorState, session: &EditorAuthoringSession,
    state: &TerrainObjectsEditorState) -> bool {
    editor.open && !editor.close_requested && !editor.scene_transition_in_progress && !editor.new_scene_dialog_open
        && editor.scene_switch_confirmation.is_none() && editor.pending_scene_transition.is_none()
        && !editor.save_requested && !session.saving && session.revision == intent.revision
        && session.terrain_revision == intent.terrain_revision
        && editor.selected_scene_id.as_ref() == Some(&intent.scene_id)
        && session.draft.as_ref().is_some_and(|draft| draft.scene_id == intent.scene_id)
        && state.creation_source == intent.source
        && state.creation.iter().zip(intent.values).all(|(current, frozen)| current.to_bits() == frozen.to_bits())
        && state.placement_mode == CreationPlacementMode::Viewport
}

/// Input阶段收口按钮与Esc；只有点放启动切中央场景，指定坐标保持正在编辑的文档。
#[allow(clippy::too_many_arguments)]
pub(super) fn control(
    mut state: ResMut<TerrainObjectsEditorState>, mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>, mouse: Res<ButtonInput<MouseButton>>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>, mut workspace: Option<ResMut<VegetationPresetEditorState>>,
    mut mode: Option<ResMut<ViewportModeState>>,
    mut shapes: Option<ResMut<crate::authoring::basic_shapes::BasicShapeState>>,
    mut models: ModelCreationContext,
) {
    if !mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
        && !mouse.any_just_released([MouseButton::Left, MouseButton::Right, MouseButton::Middle]) {
        state.placement_mouse_capture = false;
    }
    let escape = keyboard.just_pressed(KeyCode::Escape) && state.point_placement.is_some();
    let invalid = state.point_placement.as_ref().is_some_and(|intent| !current(intent, &editor, &session, &state));
    let builtin = crate::authoring::basic_shapes::is_cuboid_selection(&editor);
    let cancel = std::mem::take(&mut state.cancel_requested) && (state.point_placement.is_some() || builtin);
    if builtin && cancel {
        if let Some(shapes) = shapes.as_deref_mut() { shapes.cancel_requested = true; }
        return; // 基础形状消费者在本系统之后统一清理token、预览身份和鼠标释放。
    }
    if escape || invalid || cancel {
        state.point_placement = None;
        state.placement_mouse_capture = true;
        editor.suppress_world_pick_this_frame = true;
        if let Some(mode) = mode.as_deref_mut() {
            if mode.mode == ViewportMode::Edit { mode.mode = ViewportMode::Select; }
        }
        if escape { keyboard.clear_just_pressed(KeyCode::Escape); }
        editor.status = "已取消添加网格点放，作者数据未修改".into();
    }
    if !std::mem::take(&mut state.start_requested) { return; }
    if state.point_query.is_some() || !systems::enabled(&editor, &session, &state) { return; }
    if let Err(error) = creation::validate_selected(&state) { editor.status = error; return; }
    let Some(draft) = session.draft.as_ref() else { return; };
    let map = draft.scene_id.clone();
    if editor.selected_scene_id.as_ref() != Some(&map) { return; }
    if state.creation_source == CreationSource::Cuboid {
        let target = creation::cuboid_target(&state).expect("已通过统一规格预检");
        if let Some(shapes) = shapes.as_deref_mut() {
            shapes.creation_identity = Some((map, session.revision));
            shapes.creation_request = Some(crate::authoring::basic_shapes::CuboidCreationRequest {
                target, position: (state.placement_mode == CreationPlacementMode::Coordinates)
                    .then(|| Vec3::from_array(state.creation[..3].try_into().unwrap())),
            });
        } else { editor.status = "立方体创建入口尚未就绪".into(); }
        return;
    }
    state.placement_mouse_capture = true;
    editor.suppress_world_pick_this_frame = true; // 地形开始按钮本次释放不能落到刚揭示的场景中。
    let asset = if state.creation_source == CreationSource::ImportedGlb {
        let Some(path) = editor.selected_asset.as_ref().filter(|path| path.to_ascii_lowercase().ends_with(".glb")) else {
            editor.status = "请先在资产目录选择原始GLB文件".into(); return;
        };
        Some(path.clone())
    } else { None };
    let model_target = if let Some(path) = asset.as_deref() {
        match models.preview.as_deref().map(|preview| preview.placement_target_for(path)) {
            Some(Ok(target)) => Some(target),
            Some(Err(error)) => { editor.status = error; return; }
            None => Some(feathrise_scene_content::ScenePlacementAssetTarget::Scene0),
        }
    } else { None };
    if state.placement_mode == CreationPlacementMode::Coordinates {
        let source = state.creation_source;
        if let (Some(path), Some(target)) = (asset.clone(), model_target) {
            let point = Vec3::from_array(state.creation[..3].try_into().unwrap());
            create_model(point, path, target, state.creation[6], &mut editor, &mut session, &mut models);
        } else if let Some(mutation) = mutation(source) { systems::start(&mut session, &mut editor, &mut state, mutation); }
    } else {
        state.placement_ticket = state.placement_ticket.wrapping_add(1);
        state.point_placement = Some(PointPlacement { scene_id: map, revision: session.revision,
            terrain_revision: session.terrain_revision, ticket: state.placement_ticket,
            source: state.creation_source, values: state.creation, asset, model_target });
        if let Some(workspace) = workspace.as_deref_mut() { workspace.active_tab = PresetWorkspaceTab::Scene; }
        if let Some(mode) = mode.as_deref_mut() { mode.mode = ViewportMode::Edit; }
        editor.tool = crate::types::EditorTool::Inspect;
        editor.placement_transform_enabled = false;
        editor.status = "添加网格：请在场景中单击承托面；空场景使用Y=0工作平面，Esc取消".into();
    }
}

/// 在当前相机更新后捕获真实点击，完成值只在同场景、作者、地形和完整快照身份下采用。
#[allow(clippy::too_many_arguments)]
pub(super) fn point_place(
    mut state: ResMut<TerrainObjectsEditorState>, mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>, mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>, guard: Res<SceneEditorNumericScrubMouseGuard>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<(&RelativeCursorPosition, &ComputedNode), With<SceneEditorViewport>>,
    settings: Option<Res<feathrise_editor_core::EditorSettings>>, mut mode: Option<ResMut<ViewportModeState>>,
    workspace: Option<Res<VegetationPresetEditorState>>, windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    gate: super::resources::PanelWorkspaceGate,
    sidebar: Option<Res<crate::ui::sidebars::SceneEditorSidebarState>>,
    numeric: (Option<Res<feathrise_editor_ui::ActiveNumericInput>>, Option<Res<feathrise_editor_ui::ActiveNumericInputScrub>>),
    mut models: ModelCreationContext,
) {
    let context_valid = gate.creation_context_valid()
        && !windows.single().is_ok_and(|window| !window.focused)
        && workspace.as_ref().is_none_or(|workspace| workspace.active_tab == PresetWorkspaceTab::Scene)
        && mode.as_ref().is_none_or(|mode| mode.mode == ViewportMode::Edit)
        && sidebar.as_ref().is_none_or(|sidebar| sidebar.tool_page_visible(crate::ui::sidebars::EditorLeftSidebarTab::Meshes))
        && numeric.0.as_ref().is_none_or(|numeric| numeric.entity.is_none())
        && numeric.1.as_ref().is_none_or(|numeric| numeric.entity.is_none());
    if state.point_placement.as_ref().is_some_and(|intent| !context_valid || !current(intent, &editor, &session, &state)) {
        state.point_placement = None;
        state.placement_mouse_capture = true;
        editor.suppress_world_pick_this_frame = true;
        editor.status = "网格点放上下文已经变化，本次添加已取消".into();
    }
    let completion = state.point_query.as_mut().and_then(|query| check_ready(&mut query.task));
    if let Some(result) = completion {
        let query = state.point_query.take().unwrap();
        let valid = state.point_placement.as_ref().is_some_and(|intent| intent.ticket == query.ticket
            && current(intent, &editor, &session, &state))
            && session.draft.as_ref().and_then(|draft| draft.terrain_ground.as_ref())
                .is_some_and(|ground| Weak::ptr_eq(&Arc::downgrade(ground), &query.snapshot))
            && Weak::ptr_eq(&Arc::downgrade(&editor.terrain_hidden), &query.hidden);
        if valid {
            match result {
                Ok(Some(point)) => { accept(point, &mut state, &mut editor, &mut session, mode.as_deref_mut(), &mut models); }
                _ => editor.status = "本次点击没有取得完整真实地形命中，请重新选择落点".into(),
            }
        }
    }
    let Some(intent) = state.point_placement.as_ref() else { return; };
    if !current(intent, &editor, &session, &state) || state.point_query.is_some() || guard.blocked()
        || editor.suppress_world_pick_this_frame || !mouse.just_pressed(MouseButton::Left)
        || mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
        || mode.as_ref().is_some_and(|mode| mode.pointer_over_controls)
        || windows.single().is_ok_and(|window| !window.focused)
        || workspace.as_ref().is_some_and(|workspace| workspace.active_tab != PresetWorkspaceTab::Scene) { return; }
    let (Some(viewport), Ok((cursor, node))) = (viewport.as_deref(), nodes.single()) else { return; };
    if !cursor.cursor_over() || viewport.framed_scene_id.as_ref() != Some(&intent.scene_id) { return; }
    let Some(ray) = MoveCamera::new(&editor, viewport, node.inverse_scale_factor())
        .and_then(|camera| camera.ray(cursor.normalized?)) else { return; };
    let ticket = intent.ticket;
    let snap = settings.as_ref().map(|settings| settings.editor_snapping.translation_snap(
        keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]), [0.0; 3]));
    editor.suppress_world_pick_this_frame = true;
    state.placement_mouse_capture = true;
    if crate::authoring::basic_shapes::work_plane_is_available(&session) {
        if let Some(point) = crate::authoring::basic_shapes::plane_hit(ray) {
            let point = snap.and_then(|snap| snap.horizontal(point.to_array())).map_or(point, Vec3::from_array);
            accept(point, &mut state, &mut editor, &mut session, mode.as_deref_mut(), &mut models);
        }
        return;
    }
    let Some(snapshot) = session.draft.as_ref().and_then(|draft| draft.terrain_ground.clone())
        .filter(|snapshot| snapshot.scene_id == session.draft.as_ref().unwrap().scene_id && snapshot.revision == session.terrain_revision) else {
        editor.status = "真实地形查询源尚未完整就绪，请稍后重新单击".into(); return;
    };
    let hidden = editor.terrain_hidden.clone();
    let worker_snapshot = snapshot.clone(); let worker_hidden = hidden.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let Some((terrain, mut point)) = worker_snapshot.ray_visible_checked(ray.origin, *ray.direction, &worker_hidden)? else { return Ok(None); };
        if let Some(position) = snap.filter(|snap| snap.active()).and_then(|snap| snap.horizontal(point.to_array())) {
            match worker_snapshot.support_on(&terrain, position[0], position[2], point.y) {
                ClipmapGroundHit::Hit(support) => point = Vec3::new(position[0], support.y, position[2]),
                other => return Err(other),
            }
        }
        Ok(Some(point))
    });
    state.point_query = Some(PointQuery { ticket, snapshot: Arc::downgrade(&snapshot), hidden: Arc::downgrade(&hidden), task });
    editor.status = "正在后台查询本次网格落点，完整命中后开始创建".into();
}

fn accept(point: Vec3, state: &mut TerrainObjectsEditorState, editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession, mode: Option<&mut ViewportModeState>, models: &mut ModelCreationContext) {
    let Some(intent) = state.point_placement.take() else { return; };
    state.creation = intent.values;
    state.creation[..3].copy_from_slice(&point.to_array());
    editor.suppress_world_pick_this_frame = true;
    if let Some(mode) = mode { mode.mode = ViewportMode::Select; }
    if let (Some(path), Some(target)) = (intent.asset.clone(), intent.model_target) {
        create_model(point, path, target, intent.values[6], editor, session, models);
    } else if let Some(mutation) = mutation(intent.source) { systems::start(session, editor, state, mutation); }
}

fn create_model(point: Vec3, path: String, target: feathrise_scene_content::ScenePlacementAssetTarget,
    yaw: f32, editor: &mut SceneEditorState, session: &mut EditorAuthoringSession, models: &mut ModelCreationContext) {
    use feathrise_scene_content::{ScenePlacementHeightMode, ScenePlacementRecord, MAX_SCENE_PLACEMENTS_PER_SCENE};
    let Some(draft) = session.draft.as_ref() else { return; };
    if draft.placements.placements.len() >= MAX_SCENE_PLACEMENTS_PER_SCENE {
        editor.status = format!("当前场景已达到{MAX_SCENE_PLACEMENTS_PER_SCENE}个显式资产上限"); return;
    }
    if !point.is_finite() || !yaw.is_finite() || target.validate().is_err() { return; }
    if let Err(error) = crate::authoring::safe_asset_path(&crate::authoring::workspace_asset_root(), &path) {
        editor.status = error; return;
    }
    let map = draft.scene_id.clone();
    let id = crate::authoring::next_author_id(session, "placement");
    let record = ScenePlacementRecord {
        id: id.clone(), asset_path: path, asset_target: target, editor: Default::default(), mesh_lod: None,
        terrain_binding: None, terrain_patch: None, building_preset_id: None, courtyard: None,
        translation: point.to_array(), height_mode: ScenePlacementHeightMode::FixedHeight, terrain_offset: 0.0,
        yaw_degrees: yaw.rem_euclid(360.0), scale: [1.0; 3], water_surfaces: Vec::new(),
    };
    let entity = crate::authoring::spawn_placement_preview(&mut models.commands, &models.assets, &map, &record);
    session.draft.as_mut().unwrap().placements.placements.push(record.clone());
    session.placement_previews.insert(id.clone(), entity);
    session.undo.push(crate::authoring::EditorCommand::PlacementAdded(record));
    session.redo.clear();
    session.mark_placement_structure_changed(editor);
    if let Some(browser) = models.browser.as_deref_mut() {
        browser.select_primitive_placement_from_viewport(id);
    }
    editor.tool = crate::types::EditorTool::Inspect;
    editor.placement_transform_enabled = false;
    editor.status = "已创建模型；使用普通模型属性、撤销和场景GLB保存".into();
}

#[cfg(test)]
#[path = "point_query_tests.rs"]
mod tests;
