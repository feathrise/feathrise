//! 独占工程的平面创建与建模验收；原工具执行细分、撤销重做，原保存链负责落盘与重开。

use crate::{
    authoring::{EditorAuthoringSession, mesh_edit::{MeshEditAction, MeshEditState, MeshElementKind}},
    components::SceneEditorActionButton, resources::SceneEditorViewportState, types::EditorAction,
};
use bevy::{prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured}, tasks::{IoTaskPool, Task, futures::check_ready}};
use feathrise_scene_content::{EditableMeshGeometry, ScenePlacementAssetTarget, ScenePlacementRecord};

pub(super) fn enabled() -> bool { std::env::var("FEATHRISE_PLANE_VALIDATION").as_deref() == Ok("1") }

pub(super) fn target() -> ScenePlacementAssetTarget {
    ScenePlacementAssetTarget::EditableMesh {
        geometry: EditableMeshGeometry::from_plane_size_mm([6000; 2]).expect("固定平面规格合法"),
        color_srgb: [180, 120, 60], roughness_milli: 700, collision: true,
    }
}

#[derive(Resource)]
struct PlaneFixture {
    before: ScenePlacementRecord, after: Option<ScenePlacementRecord>, phase: u8,
    pulse: Option<Entity>, ticks: u8, started: std::time::Instant,
    surface_attempt: u8, surface_proof: Option<serde_json::Value>,
}
#[derive(Resource)]
struct Capture(Task<Result<(), String>>);
#[derive(Resource)]
struct SurfaceCapture(Task<Result<(bool, serde_json::Value), String>>);

pub(super) fn begin(world: &mut World, before: ScenePlacementRecord) {
    let mut view = world.resource_mut::<SceneEditorViewportState>();
    view.center = Vec3::from_array(before.translation);
    view.extent = 10.0; view.pitch = 0.7; view.yaw = 0.7;
    world.insert_resource(PlaneFixture {
        before, after: None, phase: 0, pulse: None, ticks: 0, started: std::time::Instant::now(),
        surface_attempt: 0, surface_proof: None,
    });
}

fn action(world: &mut World, action: MeshEditAction) { world.resource_mut::<MeshEditState>().request = Some(action); }

/// 实际主窗口完成渲染后异步写图，不能用作者几何或内部状态代替画面证据。
fn capture(world: &mut World, filename: &'static str) -> Result<(), String> {
    let directory = std::path::PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("平面截图目录未隔离")?);
    world.spawn(Screenshot::primary_window()).observe(move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
        let image = captured.image.clone(); let directory = directory.clone();
        commands.insert_resource(Capture(IoTaskPool::get().spawn(async move {
            image.try_into_dynamic().map_err(|e| e.to_string())?.save(directory.join(filename)).map_err(|e| e.to_string())
        })));
    });
    Ok(())
}

/// ECS就绪不能证明异步材质管线已出图；从实际作者目标回读四个面内部的颜色。
fn capture_surface(world: &mut World, record: &ScenePlacementRecord, attempt: u8) -> Result<(), String> {
    let view = world.resource::<SceneEditorViewportState>();
    let image = view._image.clone();
    let size = view.render_size;
    let camera = world.get::<Camera>(view.camera).ok_or("平面作者相机失效")?;
    let pose = world.get::<GlobalTransform>(view.camera).ok_or("平面作者相机没有姿态")?;
    let logical_size = camera.logical_viewport_size().ok_or("平面作者相机没有视口")?;
    let center = Vec3::from_array(record.translation);
    let probes = [[-1.5, -1.5], [-1.5, 1.5], [1.5, -1.5], [1.5, 1.5]].map(|[x, z]| {
        camera.world_to_viewport(pose, center + Vec3::new(x, 0.0, z))
            .map(|point| point * size.as_vec2() / logical_size).map_err(|error| error.to_string())
    }).into_iter().collect::<Result<Vec<_>, _>>()?;
    let directory = std::path::PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("平面截图目录未隔离")?);
    world.spawn(Screenshot::image(image)).observe(move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
        let image = captured.image.clone(); let probes = probes.clone(); let directory = directory.clone();
        commands.insert_resource(SurfaceCapture(IoTaskPool::get().spawn(async move {
            let image = image.try_into_dynamic().map_err(|e| e.to_string())?.to_rgb8();
            let counts = probes.iter().map(|point| {
                (-4..=4).flat_map(|dy| (-4..=4).map(move |dx| (point.x.round() as i32 + dx, point.y.round() as i32 + dy)))
                    .filter(|&(x, y)| x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32)
                    .filter(|&(x, y)| {
                        let [r, g, b] = image.get_pixel(x as u32, y as u32).0.map(u16::from);
                        r > 90 && g > 40 && b > 15 && r * 100 > g * 115 && g * 100 > b * 115
                    }).count()
            }).collect::<Vec<_>>();
            let accepted = counts.iter().filter(|&&count| count >= 20).count() >= 3;
            let filename = if accepted { "plane-surface-visible.png".into() } else { format!("plane-surface-attempt-{attempt}.png") };
            image.save(directory.join(&filename)).map_err(|e| e.to_string())?;
            Ok((accepted, serde_json::json!({"visible":accepted,"brown_pixels":counts,"size":size.to_array(),"screenshot":filename})))
        })));
    });
    Ok(())
}

pub(super) fn advance(world: &mut World) -> Option<Result<serde_json::Value, String>> {
    let result = world.resource_scope(|world, mut state: Mut<PlaneFixture>| -> Result<Option<serde_json::Value>, String> {
        if let Some(entity) = state.pulse.take() { let _ = world.despawn(entity); }
        let session = world.resource::<EditorAuthoringSession>();
        let current = session.draft.as_ref().and_then(|draft| draft.placements.placements.iter().find(|r| r.id == state.before.id))
            .cloned().ok_or("平面验收对象已经失效")?;
        let geometry = current.asset_target.editable_geometry().ok_or("平面缺少作者网格")?;
        if !geometry.is_open() { return Err("平面编辑意外改变了开口身份".into()); }
        let edit = world.resource::<MeshEditState>();
        if state.started.elapsed().as_secs() > 90 {
            return Err(format!("平面验收超时：phase={}，edit={}，moving={}，selected={}，vertices={}，faces={}，guard={}，{}",
                state.phase, edit.active(), edit.moving(), edit.selected_count(), geometry.vertex_count(), geometry.face_count(),
                world.resource::<crate::resources::SceneEditorNumericScrubMouseGuard>().blocked(),
                world.resource::<crate::resources::SceneEditorState>().status));
        }
        match state.phase {
            0 if edit.active() => { state.phase = 1; }
            0 => {
                let revision = session.placement_structure_revision;
                let mut browser = world.resource_mut::<crate::ui::gltf_browser::EditorGltfBrowserState>();
                if browser.placement_record_index(&current.id, revision).is_some() {
                    browser.select_primitive_placement_from_viewport(current.id.clone());
                    // 创建按钮的释放门可能仍持有输入；等原编辑会话真正进入后再推进阶段。
                    action(world, MeshEditAction::Toggle);
                }
            }
            1 if edit.active() => { action(world, MeshEditAction::Kind(MeshElementKind::Face)); state.phase = 2; }
            2 => { action(world, MeshEditAction::SelectAll); state.phase = 3; }
            3 if edit.selected_count() == 1 => { action(world, MeshEditAction::Subdivide); state.phase = 4; }
            4 if edit.subdivision_preview_for_validation().is_some_and(|(_, record, valid)| valid && record.asset_target.editable_geometry().is_some_and(|g| g.face_count() == 4)) => {
                action(world, MeshEditAction::Confirm); state.phase = 5;
            }
            5 if (geometry.vertex_count(), geometry.face_count()) == (9, 4) => {
                state.after = Some(current);
                state.ticks = 0;
                state.phase = 50;
            }
            50 if edit.active() && !edit.moving() => {
                // 保留建模态的真实面边，使截图能直接看到一切产生的2×2四边面。
                state.ticks += 1;
                if state.ticks < 12 { return Ok(None); }
                capture(world, "plane-subdivided-edit.png")?;
                state.phase = 51;
            }
            51 => {
                let Some(mut capture) = world.get_resource_mut::<Capture>() else { return Ok(None); };
                let Some(result) = check_ready(&mut capture.0) else { return Ok(None); };
                result?;
                world.remove_resource::<Capture>();
                state.pulse = Some(world.spawn((SceneEditorActionButton(EditorAction::Undo), feathrise_editor_ui::EditorButtonClick::triggered())).id());
                state.phase = 6;
            }
            6 if current == state.before => { state.phase = 60; }
            60 if !edit.owns_mouse()
                && !world.resource::<crate::resources::SceneEditorNumericScrubMouseGuard>().blocked() => {
                // Undo会使旧修订的建模会话退出；等释放门收口后再发Redo。
                state.pulse = Some(world.spawn((SceneEditorActionButton(EditorAction::Redo), feathrise_editor_ui::EditorButtonClick::triggered())).id());
                state.phase = 7;
            }
            7 if state.after.as_ref() == Some(&current) => {
                if edit.active() { action(world, MeshEditAction::Toggle); }
                state.ticks = 0;
                state.phase = 8;
            }
            8 if !edit.owns_mouse()
                && !world.resource::<crate::resources::SceneEditorNumericScrubMouseGuard>().blocked() => {
                let root = session.placement_previews.get(&current.id).copied().ok_or("平面没有真实作者根")?;
                if !feathrise_scene::builtin_placement_visual_ready(world, root) { return Ok(None); }
                // 重新展示原创建面板，截图同时记录平面入口和已细分的真实对象。
                world.resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>().creation_open = true;
                world.resource_mut::<crate::ui::sidebars::SceneEditorSidebarState>().right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
                state.ticks += 1;
                if state.ticks < 12 { return Ok(None); }
                state.surface_attempt += 1;
                capture_surface(world, &current, state.surface_attempt)?;
                state.phase = 9;
            }
            9 => {
                if let Some(mut pending) = world.get_resource_mut::<SurfaceCapture>() && let Some(result) = check_ready(&mut pending.0) {
                    let (accepted, proof) = result?;
                    world.remove_resource::<SurfaceCapture>();
                    if !accepted {
                        if state.surface_attempt >= 16 { return Err(format!("重做后正式平面仍未出图：{proof}")); }
                        state.ticks = 0;
                        state.phase = 8;
                    } else {
                        state.surface_proof = Some(proof);
                        capture(world, "plane-created-subdivided.png")?;
                        state.phase = 10;
                    }
                }
            }
            10 => {
                if let Some(mut capture) = world.get_resource_mut::<Capture>() && let Some(result) = check_ready(&mut capture.0) {
                    result?;
                    return Ok(Some(serde_json::json!({"complete":true,"vertices":9,"faces":4,"triangles":8,
                        "undo_exact":true,"redo_exact":true,"saved_record":current,
                        "object_surface":state.surface_proof,
                        "placements_path":world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements_path(),
                        "screenshot":"plane-created-subdivided.png","edit_screenshot":"plane-subdivided-edit.png","os_input_verified":false})));
                }
            }
            _ => {}
        }
        Ok(None)
    });
    match result { Ok(Some(proof)) => Some(Ok(proof)), Ok(None) => None, Err(error) => Some(Err(error)) }
}
