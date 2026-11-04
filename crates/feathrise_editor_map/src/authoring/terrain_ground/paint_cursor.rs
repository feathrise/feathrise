//! 连续画笔与严格悬停分离：后台完成点保持原射线，只有已证实的同表面路径可插值。

use super::{
    TerrainGroundSnapshot,
    resources::{CursorIdentity, CursorPaintAnchor},
};
use bevy::prelude::*;
use feathrise_map_content::TerrainId;
use feathrise_scene::{ClipmapGroundHit, TerrainObjectRaySnapshot};
use std::collections::BTreeSet;

/// 本笔自己会推进作者修订；消费方仍负责校验预期修订，光标连续性只放行这一字段。
pub(super) fn same_context(previous: &CursorIdentity, current: &CursorIdentity) -> bool {
    let mut previous = previous.clone();
    previous.revision = current.revision;
    previous == *current
}

/// 本网格雕刻换代时保留最后真点的显示；写入仍用严格新快照，移动/旋转或换源绝不沿用旧圈。
pub(super) fn same_display_context(sample: &super::resources::CursorDisplaySample,
    current: &CursorIdentity, draft: Option<&crate::authoring::EditorMapDraft>) -> bool {
    if same_context(&sample.identity, current) { return true; }
    let definition = draft.and_then(|draft| draft.terrain_documents.active.as_ref())
        .filter(|canvas| canvas.definition.id == sample.hit.terrain).map(|canvas| &canvas.definition);
    if definition.zip(sample.definition.as_ref()).is_none_or(|(current, previous)| !same_display_source(current, previous)) {
        return false;
    }
    let mut previous = sample.identity.clone();
    previous.revision = current.revision;
    previous.terrain_revision = current.terrain_revision;
    previous.ground_snapshot = current.ground_snapshot.clone();
    previous == *current // 相机、地图、工具、可见性等门保持原样；只忽略同来源高度的短暂在途状态。
}

fn same_display_source(a: &feathrise_map_content::MapTerrainObject, b: &feathrise_map_content::MapTerrainObject) -> bool {
    // 只核对决定落点位置的小型来源字段，不能在每帧显示门遍历任意数量的LOD/拼接/画布列表。
    a.id == b.id && a.enabled == b.enabled && a.transform == b.transform
        && a.world_min == b.world_min && a.world_max == b.world_max
        && a.terrain.kind == b.terrain.kind && a.terrain.glb_path == b.terrain.glb_path
        && a.terrain.height_path == b.terrain.height_path && a.terrain.resolution == b.terrain.resolution
        && a.terrain.base_height == b.terrain.base_height && a.terrain.height_scale == b.terrain.height_scale
        && a.voxel.as_ref().map(|v| (v.cell_size_meters, v.min, v.max, &v.source_path))
            == b.voxel.as_ref().map(|v| (v.cell_size_meters, v.min, v.max, &v.source_path))
}

pub(super) fn author_point(
    ground: &TerrainGroundSnapshot,
    terrain: &TerrainId,
    point: Vec3,
) -> Option<Vec3> {
    match ground.object_query(terrain)? {
        TerrainObjectRaySnapshot::Clipmap(snapshot, frame) => {
            let local = frame.to_local(point);
            if snapshot.is_three_dimensional_surface(local) {
                return None;
            }
            match snapshot.sample_authoritative_surface(local.xz()) {
                ClipmapGroundHit::Hit(point) => Some(frame.to_world(point)),
                _ => None,
            }
        }
        TerrainObjectRaySnapshot::Mesh(..) => Some(point),
    }
}

pub(super) fn connected_from(
    ground: &TerrainGroundSnapshot,
    hidden: &BTreeSet<TerrainId>,
    anchor: Option<&CursorPaintAnchor>,
    terrain: &TerrainId,
    point: Vec3,
    identity: &CursorIdentity,
) -> Option<Vec3> {
    let previous = anchor
        .filter(|anchor| anchor.hit.terrain == *terrain)?
        .hit
        .point;
    let query = ground.object_query(terrain)?;
    // 沿原真实承托层追踪完整线段；缺页、洞口、网格空隙和另一层都不能借两个端点补线。
    let ClipmapGroundHit::Hit(target) = query.sample_support_along_surface(previous, point.xz())
    else {
        return None;
    };
    if target.distance(point) > 0.01 {
        return None;
    }
    ground
        .paint_segment_unobstructed(
            terrain,
            previous,
            point,
            identity.camera,
            identity.orthographic,
            hidden,
        )
        .then_some(previous)
}

/// 相机到整段笔迹形成的射线扇与其他对象包围盒相交时保守断线；不只检查两端可见性。
pub(super) fn triangle_intersects_bounds(
    triangle: [Vec3; 3],
    minimum: Vec3,
    maximum: Vec3,
) -> bool {
    let center = (minimum + maximum) * 0.5;
    let half = (maximum - minimum) * 0.5;
    let points = triangle.map(|point| point - center);
    let edges = [
        points[1] - points[0],
        points[2] - points[1],
        points[0] - points[2],
    ];
    let separated = |axis: Vec3| {
        let projections = points.map(|point| axis.dot(point));
        let radius = half.dot(axis.abs());
        projections.into_iter().fold(f32::INFINITY, f32::min) > radius + 1.0e-5
            || projections.into_iter().fold(f32::NEG_INFINITY, f32::max) < -radius - 1.0e-5
    };
    if [Vec3::X, Vec3::Y, Vec3::Z, edges[0].cross(edges[1])]
        .into_iter()
        .any(separated)
    {
        return false;
    }
    !edges.into_iter().any(|edge| {
        [Vec3::X, Vec3::Y, Vec3::Z]
            .into_iter()
            .any(|axis| separated(edge.cross(axis)))
    })
}

#[cfg(test)]
mod tests {
    use super::super::resources::{CursorGroundSnapshot, CursorPaintHit, CursorRay};
    use super::*;
    use feathrise_map_content::{MapId, TerrainObjectTransform};
    use std::sync::Arc;

    fn plane(min: f32, max: f32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        (
            vec![
                Vec3::new(min, 0.0, -1.0),
                Vec3::new(max, 0.0, -1.0),
                Vec3::new(max, 0.0, 1.0),
                Vec3::new(min, 0.0, 1.0),
            ],
            vec![[0, 2, 1], [0, 3, 2]],
        )
    }

    fn identity(ground: &Arc<TerrainGroundSnapshot>) -> CursorIdentity {
        CursorIdentity {
            map: ground.map.clone(),
            revision: 0,
            terrain_revision: ground.revision,
            tool: crate::types::EditorTool::Surface,
            ground_snapshot: Some(CursorGroundSnapshot::capture(ground)),
            asset: None,
            asset_target: None,
            portal: None,
            camera: Transform::from_xyz(0.0, 10.0, 4.0)
                .looking_at(Vec3::ZERO, Vec3::Y)
                .to_matrix(),
            size: UVec2::splat(128),
            camera_entity: Entity::PLACEHOLDER,
            projection: Mat4::IDENTITY,
            view: crate::types::EditorView::Perspective,
            orthographic: false,
            extent: 16.0_f32.to_bits(),
            visibility: 0,
            relocation: None,
            translation_snap: None,
        }
    }

    fn anchor(identity: &CursorIdentity, owner: &TerrainId, point: Vec3) -> CursorPaintAnchor {
        CursorPaintAnchor {
            identity: identity.clone(),
            authority: 1,
            ray: CursorRay {
                origin: Vec3::Y * 10.0,
                direction: Vec3::NEG_Y,
                epoch: 1,
            },
            hit: CursorPaintHit {
                terrain: owner.clone(),
                point,
                from: None,
            },
        }
    }

    #[test]
    fn continuous_paint_path_uses_the_actual_connected_mesh_and_rejects_an_island_gap() {
        let map = MapId::parse("MAP-paint-path").unwrap();
        let owner = TerrainId::parse("paint-path").unwrap();
        for gap in [false, true] {
            let (mut vertices, mut faces) = plane(-4.0, if gap { -1.0 } else { 4.0 });
            if gap {
                let (other, indices) = plane(1.0, 4.0);
                let base = vertices.len() as u32;
                vertices.extend(other);
                faces.extend(
                    indices
                        .into_iter()
                        .map(|face| face.map(|index| index + base)),
                );
            }
            let ground = TerrainGroundSnapshot::test_mesh(
                map.clone(),
                owner.clone(),
                TerrainObjectTransform::default(),
                avian3d::prelude::Collider::trimesh(vertices, faces),
            );
            let identity = identity(&ground);
            let from = Vec3::new(-3.0, 0.0, 0.0);
            let anchor = anchor(&identity, &owner, from);
            assert_eq!(
                connected_from(
                    &ground,
                    &BTreeSet::new(),
                    Some(&anchor),
                    &owner,
                    Vec3::new(3.0, 0.0, 0.0),
                    &identity
                ),
                (!gap).then_some(from)
            );
            assert!(
                connected_from(
                    &ground,
                    &BTreeSet::new(),
                    Some(&anchor),
                    &TerrainId::parse("other-grid").unwrap(),
                    Vec3::new(3.0, 0.0, 0.0),
                    &identity
                )
                .is_none()
            );
        }
    }

    #[test]
    fn continuous_paint_path_cannot_connect_visible_endpoints_through_another_grid_occluder() {
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut target = draft.manifest.terrain_objects_for_edit()[0].clone();
        target.id = TerrainId::parse("paint-target").unwrap();
        target.transform = TerrainObjectTransform {
            scale: [4.0, 1.0, 4.0],
            ..Default::default()
        };
        let mut blocker = target.clone();
        blocker.id = TerrainId::parse("paint-blocker").unwrap();
        blocker.transform = TerrainObjectTransform {
            translation: [0.0, 5.0, 2.0],
            scale: [0.2, 1.0, 0.2],
            ..Default::default()
        };
        let (vertices, faces) = plane(-1.0, 1.0);
        let ground = TerrainGroundSnapshot::test_meshes(
            draft.map_id,
            &[target.clone(), blocker.clone()],
            avian3d::prelude::Collider::trimesh(vertices, faces),
        );
        let identity = identity(&ground);
        let from = Vec3::new(-2.0, 0.0, 0.0);
        let to = Vec3::new(2.0, 0.0, 0.0);
        let anchor = anchor(&identity, &target.id, from);
        for point in [from, to] {
            let eye = identity.camera.w_axis.truncate();
            assert_eq!(
                ground.ray(eye, (point - eye).normalize()).unwrap().0,
                target.id
            );
        }
        assert!(
            connected_from(
                &ground,
                &BTreeSet::new(),
                Some(&anchor),
                &target.id,
                to,
                &identity
            )
            .is_none()
        );
        assert_eq!(
            connected_from(
                &ground,
                &BTreeSet::from([blocker.id]),
                Some(&anchor),
                &target.id,
                to,
                &identity
            ),
            Some(from)
        );
    }
}
