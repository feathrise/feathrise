//! 可编辑的 TRS 通道与共享源文件；源 JSON/BIN 只读共享，撤销不会复制模型二进制。

use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug)]
pub struct AnimationDocument {
    pub clips: Vec<Clip>,
    pub nodes: Vec<Node>,
    /// 不支持的通道保留在源文件中，界面必须向作者显示具体原因。
    pub warnings: Vec<String>,
    pub read_only: bool,
    /// 同一作者来源的编辑与保存共享身份；重新解析即换代，节点索引不能跨代复用。
    pub source_identity: Arc<()>,
    pub(super) source: Arc<Source>,
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub channels: Vec<Channel>,
    /// GLB 原片段索引，目录选择和写回都依赖这个身份。
    pub source_index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub node: usize,
    pub path: String,
    pub node_name: String,
    pub interpolation: String,
    pub keys: Vec<Keyframe>,
    /// 原通道序号不随界面排序变化，未知通道、共享 sampler 及其他片段因此保持完整。
    pub(super) source_index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    pub time: f32,
    pub value: Vec<f32>,
    /// glTF 三次样条的切线单位为每秒导数，移动关键帧不得按帧率缩放这些值。
    pub in_tangent: Vec<f32>,
    pub out_tangent: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Debug)]
pub(super) struct Source {
    pub path: PathBuf,
    pub bytes: Arc<[u8]>,
    pub json: Value,
    pub buffers: Vec<Arc<[u8]>>,
    pub chunks: Vec<Chunk>,
    pub original_clips: Vec<Clip>,
}

#[derive(Debug)]
pub(super) struct Chunk {
    pub kind: u32,
    pub bytes: Arc<[u8]>,
}

impl Clip {
    pub fn duration(&self) -> f32 {
        self.channels
            .iter()
            .filter_map(|channel| channel.keys.last())
            .map(|key| key.time)
            .fold(0.0, f32::max)
    }
}
