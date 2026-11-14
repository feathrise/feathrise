//! 场景保存事务的资产路径保留集与外部正文保护。
//!
//! 【功能设计】水域raw R8与KTX2可以沿用旧场景的安全相对路径，但在首次文件写入前必须
//! 与当前草稿引用或写入的JSON、R16、GLB、同名Blend、地表、植被和环境资产做统一词法碰撞检查。
//! 全批写项合并后还要拒绝覆盖保留的外部正文，含私有画布和体积生成，唯一领域自己的写项除外。

use std::collections::BTreeSet;
use std::path::Path;

use feathrise_scene_content::{
    SCENE_BOUNDARIES_FILE_NAME, SCENE_CATALOG_ASSET_PATH, SCENE_MANIFEST_FILE_NAME,
    SCENE_PLACEMENTS_FILE_NAME, SCENE_PLACEMENTS_GLB_FILE_NAME, SCENE_PORTALS_FILE_NAME, SCENE_SURFACE_FILE_NAME,
    SCENE_VEGETATION_FILE_NAME, SCENE_WATER_FILE_NAME, SceneTerrainKind,
};

use super::super::files::{asset_path_collision_key, safe_asset_path};
use super::super::resources::EditorSceneDraft;

/// 原始外部正文只允许由自己的已校验领域写项替换；普通作者输出不能借用其路径。
pub(super) fn validate_preserved_document_outputs(
    root: &Path,
    draft: &EditorSceneDraft,
    writes: &[super::transaction::PreparedWrite<'_>],
    domain_was_prepared: bool,
) -> Result<(), String> {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let key = |path: &Path| {
        // 外部正文服务使用规范根，普通作者沿用词法根；两者必须按同一真实根比较。
        let anchored = path
            .strip_prefix(root)
            .map(|relative| canonical_root.join(relative))
            .unwrap_or_else(|_| path.to_path_buf());
        feathrise_scene_content::authoring_io::asset_path_collision_key(&anchored)
    };
    let mut preserved = BTreeSet::new();
    for relative in draft
        .preserved_asset_paths
        .iter()
        .map(String::as_str)
        .chain(draft.domain.as_ref().map(|domain| domain.asset_path()))
    {
        preserved.insert(key(&safe_asset_path(root, relative)?));
    }
    let own_domain = if domain_was_prepared {
        let domain = draft
            .domain
            .as_ref()
            .ok_or("外部作者写项缺少对应文档身份")?;
        Some(key(&safe_asset_path(root, domain.asset_path())?))
    } else {
        None
    };
    let mut own_domain_adopted = false;
    for write in writes {
        let target = key(&write.path);
        if !preserved.contains(&target) {
            continue;
        }
        if own_domain.as_ref() == Some(&target) && !own_domain_adopted {
            own_domain_adopted = true; // 只有唯一的真实领域写项可以替换自己的正文。
            continue;
        }
        return Err(format!(
            "保存目标{}与保留的外部作者文档路径冲突；原文件未改动",
            write.path.display()
        ));
    }
    Ok(())
}

pub(super) fn collect_reserved_asset_paths(
    root: &Path,
    draft: &EditorSceneDraft,
) -> Result<BTreeSet<String>, String> {
    let mut paths = BTreeSet::new();
    let mut reserve = |relative: &str| -> Result<(), String> {
        let path = safe_asset_path(root, relative)?;
        paths.insert(asset_path_collision_key(&path)); // 多个非水消费者共享只读资产合法，集合只为阻止水保存覆盖。
        Ok(())
    };
    let folder = draft.folder.trim_end_matches('/');
    for file_name in [
        SCENE_MANIFEST_FILE_NAME,
        SCENE_PLACEMENTS_FILE_NAME,
        SCENE_PLACEMENTS_GLB_FILE_NAME,
        SCENE_PORTALS_FILE_NAME,
        SCENE_SURFACE_FILE_NAME,
        SCENE_VEGETATION_FILE_NAME,
        SCENE_BOUNDARIES_FILE_NAME,
        "characters.scene_characters.json", // 兼容协议保留的旧游戏正文不能被其他作者文件覆盖。
    ] {
        reserve(&format!("{folder}/{file_name}"))?; // 即使可选JSON当前不存在，也保留场景协议固定文件名。
    }
    reserve(SCENE_CATALOG_ASSET_PATH)?; // 首次登记水域/边界时同一事务可能更新全局目录。
    for path in &*draft.preserved_asset_paths { reserve(path)?; }
    if let Some(domain) = &draft.domain { reserve(domain.asset_path())?; } // 保护未解释的旧文件。

    let manifest = &draft.manifest;
    manifest.environment.validate()?; // 在派生固定IBL后缀前先证明路径扩展名、范围和天气字段都合法。
    if manifest.terrain.kind == SceneTerrainKind::SparseClipmap {
        if !manifest.terrain.glb_path.is_empty() {
            return Err("稀疏 Clipmap 场景不得同时保留可写 terrain.glb 路径".to_owned());
        } // 新图只保留独立稀疏高度清单，空GLB字段不是待保留资产路径。
    } else if !manifest.terrain.glb_path.is_empty() {
        reserve(&manifest.terrain.glb_path)?; // 已有Gltf场景继续保护其真实GLB权威源。
    }
    if let Some(objects) = &manifest.terrains { for object in objects.iter() {
        if !object.terrain.glb_path.is_empty() { reserve(&object.terrain.glb_path)?; }
        for path in [object.terrain.height_path.as_deref(), object.surface_path.as_deref(),
            object.vegetation_path.as_deref(), object.water_path.as_deref(), object.voxel.as_ref().map(|v| v.source_path.as_str())]
            .into_iter().flatten() { reserve(path)?; }
    } }
    if let Some(path) = manifest.terrain.height_path.as_deref() {
        reserve(path)?;
    }
    if let Some(path) = manifest.terrain.height_texture_path.as_deref() {
        reserve(path)?;
    }
    if let Some(path) = manifest.environment.skybox_path.as_deref() {
        reserve(path)?; // 天空资源主路径身份不能被场景水域保存事务覆盖。
        let stem = &path[..path.len() - ".ktx2".len()]; // 清单预检已保证安全KTX2主路径和ASCII扩展名。
        reserve(&format!("{stem}_diffuse.ktx2"))?; // 离线辐照图同样是运行时只读环境依赖。
        reserve(&format!("{stem}_specular.ktx2"))?; // 完整GGX Mip链同样不得与水域派生输出碰撞。
    }
    if let Some(surface) = draft.surface.as_ref() {
        reserve(&surface.weight_source_path)?;
        reserve(&surface.weight_texture_path)?;
        reserve(&surface.index_source_path)?;
        reserve(&surface.index_texture_path)?;
    }
    let root_vegetation = super::super::terrain_documents::root_vegetation::source(draft);
    for vegetation in draft.vegetation.iter().chain(root_vegetation.as_ref().and_then(|source|source.vegetation)) {
        for layer in &vegetation.layers {
            reserve(&layer.asset_path)?;
            reserve(&layer.density_source_path)?;
            reserve(&layer.density_texture_path)?;
        }
        if let Some(authoring) = vegetation.authoring.as_ref() {
            for layer in &authoring.layers {
                reserve(&layer.density_source_path)?;
            }
        }
        for surface in vegetation.model_surfaces.iter() {
            for block in &surface.sample_blocks { reserve(block)?; }
        }
    }
    for placement in &draft.placements.placements {
        if !placement.asset_target.is_builtin() {
            reserve(&placement.asset_path)?;
        } // 参数化基础形状全部保存在摆放JSON中，没有应保留或读取的磁盘模型路径。
    }
    drop(reserve); // 下方需要同时检查并更新同一保留集，先结束闭包的可变借用。
    if let Some(layer) = draft.manifest.puddles.as_ref() {
        // 积水不能覆盖任何未编辑的既有资产。
        layer.validate()?; // 路径类型与尺寸先通过公共协议。
        for relative in [&layer.source_path, &layer.texture_path] {
            // 独立作者源与派生图同时保留。
            let path = super::super::files::safe_scene_private_asset_path(root, folder, relative)?; // 只能写当前私有场景。
            if !paths.insert(asset_path_collision_key(&path)) {
                // Windows大小写别名也不能跨域覆盖。
                return Err(format!("积水路径{}与其他场景资产冲突", path.display())); // 首stage前零写入拒绝。
            } // 当前积水路径已保留。
        } // 两个配对输出共同进入后续水域碰撞门。
    } // 结束积水资产保护。
    let root_water = super::super::terrain_documents::root_water::source(draft);
    reserve_water_json_paths(
        root,
        folder,
        root_water.water.map(|_| root_water.asset_path),
        &mut paths,
    )?; // 实际water JSON不能与任何其他权威、派生或只读引用资产词法别名。
    Ok(paths)
}

fn reserve_water_json_paths(
    root: &Path,
    folder: &str,
    actual_water_path: Option<&str>,
    paths: &mut BTreeSet<String>,
) -> Result<(), String> {
    let default_path = safe_asset_path(root, &format!("{folder}/{SCENE_WATER_FILE_NAME}"))?;
    let default_key = asset_path_collision_key(&default_path);
    if let Some(actual_water_path) = actual_water_path {
        let actual_path = safe_asset_path(root, actual_water_path)?;
        let actual_key = asset_path_collision_key(&actual_path);
        if paths.contains(&actual_key) {
            return Err(format!(
                "水域JSON{}与场景另一权威、派生或引用资产路径冲突",
                actual_path.display()
            )); // 首个文件写入前拒绝custom water_path覆盖R16、GLB、地表或植被等资产。
        }
        paths.insert(actual_key); // custom与默认相同也合法，因为默认键直到此处才登记。
    }
    paths.insert(default_key); // 无论是否采用自定义路径，都保护协议默认名不被水层R8/KTX2复用。
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninterpreted_domain_document_still_protects_its_path_from_water() {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        draft.preserved_asset_paths = vec!["scenes/test/custom-author.json".to_owned()].into();
        draft.water = Some(crate::authoring::water_defaults::default_water_asset(
            &draft.scene_id,
        ));
        draft.water_asset_path = draft.preserved_asset_paths[0].clone();
        assert!(collect_reserved_asset_paths(Path::new("assets"), &draft).is_err()); // 无外部写项时同样拒绝水文件冒用保留路径。
    }

    #[test]
    fn sparse_clipmap_save_accepts_empty_glb_and_reserves_height_manifest() {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121])); // 复用完整有效草稿的其他作者域。
        draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap; // 新图由稀疏清单而非GLB承担高度权威。
        draft.manifest.terrain.glb_path.clear(); // 正式新图创建协议固定为空。
        draft.manifest.terrain.height_path =
            Some("scenes/test/terrain/height.sparse_height.json".to_owned()); // 联合保存仍须保护真实清单路径。

        let paths = collect_reserved_asset_paths(Path::new("assets"), &draft)
            .expect("稀疏图保存不应把空GLB字段解释为非法资产路径");
        let height = safe_asset_path(
            Path::new("assets"),
            draft.manifest.terrain.height_path.as_deref().unwrap(),
        )
        .unwrap(); // 使用生产路径规范化同一个清单身份。
        assert!(paths.contains(&asset_path_collision_key(&height))); // 水域等其他作者输出不能覆盖稀疏高度清单。
    }

    #[test]
    fn custom_water_json_cannot_alias_an_existing_non_water_asset() {
        let root = Path::new("assets");
        let terrain = safe_asset_path(root, "scenes/MAP-alias/terrain.glb").expect("测试路径合法");
        let mut paths = BTreeSet::from([asset_path_collision_key(&terrain)]);

        assert!(
            reserve_water_json_paths(
                root,
                "scenes/MAP-alias",
                Some("scenes/MAP-alias/terrain.glb"),
                &mut paths,
            )
            .is_err()
        );
    }

    #[test]
    fn protocol_default_water_path_is_not_a_false_self_collision() {
        let mut paths = BTreeSet::new();

        reserve_water_json_paths(
            Path::new("assets"),
            "scenes/MAP-default-water",
            Some("scenes/MAP-default-water/water.scene_water.json"),
            &mut paths,
        )
        .expect("实际路径等于协议默认路径应只登记一次");

        assert_eq!(paths.len(), 1);
    }
}
