//! 原生场景领域的非阻塞调度；只在后台调用DLL、读取正文和构建索引，主帧按完整身份原子采用。
#[path = "scene_domain/ground.rs"]
mod ground;
#[path = "scene_domain/viewport.rs"]
mod viewport;
#[path = "scene_domain/visuals.rs"]
mod visuals;
use super::{resources::NativeTools, types::Job};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_editor_core::SceneEditorSystemSet;
use feathrise_editor_scene::game_authoring::{
    self,
    scene_api::{
        authoring::{
            self, AuthoringRetirement, EditorAuthoringSession, EditorCommand, SceneDomainAdapter,
            SceneDomainDocument,
        },
        resources::SceneEditorState,
    },
    scene_interaction::*,
    native_domain::{self, NativeDocument, NativeDomainPort},
};
use feathrise_plugin_api::{Request, Snapshot, scene_domain::*};
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
    document: SceneDomainDocument,
    interaction: Arc<SceneInteractionSnapshot>,
    snapshot: Snapshot,
}
struct Completion {
    expected: SceneDomainIdentity,
    kind: Kind,
    result: Result<Option<Candidate>, String>,
}
/// 历史只持有不可变文件描述，重放由后台重读并重建索引；不保留每一版整图数组和Mesh。
struct NativeHistory {
    descriptor: feathrise_plugin_api::scene_domain::SceneDomainDocument,
}
#[derive(Resource)]
pub(super) struct NativeScenes {
    project: PathBuf,
    root: PathBuf,
    index: Option<usize>,
    port: Option<Arc<NativeDomainPort>>,
    task: Option<Task<Completion>>,
    generation: u64,
    scene_id: Option<String>,
    adopted_sequence: u64,
    inputs: VecDeque<SceneDomainViewport>,
    gesture: Option<viewport::Gesture>,
    close: VecDeque<SceneDomainIdentity>,
    known: BTreeMap<String, SceneDomainIdentity>,
    last_snapshot_revision: u64,
    scene: Option<SceneDomainScene>,
    failed: Option<(SceneDomainIdentity, u64)>,
    history_entries: usize,
}
impl NativeScenes {
    fn new(project: &crate::project::Project) -> Self {
        Self {
            project: project.root.clone(),
            root: project.context.asset_roots[0].clone(),
            index: None,
            port: None,
            task: None,
            generation: 0,
            scene_id: None,
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
impl Drop for NativeScenes {
    fn drop(&mut self) {
        // 只解除本作者实例登记；不等待DLL，通道最后一位所有者释放后由后台正常销毁租约。
        if let Some(port) = &self.port {
            native_domain::unregister(&self.root, port);
        }
    }
}
pub(super) fn ready_to_exit(world: &World) -> Result<bool, String> {
    Ok(visuals::ready_to_exit(world) && world.get_resource::<NativeScenes>().is_none_or(|scenes| {
        scenes.task.is_none()
            && scenes.inputs.is_empty()
            && scenes.gesture.is_none()
            && scenes.close.is_empty()
    }))
}
/// 只由后台调用，回包直接回到发起的IO任务，不依赖主线程处理Event。
fn call(
    sender: &async_channel::Sender<Job>,
    request: SceneDomainRequest,
) -> Result<Snapshot, String> {
    request.validate()?;
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    sender
        .send_blocking(Job {
            sequence: 0,
            request: Request::SceneDomain { request },
            activate: false,
            reply: Some(reply),
        })
        .map_err(|_| "场景原生作者线程已退出")?;
    let snapshot = receiver
        .recv_timeout(std::time::Duration::from_secs(120))
        .map_err(|_| "场景原生作者没有在120秒内返回，当前草稿保留")??;
    if let Some(error) = &snapshot.error {
        return Err(error.clone());
    }
    Ok(snapshot)
}
fn establish(world: &mut World, scenes: &mut NativeScenes) -> Result<(), String> {
    if scenes.port.is_some() {
        return Ok(());
    }
    let state = world.resource::<NativeTools>();
    let mut matches = state
        .tools
        .iter()
        .filter(|(_, tool)| tool.snapshot.scene_domain.is_some());
    let Some((&index, tool)) = matches.next() else {
        return Ok(());
    };
    if matches.next().is_some() {
        return Err("当前工程安装了多个场景领域插件，请只保留一个场景正文作者".into());
    }
    if tool.snapshot.scene_domain.as_ref().is_none_or(|wire| {
        wire.ground_patch_preparation != GROUND_PATCH_PREPARATION_PROTOCOL
    }) {
        return Err("场景作者插件不支持地形跟随纯准备协议，请重新构建并安装当前场景插件".into());
    }
    let domain = tool.domain;
    let sender = tool.requests.clone();
    let port = Arc::new(NativeDomainPort {
        domain,
        project_root: scenes.project.clone(),
        asset_root: scenes.root.clone(),
        call: Arc::new(move |request| {
            call(&sender, request)?
                .scene_domain
                .ok_or_else(|| "场景原生作者缺少领域回执".into())
        }),
    });
    native_domain::register(port.clone())?;
    scenes.index = Some(index);
    scenes.port = Some(port);
    world.insert_resource(SceneDomainAdapter {
        load: native_domain::load,
        park: |_, _| {},
        restore: |world, _, _| {
            world.resource_mut::<NativeScenes>().scene_id = None;
        },
        close: close_scene,
        recover: recover,
        history: start_history,
    });
    Ok(())
}
fn identity(world: &World, scenes: &NativeScenes) -> Option<SceneDomainIdentity> {
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    let domain = scenes.port.as_ref()?.domain;
    let generation = draft
        .domain
        .as_ref()
        .and_then(|d| d.get::<NativeDocument>())
        .map_or(scenes.generation, |d| d.identity.generation);
    let ground_content = world
        .get_resource::<feathrise_scene::DynamicClipmapGroundSurface>()
        .and_then(|surface| surface.snapshot.as_ref())
        .filter(|snapshot| {
            snapshot.scene_id == draft.scene_id
                && snapshot.authoring_height_revision() == Some(session.terrain_revision)
        })
        .map(|snapshot| snapshot.content_revision_words());
    Some(SceneDomainIdentity {
        domain: domain.into(),
        scene_id: draft.scene_id.to_string(),
        generation,
        revision: session.revision,
        terrain_revision: session.terrain_revision,
        environment_revision: session.environment_revision,
        ground_content,
    })
}
fn current_document(world: &World) -> Option<SceneDomainDocument> {
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
    current: Option<SceneDomainDocument>,
    ground: Option<ground::Ground>,
) -> Result<Candidate, String> {
    let snapshot = ground::resolve(sender, &port, snapshot, ground)?;
    let wire = snapshot
        .scene_domain
        .as_ref()
        .ok_or("场景作者回执缺少领域数据")?;
    let descriptor = wire.document.as_ref().ok_or("场景作者回执缺少完整正文")?;
    let document = if let Some(old) = current
        .as_ref()
        .and_then(|d| d.get::<NativeDocument>())
        .filter(|old| {
            old.descriptor.body == descriptor.body && old.descriptor.scene == descriptor.scene
        }) {
        let mut next = old.clone();
        next.identity = wire.identity.clone().ok_or("场景作者没有返回完整身份")?;
        next.descriptor = descriptor.clone();
        native_domain::document(next)
    } else {
        native_domain::validate_snapshot(port, wire)?
    };
    let interaction = native_domain::interaction(
        document
            .get::<NativeDocument>()
            .ok_or("场景作者宿主类型不匹配")?,
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
    scenes: &mut NativeScenes,
    expected: SceneDomainIdentity,
    command: SceneDomainCommand,
    kind: Kind,
) {
    let index = scenes.index.unwrap();
    let port = scenes.port.as_ref().unwrap().clone();
    let current = current_document(world);
    let ground = ground::capture(world, &expected);
    let sender = {
        let mut state = world.resource_mut::<NativeTools>();
        let tool = state.tools.get_mut(&index).unwrap();
        tool.scene_pending = true;
        tool.requests.clone()
    };
    scenes.task = Some(IoTaskPool::get().spawn(async move {
        let result = (|| {
            let snapshot = call(
                &sender,
                SceneDomainRequest {
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
    scenes: &mut NativeScenes,
    expected: SceneDomainIdentity,
    snapshot: Snapshot,
) {
    let port = scenes.port.as_ref().unwrap().clone();
    let current = current_document(world);
    let ground = ground::capture(world, &expected);
    let sender = world.resource::<NativeTools>().tools[&scenes.index.unwrap()]
        .requests
        .clone();
    scenes.last_snapshot_revision = snapshot.revision;
    scenes.task = Some(IoTaskPool::get().spawn(async move {
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
fn apply_completion(world: &mut World, scenes: &mut NativeScenes, completion: Completion) {
    let index = scenes.index.unwrap();
    if matches!(completion.kind, Kind::History(_)) {
        world.resource_mut::<EditorAuthoringSession>().saving = false;
    }
    if !matches!(completion.kind, Kind::Close)
        && identity(world, scenes).as_ref() != Some(&completion.expected)
    {
        retire(world, completion);
        world
            .resource_mut::<NativeTools>()
            .tools
            .get_mut(&index)
            .unwrap()
            .scene_pending = false;
        scenes.inputs.clear();
        scenes.gesture = None;
        scenes.last_snapshot_revision = 0;
        return;
    }
    match completion.result {
        Ok(Some(candidate)) => {
            let wire = candidate.snapshot.scene_domain.as_ref().unwrap();
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
                        if scenes.history_entries >= 256 {
                            session.clear_history(Some(
                                &mut world.resource_mut::<AuthoringRetirement>(),
                            ));
                            scenes.history_entries = 0;
                        }
                        if let Some(before) = before {
                            session.undo.push(EditorCommand::DomainChanged {
                                domain,
                                before,
                                after,
                            });
                            scenes.history_entries += 1;
                        }
                        let redo = std::mem::take(&mut session.redo);
                        retire(world, redo);
                        session.mark_changed(&mut world.resource_mut::<SceneEditorState>());
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
                        session.mark_changed(&mut world.resource_mut::<SceneEditorState>());
                    }
                    _ => {}
                }
            });
            retire(world, retired);
            let mut interaction = world.resource_mut::<SceneInteractionState>();
            let old = interaction.snapshot.replace(candidate.interaction);
            interaction.enabled = true;
            interaction.observed_generation = completion.expected.generation;
            interaction.selected = selected;
            interaction.selected_layer = layer;
            interaction.status = candidate.snapshot.status.clone();
            drop(interaction);
            retire(world, old);
            scenes.adopted_sequence = sequence;
            scenes.failed = None;
            scenes.last_snapshot_revision = candidate.snapshot.revision;
            let mut state = world.resource_mut::<NativeTools>();
            let tool = state.tools.get_mut(&index).unwrap();
            if candidate.snapshot.revision >= tool.snapshot.revision {
                tool.snapshot = candidate.snapshot;
            }
            tool.scene_pending = false;
            state.ui_revision = state.ui_revision.wrapping_add(1);
        }
        Ok(None) => {
            world
                .resource_mut::<NativeTools>()
                .tools
                .get_mut(&index)
                .unwrap()
                .scene_pending = false;
        }
        Err(error) => {
            scenes.inputs.clear();
            scenes.gesture = None;
            world.resource_mut::<SceneEditorState>().status = format!("场景作者操作未采用：{error}");
            let mut state = world.resource_mut::<NativeTools>();
            let tool = state.tools.get_mut(&index).unwrap();
            tool.scene_pending = false;
            tool.snapshot.status = error.clone();
            tool.queue.clear();
            scenes.last_snapshot_revision = tool.snapshot.revision;
            if matches!(completion.kind, Kind::Author) {
                // 下一轮仅将原宿主正文同步回DLL一次，撤销未完整投射的暂态；失败不反复排IO。
                scenes.failed = None;
            } else {
                scenes.failed = Some((completion.expected, tool.snapshot.revision));
            }
            if error.contains("120秒") {
                tool.fault = Some(error);
            }
        }
    }
}
fn step(world: &mut World, scenes: &mut NativeScenes) {
    if let Err(error) = establish(world, scenes) {
        world.resource_mut::<SceneEditorState>().status = error;
        return;
    }
    if scenes.port.is_none() {
        return;
    }
    if let Some(task) = scenes.task.as_mut() {
        if let Some(completion) = check_ready(task) {
            scenes.task = None;
            apply_completion(world, scenes, completion);
        } else {
            return;
        }
    }
    let current = identity(world, scenes);
    if let Some(current) = &current {
        if scenes.scene_id.as_ref() != Some(&current.scene_id) {
            scenes.scene_id = Some(current.scene_id.clone());
            scenes.generation = native_domain::next_generation();
            scenes.inputs.clear();
            scenes.gesture = None;
            scenes.last_snapshot_revision = 0;
            scenes.failed = None;
            scenes.history_entries = 0;
        }
    }
    let current = identity(world, scenes);
    if let Some(close) = scenes.close.pop_front() {
        spawn_request(world, scenes, close, SceneDomainCommand::Close, Kind::Close);
        return;
    }
    let Some(current) = current else {
        world.resource_mut::<SceneInteractionState>().enabled = false;
        return;
    };
    scenes.known.insert(current.scene_id.clone(), current.clone());
    let index = scenes.index.unwrap();
    let (wire, snapshot_revision, inflight, queued) = {
        let state = world.resource::<NativeTools>();
        let tool = &state.tools[&index];
        (
            tool.snapshot.scene_domain.clone().unwrap(),
            tool.snapshot.revision,
            tool.inflight.is_some(),
            !tool.queue.is_empty(),
        )
    };
    if inflight {
        return;
    }
    if scenes
        .failed
        .as_ref()
        .is_some_and(|(failed, revision)| failed == &current && *revision == snapshot_revision)
    {
        return;
    }
    let domain = current_document(world);
    if domain.is_none() {
        spawn_request(world, scenes, current, SceneDomainCommand::Open, Kind::Initial);
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
            scenes,
            current,
            SceneDomainCommand::Saved {
                hash: native.descriptor.source_hash.clone(),
            },
            Kind::Sync,
        );
        return;
    }
    if wire.identity.as_ref() != Some(&current) {
        spawn_request(
            world,
            scenes,
            current,
            SceneDomainCommand::Adopt {
                document: native.descriptor.clone(),
            },
            Kind::Sync,
        );
        return;
    }
    if wire.edit_sequence != scenes.adopted_sequence
        && snapshot_revision != scenes.last_snapshot_revision
    {
        if wire
            .document
            .as_ref()
            .is_some_and(|d| d.hash != native.descriptor.hash)
        {
            let snapshot = world.resource::<NativeTools>().tools[&index]
                .snapshot
                .clone();
            spawn_author(world, scenes, current, snapshot);
            return;
        }
        scenes.adopted_sequence = wire.edit_sequence;
    }
    if wire
        .document
        .as_ref()
        .is_none_or(|d| d.hash != native.descriptor.hash)
    {
        spawn_request(
            world,
            scenes,
            current,
            SceneDomainCommand::Adopt {
                document: native.descriptor.clone(),
            },
            Kind::Sync,
        );
        return;
    }
    if let Some(input) = scenes.scene.take() {
        spawn_request(
            world,
            scenes,
            current,
            SceneDomainCommand::Scene { input },
            Kind::Author,
        );
        return;
    }
    if let Some(input) = scenes.inputs.pop_front() {
        spawn_request(
            world,
            scenes,
            current,
            SceneDomainCommand::Viewport { input },
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
            .scene_pending = false;
    }
}
pub(super) fn poll(world: &mut World) {
    world.resource_scope(|world, mut scenes: Mut<NativeScenes>| {
        step(world, &mut scenes);
        publish(world, &scenes);
    });
}
fn settled(world: &mut World) {
    world.resource_scope(|world, scenes: Mut<NativeScenes>| publish(world, &scenes));
}
fn publish(world: &mut World, scenes: &NativeScenes) {
    let Some(index) = scenes.index else {
        return;
    };
    let Some(identity) = identity(world, scenes) else {
        return;
    };
    let (wire, busy) = {
        let state = world.resource::<NativeTools>();
        let tool = &state.tools[&index];
        let Some(wire) = &tool.snapshot.scene_domain else {
            return;
        };
        (wire.clone(), tool.busy())
    };
    let target = GroundTarget::new(scenes.port.as_ref().unwrap().domain, &identity, &wire);
    let active = world.resource::<SceneEditorState>().tool == authoring_tool(scenes);
    let mut interaction = world.resource_mut::<SceneInteractionState>();
    interaction.enabled = true;
    interaction.observed_generation = identity.generation;
    interaction.work_pending = scenes.task.is_some() || busy || !scenes.inputs.is_empty();
    interaction.unfinished_shape = scenes.gesture.is_some();
    interaction.precise_cursor = active;
    interaction.cursor_target = active.then_some(target);
    interaction.camera_mouse_owned = active && scenes.gesture.is_some();
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
    let mut editor = world.resource_mut::<SceneEditorState>();
    editor.domain_brush_active = active && wire.continuous;
    if let Some(pointer) = &wire.pointer {
        editor.brush_radius = pointer.radius;
        editor.brush_spacing = pointer.spacing;
    }
}
fn authoring_tool(
    scenes: &NativeScenes,
) -> feathrise_editor_scene::game_authoring::scene_api::types::EditorTool {
    feathrise_editor_scene::game_authoring::scene_api::types::EditorTool::Domain(
        scenes.port.as_ref().map_or("", |p| p.domain),
    )
}
struct GroundTarget;
impl GroundTarget {
    fn new(domain:&'static str,identity:&SceneDomainIdentity,wire:&SceneDomainSnapshot)->feathrise_editor_scene::game_authoring::scene_api::viewport::vegetation_ground::GroundCursorTarget{
        feathrise_editor_scene::game_authoring::scene_api::viewport::vegetation_ground::GroundCursorTarget::Domain(DomainGroundTarget{domain,identity:format!("{identity:?}:{}:{:?}:{:?}:{}:{:?}",wire.tool,wire.selected,wire.layer,wire.selection_ticket,wire.pointer.as_ref().map(|p|&p.target)),continuous:wire.continuous,click_qualified:!wire.continuous,payload:Arc::new(identity.clone())})
    }
}
fn close_scene(world: &mut World, map: feathrise_scene_content::SceneId) {
    let mut scenes = world.resource_mut::<NativeScenes>();
    if let Some(identity) = scenes.known.remove(map.as_str()) {
        scenes.close.push_back(identity);
    }
    if scenes.scene_id.as_deref() == Some(map.as_str()) {
        scenes.scene_id = None;
        scenes.inputs.clear();
        scenes.gesture = None;
    }
}
fn recover(world: &mut World) -> bool {
    let mut scenes = world.resource_mut::<NativeScenes>();
    if scenes.task.is_some() {
        return false;
    }
    scenes.gesture = None;
    scenes.inputs.clear();
    scenes.last_snapshot_revision = 0;
    scenes.failed = None;
    true
}
fn start_history(world: &mut World, undoing: bool) -> Result<(), String> {
    world.resource_scope(|world, mut scenes: Mut<NativeScenes>| {
        if scenes.task.is_some() || scenes.gesture.is_some() {
            return Err("场景角色正在完成整笔操作".into());
        }
        let session = world.resource::<EditorAuthoringSession>();
        let command = if undoing {
            session.undo.last()
        } else {
            session.redo.last()
        }
        .ok_or("场景历史为空")?;
        let EditorCommand::DomainChanged {
            domain,
            before,
            after,
        } = command
        else {
            return Err("场景历史不属于原生领域".into());
        };
        if scenes.port.as_ref().is_none_or(|p| p.domain != *domain) {
            return Err("场景历史作者插件已不可用".into());
        }
        let value = if undoing { before } else { after };
        let descriptor = if let Some(history) = value.downcast_ref::<NativeHistory>() {
            history.descriptor.clone()
        } else if let Some(document) = value.downcast_ref::<NativeDocument>() {
            document.descriptor.clone()
        } else {
            return Err("场景历史宿主类型不匹配".into());
        };
        let expected = identity(world, &scenes).ok_or("场景已关闭")?;
        world.resource_mut::<EditorAuthoringSession>().saving = true;
        spawn_request(
            world,
            &mut scenes,
            expected,
            SceneDomainCommand::Adopt {
                document: descriptor,
            },
            Kind::History(undoing),
        );
        Ok(())
    })
}
pub(super) fn install(app: &mut App, project: &crate::project::Project) {
    visuals::install(app);
    app.insert_resource(NativeScenes::new(project)).init_resource::<SceneInteractionState>()
        .add_systems(Update,poll.after(super::systems::poll).in_set(SceneEditorSystemSet::Tasks).in_set(SceneInteractionAdoption))
        .add_systems(Update,viewport::input.in_set(game_authoring::DomainViewportInput))
        .add_systems(Update,settled.in_set(game_authoring::DomainInputSettled))
        .add_systems(Update,viewport::scene_menu.after(feathrise_editor_scene::game_authoring::scene_api::ui::scene_context_menu::SceneMenuDispatch).in_set(SceneEditorSystemSet::Input));
}

// 观测接口与外部验收扩展一起启用，保留扩展中的测试消费者。
#[cfg(all(feature = "validation", feathrise_external_validation))]
pub(super) fn validation_visuals(world: &World) -> Result<Option<serde_json::Value>, String> {
    visuals::validation_snapshot(world)
}

#[cfg(all(feature = "validation", feathrise_external_validation))]
pub(super) fn validation_state(world: &World) -> serde_json::Value {
    let Some(scenes) = world.get_resource::<NativeScenes>() else { return serde_json::Value::Null; };
    let current = identity(world, scenes);
    let wire = scenes.index.and_then(|index|world.resource::<NativeTools>().tools.get(&index))
        .and_then(|tool|tool.snapshot.scene_domain.as_ref());
    let ready = current.is_some() && wire.is_some_and(|wire| wire.identity == current)
        && scenes.task.is_none() && scenes.inputs.is_empty() && scenes.failed.is_none();
    serde_json::json!({"input_ready":ready,"current":current,"wire":wire.and_then(|wire|wire.identity.as_ref()),
        "task":scenes.task.is_some(),"queued_inputs":scenes.inputs.len(),"gesture":scenes.gesture.is_some(),
        "adopted_sequence":scenes.adopted_sequence,"last_snapshot_revision":scenes.last_snapshot_revision,
        "failed":scenes.failed})
}
