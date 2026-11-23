//! 通过真实场景引用打开源文档时，冻结从源场景到原世界的坐标框架。

use bevy::{ecs::{entity_disabling::Disabled, query::Allow}, prelude::*};
use feathrise_scene::{SubsceneActive, SubsceneInstance};
use feathrise_scene_content::{SceneId, SceneInstancePath, SceneInstancePlacement};
use std::collections::{BTreeMap, BTreeSet};

use crate::resources::SceneEditorState;

#[derive(Clone, Debug)]
pub(crate) struct ReferenceCoordinates {
    scene: SceneId,
    path: SceneInstancePath,
    world_from_scene: Mat4,
}

/// 外层框架属于一次正式进入。引用地址随运行实体更新，点击时按精确路径读取实例。
#[derive(Resource, Default)]
pub(crate) struct SceneReferenceCoordinates {
    current: Option<ReferenceCoordinates>,
    direct_instances: BTreeMap<SceneInstancePath, BTreeSet<Entity>>,
}

impl SceneReferenceCoordinates {
    pub(crate) fn world_from_scene(&self, scene: &SceneId) -> Mat4 {
        self.for_scene(scene).map_or(Mat4::IDENTITY, |frame| frame.world_from_scene)
    }

    pub(crate) fn instance_path(&self, scene: &SceneId) -> Option<&SceneInstancePath> {
        Some(&self.for_scene(scene)?.path)
    }

    fn for_scene(&self, scene: &SceneId) -> Option<&ReferenceCoordinates> {
        self.current.as_ref().filter(|frame| &frame.scene == scene)
    }

    pub(crate) fn adopt(&mut self, scene: &SceneId, pending: Option<ReferenceCoordinates>) {
        self.current = pending.filter(|frame| &frame.scene == scene);
    }

    pub(super) fn freeze(
        &self,
        scene: &SceneId,
        reference: &SceneInstancePlacement,
        instances: &Query<(&SubsceneInstance, &GlobalTransform)>,
        active_batches: &Query<(), With<SubsceneActive>>,
    ) -> Result<ReferenceCoordinates, String> {
        let direct_path = SceneInstancePath::root(scene.clone()).child(reference.id.clone());
        let mut active = self.direct_instances.get(&direct_path).into_iter().flatten().filter_map(|&entity| {
            let (instance, pose) = instances.get(entity).ok()?;
            (instance.path == direct_path && instance.source_scene_id == reference.source_scene_id
                && active_batches.contains(instance.batch)).then(|| pose.to_matrix())
        });
        let pose = active.next();
        if active.next().is_some() { return Err("当前子场景引用有多个活动实例，请等场景切换完成".into()); }
        // 子图尚在准备时读取同一个所选引用的作者变换，不能借同源兄弟实例的世界位置。
        let pose = pose.unwrap_or_else(|| reference.transform.as_transform().to_matrix());
        let world_from_scene = self.world_from_scene(scene) * pose;
        if !world_from_scene.is_finite() || !world_from_scene.inverse().is_finite() {
            return Err("当前子场景引用的世界变换无效".into());
        }
        let path = self.instance_path(scene).map_or(direct_path, |path| path.child(reference.id.clone()));
        Ok(ReferenceCoordinates { scene: reference.source_scene_id.clone(), path, world_from_scene })
    }
}

pub(super) fn index_reference(
    insert: On<Insert<SubsceneInstance>>,
    instances: Query<&SubsceneInstance, Allow<Disabled>>,
    mut coordinates: ResMut<SceneReferenceCoordinates>,
) {
    if let Ok(instance) = instances.get(insert.entity)
        && instance.path.instances.len() == 1 && instance.path.terrain_id.is_none()
    {
        coordinates.direct_instances.entry(instance.path.clone()).or_default().insert(insert.entity);
    }
}

pub(super) fn unindex_reference(
    discard: On<Discard<SubsceneInstance>>,
    instances: Query<&SubsceneInstance, Allow<Disabled>>,
    mut coordinates: ResMut<SceneReferenceCoordinates>,
) {
    if let Ok(instance) = instances.get(discard.entity)
        && let Some(entities) = coordinates.direct_instances.get_mut(&instance.path)
    {
        entities.remove(&discard.entity);
        if entities.is_empty() { coordinates.direct_instances.remove(&instance.path); }
    }
}

pub(super) fn clear_inactive(mut editor: ResMut<SceneEditorState>, mut coordinates: ResMut<SceneReferenceCoordinates>) {
    let reset = editor.reset_reference_coordinates;
    if reset { editor.reset_reference_coordinates = false; }
    if coordinates.current.as_ref().is_some_and(|frame| {
        reset || !editor.open || editor.selected_scene_id.as_ref() != Some(&frame.scene)
    }) {
        coordinates.current = None;
    }
    if editor.pending_reference_coordinates.as_ref().is_some_and(|frame| {
        editor.pending_scene_transition.as_ref() != Some(&frame.scene)
    }) {
        editor.pending_reference_coordinates = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use feathrise_scene_content::{SceneInstanceId, SceneInstanceTransform};

    fn reference(id: &str, scene: &SceneId, fallback_x: f32) -> SceneInstancePlacement {
        SceneInstancePlacement {
            id: SceneInstanceId::parse(id).unwrap(), source_scene_id: scene.clone(), label: id.into(),
            transform: SceneInstanceTransform { translation: [fallback_x, 0., 0.], ..default() },
        }
    }

    fn capture(app: &mut App, scene: &SceneId, reference: &SceneInstancePlacement) -> ReferenceCoordinates {
        let scene = scene.clone();
        let reference = reference.clone();
        app.world_mut().run_system_once(move |coordinates: Res<SceneReferenceCoordinates>,
            instances: Query<(&SubsceneInstance, &GlobalTransform)>, active: Query<(), With<SubsceneActive>>|
            coordinates.freeze(&scene, &reference, &instances, &active)).unwrap().unwrap()
    }

    fn spawn_reference(app: &mut App, parent: &SceneId, reference: &SceneInstancePlacement,
        pose: Transform, published: bool) -> Entity {
        let world = app.world_mut();
        let batch = world.spawn_empty().id();
        if published { world.entity_mut(batch).insert(SubsceneActive); }
        world.spawn((SubsceneInstance {
            path: SceneInstancePath::root(parent.clone()).child(reference.id.clone()),
            source_scene_id: reference.source_scene_id.clone(), generation: 1, batch,
        }, GlobalTransform::from(pose))).id()
    }

    #[test]
    fn reference_coordinates_compose_nested_frames_and_track_distinct_instances() {
        let mut app = App::new();
        app.init_resource::<SceneReferenceCoordinates>().add_observer(index_reference).add_observer(unindex_reference);
        let a = SceneId::parse("SCENE-ref-a").unwrap();
        let b = SceneId::parse("SCENE-ref-b").unwrap();
        let c = SceneId::parse("SCENE-ref-c").unwrap();
        let first = reference("first-b", &b, 4.);
        let second = reference("second-b", &b, 5.);
        let nested = reference("nested-c", &c, 6.);
        let first_pose = Transform::from_xyz(12., 0., 2.).with_rotation(Quat::from_rotation_y(0.5));
        let second_pose = Transform::from_xyz(-42., 1., 0.);
        spawn_reference(&mut app, &a, &first, Transform::from_xyz(999., 0., 0.), false);
        let first_entity = spawn_reference(&mut app, &a, &first, first_pose, true);
        spawn_reference(&mut app, &a, &second, second_pose, true);

        let first_frame = capture(&mut app, &a, &first);
        let second_frame = capture(&mut app, &a, &second);
        assert_ne!(first_frame.path, second_frame.path);
        assert_eq!(first_frame.world_from_scene, GlobalTransform::from(first_pose).to_matrix());
        assert_eq!(second_frame.world_from_scene, GlobalTransform::from(second_pose).to_matrix());
        app.world_mut().resource_mut::<SceneReferenceCoordinates>().adopt(&b, Some(first_frame.clone()));

        let nested_frame = capture(&mut app, &b, &nested); // 独立 B 文档里 C 还未采用时，用精确作者路径的变换。
        assert_eq!(nested_frame.path, first_frame.path.child(nested.id.clone()));
        assert_eq!(nested_frame.world_from_scene,
            first_frame.world_from_scene * nested.transform.as_transform().to_matrix());
        app.world_mut().despawn(first_entity);
        assert_eq!(capture(&mut app, &a, &first).world_from_scene, first.transform.as_transform().to_matrix());
    }

    #[test]
    fn reference_coordinates_cancel_and_independent_entry_clear_stale_frames() {
        let mut app = App::new();
        app.init_resource::<SceneReferenceCoordinates>();
        let a = SceneId::parse("SCENE-ref-parent").unwrap();
        let b = SceneId::parse("SCENE-ref-source").unwrap();
        let reference = reference("copy", &b, 4.);
        let frame = capture(&mut app, &a, &reference);
        let mut editor = SceneEditorState { open: true, selected_scene_id: Some(a), ..default() };
        editor.queue_scene_transition(b.clone(), "按引用进入");
        editor.pending_reference_coordinates = Some(frame.clone());
        editor.cancel_pending_scene_transition();
        assert!(editor.pending_reference_coordinates.is_none());
        editor.queue_scene_transition(b.clone(), "按引用进入");
        editor.pending_reference_coordinates = Some(frame.clone());
        editor.queue_scene_transition(b.clone(), "独立进入");
        assert!(editor.pending_reference_coordinates.is_none());

        let mut coordinates = app.world_mut().resource_mut::<SceneReferenceCoordinates>();
        coordinates.adopt(&b, Some(frame.clone()));
        assert_eq!(coordinates.world_from_scene(&b), frame.world_from_scene);
        coordinates.adopt(&b, None);
        assert_eq!(coordinates.world_from_scene(&b), Mat4::IDENTITY);
    }
}
