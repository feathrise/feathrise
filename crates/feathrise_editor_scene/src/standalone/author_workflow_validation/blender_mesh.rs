//! 自有工程中的 Blender GLB 往返验收，走原创建、菜单、预览、历史及场景保存。

use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand, mesh_collision_enabled,
        basic_shapes::{BasicShapeState, CuboidCreationRequest},
        blender_mesh::BlenderMeshSession,
    },
    components::{EditorPlacedAsset, SceneEditorActionButton},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    standalone::validation_fixture::{open_scene_tree, reveal_property_control, ui_visible},
    types::EditorAction,
    ui::{
        placement_transform::{PlacementTransformAction, SceneEditorPlacementTransformActionButton},
        scene_context_menu::queue_blender_action_for_validation,
    },
};
use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::{
    ecs::system::RunSystemOnce,
    mesh::VertexAttributeValues,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::InteractionDisabled,
    world_serialization::WorldAssetRoot,
};
use feathrise_scene::ScenePlacementInstance;
use feathrise_scene_content::{ScenePlacementAssetTarget, ScenePlacementRecord};
use serde_json::{Value, json};
use std::{fs, path::{Path, PathBuf}, time::{Duration, Instant}};

const PEER_POSITION: [f32; 3] = [0., 1., 0.];
// 已包含 Blender 节点的 X 放大、Y 压薄和镜像，取其非对称截面辨别负缩放与正缩放。
const STEP_PROFILE: [[f32; 2]; 6] = [[-2., 0.03125], [2., 0.03125], [2., -0.03125],
    [0., -0.03125], [0., 0.], [-2., 0.]];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    CreatePeer,
    OpenBlender,
    WaitForGeometry,
    WaitForExit,
    Undo,
    Redo,
    SwitchCollisionOff,
    SwitchCollisionOn,
    Capture,
    WaitForCapture,
    ReopenBlender,
    ReadEvidence,
}

#[derive(Resource)]
struct Fixture {
    started: Instant,
    stage: Step,
    output: PathBuf,
    original: ScenePlacementRecord,
    peer: Option<ScenePlacementRecord>,
    history_before: usize,
    blender_record: Option<ScenePlacementRecord>,
    pulse: Option<Entity>,
    ui_pulse: Option<Entity>,
    evidence: Option<Task<Result<Value, String>>>,
}

#[derive(Resource)]
struct Capture(Task<Result<(), String>>);

pub(in crate::standalone) fn enabled() -> bool {
    std::env::var("FEATHRISE_BLENDER_MESH_VALIDATION").as_deref() == Ok("1")
}

pub(in crate::standalone) fn begin(world: &mut World, original: ScenePlacementRecord) -> Result<(), String> {
    let output = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("Blender 验收缺少自有目录")?);
    let work = output.parent().ok_or("Blender 验收缺少独占工作目录")?;
    let owner: Value = serde_json::from_slice(&fs::read(work.join(".feathrise-native-validation-owner.json"))
        .map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let paths = feathrise_editor_core::editor_project_paths().ok_or("Blender 验收缺少自有工程路径")?;
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("Blender 验收缺少作者草稿")?;
    if owner["schema"] != 1 || owner["purpose"] != "ordinary-author-boundary-validation"
        || owner["work_root"] != json!(work) || paths.project_root() != work.join("独立工程 A") {
        return Err("Blender 验收只允许本轮独占创建的工程 A".into());
    }
    if draft.scene_id.as_str() != "M1-SAMPLE" || !draft.placements_path().ends_with("/scene.glb") {
        return Err(format!("Blender 验收的示例场景未使用私有 GLB：{}", draft.placements_path()));
    }
    if original.asset_target != ScenePlacementAssetTarget::DEFAULT_CUBOID {
        return Err("Blender 验收缺少原坐标创建的立方体".into());
    }
    let identity = (draft.scene_id.clone(), session.revision);
    let mut shapes = world.resource_mut::<BasicShapeState>();
    if shapes.creation_request.is_some() { return Err("原网格创建槽已占用".into()); }
    shapes.creation_identity = Some(identity);
    shapes.creation_request = Some(CuboidCreationRequest {
        target: ScenePlacementAssetTarget::DEFAULT_CUBOID,
        position: Some(Vec3::from_array(PEER_POSITION)),
    });
    world.insert_resource(Fixture {
        started: Instant::now(),
        stage: Step::CreatePeer,
        output,
        original,
        peer: None,
        history_before: 0,
        blender_record: None,
        pulse: None,
        ui_pulse: None,
        evidence: None,
    });
    Ok(())
}

pub(in crate::standalone) fn advance(world: &mut World) -> Option<Result<Value, String>> {
    match world.resource_scope(|world, mut fixture: Mut<Fixture>| drive(world, &mut fixture)) {
        Ok(Some(proof)) => Some(Ok(proof)),
        Ok(None) => None,
        Err(error) => Some(Err(error)),
    }
}

fn drive(world: &mut World, fixture: &mut Fixture) -> Result<Option<Value>, String> {
    if let Some(pulse) = fixture.pulse.take() { let _ = world.despawn(pulse); }
    if let Some(pulse) = fixture.ui_pulse.take() && let Ok(mut button) = world.get_entity_mut(pulse) {
        button.insert(feathrise_editor_ui::EditorButtonClick::default());
    }
    if fixture.started.elapsed() > Duration::from_secs(90) {
        return Err(format!("Blender 网格验收超时 {:?}。{}", fixture.stage, world.resource::<SceneEditorState>().status));
    }
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("验收草稿已失效")?;
    let current = draft.placements.placements.iter().find(|record| record.id == fixture.original.id)
        .cloned().ok_or("选中对象已从草稿消失")?;
    if let Some(peer) = &fixture.peer
        && !draft.placements.placements.iter().any(|record| record == peer)
    {
        return Err("同源立方体的完整记录被修改".into());
    }
    let guard = world.resource::<SceneEditorNumericScrubMouseGuard>().blocked();
    match fixture.stage {
        Step::CreatePeer => {
            let Some(EditorCommand::PlacementAdded(peer)) = session.undo.last().filter(|command| {
                matches!(command, EditorCommand::PlacementAdded(record)
                    if record.id != fixture.original.id && record.translation == PEER_POSITION
                    && record.asset_target == fixture.original.asset_target)
            }) else { return Ok(None); };
            fixture.peer = Some(peer.clone());
            fixture.history_before = session.undo.len();
            fixture.stage = Step::OpenBlender;
        }
        Step::OpenBlender => {
            if open_scene_tree(world)? && queue_blender_action_for_validation(world, &current.id)? {
                fixture.stage = Step::WaitForGeometry;
            }
        }
        Step::WaitForGeometry if current != fixture.original => {
            verify_round_trip(&draft.folder, &fixture.original, &current)?;
            if !world.resource::<SceneEditorState>().dirty || session.undo.len() != fixture.history_before + 1
                || !matches!(session.undo.last(), Some(EditorCommand::PlacementChanged { before, after, .. })
                    if before == &fixture.original && after == &current)
            {
                return Err("Blender GLB 应通过一条完整作者撤销记录进入脏草稿".into());
            }
            if !step_shown(world, session, &current) { return Ok(None); }
            fixture.blender_record = Some(current);
            // 只等真实摆放采用后，通知 Blender 保存第二次。退出时原会话仍读取最终回执。
            fs::File::create_new(fixture.output.join("blender-source.accepted")).map_err(|error| error.to_string())?;
            fixture.stage = Step::WaitForExit;
        }
        Step::WaitForExit => {
            if fixture.blender_record.as_ref() != Some(&current) || session.undo.len() != fixture.history_before + 1 {
                return Err("相同 Blender GLB 重复保存改变了摆放或增加了撤销记录".into());
            }
            if !world.resource::<BlenderMeshSession>().has_open_session() {
                fixture.pulse = Some(press_history(world, EditorAction::Undo));
                fixture.stage = Step::Undo;
            }
        }
        Step::Undo if current == fixture.original && !guard => {
            if !builtin_shown(world, session, &current) { return Ok(None); }
            fixture.pulse = Some(press_history(world, EditorAction::Redo));
            fixture.stage = Step::Redo;
        }
        Step::Redo if fixture.blender_record.as_ref() == Some(&current) && !guard => {
            let peer = fixture.peer.as_ref().unwrap();
            if !step_shown(world, session, &current) || !builtin_shown(world, session, peer) { return Ok(None); }
            frame_pair(world, &current, peer);
            fixture.stage = Step::SwitchCollisionOff;
        }
        Step::SwitchCollisionOff if !guard => {
            if let Some(button) = click_collision(world)? {
                fixture.ui_pulse = Some(button);
                fixture.stage = Step::SwitchCollisionOn;
            }
        }
        Step::SwitchCollisionOn if fixture.blender_record.as_ref() != Some(&current) && !guard => {
            let before = fixture.blender_record.as_ref().unwrap();
            if session.undo.len() != fixture.history_before + 2
                || !only_collision_changed(before, &current, false)
                || !matches!(session.undo.last(), Some(EditorCommand::PlacementChanged { before: old, after, .. })
                    if old == before && after == &current)
            {
                return Err("属性中的碰撞关闭修改了其他字段或未产生独立撤销记录".into());
            }
            if let Some(button) = click_collision(world)? {
                fixture.ui_pulse = Some(button);
                fixture.stage = Step::Capture;
            }
        }
        Step::Capture if fixture.blender_record.as_ref() == Some(&current) && !guard => {
            if session.undo.len() != fixture.history_before + 3
                || !matches!(session.undo.last(), Some(EditorCommand::PlacementChanged { before, after, .. })
                    if after == &current && only_collision_changed(before, after, true))
            {
                return Err("属性中的碰撞开启未还原完整摆放并产生独立撤销记录".into());
            }
            capture(world, fixture.output.join("blender-roundtrip-feathrise.png"));
            fixture.stage = Step::WaitForCapture;
        }
        Step::WaitForCapture => {
            if let Some(capture) = world.get_resource_mut::<Capture>() {
                let Some(result) = check_ready(&mut capture.into_inner().0) else { return Ok(None); };
                result?;
                world.remove_resource::<Capture>();
                fixture.stage = Step::ReopenBlender;
            }
        }
        Step::ReopenBlender => {
            if queue_blender_action_for_validation(world, &current.id)? {
                fixture.stage = Step::ReadEvidence;
            }
        }
        Step::ReadEvidence => {
            if fixture.blender_record.as_ref() != Some(&current) || session.undo.len() != fixture.history_before + 3 {
                return Err("重开 Blender 的初始化修改了已采用的模型或历史".into());
            }
            if world.resource::<BlenderMeshSession>().has_open_session() { return Ok(None); }
            if fixture.evidence.is_none() {
                let output = fixture.output.clone();
                let digest = model_digest(&draft.folder, &current)?.to_owned();
                fixture.evidence = Some(IoTaskPool::get().spawn(async move { read_blender_evidence(output, digest) }));
            }
            if let Some(proof) = fixture.evidence.as_mut().and_then(check_ready) {
                return Ok(Some(json!({
                    "complete": true, "undo_exact": true, "redo_exact": true, "peer_unchanged": true,
                    "repeat_save_exact": true, "preview_step_shown": true,
                    "collision_inherited": true, "collision_toggled_via_properties": true,
                    "saved_record": current, "peer_record": fixture.peer, "placements_path": draft.placements_path(),
                    "blender": proof?, "screenshot": "blender-roundtrip-feathrise.png", "os_input_verified": false
                })));
            }
        }
        _ => {}
    }
    Ok(None)
}

fn verify_round_trip(folder: &str, before: &ScenePlacementRecord, after: &ScenePlacementRecord) -> Result<(), String> {
    model_digest(folder, after)?;
    if mesh_collision_enabled(&after.asset_target) != Some(true) {
        return Err("Blender 回填没有继承源立方体已开启的碰撞".into());
    }
    let mut restored = after.clone();
    restored.asset_path = before.asset_path.clone();
    restored.asset_target = before.asset_target.clone();
    if &restored != before {
        return Err("更新 Blender GLB 时改变了场景摆放字段".into());
    }
    Ok(())
}

fn only_collision_changed(before: &ScenePlacementRecord, after: &ScenePlacementRecord, enabled: bool) -> bool {
    before.asset_target.is_scene0() && after.asset_target.is_scene0()
        && mesh_collision_enabled(&before.asset_target) == Some(!enabled)
        && mesh_collision_enabled(&after.asset_target) == Some(enabled)
        && ScenePlacementRecord { asset_target: before.asset_target.clone(), ..after.clone() } == *before
}

fn click_collision(world: &mut World) -> Result<Option<Entity>, String> {
    let mut buttons = world.query_filtered::<(Entity, &SceneEditorPlacementTransformActionButton), Without<InteractionDisabled>>();
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    let nodes = nodes.query(world);
    let Some(entity) = buttons.iter(world).find_map(|(entity, button)| {
        (button.0 == PlacementTransformAction::ToggleCuboidCollision
            && crate::viewport::modes::controls_visible(entity, &nodes)).then_some(entity)
    }) else { return Ok(None); };
    // 属性栏可滚动。只点击真实布局中可见的原按钮，避免隐藏父节或裁切无声通过。
    if !ui_visible(world, entity) {
        reveal_property_control(world, entity)?;
        return Ok(None);
    }
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(Some(entity))
}

fn model_digest<'a>(folder: &str, record: &'a ScenePlacementRecord) -> Result<&'a str, String> {
    record.asset_path.strip_prefix(&format!("{folder}/models/blender-"))
        .and_then(|path| path.strip_suffix(".glb"))
        .filter(|digest| record.asset_target.is_scene0() && digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| "结果未引用当前场景的标准 GLB 版本".into())
}

pub(in crate::standalone) fn saved_model(world: &World) -> Option<ScenePlacementRecord> {
    if !enabled() { return None; }
    let draft = world.resource::<EditorAuthoringSession>().draft.as_ref()?;
    draft.placements.placements.iter().find(|record| model_digest(&draft.folder, record).is_ok()).cloned()
}

pub(in crate::standalone) fn saved_collision(world: &mut World, record: &ScenePlacementRecord) -> Option<Value> {
    let scene = world.resource::<EditorAuthoringSession>().draft.as_ref()?.scene_id.clone();
    let mut instances = world.query_filtered::<(&ScenePlacementInstance, &GlobalTransform), Without<EditorPlacedAsset>>();
    let pose = instances.iter(world).find_map(|(instance, pose)| {
        (instance.is_active() && instance.matches_record(&scene, record, &[])).then_some(*pose)
    })?;
    // 从薄台阶的两端下方探测镜像后的两个不同底面。射程不可能接触地面，也能排除包围盒和旧立方体。
    let distances = world.run_system_once(move |physics: SpatialQuery, ground: feathrise_grounding::GroundSurfaceFilter| {
        [-1.5, 1.5].map(|x| physics.cast_ray_predicate(
            pose.transform_point(Vec3::new(x, -0.125, 0.5)), Dir3::Y, 0.25, true,
            &SpatialQueryFilter::default(), &|entity| ground.is_active(entity),
        ).map(|hit| hit.distance))
    }).ok()?;
    let [Some(left), Some(right)] = distances else { return None; };
    ((left - 0.125).abs() < 0.001 && (right - 0.09375).abs() < 0.001)
        .then(|| json!({"placement_id": record.id, "lower_step_distances": [left, right]}))
}

fn builtin_shown(world: &World, session: &EditorAuthoringSession, record: &ScenePlacementRecord) -> bool {
    session.placement_previews.get(&record.id).is_some_and(|&root|
        world.get::<WorldAssetRoot>(root).is_none() && feathrise_scene::builtin_placement_visual_ready(world, root))
}

fn step_shown(world: &World, session: &EditorAuthoringSession, record: &ScenePlacementRecord) -> bool {
    let Some(&root) = session.placement_previews.get(&record.id) else { return false; };
    let Some(instance) = world.get::<ScenePlacementInstance>(root) else { return false; };
    if !instance.is_active() || instance.asset_path() != record.asset_path || !instance.asset_target().is_scene0()
        || !world.get::<WorldAssetRoot>(root).and_then(|asset| asset.0.path()).is_some_and(|path|
            path.path() == Path::new(&record.asset_path) && path.label() == Some("Scene0"))
    {
        return false;
    }
    let Some(root_transform) = world.get::<GlobalTransform>(root) else { return false; };
    let to_model = root_transform.affine().inverse();
    let mut stack = vec![root];
    let mut points = Vec::new();
    while let Some(entity) = stack.pop() {
        if let Some(children) = world.get::<Children>(entity) { stack.extend(children.iter()); }
        let Some(handle) = world.get::<Mesh3d>(entity) else { continue; };
        let Some(mesh) = world.resource::<Assets<Mesh>>().get(handle.id()) else { return false; };
        let Ok(VertexAttributeValues::Float32x3(positions)) = mesh.try_attribute(Mesh::ATTRIBUTE_POSITION)
            else { return false; };
        let Some(transform) = world.get::<GlobalTransform>(entity) else { return false; };
        let material = world.get::<MeshMaterial3d<StandardMaterial>>(entity)
            .and_then(|handle| world.resource::<Assets<StandardMaterial>>().get(handle.id()));
        if !material.is_some_and(|mat| {
            let color = mat.base_color.to_linear();
            (color.red - 0.8).abs() < 0.001 && (color.green - 0.05).abs() < 0.001 && (color.blue - 0.125).abs() < 0.001
        }) { return false; }
        // 探针使用真实场景子网格和节点变换，移除摆放根的世界位姿后核对非对称截面。
        let local = to_model * transform.affine();
        points.extend(positions.iter().map(|point| local.transform_point3(Vec3::from_array(*point))));
    }
    let expected: Vec<_> = STEP_PROFILE.iter().flat_map(|&[x, y]| [-1.5, 1.5].map(|z| Vec3::new(x, y, z))).collect();
    let covered = |targets: &[Vec3], samples: &[Vec3]| targets.iter().all(|point|
        samples.iter().any(|sample| point.abs_diff_eq(*sample, 0.001)));
    covered(&expected, &points) && covered(&points, &expected)
}

fn press_history(world: &mut World, action: EditorAction) -> Entity {
    world.spawn((SceneEditorActionButton(action), feathrise_editor_ui::EditorButtonClick::triggered())).id()
}

fn frame_pair(world: &mut World, target: &ScenePlacementRecord, peer: &ScenePlacementRecord) {
    world.resource_mut::<SceneEditorState>().orthographic = false;
    let mut view = world.resource_mut::<SceneEditorViewportState>();
    view.center = Vec3::from_array(target.translation).lerp(Vec3::from_array(peer.translation), 0.5);
    view.extent = 8.0;
    view.yaw = 0.72;
    view.pitch = 0.55;
}

fn capture(world: &mut World, path: PathBuf) {
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>, mut commands: Commands| {
        let image = event.image.clone();
        let path = path.clone();
        commands.insert_resource(Capture(IoTaskPool::get().spawn(async move {
            image.try_into_dynamic().map_err(|error| error.to_string())?.save(path).map_err(|error| error.to_string())
        })));
    });
}

fn read_blender_evidence(output: PathBuf, digest: String) -> Result<Value, String> {
    let read = |phase: &str| -> Result<Value, String> {
        let image = output.join(format!("{phase}.png"));
        if !fs::metadata(&image).is_ok_and(|data| data.is_file() && data.len() > 0) {
            return Err(format!("实际 Blender 窗口缺少截图 {}", image.display()));
        }
        let bytes = fs::read(output.join(format!("{phase}.json"))).map_err(|error| error.to_string())?;
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
    };
    let first = read("blender-source")?;
    let second = read("blender-reopened")?;
    if first["glb_sha256"] != digest || first["repeat_export_exact"] != true
        || second["source_sha256"] != digest || second["step_restored"] != true
        || first["session"] == second["session"]
    {
        return Err("Blender 保存出件、场景采用版本或重开模型不一致".into());
    }
    for (phase, saves) in [("blender-source", 3), ("blender-reopened", 1)] {
        let log = fs::read_to_string(output.join(format!("{phase}.log"))).map_err(|error| error.to_string())?;
        if log.matches("[Bevy GLB 自动导出] 已更新").count() != saves {
            return Err(format!("{phase} 的 Blender GLB 出件日志数量不符"));
        }
    }
    Ok(json!({"first": first, "second": second}))
}
