use bevy::world_serialization::WorldAsset;

use super::*;

#[test]
fn scene_mesh_index_keeps_original_glb_identity_after_display_lod_swap() {
    let mut assets = Assets::<Mesh>::default();
    let original = assets.add(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
    let display = assets.add(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
    let mut world = World::new();
    let root = world.spawn_empty().id();
    let node = world.spawn(Name::new("建筑")).id();
    let primitive = world.spawn((
        Mesh3d(display), GltfMeshName("墙体".into()),
        feathrise_scene::StaticMeshLodOriginal { mesh: original.clone() },
    )).id();
    world.entity_mut(root).add_child(node);
    world.entity_mut(node).add_child(primitive);
    let mut build = BrowserSceneBuild::new("gltf/building.glb".into(), Handle::<WorldAsset>::default().id(), 1, &world);
    for _ in 0..16 {
        if advance_scene_build(&mut build, &world, |mesh| (mesh.0 == original).then_some(42)) { break; }
    }
    assert!(build.completed);
    assert_eq!(build.meshes.len(), 1);
    assert_eq!(build.meshes[0].mesh_index, Some(42));
    assert_eq!(build.meshes[0].node_path, ["建筑"]);
    assert!(build.meshes[0].valid); // 显示用的派生句柄不能把原作者稳定引用变成孤儿网格。
}

#[test]
fn worst_case_world_requires_many_bounded_updates() {
    let mut world = World::new();
    for _ in 0..MAX_GLTF_WORLD_ENTITIES {
        world.spawn_empty(); // 8192个根同时覆盖根发现和逐根DFS最坏路径。
    }
    let mut build = BrowserSceneBuild::new(
        "gltf/worst.glb".to_owned(),
        Handle::<WorldAsset>::default().id(),
        1,
        &world,
    );

    assert!(!advance_scene_build(&mut build, &world, |_| None));
    assert_eq!(
        build.entity_slot_cursor as usize,
        MAX_GLTF_SCENE_ROOT_DISCOVERY_PER_FRAME,
    ); // 首帧绝不扫描全部8192实体槽位。

    let mut updates = 1usize;
    while !advance_scene_build(&mut build, &world, |_| None) {
        updates += 1;
        assert!(updates < 128, "固定预算状态机应在有界帧数内完成");
    }
    updates += 1;

    assert!(updates > 32); // 根发现32帧后，逐根DFS仍继续按操作预算推进。
    assert!(build.meshes.is_empty());
}

#[test]
fn wide_root_does_not_copy_all_children_in_one_update() {
    let mut world = World::new();
    let root = world.spawn_empty().id();
    let children = (0..1_024)
        .map(|index| world.spawn(Name::new(format!("node-{index}"))).id())
        .collect::<Vec<_>>();
    world.entity_mut(root).add_children(&children);
    let mut build = BrowserSceneBuild::new(
        "gltf/wide.glb".to_owned(),
        Handle::<WorldAsset>::default().id(),
        1,
        &world,
    );

    while !build.roots_discovered {
        advance_scene_build(&mut build, &world, |_| None);
    }
    let visited_before = build.visited_entities;
    assert!(!advance_scene_build(&mut build, &world, |_| None));
    assert!(
        build.visited_entities.saturating_sub(visited_before)
            <= MAX_GLTF_SCENE_TRAVERSAL_OPERATIONS_PER_FRAME,
    ); // 单个Children列表也只能逐项推进，不能整批clone到栈。
}

#[test]
fn duplicate_node_path_and_mesh_name_is_visible_but_invalid() {
    let mut world = World::new();
    let root = world.spawn_empty().id();
    let first_node = world.spawn(Name::new("河面对象")).id();
    let second_node = world.spawn(Name::new("河面对象")).id(); // 故意复现GLB中同一父级的重名对象。
    let first_mesh = world
        .spawn((
            Mesh3d(Handle::<Mesh>::default()),
            GltfMeshName("河面网格".to_owned()),
        ))
        .id();
    let second_mesh = world
        .spawn((
            Mesh3d(Handle::<Mesh>::default()),
            GltfMeshName("河面网格".to_owned()),
        ))
        .id();
    world
        .entity_mut(root)
        .add_children(&[first_node, second_node]);
    world.entity_mut(first_node).add_child(first_mesh);
    world.entity_mut(second_node).add_child(second_mesh);
    let mut build = BrowserSceneBuild::new(
        "gltf/duplicate.glb".to_owned(),
        Handle::<WorldAsset>::default().id(),
        1,
        &world,
    );

    while !advance_scene_build(&mut build, &world, |_| Some(0)) {}

    assert_eq!(build.meshes.len(), 1); // 浏览器只需保留一个诊断行，不能提供两个无法区分的按钮目标。
    assert_eq!(build.meshes[0].node_path, ["河面对象"]);
    assert_eq!(build.meshes[0].mesh_name, "河面网格");
    assert!(!build.meshes[0].valid); // 运行时会命中两个实体，因此作者赋水必须在UI阶段失败关闭。
    assert!(build.meshes[0].ambiguous); // 可见树必须显示具体歧义原因，引导回Blender重命名。
}
