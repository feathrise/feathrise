//! Mesh预览冻结当前作者画布的完整采样合同，避免借用场景容器的空地形或兄弟网格。

use bevy::prelude::Transform;
use feathrise_scene_content::SceneTerrainDescriptor;
use std::sync::Arc;

use crate::authoring::EditorSceneDraft;

pub(super) struct MeshPreviewTerrain {
    pub heights: Arc<Vec<u16>>,
    pub terrain: SceneTerrainDescriptor,
    pub world_min: [f32; 2],
    pub world_max: [f32; 2],
    pub transform: Transform,
}

pub(super) fn capture(draft: &EditorSceneDraft) -> Option<MeshPreviewTerrain> {
    let source = draft.terrain_manifest();
    Some(MeshPreviewTerrain {
        heights: draft.heights.clone()?,
        terrain: source.terrain.clone(),
        world_min: source.world_min,
        world_max: source.world_max,
        // 原高度是对象源坐标；实例姿态与建筑世界净空都在散布完成后应用同一来源框架。
        transform: draft
            .terrain_documents
            .active
            .as_ref()
            .map_or(Transform::IDENTITY, |active| {
                active.definition.transform.as_transform()
            }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    use bevy::prelude::*;
    use feathrise_scene_content::{BuildingWorldQuad, TerrainId, TerrainObjectTransform};

    #[test]
    fn private_mesh_preview_uses_object_sampling_and_world_pose_instead_of_empty_container() {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("mesh-preview-ground").unwrap();
        object.transform = TerrainObjectTransform {
            translation: [40., 7., -30.],
            yaw_degrees: 90.,
            scale: [2.; 3],
            ..Default::default()
        };
        let manifest = object.source_manifest(&draft.manifest);
        draft.manifest.terrain = SceneTerrainDescriptor::empty();
        draft.manifest.world_min = [-800.; 2];
        draft.manifest.world_max = [900.; 2];
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_documents.active = Some(TerrainDocumentIdentity {
            definition: Arc::new(object),
            manifest,
            folder: "private".into(),
        });
        let source = capture(&draft).unwrap();
        assert_eq!(draft.manifest.terrain.resolution, [0, 0]);
        assert_eq!(source.terrain.resolution, [11, 11]);
        let layer = feathrise_scene_content::SceneVegetationLayer {
            id: "source-test".into(),
            label: "source-test".into(),
            kind: feathrise_scene_content::SceneVegetationKind::Mesh,
            runtime_profile: Some("source-test".into()),
            runtime_layer_id: Some(7),
            prototype_id: Some(107),
            asset_path: "source.glb".into(),
            density_source_path: "source.r8".into(),
            density_texture_path: "source.ktx2".into(),
            resolution: [2, 2],
            spacing_meters: 1.,
            scale_range: [1., 1.],
            max_instances: 1,
            seed: 1,
        };
        let build = super::super::build_vegetation_preview(
            &layer,
            &[255; 4],
            &source.heights,
            source.terrain.resolution,
            source.world_min,
            source.world_max,
            source.terrain.base_height,
            source.terrain.height_scale,
            source.transform,
            &[] as &[BuildingWorldQuad],
            0.0,
        )
        .unwrap();
        assert_eq!(
            build.transforms.len(),
            100,
            "原十米源范围仍是完整100个候选，容器范围不得参与散布"
        );
        for value in &build.transforms {
            let local = source
                .transform
                .to_matrix()
                .inverse()
                .transform_point3(value.translation);
            assert!((source.world_min[0]..=source.world_max[0]).contains(&local.x));
            assert!((source.world_min[1]..=source.world_max[1]).contains(&local.z));
            assert!((local.y - source.terrain.base_height).abs() < 0.001);
            assert_eq!(value.scale, Vec3::splat(2.));
        }
    }
}
