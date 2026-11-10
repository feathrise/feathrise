//! 地表层事务的完整快照、稳定输入身份与后台完成值。

use bevy::prelude::Vec3; // 引入地表作者层事务所需类型和纯助手。
use feathrise_scene_content::SceneSurfaceAsset; // 完整定义包含局部层顺序、路径与共同分辨率。
use std::sync::Arc; // 两张整图作者源只在后台复制，输入和历史共享原像素。 // 等待独立写侧的笔刷保留原世界空间端点。

#[derive(Clone, Copy)] // 有界队列只保留固定大小标量段，内存不随等待时长增长。
pub(crate) struct PendingSurfaceBrushSample {
    // 完成当前地表作者事务的受检参数或值转换。
    // 共享像素准备完成后仍按按下时的完整语义补画。
    pub(crate) from: Vec3,  // 本段起点保持上一完整采样位置。
    pub(crate) to: Vec3,    // 本段冻结终点；新移动另排真实线段，不能覆盖折点。
    pub(crate) radius: f32, // 冻结当前段的米制半径。
    pub(crate) erase: bool, // 冻结Shift擦除语义，之后松键不能改写旧段。
    pub(crate) amount: f32, // 仅同点静止段可按渐近公式累计曝光，不能加深之前的移动路径。
} // 冻结样本结束。

#[derive(Clone)] // 克隆只复制小型地表定义及两个Arc句柄。
pub(crate) struct SurfaceAuthoringSnapshot {
    // 完成当前地表作者事务的受检参数或值转换。
    // 定义、索引和权重必须作为一个版本发布。
    pub(super) surface: SceneSurfaceAsset, // 完整地表作者文档。
    pub(super) weights: Arc<Vec<u8>>,    // RGBA8权重源的只读共享快照。
    pub(super) indices: Arc<Vec<u8>>,    // RGBA32_UINT小端局部层索引源。
} // 完整快照结束。

#[derive(Clone)] // 撤销栈转移不复制整图像素。
/// 外部作者只携带该历史值；内容和创建仍由普通编辑器自己的受检事务掌握。
pub struct SurfaceAuthoringChange {
    // 完成当前地表作者事务的受检参数或值转换。
    // 一次删层只产生一条原子历史命令。
    pub(crate) removed_index: usize, // 撤销选回恢复层，重做钳制到同位置剩余层。
    pub(super) before: SurfaceAuthoringSnapshot, // 删除前完整定义与原始字节。
    pub(super) after: SurfaceAuthoringSnapshot, // 删除后经过重映射的完整版本。
} // 前后快照结束。

#[derive(Clone, Copy)] // 操作种类属于固定大小任务身份。
pub(super) enum SurfaceEditKind {
    // 完成当前地表作者事务的受检参数或值转换。
    // 单个后台槽区分作者删除和无语义变化的写侧准备。
    Delete { index: usize }, // 删除前被冻结的场景局部层下标。
    Materialize,             // 两图复制成功后同时替换独立写侧，不改变作者修订。
} // 操作种类结束。

pub(super) struct SurfaceEditResult {
    // 完成当前地表作者事务的受检参数或值转换。
    // 所有输出携带完整输入身份，失败仍可区分迟到结果。
    pub(super) ticket: u64,                      // 只允许当前任务票据采用。
    pub(super) revision: u64,                    // 全部作者修改都能使旧结果失效。
    pub(super) surface_revision: u64,            // 地表预览和作者图的专用修订。
    pub(super) before: SurfaceAuthoringSnapshot, // 场景、定义以及两个Arc身份共同冻结输入。
    pub(super) kind: SurfaceEditKind,            // 采用时决定是否登记删除历史和脏标记。
    pub(super) result: Result<SurfaceAuthoringSnapshot, String>, // 完整成功值或可见错误，绝不返回半张图。
} // 后台完成值结束。
