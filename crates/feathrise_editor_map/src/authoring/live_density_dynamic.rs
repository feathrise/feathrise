//! 动态地图植被草稿后台图片准备及完整快照发布；不写磁盘、不依赖编辑器显隐。

use super::resources::EditorAuthoringSession; // 完成修订与Arc密度均由现有编译器提供。
use bevy::asset::RenderAssetUsages; // CPU净空派生和GPU上传共同持有密度。
use bevy::image::ImageSampler; // 概率纹理采用线性最近采样。
use bevy::prelude::*; // 共享图片资源和强句柄。
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat}; // 正式单层R8格式。
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready}; // single-flight后台复制整图像素。
use feathrise_map_content::{MapId, MapVegetationAsset}; // 与正式层同源定义。
use feathrise_scene::{DynamicBladeDraft, DynamicBladeDraftSnapshot}; // 完整输入交还正式四级运行时。
use std::collections::{BTreeMap, BTreeSet, VecDeque}; // 逐帧登记图片，整批齐备后才发布。

type Identity = (super::vegetation_compile::VegetationCompileSource, u64); // 地图、网格、结构与完整编译修订共同绑定预览。
type Prepared = (
    Identity,                               // 完整输入地图和作者修订。
    MapVegetationAsset,                     // 同代Blade层定义。
    VecDeque<(String, Image)>,              // 按帧登记的已完成图片。
    BTreeMap<String, std::sync::Arc<[u8]>>, // 建筑净空共享原始像素。
    BTreeSet<String>,                       // worker顺带得到的非零正式层。
    bool,                                   // 布局及Mesh密度不变时只更新Blade草窗密度。
); // 图片与净空CPU源在后台共同完成。

#[derive(Default)]
pub(crate) struct DynamicLiveDensityState {
    task: Option<Task<Result<Prepared, String>>>, // 在途任务必须真正结束后才归还唯一槽。
    staging: Option<Prepared>,                    // 未登记图片按一帧一张限流。
    handles: BTreeMap<String, Handle<Image>>,     // 完整登记前不能暴露给正式窗口。
    applied: Option<Identity>,                    // 稳定帧只比较常数身份。
    attempted: Option<Identity>,                  // 同一失败修订不重复启动。
}

impl DynamicLiveDensityState {
    pub(super) fn sync(
        &mut self,                        // 唯一后台槽与登记进度。
        session: &EditorAuthoringSession, // 关闭F12后继续消费最后会话草稿。
        active_map: Option<&MapId>,       // 只向当前玩法地图发布。
        images: &mut Assets<Image>,       // 主世界每帧最多登记一张图。
        output: &mut DynamicBladeDraft,   // 发布时定义与句柄不可分割。
        planar: bool,                    // 当前体积索引完整且无三维继承，才允许复用室外草窗。
    ) {
        let current = session.draft.as_ref().filter(|draft| {
            Some(&draft.map_id) == active_map && draft.terrain_manifest().is_data_driven()
        }); // 换图或固定场景不沿用旧输入。
        let identity = current.and_then(|_| super::vegetation_compile::VegetationCompileSource::capture(session))
            .filter(|source| source.structure_revision == session.vegetation_compiled_structure_revision
                && (session.vegetation_compiled_revision > 0 || session.vegetation_revision == 0))
            .map(|source| (source, session.vegetation_compiled_revision)); // 使用实际完整结果；连续刷印不饿死旧完整预览。
        if current.is_none() {
            output.clear(); // 草稿卸载或切图时撤销旧快照。
            self.applied = None; // 下次同图重开重新准备。
            self.attempted = None; // 清除旧失败门。
        }
        if let Some(result) = self.task.as_mut().and_then(check_ready) {
            self.task = None; // 真实完成后才释放single-flight槽。
            match result {
                Ok(prepared) if same_source_result(&prepared.0, identity.as_ref()) => {
                    self.staging = Some(prepared)
                } // 仅完整当前结果进入上传队列。
                Ok(_) => {} // 迟到结果无副作用。
                Err(error) => {
                    error!("Blade草稿准备失败：{error}"); // 每个失败修订只记录一次。
                }
            }
        }
        if self
            .staging
            .as_ref()
            .is_some_and(|value| !same_source_result(&value.0, identity.as_ref()))
        {
            self.staging = None; // 来源/结构换代才丢弃登记队列，持续刷印不重启同源上传。
            self.handles.clear(); // 释放未发布强句柄。
        }
        if let Some((id, definition, pending, sources, nonzero_layers, density_only)) = self.staging.as_mut() {
            if let Some((layer_id, image)) = pending.pop_front() {
                self.handles.insert(layer_id, images.add(image)); // 每帧最多登记一张，避免完成峰值。
            }
            if pending.is_empty() {
                let snapshot = DynamicBladeDraftSnapshot {
                    source_revision: id.1,                        // 同批定义和图片的作者修订回执。
                    sources: std::mem::take(sources),             // 大数组仅转移Arc，主线程不复制。
                    definition: definition.clone(),               // 小型描述符与全部图片同修订。
                    densities: std::mem::take(&mut self.handles), // 强句柄整体转移。
                    nonzero_layers: std::mem::take(nonzero_layers), // Mesh原型请求无需帧线程扫图。
                };
                if *density_only && planar { output.publish_density(snapshot); } else { output.publish(snapshot); }
                if !*density_only || !planar { info!(
                    "植被草稿已发布：map={} source_revision={} 层={}",
                    id.0.map,
                    id.1,
                    definition.layers.len()
                ); } // 只记录布局采用；连续像素更新不逐帧输出控制台日志。
                self.applied = Some(id.clone()); // 发布完成后稳定帧常数退出。
                self.staging = None; // 不保留重复图片。
            }
            return; // 本帧不再启动另一个任务。
        }
        let Some(id) = identity else { return }; // 没有当前地图草稿。
        if output.for_map(&id.0.map).is_some_and(|snapshot| snapshot.source_revision==id.1
            && current.and_then(|d|d.vegetation.as_ref()).is_some_and(|v|v.layers==snapshot.definition.layers)) {
            self.applied=Some(id); // 跨网格事务已经发布当前网格的同代完整快照，不再准备重复图片。
            return;
        }
        if self.task.is_some()
            || self.applied.as_ref() == Some(&id)
            || self.attempted.as_ref() == Some(&id)
        {
            return; // 编译中、失败或稳定状态均不重复分配。
        }
        let draft = current.expect("当前身份来自同一草稿"); // 上方身份存在门。
        let Some(definition) = draft.vegetation.clone() else {
            return;
        }; // 无植被协议时保留正式来源。
        let densities = draft.vegetation_densities.clone(); // 仅克隆Arc，不在帧线程复制像素。
        let previous = self.applied.as_ref().filter(|applied| applied.0 == id.0)
            .and_then(|_| output.frozen_for_map(&id.0.map)); // 同网格来源才可在worker比较旧正式规格与像素。
        self.attempted = Some(id.clone()); // 一次修订最多一个准备任务。
        self.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let density_only = if let Some(previous) = previous {
                serde_json::to_vec(&previous.definition.layers).map_err(|error| error.to_string())?
                    == serde_json::to_vec(&definition.layers).map_err(|error| error.to_string())?
                    && definition.layers.iter().filter(|layer| layer.kind == feathrise_map_content::MapVegetationKind::Mesh)
                        .all(|layer| previous.sources.get(&layer.id).zip(densities.get(&layer.id))
                            .is_some_and(|(before, after)| before.as_ref() == after.as_slice()))
            } else { false }; // 整图比较留在worker；Mesh散布或正式布局改变仍走完整原子重建。
            let mut prepared = VecDeque::new(); // 图片按定义顺序逐帧登记。
            let mut sources = BTreeMap::new(); // 同代净空源在后台冻结。
            let mut nonzero_layers = BTreeSet::new(); // 扫描留在worker，避免4096²画布进入帧路径。
            for layer in &definition.layers {
                let source = densities
                    .get(&layer.id)
                    .ok_or_else(|| format!("{}缺少编译密度", layer.id))?; // 失败拒绝整批。
                if source.len() != layer.resolution[0] as usize * layer.resolution[1] as usize {
                    return Err(format!("{}密度尺寸不匹配", layer.id)); // 不创建半张GPU图片。
                }
                let mut image = Image::new(
                    Extent3d {
                        width: layer.resolution[0],  // 作者图宽度。
                        height: layer.resolution[1], // 作者图高度。
                        depth_or_array_layers: 1,    // 单层二维纹理。
                    }, // 精确作者尺寸。
                    TextureDimension::D2,    // Blade使用二维密度。
                    source.as_ref().clone(), // 大像素复制仅在后台执行。
                    TextureFormat::R8Unorm,  // 线性密度，与正式KTX2基础级一致。
                    RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD, // 同时支持净空和渲染。
                );
                image.sampler = ImageSampler::nearest(); // 不对概率做颜色或线性插值。
                if source.iter().any(|value| *value != 0) {
                    nonzero_layers.insert(layer.id.clone()); // 空Mesh层不加载四级大原型。
                }
                sources.insert(layer.id.clone(), std::sync::Arc::from(source.as_slice())); // 整图复制仅发生在worker。
                prepared.push_back((layer.id.clone(), image)); // 完整完成值不访问World。
            }
            Ok((id, definition, prepared, sources, nonzero_layers, density_only)) // 同来源的完整结果才允许采用。
        }));
    }
}

fn same_source_result(result: &Identity, current: Option<&Identity>) -> bool {
    current.is_some_and(|current| result.0 == current.0 && result.1 <= current.1)
} // 只容许同来源的像素修订领先；关闭F12不改变来源，换网格或结构仍拒绝迟到结果。

#[cfg(test)]
mod tests; // 实际系统回归覆盖无文件新层、关闭后采用和换图迟到拒收。
