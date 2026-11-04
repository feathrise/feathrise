//! 原生地图领域的非阻塞调度；只在后台调用DLL、读取正文和构建索引，主帧按完整身份原子采用。
#[path = "map_domain/ground.rs"]
mod ground;
#[path = "map_domain/viewport.rs"]
mod viewport;
#[path = "map_domain/visuals.rs"]
mod visuals;
use super::{resources::NativeTools, types::Job};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_editor_core::MapEditorSystemSet;
use feathrise_editor_map::game_authoring::{
    self,
    map_api::{
        authoring::{
            self, AuthoringRetirement, EditorAuthoringSession, EditorCommand, MapDomainAdapter,
            MapDomainDocument,
        },
        resources::MapEditorState,
    },
    map_interaction::*,
    native_domain::{self, NativeDocument, NativeDomainPort},
};
use feathrise_plugin_api::{Request, Snapshot, map_domain::*};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone, Copy)]
enum Kind {
    Initial,
    Sync,
    Author,
    History(bool),
    Close,
}
struct Candidate {
    document: MapDomainDocument,
    interaction: Arc<MapInteractionSnapshot>,
    snapshot: Snapshot,
}
struct Completion {
    expected: MapDomainIdentity,
    kind: Kind,
    result: Result<Option<Candidate>, String>,
}
/// 历史只持有不可变文件描述，重放由后台重读并重建索引；不保留每一版整图数组和Mesh。
struct NativeHistory {
    descriptor: feathrise_plugin_api::map_domain::MapDomainDocument,
}
#[derive(Resource)]
pub(super) struct NativeMaps {
    project: PathBuf,
    root: PathBuf,
    index: Option<usize>,
    port: Option<Arc<NativeDomainPort>>,
    task: Option<Task<Completion>>,
    generation: u64,
    map: Option<String>,
    adopted_sequence: u64,
    inputs: VecDeque<MapDomainViewport>,
    gesture: Option<viewport::Gesture>,
    close: VecDeque<MapDomainIdentity>,
    known: BTreeMap<String, MapDomainIdentity>,
    last_snapshot_revision: u64,
    scene: Option<MapDomainScene>,
    failed: Option<(MapDomainIdentity, u64)>,
    history_entries: usize,
}
impl NativeMaps {
    fn new(project: &crate::project::Project) -> Self {
        Self {
            project: project.root.clone(),
            root: project.context.asset_roots[0].clone(),
            index: None,
            port: None,
            task: None,
            generation: 0,
            map: None,
            adopted_sequence: 0,
            inputs: Default::default(),
            gesture: None,
            close: Default::default(),
            known: Default::default(),
            last_snapshot_revision: 0,
            scene: None,
            failed: None,
            history_entries: 0,
        }
    }
}
impl Drop for NativeMaps {
    fn drop(&mut self) {
        // 只解除本作者实例登记；不等待DLL，通道最后一位所有者释放后由后台正常销毁租约。
        if let Some(port) = &self.port {
            native_domain::unregister(&self.root, port);
        }
    }
}
pub(super) fn ready_to_exit(world: &World) -> Result<bool, String> {
    Ok(visuals::ready_to_exit(world) && world.get_resource::<NativeMaps>().is_none_or(|maps| {
        maps.task.is_none()
            && maps.inputs.is_empty()
            && maps.gesture.is_none()
            && maps.close.is_empty()
    }))
}
/// 只由后台调用，回包直接回到发起的IO任务，不依赖主线程处理Event。
fn call(
    sender: &async_channel::Sender<Job>,
    request: MapDomainRequest,
) -> Result<Snapshot, String> {
    request.validate()?;
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    sender
        .send_blocking(Job {
            sequence: 0,
            request: Request::MapDomain { request },
            activate: false,
            reply: Some(reply),
        })
        .map_err(|_| "地图原生作者线程已退出")?;
    let snapshot = receiver
        .recv_timeout(std::time::Duration::from_secs(120))
        .map_err(|_| "地图原生作者没有在120秒内返回，当前草稿保留")??;
    if let Some(error) = &snapshot.error {
        return Err(error.clone());
    }
    Ok(snapshot)
}
fn establish(world: &mut World, maps: &mut NativeMaps) -> Result<(), String> {
    if maps.port.is_some() {
        return Ok(());
    }
    let state = world.resource::<NativeTools>();
    let mut matches = state
        .tools
        .iter()
        .filter(|(_, tool)| tool.snapshot.map_domain.is_some());
    let Some((&index, tool)) = matches.next() else {
        return Ok(());
    };
    if matches.next().is_some() {
        return Err("当前工程安装了多个地图领域插件，请只保留一个地图正文作者".into());
    }
    if tool.snapshot.map_domain.as_ref().is_none_or(|wire| {
        wire.ground_patch_preparation != GROUND_PATCH_PREPARATION_PROTOCOL
    }) {
        return Err("地图作者插件不支持地形跟随纯准备协议，请重新构建并安装当前地图插件".into());
    }
    let domain = tool.domain;
    let sender = tool.requests.clone();
    let port = Arc::new(NativeDomainPort {
        domain,
        project_root: maps.project.clone(),
        asset_root: maps.root.clone(),
        call: Arc::new(move |request| {
            call(&sender, request)?
                .map_domain
                .ok_or_else(|| "地图原生作者缺少领域回执".into())
        }),
    });
    native_domain::register(port.clone())?;
    maps.index = Some(index);
    maps.port = Some(port);
    world.insert_resource(MapDomainAdapter {
        load: native_domain::load,
        park: |_, _| {},
        restore: |world, _, _| {
            world.resource_mut::<NativeMaps>().map = None;
        },
        close: close_map,
        recover: recover,
        history: start_history,
    });
    Ok(())
}
fn identity(world: &World, maps: &NativeMaps) -> Option<MapDomainIdentity> {
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    let domain = maps.port.as_ref()?.domain;
    let generation = draft
        .domain
        .as_ref()
        .and_then(|d| d.get::<NativeDocument>())
        .map_or(maps.generation, |d| d.identity.generation);
    let ground_content = world
        .get_resource::<feathrise_scene::DynamicClipmapGroundSurface>()
        .and_then(|surface| surface.snapshot.as_ref())
        .filter(|snapshot| {
            snapshot.map_id == draft.map_id
                && snapshot.authoring_height_revision() == Some(session.terrain_revision)
        })
        .map(|snapshot| snapshot.content_revision_words());
    Some(MapDomainIdentity {
        domain: domain.into(),
        map_id: draft.map_id.to_string(),
        generation,
        revision: session.revision,
        terrain_revision: session.terrain_revision,
        environment_revision: session.environment_revision,
        ground_content,
    })
}
fn current_document(world: &World) -> Option<MapDomainDocument> {
    world
        .get_resource::<EditorAuthoringSession>()?
        .draft
        .as_ref()?
        .domain
        .clone()
}
fn candidate(
    sender: &async_channel::Sender<Job>,
    port: Arc<NativeDomainPort>,
    snapshot: Snapshot,
    current: Option<MapDomainDocument>,
    ground: Option<ground::Ground>,
) -> Result<Candidate, String> {
    let snapshot = ground::resolve(sender, &port, snapshot, ground)?;
    let wire = snapshot
        .map_domain
        .as_ref()
        .ok_or("地图作者回执缺少领域数据")?;
    let descriptor = wire.document.as_ref().ok_or("地图作者回执缺少完整正文")?;
    let document = if let Some(old) = current
        .as_ref()
        .and_then(|d| d.get::<NativeDocument>())
        .filter(|old| {
            old.descriptor.body == descriptor.body && old.descriptor.scene == descriptor.scene
        }) {
        let mut next = old.clone();
        next.identity = wire.identity.clone().ok_or("地图作者没有返回完整身份")?;
        next.descriptor = descriptor.clone();
        native_domain::document(next)
    } else {
        native_domain::validate_snapshot(port, wire)?
    };
    let interaction = native_domain::interaction(
        document
            .get::<NativeDocument>()
            .ok_or("地图作者宿主类型不匹配")?,
        wire.identity.as_ref().map_or(0, |i| i.generation),
    );
    Ok(Candidate {
        document,
        interaction,
        snapshot,
    })
}
fn spawn_request(
    world: &mut World,
    maps: &mut NativeMaps,
    expected: MapDomainIdentity,
    command: MapDomainCommand,
    kind: Kind,
) {
    let index = maps.index.unwrap();
    let port = maps.port.as_ref().unwrap().clone();
    let current = current_document(world);
    let ground = ground::capture(world, &expected);
    let sender = {
        let mut state = world.resource_mut::<NativeTools>();
        let tool = state.tools.get_mut(&index).unwrap();
        tool.map_pending = true;
        tool.requests.clone()
    };
    maps.task = Some(IoTaskPool::get().spawn(async move {
        let result = (|| {
            let snapshot = call(
                &sender,
                MapDomainRequest {
                    identity: expected.clone(),
                    command,
                },
            )?;
            if matches!(kind, Kind::Close) {
                Ok(None)
            } else {
                candidate(&sender, port, snapshot, current, ground).map(Some)
            }
        })();
        Completion {
            expected,
            kind,
            result,
        }
    }));
}
fn spawn_author(
    world: &mut World,
    maps: &mut NativeMaps,
    expected: MapDomainIdentity,
    snapshot: Snapshot,
) {
    let port = maps.port.as_ref().unwrap().clone();
    let current = current_document(world);
    let ground = ground::capture(world, &expected);
    let sender = world.resource::<NativeTools>().tools[&maps.index.unwrap()]
        .requests
        .clone();
    maps.last_snapshot_revision = snapshot.revision;
    maps.task = Some(IoTaskPool::get().spawn(async move {
        Completion {
            expected,
            kind: Kind::Author,
            result: candidate(&sender, port, snapshot, current, ground).map(Some),
        }
    }));
}
fn retire<T: Send + Sync + 'static>(world: &mut World, value: T) {
    world.resource_mut::<AuthoringRetirement>().push(value);
}
fn apply_completion(world: &mut World, maps: &mut NativeMaps, completion: Completion) {
    let index = maps.index.unwrap();
    if matches!(completion.kind, Kind::History(_)) {
        world.resource_mut::<EditorAuthoringSession>().saving = false;
    }
    if !matches!(completion.kind, Kind::Close)
        && identity(world, maps).as_ref() != Some(&completion.expected)
    {
        retire(world, completion);
        world
            .resource_mut::<NativeTools>()
            .tools
            .get_mut(&index)
            .unwrap()
            .map_pending = false;
        maps.inputs.clear();
        maps.gesture = None;
        maps.last_snapshot_revision = 0;
        return;
    }
    match completion.result {
        Ok(Some(candidate)) => {
            let wire = candidate.snapshot.map_domain.as_ref().unwrap();
            let sequence = wire.edit_sequence;
            let selected = wire.selected.clone();
            let layer = wire.layer.clone();
            let mut retired = None;
            world.resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
                let draft = session.draft.as_mut().expect("完整身份已确认草稿");
                let changed = draft
                    .domain
                    .as_ref()
                    .is_none_or(|old| old.hash() != candidate.document.hash());
                let before = draft
                    .domain
                    .as_ref()
                    .and_then(|d| d.get::<NativeDocument>())
                    .map(|d| {
                        Arc::new(NativeHistory {
                            descriptor: d.descriptor.clone(),
                        }) as authoring::DomainValue
                    });
                let after = Arc::new(NativeHistory {
                    descriptor: candidate
                        .document
                        .get::<NativeDocument>()
                        .expect("后台已验证类型")
                        .descriptor
                        .clone(),
                }) as authoring::DomainValue;
                let domain = candidate.document.domain;
                retired = draft.domain.replace(candidate.document);
                match completion.kind {
                    Kind::Author if changed => {
                        // 固定256笔的描述历史；整段退役O(1)移交后台，同时正确复位原画布归属游标。
                        if maps.history_entries >= 256 {
                            session.clear_history(Some(
                                &mut world.resource_mut::<AuthoringRetirement>(),
                            ));
                            maps.history_entries = 0;
                        }
                        if let Some(before) = before {
                            session.undo.push(EditorCommand::DomainChanged {
                                domain,
                                before,
                                after,
                            });
                            maps.history_entries += 1;
                        }
                        let redo = std::mem::take(&mut session.redo);
                        retire(world, redo);
                        session.mark_changed(&mut world.resource_mut::<MapEditorState>());
                    }
                    Kind::History(undoing) => {
                        let command = if undoing {
                            session.pop_undo()
                        } else {
                            session.redo.pop()
                        };
                        if let Some(command) = command {
                            if undoing {
                                session.redo.push(command);
                            } else {
                                session.undo.push(command);
                            }
                        }
                        session.mark_changed(&mut world.resource_mut::<MapEditorState>());
                    }
                    _ => {}
                }
            });
            retire(world, retired);
            let mut interaction = world.resource_mut::<MapInteractionState>();
            let old = interaction.snapshot.replace(candidate.interaction);
            interaction.enabled = true;
            interaction.observed_generation = completion.expected.generation;
            interaction.selected = selected;
            interaction.selected_layer = layer;
            interaction.status = candidate.snapshot.status.clone();
            drop(interaction);
            retire(world, old);
            maps.adopted_sequence = sequence;
            maps.failed = None;
            maps.last_snapshot_revision = candidate.snapshot.revision;
            let mut state = world.resource_mut::<NativeTools>();
            let tool = state.tools.get_mut(&index).unwrap();
            if candidate.snapshot.revision >= tool.snapshot.revision {
                tool.snapshot = candidate.snapshot;
            }
            tool.map_pending = false;
            state.ui_revision = state.ui_revision.wrapping_add(1);
        }
        Ok(None) => {
            world
                .resource_mut::<NativeTools>()
                .tools
                .get_mut(&index)
                .unwrap()
                .map_pending = false;
        }
        Err(error) => {
            maps.inputs.clear();
            maps.gesture = None;
            world.resource_mut::<MapEditorState>().status = format!("地图作者操作未采用：{error}");
            let mut state = world.resource_mut::<NativeTools>();
            let tool = state.tools.get_mut(&index).unwrap();
            tool.map_pending = false;
            tool.snapshot.status = error.clone();
            tool.queue.clear();
            maps.last_snapshot_revision = tool.snapshot.revision;
            if matches!(completion.kind, Kind::Author) {
                // 下一轮仅将原宿主正文同步回DLL一次，撤销未完整投射的暂态；失败不反复排IO。
                maps.failed = None;
            } else {
                maps.failed = Some((completion.expected, tool.snapshot.revision));
            }
            if error.contains("120秒") {
                tool.fault = Some(error);
            }
        }
    }
}
fn step(world: &mut World, maps: &mut NativeMaps) {
    if let Err(error) = establish(world, maps) {
        world.resource_mut::<MapEditorState>().status = error;
        return;
    }
    if maps.port.is_none() {
        return;
    }
    if let Some(task) = maps.task.as_mut() {
        if let Some(completion) = check_ready(task) {
            maps.task = None;
            apply_completion(world, maps, completion);
        } else {
            return;
        }
    }
    let current = identity(world, maps);
    if let Some(current) = &current {
        if maps.map.as_ref() != Some(&current.map_id) {
            maps.map = Some(current.map_id.clone());
            maps.generation = native_domain::next_generation();
            maps.inputs.clear();
            maps.gesture = None;
            maps.last_snapshot_revision = 0;
            maps.failed = None;
            maps.history_entries = 0;
        }
    }
    let current = identity(world, maps);
    if let Some(close) = maps.close.pop_front() {
        spawn_request(world, maps, close, MapDomainCommand::Close, Kind::Close);
        return;
    }
    let Some(current) = current else {
        world.resource_mut::<MapInteractionState>().enabled = false;
        return;
    };
    maps.known.insert(current.map_id.clone(), current.clone());
    let index = maps.index.unwrap();
    let (wire, snapshot_revision, inflight, queued) = {
        let state = world.resource::<NativeTools>();
        let tool = &state.tools[&index];
        (
            tool.snapshot.map_domain.clone().unwrap(),
            tool.snapshot.revision,
            tool.inflight.is_some(),
            !tool.queue.is_empty(),
        )
    };
    if inflight {
        return;
    }
    if maps
        .failed
        .as_ref()
        .is_some_and(|(failed, revision)| failed == &current && *revision == snapshot_revision)
    {
        return;
    }
    let domain = current_document(world);
    if domain.is_none() {
        spawn_request(world, maps, current, MapDomainCommand::Open, Kind::Initial);
        return;
    }
    let Some(native) = domain.as_ref().and_then(|d| d.get::<NativeDocument>()) else {
        return;
    };
    if wire.document.as_ref().is_some_and(|d| {
        d.dirty
            && !native.descriptor.dirty
            && d.hash == native.descriptor.hash
            && d.source_hash != native.descriptor.source_hash
    }) {
        spawn_request(
            world,
            maps,
            current,
            MapDomainCommand::Saved {
                hash: native.descriptor.source_hash.clone(),
            },
            Kind::Sync,
        );
        return;
    }
    if wire.identity.as_ref() != Some(&current) {
        spawn_request(
            world,
            maps,
            current,
            MapDomainCommand::Adopt {
                document: native.descriptor.clone(),
            },
            Kind::Sync,
        );
        return;
    }
    if wire.edit_sequence != maps.adopted_sequence
        && snapshot_revision != maps.last_snapshot_revision
    {
        if wire
            .document
            .as_ref()
            .is_some_and(|d| d.hash != native.descriptor.hash)
        {
            let snapshot = world.resource::<NativeTools>().tools[&index]
                .snapshot
                .clone();
            spawn_author(world, maps, current, snapshot);
            return;
        }
        maps.adopted_sequence = wire.edit_sequence;
    }
    if wire
        .document
        .as_ref()
        .is_none_or(|d| d.hash != native.descriptor.hash)
    {
        spawn_request(
            world,
            maps,
            current,
            MapDomainCommand::Adopt {
                document: native.descriptor.clone(),
            },
            Kind::Sync,
        );
        return;
    }
    if let Some(input) = maps.scene.take() {
        spawn_request(
            world,
            maps,
            current,
            MapDomainCommand::Scene { input },
            Kind::Author,
        );
        return;
    }
    if let Some(input) = maps.inputs.pop_front() {
        spawn_request(
            world,
            maps,
            current,
            MapDomainCommand::Viewport { input },
            Kind::Author,
        );
        return;
    }
    if !queued {
        world
            .resource_mut::<NativeTools>()
            .tools
            .get_mut(&index)
            .unwrap()
            .map_pending = false;
    }
}
pub(super) fn poll(world: &mut World) {
    world.resource_scope(|world, mut maps: Mut<NativeMaps>| {
        step(world, &mut maps);
        publish(world, &maps);
    });
}
fn settled(world: &mut World) {
    world.resource_scope(|world, maps: Mut<NativeMaps>| publish(world, &maps));
}
fn publish(world: &mut World, maps: &NativeMaps) {
    let Some(index) = maps.index else {
        return;
    };
    let Some(identity) = identity(world, maps) else {
        return;
    };
    let (wire, busy) = {
        let state = world.resource::<NativeTools>();
        let tool = &state.tools[&index];
        let Some(wire) = &tool.snapshot.map_domain else {
            return;
        };
        (wire.clone(), tool.busy())
    };
    let target = GroundTarget::new(maps.port.as_ref().unwrap().domain, &identity, &wire);
    let active = world.resource::<MapEditorState>().tool == authoring_tool(maps);
    let mut interaction = world.resource_mut::<MapInteractionState>();
    interaction.enabled = true;
    interaction.observed_generation = identity.generation;
    interaction.work_pending = maps.task.is_some() || busy || !maps.inputs.is_empty();
    interaction.unfinished_shape = maps.gesture.is_some();
    interaction.precise_cursor = active;
    interaction.cursor_target = active.then_some(target);
    interaction.camera_mouse_owned = active && maps.gesture.is_some();
    if interaction.selection_request.is_none() {
        interaction.selected = wire.selected;
        interaction.selected_layer = wire.layer;
    }
    if interaction.close_request.is_some()
        && !interaction.work_pending
        && !interaction.unfinished_shape
    {
        interaction.close_completed = interaction.close_request.take();
    }
    drop(interaction);
    let mut editor = world.resource_mut::<MapEditorState>();
    editor.domain_brush_active = active && wire.continuous;
    if let Some(pointer) = &wire.pointer {
        editor.brush_radius = pointer.radius;
        editor.brush_spacing = pointer.spacing;
    }
}
fn authoring_tool(
    maps: &NativeMaps,
) -> feathrise_editor_map::game_authoring::map_api::types::EditorTool {
    feathrise_editor_map::game_authoring::map_api::types::EditorTool::Domain(
        maps.port.as_ref().map_or("", |p| p.domain),
    )
}
struct GroundTarget;
impl GroundTarget {
    fn new(domain:&'static str,identity:&MapDomainIdentity,wire:&MapDomainSnapshot)->feathrise_editor_map::game_authoring::map_api::viewport::vegetation_ground::GroundCursorTarget{
        feathrise_editor_map::game_authoring::map_api::viewport::vegetation_ground::GroundCursorTarget::Domain(DomainGroundTarget{domain,identity:format!("{identity:?}:{}:{:?}:{:?}:{}:{:?}",wire.tool,wire.selected,wire.layer,wire.selection_ticket,wire.pointer.as_ref().map(|p|&p.target)),continuous:wire.continuous,click_qualified:!wire.continuous,payload:Arc::new(identity.clone())})
    }
}
fn close_map(world: &mut World, map: feathrise_map_content::MapId) {
    let mut maps = world.resource_mut::<NativeMaps>();
    if let Some(identity) = maps.known.remove(map.as_str()) {
        maps.close.push_back(identity);
    }
    if maps.map.as_deref() == Some(map.as_str()) {
        maps.map = None;
        maps.inputs.clear();
        maps.gesture = None;
    }
}
fn recover(world: &mut World) -> bool {
    let mut maps = world.resource_mut::<NativeMaps>();
    if maps.task.is_some() {
        return false;
    }
    maps.gesture = None;
    maps.inputs.clear();
    maps.last_snapshot_revision = 0;
    maps.failed = None;
    true
}
fn start_history(world: &mut World, undoing: bool) -> Result<(), String> {
    world.resource_scope(|world, mut maps: Mut<NativeMaps>| {
        if maps.task.is_some() || maps.gesture.is_some() {
            return Err("地图角色正在完成整笔操作".into());
        }
        let session = world.resource::<EditorAuthoringSession>();
        let command = if undoing {
            session.undo.last()
        } else {
            session.redo.last()
        }
        .ok_or("地图历史为空")?;
        let EditorCommand::DomainChanged {
            domain,
            before,
            after,
        } = command
        else {
            return Err("地图历史不属于原生领域".into());
        };
        if maps.port.as_ref().is_none_or(|p| p.domain != *domain) {
            return Err("地图历史作者插件已不可用".into());
        }
        let value = if undoing { before } else { after };
        let descriptor = if let Some(history) = value.downcast_ref::<NativeHistory>() {
            history.descriptor.clone()
        } else if let Some(document) = value.downcast_ref::<NativeDocument>() {
            document.descriptor.clone()
        } else {
            return Err("地图历史宿主类型不匹配".into());
        };
        let expected = identity(world, &maps).ok_or("地图已关闭")?;
        world.resource_mut::<EditorAuthoringSession>().saving = true;
        spawn_request(
            world,
            &mut maps,
            expected,
            MapDomainCommand::Adopt {
                document: descriptor,
            },
            Kind::History(undoing),
        );
        Ok(())
    })
}
pub(super) fn install(app: &mut App, project: &crate::project::Project) {
    visuals::install(app);
    app.insert_resource(NativeMaps::new(project)).init_resource::<MapInteractionState>()
        .add_systems(Update,poll.after(super::systems::poll).in_set(MapEditorSystemSet::Tasks).in_set(MapInteractionAdoption))
        .add_systems(Update,viewport::input.in_set(game_authoring::DomainViewportInput))
        .add_systems(Update,settled.in_set(game_authoring::DomainInputSettled))
        .add_systems(Update,viewport::scene_menu.after(feathrise_editor_map::game_authoring::map_api::ui::scene_context_menu::SceneMenuDispatch).in_set(MapEditorSystemSet::Input));
}

#[cfg(feature = "validation")]
pub(super) fn validation_visuals(world: &World) -> Result<Option<serde_json::Value>, String> {
    visuals::validation_snapshot(world)
}

#[cfg(feature = "validation")]
pub(super) fn validation_state(world: &World) -> serde_json::Value {
    let Some(maps) = world.get_resource::<NativeMaps>() else { return serde_json::Value::Null; };
    let current = identity(world, maps);
    let wire = maps.index.and_then(|index|world.resource::<NativeTools>().tools.get(&index))
        .and_then(|tool|tool.snapshot.map_domain.as_ref());
    let ready = current.is_some() && wire.is_some_and(|wire| wire.identity == current)
        && maps.task.is_none() && maps.inputs.is_empty() && maps.failed.is_none();
    serde_json::json!({"input_ready":ready,"current":current,"wire":wire.and_then(|wire|wire.identity.as_ref()),
        "task":maps.task.is_some(),"queued_inputs":maps.inputs.len(),"gesture":maps.gesture.is_some(),
        "adopted_sequence":maps.adopted_sequence,"last_snapshot_revision":maps.last_snapshot_revision,
        "failed":maps.failed})
}
