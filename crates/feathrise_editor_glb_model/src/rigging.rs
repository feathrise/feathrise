//! 人形绑骨的任务入口、检测回执与待发布结果。

use crate::GlbTask;
use bevy::prelude::Resource;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

pub type Landmarks = BTreeMap<String, [f32; 3]>;
pub type BlenderLocator = fn() -> Result<PathBuf, String>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RigBackend {
    #[default]
    Native,
    Blender,
}

/// 检测时冻结源文件及所用后端，生成时把校正点交还同一个来源。
pub trait RigSource: Send + Sync {
    fn bind(
        self: Arc<Self>,
        reference_path: String,
        landmarks: Landmarks,
        cancel: Arc<AtomicBool>,
    ) -> GlbTask<PreparedRig>;
}

pub struct AnimationAsset {
    pub path: String,
    pub clips: Vec<String>, // 顺序与 glTF animations 数组一致，包含未命名动作的真实序号。
    pub can_reference: bool,
}

pub struct Inspection {
    pub input: Arc<dyn RigSource>,
    pub landmarks: Landmarks,
    pub animations: Vec<AnimationAsset>,
    pub warnings: Vec<String>,
}

pub struct PublishedRig {
    pub path: String,
    pub warnings: Vec<String>,
}

type Publish = Box<dyn FnOnce(&AtomicBool) -> Result<PublishedRig, String> + Send + Sync>;

/// 暂存结果携带一次发布权。会话校验通过后，场景才把它交给原 IO 工作槽。
pub struct PreparedRig {
    pub joints: usize,
    pub vertices: usize,
    publish: Publish,
}

impl PreparedRig {
    pub fn new(
        joints: usize,
        vertices: usize,
        publish: impl FnOnce(&AtomicBool) -> Result<PublishedRig, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            joints,
            vertices,
            publish: Box::new(publish),
        }
    }

    pub fn publish(self, cancel: &AtomicBool) -> Result<PublishedRig, String> {
        (self.publish)(cancel)
    }
}

type Inspect =
    fn(PathBuf, String, RigBackend, BlenderLocator, Arc<AtomicBool>) -> GlbTask<Inspection>;

#[derive(Resource, Clone, Copy)]
pub struct RiggingOperations {
    pub catalog: fn(&Path, &AtomicBool) -> Result<Vec<AnimationAsset>, String>,
    pub inspect: Inspect,
}
