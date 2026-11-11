//! 私有网格替换仅作用于当前预览根与两个动作容器。A/B 对照复用已发布姿态。

use super::super::animation::{AnimationEditorState, WeightBinding};
use super::{
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
    sources: HashMap<AssetId<Mesh>, usize>,
    stack: Vec<(Entity, usize)>,
    parts: Vec<Part>,
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
        self.applied = None;
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
    meshes: Query<(&Mesh3d, Option<&MeshMaterial3d<StandardMaterial>>), With<SkinnedMesh>>,
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
            .extend(roots.into_iter().flatten().map(|root| (root, 0)));
        if changed_source {
            draw.sources.clear();
        }
        return; // 等原组件归位，避免将上个版本的私有句柄误当成原始图元。
    }
    if draw.sources.is_empty() {
        let Some(gltf) = gltfs.as_ref().and_then(|assets| assets.get(&session.gltf)) else {
            return;
        };
        let Some(assets) = gltf_meshes else { return };
        let mut sources = HashMap::new();
        for (index, part) in session.doc.surfaces.iter().enumerate() {
            let Some(mesh) = gltf
                .meshes
                .get(part.mesh)
                .and_then(|handle| assets.get(handle))
            else {
                return;
            };
            let Some(primitive) = mesh.primitives.get(part.primitive) else {
                return;
            };
            sources.insert(primitive.mesh.id(), index);
        }
        draw.sources = sources;
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
        let Some((entity, depth)) = draw.stack.pop() else {
            break;
        };
        if depth < 128
            && let Ok(list) = children.get(entity)
        {
            draw.stack
                .extend(list.iter().map(|child| (child, depth + 1)));
        }
        if let Ok((mesh, material)) = meshes.get(entity)
            && let Some(&surface) = draw.sources.get(&mesh.0.id())
        {
            let material = material.map(|material| material.0.clone());
            draw.parts.push(Part {
                entity,
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
            .filter(|(surface, _)| *surface == part.surface && heat);
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
