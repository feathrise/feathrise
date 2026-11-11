//! 宿主只桥接工具语义；最新完整集合入队后才推进发送代次，拥塞时保留待发送值。

use super::resources::{EmbeddedStatus, Outgoing, Runtime};
use bevy::prelude::*;
use feathrise_editor_protocol::Message;
use feathrise_editor_scene::standalone::embedded_tools::EmbeddedAuthorTools;

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::resources::Active;
    use feathrise_editor_protocol::{AuthorTool, AuthorToolPreview, Mode, SessionIdentity, TransformPreview};
    use std::{sync::mpsc, thread};

    fn fixture() -> (World, Runtime, EmbeddedStatus, mpsc::Receiver<Outgoing>) {
        // 使用生产Active的一格有界控制邮箱；测试不启动构建、子进程或管道线程。
        let (sender, commands) = mpsc::sync_channel(1);
        let (_, receiver) = mpsc::sync_channel(1);
        let (_, result) = mpsc::sync_channel(1);
        let mut runtime = Runtime::default();
        runtime.identity = Some(SessionIdentity { project_id: "tools-fixture".into(), session: 8, generation: 3 });
        runtime.scene_id = Some("map-a".into());
        runtime.active = Some(Active { control: default(), sender, receiver, result, thread: thread::spawn(|| {}) });
        let mut world = World::new();
        world.insert_resource(EmbeddedAuthorTools {
            revision: 40, scene_id: Some("map-a".into()), generation: 9,
            preview: AuthorToolPreview { items: vec![AuthorTool::Transform(TransformPreview::Move {
                spatial: false, origin: [1., 2., 3.], length: 2., axes: 1, active_axes: 1,
                plane: false, plane_active: false, plane_cross: false,
                plane_color: [1.; 4], color_active: [1.; 4], axis_colors: [[1.; 4]; 3],
                anchor: None, marker_radius: None,
            })] }, error: None,
        });
        (world, runtime, EmbeddedStatus { applied_revision: 7, ..default() }, commands)
    }

    #[test]
    fn live_move_and_empty_clear_bind_to_applied_scene_revision() {
        let (mut world, mut runtime, mut status, commands) = fixture();
        let expected = world.resource::<EmbeddedAuthorTools>().preview.clone();
        synchronize(&mut world, &mut runtime, &mut status);
        let move_message = commands.try_recv().unwrap();
        assert_eq!(move_message.generation, 3, "保持本次会话场景代次");
        assert_eq!(move_message.message, Message::SetAuthorTools { revision: 7, preview: expected });
        assert!(move_message.payload.is_none());
        assert_eq!(runtime.tools_generation, 9);

        // 草稿继续推进且尚未产生完整快照，离开工具仍须立即清除已显示的旧手柄。
        {
            let mut tools = world.resource_mut::<EmbeddedAuthorTools>();
            tools.revision = 41;
            tools.generation = 10;
            tools.preview.items.clear();
        }
        synchronize(&mut world, &mut runtime, &mut status);
        assert_eq!(commands.try_recv().unwrap().message,
            Message::SetAuthorTools { revision: 7, preview: AuthorToolPreview::default() });
        assert_eq!(runtime.tools_generation, 10);
        synchronize(&mut world, &mut runtime, &mut status);
        assert!(matches!(commands.try_recv(), Err(mpsc::TryRecvError::Empty)));
        runtime.active.take().unwrap().thread.join().unwrap();
    }

    #[test]
    fn full_queue_keeps_latest_tools_retryable_and_foreign_or_busy_modes_send_nothing() {
        let (mut world, mut runtime, mut status, commands) = fixture();
        assert!(runtime.active.as_ref().unwrap().sender.try_send(Outgoing::control(Message::Stop)).is_ok());
        synchronize(&mut world, &mut runtime, &mut status);
        assert_eq!(runtime.tools_generation, 0, "拥塞不等于工具已发送");
        {
            let mut tools = world.resource_mut::<EmbeddedAuthorTools>();
            tools.generation = 10;
            tools.revision = 41;
            tools.preview.items.clear();
        }
        assert!(matches!(commands.try_recv().unwrap().message, Message::Stop));
        synchronize(&mut world, &mut runtime, &mut status);
        assert_eq!(commands.try_recv().unwrap().message,
            Message::SetAuthorTools { revision: 7, preview: AuthorToolPreview::default() });
        assert_eq!(runtime.tools_generation, 10, "队列恢复后发送最新完整集合");

        for gate in 0..6 {
            runtime.tools_generation = 0;
            runtime.waiting_mode = None;
            runtime.snapshot_in_flight = None;
            status.playing = false;
            status.applied_revision = 7;
            world.resource_mut::<EmbeddedAuthorTools>().scene_id = Some("map-a".into());
            match gate {
                0 => world.resource_mut::<EmbeddedAuthorTools>().scene_id = Some("map-b".into()),
                1 => status.playing = true,
                2 => runtime.snapshot_in_flight = Some(8),
                3 => runtime.waiting_mode = Some((1, Mode::Playing)),
                4 => status.applied_revision = 0,
                _ => world.resource_mut::<EmbeddedAuthorTools>().scene_id = None,
            }
            synchronize(&mut world, &mut runtime, &mut status);
            assert_eq!(runtime.tools_generation, 0, "拒绝门{gate}不能消费工具代次");
            assert!(matches!(commands.try_recv(), Err(mpsc::TryRecvError::Empty)), "拒绝门{gate}不能发送旧图工具");
        }
        runtime.active.take().unwrap().thread.join().unwrap();
    }
}

pub(super) fn synchronize(world: &mut World, runtime: &mut Runtime, status: &mut EmbeddedStatus) {
    if status.playing || runtime.waiting_mode.is_some() || runtime.snapshot_in_flight.is_some() {
        return;
    }
    let tools = world.resource::<EmbeddedAuthorTools>();
    if tools.generation == runtime.tools_generation || status.applied_revision == 0
        || tools.scene_id.is_none()
        || tools.scene_id.as_deref() != runtime.scene_id.as_deref() {
        return;
    }
    let generation = tools.generation;
    let error = tools.error.clone();
    // 连续拖动会先推进草稿代次，完整快照要到松手才发送；工具仍附着同图已采用场景。
    // 贴地高度另由稳定来源身份核验，切换场景则必须等待对应完整快照正式采用。
    let message = Message::SetAuthorTools { revision: status.applied_revision, preview: tools.preview.clone() };
    if super::systems::send(runtime, Outgoing::control(message), status) {
        runtime.tools_generation = generation;
        if let Some(error) = error { super::systems::feedback(world, &error); }
    }
}
