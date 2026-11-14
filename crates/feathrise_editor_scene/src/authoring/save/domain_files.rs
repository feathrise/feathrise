//! 外部作者文件与首次目录登记加入原场景事务；源冲突在暂存前与正式替换前各检查一次。

use super::transaction::PreparedWrite;
use crate::authoring::EditorSceneDraft;
use feathrise_scene_content::authoring_io::{
    asset_path_collision_key, compare_content_hash, file_hash,
};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct DraftSourceSaveGuard {
    domain: Vec<(PathBuf, String)>, // 精确原字节版本，不与语义摘要混用。
    domain_target: Option<String>,  // 正文替换后不再比较旧源版本；路径键统一Windows普通/扩展拼写。
    vegetation: Option<(PathBuf, String)>, // 首次Clipmap补登记不能覆盖打开后外部修改的私有植被JSON。
    water: Option<(PathBuf, String)>,      // 首次Clipmap补登记同样保护既有或打开后新建的水域JSON。
    catalog: Option<(PathBuf, String)>,    // 首次登记防止覆盖外部目录编辑。
}

impl DraftSourceSaveGuard {
    pub(super) fn prepare(root: &Path, draft: &EditorSceneDraft) -> Result<Self, String> {
        let mut guard = Self::default(); // 各可选内容域共享同一个正式替换前守卫。
        let root_water = crate::authoring::terrain_documents::root_water::source(draft);
        let root_vegetation_hash = crate::authoring::terrain_documents::root_vegetation::source(draft)
            .and_then(|source| source.registration_source_hash); // 根槽停放时仍核验真正根文件。
        if let Some(expected) = root_vegetation_hash {
            // 旧Clipmap首次显式保存才增加源冲突检查。
            let path = root
                .join(&draft.folder)
                .join(feathrise_scene_content::SCENE_VEGETATION_FILE_NAME); // 与加载与保存使用同一标准私有文件。
            compare_content_hash(expected, &file_hash(&path)?)?; // stage前拒绝旧内容被外部修改或新增。
            guard.vegetation = Some((path, expected.clone())); // 替换前再次核对同一原字节。
        } // 已登记植被维持原保存行为。
        if let Some(expected) = root_water.registration_source_hash {
            let path = crate::authoring::files::safe_scene_private_asset_path(
                root,
                &draft.folder,
                root_water.asset_path,
            )?;
            compare_content_hash(expected, &file_hash(&path)?)?; // 原文件存在状态与SHA-256在任何暂存前核对。
            guard.water = Some((path, expected.clone()));
        }
        if root_vegetation_hash.is_some()
            || root_water.registration_required
            || draft.boundary_catalog_registration_required
        {
            // 任何新入口都不能丢并发目录编辑。
            let path = root.join(feathrise_scene_content::SCENE_CATALOG_ASSET_PATH); // 目录是事务最后写项。
            guard.catalog = Some((path.clone(), file_hash(&path)?)); // 冻结本批开始时完整目录版本。
        } // 后续编码合并最新目录，再次替换前检查仍未变化。
        if let Some(domain) = draft.domain.as_ref().filter(|domain| domain.dirty()) {
            let source = (domain.ops.guard)(root, &*domain.value)?;
            guard.domain_target = Some(asset_path_collision_key(
                &crate::authoring::files::safe_asset_path(root, domain.asset_path())?,
            ));
            guard.domain = source.files;
            if guard.catalog.is_none()
                && let Some(hash) = source.catalog_hash
            {
                guard.catalog = Some((
                    root.join(feathrise_scene_content::SCENE_CATALOG_ASSET_PATH),
                    hash,
                ));
            }
        }
        Ok(guard) // 返回全部需要保护的文档。
    }
    pub(super) fn check(&self, path: &Path) -> Result<(), String> {
        if let Some((target, expected)) = &self.vegetation
            && path == target
        {
            // 植被写入后不重复拿旧摘要比较，其他文件替换只校验尚未发布的目录。
            compare_content_hash(expected, &file_hash(target)?)?; // 防止stage期间的外部写入被覆盖。
        } // 失败由跨文件事务回滚已替换文件。
        if let Some((target, expected)) = &self.water
            && path == target
        {
            compare_content_hash(expected, &file_hash(target)?)?; // stage到正式替换之间出现的外部水域修改必须使整批回滚。
        }
        for (target, expected) in &self.domain {
            let key = asset_path_collision_key(target);
            if asset_path_collision_key(path) == key || self.domain_target.as_ref() != Some(&key) {
                compare_content_hash(expected, &file_hash(target)?)?;
            }
        }
        if let Some((target, expected)) = &self.catalog {
            compare_content_hash(expected, &file_hash(target)?)?;
        }
        Ok(()) // 最后目录替换之后不再调用钩子，检查的是同一原版本。
    }
}

pub(super) fn prepare_domain_write<'a>(
    root: &Path,
    draft: &'a EditorSceneDraft,
    writes: &mut Vec<PreparedWrite<'a>>,
) -> Result<Option<String>, String> {
    let Some(domain) = draft.domain.as_ref().filter(|domain| domain.dirty()) else {
        return Ok(None);
    };
    let (write, hash) = (domain.ops.prepare)(root, &*domain.value)?
        .ok_or("外部文档标记为未保存，但适配没有提供完整写项；为保护原文档，本次保存未开始")?;
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let expected = crate::authoring::files::safe_asset_path(&canonical_root, domain.asset_path())?;
    let actual = write
        .path
        .strip_prefix(root)
        .map(|relative| canonical_root.join(relative))
        .unwrap_or_else(|_| write.path.clone());
    if feathrise_scene_content::authoring_io::asset_path_collision_key(&actual)
        != feathrise_scene_content::authoring_io::asset_path_collision_key(&expected)
    {
        return Err("外部作者适配返回的保存目标与当前文档路径不一致；原文件未改动".into());
    }
    writes.push(write);
    Ok(Some(hash))
}
