//! 建筑整平只写实际命中的作者对象；世界占地、源格点和最终世界平台Y使用同一TRS桥。

use crate::authoring::{
    EditorAuthoringSession, EditorMapDraft, placement_grounding::PlacementTerrainSampler,
};
use crate::resources::MapEditorState;
use bevy::prelude::*;
use feathrise_map_content::{BuildingWorldQuad, MapTerrainObject, TerrainObjectTransform};
use std::sync::Arc;

pub(super) fn freeze(
    session: &EditorAuthoringSession,
    editor: &MapEditorState,
) -> Result<Option<Arc<MapTerrainObject>>, String> {
    let draft = session.draft.as_ref().ok_or("建筑整平地图草稿未就绪")?;
    if draft.manifest.terrains.is_none() {
        return Ok(None);
    }
    if !crate::authoring::terrain_documents::canvas_writable(session, editor) {
        return Err("建筑整平需要先选择并载入可写承托地形画布；本次建筑落点尚未应用".into());
    }
    Ok(draft
        .terrain_documents
        .active
        .as_ref()
        .map(|active| active.definition.clone()))
}

pub(super) fn matches(draft: &EditorMapDraft, frame: Option<&Arc<MapTerrainObject>>) -> bool {
    match frame {
        Some(frame) => draft
            .terrain_documents
            .active
            .as_ref()
            .is_some_and(|active| {
                active.definition.id == frame.id && active.definition.transform == frame.transform
            }),
        None => draft.manifest.terrains.is_none(),
    }
}

/// 必须在原校验worker执行；跨岛地基不能把B上的点击静默写进当前A作者源。
pub(super) fn verify_support(
    frame: Option<&Arc<MapTerrainObject>>,
    sampler: &PlacementTerrainSampler,
    footprint: BuildingWorldQuad,
) -> Result<(), String> {
    let Some(frame) = frame else {
        return if sampler.is_object_surface() {
            Err("关联地形作者画布尚未完整载入；本次落点已保留".into())
        } else {
            Ok(())
        };
    };
    for point in footprint.foundation_samples() {
        let actual = sampler
            .bind_pose(
                TerrainObjectTransform {
                    translation: [point.x, 0.0, point.y],
                    ..default()
                },
                None,
            )?
            .ok_or("建筑整平缺少真实对象承托身份")?;
        if actual.terrain_id != frame.id {
            return Err(format!(
                "建筑整平命中地形{}，当前作者画布为{}；本次落点已保留，请选择承托对象后重新确认",
                actual.terrain_id.as_str(),
                frame.id.as_str()
            ));
        }
    }
    if sampler.object_pose(&frame.id) != Some(frame.transform) {
        return Err(
            "建筑整平的作者姿态与实际承托姿态尚未同代；已保留本次落点，请就绪后重新确认".into(),
        );
    }
    Ok(())
}

pub(super) fn source_quad(
    frame: Option<&Arc<MapTerrainObject>>,
    quad: BuildingWorldQuad,
) -> BuildingWorldQuad {
    let Some(frame) = frame else {
        return quad;
    };
    let inverse = frame.transform.as_transform().to_matrix().inverse();
    BuildingWorldQuad {
        corners: quad.corners.map(|point| {
            inverse
                .transform_point3(Vec3::new(point.x, 0.0, point.y))
                .xz()
        }),
    }
}

pub(super) fn world_height(frame: Option<&Arc<MapTerrainObject>>, source: f32) -> f32 {
    frame.map_or(source, |frame| {
        frame.transform.translation[1] + source * frame.transform.scale[1]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_map_content::{BuildingFootprintRect, TerrainId};

    #[test]
    fn moved_rotated_and_nonuniformly_scaled_terrain_preserves_source_footprint_and_platform_height()
     {
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.transform = TerrainObjectTransform {
            translation: [30.0, 7.0, -20.0],
            yaw_degrees: 37.0,
            scale: [2.0, 3.0, 0.5],
            ..Default::default()
        };
        let object = Arc::new(object);
        let local = BuildingWorldQuad::from_local_rect(
            BuildingFootprintRect {
                center: [0.0; 2],
                half_extents: [2.0, 1.0],
            },
            [4.0, 0.0, -3.0],
            25.0,
            [1.2, 1.0, 0.8],
        );
        let matrix = object.transform.as_transform().to_matrix();
        let world = BuildingWorldQuad {
            corners: local.corners.map(|point| {
                matrix
                    .transform_point3(Vec3::new(point.x, 0.0, point.y))
                    .xz()
            }),
        };
        let restored = source_quad(Some(&object), world);
        for (actual, expected) in restored.corners.into_iter().zip(local.corners) {
            assert!(actual.abs_diff_eq(expected, 0.00001));
        }
        assert_eq!(world_height(Some(&object), 3.5), 17.5);
    }

    #[test]
    fn real_support_on_another_island_never_authorizes_writing_the_selected_canvas() {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut selected = draft.manifest.terrain_objects_for_edit()[0].clone();
        selected.id = TerrainId::parse("canvas-a").unwrap();
        let selected = Arc::new(selected);
        let shape = avian3d::prelude::Collider::trimesh(
            vec![
                Vec3::new(-2.0, 0.0, -2.0),
                Vec3::new(2.0, 0.0, -2.0),
                Vec3::new(2.0, 0.0, 2.0),
                Vec3::new(-2.0, 0.0, 2.0),
            ],
            vec![[0, 2, 1], [0, 3, 2]],
        );
        draft.manifest.terrains = Some(Arc::from([(*selected).clone()]));
        draft.terrain_ground = Some(
            crate::authoring::terrain_ground::TerrainGroundSnapshot::test_mesh(
                draft.map_id.clone(),
                TerrainId::parse("island-b").unwrap(),
                default(),
                shape,
            ),
        );
        let sampler = PlacementTerrainSampler::from_draft(&draft)
            .unwrap()
            .for_selected_surface(Vec3::ZERO);
        let footprint = BuildingWorldQuad::from_local_rect(
            BuildingFootprintRect {
                center: [0.0; 2],
                half_extents: [0.5; 2],
            },
            [0.0; 3],
            0.0,
            [1.0; 3],
        );
        let error = verify_support(Some(&selected), &sampler, footprint).unwrap_err();
        assert!(error.contains("canvas-a") && error.contains("island-b"));
        assert!(error.contains("已保留"));
    }
}
