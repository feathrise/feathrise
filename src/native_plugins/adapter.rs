//! 将异步插件保存回执接入既有保存全部、退出与F12事务；不以“已发送”冒充落盘。
use super::resources::NativeTools;
use bevy::prelude::*;
use feathrise_editor_map::standalone::{
    ProjectAuthorAdapter, ProjectAuthorAdapters, ProjectAuthorDocument, ProjectAuthorSnapshot,
};
use feathrise_plugin_api::Request;

pub(super) fn install(app: &mut App) {
    app.init_resource::<ProjectAuthorAdapters>();
    app.world_mut()
        .resource_mut::<ProjectAuthorAdapters>()
        .register(ProjectAuthorAdapter {
            domain: "native.plugins",
            order: 20,
            collect,
            saved_play_blocker: blocker,
            request,
            poll,
            catalog_hash: |_, _| None,
            rebase_own_write: |_, _, _| {},
        })
        .expect("原生插件聚合保存域只安装一次");
}
fn identity(index: usize, document: u32) -> u64 {
    ((index as u64 + 1) << 32) | u64::from(document)
}
fn decode(id: u64) -> (usize, u32) {
    (((id >> 32).saturating_sub(1)) as usize, id as u32)
}
fn collect(world: &World) -> ProjectAuthorSnapshot {
    let state = world.resource::<NativeTools>();
    let mut result = ProjectAuthorSnapshot::default();
    if state.discovery_pending || state.loading > 0 || state.tools.values().any(|t| t.busy()) {
        result.pending = Some("原生插件正在加载或处理作者操作".into());
    }
    if state.input_pending {
        result.blocked = Some("请先提交或取消插件字段输入".into());
    }
    for (&index, tool) in &state.tools {
        for tab in tool.snapshot.tabs.iter().filter(|tab| tab.dirty) {
            result.unsaved.push(ProjectAuthorDocument {
                id: identity(index, tab.id),
                label: format!("{}：{}", tool.snapshot.tool, tab.title),
            });
            if let Some(error) = &tool.fault {
                result.blocked = Some(error.clone());
            }
        }
    }
    result
}
fn blocker(world: &World) -> Option<String> {
    let state = world.resource::<NativeTools>();
    let snapshot = collect(world);
    snapshot
        .pending
        .or(snapshot.blocked)
        .or_else(|| state.errors.first().cloned())
        .or_else(|| state.tools.values().find_map(|tool| tool.fault.clone()))
}
fn request(world: &mut World, id: u64) -> Result<(), String> {
    let (index, document) = decode(id);
    let mut state = world.resource_mut::<NativeTools>();
    let tool = state.tools.get_mut(&index).ok_or("插件已关闭")?;
    let before = tool
        .snapshot
        .tabs
        .iter()
        .find(|t| t.id == document)
        .ok_or("插件文档已关闭")?
        .saved_sequence;
    let sequence = tool.send(Request::Save { document }, false)?;
    state.save_tickets.insert(id, (sequence, before));
    Ok(())
}
fn poll(world: &World, id: u64) -> Result<bool, String> {
    let (index, document) = decode(id);
    let state = world.resource::<NativeTools>();
    let tool = state.tools.get(&index).ok_or("插件已关闭")?;
    if let Some(error) = &tool.fault {
        return Err(error.clone());
    }
    let &(sequence, before) = state.save_tickets.get(&id).ok_or("插件保存请求身份丢失")?;
    if tool.completed < sequence {
        return Ok(false);
    }
    let tab = tool
        .snapshot
        .tabs
        .iter()
        .find(|t| t.id == document)
        .ok_or("保存期间插件文档关闭")?;
    if tab.saved_sequence > before && !tab.dirty {
        return Ok(true);
    }
    Err(tool
        .snapshot
        .error
        .clone()
        .unwrap_or_else(|| "插件没有返回成功保存回执，草稿保留".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_plugins::resources::Tool;
    use feathrise_plugin_api::{Document, Snapshot};

    fn world() -> World {
        let (_, receiver) = async_channel::bounded(1);
        let (requests, _) = async_channel::bounded(1);
        let tool = Tool {
            domain: "fixture",
            snapshot: Snapshot {
                plugin_id: "fixture".into(),
                tool: "工具".into(),
                tabs: vec![Document {
                    id: 1,
                    title: "草稿".into(),
                    dirty: true,
                    saved_sequence: 2,
                }],
                ..Default::default()
            },
            requests,
            queue: Default::default(),
            inflight: None,
            completed: 0,
            sequence: 0,
            fault: None,
            library_open: false,
            close_confirmation: None,
            choices: None,
            latest_activation: 0,
            map_pending: false,
        };
        let mut world = World::new();
        world.insert_resource(NativeTools {
            tools: [(0, tool)].into(),
            receiver,
            discovery_pending: false,
            loading: 0,
            errors: Vec::new(),
            management_errors: Default::default(),
            load_states: Default::default(),
            load_origins: Default::default(),
            ui_revision: 1,
            diagnostics: Default::default(),
            input_pending: false,
            save_tickets: Default::default(),
        });
        world
    }
    #[test]
    fn save_all_requires_new_success_receipt_and_keeps_failed_draft() {
        let mut world = world();
        let id = identity(0, 1);
        assert_eq!(collect(&world).unsaved[0].id, id);
        request(&mut world, id).unwrap();
        assert!(!poll(&world, id).unwrap());
        {
            let mut state = world.resource_mut::<NativeTools>();
            let tool = state.tools.get_mut(&0).unwrap();
            tool.queue.clear();
            tool.completed = 1;
            tool.snapshot.error = Some("磁盘版本冲突".into());
        }
        assert_eq!(poll(&world, id).unwrap_err(), "磁盘版本冲突");
        assert_eq!(collect(&world).unsaved.len(), 1);
        request(&mut world, id).unwrap();
        assert!(!poll(&world, id).unwrap());
        {
            let mut state = world.resource_mut::<NativeTools>();
            let tool = state.tools.get_mut(&0).unwrap();
            tool.queue.clear();
            tool.completed = 2;
            tool.snapshot.error = None;
            tool.snapshot.tabs[0].dirty = false;
        }
        assert!(poll(&world, id).is_err(), "旧clean快照不能冒充新保存");
        world
            .resource_mut::<NativeTools>()
            .tools
            .get_mut(&0)
            .unwrap()
            .snapshot
            .tabs[0]
            .saved_sequence = 3;
        assert!(poll(&world, id).unwrap());
        assert!(collect(&world).unsaved.is_empty());
    }
    #[test]
    fn load_input_and_worker_failure_gate_saved_play() {
        let mut world = world();
        world.resource_mut::<NativeTools>().loading = 1;
        assert!(blocker(&world).is_some());
        world.resource_mut::<NativeTools>().loading = 0;
        world.resource_mut::<NativeTools>().input_pending = true;
        assert!(collect(&world).blocked.is_some());
        world.resource_mut::<NativeTools>().input_pending = false;
        world
            .resource_mut::<NativeTools>()
            .tools
            .get_mut(&0)
            .unwrap()
            .fault = Some("插件故障".into());
        assert_eq!(blocker(&world).as_deref(), Some("插件故障"));
        assert!(request(&mut world, identity(0, 1)).is_err());
        assert_eq!(collect(&world).unsaved.len(), 1);
    }

    #[test]
    fn rejected_field_cancels_following_save_without_hanging_transaction() {
        let mut world = world();
        world
            .resource_mut::<NativeTools>()
            .tools
            .get_mut(&0)
            .unwrap()
            .send(
                Request::Edit {
                    document: 1,
                    field: "name".into(),
                    value: String::new(),
                },
                false,
            )
            .unwrap();
        let id = identity(0, 1);
        request(&mut world, id).unwrap();
        let mut state = world.resource_mut::<NativeTools>();
        let tool = state.tools.get_mut(&0).unwrap();
        tool.snapshot.error = Some("字段被拒绝".into());
        tool.cancel_queued();
        assert!(!tool.busy());
        assert_eq!(poll(&world, id).unwrap_err(), "字段被拒绝");
        assert_eq!(collect(&world).unsaved.len(), 1);
    }
}
