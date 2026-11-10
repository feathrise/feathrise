//! 实时作者结果的可靠投递。固定四个预览域共用时钟，完整快照等待已发消息而不等待未发作者工作。
//! 四域共享一个正文在途槽并轮转，防止大像素副本叠加；Begin/End只有收到真实采用回执才出队。

use super::resources::{EmbeddedStatus, Outgoing, Runtime};
use crate::viewport_mode::ViewportMode;
use bevy::prelude::*;
use feathrise_editor_protocol::{Message, PreviewKind};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct PreviewFlights {
    identity: Option<(u64, u64)>,
    flights: [Option<Flight>; 4],
    next_domain: usize,
}
struct Flight {
    sequence: u64,
    started: Instant,
}
impl PreviewFlights {
    fn slot(&mut self, kind: PreviewKind) -> &mut Option<Flight> {
        &mut self.flights[domain_index(kind)]
    }
    pub(super) fn empty(&self) -> bool {
        self.flights.iter().all(Option::is_none)
    }
}

fn domain_index(kind: PreviewKind) -> usize {
    match kind {
        PreviewKind::Placement => 0,
        PreviewKind::Vegetation => 1,
        PreviewKind::AuthorDomain => 2,
        PreviewKind::SurfaceTrail => 3,
    }
}

pub(super) fn configure(
    world: &mut World,
    runtime: &mut Runtime,
    status: &EmbeddedStatus,
    selected: ViewportMode,
) {
    let identity = runtime
        .identity
        .as_ref()
        .map(|id| (id.session, id.generation));
    if runtime.previews.identity != identity {
        runtime.previews = PreviewFlights {
            identity,
            ..default()
        };
    }
    let enabled = selected == ViewportMode::EmbeddedWorld
        && status.ready
        && !runtime.exit_requested
        && runtime
            .active
            .as_ref()
            .is_some_and(|active| !active.control.is_cancelled());
    let (session, generation) = identity.unwrap_or_default();
    super::preview_sources::configure(world, enabled, session, generation);
    if !enabled {
        runtime.previews = PreviewFlights {
            identity,
            ..default()
        };
    }
}

pub(super) fn sync(world: &mut World, runtime: &mut Runtime, status: &mut EmbeddedStatus) {
    // 切换场景仅由完整快照推进；同一帧先更新身份，不能把上个场景手势带入新代次。
    configure(world, runtime, status, ViewportMode::EmbeddedWorld);
    if let Some(error) = super::preview_sources::error(world) {
        let error = format!("实时作者预览未采用：{error}；草稿与历史保留，修正后点击“预览”重新连接");
        if status.error.as_ref() != Some(&error) {
            world.write_message(feathrise_editor_scene::standalone::SavedPlayFeedback(
                error.clone(),
            ));
            world
                .resource_mut::<crate::viewport_mode::ViewportModeState>()
                .status = error.clone();
            status.error = Some(error);
        }
        return;
    }
    if runtime
        .previews
        .flights
        .iter()
        .flatten()
        .any(|flight| flight.started.elapsed() > Duration::from_secs(30))
    {
        status.error = Some("运行实例未确认实时作者结果，正在重新收口；原作者草稿和历史保留".into());
        // 四个有界通道的身份即可定位互等；不序列化场景正文或扫描全场景。
        eprintln!("实时预览确认超时：snapshot_revision={} flights={:?}",
            runtime.sent_revision,
            runtime.previews.flights.iter().enumerate().map(|(domain, flight)|
                (domain, flight.as_ref().map(|flight| flight.sequence))).collect::<Vec<_>>());
        if let Some(active) = &runtime.active {
            active.control.cancel();
        }
        return;
    }
    if status.playing
        || runtime.waiting_mode.is_some()
        || status.error.is_some()
        || runtime.snapshot_in_flight.is_some()
        || !runtime.previews.empty()
    {
        return;
    }
    let Some((session, generation)) = runtime.previews.identity else {
        return;
    };
    // 每帧至多四个Arc引用；轮转防止连续刷草让轨迹/其他原预览饿死。
    let mut offers = super::preview_sources::offers(world);
    let start = offers
        .iter()
        .position(|ready| domain_index(ready.kind) >= runtime.previews.next_domain)
        .unwrap_or(0);
    offers.rotate_left(start);
    for ready in offers {
        if runtime.previews.slot(ready.kind).is_some()
            || (ready.session, ready.generation) != (session, generation)
            || runtime.scene_id.as_deref() != Some(ready.scene_id.as_str())
        {
            continue;
        }
        if super::systems::send(
            runtime,
            Outgoing {
                generation,
                message: Message::Preview {
                    kind: ready.kind,
                    sequence: ready.sequence,
                    scene_id: ready.scene_id,
                    format: ready.format.into(),
                },
                payload: Some(ready.bytes),
            },
            status,
        ) {
            *runtime.previews.slot(ready.kind) = Some(Flight {
                sequence: ready.sequence,
                started: Instant::now(),
            });
            runtime.previews.next_domain = (domain_index(ready.kind) + 1) % 4;
            break;
        }
    }
}

/// 调用前外层已验证项目、会话和场景代次；域内仍需精确匹配唯一在途序号。
pub(super) fn acknowledge(
    world: &mut World,
    runtime: &mut Runtime,
    kind: PreviewKind,
    sequence: u64,
) {
    let flight = runtime.previews.slot(kind);
    if !flight
        .as_ref()
        .is_some_and(|flight| flight.sequence == sequence)
    {
        return;
    }
    super::preview_sources::acknowledge(world, kind, sequence);
    *flight = None;
}

pub(super) fn settled(world: &World, runtime: &Runtime) -> bool {
    runtime.previews.empty() && super::preview_sources::settled(world)
}

/// 验收只读取固定四个通道的控制状态，不遍历场景或复制预览正文。
#[cfg(feature = "validation")]
pub(super) fn diagnostic(world: &World) -> serde_json::Value {
    use serde_json::json;
    let Some(runtime) = world.get_non_send::<Runtime>() else {
        return json!(null);
    };
    let flights: Vec<_> = runtime
        .previews
        .flights
        .iter()
        .enumerate()
        .map(|(domain, flight)| {
            json!({"domain":domain,"sequence":flight.as_ref().map(|value|value.sequence),
            "age_ms":flight.as_ref().map(|value|value.started.elapsed().as_millis())})
        })
        .collect();
    json!({"snapshot_in_flight":runtime.snapshot_in_flight,"sent_revision":runtime.sent_revision,
        "waiting_mode":format!("{:?}",runtime.waiting_mode),"flights":flights,
        "sources":super::preview_sources::diagnostic(world)})
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_editor_scene::standalone::embedded_vegetation::EmbeddedVegetationState;

    #[test]
    fn embedded_preview_ack_cannot_release_another_domain_or_sequence() {
        let mut world = World::new();
        world.init_resource::<EmbeddedVegetationState>();
        let mut runtime = Runtime::default();
        *runtime.previews.slot(PreviewKind::Vegetation) = Some(Flight {
            sequence: 4,
            started: Instant::now(),
        });
        acknowledge(&mut world, &mut runtime, PreviewKind::Placement, 4);
        acknowledge(&mut world, &mut runtime, PreviewKind::AuthorDomain, 4);
        acknowledge(&mut world, &mut runtime, PreviewKind::SurfaceTrail, 4);
        acknowledge(&mut world, &mut runtime, PreviewKind::Vegetation, 3);
        assert!(runtime.previews.slot(PreviewKind::Vegetation).is_some());
        assert_eq!(
            world
                .resource::<EmbeddedVegetationState>()
                .acknowledged_sequence,
            0
        );
        acknowledge(&mut world, &mut runtime, PreviewKind::Vegetation, 4);
        assert!(runtime.previews.empty());
        assert_eq!(
            world
                .resource::<EmbeddedVegetationState>()
                .acknowledged_sequence,
            4
        );
    }
}
