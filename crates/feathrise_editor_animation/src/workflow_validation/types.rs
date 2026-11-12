//! 动画原控件桥对有限验收暴露的轻量证据；不携带整份曲线、历史或作者文档。

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct KeyIdentity {
    pub clip: usize,
    pub channel: usize,
    pub key: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KeyProof {
    pub time: f32,
    pub value: Vec<f32>,
    pub in_tangent: Vec<f32>,
    pub out_tangent: Vec<f32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnimationProbe {
    pub key_identity: KeyIdentity,
    pub key: KeyProof,
    pub revision: u64,
    pub cursor: usize,
    pub saved_cursor: Option<usize>,
    pub history_len: usize,
    pub dirty: bool,
    pub actual_clip: usize,
    pub actual_channel: Option<usize>,
    pub actual_key: Option<usize>,
    pub position: f32,
    pub channel_selected: bool,
    pub key_selected: bool,
    pub save_receipt: Option<Result<(), String>>,
}

#[derive(Clone, Copy, Debug)]
pub enum AnimationCommand {
    NextKey,
    Undo,
    Redo,
    Save,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelProbe {
    pub generation: u64,
    pub root: String,
    pub mesh_entity: String,
    pub mesh_id: String,
    pub material_id: String,
    pub image_id: String,
    pub positions: Vec<[f32; 3]>,
    pub base_color_linear: [f32; 4],
    pub roughness: f32,
    pub image_size: [u32; 2],
    pub first_pixel: Vec<u8>,
}
