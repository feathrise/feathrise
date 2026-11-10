//! 主帧只读一个现成小网格或共享根；瓦片差分、定义克隆及编码都留在后台。
use super::resources::*;
use crate::authoring::{EditorAuthoringSession, mesh_edit::MeshEditState};
use bevy::prelude::*;
use feathrise_scene_content::{
    SceneId, SparseHeightLiveOverride, SparseHeightLiveSnapshot, TerrainId,
    embedded_author_preview::{AuthorPreview, encode},
    embedded_snapshot::SparseHeightSnapshotTile,
};
use feathrise_scene::{
    DynamicClipmapWaterDraft, DynamicClipmapWaterDraftSnapshot, TerrainObjectAuthoringTarget,
};
use std::sync::Arc;

pub(super) enum Frozen {
    Pose(super::poses::FrozenPose),
    Value(AuthorPreview),
    Height {
        scene_id: SceneId,
        terrain: Option<TerrainId>,
        now: SparseHeightLiveSnapshot,
        before: Option<SparseHeightLiveSnapshot>,
    },
    Water {
        scene_id: SceneId,
        terrain: Option<TerrainId>,
        root: bool,
        revision: u64,
        snapshot: Option<Arc<DynamicClipmapWaterDraftSnapshot>>,
    },
}

pub(super) fn identity(world: &World, state: &EmbeddedAuthorPreviewState) -> Option<Identity> {
    if !state.enabled || state.session == 0 || state.generation == 0 {
        return None;
    }
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    if draft.terrain_documents.loading
        || !draft.manifest.is_data_driven()
        || world
            .get_resource::<crate::resources::SceneEditorState>()
            .is_some_and(|editor| editor.scene_transition_in_progress)
    {
        return None;
    }
    Some(Identity {
        scene_id: draft.scene_id.clone(),
        session: state.session,
        generation: state.generation,
        epoch: state.epoch,
    })
}
fn terrain(world: &World, map: &SceneId, state: &EmbeddedAuthorPreviewState) -> Option<Option<TerrainId>> {
    let draft = world
        .get_resource::<EditorAuthoringSession>()?
        .draft
        .as_ref()?;
    if draft.manifest.terrains.is_none() {
        return Some(None);
    }
    let id = &draft.terrain_documents.active.as_ref()?.definition.id;
    let target = world.get_resource::<TerrainObjectAuthoringTarget>()?;
    (target.applied_to(map, id) && state.applied_terrain(map, id, state.session, state.generation))
        .then(|| Some(id.clone()))
}
pub(super) fn capture(
    world: &World,
    state: &mut EmbeddedAuthorPreviewState,
    map: &SceneId,
) -> Option<(Key, Version, Frozen)> {
    // 六个固定来源轮转，连续笔划不能阻塞整体变换的可靠结束边沿。
    for _ in 0..8 {
        let cursor = state.cursor;
        state.cursor = (cursor + 1) % 8;
        let item = match cursor {
            6 | 7 => world.get_resource::<EmbeddedExternalAuthorVisuals>().and_then(|source|if cursor==6 {source.content.as_ref()} else {source.pose.as_ref()}).filter(|source|&source.scene_id==map).map(|source| (
                Key::Extension(cursor),Version {revision:source.revision,present:true},
                Frozen::Value(AuthorPreview::Extension {scene_id:map.clone(),revision:source.revision,domain:source.domain.clone(),payload:source.bytes.clone()}))),
            0 => world
                .get_resource::<MeshEditState>()
                .filter(|source| source.embedded_scene.as_ref() == Some(map))
                .map(|source| {
                    (
                        Key::Mesh,
                        Version {
                            revision: source.embedded_revision,
                            present: source.embedded_preview.is_some(),
                        },
                        Frozen::Value(AuthorPreview::Mesh {
                            scene_id: map.clone(),
                            revision: source.embedded_revision,
                            value: source.embedded_preview.clone(),
                            committed: source.embedded_committed,
                        }),
                    )
                }),
            1 => terrain(world, map, state).and_then(|id| {
                let source = world.get_resource::<SparseHeightLiveOverride>()?;
                let present = source.scene_id() == Some(map);
                let key = Key::Height(id.clone());
                if !present && !state.acknowledged.contains_key(&key) {
                    return None;
                }
                let now = if present {
                    source.snapshot()
                } else {
                    SparseHeightLiveOverride::default().snapshot()
                };
                Some((
                    key,
                    Version {
                        revision: source.revision(),
                        present,
                    },
                    Frozen::Height {
                        scene_id: map.clone(),
                        terrain: id.clone(),
                        now,
                        before: state.heights.get(&id).cloned(),
                    },
                ))
            }),
            4 | 5 => super::poses::capture(state, cursor == 5),
            _ => {
                let root = cursor == 3;
                let id = if root {
                    Some(None)
                } else {
                    terrain(world, map, state)
                };
                id.and_then(|id| {
                    let source = world.get_resource::<DynamicClipmapWaterDraft>()?;
                    let key = Key::Water(id.clone(), root);
                    let snapshot = if root {
                        source.shared_root_for_scene(map)
                    } else {
                        source.shared_for_scene(map)
                    };
                    if snapshot.is_none() && !state.acknowledged.contains_key(&key) {
                        return None;
                    }
                    let revision = if root {
                        source.root_revision_for_scene(map)
                    } else {
                        source.revision_for_scene(map)
                    }
                    .unwrap_or(0);
                    Some((
                        key,
                        Version {
                            revision,
                            present: snapshot.is_some(),
                        },
                        Frozen::Water {
                            scene_id: map.clone(),
                            terrain: id,
                            root,
                            revision,
                            snapshot,
                        },
                    ))
                })
            }
        };
        if let Some((key, version, frozen)) = item
            && state.acknowledged.get(&key) != Some(&version)
            && state.failed.as_ref() != Some(&(key.clone(), version.clone()))
        {
            return Some((key, version, frozen));
        }
    }
    None
}
pub(super) fn encode_frozen(
    identity: Identity,
    sequence: u64,
    key: Key,
    version: Version,
    frozen: Frozen,
) -> Completion {
    let mut height = None;
    let result = (|| {
        let value = match frozen {
            Frozen::Pose(pose) => pose.encode(identity.scene_id.clone(), version.revision),
            Frozen::Value(value) => value,
            Frozen::Height {
                scene_id: map,
                terrain,
                now,
                before,
            } => {
                let reset = before
                    .as_ref()
                    .is_none_or(|old| old.scene_id() != now.scene_id())
                    || now.tile_count() == 0;
                let mut tiles = Vec::new();
                now.visit_changed_tiles(if reset { None } else { before.as_ref() }, |key, tile| {
                    tiles.push(SparseHeightSnapshotTile {
                        key,
                        bytes: Arc::new(tile.encode_bytes()),
                    });
                });
                let revision = version.revision;
                height = Some(now);
                AuthorPreview::Height {
                    scene_id: map,
                    terrain_id: terrain,
                    revision,
                    reset,
                    tiles,
                }
            }
            Frozen::Water {
                scene_id: map,
                terrain,
                root,
                revision,
                snapshot,
            } => AuthorPreview::Water {
                scene_id: map,
                terrain_id: terrain,
                root,
                revision: snapshot
                    .as_ref()
                    .map_or(revision, |snapshot| snapshot.source_revision),
                definition: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.definition.clone()),
                masks: snapshot
                    .map(|snapshot| snapshot.sources.clone())
                    .unwrap_or_default(),
            },
        };
        Ok(Arc::new(EmbeddedAuthorPreview {
            session: identity.session,
            generation: identity.generation,
            sequence,
            scene_id: identity.scene_id.0.clone(),
            bytes: Arc::new(encode(&value)?),
        }))
    })();
    Completion {
        identity,
        key,
        version,
        height,
        result,
    }
}
