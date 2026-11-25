//! 工程3D通过已协商的增量预览通道接入演出草稿。完整文件仅按修订发送一次，游标只保留最新意图。
use super::{
    resources::{EmbeddedStatus, Outgoing, Runtime},
    systems::send,
};
use bevy::prelude::*;
use feathrise_editor_protocol::{Message, PreviewKind};
use feathrise_editor_scene::standalone::CinematicPreview;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
pub(super) const CAP: &str = "cinematic-preview-v1";
const HIGH: u64 = 1 << 63;
#[derive(Default)]
pub(super) struct Link {
    pub supported: bool,
    sequence: u64,
    flight: Option<(u64, Instant)>,
    scene: Option<String>,
    bytes: Option<Arc<Vec<u8>>>,
    time: Option<(u32, bool)>,
    failed: bool,
}
impl Link {
    pub fn ack(&mut self, sequence: u64) -> bool {
        if sequence & HIGH == 0 {
            return false;
        }
        if self.flight.is_some_and(|(i, _)| i == sequence) {
            self.flight = None;
        }
        true
    }
    pub fn fail(&mut self, sequence: u64) -> bool {
        if !self.flight.is_some_and(|(id, _)| id == sequence) {
            return false;
        }
        self.flight = None;
        self.failed = true;
        true
    }
}
pub(super) fn sync(world: &mut World, r: &mut Runtime, status: &mut EmbeddedStatus) {
    if !r.cinematic.supported
        || status.playing
        || r.waiting_mode.is_some()
        || r.snapshot_in_flight.is_some()
    {
        return;
    }
    let Some(scene) = r.scene_id.clone() else {
        return;
    };
    if r.cinematic.scene.as_ref() != Some(&scene) {
        r.cinematic = Link {
            supported: true,
            sequence: r.cinematic.sequence,
            scene: Some(scene.clone()),
            ..default()
        };
    }
    if let Some((_, at)) = r.cinematic.flight {
        if at.elapsed() > Duration::from_secs(30) {
            r.cinematic.flight = None;
            r.cinematic.failed = true;
            world.write_message(feathrise_editor_scene::standalone::SavedPlayFeedback(
                "工程未确认剧情演出，请退出试播后重试".into(),
            ));
        }
        return;
    }
    let desired = world
        .get_resource::<CinematicPreview>()
        .and_then(|s| s.active.as_ref());
    let (format, body, time) = if let Some(f) = desired {
        if r.cinematic.failed {
            return;
        }
        if r.cinematic
            .bytes
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &f.bytes))
        {
            ("cinematic-document-v1", f.bytes.clone(), None)
        } else if r.cinematic.time != Some((f.time.to_bits(), f.playing)) {
            let body = serde_json::to_vec(
                &serde_json::json!({"id":f.id,"time":f.time,"playing":f.playing}),
            )
            .expect("有限演出游标");
            (
                "cinematic-seek-v1",
                Arc::new(body),
                Some((f.time.to_bits(), f.playing)),
            )
        } else {
            return;
        }
    } else if r.cinematic.bytes.is_some() {
        ("cinematic-stop-v1", Arc::new(b"{}".to_vec()), None)
    } else {
        r.cinematic.failed = false;
        return;
    };
    let sequence = (r.cinematic.sequence.wrapping_add(1)) | HIGH;
    let message = Message::Preview {
        kind: PreviewKind::AuthorDomain,
        sequence,
        scene_id: scene,
        format: format.into(),
    };
    if send(
        r,
        Outgoing {
            generation: 0,
            message,
            payload: Some(body.clone()),
        },
        status,
    ) {
        r.cinematic.sequence = sequence;
        r.cinematic.flight = Some((sequence, Instant::now()));
        r.cinematic.time = time;
        if format == "cinematic-document-v1" {
            r.cinematic.bytes = Some(body);
        }
        if format == "cinematic-stop-v1" {
            r.cinematic.bytes = None;
            r.cinematic.failed = false;
        }
    }
}
