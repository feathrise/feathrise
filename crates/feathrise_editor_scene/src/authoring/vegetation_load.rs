//! 植被正式输出和独立作者R8的IO加载边界。

use std::path::Path; // 受限assets根与作者相对路径拼接。
use std::sync::Arc; // 正式基线和一对一作者迁移共享像素直到首次落笔。

use super::files::safe_asset_path; // 所有作者路径必须留在assets根。
use super::image_io::{is_png_path, read_density_png}; // HN-01既有正式源继续支持PNG。
use super::load::read_exact_bytes; // 裸R8使用统一长度校验与诊断。
use super::resources::EditorSceneDraft; // 完整结果直接写入尚未发布的后台草稿。

/// 未登记的旧Clipmap只在IO任务读取标准私有文件；任何坏文件都不能被空白草稿覆盖。
pub(super) fn load_unregistered_clipmap_vegetation(
    root: &Path,
    draft: &mut EditorSceneDraft,
) -> Result<bool, String> {
    // 调用方已证明当前没有正式植被句柄。
    use feathrise_scene_content::{
        SCENE_SCHEMA_VERSION, SCENE_VEGETATION_FILE_NAME, SceneVegetationAsset,
    }; // 继续使用唯一植被作者格式。
    let relative = format!(
        "{}/{}",
        draft.terrain_folder().trim_end_matches('/'),
        SCENE_VEGETATION_FILE_NAME
    ); // 创建、加载与保存共享同一个协议路径。
    let path = super::files::safe_scene_private_asset_path(root, &draft.terrain_folder(), &relative)?; // 旧目录也不能逃逸当前场景私有根。
    let (mut vegetation, source_hash) = match std::fs::read(&path) {
        // 仅后台执行磁盘读取。
        Ok(bytes) => {
            // 既有作者内容优先，绝不靠文件缺登记就重新初始化。
            let value: SceneVegetationAsset = serde_json::from_slice(&bytes)
                .map_err(|error| format!("解析旧Clipmap植被文件{}失败：{error}", path.display()))?; // 损坏显式报错。
            (
                value,
                feathrise_scene_content::authoring_io::bytes_hash(&bytes),
            ) // 摘要与解析内容来自同一份原字节。
        } // 继续校验真实场景身份。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            SceneVegetationAsset {
                model_surfaces: Vec::new().into(),
                // 只有确切不存在才允许建立空内存草稿。
                schema_version: SCENE_SCHEMA_VERSION, // 新草稿使用当前版本。
                scene_id: draft.scene_id.clone(),       // 不借用其他场景的默认身份。
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
    if vegetation.scene_id != draft.scene_id || vegetation.schema_version != SCENE_SCHEMA_VERSION {
        // 同时验证版本与所属场景。
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
    if draft.manifest.terrains.is_some() && draft.terrain_documents.active.is_none() && !migrated {
        draft.terrain_documents.root_vegetation_unregistered_hash = Some(source_hash);
    } else {
        draft.vegetation_registration_source_hash = Some(source_hash);
    } // 空场景打开本身不构成修改；首次作者操作再移交已冻结的并发基线。
    Ok(migrated) // 供源加载判断是否从正式R8共享一对一作者基线。
} // 主线程仍只轮询完整草稿完成值。

/// 加载正式输出、冻结基线和作者画布；任一层失败时整张草稿不发布。
pub(super) fn load_vegetation_sources(
    root: &Path,
    draft: &mut EditorSceneDraft,
    migrated_authoring: bool,
) -> Result<(), String> {
    // 读取与身份校验仍在原有加载worker中完成；只有全部分块完整才发布作者画布。
    let folder = draft.terrain_folder().to_owned();
    if let Some(vegetation) = draft.vegetation.as_mut() {
        feathrise_scene_content::load_model_vegetation_blocks(root, &folder, vegetation)?;
    }
    // 模型根仅编译稀疏样本，不会生成二维正式R8；读取必须沿用编译端的同一载体资格。
    // 作者R8及模型块仍逐项严格读取，真正二维画布也不能用缺文件的零值兜底。
    let formal_layers = draft
        .vegetation
        .as_ref()
        .filter(|_| super::vegetation_compile::density_canvas_is_active(draft))
        .map(|vegetation| vegetation.layers.clone())
        .unwrap_or_default(); // 描述很小，避免遍历时与草稿像素表可变借用冲突。
    for layer in formal_layers {
        let expected = layer.resolution[0] as usize * layer.resolution[1] as usize; // 公共格式上限保证固定内存预算。
        let path = safe_asset_path(root, &layer.density_source_path)?; // 旧PNG和新R8都必须留在assets根。
        let density = if draft.vegetation_dirty_layers.contains(&layer.id) && !path.exists() {
            vec![0; expected] // 旧动态场景本次补齐的正式profile从空白画布开始。
        } else if is_png_path(&path) {
            read_density_png(&path, layer.resolution)? // HN-01继续逐纹素读取既有PNG权威源。
        } else {
            read_exact_bytes(&path, expected, "植被正式密度源")? // 动态场景裸R8严格校验长度。
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

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::{
        ModelSurfaceAnchor, ModelSurfaceVegetation, ModelSurfaceVegetationSample,
        SceneVegetationAuthorLayer, SceneVegetationAsset, SceneVegetationKind, SceneVegetationLayer,
    };

    /// 真正写入模型块、根JSON与作者R8，刻意不生成只供二维地形使用的正式R8。
    fn saved_model_fixture(model_root: bool) -> (std::path::PathBuf, EditorSceneDraft, SceneVegetationAsset, Vec<u8>) {
        let parent = std::env::temp_dir();
        let root = parent.join(format!("feathrise-model-root-load-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let mut draft = crate::authoring::test_support::test_clipmap_draft(Arc::new(vec![0; 121]), 1);
        if model_root {
            draft.manifest.terrains = Some(Arc::default());
        } else {
            // 独立Clipmap真正成为活动二维画布，不能借同文件的模型样本绕过正式R8校验。
            let definition = draft.manifest.terrains.as_ref().unwrap()[0].clone();
            draft.terrain_documents.active = Some(crate::authoring::terrain_documents::TerrainDocumentIdentity {
                manifest: definition.source_manifest(&draft.manifest),
                folder: definition.private_folder(&draft.scene_id), definition: Arc::new(definition),
            });
        }
        let folder = draft.terrain_folder().to_owned();
        let output = SceneVegetationLayer { id: "grass".into(), label: "草".into(), kind: SceneVegetationKind::Blade,
            runtime_profile: Some("ordinary".into()), runtime_layer_id: Some(1), prototype_id: None,
            asset_path: "grass.glb".into(), density_source_path: format!("{folder}/vegetation/grass.r8"),
            density_texture_path: format!("{folder}/vegetation/grass.ktx2"), resolution: [2, 2], spacing_meters: 0.25,
            scale_range: [1., 1.], max_instances: 4, seed: 1 };
        let mut authoring = crate::authoring::vegetation_defaults::empty_vegetation_authoring();
        let author_path = format!("{folder}/vegetation/author/author-grass.r8");
        authoring.layers.push(SceneVegetationAuthorLayer { id: "author-grass".into(), label: "草区域".into(),
            group_id: authoring.groups[0].id.clone(), output_layer_id: "grass".into(), density_source_path: author_path.clone(),
            resolution: [2, 2], enabled: true, locked: false, opacity: 1., priority: 0, weight: 1. });
        authoring.validate(std::slice::from_ref(&output)).unwrap();
        let mut model = ModelSurfaceVegetation::empty("scene-glb-plane", [7; 32], 0.25);
        model.samples.push(ModelSurfaceVegetationSample { anchor: ModelSurfaceAnchor {
            primitive: 0, triangle: 2, barycentric: [0.2, 0.3] }, layers: vec![("author-grass".into(), 200)] });
        let vegetation = SceneVegetationAsset { scene_id: draft.scene_id.clone(), layers: vec![output],
            authoring: Some(authoring), model_surfaces: vec![Arc::new(model)].into(), ..Default::default() };
        let (metadata, blocks) = feathrise_scene_content::prepare_model_vegetation_blocks(&folder, &vegetation).unwrap();
        for (relative, bytes) in blocks {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let author_pixels = vec![0, 64, 128, 255];
        let author_path = root.join(author_path);
        std::fs::create_dir_all(author_path.parent().unwrap()).unwrap();
        std::fs::write(author_path, &author_pixels).unwrap();
        let metadata_path = root.join(&folder).join(feathrise_scene_content::SCENE_VEGETATION_FILE_NAME);
        std::fs::write(&metadata_path, serde_json::to_vec(&metadata).unwrap()).unwrap();
        draft.vegetation = Some(serde_json::from_slice(&std::fs::read(metadata_path).unwrap()).unwrap());
        assert!(!root.join(&vegetation.layers[0].density_source_path).exists());
        (root, draft, vegetation, author_pixels)
    }

    fn remove_fixture(root: &Path) {
        // 只递归清理由本测试create_dir独占创建的临时根，先核对绝对父目录与本轮保留前缀。
        assert!(root.is_absolute());
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("feathrise-model-root-load-"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn model_root_reopens_real_blocks_and_author_r8_without_formal_density() {
        let (root, mut draft, original, author_pixels) = saved_model_fixture(true);
        let result = load_vegetation_sources(&root, &mut draft, false);
        remove_fixture(&root);
        result.expect("模型根不消费二维正式R8，真实模型块与作者源完整时必须可重开");
        let loaded = draft.vegetation.as_ref().unwrap();
        assert_eq!(loaded.model_surfaces, original.model_surfaces);
        assert_eq!(loaded.layers, original.layers);
        assert_eq!(loaded.authoring, original.authoring);
        assert_eq!(draft.vegetation_author_densities["author-grass"].as_slice(), author_pixels);
        assert!(draft.vegetation_densities.is_empty(), "模型根不凭空制造二维正式密度");
        assert!(draft.vegetation_dirty_layers.is_empty() && draft.vegetation_author_dirty_layers.is_empty());
    }

    #[test]
    fn active_clipmap_still_rejects_missing_formal_r8_even_with_model_blocks() {
        let (root, mut draft, _, _) = saved_model_fixture(false);
        let result = load_vegetation_sources(&root, &mut draft, false);
        remove_fixture(&root);
        assert!(result.unwrap_err().contains("植被正式密度源"), "真实二维画布缺源仍须失败关闭");
    }
}
