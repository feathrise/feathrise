//! 独立积水raw与KTX2作者文件；所有规模化IO和编码由调用方后台任务执行。
use super::super::files::safe_scene_private_asset_path; // 验证项目根与当前场景私有目录。
use super::super::resources::EditorSceneDraft; // 冻结完整草稿。
use super::super::save::transaction::PreparedWrite; // 与其他作者文件共同stage、替换及回滚。
use std::{path::Path, sync::Arc}; // 安全路径与共享作者源。 // 不允许积水路径指向其他场景。
pub(crate) fn read_source(root: &Path, draft: &mut EditorSceneDraft) -> Result<(), String> {
    // 只在草稿IO任务调用。
    let Some(layer) = draft.terrain_manifest().puddles.as_ref() else {
        return Ok(());
    }; // 旧图无额外IO且保持无积水。
    layer.validate()?; // 首先拒绝尺寸及文件类别错误。
    let source = safe_scene_private_asset_path(root, &draft.terrain_folder(), &layer.source_path)?; // 限制作者源私有范围。
    safe_scene_private_asset_path(root, &draft.terrain_folder(), &layer.texture_path)?; // 派生图也必须属于当前场景。
    let size = layer.resolution[0] as usize * layer.resolution[1] as usize; // 已通过每轴4096上限。
    draft.puddle_mask = Some(Arc::new(super::super::load::read_exact_bytes(
        &source,      // 当前场景经安全校验的绝对源路径。
        size,         // 严格要求一个字节对应一个潜力纹素。
        "积水潜力R8", // 读取错误的作者可见领域名称。
    )?)); // 失败不采用整个草稿。
    Ok(()) // 完整作者版本由加载主流程原子发布。
} // 结束后台读取。
pub(crate) fn prepare_writes<'a>(
    root: &Path,                         // 当前项目assets根。
    draft: &EditorSceneDraft,              // 保存启动时冻结的完整作者草稿。
    writes: &mut Vec<PreparedWrite<'a>>, // 与其他场景文件共享的stage及回滚列表。
) -> Result<(), String> {
    // 本函数不创建独立事务。
    let Some(layer) = draft.terrain_manifest().puddles.as_ref() else {
        return Ok(());
    }; // 删除引用仅由清单事务保存。
    layer.validate()?; // 未编辑的坏元数据也不得写入清单。
    if !draft.puddles_dirty {
        return Ok(());
    } // 其他作者功能保存不重复派生积水图。
    let mask = draft
        .puddle_mask
        .clone()
        .ok_or("积水描述缺少完整R8作者源")?; // 不能把空指针当成空画布。
    let resolution = layer.resolution; // 冻结编码尺寸。
    if mask.len() != resolution[0] as usize * resolution[1] as usize {
        return Err("积水R8长度与声明尺寸不符".into());
    } // 首stage前拒绝坏数据。
    let source = safe_scene_private_asset_path(root, &draft.terrain_folder(), &layer.source_path)?; // 私有路径安全门。
    let texture = safe_scene_private_asset_path(root, &draft.terrain_folder(), &layer.texture_path)?; // 私有派生路径安全门。
    writes.push(PreparedWrite::shared(source, mask.clone())); // raw保存不深拷贝。
    writes.push(PreparedWrite::deferred(texture, move || {
        super::super::ktx2::encode_r8(mask.as_slice(), resolution)
    })); // 按同一版本构建完整Mip。
    Ok(()) // 统一事务负责多文件原子替换和错误回滚。
} // 结束完整保存准备。
