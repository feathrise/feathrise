//! 将全部正式Blade输出的内存R8完成值同步到运行时Image基础Mip。

use std::collections::BTreeMap; // 最多八个正式Blade输出按稳定ID保留各自Image同步进度。

use bevy::image::{ImageLoaderSettings, ImageSampler}; // 与现有GPU叶片加载保持线性最近采样。
use bevy::prelude::*; // AssetServer、Assets<Image>与编辑资源。
use bevy::render::render_resource::TextureFormat;
use feathrise_scene_content::{SceneId, SceneVegetationKind};

use super::resources::EditorAuthoringSession; // 只采用已完整编译的正式密度修订。
use crate::resources::SceneEditorState;
use crate::types::EditorTool;

/// 单个正式Blade输出的强句柄与已同步修订。
#[derive(Default)]
struct LiveBladeLayerState {
    path: String,          // 当前正式密度KTX2路径，热换路径时重建强句柄。
    handle: Handle<Image>, // 直接修改运行时已经消费的同一Image资产。
    applied_revision: u64, // 该正式输出最近一次完整写入的作者编译修订。
}

/// 当前场景全部正式Blade输出的有界同步状态；删除最后作者层后仍可把旧Image归零。
#[derive(Resource, Default)]
pub(crate) struct LiveBladeDensityState {
    pub(crate) dynamic: super::live_density_dynamic::DynamicLiveDensityState, // 同图草稿任务在关闭F12后继续完成。
    model_identity: Option<(SceneId, u64, u64, usize)>, // 索引地址区分同图重开后归零的作者修订。
    scene_id: Option<SceneId>,   // 切换场景时整表失效，旧句柄不得接管新场景。
    prepared_revision: u64,  // 已为哪个编译修订建立正式层句柄集合。
    completed_revision: u64, // 全部Blade输出均写入后的完整修订门。
    layers: BTreeMap<String, LiveBladeLayerState>, // 固定正式层数决定上限，不随作者图数量增长。
}

fn request_numeric_image(asset_server: &AssetServer, path: String) -> Handle<Image> {
    asset_server
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = ImageSampler::nearest();
        })
        .load(path)
}

/// 只读核对基础Mip布局；失败时不得先取得`AssetMut`，否则即使没写像素也会发布Modified事件。
fn can_update_full_image(image: &Image, density: &[u8], resolution: [u32; 2]) -> bool {
    let Some(expected) = (resolution[0] as usize).checked_mul(resolution[1] as usize) else {
        return false; // 损坏尺寸不能绕回一个看似合法的小型基础级。
    };
    if image.texture_descriptor.size.width != resolution[0]
        || image.texture_descriptor.size.height != resolution[1]
        || image.texture_descriptor.size.depth_or_array_layers != 1
        || density.len() != expected
    {
        return false;
    }
    let Some(data) = image.data.as_ref() else {
        return false; // GPU专用且无主世界像素的Image不能接受作者热同步。
    };
    match image.texture_descriptor.format {
        TextureFormat::R8Unorm => data.len() >= density.len(), // KTX2在基础级后还带完整Mip，不能要求总长度恰好相等。
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => density
            .len()
            .checked_mul(4)
            .is_some_and(|base_bytes| data.len() >= base_bytes), // 旧RGBA密度图同样允许尾随Mip。
        _ => false, // 压缩、整数或其他颜色格式不能冒充线性密度。
    }
}

/// 写入GPU Blade实际固定读取的基础Mip；保存事务仍会在后台重新生成完整Mip链。
fn update_full_image(image: &mut Image, density: &[u8], resolution: [u32; 2]) -> bool {
    if !can_update_full_image(image, density, resolution) {
        return false; // 调用系统会先只读预检；这里保留防御门供测试和未来复用。
    }
    let data = image.data.as_mut().expect("基础Mip布局已确认存在CPU像素"); // 预检后的同系统可变借用不会改变资产结构。
    match image.texture_descriptor.format {
        TextureFormat::R8Unorm => {
            data[..density.len()].copy_from_slice(density); // Shader使用textureLoad(..., 0)，尾随Mip保持原资产布局供保存热更替换。
        }
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => {
            let base_bytes = density.len() * 4; // 乘法已由预检确认不会溢出且数据充足。
            for (pixel, value) in data[..base_bytes]
                .chunks_exact_mut(4)
                .zip(density.iter().copied())
            {
                pixel.copy_from_slice(&[value, value, value, 255]); // 兼容旧RGBA密度资产并保持非sRGB数值一致。
            }
        }
        _ => unreachable!("基础Mip格式已由只读预检限制为R8或RGBA8"), // 坏格式不会触发AssetEvent。
    }
    true
}

/// 作者图层完整编译后触发现有正式Image资产的Modified事件；动态场景四级窗口直接读取，
/// HN-01继续由既有跨层布局与GPU候选门采用，不为每张作者图创建新窗口。
pub(crate) fn sync_live_blade_density(
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut live: ResMut<LiveBladeDensityState>,
    active: Res<feathrise_scene_content::ActiveDynamicScene>, // 当前玩法场景身份隔离会话图片。
    mut dynamic: ResMut<feathrise_scene::DynamicBladeDraft>, // 正式四级窗口读取完整内存快照。
    volume: Option<Res<super::volume_sculpt::VolumeAuthoringState>>,
    cross: Option<Res<super::vegetation_paint_scope::VegetationPaintState>>,
    mut models: Option<ResMut<feathrise_scene::DynamicModelVegetationDraft>>,
) {
    if let Some(models) = models.as_deref_mut() {
        let current = session.draft.as_ref().filter(|draft| Some(&draft.scene_id) == active.scene_id.as_ref());
        let identity = current.map(|draft| (draft.scene_id.clone(), session.vegetation_revision, session.vegetation_structure_revision,
            super::terrain_documents::root_vegetation::source(draft).and_then(|source| source.vegetation)
                .map_or(0, |definition| std::sync::Arc::as_ptr(&definition.model_surfaces) as usize)));
        if identity != live.model_identity && !session.model_vegetation_busy {
            if let Some(draft) = current {
                if let Some(definition) = super::terrain_documents::root_vegetation::source(draft).and_then(|source| source.vegetation) {
                    models.publish(draft.scene_id.clone(), session.vegetation_revision, std::sync::Arc::new(definition.clone()));
                } else { models.clear(); }
            } else { models.clear(); }
            live.model_identity = identity;
        }
    }
    let planar = session.draft.as_ref().is_some_and(|draft| draft.terrain_manifest().terrain.kind != feathrise_scene_content::SceneTerrainKind::Voxel)
        && volume.as_ref().is_none_or(|volume| volume.planar_density_refresh_available(&session));
    if cross.as_ref().is_some_and(|state|state.busy()) {return;} // 跨网格完整批次拥有本笔的预览发布权。
    live.dynamic
        .sync(&session, active.scene_id.as_ref(), &mut images, &mut dynamic, planar); // 关闭或切换工具后继续完成最后一笔。
    if session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.terrain_manifest().is_data_driven())
    {
        return; // 动态场景新层没有磁盘纹理，禁止进入旧固定场景Image写入路径。
    }
    if !editor.open || editor.tool != EditorTool::Vegetation {
        return;
    }
    if session.vegetation_compiled_revision != session.vegetation_revision {
        return; // 新作者修订尚未完整编译时保留上一套全部Blade Image，禁止跨修订半套采用。
    }
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    let Some(vegetation) = draft.vegetation.as_ref() else {
        return; // 当前场景没有通用植被正式输出。
    };
    let revision = session.vegetation_compiled_revision; // 全部输出必须向同一原子完成修订收敛。
    if live.scene_id.as_ref() != Some(&draft.scene_id) {
        live.scene_id = Some(draft.scene_id.clone());
        live.layers.clear(); // 场景身份改变后释放全部旧正式层强句柄。
        live.prepared_revision = u64::MAX; // 当前修订必须重新建立固定正式层集合。
        live.completed_revision = u64::MAX; // 新场景尚未完成任何Blade同步。
    }
    if live.completed_revision == revision {
        return; // 稳定帧在常数身份比较后退出，不遍历正式层或接触Image像素。
    }
    if live.prepared_revision != revision {
        live.layers.retain(|layer_id, _| {
            vegetation
                .layers
                .iter()
                .any(|layer| layer.kind == SceneVegetationKind::Blade && layer.id == *layer_id) // 只保留当前场景仍登记的正式Blade身份。
        });
        for layer in vegetation
            .layers
            .iter()
            .filter(|layer| layer.kind == SceneVegetationKind::Blade)
        {
            let current = live.layers.entry(layer.id.clone()).or_default(); // 固定最多八层，不随作者层数量扩张。
            if current.path != layer.density_texture_path {
                current.path = layer.density_texture_path.clone(); // 路径变化视为新的正式Image身份。
                current.handle = request_numeric_image(&asset_server, current.path.clone()); // 强句柄异步等待AssetServer现有加载流程。
                current.applied_revision = u64::MAX; // 新Image就绪后必须接受当前完整R8。
            }
        }
        live.prepared_revision = revision; // 本修订句柄集合只建立一次。
    }
    for layer in vegetation
        .layers
        .iter()
        .filter(|layer| layer.kind == SceneVegetationKind::Blade)
    {
        let Some(current) = live.layers.get_mut(&layer.id) else {
            continue; // 上方准备后的防御性身份门。
        };
        if current.applied_revision == revision {
            continue; // 当前正式输出已经属于完整目标修订。
        }
        let Some(density) = draft.vegetation_densities.get(&layer.id) else {
            continue; // 编译结果缺层时保持上一有效Image，不发布半套零图。
        };
        let ready = images.get(&current.handle).is_some_and(|image| {
            can_update_full_image(image, density.as_slice(), layer.resolution)
        }); // 必须先只读验证；`Assets::get_mut`一旦发生可变解引用就会产生Modified事件。
        if !ready {
            continue; // 资产仍在异步上传时尝试其他已就绪层，绝不阻塞等待。
        }
        let Some(mut image) = images.get_mut(&current.handle) else {
            continue; // 同一系统内只读检查后的防御性资产存在门。
        };
        if update_full_image(&mut image, density.as_slice(), layer.resolution) {
            current.applied_revision = revision; // 单层基础Mip完整写入后才推进该层采用门。
            break; // 每帧最多上传一个正式Image基础级，限制主线程与RenderAsset峰值。
        }
    }
    if vegetation
        .layers
        .iter()
        .filter(|layer| layer.kind == SceneVegetationKind::Blade)
        .all(|layer| {
            live.layers
                .get(&layer.id)
                .is_some_and(|current| current.applied_revision == revision)
        })
    {
        live.completed_revision = revision; // 全部Blade正式输出同修订完成后，稳定帧恢复常数退出。
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, ImageType};
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    use super::super::ktx2::encode_r8; // 用正式编辑器编码器复现带完整Mip的运行时KTX2。

    #[test]
    fn full_sync_writes_rgba_density_without_changing_dimensions() {
        let mut image = Image::new(
            Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![0; 8],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::MAIN_WORLD,
        );
        assert!(update_full_image(&mut image, &[17, 203], [2, 1]));
        assert_eq!(image.data.unwrap(), [17, 17, 17, 255, 203, 203, 203, 255]);
    }

    #[test]
    fn full_sync_can_clear_an_existing_r8_formal_density() {
        let mut image = Image::new(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![255; 4], // 模拟删除作者层前仍在运行时显示的满密度正式图。
            TextureFormat::R8Unorm,
            RenderAssetUsages::MAIN_WORLD,
        );
        assert!(update_full_image(&mut image, &[0; 4], [2, 2]));
        assert_eq!(image.data.unwrap(), [0; 4]); // 零完成值必须覆盖旧树叶片密度而非保留旧Image。
    }

    #[test]
    fn full_sync_accepts_mipped_ktx2_and_only_replaces_the_shader_base_level() {
        let source = (0_u8..16).collect::<Vec<_>>(); // 四乘四基础级会派生二乘二和一乘一两个尾随Mip。
        let encoded = encode_r8(&source, [4, 4]).expect("正式R8 KTX2编码应成功");
        let mut image = Image::from_buffer(
            &encoded,                      // 直接走Bevy正式KTX2解码器取得真实连续Mip布局。
            ImageType::Extension("ktx2"),  // 明确使用KTX2容器路径。
            CompressedImageFormats::NONE,  // 编辑器派生纹理未压缩，不依赖GPU压缩支持。
            false,                         // 密度是线性数值而不是sRGB颜色。
            ImageSampler::nearest(),       // 与正式密度加载设置一致。
            RenderAssetUsages::MAIN_WORLD, // 测试需要CPU像素可写。
        )
        .expect("正式KTX2必须能被Bevy解码");
        assert_eq!(image.texture_descriptor.mip_level_count, 3); // 确认不是单Mip夹具。
        let tail_before = image.data.as_ref().expect("KTX2保留CPU像素")[16..].to_vec(); // 冻结旧尾随Mip验证不会破坏布局。

        assert!(update_full_image(&mut image, &[203; 16], [4, 4])); // 上次回归正是在此因总长度不相等而失败。
        let data = image.data.expect("更新后CPU像素仍存在");
        assert_eq!(&data[..16], &[203; 16]); // GPU Blade显式读取Mip 0，未保存结果当帧可见。
        assert_eq!(&data[16..], tail_before.as_slice()); // 保存时再由后台编码器完整重建Mip，不改变当前Image结构。
    }
}
