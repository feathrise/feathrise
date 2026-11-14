//! 独立小来源的真实三角面必须接住窄屏、宽屏和上下两面的原相机射线，且不与示例地面串用。
use super::*;
use feathrise_scene::TerrainObjectRaySnapshot;
use feathrise_scene_core::residency::SceneSourceFrame;

fn plane(size: f32) -> avian3d::prelude::Collider {
    let half = size * 0.5;
    avian3d::prelude::Collider::trimesh(vec![Vec3::new(-half,0.,-half),Vec3::new(half,0.,-half),
        Vec3::new(half,0.,half),Vec3::new(-half,0.,half)],vec![[0,2,1],[0,3,2]])
}

#[test]
fn geometry_fixture_strokes_hit_isolated_small_surfaces_at_every_viewport_aspect() {
    // 与正式M1-SAMPLE种子一致的64m零高度真实三角面；旧±24m布置会错误命中它。
    let starter = TerrainObjectRaySnapshot::Mesh(plane(64.),SceneSourceFrame::from_transform(Transform::IDENTITY));
    assert!(starter.ray(Vec3::new(-24.5,200.,24.),Vec3::NEG_Y).is_some());
    for domain in [Domain::Height, Domain::HeightVegetation, Domain::Voxel, Domain::VoxelUnderside] {
        let fixture = GeometryFixture {domain,..Default::default()};
        let target = if matches!(domain, Domain::Height | Domain::HeightVegetation) {
            TerrainObjectRaySnapshot::Mesh(plane(4.),SceneSourceFrame::from_transform(Transform::from_translation(Creation::Small.origin())))
        } else {
            // 真实0.05m薄盒的顶面为Y=0、底面为Y=-0.05；上下射线不能借另一面通过。
            TerrainObjectRaySnapshot::Mesh(avian3d::prelude::Collider::cuboid(4.,0.05,4.),
                SceneSourceFrame::from_transform(Transform::from_translation(Creation::Voxel.origin()-Vec3::Y*0.025)))
        };
        for render_size in [UVec2::new(800,600),UVec2::new(1600,450),UVec2::new(600,1200)] {
            let viewport = SceneEditorViewportState {_image:Handle::default(),camera:Entity::PLACEHOLDER,
                center:fixture.center(),scene_center:Vec3::ZERO,scene_half_extents:Vec3::new(32.,80.,32.),
                extent:8.,yaw:0.,pitch:if domain==Domain::VoxelUnderside {-std::f32::consts::FRAC_PI_2}
                    else {std::f32::consts::FRAC_PI_2},framed_scene_id:None,render_size};
            let (mut projection, transform) = crate::viewport::editor_camera_pose(true,&viewport);
            projection.update(render_size.x as f32,render_size.y as f32);
            let clip_to_world = transform.to_matrix()*projection.get_clip_from_view().inverse();
            for tail in [false,true] {
                let pointer = stroke_pointer(&viewport,tail);
                let ndc = Vec2::new(pointer.x*2.,-pointer.y*2.);
                let near = clip_to_world.project_point3(ndc.extend(1.));
                let far = clip_to_world.project_point3(ndc.extend(0.));
                let direction = (far-near).normalize();
                let hit = target.ray(near,direction).expect("真实小来源必须接住原相机射线");
                assert!(hit.distance(viewport.center+Vec3::X*if tail {0.5}else{-0.5})<0.001,
                    "{domain:?} {render_size:?} {tail}: {hit:?}");
                assert!(starter.ray(near,direction).is_none(),"独立夹具的射线不能再与示例地面重叠");
            }
        }
    }
}
