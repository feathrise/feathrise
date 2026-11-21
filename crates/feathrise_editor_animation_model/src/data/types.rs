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
    /// 初次载入时已有的片段，即使撤销本会话添加的动作也必须保留。
    pub(super) preserved_clip_count: usize,
    /// 记录初次读取的原始通道数，包含作者不支持的通道；保存后仍保留撤销新增轨道的边界。
    pub(super) preserved_channel_counts: Vec<usize>,
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

/// 节点沿用 glTF 索引，父级指向同一骨架列表中的位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewJoint {
    pub node: usize,
    pub parent: Option<usize>,
}

#[derive(Debug)]
pub(super) struct Source {
    pub path: PathBuf,
    pub bytes: Arc<[u8]>,
    pub json: Value,
    pub buffers: Vec<Arc<[u8]>>,
    pub chunks: Vec<Chunk>,
    pub original_clips: Vec<Clip>,
    pub preview_joints: Arc<[PreviewJoint]>,
    pub animation_nodes: Vec<usize>,
}

#[derive(Debug)]
pub(super) struct Chunk {
    pub kind: u32,
    pub bytes: Arc<[u8]>,
}

impl Clip {
    /// 用普通 GLB 变换通道承载其他编辑入口产生的关键帧。
    pub fn push_transform_channel(
        &mut self,
        node: usize,
        node_name: &str,
        path: &'static str,
        keys: Vec<Keyframe>,
    ) -> Result<(), String> {
        let channel = Channel {
            node,
            node_name: node_name.into(),
            path: path.into(),
            interpolation: "LINEAR".into(),
            keys,
            source_index: self.channels.len(),
        };
        self.push_existing_channel(channel)
    }

    pub fn push_existing_channel(&mut self, mut channel: Channel) -> Result<(), String> {
        channel.validate()?;
        channel.source_index = self.channels.len();
        self.channels.push(channel);
        Ok(())
    }

    pub fn duration(&self) -> f32 {
        self.channels
            .iter()
            .filter_map(|channel| channel.keys.last())
            .map(|key| key.time)
            .fold(0.0, f32::max)
    }
}
