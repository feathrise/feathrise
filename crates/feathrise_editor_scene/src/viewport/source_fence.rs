//! 缓存子场景占用范围：每个落点最多检查64个索引节点，缺页也不能穿透写父图。
use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, Task, futures::check_ready}};
use feathrise_scene_content::{ExpandedSceneInstance, SceneId, SceneInstanceGraph, SceneInstanceGraphPhase};
use std::sync::Arc;
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState};
use crate::authoring::terrain_ground::TerrainGroundCursor;
use super::vegetation_ground::VegetationGroundCursor;

#[derive(Clone, PartialEq, Eq)]
struct Identity { scene_id: SceneId, graph: u64, snapshot: usize }
#[derive(Clone)]
struct Area { world: Rect, local: Rect, inverse: Mat4 }
enum Kind { Leaf(Area), Branch([usize; 2]) }
struct Node { bounds: Rect, kind: Kind }
#[derive(Resource, Default)]
pub(crate) struct SourceAuthoringFence {
    task: Option<Task<(Identity, Arc<Vec<Node>>)>>,
    release: Option<Task<()>>,
    accepted: Option<(Identity, Arc<Vec<Node>>)>,
}

fn build(instances: &[ExpandedSceneInstance]) -> Arc<Vec<Node>> {
    fn node(areas: &mut [Area], nodes: &mut Vec<Node>) -> usize {
        let mut bounds = areas[0].world;
        for area in &areas[1..] { bounds.min = bounds.min.min(area.world.min); bounds.max = bounds.max.max(area.world.max); }
        let slot = nodes.len();
        nodes.push(Node { bounds, kind: Kind::Leaf(areas[0].clone()) });
        if areas.len() > 1 {
            let axis = usize::from(bounds.width() < bounds.height());
            let middle = areas.len() / 2;
            areas.select_nth_unstable_by(middle, |a, b| a.world.center()[axis].total_cmp(&b.world.center()[axis]));
            let (left, right) = areas.split_at_mut(middle);
            let children = [node(left, nodes), node(right, nodes)];
            nodes[slot].kind = Kind::Branch(children);
        }
        slot
    }
    // 子场景容器不封锁空处；移出旧范围的网格仍受保护，父图自己的网格继续可编辑。
    let mut areas: Vec<_> = instances.iter().filter(|instance| !instance.path.instances.is_empty() && !instance.container).map(|instance| {
        let scale = instance.terrain.as_ref().map_or(Vec3::ONE, |terrain| Vec3::from_array(terrain.transform.scale));
        let up = instance.transform.as_transform().rotation * Vec3::Y;
        // 源高度可雕刻到创建域之外；倾斜后其投影不能用旧高度盒裁掉。
        // 仅放开粗筛，仍由完整逆变换后的源矩形精查，固定64节点预算继续保守关门。
        let world = if up.x != 0.0 || up.z != 0.0 {
            Rect::from_corners(Vec2::splat(-f32::MAX), Vec2::splat(f32::MAX))
        } else { instance.world_bounds };
        Area { world, local: instance.local_bounds,
            inverse: instance.transform.as_transform().with_scale(scale).to_matrix().inverse() }
    }).collect();
    let mut nodes = Vec::with_capacity(areas.len().saturating_mul(2));
    if !areas.is_empty() { node(&mut areas, &mut nodes); }
    Arc::new(nodes)
}

fn occupied(nodes: &[Node], point: Vec3) -> bool {
    if nodes.is_empty() { return false; }
    let mut stack = [0_usize; 64]; let mut count = 1;
    for _ in 0..64 {
        if count == 0 { return false; }
        count -= 1; let node = &nodes[stack[count]];
        if !node.bounds.contains(point.xz()) { continue; }
        match &node.kind {
            Kind::Leaf(area) => {
                if area.local.contains(area.inverse.transform_point3(point).xz()) { return true; }
            }
            Kind::Branch(children) => {
                if count + 2 > stack.len() { return true; }
                stack[count..count + 2].copy_from_slice(children); count += 2;
            }
        }
    }
    count != 0 // 极密集重叠达到固定查询预算时保守关闭，不在帧内退化成完整扫描。
}

pub(crate) fn update(mut fence: ResMut<SourceAuthoringFence>, mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>, graph: Option<Res<SceneInstanceGraph>>,
    mut ground: ResMut<VegetationGroundCursor>, objects_cursor: Option<Res<TerrainGroundCursor>>,
    mut water: Option<ResMut<super::water_plane::WaterPlaneCursor>>) {
    if fence.release.as_mut().and_then(check_ready).is_some() { fence.release = None; }
    let draft = session.draft.as_ref();
    let has_references = draft.is_some_and(|draft| !draft.manifest.subscenes.is_empty());
    let graph = graph.as_deref().filter(|graph| draft.is_some_and(|draft| graph.root_scene_id.as_ref() == Some(&draft.scene_id)));
    let identity = graph.filter(|graph| graph.phase == SceneInstanceGraphPhase::Ready).map(|graph| Identity {
        scene_id: graph.root_scene_id.clone().unwrap(), graph: graph.revision,
        snapshot: Arc::as_ptr(&graph.instances) as *const () as usize });
    if let Some((key, nodes)) = fence.task.as_mut().and_then(check_ready) {
        fence.task = None;
        let old = if identity.as_ref() == Some(&key) { fence.accepted.replace((key, nodes)).map(|(_, nodes)| nodes) }
            else { Some(nodes) };
        if let Some(old) = old { fence.release = Some(AsyncComputeTaskPool::get().spawn(async move { drop(old); })); }
    }
    if let Some(identity) = &identity
        && fence.accepted.as_ref().is_none_or(|(key, _)| key != identity)
        && fence.task.is_none() && fence.release.is_none() {
        let instances = graph.unwrap().instances.clone(); let identity = identity.clone();
        fence.task = Some(AsyncComputeTaskPool::get().spawn(async move { (identity, build(&instances)) }));
    }
    let points = ground.source_fence_points();
    let paint = objects_cursor.as_deref().and_then(|cursor| cursor.paint_hit());
    // 异步连续笔刷可以消费原射线完成点；严格悬停空窗不能绕过子场景只读保护。
    // 只补两个受检作者点，查询仍有固定上界；纯显示圆环不授予任何写入权限。
    let points = [editor.cursor_world, editor.brush_cursor_world, points[0], points[1], points[2],
        paint.map(|hit| hit.point), paint.and_then(|hit| hit.from)];
    let bypass = !editor.open || editor.tool == crate::types::EditorTool::Inspect
        || (!has_references && graph.is_none_or(|graph| graph.instances.len() <= 1)); // 整体选择保留真实父地面遮挡，保护门只约束内部作者操作。
    let index = fence.accepted.as_ref().filter(|(key, _)| identity.as_ref() == Some(key));
    let blocked = !bypass && index.is_some_and(|(_, nodes)| points.iter().flatten().any(|point| occupied(nodes, *point)));
    let allowed = bypass || (index.is_some() && !blocked);
    ground.source_fence_closed = !allowed;
    if !allowed {
        if let Some(water) = water.as_mut() { water.point = None; water.world = None; } // 只读区域同样切断平面轨迹，返回可写区域后不得跨区补线。
        editor.cursor_world = None; editor.cursor_triangle = None;
        editor.brush_cursor_world = None; editor.brush_cursor_triangle = None;
        ground.paint_ready = false;
        if blocked {
            ground.reject_source_click();
            editor.status = "此处属于子场景范围，请打开源场景编辑内部内容".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::*;
    use std::collections::BTreeMap;
    #[test]
    fn readonly_source_area_uses_rotated_local_bounds() {
        let root = SceneId::parse("MAP-parent").unwrap(); let source = SceneId::parse("MAP-source").unwrap();
        let sources = BTreeMap::from([(root.clone(), Arc::new(SceneInstanceSource { terrains: None, scene_id: root.clone(), revision: 1,
            world_bounds: Rect::from_corners(Vec2::splat(-100.0), Vec2::splat(100.0)),
            instances: Arc::from([SceneInstancePlacement { id: SceneInstanceId::parse("child").unwrap(), source_scene_id: source.clone(),
                label: String::new(), transform: SceneInstanceTransform { translation: [10.0, 8.0, 20.0], yaw_degrees: 45.0,  ..Default::default() } }]) })),
            (source.clone(), Arc::new(SceneInstanceSource { terrains: None, scene_id: source, revision: 1,
                world_bounds: Rect::from_corners(Vec2::splat(-2.0), Vec2::splat(2.0)), instances: Arc::from([]) }))]);
        let expanded = expand_scene_instances(&root, &sources).unwrap(); let nodes = build(&expanded);
        assert!(occupied(&nodes, Vec3::new(10.0, 0.0, 20.0))); // 源高于父地面也不能穿透修改父图。
        let corner = Vec3::new(12.5, 0.0, 22.5);
        assert!(expanded[1].world_bounds.contains(corner.xz()));
        assert!(!occupied(&nodes, corner)); // 世界AABB角落实际不在旋转源图内。
        assert!(!occupied(&nodes, Vec3::ZERO));
    }

    #[test]
    fn child_container_does_not_fence_empty_space_but_its_offset_mesh_is_protected() {
        let root = SceneId::parse("MAP-parent").unwrap();
        let child = SceneId::parse("MAP-child").unwrap();
        let object = SceneTerrainObject {clipmap_stitches: Default::default(),
            mesh_lod: None, id: TerrainId::parse("ground").unwrap(), display_name: "子图网格".into(), enabled: true,
            transform: TerrainObjectTransform { translation: [5000.0, 0.0, 0.0], scale: [2.0, 1.0, 3.0], ..Default::default() },
            world_min: [-10.0; 2], world_max: [10.0; 2], terrain: SceneTerrainDescriptor::empty(),
            voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None,
        };
        let old_bounds = Rect::from_corners(Vec2::splat(-0.5), Vec2::splat(0.5));
        let sources = BTreeMap::from([
            (root.clone(), Arc::new(SceneInstanceSource { scene_id: root.clone(), revision: 1, world_bounds: old_bounds,
                terrains: Some(Arc::from([])), instances: Arc::from([SceneInstancePlacement {
                    id: SceneInstanceId::parse("child").unwrap(), source_scene_id: child.clone(), label: String::new(), transform: Default::default(),
                }]) })),
            (child.clone(), Arc::new(SceneInstanceSource { scene_id: child, revision: 1, world_bounds: old_bounds,
                terrains: Some(Arc::from([object])), instances: Arc::from([]) })),
        ]);
        let nodes = build(&expand_scene_instances(&root, &sources).unwrap());
        assert!(!occupied(&nodes, Vec3::ZERO));
        assert!(occupied(&nodes, Vec3::new(5019.0, 0.0, 29.0)));
        assert!(!occupied(&nodes, Vec3::new(5021.0, 0.0, 0.0)));
    }

    #[test]
    fn tilted_child_fence_accepts_real_height_beyond_the_creation_domain() {
        let root = SceneId::parse("MAP-parent").unwrap();
        let child = SceneId::parse("MAP-child").unwrap();
        let bounds = Rect::from_corners(Vec2::splat(-2.0), Vec2::splat(2.0));
        let object = SceneTerrainObject { clipmap_stitches: default(), mesh_lod: None,
            id: TerrainId::parse("tilted-ground").unwrap(), display_name: "倾斜网格".into(), enabled: true,
            transform: TerrainObjectTransform { pitch_degrees: 40.0, roll_degrees: 20.0, ..default() },
            world_min: bounds.min.to_array(), world_max: bounds.max.to_array(),
            terrain: SceneTerrainDescriptor { base_height: -1.0, height_scale: 2.0, ..SceneTerrainDescriptor::empty() },
            voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None };
        let sources = BTreeMap::from([
            (root.clone(), Arc::new(SceneInstanceSource { scene_id: root.clone(), revision: 1, world_bounds: bounds,
                terrains: Some(Arc::from([])), instances: Arc::from([SceneInstancePlacement {
                    id: SceneInstanceId::parse("child").unwrap(), source_scene_id: child.clone(), label: String::new(), transform: default() }]) })),
            (child.clone(), Arc::new(SceneInstanceSource { scene_id: child, revision: 1, world_bounds: bounds,
                terrains: Some(Arc::from([object])), instances: Arc::from([]) })),
        ]);
        let expanded = expand_scene_instances(&root, &sources).unwrap();
        let mesh = expanded.iter().find(|node| node.terrain.is_some()).unwrap();
        let point = mesh.world_pose().transform_point(Vec3::new(0.0, 200.0, 0.0));
        assert!(!mesh.world_bounds.contains(point.xz()), "创建高度域不代表当前实际雕刻高度");
        let nodes = build(&expanded);
        assert!(occupied(&nodes, point), "倾斜后的真实命中不能穿透子场景作者保护");
        assert!(!occupied(&nodes, mesh.world_pose().transform_point(Vec3::new(3.0, 200.0, 0.0))));
    }

    #[test]
    fn continuous_paint_completed_points_obey_the_child_source_fence_without_strict_hover() {
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let root = draft.scene_id.clone();
        let child = SceneId::parse("MAP-readonly-child").unwrap();
        let bounds = Rect::from_corners(Vec2::splat(-2.0), Vec2::splat(2.0));
        let sources = BTreeMap::from([
            (root.clone(), Arc::new(SceneInstanceSource { scene_id: root.clone(), revision: 1,
                world_bounds: bounds, terrains: None, instances: Arc::from([SceneInstancePlacement {
                    id: SceneInstanceId::parse("child").unwrap(), source_scene_id: child.clone(), label: String::new(),
                    transform: SceneInstanceTransform { translation: [10.0, 0.0, 0.0], yaw_degrees: 30.0,  ..Default::default() },
                }]) })),
            (child.clone(), Arc::new(SceneInstanceSource { scene_id: child, revision: 1,
                world_bounds: bounds, terrains: None, instances: Arc::from([]) })),
        ]);
        let instances: Arc<[ExpandedSceneInstance]> = expand_scene_instances(&root, &sources).unwrap().into();
        let identity = Identity { scene_id: root.clone(), graph: 1, snapshot: Arc::as_ptr(&instances) as *const () as usize };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(SourceAuthoringFence { accepted: Some((identity, build(&instances))), ..default() })
            .insert_resource(SceneInstanceGraph { root_scene_id: Some(root), revision: 1, instances,
                phase: SceneInstanceGraphPhase::Ready, ..default() })
            .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
            .insert_resource(SceneEditorState { open: true, tool: crate::types::EditorTool::Vegetation, ..default() })
            .init_resource::<VegetationGroundCursor>()
            .init_resource::<TerrainGroundCursor>()
            .add_systems(Update, update);
        let target = TerrainId::parse("paint-grid").unwrap();
        for from in [false, true] {
            app.world_mut().resource_mut::<TerrainGroundCursor>().test_paint_hit(target.clone(),
                if from { Vec3::ZERO } else { Vec3::X * 10.0 }, from.then_some(Vec3::X * 10.0));
            app.update();
            assert!(app.world().resource::<VegetationGroundCursor>().source_fence_closed,
                "严格悬停为空时，本帧受理的点和刷段起点仍不能写子场景");
        }
        *app.world_mut().resource_mut::<TerrainGroundCursor>() = default();
        app.update();
        assert!(!app.world().resource::<VegetationGroundCursor>().source_fence_closed);
    }
}
