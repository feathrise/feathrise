//! 复用内置网格编码器构造标准几何，再移植到引用 GLB 的现有场景。

use super::{GlbParent, GlbStructure, SourceMeshProperties, Vec3};
use feathrise_scene_content::{
    ScenePlacementAssetTarget, ScenePlacementRecord, ScenePlacementsAsset, write_scene_placements_glb,
};
use std::collections::HashSet;

pub(super) fn add(
    document: &mut GlbStructure,
    parent: GlbParent,
    shape: ScenePlacementAssetTarget,
    local: Vec3,
) -> Result<(), String> {
    let (ScenePlacementAssetTarget::Cuboid { collision, .. }
        | ScenePlacementAssetTarget::EditableMesh { collision, .. }) = &shape
    else {
        return Err("源 GLB 只能新建立方体或平面".into());
    };
    let collision = *collision;
    let label = crate::authoring::basic_shapes::creation_label(&shape);
    let children = document.children(parent)?;
    let names: HashSet<_> = children.iter().map(|node| node.name.as_str()).collect();
    let name = (0..=children.len()).map(|ordinal| {
        if ordinal == 0 { label.to_owned() } else { format!("{label}.{ordinal:03}") }
    }).find(|name| !names.contains(name.as_str())).ok_or("无法为新网格分配名称")?;
    let seed = ScenePlacementsAsset {
        placements: vec![ScenePlacementRecord {
            id: name, asset_path: String::new(), asset_target: shape, editor: Default::default(),
            mesh_lod: None, terrain_binding: None, terrain_patch: None, building_preset_id: None, courtyard: None,
            translation: local.to_array(), height_mode: Default::default(), terrain_offset: 0.0,
            yaw_degrees: 0.0, scale: [1.0; 3], water_surfaces: Vec::new(),
        }],
        ..Default::default()
    };
    let bytes = write_scene_placements_glb(&seed, "", None)?;
    let mut seed = GlbStructure::from_bytes(&bytes)?;
    let (node, mesh) = seed
        .children(GlbParent::Scene(0))?
        .into_iter()
        .find_map(|node| node.mesh.map(|mesh| (node, mesh)))
        .ok_or("新网格未编码到 GLB")?;
    // 主场景编码器只命名摆放节点；引用 GLB 还需要把网格名写进实际网格数据。
    seed.rename_mesh(mesh, &node.name)?;
    // 保留标准几何、材质与节点变换；临时场景的摆放身份不进入外部 GLB。
    seed.clear_node_extras(node.index)?;
    let index = seed.transfer_node_to(node.index, document, parent)?;
    SourceMeshProperties { collision: Some(collision), ..Default::default() }.write(document, index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Mat4, SourceGlbEdits, SourceGlbOperation, SourceGlbTarget};

    struct SourceFixture(std::path::PathBuf);

    impl Drop for SourceFixture {
        fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
    }

    #[test]
    fn source_glb_primitives_keep_existing_nodes_and_store_standard_geometry() {
        let bytes = write_scene_placements_glb(&ScenePlacementsAsset::default(), "", None).unwrap();
        let mut document = GlbStructure::from_bytes(&bytes).unwrap();
        let scene = GlbParent::Scene(0);
        let folder = document.create_folder(scene, "既有文件夹").unwrap();
        let root = std::env::temp_dir().canonicalize().unwrap();
        let serial = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let relative = format!("feathrise-source-primitives-{}-{serial}.glb", std::process::id());
        let source = SourceFixture(root.join(&relative));
        std::fs::write(&source.0, document.encode().unwrap()).unwrap();
        let plane_local = Vec3::new(-4.0, 3.5, 2.0);
        let reference = Mat4::from_translation(Vec3::new(13.0, -5.0, 24.0));
        let mut edits = SourceGlbEdits::default();
        for (plane, world_center) in [(false, None), (true, Some(reference.transform_point3(plane_local)))] {
            let target = SourceGlbTarget { path: relative.clone(), node: None, world: reference };
            let shape = crate::ui::terrain_objects::tree_shape_target(plane).unwrap();
            edits = super::super::edit(&root, &edits, target, SourceGlbOperation::AddPrimitive { shape, world_center })
                .unwrap().unwrap().0;
        }

        let bytes = &edits.version(&relative).unwrap().bytes;
        let standard = bevy::gltf::gltf::Gltf::from_slice(bytes).unwrap();
        let names = ["立方体", "平面"];
        assert_eq!(standard.meshes().count(), names.len(), "两种形状都是标准 GLB 可读取的网格");
        for (mesh, name) in standard.meshes().zip(names) {
            assert_eq!(mesh.name(), Some(name), "外部 GLB 稳定引用需要网格数据拥有自己的显式名称");
        }
        let saved = GlbStructure::from_bytes(bytes).unwrap();
        assert!(saved.node(folder).unwrap().folder, "原有节点不参与重新编号");
        let shapes = saved.children(scene).unwrap().into_iter().filter(|node| node.mesh.is_some())
            .collect::<Vec<_>>();
        assert_eq!(shapes.iter().map(|node| node.name.as_str()).collect::<Vec<_>>(), names);
        // 偏移引用中的立方体使用文件原点，平面仍执行世界中心到本地的换算。
        for (node, local) in shapes.into_iter().zip([Vec3::ZERO, plane_local]) {
            assert_eq!(saved.node_transform(node.index).unwrap().translation, local);
            assert_eq!(SourceMeshProperties::read(&saved, node.index).unwrap().collision, Some(true));
        }
    }
}
