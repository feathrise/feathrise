//! 场景保存事务的纯编码、私有水JSON目标解析与可选目录快照辅助逻辑。

use std::path::{Path, PathBuf}; // 水域JSON必须解析为当前场景私有目录内的确定目标。

use feathrise_scene_content::{
    SCENE_BOUNDARIES_FILE_NAME, SCENE_CATALOG_ASSET_PATH, SCENE_SCHEMA_VERSION, SceneCatalogAsset,
}; // 首次水域或边界保存把新目录字节纳入同一回滚事务。

use super::super::files::safe_scene_private_asset_path; // 自定义水JSON仍受当前场景私有目录约束。
use super::super::resources::EditorSceneDraft; // 可选目录登记读取保存任务冻结的完整草稿。
use super::transaction::encode_json_bytes; // 全局目录也必须在首个暂存文件创建前完成序列化。

#[cfg(test)]
pub(super) fn encode_heights(values: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 2);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// 旧动态场景首次保存河水或边界时生成全局目录的新字节；实际替换由整批事务最后执行。
pub(super) fn prepare_optional_catalog_bytes(
    root: &std::path::Path,
    draft: &EditorSceneDraft,
) -> Result<Option<(PathBuf, Vec<u8>)>, String> {
    let root_water = crate::authoring::terrain_documents::root_water::source(draft);
    // 私有网格的植被入口由terrain对象记录持有，不能借活动槽把它登记成场景根植被。
    let root_vegetation_registration = crate::authoring::terrain_documents::root_vegetation::source(draft)
        .is_some_and(|source| source.registration_source_hash.is_some());
    if !root_vegetation_registration // 旧Clipmap首次保存纳入相同目录事务。
        && !root_water.registration_required
        && !draft.boundary_catalog_registration_required
        && !draft.domain.as_ref().is_some_and(|domain| domain.registration_required())
    {
        return Ok(None); // 没有新增入口时不读取也不重写全局目录。
    }
    let catalog_path = root.join(SCENE_CATALOG_ASSET_PATH);
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取场景目录{}失败：{error}", catalog_path.display()))?;
    let mut catalog: SceneCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析场景目录{}失败：{error}", catalog_path.display()))?;
    if catalog.schema_version != SCENE_SCHEMA_VERSION {
        return Err(format!(
            "场景目录版本{}与编辑器版本{}不一致",
            catalog.schema_version, SCENE_SCHEMA_VERSION
        ));
    }
    let entry = catalog
        .scenes
        .iter_mut()
        .find(|entry| entry.id == draft.scene_id)
        .ok_or_else(|| format!("场景目录不存在{}", draft.scene_id))?;
    if let Some(domain) = draft.domain.as_ref().filter(|domain| domain.registration_required()) {
        (domain.ops.register)(&*domain.value, entry)?;
    } // 只有实际安装的外部作者能登记自己的文档，普通场景保留旧入口。

    if root_vegetation_registration {
        // 补登记前拒绝外部工具已经改向其他文件的目录入口。
        let path = format!(
            "{}/{}",
            draft.folder.trim_end_matches('/'),
            feathrise_scene_content::SCENE_VEGETATION_FILE_NAME
        ); // 与实际私有JSON写入目标严格一致。
        if entry.folder != draft.folder
            || entry
                .vegetation_path
                .as_ref()
                .is_some_and(|current| current != &path)
        {
            // 不覆盖打开后外部移动或重新登记。
            return Err("场景植被目录入口已被外部修改，请重新打开场景后保存".to_owned()); // 整批尚未暂存，原作者文档完整保留。
        } // 同路径登记可安全复用。
        entry.vegetation_path = Some(path); // JSON与作者R8全部写入后最后发布目录。
    } // 失败继续保留草稿登记责任。
    if root_water.registration_required {
        if entry.folder != draft.folder
            || entry
                .water_path
                .as_ref()
                .is_some_and(|current| current != root_water.asset_path)
        {
            return Err("场景水域目录入口已被外部修改，请重新打开场景后保存".to_owned());
        } // 允许外部恰好登记相同路径，但文件摘要守卫仍会验证内容未改变。
        entry.water_path = Some(root_water.asset_path.to_owned()); // 根水和私有网格分别保存，目录只登记真正的场景根路径。
    }
    if draft.boundary_catalog_registration_required {
        entry.boundary_path = Some(format!(
            "{}/{}",
            entry.folder.trim_end_matches('/'),
            SCENE_BOUNDARIES_FILE_NAME
        )); // 边界JSON已经完整写入，运行时热更不会观察到半成品。
    }
    let bytes = encode_json_bytes(&catalog_path, &catalog)?; // 目录序列化失败仍发生在任何事务暂存之前。
    Ok(Some((catalog_path, bytes)))
}

pub(super) fn resolve_water_json_output_path(
    root: &Path,
    folder: &str,
    water_asset_path: &str,
    water_present: bool,
) -> Result<Option<PathBuf>, String> {
    water_present
        .then(|| safe_scene_private_asset_path(root, folder, water_asset_path))
        .transpose()
}
