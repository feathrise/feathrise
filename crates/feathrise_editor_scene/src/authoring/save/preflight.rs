//! 场景文件第一次落盘前的水域零写入预检。
//!
//! 【功能设计】正式格式、运行时网格预算、全部脏R8路径与输入长度先纯验证；KTX2随后逐项
//! 编码到事务stage，失败只清除内部文件，不让八张最大纹理同时常驻或改变正式资产。

use std::collections::{BTreeMap, BTreeSet}; // 脏静水体与作者画布按稳定ID确定性匹配。
use std::path::{Path, PathBuf}; // 受检源文件和派生纹理都保存绝对工作区路径。
use std::sync::Arc; // 预检完成值与后台保存快照共享R8所有权。

use feathrise_scene_content::{SCENE_SCHEMA_VERSION, SceneId, SceneWaterAsset}; // 水域身份和格式版本必须先验收。
use feathrise_scene::validate_spline_water_build_budget; // 保存与正式运行时复用同一实际截面和三角预算。

use super::super::files::{asset_path_collision_key, safe_water_asset_path}; // 水输出必须同时受assets根和当前场景私有目录约束。

/// 水域保存预检的完整输入；调用方逐项编码到stage后立即释放派生结果。
pub(super) struct PreparedWaterFile {
    pub(super) source_path: PathBuf,  // 当前场景私有raw R8正式目标。
    pub(super) texture_path: PathBuf, // 与raw一一对应的运行时KTX2目标。
    pub(super) mask: Arc<Vec<u8>>,    // 保存任务冻结的不可变作者画布。
    pub(super) resolution: [u32; 2],  // 延迟编码使用已经通过schema验收的声明尺寸。
}

pub(super) fn prepare_water_files(
    root: &Path,
    scene_id: &SceneId,
    scene_folder: &str,
    water: Option<&SceneWaterAsset>,
    dirty_layers: &BTreeSet<String>,
    masks: &BTreeMap<String, Arc<Vec<u8>>>,
    reserved_asset_paths: &BTreeSet<String>,
) -> Result<Vec<PreparedWaterFile>, String> {
    let Some(water) = water else {
        return Ok(Vec::new());
    };
    if water.schema_version != SCENE_SCHEMA_VERSION {
        return Err(format!(
            "场景{scene_id}水域版本{}与编辑器版本{}不一致",
            water.schema_version, SCENE_SCHEMA_VERSION
        ));
    }
    water.validate(scene_id)?; // 空路径、悬空端点、逆坡和超预算必须在首个atomic_write前拒绝。
    validate_spline_water_build_budget(water)?; // schema合法但超长的样条也不得落盘成运行时必失败资产。
    let mut files = Vec::new();
    let mut output_paths = reserved_asset_paths.clone(); // 本草稿全部非水资产先占位，水输出不得覆盖引用或权威文件。
    for layer in &water.layers {
        let source_path = safe_water_asset_path(root, scene_folder, &layer.mask_source_path)?;
        let texture_path = safe_water_asset_path(root, scene_folder, &layer.mask_texture_path)?;
        if !output_paths.insert(asset_path_collision_key(&source_path))
            || !output_paths.insert(asset_path_collision_key(&texture_path))
        {
            return Err(format!(
                "静水体{}的R8与KTX2输出路径和另一水域文件冲突",
                layer.id
            )); // 同层相等或跨层别名都必须在任何atomic_write前整批失败。
        }
        if !dirty_layers.contains(&layer.id) {
            continue; // 未改水层不编码，但仍必须保护其文件不被本次其他dirty输出覆盖。
        }
        let mask = masks
            .get(&layer.id)
            .cloned()
            .ok_or_else(|| format!("静水体{}缺少R8作者源", layer.id))?;
        let expected_len = usize::try_from(layer.resolution[0])
            .ok()
            .and_then(|width| {
                usize::try_from(layer.resolution[1])
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| format!("静水体{}的R8尺寸超出平台地址空间", layer.id))?;
        if mask.len() != expected_len {
            return Err(format!(
                "静水体{}的R8为{}字节，声明尺寸需要{}字节",
                layer.id,
                mask.len(),
                expected_len
            )); // 输入长度先验收，延迟编码不会因坏作者源在stage中途才暴露。
        }
        files.push(PreparedWaterFile {
            source_path,                  // 全部目标仍在事务首个stage前统一去重。
            texture_path,                 // KTX2路径已经通过私有目录和词法别名检查。
            mask,                         // Arc只冻结输入，不复制最大4096²画布。
            resolution: layer.resolution, // 单项stage时才生成本层完整Mip链。
        });
    }
    Ok(files)
}
