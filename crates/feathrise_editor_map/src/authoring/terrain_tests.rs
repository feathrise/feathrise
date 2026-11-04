//! 任意GLB拓扑地形笔刷与预览接管回归。

use std::{collections::BTreeMap, sync::Arc};

use super::*;
use feathrise_map_content::{
    MapId,
};
use feathrise_map_terrain::{
    TerrainMeshInput, TerrainSampleSpec, build_terrain_editing_index, derive_terrain,
};

pub(crate) use super::super::test_support::test_heightfield_draft;

#[test]
fn raise_and_lower_use_world_meter_speed() {
    assert_eq!(
        raise_lower_delta_meters(EditorTool::Raise, 20.0, 0.5, 0.25, 1.0),
        2.5
    );
    assert_eq!(
        raise_lower_delta_meters(EditorTool::Lower, 20.0, 0.5, 0.25, 1.0),
        -2.5
    );
    assert_eq!(
        raise_lower_delta_meters(EditorTool::Raise, 40.0, 0.5, 0.25, 1.0),
        5.0
    );
    assert_eq!(
        raise_lower_delta_meters(EditorTool::Smooth, 20.0, 1.0, 1.0, 1.0),
        0.0
    );
}

#[test]
fn outside_brush_clips_to_actual_edge_vertices_and_avoids_unneeded_cow() {
    let mut draft = test_heightfield_draft(Arc::new(vec![1_000; 121]));
    let mut before = BTreeMap::new();
    assert!(modify_vertex_neighborhood(
        EditorTool::Raise,
        Vec3::new(-0.5, 1_000.0, 5.0),
        1.0,
        1.0,
        10.0,
        1.0,
        None,
        None,
        &mut before,
        Some(&mut draft),
    ));
    let mesh = draft.terrain_mesh.as_ref().expect("GLB仍存在");
    assert!(mesh.positions[55][1] > 1_000.0);
    assert_eq!(mesh.positions[56][1], 1_000.0);

    let mesh_pointer = Arc::as_ptr(mesh);
    let mut untouched = BTreeMap::new();
    assert!(!modify_vertex_neighborhood(
        EditorTool::Raise,
        Vec3::new(-2.1, 1_000.0, 5.0),
        1.0,
        1.0,
        10.0,
        1.0,
        None,
        None,
        &mut untouched,
        Some(&mut draft),
    ));
    assert_eq!(
        Arc::as_ptr(draft.terrain_mesh.as_ref().expect("GLB仍存在")),
        mesh_pointer
    );
    assert!(untouched.is_empty());
}

#[test]
fn background_snapshot_blocks_in_radius_edit_without_sync_cow() {
    let mut draft = test_heightfield_draft(Arc::new(vec![1_000; 121]));
    let observer = Arc::clone(draft.terrain_mesh.as_ref().expect("GLB存在"));
    let original_height = observer.positions[60][1];
    let mut untouched = BTreeMap::new();

    assert!(!modify_vertex_neighborhood(
        EditorTool::Raise,
        Vec3::new(5.0, original_height, 5.0),
        1.0,
        1.0,
        10.0,
        1.0,
        None,
        None,
        &mut untouched,
        Some(&mut draft),
    ));

    let author_mesh = draft.terrain_mesh.as_ref().expect("GLB仍存在");
    assert!(Arc::ptr_eq(author_mesh, &observer));
    assert_eq!(author_mesh.positions[60][1], original_height);
    assert!(untouched.is_empty());
}

#[test]
fn smoothing_reads_real_topology_neighbors() {
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    let mesh = Arc::make_mut(draft.terrain_mesh.as_mut().expect("GLB存在"));
    mesh.positions[60][1] = 10.0; // 中心顶点与规则索引中的真实一环连接。
    let mut before = BTreeMap::new();
    assert!(modify_vertex_neighborhood(
        EditorTool::Smooth,
        Vec3::new(5.0, 10.0, 5.0),
        0.2,
        1.0,
        1.0,
        0.25,
        None,
        None,
        &mut before,
        Some(&mut draft),
    ));
    let center = draft.terrain_mesh.as_ref().expect("GLB存在").positions[60][1];
    assert!(center < 10.0 && center >= 0.0);
    assert_eq!(before.get(&60), Some(&10.0));
}

#[test]
fn legacy_height_smooth_and_flatten_consume_the_shared_sculpt_speed() {
    for tool in [EditorTool::Smooth, EditorTool::Flatten] {
        for speed in [4., 40.] {
            let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
            Arc::get_mut(draft.terrain_mesh.as_mut().unwrap()).unwrap().positions[60][1] = 10.;
            let mut before = BTreeMap::new();
            assert!(modify_vertex_neighborhood(tool, Vec3::new(5., 10., 5.), 0.2, 0.5, speed, 0.05,
                Some(2.), None, &mut before, Some(&mut draft)));
            let height = draft.terrain_mesh.as_ref().unwrap().positions[60][1];
            assert!((height - (10. - speed * 0.5 * 0.05)).abs() < 0.00001);
            assert_eq!(before.get(&60), Some(&10.));
        }
    }
}

#[test]
fn smoothing_keeps_blender_attribute_split_seams_single_valued() {
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    let mesh = TerrainMeshInput {
        positions: vec![
            [4.0, 0.0, 4.0],  // NW只属于首面。
            [4.0, 0.0, 6.0],  // SW首份属性顶点。
            [6.0, 10.0, 4.0], // NE首份属性顶点。
            [6.0, 10.0, 4.0], // NE由Blender/glTF属性边界拆出的副本。
            [4.0, 0.0, 6.0],  // SW对应副本。
            [6.0, 10.0, 6.0], // SE只属于次面。
        ],
        indices: vec![0, 1, 2, 3, 4, 5], // 几何连续的共边在索引层完全断开。
    }; // 该夹具会让旧逐索引一环算法把两个NE副本分别平滑到不同高度。
    let editing_index = build_terrain_editing_index(&mesh).expect("属性拆点地形必须建立编辑索引");
    draft.terrain_mesh = Some(Arc::new(mesh)); // 安装唯一作者写侧。
    draft.terrain_editing_index = Some(Arc::new(editing_index)); // 安装同拓扑焊接索引。
    draft.manifest.world_min = [4.0, 4.0]; // 派生规格严格覆盖测试四边形。
    draft.manifest.world_max = [6.0, 6.0];
    draft.manifest.terrain.resolution = [3, 3]; // 边、中心和共享对角线均被采样。
    draft.manifest.terrain.height_scale = 20.0; // 完整覆盖零至十米测试坡面。
    let mut before = BTreeMap::new();
    assert!(modify_vertex_neighborhood(
        EditorTool::Smooth,
        Vec3::new(6.0, 10.0, 4.0),
        0.2,
        1.0,
        1.0,
        0.25,
        None,
        None,
        &mut before,
        Some(&mut draft),
    )); // 满强度单帧平滑只命中两个NE属性副本。
    let mesh = draft.terrain_mesh.as_ref().expect("GLB仍存在");
    assert_eq!(mesh.positions[2][1], mesh.positions[3][1]); // 同一逻辑点必须原子写入完全相同的f32高度。
    assert_eq!(before.get(&2), Some(&10.0)); // 撤销保留首份源顶点精确旧值。
    assert_eq!(before.get(&3), Some(&10.0)); // 属性副本同样拥有独立源索引撤销记录。
    derive_terrain(
        (**mesh).clone(),
        TerrainSampleSpec {
            resolution: [3, 3],
            world_min: [4.0, 4.0],
            world_max: [6.0, 6.0],
            base_height: 0.0,
            height_scale: 20.0,
        },
    )
    .expect("焊接平滑后的属性拆点共边必须继续通过三维地形派生");
}

#[test]
fn sculpting_an_overlapping_cave_edits_only_the_hit_topology_layer() {
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121])); // 复用动态地图清单与作者状态夹具。
    let mesh = TerrainMeshInput {
        positions: vec![
            [-1.0, 10.0, -1.0],
            [1.0, 10.0, -1.0],
            [-1.0, 10.0, 1.0],
            [1.0, 10.0, 1.0],
            [-1.0, 0.0, -1.0],
            [1.0, 0.0, -1.0],
            [-1.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
        ], // 完全重合投影的上层室外地面与下层洞穴地板。
        indices: vec![0, 2, 1, 1, 2, 3, 4, 6, 5, 5, 6, 7], // 两层拓扑彼此断开且各自完整。
    }; // 完成洞穴多层笔刷夹具。
    let editing_index = build_terrain_editing_index(&mesh).expect("洞穴多层必须建立编辑索引");
    draft.terrain_mesh = Some(Arc::new(mesh)); // 安装唯一作者网格写侧。
    draft.terrain_editing_index = Some(Arc::new(editing_index)); // 安装完整XYZ焊接和邻接索引。
    draft.manifest.world_min = [-1.0, -1.0]; // 笔刷与实际双层网格范围一致。
    draft.manifest.world_max = [1.0, 1.0];
    draft.manifest.terrain.base_height = -10.0; // 上下两层均位于合法编码范围。
    draft.manifest.terrain.height_scale = 30.0;
    let mut before = BTreeMap::new(); // 收集本次局部源顶点旧高度。

    assert!(modify_vertex_neighborhood(
        EditorTool::Raise,
        Vec3::new(0.0, 10.0, 0.0),
        2.0,
        1.0,
        1.0,
        1.0,
        None,
        Some(0), // 视口命中上层首三角形。
        &mut before,
        Some(&mut draft),
    )); // 圆内同时存在两层顶点，但局部拓扑筛选只采用命中层。
    let positions = &draft.terrain_mesh.as_ref().expect("GLB仍存在").positions;
    assert!(positions[..4].iter().all(|position| position[1] > 10.0)); // 上层四点沿真实拓扑连通并被抬高。
    assert!(positions[4..].iter().all(|position| position[1] == 0.0)); // 同X/Z洞穴地板不得被外部一笔穿透修改。
    assert_eq!(before.len(), 4); // 撤销差分只记录实际命中层的四个源顶点。
} // 结束洞穴笔刷分层回归。

#[test]
fn clean_terrain_keeps_runtime_pbr_until_dirty_surface_preview_is_current() {
    let mut app = App::new();
    app.init_resource::<MapEditorState>()
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<SurfacePreviewState>()
        .add_systems(Update, sync_terrain_preview_visibility);
    let draft = test_heightfield_draft(Arc::new(vec![1_000; 121]));
    let map_id = draft.map_id.clone();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.open = true;
        editor.selected_map_id = Some(map_id.clone());
    }
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft = Some(draft);
    {
        let mut preview = app.world_mut().resource_mut::<TerrainPreviewState>();
        preview.applied_map_id = Some(map_id.clone());
        preview.applied_revision = 0;
    }
    {
        let mut preview = app.world_mut().resource_mut::<SurfacePreviewState>();
        preview.desired_map_id = Some(map_id.clone());
        preview.applied_map_id = Some(map_id.clone());
        preview.desired_revision = 0;
        preview.applied_revision = 0;
        preview.desired_preset_revision = 7;
        preview.applied_preset_revision = 7;
        preview.material = Some(Handle::default()); // 模拟已经完成GPU预热的完整PBR材质。
    }
    let preview_entity = app
        .world_mut()
        .spawn((
            EditorTerrainPreview,
            Visibility::Inherited,
            MeshMaterial3d::<StandardMaterial>(Handle::default()),
        )) // 放入历史纯色组件，确认接管时会清理。
        .id();
    let runtime_entity = app
        .world_mut()
        .spawn((DynamicMapTerrainPrimitive, Visibility::Inherited))
        .id();
    app.update();
    assert_eq!(
        app.world()
            .entity(preview_entity)
            .get::<Visibility>()
            .copied(),
        Some(Visibility::Hidden)
    );
    assert_eq!(
        app.world()
            .entity(runtime_entity)
            .get::<Visibility>()
            .copied(),
        Some(Visibility::Inherited)
    );

    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .expect("测试草稿存在")
        .surface_dirty = true;
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .material = None; // 首次GPU候选未完成，不得显示无材质的作者网格。
    app.update(); // 先验证首次等待与后续保留旧结果两种路径不同。
    assert_eq!(
        app.world().entity(preview_entity).get::<Visibility>(),
        Some(&Visibility::Hidden)
    ); // 没有完整PBR时作者网格必须隐藏。
    assert_eq!(
        app.world().entity(runtime_entity).get::<Visibility>(),
        Some(&Visibility::Inherited)
    ); // 初次等待仍显示原正式地表。
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .material = Some(Handle::default()); // 模拟完整GPU候选原子采用。
    app.update();
    assert_eq!(
        app.world()
            .entity(preview_entity)
            .get::<Visibility>()
            .copied(),
        Some(Visibility::Inherited)
    ); // 只有当前未保存PBR完整就绪后，作者预览才接管画面。
    assert!(
        app.world()
            .entity(preview_entity)
            .contains::<MeshMaterial3d<TerrainSurfaceMaterial>>()
    ); // 可见作者网格必须实际绑定正式地表材质。
    assert!(
        !app.world()
            .entity(preview_entity)
            .contains::<MeshMaterial3d<StandardMaterial>>()
    ); // 旧纯色材质必须移除，不能混合两条渲染路径。
    assert_eq!(
        app.world()
            .entity(runtime_entity)
            .get::<Visibility>()
            .copied(),
        Some(Visibility::Hidden)
    );

    app.init_resource::<feathrise_scene::DynamicSurfaceAuthorPreview>(); // 正式资源全局存在不能误禁用浏览其他地图。
    app.insert_resource(feathrise_map_content::ActiveDynamicMap {
        map_id: Some(MapId("OTHER-MAP".to_owned())),
    }); // 游戏仍在另一图。
    app.update(); // 编辑器当前草稿继续独立预览。
    assert_eq!(
        app.world().get::<Visibility>(preview_entity),
        Some(&Visibility::Inherited)
    ); // 非当前游戏地图保持作者网格。
    app.world_mut()
        .resource_mut::<feathrise_map_content::ActiveDynamicMap>()
        .map_id = Some(map_id.clone()); // 当前草稿变为游戏地图。
    app.update(); // 由正式源统一承接PBR材质。
    assert_eq!(
        app.world().get::<Visibility>(preview_entity),
        Some(&Visibility::Hidden)
    ); // 不再用替身隐藏正式地形。
    assert_eq!(
        app.world().get::<Visibility>(runtime_entity),
        Some(&Visibility::Inherited)
    ); // 游戏正式地形保持可见。
    app.world_mut()
        .remove_resource::<feathrise_scene::DynamicSurfaceAuthorPreview>(); // 后续仍验证无scene插件独立预览退路。
    app.world_mut()
        .remove_resource::<feathrise_map_content::ActiveDynamicMap>(); // 恢复原测试配置。
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .desired_revision = 1;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .surface_revision = 1; // 新笔划推进作者修订，旧完整GPU预览仍有效。
    app.update();
    assert_eq!(
        app.world()
            .entity(preview_entity)
            .get::<Visibility>()
            .copied(),
        Some(Visibility::Inherited)
    ); // 新地表仍在后台准备时保留上一完整作者PBR，不能闪回磁盘上的旧图。
    assert_eq!(
        app.world()
            .entity(runtime_entity)
            .get::<Visibility>()
            .copied(),
        Some(Visibility::Hidden)
    );

    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .terrain_revision = 1; // 地形已改但新网格未完成时不能显示旧拓扑。
    app.update(); // 核对地形修订门仍严格生效。
    assert_eq!(
        app.world().entity(preview_entity).get::<Visibility>(),
        Some(&Visibility::Hidden)
    ); // 旧拓扑不能冒充当前预览。
    assert_eq!(
        app.world().entity(runtime_entity).get::<Visibility>(),
        Some(&Visibility::Inherited)
    ); // 正式网格恢复原显隐。
    app.world_mut()
        .resource_mut::<TerrainPreviewState>()
        .applied_revision = 1; // 当前地形完整后恢复作者地表。
    app.update(); // 接管当前拓扑。
    assert_eq!(
        app.world().entity(preview_entity).get::<Visibility>(),
        Some(&Visibility::Inherited)
    ); // 当前网格可使用已完整地表。
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .expect("草稿存在")
        .surface_dirty = false; // 保存清理脏标记后归还运行时。
    app.update(); // 保存与关闭各自覆盖恢复分支。
    assert_eq!(
        app.world().entity(runtime_entity).get::<Visibility>(),
        Some(&Visibility::Inherited)
    ); // 保存后运行时恢复。
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .expect("草稿存在")
        .surface_dirty = true; // 重新制造未保存状态验证关闭。
    app.update(); // 再次接管。
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert_eq!(
        app.world()
            .entity(preview_entity)
            .get::<Visibility>()
            .copied(),
        Some(Visibility::Hidden)
    );
    assert_eq!(
        app.world()
            .entity(runtime_entity)
            .get::<Visibility>()
            .copied(),
        Some(Visibility::Inherited)
    );
}

#[path = "terrain_rebuild_tests.rs"]
mod rebuild; // 真实ECS派生任务验证对象源、失败去重和迟到结果隔离。
