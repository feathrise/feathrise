//! 地图领域显示的单飞后台准备；主帧只比较完整身份、交换Mesh/Arc并更新唯一ghost姿态。
use super::*;
use bevy::camera::visibility::RenderLayers;
use feathrise_runtime::project::author_visuals::{
    self as render, AuthorPrefabError, AuthorPrefabReady,
};
use feathrise_map_content::{MapId, author_visuals::*};
use feathrise_editor_map::game_authoring::map_api::{
    resources::MapEditorViewportState, types::EditorTool,
};
use feathrise_editor_map::standalone::embedded_author_preview::{
    EmbeddedExternalAuthorVisuals, ExternalAuthorVisual,
};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
struct Key {
    identity: MapDomainIdentity,
    hash: String,
    dependencies: String,
    selection: u64,
    selected: Option<String>,
    layer: Option<String>,
    tool: String,
    target: String,
    spacing: u32,
    cells: u64,
}
struct Prepared {
    mesh: Mesh,
    anchors: Arc<HashMap<String, [f32; 3]>>,
    prefab: Option<Arc<AuthorPrefab>>,
    ghost: Option<DomainGhost>,
    key: String,
    content: Arc<Vec<u8>>,
    snapshot: Snapshot,
}
#[derive(Resource, Default)]
struct Visuals {
    domain: String,
    task: Option<Task<(Key, Result<Prepared, String>)>>,
    attempted: Option<Key>,
    adopted: Option<Key>,
    marker: Option<(Entity, Handle<Mesh>, Handle<StandardMaterial>)>,
    ghost: Option<Entity>,
    anchors: Arc<HashMap<String, [f32; 3]>>,
    description: Option<DomainGhost>,
    key: String,
    sequence: u64,
    last_pose: Option<AuthorVisualPosition>,
    map: Option<MapId>,
}
fn density(world: &World) -> (f32, u64) {
    let (extent, size) = world
        .get_resource::<MapEditorViewportState>()
        .map_or((60., UVec2::new(1280, 720)), |v| (v.extent, v.render_size));
    let spacing = (extent / size.y.max(1) as f32 * 12.)
        .max(0.7)
        .log2()
        .ceil()
        .exp2();
    (
        spacing,
        (u64::from(size.x) * u64::from(size.y)).div_ceil(144).max(1),
    )
}
fn wanted(world: &World) -> Option<Key> {
    let maps = world.resource::<NativeMaps>();
    let index = maps.index?;
    let identity = identity(world, maps)?;
    let editor = world.resource::<MapEditorState>();
    if !editor.open
        || editor.close_requested
        || editor.map_transition_in_progress
        || !matches!(editor.tool, EditorTool::Inspect) && editor.tool != authoring_tool(maps)
    {
        return None;
    }
    let tool = &world.resource::<NativeTools>().tools[&index];
    let wire = tool.snapshot.map_domain.as_ref()?;
    if wire.identity.as_ref() != Some(&identity) || maps.task.is_some() || maps.gesture.is_some() {
        return None;
    }
    let document = wire.document.as_ref()?;
    let (spacing, cells) = density(world);
    Some(Key {
        identity,
        hash: document.hash.clone(),
        dependencies: document
            .dependencies
            .iter()
            .map(|d| d.hash.as_str())
            .collect::<Vec<_>>()
            .join(":"),
        selection: wire.selection_ticket,
        selected: wire.selected.clone(),
        layer: wire.layer.clone(),
        tool: wire.tool.clone(),
        target: wire
            .pointer
            .as_ref()
            .map_or(String::new(), |p| p.target.clone()),
        spacing: spacing.to_bits(),
        cells,
    })
}
fn prepare(
    project: &std::path::Path,
    snapshot: Snapshot,
    ground: Option<ground::Ground>,
    key: &Key,
) -> Result<Prepared, String> {
    let wire = snapshot.map_domain.as_ref().ok_or("地图作者显示回执缺失")?;
    if wire.identity.as_ref() != Some(&key.identity) {
        return Err("地图作者显示回执已过期".into());
    }
    let document = wire.document.as_ref().ok_or("地图作者显示回执缺少正文")?;
    if document.hash != key.hash
        || wire.selection_ticket != key.selection
        || wire.selected != key.selected
        || wire.layer != key.layer
        || wire.tool != key.tool
        || wire.pointer.as_ref().map_or("", |p| p.target.as_str()) != key.target
        || document
            .dependencies
            .iter()
            .map(|d| d.hash.as_str())
            .collect::<Vec<_>>()
            .join(":")
            != key.dependencies
    {
        return Err("地图作者显示的正文、目录、工具或稳定选择已变化".into());
    }
    let visuals = wire.visuals.as_ref().ok_or("地图作者没有返回显示结果")?;
    visuals.validate()?;
    let mut lines: AuthorLines =
        serde_json::from_slice(&native_domain::read_blob(project, &visuals.lines)?)
            .map_err(|e| e.to_string())?;
    lines.validate()?;
    let mut anchors: HashMap<_, _> = lines
        .anchors
        .iter()
        .map(|a| (a.id.clone(), a.position))
        .collect();
    if let Some(ground) = ground {
        for group in &lines.ground_groups {
            if let Ok(position) = ground.project(group.position) {
                let delta = position[1] - group.position[1];
                for vertex in &mut lines.positions[group.first..group.first + group.count] {
                    vertex[1] += delta;
                }
                if let Some(id) = &group.anchor {
                    anchors.insert(id.clone(), position);
                }
            }
        }
        for anchor in &mut lines.anchors {
            if let Some(position) = anchors.get(&anchor.id) {
                anchor.position = *position;
            }
        }
    }
    let prefab = visuals
        .prefab
        .as_ref()
        .map(|blob| -> Result<Arc<AuthorPrefab>, String> {
            let value: AuthorPrefab =
                serde_json::from_slice(&native_domain::read_blob(project, blob)?)
                    .map_err(|e| e.to_string())?;
            value.validate()?;
            Ok(Arc::new(value))
        })
        .transpose()?;
    let runtime = visuals
        .runtime
        .as_ref()
        .map(|blob| -> Result<serde_json::Value, String> {
            serde_json::from_slice(&native_domain::read_blob(project, blob)?)
                .map_err(|e| e.to_string())
        })
        .transpose()?;
    let content_key =
        feathrise_map_content::authoring_io::bytes_hash(format!("{key:?}").as_bytes());
    let content = Arc::new(
        serde_json::to_vec(&AuthorVisualContent {
            key: content_key.clone(),
            lines: lines.clone(),
            runtime,
        })
        .map_err(|e| e.to_string())?,
    );
    Ok(Prepared {
        mesh: render::line_mesh(lines),
        anchors: Arc::new(anchors),
        prefab,
        ghost: visuals.ghost.clone(),
        key: content_key,
        content,
        snapshot,
    })
}
fn clear(world: &mut World, state: &mut Visuals) {
    if let Some(root) = state.ghost.take() {
        world.despawn(root);
    }
    if let Some((entity, mesh, material)) = state.marker.take() {
        world.despawn(entity);
        let old = world.resource_mut::<Assets<Mesh>>().remove(mesh.id());
        world
            .resource_mut::<Assets<StandardMaterial>>()
            .remove(material.id());
        retire(world, old);
    }
    state.adopted = None;
    state.description = None;
    state.last_pose = None;
    retire(world, std::mem::take(&mut state.anchors));
}
fn next(state: &mut Visuals) -> u64 {
    state.sequence = state.sequence.checked_add(1).expect("地图视觉序号耗尽");
    state.sequence
}
fn publish_content(world: &mut World, state: &mut Visuals, map: MapId, bytes: Arc<Vec<u8>>) {
    let revision = next(state);
    let mut source = world.resource_mut::<EmbeddedExternalAuthorVisuals>();
    let old = source.content.replace(ExternalAuthorVisual {
        map: map.clone(),
        revision,
        domain: format!("{}/content", state.domain),
        bytes,
    });
    drop(source);
    retire(world, old);
    state.map = Some(map);
}
fn position(world: &World, state: &Visuals) -> Option<[f32; 3]> {
    let ghost = state.description.as_ref()?;
    if ghost.follow_pointer {
        let maps = world.resource::<NativeMaps>();
        let editor = world.resource::<MapEditorState>();
        if editor.tool != authoring_tool(maps) || editor.suppress_world_pick_this_frame {
            return None;
        }
        return if editor.domain_brush_active {
            editor.brush_cursor_world
        } else {
            editor.cursor_world
        }
        .map(|p| p.to_array());
    }
    ghost.position.or_else(|| {
        ghost
            .anchor
            .as_ref()
            .and_then(|id| state.anchors.get(id).copied())
    })
}
fn pose(world: &mut World, state: &mut Visuals) {
    let position = position(world, state);
    let (yaw, elevation) = state
        .description
        .as_ref()
        .map_or((0., 0.), |g| (g.yaw_degrees, g.elevation));
    let maps = world.resource::<NativeMaps>();
    let active = world.resource::<MapEditorState>().tool == authoring_tool(maps);
    let position = active.then_some(position).flatten();
    if let Some(root) = state.ghost {
        let ready = world.get::<AuthorPrefabReady>(root).is_some();
        if let Some(position) = position {
            world.entity_mut(root).insert(
                Transform::from_translation(Vec3::from_array(position) + Vec3::Y * elevation)
                    .with_rotation(Quat::from_rotation_y(yaw.to_radians())),
            );
        }
        if let Some(error) = world.get::<AuthorPrefabError>(root).map(|e| e.0.clone()) {
            world.resource_mut::<MapEditorState>().status = format!("角色显示未就绪：{error}");
        }
        world
            .entity_mut(root)
            .insert(if ready && position.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
    }
    let value = AuthorVisualPosition {
        key: state.key.clone(),
        position,
        yaw_degrees: yaw,
        elevation,
    };
    if state.last_pose.as_ref() != Some(&value) {
        if let Some(map) = state.map.clone() {
            let bytes = Arc::new(serde_json::to_vec(&value).expect("固定姿态可序列化"));
            let revision = next(state);
            let old = world
                .resource_mut::<EmbeddedExternalAuthorVisuals>()
                .pose
                .replace(ExternalAuthorVisual {
                    map,
                    revision,
                    domain: format!("{}/pose", state.domain),
                    bytes,
                });
            retire(world, old);
        }
        state.last_pose = Some(value);
    }
}
fn advance(world: &mut World) {
    world.resource_scope(|world, mut state: Mut<Visuals>| {
        let current = wanted(world);
        if state.adopted.as_ref() != current.as_ref() && state.adopted.is_some() {
            clear(world, &mut state);
            if let Some(map) = state.map.clone() {
                let content = AuthorVisualContent {
                    key: format!("clear-{}", state.sequence),
                    lines: Default::default(),
                    runtime: None,
                };
                publish_content(
                    world,
                    &mut state,
                    map,
                    Arc::new(serde_json::to_vec(&content).unwrap()),
                );
            }
        }
        if let Some((expected, result)) = state.task.as_mut().and_then(check_ready) {
            state.task = None;
            if current.as_ref() == Some(&expected) {
                match result {
                    Ok(prepared) => {
                        let mesh = world.resource_mut::<Assets<Mesh>>().add(prepared.mesh);
                        let material = world.resource_mut::<Assets<StandardMaterial>>().add(
                            StandardMaterial {
                                base_color: Color::WHITE,
                                unlit: true,
                                ..default()
                            },
                        );
                        let entity = world
                            .spawn((
                                Name::new("外部地图作者候选标记"),
                                Mesh3d(mesh.clone()),
                                MeshMaterial3d(material.clone()),
                                Transform::default(),
                                Visibility::Inherited,
                                RenderLayers::layer(31),
                            ))
                            .id();
                        state.marker = Some((entity, mesh, material));
                        state.ghost = prepared.prefab.map(|prefab| {
                            render::spawn_prefab(world, prefab, RenderLayers::layer(31))
                        });
                        state.anchors = prepared.anchors;
                        state.description = prepared.ghost;
                        state.key = prepared.key;
                        state.adopted = Some(expected.clone());
                        state.domain = expected.identity.domain.clone();
                        publish_content(
                            world,
                            &mut state,
                            MapId::parse(&expected.identity.map_id).unwrap(),
                            prepared.content,
                        );
                        let index = world.resource::<NativeMaps>().index.unwrap();
                        let mut tools = world.resource_mut::<NativeTools>();
                        let tool = tools.tools.get_mut(&index).unwrap();
                        if prepared.snapshot.revision >= tool.snapshot.revision {
                            tool.snapshot = prepared.snapshot;
                            tools.ui_revision = tools.ui_revision.wrapping_add(1);
                        }
                    }
                    Err(error) => {
                        world.resource_mut::<MapEditorState>().status =
                            format!("地图辅助显示未采用：{error}")
                    }
                }
            } else {
                retire(world, result);
            }
        }
        if let Some(expected) = current {
            if state.task.is_none() && state.attempted.as_ref() != Some(&expected) {
                let maps = world.resource::<NativeMaps>();
                let tool = &world.resource::<NativeTools>().tools[&maps.index.unwrap()];
                if tool.inflight.is_none() && tool.queue.is_empty() && maps.task.is_none() {
                    let sender = tool.requests.clone();
                    let project = maps.project.clone();
                    let ground = ground::capture(world, &expected.identity);
                    state.attempted = Some(expected.clone());
                    state.task = Some(IoTaskPool::get().spawn(async move {
                        let result = call(
                            &sender,
                            MapDomainRequest {
                                identity: expected.identity.clone(),
                                command: MapDomainCommand::Visuals {
                                    spacing: f32::from_bits(expected.spacing),
                                    visible_cells: expected.cells,
                                },
                            },
                        )
                        .and_then(|snapshot| prepare(&project, snapshot, ground, &expected));
                        (expected, result)
                    }));
                }
            }
        } else {
            state.attempted = None;
        }
        pose(world, &mut state);
    });
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<Visuals>()
        .init_resource::<EmbeddedExternalAuthorVisuals>()
        .add_systems(
            Update,
            advance
                .after(super::poll)
                .after(game_authoring::DomainInputSettled)
                .before(MapEditorSystemSet::Presentation),
        );
}
pub(super) fn ready_to_exit(world: &World) -> bool {
    world
        .get_resource::<Visuals>()
        .is_none_or(|v| v.task.is_none())
}

/// 只在显式验收中冻结实际实体事实；发布了描述或创建了截图请求均不能充当模型就绪。
#[cfg(all(feature = "validation", feathrise_external_validation))]
pub(super) fn validation_snapshot(world: &World) -> Result<Option<serde_json::Value>, String> {
    let Some(state) = world.get_resource::<Visuals>() else {
        return Ok(None);
    };
    let Some(adopted) = state.adopted.as_ref() else {
        return Ok(None);
    };
    if wanted(world).as_ref() != Some(adopted) || state.task.is_some() {
        return Ok(None);
    }
    let mesh_assets = world.resource::<Assets<Mesh>>();
    let (marker, marker_vertices, marker_visible, marker_topology) =
        state
            .marker
            .as_ref()
            .map_or((None, 0, false, String::new()), |(entity, mesh, _)| {
                let mesh = mesh_assets.get(mesh);
                (
                    Some(entity.to_bits()),
                    mesh.map_or(0, Mesh::count_vertices),
                    visible(world, *entity),
                    mesh.map_or(String::new(), |mesh| {
                        format!("{:?}", mesh.primitive_topology())
                    }),
                )
            });
    let Some(root) = state.ghost else {
        return Ok(None);
    };
    if let Some(error) = world.get::<AuthorPrefabError>(root) {
        return Err(error.0.clone());
    }
    let ready = world.get::<AuthorPrefabReady>(root).is_some();
    let root_visible = visible(world, root);
    let mut queue = std::collections::VecDeque::from([root]);
    let mut observed = 0;
    let (mut mesh_count, mut visible_meshes, mut vertices) = (0, 0, 0);
    // 本轮中性模型有限；诊断读取也有固定上限，绝不在主帧扫描整个工程或无界骨架。
    while let Some(entity) = queue.pop_front() {
        observed += 1;
        if observed > 512 {
            return Err("地图视觉验收模型超过512实体预算".into());
        }
        if let Some(mesh) = world
            .get::<Mesh3d>(entity)
            .and_then(|mesh| mesh_assets.get(&mesh.0))
        {
            mesh_count += 1;
            vertices += mesh.count_vertices();
            if visible(world, entity) {
                visible_meshes += 1;
            }
        }
        if let Some(children) = world.get::<Children>(entity) {
            queue.extend(children.iter());
        }
    }
    Ok(Some(
        serde_json::json!({"source":"local adopted mesh+AuthorPrefabReady+Visibility+InheritedVisibility+ViewVisibility",
        "map":adopted.identity.map_id,"document_hash":adopted.hash,"identity":state.key,
        "current":true,"selected":adopted.selected,"marker":marker,"marker_vertices":marker_vertices,
        "marker_topology":marker_topology,"marker_visible":marker_visible,
        "root":root.to_bits(),"prefab_ready":ready,"root_visible":root_visible,
        "model_meshes":mesh_count,"model_vertices":vertices,"visible_model_meshes":visible_meshes,
        "ready":marker_vertices>0 && marker_visible && ready && root_visible && vertices>0 && visible_meshes>0}),
    ))
}

#[cfg(all(feature = "validation", feathrise_external_validation))]
fn visible(world: &World, entity: Entity) -> bool {
    world.get::<Visibility>(entity).is_some_and(|visibility|*visibility != Visibility::Hidden)
        && world.get::<InheritedVisibility>(entity).is_some_and(|value| value.get())
        // 预览根未直接携带Mesh，不要求其ViewVisibility；可见网格仍须经过当前视图剔除。
        && (world.get::<Mesh3d>(entity).is_none()
            || world.get::<ViewVisibility>(entity).is_some_and(|value| value.get()))
}
