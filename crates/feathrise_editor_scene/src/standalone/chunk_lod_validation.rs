//! 自有工程的真实编辑器验收：原建模输入细分、撤销重做、分块显示与GPU截图，随后交回原保存链。
use crate::{authoring::{EditorAuthoringSession, mesh_edit::{MeshEditState, MeshEditAction, MeshElementKind}},
    components::SceneEditorActionButton, resources::{SceneEditorState, SceneEditorViewportState}, types::EditorAction};
use bevy::{prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured}, tasks::{IoTaskPool, Task, futures::check_ready}};
use feathrise_scene_content::ScenePlacementRecord;
use feathrise_scene_content::MeshLodMode;
use crate::ui::placement_transform::{PlacementTransformAction as LodAction, PlacementTransformField as LodField,
    SceneEditorPlacementTransformActionButton};
use feathrise_scene::{ScenePlacementInstance, StaticMeshChunkLodStatus};
use std::{path::PathBuf, time::Instant};

#[derive(Resource)]
pub(super) struct ChunkFixture {
    id: String,
    phase: u8,
    before: Option<ScenePlacementRecord>,
    after: Option<ScenePlacementRecord>,
    started: Instant,
    pulse: Option<Entity>,
    ui_pulse: Option<Entity>,
    ticks: usize,
    original_view: (Vec3, f32, f32, f32, bool),
    proof: serde_json::Value,
}

#[derive(Resource)]
struct Capture(Task<Result<(), String>>);

pub(super) fn begin(world: &mut World, id: &str) -> Result<bool, String> {
    if std::env::var("FEATHRISE_CHUNK_LOD_VALIDATION").as_deref() != Ok("1") { return Ok(false); }
    if !world.contains_resource::<super::FixtureInstalled>() || world.contains_resource::<ChunkFixture>() {
        return Err("分块LOD专项只允许本轮自有工程启动一次".into());
    }
    let view = world.resource::<SceneEditorViewportState>();
    let original_view = (view.center, view.extent, view.yaw, view.pitch, world.resource::<SceneEditorState>().orthographic);
    world.insert_resource(ChunkFixture { id: id.into(), phase: 0, before: None, after: None,
        started: Instant::now(), pulse: None, ui_pulse: None, ticks: 0, original_view, proof: serde_json::json!({
            "placement_id":id,"complete":false,"scope":"original mesh edit input, undo/redo, actual adopted chunk geometry and GPU; physical keyboard not exercised"
        }) });
    Ok(true)
}

fn action(world: &mut World, action: MeshEditAction) { world.resource_mut::<MeshEditState>().request = Some(action); }

/// 只点击正式属性树里的按钮，不能另造一个标记绕过缺失入口。
fn lod_click(world: &mut World, action: LodAction) -> Result<Entity, String> {
    let entity = world.query::<(Entity, &SceneEditorPlacementTransformActionButton)>().iter(world)
        .find_map(|(entity, button)| (button.0 == action).then_some(entity)).ok_or("属性栏缺少正式LOD按钮")?;
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(entity)
}

fn ui_visible(world: &World, entity: Entity) -> bool {
    let Some(node) = world.get::<ComputedNode>(entity) else { return false; };
    let Some(transform) = world.get::<UiGlobalTransform>(entity) else { return false; };
    if node.is_empty() { return false; }
    let center = transform.translation;
    let mut cursor = entity;
    for _ in 0..24 {
        if world.get::<Node>(cursor).is_some_and(|node| node.display == Display::None)
            || world.get::<InheritedVisibility>(cursor).is_some_and(|visible| !visible.get()) { return false; }
        if let (Some(node), Some(computed), Some(transform)) = (world.get::<Node>(cursor),
            world.get::<ComputedNode>(cursor), world.get::<UiGlobalTransform>(cursor))
            && !node.overflow.is_visible() && transform.try_inverse().is_none_or(|inverse|
                !computed.resolve_clip_rect(node.overflow, node.overflow_clip_margin).contains(inverse.transform_point2(center))) { return false; }
        let Some(parent) = world.get::<ChildOf>(cursor) else { return true; };
        cursor = parent.parent();
    }
    false
}

fn lod_value(world: &mut World, field: LodField, value: f32) -> Result<(), String> {
    let entity = world.query::<(Entity, &LodField)>().iter(world)
        .find_map(|(entity, actual)| (*actual == field).then_some(entity)).ok_or("属性栏缺少正式LOD数值根")?;
    world.write_message(feathrise_editor_ui::NumericInputCommitted { entity,
        value: feathrise_editor_ui::NumericValue::F32(value), outcome: feathrise_editor_ui::NumericInputCommitOutcome::Accepted,
        source: feathrise_editor_ui::NumericInputCommitSource::Text, changed: true, edited: true });
    Ok(())
}

/// 使用原滚动宿主把两组LOD定位到视区；不改布局或重新挂载控件。
fn reveal_lod(world: &mut World) -> Result<(), String> {
    let button = world.query::<(Entity, &SceneEditorPlacementTransformActionButton)>().iter(world)
        .find_map(|(entity, button)| (button.0 == LodAction::DisplayLodCustom).then_some(entity)).ok_or("没有LOD按钮")?;
    let top = world.get::<UiGlobalTransform>(button).ok_or("LOD尚未布局")?.translation.y;
    let mut cursor = button;
    for _ in 0..24 {
        if world.get::<crate::components::SceneEditorRightPanelScroll>(cursor).is_some() {
            let node = world.get::<ComputedNode>(cursor).ok_or("属性滚动区尚未布局")?;
            let viewport_top = world.get::<UiGlobalTransform>(cursor).ok_or("属性滚动区没有布局位置")?.translation.y - node.size().y * 0.5;
            let delta = (top - viewport_top - 64.0) * node.inverse_scale_factor();
            world.get_mut::<ScrollPosition>(cursor).ok_or("属性区缺少原滚动位置")?.y += delta;
            return Ok(());
        }
        cursor = world.get::<ChildOf>(cursor).ok_or("LOD未挂在右侧属性树")?.parent();
    }
    Err("LOD属性层级超出验收边界".into())
}

fn observe(world: &mut World, center: Vec3, distance: f32) {
    let mut view = world.resource_mut::<SceneEditorViewportState>();
    view.extent = 8.0; view.center = center;
    view.yaw = std::f32::consts::FRAC_PI_2; view.pitch = 0.1;
    // 正式轨道缩放下限为8米；通过平移观察中心接近小物件，不修改产品下限或直接覆盖相机实体。
    let (_, pose) = crate::viewport::editor_camera_pose(false, &view);
    view.center = center + pose.back().as_vec3() * (distance - 8.0);
    world.resource_mut::<SceneEditorState>().orthographic = false;
}

fn record(world: &World, id: &str) -> Result<ScenePlacementRecord, String> {
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().filter(|draft| draft.scene_id.as_str() == "M1-SAMPLE")
        .ok_or("分块验收场景身份变化")?;
    draft.placements.placements.iter().find(|record| record.id == id).cloned().ok_or("分块验收对象已被删除".into())
}

fn status(world: &mut World, id: &str) -> Option<(Entity, StaticMeshChunkLodStatus)> {
    let mut query = world.query::<(Entity, &StaticMeshChunkLodStatus)>();
    query.iter(world).find_map(|(entity, status)| {
        let mut cursor = entity;
        for _ in 0..8 {
            if let Some(owner) = world.get::<ScenePlacementInstance>(cursor) {
                return (owner.id() == id).then(|| (entity, status.clone()));
            }
            cursor = world.get::<ChildOf>(cursor)?.parent();
        }
        None
    })
}

pub(super) fn advance(world: &mut World) -> Option<Result<serde_json::Value, String>> {
    let result = world.resource_scope(|world, mut state: Mut<ChunkFixture>| -> Result<bool, String> {
        if let Some(entity) = state.pulse.take() { let _ = world.despawn(entity); }
        if let Some(entity) = state.ui_pulse.take() { world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::default()); }
        if state.started.elapsed().as_secs() > 90 {
            let history = world.resource::<EditorAuthoringSession>();
            return Err(format!("分块LOD验收超时，phase={}，{}，lod={}，edit={}，undo={}，redo={}，guard={}", state.phase,
                world.resource::<SceneEditorState>().status, world.resource::<feathrise_scene::MeshLodRuntimeStatus>().detail,
                world.resource::<MeshEditState>().active(), history.undo.len(), history.redo.len(),
                world.resource::<crate::resources::SceneEditorNumericScrubMouseGuard>().blocked()));
        }
        let current = record(world, &state.id)?;
        let geometry = current.asset_target.editable_geometry().ok_or("验收对象丢失作者网格")?;
        let edit = world.resource::<MeshEditState>();
        match state.phase {
            0 if edit.active() => { action(world, MeshEditAction::Kind(MeshElementKind::Face)); state.phase = 1; }
            0 => {
                // 创建帧仍由原按钮持有输入，且场景树索引尚在后台换代；等真实身份采用再发送编辑输入。
                let revision = world.resource::<EditorAuthoringSession>().placement_structure_revision;
                let mut browser = world.resource_mut::<crate::ui::gltf_browser::EditorGltfBrowserState>();
                if browser.placement_record_index(&state.id, revision).is_some() {
                    browser.select_primitive_placement_from_viewport(state.id.clone());
                    action(world, MeshEditAction::Toggle);
                }
            }
            1 => { action(world, MeshEditAction::SelectAll); state.phase = 2; }
            2 => { action(world, MeshEditAction::Subdivide); state.phase = 3; }
            3 if edit.moving() => { action(world, MeshEditAction::Confirm); state.phase = 4; }
            4 if geometry.vertex_count == 26 && geometry.face_count == 24 => {
                state.before = Some(current);
                // 只细分同一原始面上的四个子面；邻面会真实补齐边点，仍须遵守64面容量。
                world.resource_mut::<MeshEditState>().select_for_test(MeshElementKind::Face, 0xf);
                action(world, MeshEditAction::Subdivide); state.phase = 5;
            }
            5 if world.resource::<SceneEditorState>().status.starts_with("细分预览无效") => {
                return Err(world.resource::<SceneEditorState>().status.clone());
            }
            5 if edit.moving() => { action(world, MeshEditAction::Confirm); state.phase = 6; }
            6 if geometry.vertex_count > 26 && geometry.triangles().len() > 64 => {
                geometry.validate()?;
                state.proof["author_vertices"] = geometry.vertex_count.into();
                state.proof["author_triangles"] = geometry.triangles().len().into();
                state.after = Some(current);
                state.pulse = Some(world.spawn((SceneEditorActionButton(EditorAction::Undo), feathrise_editor_ui::EditorButtonClick::triggered())).id());
                state.phase = 7;
            }
            7 if state.before.as_ref() == Some(&current) => {
                state.proof["undo_exact"] = true.into();
                state.phase = 70;
            }
            70 if !edit.owns_mouse()
                && !world.resource::<crate::resources::SceneEditorNumericScrubMouseGuard>().blocked() => {
                // 原Undo会退出旧修订的点边面会话；等其释放输入后再点Redo，不能把一次被门吞掉的点击当回执。
                state.pulse = Some(world.spawn((SceneEditorActionButton(EditorAction::Redo), feathrise_editor_ui::EditorButtonClick::triggered())).id());
                state.phase = 8;
            }
            8 if state.after.as_ref() == Some(&current) => {
                state.proof["redo_exact"] = true.into();
                // Undo/Redo更换作者修订时原输入系统可能已自动退回对象模式，不能反向重新进入编辑。
                if edit.active() { action(world, MeshEditAction::Toggle); }
                state.phase = 9;
            }
            9 if !edit.owns_mouse() && !world.resource::<crate::resources::SceneEditorNumericScrubMouseGuard>().blocked() => {
                let tab = world.query::<(Entity, &crate::ui::sidebars::SceneEditorSidebarTabButton)>().iter(world)
                    .find_map(|(entity, button)| (button.0 == crate::ui::sidebars::EditorSidebarTab::Right(
                        crate::ui::sidebars::EditorRightSidebarTab::Scene)).then_some(entity)).ok_or("缺少原属性页签")?;
                world.entity_mut(tab).insert(feathrise_editor_ui::EditorButtonClick::triggered());
                state.ui_pulse = Some(tab); state.phase = 18;
            }
            18 => { reveal_lod(world)?; state.phase = 19; }
            19 => {
                let button = world.query::<(Entity, &SceneEditorPlacementTransformActionButton)>().iter(world)
                    .find_map(|(entity, button)| (button.0 == LodAction::DisplayLodCustom).then_some(entity)).ok_or("没有显示LOD按钮")?;
                if ui_visible(world, button) {
                    state.ui_pulse = Some(lod_click(world, LodAction::DisplayLodCustom)?); state.phase = 20;
                }
            }
            20 if matches!(current.mesh_lod.unwrap_or_default().display, MeshLodMode::Override(_)) => {
                state.ui_pulse = Some(lod_click(world, LodAction::CollisionLodCustom)?); state.phase = 21;
            }
            21 if matches!(current.mesh_lod.unwrap_or_default().collision, MeshLodMode::Override(_)) => {
                lod_value(world, LodField::LodHysteresis, 0.2)?;
                lod_value(world, LodField::LodSafeNear, 64.0)?; state.phase = 22;
            }
            22 if matches!(current.mesh_lod.unwrap_or_default().display, MeshLodMode::Override(value) if value.hysteresis_ratio == 0.2)
                && matches!(current.mesh_lod.unwrap_or_default().collision, MeshLodMode::Override(value) if value.safe_near_meters == 64.0) => {
                state.before = Some(current);
                lod_value(world, LodField::LodCollisionError, 0.0123)?; state.phase = 23;
            }
            23 if matches!(current.mesh_lod.unwrap_or_default().collision, MeshLodMode::Override(value) if value.max_error_meters == 0.0123) => {
                state.after = Some(current);
                state.pulse = Some(world.spawn((SceneEditorActionButton(EditorAction::Undo), feathrise_editor_ui::EditorButtonClick::triggered())).id());
                state.phase = 24;
            }
            24 if state.before.as_ref() == Some(&current) => {
                state.proof["lod_undo_exact"] = true.into();
                state.pulse = Some(world.spawn((SceneEditorActionButton(EditorAction::Redo), feathrise_editor_ui::EditorButtonClick::triggered())).id());
                state.phase = 25;
            }
            25 if state.after.as_ref() == Some(&current) => {
                state.proof["lod_redo_exact"] = true.into();
                state.ui_pulse = Some(lod_click(world, LodAction::DisplayLodOriginal)?); state.phase = 26;
            }
            26 if current.mesh_lod.unwrap_or_default().display == MeshLodMode::Disabled => {
                // 策略回执不能冒充实际恢复：先检查正式显示网格已换回原始句柄，再恢复自定义。
                let restored = world.query::<(Entity, &feathrise_scene::StaticMeshLodOriginal, &Mesh3d)>().iter(world)
                    .any(|(entity, original, mesh)| original.mesh == mesh.0 && {
                        let mut cursor = entity;
                        (0..8).any(|_| {
                            if world.get::<ScenePlacementInstance>(cursor).is_some_and(|owner| owner.id() == state.id) { return true; }
                            if let Some(parent) = world.get::<ChildOf>(cursor) { cursor = parent.parent(); }
                            false
                        })
                    });
                if restored {
                    state.proof["lod_original_restored"] = true.into();
                    state.ui_pulse = Some(lod_click(world, LodAction::DisplayLodCustom)?); state.phase = 27;
                }
            }
            27 if matches!(current.mesh_lod.unwrap_or_default().display, MeshLodMode::Override(_)) => {
                lod_value(world, LodField::LodHysteresis, 0.2)?;
                reveal_lod(world)?; state.phase = 28;
            }
            28 if matches!(current.mesh_lod.unwrap_or_default().display, MeshLodMode::Override(value) if value.hysteresis_ratio == 0.2) => {
                state.proof["lod_properties_ui"] = true.into();
                observe(world, Vec3::from_array(current.translation), 6.0);
                state.phase = 10; state.ticks = 0;
            }
            10 => {
                state.ticks += 1;
                if let Some((entity, applied)) = status(world, &state.id) {
                    if state.proof["far_levels"].is_null() {
                        state.proof["far_levels"] = serde_json::json!(applied.levels);
                        state.proof["far_triangles"] = applied.triangles.into();
                    }
                    state.proof["last_levels"] = serde_json::json!(applied.levels);
                    if applied.levels.contains(&0) && applied.levels.iter().any(|&level| level > 0) {
                        let fields = world.query::<(Entity, &LodField)>().iter(world).filter(|(_, field)| matches!(field,
                            LodField::LodScreenError | LodField::LodHysteresis | LodField::LodSafeNear | LodField::LodCollisionError))
                            .map(|(entity, _)| entity).collect::<Vec<_>>();
                        if fields.len() != 4 || fields.iter().any(|entity| !ui_visible(world, *entity)) {
                            return Err("LOD四个正式数值控件未全部位于属性栏可见区域".into());
                        }
                        let preview = world.resource::<EditorAuthoringSession>().placement_previews.get(&state.id).copied()
                            .ok_or("LOD属性未维护作者预览身份")?;
                        if world.get::<ScenePlacementInstance>(preview).map(|owner| owner.mesh_lod_policy()) != current.mesh_lod {
                            return Err("LOD作者策略未同步到实际预览根".into());
                        }
                        state.proof["lod_fields_visible"] = true.into();
                        let original = world.get::<feathrise_scene::StaticMeshLodOriginal>(entity).ok_or("派生显示未保留作者句柄")?;
                        if original.mesh == world.get::<Mesh3d>(entity).unwrap().0 { return Err("分块回执与真实Mesh不一致".into()); }
                        state.proof["mixed_levels"] = serde_json::json!(applied.levels);
                        state.proof["mixed_triangles"] = applied.triangles.into();
                        state.proof["saved_record"] = serde_json::to_value(&current).map_err(|error| error.to_string())?;
                        state.proof["placements_path"] = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements_path().into();
                        let directory = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("截图目录未隔离")?);
                        world.spawn(Screenshot::primary_window()).observe(move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
                            let image = captured.image.clone(); let directory = directory.clone();
                            commands.insert_resource(Capture(IoTaskPool::get().spawn(async move {
                                image.try_into_dynamic().map_err(|error| error.to_string())?
                                    .save(directory.join("chunk-lod-mixed.png")).map_err(|error| error.to_string())
                            })));
                        });
                        state.phase = 11;
                    }
                }
                // 每档至少等待真实后台采用；观察距离逐档接近，不能直接写入预期LOD结果。
                if state.phase == 10 && state.ticks % 15 == 0 {
                    let distance = 6.0 - (state.ticks / 15) as f32 * 0.1;
                    if distance < 0.6 { return Err(format!("真实相机范围内没有观察到分块混合档位：{}", state.proof)); }
                    observe(world, Vec3::from_array(current.translation), distance);
                }
            }
            11 => {
                if let Some(mut capture) = world.get_resource_mut::<Capture>()
                    && let Some(result) = check_ready(&mut capture.0) {
                    result?;
                    let (center, extent, yaw, pitch, orthographic) = state.original_view;
                    let mut view = world.resource_mut::<SceneEditorViewportState>();
                    view.center = center; view.extent = extent; view.yaw = yaw; view.pitch = pitch;
                    world.resource_mut::<SceneEditorState>().orthographic = orthographic;
                    state.proof["screenshot"] = "chunk-lod-mixed.png".into(); state.proof["complete"] = true.into();
                    return Ok(true);
                }
            }
            _ => {}
        }
        Ok(false)
    });
    match result {
        Ok(false) => None,
        Ok(true) => Some(Ok(world.resource::<ChunkFixture>().proof.clone())),
        Err(error) => Some(Err(error)),
    }
}
