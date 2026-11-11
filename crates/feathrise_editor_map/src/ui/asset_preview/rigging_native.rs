//! 编辑器原生绑骨。单个工作槽协调文件 IO 和纯 CPU 计算，复用既有候选发布流程。

use bevy::{
    prelude::{Quat, Vec3},
    tasks::AsyncComputeTaskPool,
};
use serde_json::Value;
use std::{
    fs,
    ops::Range,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use super::{Bound, Detected, Landmarks, files};

#[path = "rigging_native_glb.rs"]
mod glb;
#[path = "rigging_native_skeleton.rs"]
mod skeleton;
#[path = "rigging_native_weights.rs"]
mod weights;

/// 按 Scene0 网格实例和 primitive 顺序连续展开。顶点与三角形在同一场景坐标中计算。
pub(super) struct Geometry {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub parts: Vec<Range<usize>>,
}

/// 关节索引与参考 skin 的 joint 列表一致，局部变换在导出时由拟合后的场景变换求出。
#[derive(Clone)]
pub(super) struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    pub position: Vec3,
    pub rotation: Quat,
}

#[derive(Clone, Copy)]
pub(super) struct VertexSkin {
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

pub(super) async fn inspect(source: &Path, cancel: Arc<AtomicBool>) -> Result<Detected, String> {
    let bytes = read_snapshot(source, &cancel)?;
    AsyncComputeTaskPool::get()
        .spawn(async move {
            let model = glb::decode(bytes, &cancel)?;
            let (landmarks, warnings) = skeleton::estimate(&model.geometry, &cancel)?;
            Ok(Detected {
                landmarks,
                warnings,
            })
        })
        .await
}

pub(super) async fn bind(
    source: &Path,
    reference: Value,
    landmarks: &Landmarks,
    output: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<Bound, String> {
    let bytes = read_snapshot(source, &cancel)?;
    let landmarks = landmarks.clone();
    let computing = Arc::clone(&cancel);
    // 等待计算时归还 IO 线程，整个候选仍占据原工作槽。取消或过期的工作无法发布。
    let (glb, receipt) = AsyncComputeTaskPool::get()
        .spawn(async move {
            let model = glb::decode(bytes, &computing)?;
            let reference = glb::reference(&reference, &computing)?;
            let joints = skeleton::fit(&model.geometry, &reference, &landmarks, &computing)?;
            let (weights, warnings) = weights::solve(&model.geometry, &joints, &computing)?;
            let receipt = Bound {
                warnings,
                joints: joints.len(),
                vertices: weights.len(),
            };
            let glb = glb::encode(model, &joints, &weights, &computing)?;
            Ok::<_, String>((glb, receipt))
        })
        .await?;
    files::active(&cancel)?;
    glb.write(output, &cancel)?;
    Ok(receipt)
}

fn read_snapshot(path: &Path, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    // 输入是已通过大小检查、在当前任务独占目录内冻结的副本。
    files::active(cancel)?;
    let bytes = fs::read(path).map_err(|error| format!("读取原生绑骨快照失败：{error}"))?;
    files::active(cancel)?;
    Ok(bytes)
}
