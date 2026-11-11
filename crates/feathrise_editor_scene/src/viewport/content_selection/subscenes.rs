//! 每帧冻结两份源地表Arc，后台真实求交后只返回父图完整引用身份。
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_scene_content::{SceneId, SceneInstancePlacement};
use feathrise_scene::{SubsceneInstance, SubsceneActive, ClipmapGroundSnapshot, ClipmapGroundHit, DynamicClipmapGroundSurface};
use feathrise_scene_core::residency::InstanceResourceTable;
use std::{ops::Bound::{Excluded, Unbounded}, sync::Arc};
use super::layers::{LayerCandidate, LayerTarget};
#[derive(Default, Clone)]
pub(crate) struct Capture {
    cursor: Option<Entity>, complete: bool, entries: Vec<Snapshot>,
    pub references: Arc<[SceneInstancePlacement]>,
}
#[derive(Clone)]
struct Snapshot { id: String, frame: Mat4, ground: ClipmapGroundSnapshot }
#[derive(SystemParam)]
pub(crate) struct PickInputs<'w, 's> {
    surfaces: Option<Res<'w, InstanceResourceTable<DynamicClipmapGroundSurface>>>,
    roots: Query<'w, 's, (&'static SubsceneInstance, &'static GlobalTransform)>,
    active: Query<'w, 's, (), With<SubsceneActive>>,
}
impl PickInputs<'_, '_> {
    pub(super) fn capture(&self, map: &SceneId, capture: &mut Capture) -> bool {
        if capture.complete { return true; }
        let Some(surfaces) = self.surfaces.as_ref() else { capture.complete = true; return true; };
        for _ in 0..2 {
            let next = match capture.cursor { Some(cursor) => surfaces.values.range((Excluded(cursor), Unbounded)).next(), None => surfaces.values.first_key_value() };
            let Some((&entity, surface)) = next else { capture.complete = true; return true; };
            capture.cursor = Some(entity);
            let Ok((instance, global)) = self.roots.get(entity) else { continue; };
            if instance.path.root_scene_id != *map || !self.active.contains(instance.batch) { continue; }
            let Some(id) = instance.path.instances.first() else { continue; };
            if let Some(ground) = &surface.snapshot {
                capture.entries.push(Snapshot { id: id.to_string(), frame: global.to_matrix(), ground: ground.clone() });
            }
        }
        false
    }
}
impl Capture {
    pub(super) fn candidates(&self, ray: Ray3d, occlusion: Option<Vec3>) -> Vec<LayerCandidate> {
        let mut hits = std::collections::BTreeMap::<String, f32>::new();
        for source in &self.entries {
            let inverse = source.frame.inverse();
            if let ClipmapGroundHit::Hit(local) = source.ground.ray_intersection(inverse.transform_point3(ray.origin), inverse.transform_vector3(*ray.direction)) {
                let point = source.frame.transform_point3(local); let distance = ray.origin.distance(point);
                if occlusion.is_some_and(|root| distance > ray.origin.distance(root) + 0.02) { continue; }
                hits.entry(source.id.clone()).and_modify(|old| *old = old.min(distance)).or_insert(distance);
            }
        }
        hits.into_iter().filter_map(|(id, distance)| {
            let reference = self.references.iter().find(|reference| reference.id.as_str() == id)?;
            Some(LayerCandidate { author_index: None, target: LayerTarget::Subscene(id),
                label: format!("子场景 · {}", reference.label), rank: 0, distance })
        }).collect()
    }
}
