//! 工程3D状态投影回归：动画依赖真实时间，阶段依赖真实生命周期，不发布重复操作反馈。

use super::{ViewportMode, ViewportModeState, components::{ConnectionBorderSegment, ConnectionStatus, LaunchStatus, ModeStatus}, ui::present_connection};
use bevy::prelude::*;
use bevy::ecs::system::RunSystemOnce;
use crate::embedded_viewport::EmbeddedStatus;
use feathrise_editor_core::ProjectLaunchSettings;
use feathrise_editor_map::standalone::{SavedPlayFeedback, embedded_snapshot::EmbeddedSnapshotState};
use feathrise_editor_ui::feedback::{PanelStatusPhase, PanelStatusState};

fn fixture() -> (World, Entity, Entity) {
    let mut world = World::new();
    world.init_resource::<ViewportModeState>();
    world.resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
    world.init_resource::<EmbeddedStatus>();
    world.init_resource::<EmbeddedSnapshotState>();
    world.init_resource::<ProjectLaunchSettings>();
    world.init_resource::<Time<Real>>();
    world.init_resource::<Messages<SavedPlayFeedback>>();
    let spinner = world.spawn((ConnectionStatus, Node {
        position_type: PositionType::Absolute, ..default()
    }, ComputedNode { size: Vec2::new(88.0, 26.0), inverse_scale_factor: 1.0, ..default() })).id();
    world.spawn((ConnectionBorderSegment(0), Node::default(), UiTransform::default(), ChildOf(spinner)));
    let status = world.spawn((ModeStatus, PanelStatusState::default())).id();
    (world, spinner, status)
}

#[test]
fn busy_border_moves_with_real_time_without_republishing_feedback() {
    let (mut world, spinner, status) = fixture();
    world.resource_mut::<EmbeddedStatus>().active = true;
    present_connection(&mut world);
    assert_eq!(world.get::<Node>(spinner).unwrap().display, Display::Flex);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().summary, "工程3D：启动中");
    let segment = world.get::<Children>(spinner).unwrap()[0];
    world.run_system_once(super::busy_border::animate).unwrap();
    let first = world.get::<UiTransform>(segment).unwrap().translation;
    world.resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_millis(150));
    present_connection(&mut world);
    world.run_system_once(super::busy_border::animate).unwrap();
    assert_ne!(world.get::<UiTransform>(segment).unwrap().translation, first);
    assert_eq!(world.get::<Node>(spinner).unwrap().position_type, PositionType::Absolute);
    assert!(world.get::<Text>(spinner).is_none(), "忙碌只画在边框层，不添加状态文字或图标槽");
    assert!(world.resource::<Messages<SavedPlayFeedback>>().is_empty());

    world.resource_mut::<EmbeddedStatus>().ready = true;
    {
        let mut capture = world.resource_mut::<EmbeddedSnapshotState>();
        capture.map_id = Some("MAP-awaiting-current-snapshot".into());
        capture.pending = true;
    }
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().summary, "工程3D：同步中");
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().phase, PanelStatusPhase::Working);
}

#[test]
fn idle_failure_restart_and_saved_mode_use_bottom_projection() {
    let (mut world, spinner, status) = fixture();
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().summary, "工程3D：未连接");
    assert_eq!(world.get::<Node>(spinner).unwrap().display, Display::None);
    {
        let mut runtime = world.resource_mut::<EmbeddedStatus>();
        runtime.active = true;
        runtime.ready = true; // 从未采用地图的空工程按原生命周期合同连接。
    }
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().summary, "工程3D：已连接");
    assert_eq!(world.get::<Node>(spinner).unwrap().display, Display::None);
    world.resource_mut::<EmbeddedStatus>().error = Some("实际构建失败".into());
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().phase, PanelStatusPhase::Failed);
    assert_eq!(world.get::<Node>(spinner).unwrap().display, Display::None);
    world.resource_mut::<EmbeddedStatus>().error = None;
    world.resource_mut::<ProjectLaunchSettings>().switching_profile = true;
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().summary, "工程3D：重启中");
    assert_eq!(world.get::<Node>(spinner).unwrap().display, Display::Flex);

    world.resource_mut::<ViewportModeState>().selected = ViewportMode::SavedWorld;
    present_connection(&mut world);
    assert_eq!(*world.get::<PanelStatusState>(status).unwrap(), PanelStatusState::default());
    assert_eq!(world.get::<Node>(spinner).unwrap().display, Display::None);
}

#[test]
fn modal_occlusion_preserves_connection_but_never_confirms_a_new_snapshot_revision() {
    use feathrise_editor_map::standalone::embedded_snapshot::EmbeddedSnapshot;
    use std::sync::Arc;
    let (mut world, spinner, status) = fixture();
    world.insert_resource(feathrise_editor_core::EditorHostOverlay { active: true });
    world.insert_resource(super::ViewportGeometry { visible: false, focused: false, ..default() });
    {
        let mut runtime = world.resource_mut::<EmbeddedStatus>();
        runtime.active = true;
        runtime.ready = true;
        runtime.applied_revision = 7;
    }
    {
        let mut capture = world.resource_mut::<EmbeddedSnapshotState>();
        capture.enabled = true;
        capture.map_id = Some("MAP-modal-connection".into());
        capture.revision = 7;
        capture.ready = Some(Arc::new(EmbeddedSnapshot {
            map_id: "MAP-modal-connection".into(), author_revision: 7,
            bytes: Default::default(), pose_targets: Default::default(),
        }));
    }
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().summary, "工程3D：已连接");
    assert_eq!(world.get::<Node>(spinner).unwrap().display, Display::None);
    {
        let mut capture = world.resource_mut::<EmbeddedSnapshotState>();
        capture.revision = 8;
        capture.pending = true;
    }
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().summary, "工程3D：同步中");
    world.resource_mut::<feathrise_editor_core::EditorHostOverlay>().active = false;
    world.resource_mut::<EmbeddedSnapshotState>().pending = false;
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().phase, PanelStatusPhase::Working,
        "取消遮挡和清除pending都不能用旧修订7冒充当前8已采用");
    world.resource_mut::<EmbeddedSnapshotState>().ready = Some(Arc::new(EmbeddedSnapshot {
        map_id: "MAP-modal-connection".into(), author_revision: 8,
        bytes: Default::default(), pose_targets: Default::default(),
    }));
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().phase, PanelStatusPhase::Working,
        "完整编码完成仍须等待工程采用回执");
    world.resource_mut::<EmbeddedStatus>().applied_revision = 8;
    present_connection(&mut world);
    assert_eq!(world.get::<PanelStatusState>(status).unwrap().summary, "工程3D：已连接");
    assert_eq!(world.get::<Node>(spinner).unwrap().display, Display::None);
    assert!(world.resource::<Messages<SavedPlayFeedback>>().is_empty());
}

#[test]
fn launch_border_is_mounted_once_with_sixteen_segments_and_no_layout_slot() {
    let mut app = App::new();
    app.add_systems(Update, super::ui::mount_launch);
    let button = app.world_mut().spawn((feathrise_editor_core::ProjectLaunchButton, Node { width: px(64), height: px(26), ..default() })).id();
    app.update();
    app.update();
    let world = app.world_mut();
    let borders = world.query_filtered::<(Entity, &Node, &ChildOf), With<LaunchStatus>>().iter(world).collect::<Vec<_>>();
    assert_eq!(borders.len(), 1);
    let (border, node, parent) = borders[0];
    assert_eq!(parent.parent(), button);
    assert_eq!(node.position_type, PositionType::Absolute);
    assert_eq!(node.display, Display::None);
    assert_eq!(world.get::<Children>(border).unwrap().len(), 16);
    assert_eq!(world.get::<Node>(button).unwrap().width, px(64));
}

#[test]
fn launch_border_uses_preparing_state_and_stops_while_the_same_run_remains_active() {
    let (mut world, _, _) = fixture();
    world.resource_mut::<ViewportModeState>().selected = ViewportMode::SavedWorld;
    world.insert_resource(crate::native_play::NativePlayStatus { active: true, preparing: true, ..default() });
    let border = world.spawn((LaunchStatus, Node { position_type: PositionType::Absolute, ..default() },
        ComputedNode { size: Vec2::new(64.0, 26.0), inverse_scale_factor: 1.0, ..default() })).id();
    let segment = world.spawn((ConnectionBorderSegment(0), Node::default(), UiTransform::default(), ChildOf(border))).id();
    world.run_system_once(super::ui::present_launch).unwrap();
    assert_eq!(world.get::<Node>(border).unwrap().display, Display::Flex);
    world.run_system_once(super::busy_border::animate).unwrap();
    let first = world.get::<UiTransform>(segment).unwrap().translation;
    world.resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_millis(150));
    world.run_system_once(super::busy_border::animate).unwrap();
    assert_ne!(world.get::<UiTransform>(segment).unwrap().translation, first);
    world.resource_mut::<crate::native_play::NativePlayStatus>().preparing = false;
    world.run_system_once(super::ui::present_launch).unwrap();
    assert!(world.resource::<crate::native_play::NativePlayStatus>().active);
    assert_eq!(world.get::<Node>(border).unwrap().display, Display::None);
    world.resource_mut::<crate::native_play::NativePlayStatus>().preparing = true;
    world.resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
    world.run_system_once(super::ui::present_launch).unwrap();
    assert_eq!(world.get::<Node>(border).unwrap().display, Display::None, "预览不借用普通启动的准备状态");
    assert!(world.resource::<Messages<SavedPlayFeedback>>().is_empty());
}
