//! 动画/资产验收只持一份后台IO任务；原动画计算与保存仍由原作者单槽拥有。

use super::types::*;
use crate::ui::asset_preview::animation::workflow_validation::{AnimationProbe, ModelProbe};
use bevy::{ecs::message::MessageCursor, gltf::Gltf, prelude::*, tasks::Task};
use serde_json::Value;
use std::{sync::Arc, time::Instant};

#[derive(Resource, Default)]
pub(super) struct Fixture {
    pub phase: Phase,
    pub started: Option<Instant>,
    pub paths: Option<Paths>,
    pub io: Option<Task<Result<IoResult, String>>>,
    pub pulse: Option<Entity>,
    pub before: Option<AnimationProbe>,
    pub edited: Option<AnimationProbe>,
    pub undone: Option<AnimationProbe>,
    pub redone: Option<AnimationProbe>,
    pub saved: Option<AnimationProbe>,
    pub disk_before: String,
    pub disk_after: String,
    pub disk_key: Option<Value>,
    pub model_bytes: Option<Arc<Vec<u8>>>,
    pub texture_bytes: Option<Arc<Vec<u8>>>,
    pub model_before: Option<ModelProbe>,
    pub model_mesh: Option<ModelProbe>,
    pub model_material: Option<ModelProbe>,
    pub model_texture: Option<ModelProbe>,
    pub model_sha256: Vec<String>,
    pub texture_sha256: Option<String>,
    pub animation_handle: Option<Handle<Gltf>>,
    pub model_handle: Option<Handle<Gltf>>,
    pub mesh_handle: Option<Handle<Mesh>>,
    pub material_handle: Option<Handle<StandardMaterial>>,
    pub image_handle: Option<Handle<Image>>,
    pub gltf_events: MessageCursor<AssetEvent<Gltf>>,
    pub mesh_events: MessageCursor<AssetEvent<Mesh>>,
    pub material_events: MessageCursor<AssetEvent<StandardMaterial>>,
    pub image_events: MessageCursor<AssetEvent<Image>>,
    pub events: Events,
    pub animation_event_baseline: u64,
    pub event_baseline: Events,
    pub trace_phase: Option<Phase>,
    pub trace: Vec<Value>,
    pub pending_action: Option<(String, Value)>,
    pub report: Option<Value>,
    pub error: Option<String>,
}
