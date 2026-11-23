//! 使用已选引用与 GLB 作者父链换算节点坐标，提交仍以源节点的直属父级为基准。

use super::pose::Control;
use crate::{authoring::EditorAuthoringSession, ui::{
    placement_water::PlacementInstanceIndex, scene_context_menu::{SceneObjectKey, SceneObjectTarget},
    subscenes::SceneReferenceCoordinates,
}, viewport::resolve_active_root};
use bevy::{ecs::system::SystemParam, prelude::*, world_serialization::{InstanceId, WorldAssetRoot, WorldInstance}};
use feathrise_editor_animation_model::data::GlbStructure;
use feathrise_scene::{ScenePlacementInstance, SubsceneActive, SubsceneInstance};
use feathrise_scene_core::residency::{SceneInstanceOwner, SceneTerrainOwner};

#[derive(Component, Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum Space {
    #[default]
    Glb,
    Scene,
    World,
}

impl Space {
    pub(super) const ALL: [Self; 3] = [Self::Glb, Self::Scene, Self::World];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Glb => "源 GLB",
            Self::Scene => "当前场景",
            Self::World => "世界",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EditFrame {
    pub(super) space: Space,
    from_parent: Mat4,
    to_parent: Mat4,
    instance: Option<(Entity, Option<InstanceId>)>,
}

impl EditFrame {
    #[cfg(test)]
    pub(super) fn glb(parent: Mat4) -> Self { Self::new(Space::Glb, parent, None).unwrap() }

    fn new(space: Space, from_parent: Mat4, instance: Option<(Entity, Option<InstanceId>)>) -> Option<Self> {
        let determinant = from_parent.determinant();
        if !from_parent.is_finite() || !determinant.is_finite() || determinant == 0.0 { return None; }
        let to_parent = from_parent.inverse();
        to_parent.is_finite().then_some(Self { space, from_parent, to_parent, instance })
    }

    pub(super) fn position(self, pose: Transform) -> Vec3 {
        self.from_parent.transform_point3(pose.translation)
    }

    pub(super) fn pose(self, pose: Transform) -> Option<Transform> {
        map_pose(self.from_parent, pose).ok()
    }

    pub(super) fn apply(self, control: Control, pose: &mut Transform, value: f32) -> Result<(), String> {
        if self.from_parent == Mat4::IDENTITY {
            control.apply(pose, value);
        } else if let Some(axis) = control.0.translation_axis() {
            // 位置编辑仅求点的逆变换。已存在的非等比缩放组合不会影响旋转和缩放原值。
            let mut position = self.position(*pose);
            if position[axis] == value { return Ok(()); }
            position[axis] = value;
            let translation = self.to_parent.transform_point3(position);
            if !translation.is_finite() { return Err("换算后的源 GLB 节点位置无效".into()); }
            pose.translation = translation;
        } else {
            let mut displayed = map_pose(self.from_parent, *pose)?;
            control.apply(&mut displayed, value);
            let next = map_pose(self.to_parent, displayed)?;
            pose.rotation = next.rotation;
            pose.scale = next.scale;
        }
        Ok(())
    }
}

fn map_pose(basis: Mat4, pose: Transform) -> Result<Transform, String> {
    if basis == Mat4::IDENTITY { return Ok(pose); }
    // 先换算朝向的三根单位轴，再乘原节点的缩放。零缩放仍可显示并重新输入。
    let linear = Mat4 { w_axis: Vec4::W, ..basis } * Mat4::from_quat(pose.rotation);
    let mut mapped = GlbStructure::decompose_node_matrix(linear)?;
    mapped.translation = basis.transform_point3(pose.translation);
    mapped.scale *= pose.scale;
    if !mapped.translation.is_finite() || !mapped.scale.is_finite() {
        return Err("节点坐标换算超出有效范围".into());
    }
    Ok(mapped)
}

#[derive(SystemParam)]
pub(super) struct Frames<'w, 's> {
    session: Res<'w, EditorAuthoringSession>,
    index: Option<Res<'w, PlacementInstanceIndex>>,
    placements: Query<'w, 's, (Entity, &'static ScenePlacementInstance)>,
    reference: Option<Res<'w, SceneReferenceCoordinates>>,
    glbs: Query<'w, 's, (&'static GlobalTransform, Option<&'static WorldInstance>, Option<&'static SceneInstanceOwner>), With<WorldAssetRoot>>,
    scene_roots: Query<'w, 's, (&'static GlobalTransform, &'static SubsceneInstance, Option<&'static SceneTerrainOwner>)>,
    active: Query<'w, 's, (), With<SubsceneActive>>,
}

impl Frames<'_, '_> {
    pub(super) fn get(&self, space: Space, target: &SceneObjectTarget, parent_in_glb: Mat4) -> Option<EditFrame> {
        if space == Space::Glb { return EditFrame::new(space, parent_in_glb, None); }
        let SceneObjectKey::Placement { id, mesh: Some(_), .. } = &target.key else { return None; };
        // 与选择共用精确的活动根解析，不依赖临时悬停或网格高亮的图元数量。
        let (root, _) = resolve_active_root(&target.scene_id, id, &self.session, self.index.as_ref()?, &self.placements).ok()??;
        let (global, instance, owner) = self.glbs.get(root).ok()?;
        let root_in_view = global.to_matrix();
        let view_from_scene = if let Some(owner) = owner {
            let (pose, instance, terrain) = self.scene_roots.get(owner.0).ok()?;
            if !self.active.contains(instance.batch) { return None; }
            // 独立地形也有实例上下文，场景档的原点须继续追溯到承载它的场景。
            match terrain {
                None => pose.to_matrix(),
                Some(SceneTerrainOwner { scene_instance: None }) => Mat4::IDENTITY,
                Some(SceneTerrainOwner { scene_instance: Some(scene) }) => {
                    let (pose, instance, _) = self.scene_roots.get(*scene).ok()?;
                    if !self.active.contains(instance.batch) { return None; }
                    pose.to_matrix()
                }
            }
        } else {
            Mat4::IDENTITY
        };
        let from_parent = match space {
            Space::Scene => view_from_scene.inverse() * root_in_view * parent_in_glb,
            Space::World => {
                let world_from_view = self.reference.as_ref().map_or(Mat4::IDENTITY,
                    |reference| reference.world_from_scene(&target.scene_id));
                world_from_view * root_in_view * parent_in_glb
            }
            Space::Glb => unreachable!(),
        };
        EditFrame::new(space, from_parent, Some((root, instance.map(|id| **id))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{gltf_browser::EditorGltfMeshSelection, placement_transform::PlacementTransformField as Field};
    use feathrise_scene_content::{SceneInstanceId, SceneInstancePath};

    #[test]
    fn source_glb_spaces_use_model_scene_and_nested_world_coordinates() {
        let parent = Mat4::from_translation(Vec3::Y * 2.0);
        let glb = Mat4::from_translation(Vec3::Y * 5.0);
        let outer = Mat4::from_translation(Vec3::Y * 100.0);
        let mut local = Transform::from_xyz(0.0, 3.0, 0.0);
        let author_before = local;
        let source = EditFrame::glb(parent);
        let scene = EditFrame::new(Space::Scene, glb * parent, None).unwrap();
        let world = EditFrame::new(Space::World, outer * glb * parent, None).unwrap();
        assert_eq!([source, scene, world].map(|frame| frame.position(local).y), [5.0, 10.0, 110.0]);
        assert_eq!(local, author_before);
        world.apply(Control(Field::TranslationY), &mut local, 112.0).unwrap();
        assert_eq!(local, Transform::from_xyz(0.0, 5.0, 0.0));
        assert_eq!([source, scene, world].map(|frame| frame.position(local).y), [7.0, 12.0, 112.0]);
    }

    #[test]
    fn source_glb_spaces_read_the_adopted_reference_root_without_mesh_highlight() {
        let (mut app, root, _) = crate::viewport::placement_move::interaction_tests::app();
        let world = app.world_mut();
        let draft = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
        let scene = draft.scene_id.clone();
        let id = draft.placements.placements[0].id.clone();
        let target = SceneObjectTarget { scene_id: scene.clone(), revision: 7,
            key: SceneObjectKey::Placement { index: 0, id: id.clone(), mesh: Some(EditorGltfMeshSelection {
                placement_id: id, node_path: vec!["窗户".into()], mesh_name: "窗户".into(),
                mesh_index: Some(0), node_index: Some(1), valid: true, orphaned_water_binding: false, empty_source_folder: false,
            }) }, name: "窗户".into(), hidden: false, locked: false, copyable: false };
        let batch = world.spawn(SubsceneActive).id();
        let owner = world.spawn((
            SubsceneInstance { path: SceneInstancePath::root(scene.clone()).child(SceneInstanceId::parse("inner").unwrap()),
                source_scene_id: scene, generation: 2, batch },
            GlobalTransform::from(Transform::from_xyz(0.0, 100.0, 0.0)),
        )).id();
        world.entity_mut(root).insert((SceneInstanceOwner(owner), WorldAssetRoot(Handle::default()),
            GlobalTransform::from(Transform::from_xyz(0.0, 105.0, 0.0))));
        let probe = world.register_system(move |frames: Frames| {
            Space::ALL.map(|space| frames.get(space, &target, Mat4::from_translation(Vec3::Y * 2.0))
                .map(|frame| frame.position(Transform::IDENTITY).y))
        });
        assert_eq!(world.run_system(probe).unwrap(), [Some(2.0), Some(7.0), Some(107.0)]);
        // 同一摆放若由独立地形的上下文生成，地形原点仍不能冒充主场景原点。
        world.entity_mut(owner).insert(SceneTerrainOwner { scene_instance: None });
        assert_eq!(world.run_system(probe).unwrap(), [Some(2.0), Some(107.0), Some(107.0)]);
        world.entity_mut(batch).remove::<SubsceneActive>();
        assert_eq!(world.run_system(probe).unwrap(), [Some(2.0), None, None]);
    }

    #[test]
    fn source_glb_spaces_convert_rotations_and_scale_back_to_the_source_parent() {
        let basis = Transform::from_xyz(100.0, -25.0, 8.0).with_rotation(Quat::from_rotation_y(0.33))
            .with_scale(Vec3::splat(2.0)).to_matrix();
        let world = EditFrame::new(Space::World, basis, None).unwrap();
        let mut local = Transform::from_xyz(2.0, 3.0, 1.0).with_scale(Vec3::new(0.0, 1.0, 2.0));
        let mut expected = world.pose(local).unwrap();
        assert_eq!(expected.scale.x, 0.0);
        Control(Field::RotationZ).apply(&mut expected, 37.0);
        Control(Field::ScaleX).apply(&mut expected, 1.5);
        world.apply(Control(Field::RotationZ), &mut local, 37.0).unwrap();
        world.apply(Control(Field::ScaleX), &mut local, 1.5).unwrap();
        assert!((basis * local.to_matrix()).abs_diff_eq(expected.to_matrix(), 0.0001));
        assert_eq!(local.translation, Vec3::new(2.0, 3.0, 1.0));
    }

    #[test]
    fn source_glb_spaces_preserve_shear_when_moving_and_reject_lossy_rotation() {
        let basis = Mat4::from_scale(Vec3::new(2.0, 1.0, 3.0));
        let world = EditFrame::new(Space::World, basis, None).unwrap();
        let mut local = Transform::IDENTITY;
        assert!(world.apply(Control(Field::RotationY), &mut local, 25.0).is_err());
        assert_eq!(local, Transform::IDENTITY);
        local.rotation = Quat::from_rotation_y(0.4);
        let original = local.rotation;
        assert!(world.pose(local).is_none());
        world.apply(Control(Field::TranslationX), &mut local, 6.0).unwrap();
        assert_eq!(local.translation.x, 3.0);
        assert_eq!(local.rotation, original);
    }
}
