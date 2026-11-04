//! 未登记动态地图根水文件的后台兼容装载；空地图与旧Clipmap共享同一读取和版本守卫。

use std::path::Path; // 所有磁盘访问均发生在现有IO任务内。

use feathrise_map_content::{MAP_SCHEMA_VERSION, MAP_WATER_FILE_NAME, MapWaterAsset};

use super::files::safe_map_private_asset_path; // 标准文件也必须留在当前地图私有目录。
use super::resources::EditorMapDraft; // 完整结果仍由外层加载任务原子发布。
use super::water_defaults::default_water_asset; // 确切缺失时建立空内存草稿。

/// 旧Clipmap没有目录入口时优先保留标准私有文件；损坏、错图或缺R8均明确失败。
pub(super) fn load_unregistered_clipmap_water(
    root: &Path,
    draft: &mut EditorMapDraft,
) -> Result<(), String> {
    let relative = if draft.water_asset_path.is_empty() {
        format!("{}/{}", draft.terrain_folder().trim_end_matches('/'), MAP_WATER_FILE_NAME)
    } else { draft.water_asset_path.clone() }; // 已冻结自定义路径时始终读取真实文件，不悄悄改写默认名。
    let path = safe_map_private_asset_path(root, &draft.terrain_folder(), &relative)?;
    let (water, source_hash) = match std::fs::read(&path) {
        Ok(bytes) => {
            let water: MapWaterAsset = serde_json::from_slice(&bytes)
                .map_err(|error| format!("解析旧Clipmap水域文件{}失败：{error}", path.display()))?;
            (
                water,
                feathrise_map_content::authoring_io::bytes_hash(&bytes),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (default_water_asset(&draft.map_id), "missing".to_owned())
        }
        Err(error) => {
            return Err(format!(
                "读取旧Clipmap水域文件{}失败：{error}",
                path.display()
            ));
        }
    };
    if water.schema_version != MAP_SCHEMA_VERSION {
        return Err(format!(
            "旧Clipmap水域文件{}版本{}与当前版本{}不一致",
            path.display(),
            water.schema_version,
            MAP_SCHEMA_VERSION
        ));
    }
    water
        .validate(&draft.map_id)
        .map_err(|error| format!("旧Clipmap水域文件{}无效：{error}", path.display()))?; // 旧河流和瀑布也完整校验并保留，静水保存不得把它们删除。
    draft.water_asset_path = relative;
    draft.water = Some(water);
    draft.water_catalog_registration_required = true;
    draft.water_registration_source_hash = Some(source_hash);
    Ok(())
}

/// 按水层持久化规格完整载入R8；生产打开和保存往返回归共用同一入口。
pub(super) fn load_sources(root: &Path, draft: &mut EditorMapDraft) -> Result<(), String> {
    use std::sync::Arc;
    use super::{files::safe_water_asset_path, load::read_exact_bytes};
    let water_layers = draft
        .water
        .as_ref()
        .map(|water| water.layers.clone())
        .unwrap_or_default();
    for layer in water_layers {
        let expected = layer.resolution[0] as usize * layer.resolution[1] as usize;
        let path = safe_water_asset_path(root, &draft.folder, &layer.mask_source_path)?;
        let texture_path =
            safe_water_asset_path(root, &draft.folder, &layer.mask_texture_path)?; // 即使本次只读raw R8，也不能让坏派生路径进入可保存草稿。
        if draft.water_registration_source_hash.is_some()
            && draft.water_registration_source_hash.as_deref() != Some("missing")
            && !texture_path.is_file()
        {
            return Err(format!(
                "未登记静水体{}缺少KTX2派生遮罩{}",
                layer.id,
                texture_path.display()
            )); // 已有私有JSON不允许以缺失GPU文件的损坏状态完成首次登记。
        }
        let mask = if draft.water_registration_source_hash.as_deref() == Some("missing")
            && !path.exists()
        {
            vec![0; expected]
        } else {
            read_exact_bytes(&path, expected, "静水R8覆盖源")?
        };
        if draft
            .water_masks
            .insert(layer.id.clone(), Arc::new(mask))
            .is_some()
        {
            return Err(format!("静水体ID重复：{}", layer.id));
        }
    }
    Ok(())
}
