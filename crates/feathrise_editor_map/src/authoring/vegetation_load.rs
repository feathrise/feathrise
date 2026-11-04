//! 植被正式输出和独立作者R8的IO加载边界。

use std::path::Path; // 受限assets根与作者相对路径拼接。
use std::sync::Arc; // 正式基线和一对一作者迁移共享像素直到首次落笔。

use super::files::safe_asset_path; // 所有作者路径必须留在assets根。
use super::image_io::{is_png_path, read_density_png}; // HN-01既有正式源继续支持PNG。
use super::load::read_exact_bytes; // 裸R8使用统一长度校验与诊断。
use super::resources::EditorMapDraft; // 完整结果直接写入尚未发布的后台草稿。

/// 未登记的旧Clipmap只在IO任务读取标准私有文件；任何坏文件都不能被空白草稿覆盖。
pub(super) fn load_unregistered_clipmap_vegetation(
    root: &Path,
    draft: &mut EditorMapDraft,
) -> Result<bool, String> {
    // 调用方已证明当前没有正式植被句柄。
    use feathrise_map_content::{
        MAP_SCHEMA_VERSION, MAP_VEGETATION_FILE_NAME, MapVegetationAsset,
    }; // 继续使用唯一植被作者格式。
    let relative = format!(
        "{}/{}",
        draft.terrain_folder().trim_end_matches('/'),
        MAP_VEGETATION_FILE_NAME
    ); // 创建、加载与保存共享同一个协议路径。
    let path = super::files::safe_map_private_asset_path(root, &draft.terrain_folder(), &relative)?; // 旧目录也不能逃逸当前地图私有根。
    let (mut vegetation, source_hash) = match std::fs::read(&path) {
        // 仅后台执行磁盘读取。
        Ok(bytes) => {
            // 既有作者内容优先，绝不靠文件缺登记就重新初始化。
            let value: MapVegetationAsset = serde_json::from_slice(&bytes)
                .map_err(|error| format!("解析旧Clipmap植被文件{}失败：{error}", path.display()))?; // 损坏显式报错。
            (
                value,
                feathrise_map_content::authoring_io::bytes_hash(&bytes),
            ) // 摘要与解析内容来自同一份原字节。
        } // 继续校验真实地图身份。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            MapVegetationAsset {
                // 只有确切不存在才允许建立空内存草稿。
                schema_version: MAP_SCHEMA_VERSION, // 新草稿使用当前版本。
                map_id: draft.map_id.clone(),       // 不借用其他地图的默认身份。
                layers: Vec::new(),                 // 不预建全目录密度图。
                authoring: Some(super::vegetation_defaults::empty_vegetation_authoring()), // 空作者组可直接接收第一张Blade画布。
            },
            "missing".to_owned(),
        ), // 首次保存前如果有人新建文件，冲突门会拒绝覆盖。
        Err(error) => {
            return Err(format!(
                "读取旧Clipmap植被文件{}失败：{error}",
                path.display()
            ));
        } // 权限或IO错误不能伪装成缺失。
    }; // 不在打开操作中创建文件或目录。
    if vegetation.map_id != draft.map_id || vegetation.schema_version != MAP_SCHEMA_VERSION {
        // 同时验证版本与所属地图。
        return Err(format!(
            "旧Clipmap植被文件{}身份或版本不一致",
            path.display()
        )); // 错误内容不进入作者草稿。
    } // 下一步只补缺失作者结构，不改变正式输出。
    let migrated =
        super::vegetation_defaults::ensure_vegetation_authoring(&mut vegetation, &draft.terrain_folder()); // 老正式密度仍走一对一无损迁移。
    vegetation
        .authoring
        .as_ref()
        .expect("上方已建立作者结构")
        .validate(&vegetation.layers)?; // 正式ID、作者引用、尺寸及组参数在读取R8前验证。
    for layer in &vegetation.layers {
        // 后台审核输出路径及散布参数，避免坏描述被首次保存正式登记。
        safe_asset_path(root, &layer.density_source_path)?; // 源必须处于assets根。
        safe_asset_path(root, &layer.density_texture_path)?; // 派生纹理也不能越界。
        if !layer.spacing_meters.is_finite()
            || layer.spacing_meters <= 0.0
            || layer
                .scale_range
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
            || layer.scale_range[0] > layer.scale_range[1]
        {
            // 与正式散布要求一致。
            return Err(format!("旧Clipmap植被层{}散布参数无效", layer.id)); // 不用默认参数掩盖坏作者文件。
        } // 既有Mesh记录仍保留，新增/复制入口另行拒绝该类型。
    } // 后续统一源加载验证像素长度。
    draft.vegetation = Some(vegetation); // 结果仍处于未发布的后台草稿。
    draft.vegetation_registration_source_hash = Some(source_hash); // 只有显式保存成功才解除登记责任。
    Ok(migrated) // 供源加载判断是否从正式R8共享一对一作者基线。
} // 主线程仍只轮询完整草稿完成值。

/// 加载正式输出、冻结基线和作者画布；任一层失败时整张草稿不发布。
pub(super) fn load_vegetation_sources(
    root: &Path,
    draft: &mut EditorMapDraft,
    migrated_authoring: bool,
) -> Result<(), String> {
    let formal_layers = draft
        .vegetation
        .as_ref()
        .map(|vegetation| vegetation.layers.clone())
        .unwrap_or_default(); // 描述很小，避免遍历时与草稿像素表可变借用冲突。
    for layer in formal_layers {
        let expected = layer.resolution[0] as usize * layer.resolution[1] as usize; // 公共格式上限保证固定内存预算。
        let path = safe_asset_path(root, &layer.density_source_path)?; // 旧PNG和新R8都必须留在assets根。
        let density = if draft.vegetation_dirty_layers.contains(&layer.id) && !path.exists() {
            vec![0; expected] // 旧动态地图本次补齐的正式profile从空白画布开始。
        } else if is_png_path(&path) {
            read_density_png(&path, layer.resolution)? // HN-01继续逐纹素读取既有PNG权威源。
        } else {
            read_exact_bytes(&path, expected, "植被正式密度源")? // 动态地图裸R8严格校验长度。
        };
        if draft
            .vegetation_densities
            .insert(layer.id.clone(), Arc::new(density))
            .is_some()
        {
            return Err(format!("植被正式层ID重复：{}", layer.id)); // 重复身份不能原子映射到运行时。
        }
    }

    let author_layers = draft
        .vegetation
        .as_ref()
        .and_then(|vegetation| vegetation.authoring.as_ref())
        .map(|authoring| authoring.layers.clone())
        .unwrap_or_default(); // 作者图层顺序由JSON稳定保存。
    for layer in author_layers {
        let expected = layer.resolution[0] as usize * layer.resolution[1] as usize; // 公共校验已限制4096²。
        let path = safe_asset_path(root, &layer.density_source_path)?; // 作者源同样不得越过assets根。
        let use_formal_baseline = !path.exists()
            && (migrated_authoring
                || draft
                    .vegetation_dirty_layers
                    .contains(&layer.output_layer_id)); // 本次新增一对一层从正式输出无损复制。
        let density = if use_formal_baseline {
            let formal = draft
                .vegetation_densities
                .get(&layer.output_layer_id)
                .ok_or_else(|| {
                    format!(
                        "植被作者层{}缺少正式基线{}",
                        layer.id, layer.output_layer_id
                    )
                })?; // 不用零图掩盖未知正式输出。
            if formal.len() != expected {
                return Err(format!(
                    "植被作者层{}分辨率与正式基线{}不一致",
                    layer.id, layer.output_layer_id
                )); // 一对一迁移必须逐纹素无损。
            }
            draft
                .vegetation_author_dirty_layers
                .insert(layer.id.clone()); // 保存后才创建私有作者R8。
            Arc::clone(formal) // 首次落笔通过Arc::make_mut写时复制。
        } else {
            Arc::new(read_exact_bytes(&path, expected, "植被作者R8源")?) // 已登记作者图缺文件时明确失败。
        };
        if draft
            .vegetation_author_densities
            .insert(layer.id.clone(), density)
            .is_some()
        {
            return Err(format!("植被作者层ID重复：{}", layer.id)); // 画布身份必须唯一。
        }
    }
    Ok(()) // 三套像素表全部完整后才由外层任务发布草稿。
}
