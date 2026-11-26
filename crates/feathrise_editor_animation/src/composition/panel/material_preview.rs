//! 实际 Scene0 与双缓冲动作容器共用图元身份，只改当前窗口实例的材质句柄。

use super::*;
use crate::{
    data::MaterialSlot,
    resources::BindingState,
    types::{EmbeddedNodeIndex, EmbeddedPrimitive},
};
use bevy::{render::render_resource::Face, world_serialization::WorldInstanceReady};
use feathrise_editor_animation_model::host::AnimationHost;
use std::collections::{BTreeMap, VecDeque, btree_map};

const BUDGET: usize = 128;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum PreviewMaterial {
    Default { inverted: bool },
    Asset(usize),
    Mirrored(usize),
}

enum Cleanup {
    Restore(btree_map::IntoIter<Entity, Handle<StandardMaterial>>),
    Release(btree_map::IntoValues<PreviewMaterial, Handle<StandardMaterial>>),
}

#[derive(Resource, Default)]
struct Draw {
    source: Option<String>,
    material_source: Option<std::sync::Weak<()>>,
    handles: BTreeMap<PreviewMaterial, Handle<StandardMaterial>>,
    revision: Option<std::sync::Weak<BTreeMap<MaterialSlot, Option<usize>>>>,
    roots: [Option<Entity>; 4],
    isolated: Option<(Entity, MaterialSlot, Option<usize>)>,
    refresh: bool,
    stack: Vec<(Entity, usize)>,
    original: BTreeMap<Entity, Handle<StandardMaterial>>,
    cleanup: VecDeque<Cleanup>,
    pending: VecDeque<Entity>,
}

impl Draw {
    fn material_handle(
        &mut self,
        server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
        path: &str,
        index: Option<usize>,
        inverted: bool,
    ) -> Handle<StandardMaterial> {
        self.handles
            .entry(index.map_or(
                PreviewMaterial::Default { inverted },
                PreviewMaterial::Asset,
            ))
            .or_insert_with(|| match index {
                Some(index) => {
                    let reference =
                        bevy::asset::AssetPath::from(format!("{path}#Material{index}/std"));
                    server
                        .get_handle(reference.clone())
                        .unwrap_or_else(|| server.load(reference))
                }
                // 保存最后一个默认槽后，GLB 已不再发布 DefaultMaterial/std。撤销草稿按 glTF
                // 的默认金属度、粗糙度恢复视口，以便再次保存时重新省略这个槽的材质索引。
                None => materials.add(StandardMaterial {
                    metallic: 1.0,
                    perceptual_roughness: 1.0,
                    cull_mode: Some(if inverted { Face::Front } else { Face::Back }),
                    ..default()
                }),
            })
            .clone()
    }

    fn retire_materials(&mut self) {
        if !self.handles.is_empty() {
            let old = std::mem::take(&mut self.handles);
            self.cleanup.push_back(Cleanup::Release(old.into_values()));
        }
    }

    // 恢复可见实例和释放旧文档强句柄共享一帧预算。上一批清完才处理新实例，连续切文件也不会累积批次。
    fn clean(&mut self, mut restore: impl FnMut(Entity, Handle<StandardMaterial>)) {
        for _ in 0..BUDGET {
            let Some(batch) = self.cleanup.front_mut() else {
                break;
            };
            let remaining = match batch {
                Cleanup::Restore(entries) => {
                    if let Some((entity, handle)) = entries.next() {
                        restore(entity, handle);
                    }
                    entries.len()
                }
                Cleanup::Release(entries) => {
                    drop(entries.next());
                    entries.len()
                }
            };
            if remaining == 0 {
                self.cleanup.pop_front();
            }
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Draw>().add_observer(ready).add_systems(
        Update,
        preview
            .in_set(AnimationSystems::Presentation)
            .in_set(SceneEditorSystemSet::Presentation),
    );
}

fn ready(event: On<WorldInstanceReady>, mut draw: ResMut<Draw>) {
    if draw.roots.contains(&Some(event.entity)) {
        draw.refresh = true;
    }
}

fn preview(
    state: Res<AnimationEditorState>,
    panel: Res<State>,
    scene: Res<AssetPreviewState>,
    host: Res<AnimationHost>,
    binding: Option<Res<BindingState>>,
    server: Option<Res<AssetServer>>,
    mut assets: Option<ResMut<Assets<StandardMaterial>>>,
    mut draw: ResMut<Draw>,
    children: Query<&Children>,
    nodes: Query<&EmbeddedNodeIndex>,
    mut meshes: Query<(
        Option<&ChildOf>,
        Option<&EmbeddedPrimitive>,
        &GlobalTransform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let (Some(server), Some(assets)) = (server.as_deref(), assets.as_deref_mut()) else {
        return;
    };
    let owner = scene
        .selected_path
        .as_deref()
        .zip(state.current())
        .filter(|(path, s)| {
            s.document.has_material_draft()
                && !host.weights_own_viewport
                && state
                    .author_path()
                    .and_then(|p| p.strip_prefix(&host.asset_root).ok())
                    == Some(Path::new(path))
        });
    let identity = owner.map(|(_, s)| Arc::downgrade(&s.document.source_identity));
    let same = match (&draw.material_source, &identity) {
        (Some(a), Some(b)) => std::sync::Weak::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    };
    if !same {
        draw.retire_materials();
        draw.material_source = identity;
    }
    let active = owner.filter(|(_, s)| !s.source_changed);
    let source = active.map(|(p, _)| p);
    // 单图元模式展示网格资源。同一资源可能属于多实例，只有面板明确选择的对象决定该副本的材质。
    let isolated = active.and_then(|(_, s)| {
        let model = scene.model.as_ref()?;
        let choice = model.selected_primitive?;
        let node = panel.slot_node?;
        (s.document.disk_mesh_index(node) == Some(choice.id.mesh)).then_some((
            model.primitive_root?,
            MaterialSlot {
                node,
                primitive: choice.id.primitive,
            },
            choice.material,
        ))
    });
    let roots = source.map_or([None; 4], |path| {
        let [a, b] = binding
            .as_deref()
            .map_or([None; 2], |r| r.weight_roots(path));
        [scene.scene_root(), a, b, isolated.map(|(root, _, _)| root)]
    });
    let revision = active.map(|(_, s)| Arc::downgrade(s.document.material_changes()));
    let changed = match (&draw.revision, &revision) {
        (Some(a), Some(b)) => !std::sync::Weak::ptr_eq(a, b),
        (None, None) => false,
        _ => true,
    };
    if draw.source.as_deref() != source
        || draw.roots != roots
        || draw.isolated != isolated
        || draw.refresh
    {
        // 根失效时先恢复还在显示的旧实例，再释放其余缓存的材质。
        if !draw.original.is_empty() {
            let old = std::mem::take(&mut draw.original);
            draw.cleanup.push_front(Cleanup::Restore(old.into_iter()));
        }
        draw.source = source.map(str::to_owned);
        draw.roots = roots;
        draw.isolated = isolated;
        draw.refresh = true;
    }
    if changed || draw.refresh {
        draw.stack = roots.into_iter().flatten().map(|e| (e, 0)).collect();
        draw.pending.clear();
        draw.revision = revision;
        draw.refresh = false;
    }
    draw.clean(|e, h| {
        if let Ok((_, _, _, mut mesh)) = meshes.get_mut(e) {
            mesh.0 = h;
        }
    });
    if !draw.cleanup.is_empty() {
        return;
    }
    let Some((path, session)) = active else {
        return;
    };
    let retry = draw.pending.len().min(BUDGET / 2);
    for i in 0..BUDGET {
        let entity = if i < retry {
            draw.pending.pop_front().unwrap()
        } else {
            let Some(&(e, cursor)) = draw.stack.last() else {
                break;
            };
            if let Some(child) = children.get(e).ok().and_then(|c| c.get(cursor)).copied() {
                draw.stack.last_mut().unwrap().1 += 1;
                draw.stack.push((child, 0));
            } else {
                draw.stack.pop();
            }
            if cursor != 0 {
                continue;
            }
            e
        };
        let Ok((parent, prim, transform, mut mesh)) = meshes.get_mut(entity) else {
            continue;
        };
        let (slot, original, inverted) =
            if let Some((_, slot, original)) = isolated.filter(|(e, _, _)| *e == entity) {
                (slot, original, false)
            } else if let Some((parent, prim)) = parent.zip(prim)
                && let Ok(node) = nodes.get(parent.parent())
            {
                // 默认材质没有负缩放标签，换成显式材质时按该实例的真实手性剔除背面。
                let inverted = prim.inverted
                    || prim.material.is_none() && transform.affine().matrix3.determinant() < 0.0;
                (
                    MaterialSlot {
                        node: node.0,
                        primitive: prim.index,
                    },
                    prim.material,
                    inverted,
                )
            } else {
                continue;
            };
        let Some(desired) = session.document.slot_material(slot) else {
            continue;
        };
        if desired == original {
            if let Some(h) = draw.original.remove(&entity) {
                mesh.0 = h;
            }
            continue;
        }
        // 保持被替换材质的强句柄直到离开文档。热通知、撤销或双容器重建时，未被原网格使用的材质也不会反复卸载再请求。
        let mut handle = draw.material_handle(server, assets, path, desired, inverted);
        if inverted && let Some(index) = desired {
            let key = PreviewMaterial::Mirrored(index);
            if let Some(h) = draw.handles.get(&key) {
                handle = h.clone();
            } else {
                let Some(mut inverted) = assets.get(&handle).cloned() else {
                    draw.pending.push_back(entity);
                    continue;
                };
                if inverted.cull_mode.is_some() {
                    inverted.cull_mode = Some(Face::Front);
                }
                handle = assets.add(inverted);
                draw.handles.insert(key, handle.clone());
            }
        }
        draw.original
            .entry(entity)
            .or_insert_with(|| mesh.0.clone());
        mesh.0 = handle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_can_preview_a_default_slot_removed_from_the_saved_glb() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        let server = app.world().resource::<AssetServer>();
        let mut assets = Assets::<StandardMaterial>::default();
        let mut draw = Draw::default();
        let path = "saved-without-default.glb";
        for (inverted, cull_mode) in [(false, Face::Back), (true, Face::Front)] {
            let handle = draw.material_handle(server, &mut assets, path, None, inverted);
            let material = assets
                .get(&handle)
                .expect("撤销预览不能依赖磁盘 GLB 的默认材质标签");
            assert_eq!(material.metallic, 1.0);
            assert_eq!(material.perceptual_roughness, 1.0);
            assert_eq!(material.cull_mode, Some(cull_mode));
            assert_eq!(
                draw.material_handle(server, &mut assets, path, None, inverted),
                handle
            );
        }
    }

    #[test]
    fn retiring_materials_shares_the_restore_budget_and_skips_empty_switches() {
        let mut assets = Assets::<StandardMaterial>::default();
        let handle = assets.add(StandardMaterial::default());
        let Handle::Strong(strong) = &handle else {
            panic!("预览回归需要真实强句柄");
        };
        let baseline = Arc::strong_count(strong);
        let mut draw = Draw::default();
        for i in 0..BUDGET + 1 {
            draw.handles
                .insert(PreviewMaterial::Asset(i), handle.clone());
            draw.handles
                .insert(PreviewMaterial::Mirrored(i), handle.clone());
        }
        draw.retire_materials();
        draw.retire_materials();
        assert!(draw.handles.is_empty());
        assert_eq!(draw.cleanup.len(), 1);
        assert_eq!(Arc::strong_count(strong), baseline + 2 * (BUDGET + 1));

        let original = BTreeMap::from([(Entity::PLACEHOLDER, handle.clone())]);
        draw.cleanup
            .push_front(Cleanup::Restore(original.into_iter()));
        let mut restored = 0;
        draw.clean(|entity, _| {
            assert_eq!(entity, Entity::PLACEHOLDER);
            restored += 1;
        });
        assert_eq!(restored, 1);
        assert_eq!(Arc::strong_count(strong), baseline + BUDGET + 3);
        draw.clean(|_, _| panic!("旧实例只能恢复一次"));
        assert_eq!(Arc::strong_count(strong), baseline + 3);
        draw.clean(|_, _| panic!("旧实例只能恢复一次"));
        assert_eq!(Arc::strong_count(strong), baseline);
        draw.retire_materials();
        assert!(draw.cleanup.is_empty());
    }
}
