//! 小Clipmap二维密度的共享输入和后台全页证据；不在帧内扫描或复制作者像素。
use super::types::GeometryDensityProof;
use feathrise_scene_content::{SceneId, SceneTerrainObject, SceneVegetationAsset,
    SceneVegetationAuthorLayer, SceneVegetationLayer};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

pub(super) struct DensityInput {
    author: SceneVegetationAuthorLayer,
    output: SceneVegetationLayer,
    author_pixels: Arc<Vec<u8>>,
    output_pixels: Arc<Vec<u8>>,
}

pub(super) fn freeze(session: &crate::authoring::EditorAuthoringSession, selected: usize) -> Result<DensityInput, String> {
    let draft = session.draft.as_ref().ok_or("二维密度草稿不存在")?;
    let vegetation = draft.vegetation.as_ref().ok_or("小Clipmap缺少植被文档")?;
    let author = vegetation.authoring.as_ref().and_then(|a| a.layers.get(selected))
        .ok_or("小Clipmap缺少所选作者层")?.clone();
    let output = vegetation.layers.iter().find(|layer| layer.id == author.output_layer_id)
        .ok_or("二维作者层的正式输出不存在")?.clone();
    Ok(DensityInput { author_pixels: draft.vegetation_author_densities.get(&author.id)
        .ok_or("二维作者页尚未分配")?.clone(),
        output_pixels: draft.vegetation_densities.get(&output.id).ok_or("正式R8尚未编译")?.clone(), author, output })
}

pub(super) fn from_saved(root: &Path, scene: &SceneId, object: &SceneTerrainObject,
    expected: &GeometryDensityProof) -> Result<DensityInput, String> {
    let path = object.vegetation_path.as_deref().ok_or("保存没有登记Clipmap私有植被文档")?;
    let vegetation: SceneVegetationAsset = serde_json::from_slice(&super::readback::source_bytes(root, path)?)
        .map_err(|error| error.to_string())?;
    if vegetation.scene_id != *scene { return Err("持久二维植被场景身份错误".into()); }
    let author = vegetation.authoring.as_ref().and_then(|a| a.layers.iter().find(|l| l.id == expected.author_layer))
        .ok_or("持久植被缺少本笔作者层")?.clone();
    let output = vegetation.layers.iter().find(|l| l.id == expected.output_layer)
        .ok_or("持久植被缺少本笔正式输出")?.clone();
    if author.output_layer_id != output.id { return Err("持久作者层与正式输出身份错配".into()); }
    Ok(DensityInput {
        author_pixels: Arc::new(super::readback::source_bytes(root, &author.density_source_path)?),
        output_pixels: Arc::new(super::readback::source_bytes(root, &output.density_source_path)?), author, output,
    })
}

pub(super) fn capture(input: DensityInput) -> Result<GeometryDensityProof, String> {
    let summarize = |pixels: &[u8], resolution: [u32; 2]| -> Result<(String, usize), String> {
        let length = u64::from(resolution[0]) * u64::from(resolution[1]);
        if length > 32 * 1024 * 1024 || length != pixels.len() as u64 {
            return Err("二维密度长度错误或超过32MiB验收上限".into());
        }
        Ok((Sha256::digest(pixels).iter().map(|b| format!("{b:02x}")).collect(),
            pixels.iter().filter(|value| **value != 0).count()))
    };
    let (author_sha256, author_nonzero) = summarize(&input.author_pixels, input.author.resolution)?;
    let (output_sha256, output_nonzero) = summarize(&input.output_pixels, input.output.resolution)?;
    Ok(GeometryDensityProof {
        author_layer: input.author.id, output_layer: input.output.id,
        author_source_path: input.author.density_source_path, output_source_path: input.output.density_source_path,
        author_resolution: input.author.resolution, output_resolution: input.output.resolution,
        author_sha256, output_sha256, author_nonzero, output_nonzero,
    })
}
