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
    let node = seed.children(GlbParent::Scene(0))?.into_iter().find(|node| node.mesh.is_some())
        .ok_or("新网格未编码到 GLB")?;
    // 保留标准几何、材质与节点变换；临时场景的摆放身份不进入外部 GLB。
    seed.clear_node_extras(node.index)?;
    let index = seed.transfer_node_to(node.index, document, parent)?;
    SourceMeshProperties { collision: Some(collision), ..Default::default() }.write(document, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_glb_primitives_keep_existing_nodes_and_store_standard_geometry() {
        let bytes = write_scene_placements_glb(&ScenePlacementsAsset::default(), "", None).unwrap();
        let mut document = GlbStructure::from_bytes(&bytes).unwrap();
        let scene = GlbParent::Scene(0);
        let folder = document.create_folder(scene, "既有文件夹").unwrap();
        let local = Vec3::new(-4.0, 3.5, 2.0);
        for plane in [false, true] {
            let shape = crate::ui::terrain_objects::tree_shape_target(plane).unwrap();
            add(&mut document, scene, shape, local).unwrap();
        }

        let bytes = document.encode().unwrap();
        let standard = bevy::gltf::gltf::Gltf::from_slice(&bytes).unwrap();
        assert_eq!(standard.meshes().count(), 2, "两种形状都是标准 GLB 可读取的网格");
        let saved = GlbStructure::from_bytes(&bytes).unwrap();
        assert!(saved.node(folder).unwrap().folder, "原有节点不参与重新编号");
        let shapes = saved.children(scene).unwrap().into_iter().filter(|node| node.mesh.is_some())
            .collect::<Vec<_>>();
        assert_eq!(shapes.iter().map(|node| node.name.as_str()).collect::<Vec<_>>(), ["立方体", "平面"]);
        for node in shapes {
            assert_eq!(saved.node_transform(node.index).unwrap().translation, local);
            assert_eq!(SourceMeshProperties::read(&saved, node.index).unwrap().collision, Some(true));
        }
    }
}
