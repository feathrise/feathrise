//! 自有资产路径、后台结果与有限验收阶段；正式工程资产不承担这些测试容量限制。

use serde::Serialize;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

pub(super) const ANIMATION: &str = "scene/animation.glb";
pub(super) const MODEL: &str = "scene/probe.glb";
pub(super) const TEXTURE: &str = "scene/albedo.png";
pub(super) const COLOR: [f32; 4] = [0.35, 0.8, 0.6, 1.0];
pub(super) const PIXEL: [u8; 4] = [41, 119, 227, 255];

#[derive(Clone)]
pub(super) struct Paths {
    pub project: PathBuf,
    pub assets: PathBuf,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Idle,
    Preparing,
    OpenAnimation,
    SelectChannel,
    LocateKey,
    Edit,
    Undo,
    Redo,
    Save,
    ReadSaved,
    OpenModel,
    MeshWrite,
    MeshReload,
    MaterialWrite,
    MaterialReload,
    TextureWrite,
    TextureReload,
    ReturnScene,
    Complete,
    Failed,
}

#[derive(Clone, Copy)]
pub(super) enum Mutation {
    Mesh,
    Material,
    Texture,
}

pub(super) struct Prepared {
    pub animation_sha256: String,
    pub model: Arc<Vec<u8>>,
    pub texture: Arc<Vec<u8>>,
}

pub(super) enum IoResult {
    Prepared(Prepared),
    Written { bytes: Arc<Vec<u8>>, sha256: String },
    SavedAnimation { sha256: String, key: Value },
}

#[derive(Clone, Default, Serialize)]
pub(super) struct Events {
    pub animation: u64,
    pub gltf: u64,
    pub mesh: u64,
    pub material: u64,
    pub image: u64,
}
