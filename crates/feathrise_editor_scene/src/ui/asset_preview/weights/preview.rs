//! 私有网格替换仅作用于当前预览根与两个动作容器。A/B 对照复用已发布姿态。

use super::super::animation::{AnimationEditorState, WeightBinding};
use super::{
    asset::Geometry,
    resources::{Key, WeightEditor},
    systems::Target,
};
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState};
use bevy::{
    gltf::{Gltf, GltfMesh},
    mesh::skinning::SkinnedMesh,
    prelude::*,
    world_serialization::WorldInstanceReady,
};
use std::collections::HashMap;

const NODES_PER_FRAME: usize = 128;

#[cfg(feature = "validation")]
pub(super) fn drawn(world: &World, root: Entity, original: bool) -> bool {
    let draw = world.resource::<WeightPreview>();
    let state = world.resource::<WeightEditor>();
    let Some(session) = state.session.as_ref() else {
        return false;
    };
    draw.parts.iter().any(|part| {
        let Some(draft) = session.drafts[part.surface].shaded.as_ref() else {
            return false;
        };
        let Some(actual) = world.get::<Mesh3d>(part.entity) else {
            return false;
        };
        let expected = if original { &part.original_mesh } else { draft };
        if &actual.0 != expected {
            return false;
        }
        let mut entity = part.entity;
        for _ in 0..128 {
            if entity == root {
                return true;
            }
            let Some(parent) = world.get::<ChildOf>(entity) else {
                return false;
            };
            entity = parent.parent();
        }
        false
    })
}

struct Part {
    entity: Entity,
    root: Entity,
    surface: usize,
    original_mesh: Handle<Mesh>,
    original_material: Option<Handle<StandardMaterial>>,
    current_mesh: Handle<Mesh>,
    current_material: Option<Handle<StandardMaterial>>,
}

#[derive(Resource, Default)]
pub(super) struct WeightPreview {
    key: Option<Key>,
    roots: [Option<Entity>; 3],
    ready_revision: u64,
    scanned_ready: u64,
    sources: HashMap<AssetId<Mesh>, Geometry>,
    source_cursor: usize,
    stack: Vec<(Entity, Entity, usize)>,
    parts: Vec<Part>,
    pose_parts: HashMap<Entity, Vec<(Entity, Geometry)>>,
    scan_epoch: u64,
    heat_material: Option<Handle<StandardMaterial>>,
    applied: Option<(u64, bool)>,
}

impl WeightPreview {
    fn restore(&mut self, commands: &mut Commands) {
        for part in self.parts.drain(..) {
            if part.current_mesh != part.original_mesh {
                commands
                    .entity(part.entity)
                    .try_insert(Mesh3d(part.original_mesh));
            }
            if part.current_material != part.original_material {
                if let Some(material) = part.original_material {
                    commands
                        .entity(part.entity)
                        .try_insert(MeshMaterial3d(material));
                } else {
                    commands
                        .entity(part.entity)
                        .try_remove::<MeshMaterial3d<StandardMaterial>>();
                }
            }
        }
        self.stack.clear();
        self.pose_parts.clear();
        self.scan_epoch = self.scan_epoch.wrapping_add(1);
        self.applied = None;
    }

    pub(super) fn pose_parts(
        &self,
        key: &Key,
        root: Entity,
    ) -> Option<(u64, &[(Entity, Geometry)])> {
        if self.key.as_ref() != Some(key) || !self.stack.is_empty() || self.applied.is_none() {
            return None;
        }
        Some((self.scan_epoch, self.pose_parts.get(&root)?.as_slice()))
    }

    pub(super) fn surface_entities(
        &self,
        key: &Key,
        surface: usize,
    ) -> impl Iterator<Item = Entity> + '_ {
        let matches = self.key.as_ref() == Some(key);
        // 静置和两个动作容器已经按当前模型登记，无需遍历场景层级或刚性附件。
        self.parts
            .iter()
            .filter(move |part| matches && part.surface == surface)
            .map(|part| part.entity)
    }
}

pub(super) fn ready(
    event: On<WorldInstanceReady>,
    target: Target,
    state: Res<WeightEditor>,
    binding: Option<Res<WeightBinding>>,
    mut draw: ResMut<WeightPreview>,
) {
    let Some(session) = state
        .session
        .as_ref()
        .filter(|session| target.matches(&session.key))
    else {
        return;
    };
    if target.preview.scene_root() == Some(event.entity)
        || binding.as_ref().is_some_and(|binding| {
            binding
                .weight_roots(&session.key.path)
                .contains(&Some(event.entity))
        })
    {
        draw.ready_revision = draw.ready_revision.wrapping_add(1);
    }
}

pub(super) fn update(
    mut commands: Commands,
    target: Target,
    state: Res<WeightEditor>,
    sidebar: Res<SceneEditorSidebarState>,
    animation: Option<Res<AnimationEditorState>>,
    binding: Option<Res<WeightBinding>>,
    gltfs: Option<Res<Assets<Gltf>>>,
    gltf_meshes: Option<Res<Assets<GltfMesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    children: Query<&Children>,
    meshes: Query<(
        &Mesh3d,
        Option<&MeshMaterial3d<StandardMaterial>>,
        Has<SkinnedMesh>,
    )>,
    mut draw: ResMut<WeightPreview>,
) {
    let session = state
        .session
        .as_ref()
        .filter(|session| target.matches(&session.key) && !session.stale);
    let Some(session) = session else {
        if draw.key.take().is_some() {
            draw.restore(&mut commands);
            draw.sources.clear();
            draw.source_cursor = 0;
            draw.roots = [None; 3];
        }
        return;
    };
    let model = target.preview.scene_root();
    let [first, second] = binding
        .as_ref()
        .map_or([None; 2], |binding| binding.weight_roots(&session.key.path));
    let roots = [model, first, second];
    let changed_source = draw.key.as_ref() != Some(&session.key);
    if changed_source || draw.roots != roots || draw.scanned_ready != draw.ready_revision {
        draw.restore(&mut commands);
        draw.key = Some(session.key.clone());
        draw.roots = roots;
        draw.scanned_ready = draw.ready_revision;
        draw.stack
            .extend(roots.into_iter().flatten().map(|root| (root, root, 0)));
        if changed_source {
            draw.sources.clear();
            draw.source_cursor = 0;
        }
        return; // 等原组件归位，避免将上个版本的私有句柄误当成原始图元。
    }
    let skinned = session.doc.surfaces.len();
    let source_count = skinned + session.doc.rigid.len();
    if draw.source_cursor < source_count {
        let Some(gltf) = gltfs.as_ref().and_then(|assets| assets.get(&session.gltf)) else {
            return;
        };
        let Some(assets) = gltf_meshes else { return };
        let end = (draw.source_cursor + NODES_PER_FRAME).min(source_count);
        while draw.source_cursor < end {
            let index = draw.source_cursor;
            let (mesh, primitive, geometry) = if index < skinned {
                let part = &session.doc.surfaces[index];
                (part.mesh, part.primitive, Geometry::Skin(index))
            } else {
                let index = index - skinned;
                let part = &session.doc.rigid[index];
                (part.mesh, part.primitive, Geometry::Rigid(index))
            };
            let Some(mesh) = gltf.meshes.get(mesh).and_then(|handle| assets.get(handle)) else {
                return;
            };
            let Some(primitive) = mesh.primitives.get(primitive) else {
                return;
            };
            draw.sources.insert(primitive.mesh.id(), geometry);
            draw.source_cursor += 1;
        }
        if draw.source_cursor < source_count {
            return;
        }
    }
    let heat = sidebar.right == EditorRightSidebarTab::Weights
        && session.heat
        && !session.original
        && animation
            .as_ref()
            .is_none_or(|animation| !animation.weight_motion_active());
    if heat && draw.heat_material.is_none() {
        let Some(materials) = materials.as_deref_mut() else {
            return;
        };
        draw.heat_material = Some(materials.add(StandardMaterial {
            unlit: true,
            base_color: Color::WHITE,
            cull_mode: None,
            ..default()
        }));
    }
    let mut added = false;
    for _ in 0..NODES_PER_FRAME {
        let Some((entity, root, depth)) = draw.stack.pop() else {
            break;
        };
        if depth < 128
            && let Ok(list) = children.get(entity)
        {
            draw.stack
                .extend(list.iter().map(|child| (child, root, depth + 1)));
        }
        if let Ok((mesh, material, skinned)) = meshes.get(entity)
            && let Some(&geometry) = draw.sources.get(&mesh.0.id())
            && skinned == matches!(geometry, Geometry::Skin(_))
        {
            draw.pose_parts
                .entry(root)
                .or_default()
                .push((entity, geometry));
            let Geometry::Skin(surface) = geometry else {
                continue;
            };
            let material = material.map(|material| material.0.clone());
            draw.parts.push(Part {
                entity,
                root,
                surface,
                original_mesh: mesh.0.clone(),
                original_material: material.clone(),
                current_mesh: mesh.0.clone(),
                current_material: material,
            });
            added = true;
        }
    }
    if !draw.stack.is_empty() {
        return;
    }
    let revision = (state.revision, heat);
    if !added && draw.applied == Some(revision) {
        return;
    }
    let heat_material = draw.heat_material.clone();
    for part in &mut draw.parts {
        let heat_mesh = session
            .heat_mesh
            .as_ref()
            .filter(|(surface, _)| *surface == part.surface && heat && Some(part.root) == model);
        let draft = (!session.original)
            .then(|| session.drafts[part.surface].shaded.as_ref())
            .flatten();
        let mesh = heat_mesh
            .map(|(_, mesh)| mesh)
            .or(draft)
            .unwrap_or(&part.original_mesh);
        let material = if heat_mesh.is_some() {
            heat_material.as_ref()
        } else {
            part.original_material.as_ref()
        };
        if &part.current_mesh != mesh {
            commands
                .entity(part.entity)
                .try_insert(Mesh3d(mesh.clone()));
            part.current_mesh = mesh.clone();
        }
        if part.current_material.as_ref() != material {
            if let Some(material) = material {
                commands
                    .entity(part.entity)
                    .try_insert(MeshMaterial3d(material.clone()));
            } else {
                commands
                    .entity(part.entity)
                    .try_remove::<MeshMaterial3d<StandardMaterial>>();
            }
            part.current_material = material.cloned();
        }
    }
    draw.applied = Some(revision);
}
