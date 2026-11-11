//! Clipmap水域作者草稿的后台静水图片准备与完整水网快照发布。

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use feathrise_scene_content::{SceneId, SceneTerrainKind, SceneWaterAsset};
use feathrise_scene::{DynamicClipmapWaterDraft, DynamicClipmapWaterDraftSnapshot};

use super::resources::EditorAuthoringSession;
use super::water_handoff::WaterFileHandoff;

type Identity = (SceneId, u64); // 场景与作者水修订共同拒绝切换场景或继续落笔后的迟到结果。
type Prepared = (
    Identity,
    SceneWaterAsset,
    VecDeque<(String, Image)>,
    BTreeMap<String, Arc<[u8]>>,
);

/// 一个场景只允许一个R8准备任务；主线程每帧最多登记一张GPU图片。
#[derive(Default)]
struct LiveWaterChannel {
    task: Option<Task<Result<Prepared, String>>>,
    staging: Option<Prepared>,
    handles: BTreeMap<String, Handle<Image>>,
    applied: Option<Identity>,
    attempted: Option<Identity>,
    failed: Option<(Identity, String)>, // 发布前失败也保留精确作者身份，预览不能永久等待不存在的草稿。
}
impl LiveWaterChannel {
    fn failure_for(&self, map: &SceneId, revision: u64) -> Option<&str> {
        self.failed.as_ref().filter(|(id, _)| &id.0 == map && id.1 == revision)
            .map(|(_, detail)| detail.as_str())
    }
}

/// 根水与网格水拥有独立完成缓存；两者共享一个CPU准备槽和每帧一张图片的登记额度。
#[derive(Resource, Default)]
pub(crate) struct LiveClipmapWaterState {
    terrain: LiveWaterChannel,
    root: LiveWaterChannel,
}
impl LiveClipmapWaterState {
    pub(crate) fn failure_for(&self, map: &SceneId, revision: u64) -> Option<&str> {
        self.terrain.failure_for(map, revision)
    }
    pub(crate) fn root_failure_for(&self, map: &SceneId, revision: u64) -> Option<&str> {
        self.root.failure_for(map, revision)
    }
}

struct WaterSource<'a> {
    scene_id: &'a SceneId,
    revision: u64,
    definition: Option<&'a SceneWaterAsset>,
    masks: &'a BTreeMap<String, Arc<Vec<u8>>>,
}

/// 定义、共享CPU R8和全部GPU句柄只有在同一作者修订齐备后才一起发布。
pub(crate) fn sync_live_clipmap_water(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    active: Res<feathrise_scene_content::ActiveDynamicScene>,
    mut images: ResMut<Assets<Image>>,
    mut state: ResMut<LiveClipmapWaterState>,
    mut output: ResMut<DynamicClipmapWaterDraft>,
    handoff: Option<Res<WaterFileHandoff>>,
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
    mut editor: Option<ResMut<crate::resources::SceneEditorState>>,
) {
    let draft = session.draft.as_ref().filter(|draft| {
        Some(&draft.scene_id) == active.scene_id.as_ref()
            && draft.terrain_manifest().is_data_driven()
    });
    let terrain = draft.filter(|draft| draft.terrain_manifest().terrain.kind == SceneTerrainKind::SparseClipmap);
    let root_view = draft.filter(|draft| draft.manifest.terrain.is_empty())
        .map(super::terrain_documents::root_water::source);
    let root = draft.zip(root_view.as_ref()).map(|(draft, source)| WaterSource {
        scene_id: &draft.scene_id,
        revision: super::terrain_documents::root_water::revision(draft, session.water_revision),
        definition: source.water, masks: source.masks,
    });
    let terrain_source = terrain.map(|draft| WaterSource { scene_id: &draft.scene_id, revision: session.water_revision,
        definition: draft.water.as_ref(), masks: &draft.water_masks });
    let formal_root = draft.and_then(|draft| handoff.as_deref()
        .and_then(|handoff| handoff.formal_water_revision_for_canvas(&draft.scene_id, None)));
    let formal_terrain = terrain.and_then(|draft| handoff.as_deref()
        .and_then(|handoff| handoff.formal_water_revision_for_canvas(&draft.scene_id,
            draft.terrain_documents.active.as_ref().map(|canvas| &canvas.definition.id))));
    let texture_limit = device.as_deref().map(|device| device.limits().max_texture_dimension_2d);
    let state = &mut *state;
    let mut uploaded = false;
    let allow_root = state.terrain.task.is_none();
    sync_channel(&mut commands, &mut images, &mut state.root, &mut output, root, formal_root,
        true, allow_root, &mut uploaded, texture_limit, editor.as_deref_mut());
    let allow_terrain = state.root.task.is_none();
    sync_channel(&mut commands, &mut images, &mut state.terrain, &mut output, terrain_source, formal_terrain,
        false, allow_terrain, &mut uploaded, texture_limit, editor.as_deref_mut());
}

#[allow(clippy::too_many_arguments)]
fn sync_channel(commands: &mut Commands, images: &mut Assets<Image>, state: &mut LiveWaterChannel,
    output: &mut DynamicClipmapWaterDraft, current: Option<WaterSource<'_>>, formal_revision: Option<u64>,
    root: bool, allow_start: bool, uploaded: &mut bool, texture_limit: Option<u32>,
    mut editor: Option<&mut crate::resources::SceneEditorState>) {
    let identity = current.as_ref().map(|source| (source.scene_id.clone(), source.revision));
    if state.failed.as_ref().is_some_and(|(id, _)| Some(id) != identity.as_ref()) {
        state.failed = None; // 新图或新一笔只等待自己的结果，旧失败不能覆盖它的反馈。
    }
    if identity.is_none() {
        if root { output.clear_root(); } else { output.clear(); }
        if let Some(old) = state.staging.take() {
            feathrise_scene::retire_water_cpu(commands, old);
        }
        state.handles.clear();
        state.applied = None;
        state.attempted = None;
    }

    if formal_revision.is_some() {
        // 【保存设计】正式JSON/R8/KTX2必须亲自完成一次运行时采用，不能让内容相同的内存草稿
        // 抢先满足交接代次。门结束后即使作者修订未变，也必须重新发布供下一次保存预检使用。
        if root { output.clear_root(); } else { output.clear(); }
        if let Some(old) = state.staging.take() {
            feathrise_scene::retire_water_cpu(commands, old);
        }
        state.handles.clear();
        state.applied = None; // 正式文件就绪不能冒充已被撤回的内存发布。
        if state.task.is_none() {
            state.attempted = None;
        } // 在途任务仍由原attempted身份解释；不能把迟到结果或错误改挂到新一笔。
        return;
    }

    if let Some(result) = state.task.as_mut().and_then(check_ready) {
        state.task = None; // 任务实际完成前不释放唯一后台槽。
        match result {
            Ok(prepared)
                if Some(&prepared.0) == identity.as_ref()
                    && state.applied.as_ref() != Some(&prepared.0) =>
            {
                state.staging = Some(prepared);
            }
            Ok(old) => {
                feathrise_scene::retire_water_cpu(commands, old);
            } // 大型迟到图片在后台释放。
            Err(error) => {
                // attempted只会在唯一任务启动时设置；迟到旧任务结束前不能启动或借用另一代身份。
                if let Some(id) = state.attempted.as_ref().filter(|id| Some(*id) == identity.as_ref()).cloned() {
                    if let Some(editor) = editor.as_deref_mut() {
                        editor.status = format!("Clipmap水域草稿准备失败：{error}");
                    }
                    state.failed = Some((id, error.clone()));
                    error!("Clipmap水域草稿准备失败：{error}");
                }
            }
        }
    }
    if state
        .staging
        .as_ref()
        .is_some_and(|prepared| Some(&prepared.0) != identity.as_ref())
    {
        if let Some(old) = state.staging.take() {
            feathrise_scene::retire_water_cpu(commands, old);
        }
        state.handles.clear();
    }

    if let Some((_, _, pending, _)) = state.staging.as_mut() {
        if !*uploaded && let Some((layer_id, image)) = pending.pop_front() {
            state.handles.insert(layer_id, images.add(image)); // 一帧移动一张完整图片，不限制层数或作者总字节。
            *uploaded = true;
        }
        if pending.is_empty() {
            let (id, definition, _, sources) = state.staging.take().expect("完整草稿候选存在");
            let counts = (
                definition.layers.len(),
                definition.rivers.len(),
                definition.waterfalls.len(),
            );
            let snapshot = DynamicClipmapWaterDraftSnapshot {
                source_revision: id.1,
                definition,
                sources,
                masks: std::mem::take(&mut state.handles),
            };
            if root { output.publish_root(snapshot); } else { output.publish(snapshot); }
            info!(
                "Clipmap水域草稿已发布：map={} source_revision={} 静水层={} 河流={} 瀑布={}",
                id.0, id.1, counts.0, counts.1, counts.2
            );
            state.applied = Some(id);
            state.failed = None;
        }
        return;
    }

    let Some(id) = identity else { return };
    if !allow_start || state.task.is_some()
        || state.applied.as_ref() == Some(&id)
        || state.attempted.as_ref() == Some(&id)
        || state.failure_for(&id.0, id.1).is_some() // 正式门只撤回发布缓存，同修订失败仍只报告一次。
    {
        return;
    }
    let source = current.expect("水域发布身份来自当前作者草稿");
    let Some(definition) = source.definition.cloned() else {
        if root { output.clear_root(); } else { output.clear(); }
        state.applied = Some(id);
        return;
    };
    let masks = source.masks.clone(); // 只复制Arc；大R8像素复制全部留在worker。
    state.attempted = Some(id.clone());
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let mut pending = VecDeque::with_capacity(definition.layers.len());
        let mut sources = BTreeMap::new();
        for layer in &definition.layers {
            if texture_limit.is_some_and(|limit| layer.resolution.iter().any(|&size| size > limit))
            {
                return Err(format!(
                    "静水体{}的{}×{}遮罩超出当前GPU二维纹理能力{}",
                    layer.id,
                    layer.resolution[0],
                    layer.resolution[1],
                    texture_limit.unwrap()
                ));
            }
            let source = masks
                .get(&layer.id)
                .ok_or_else(|| format!("静水体{}缺少R8作者源", layer.id))?;
            let expected = (layer.resolution[0] as usize)
                .checked_mul(layer.resolution[1] as usize)
                .ok_or_else(|| format!("静水体{}的R8尺寸溢出", layer.id))?;
            if source.len() != expected {
                return Err(format!("静水体{}的R8尺寸与定义不一致", layer.id));
            }
            let mut pixels = Vec::new();
            pixels
                .try_reserve_exact(expected)
                .map_err(|_| format!("静水体{}的R8上传图片内存分配失败", layer.id))?;
            pixels.extend_from_slice(source.as_slice()); // 所有大像素复制只发生在worker。
            let mut image = Image::new(
                Extent3d {
                    width: layer.resolution[0],
                    height: layer.resolution[1],
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                pixels,
                TextureFormat::R8Unorm,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            );
            image.sampler = ImageSampler::linear(); // 与Shader中的抗锯齿岸缘采样保持一致。
            sources.insert(layer.id.clone(), Arc::from(source.as_slice()));
            pending.push_back((layer.id.clone(), image));
        }
        Ok((id, definition, pending, sources))
    }));
}

#[cfg(test)]
mod tests;
