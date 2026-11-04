//! 自动GLB仅写当前地图私有摘要路径；先验证源与网格，再参加跨文件事务。
use super::{
    super::{files::safe_asset_path, resources::EditorMapDraft},
    transaction::PreparedWrite,
};
use crate::authoring::volume_sculpt::VolumeSaveFile;
use bevy::tasks::AsyncComputeTaskPool;
use feathrise_map_terrain::{
    parse_dynamic_terrain_glb,
    volume::sparse::{
        MAX_AUTHOR_VOLUME_GLB_BYTES, VolumeSource, build_author_volume_mesh,
        decode_author_volume_glb,
        paged::{PagedVolume, validate_roots},
    },
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read, path::Path, sync::Arc};

pub(super) struct VerifiedVolume {
    bytes: Arc<Vec<u8>>, // 同一不可变文件身份，IO阶段不能拿其他文件套用CPU证明。
    hash: String,
    footprint: Vec<[[f32; 2]; 2]>,
    paged: Option<PagedVolume>,
}

pub(super) async fn verify_async(files: &[VolumeSaveFile]) -> Result<Vec<VerifiedVolume>, String> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    if files.len() > feathrise_map_content::MAX_TERRAIN_CUTOUTS {
        return Err("三维保存文件超过32项预算".into());
    }
    let files = files.to_vec(); // 浅共享有界Arc集合；保存单飞槽一直保持到CPU与IO共同收口。
    AsyncComputeTaskPool::get()
        .spawn(async move { verify(&files) })
        .await
}

fn verify(files: &[VolumeSaveFile]) -> Result<Vec<VerifiedVolume>, String> {
    if files.len() > feathrise_map_content::MAX_TERRAIN_CUTOUTS
        || files
            .iter()
            .try_fold(0usize, |n, f| n.checked_add(f.bytes.len()))
            .is_none_or(|n| n > 64 * 1024 * 1024)
    {
        return Err("三维保存文件超过32项或64MiB总字节预算".into());
    }
    files
        .iter()
        .map(|file| {
            if file.bytes.len() > MAX_AUTHOR_VOLUME_GLB_BYTES {
                return Err("三维输出超过32MiB预算".into());
            }
            let hash: String = Sha256::digest(file.bytes.as_slice())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let source = decode_author_volume_glb(&file.bytes, Arc::default())?;
            if !matches!(source, VolumeSource::Paged(_))
                && build_author_volume_mesh(&source)? != parse_dynamic_terrain_glb(&file.bytes)?
            {
                return Err("三维密度源和GLB网格不一致，保存零写入停止".into());
            }
            Ok(VerifiedVolume {
                bytes: file.bytes.clone(),
                hash,
                footprint: source.footprint(),
                paged: if let VolumeSource::Paged(v) = source {
                    Some(v)
                } else {
                    None
                },
            })
        })
        .collect()
}

#[cfg(test)]
pub(super) fn prepare<'a>(
    root: &Path,
    draft: &EditorMapDraft,
    files: &[VolumeSaveFile],
) -> Result<Vec<PreparedWrite<'a>>, String> {
    prepare_verified(root, draft, files, &verify(files)?)
}

pub(super) fn prepare_verified<'a>(
    root: &Path,
    draft: &EditorMapDraft,
    files: &[VolumeSaveFile],
    verified: &[VerifiedVolume],
) -> Result<Vec<PreparedWrite<'a>>, String> {
    if files.len() > feathrise_map_content::MAX_TERRAIN_CUTOUTS || verified.len() != files.len() {
        return Err("三维保存文件预算或CPU证明数量无效".into());
    }
    let mut ids = BTreeSet::new();
    let mut outputs = Vec::new();
    for (file, proof) in files.iter().zip(verified) {
        let record = draft
            .placements
            .placements
            .iter()
            .find(|r| r.id == file.placement_id)
            .ok_or("三维输出没有当前摆放记录")?;
        if !ids.insert(&file.placement_id)
            || file.path != record.asset_path
            || !record
                .terrain_patch
                .as_ref()
                .is_some_and(|p| p.sculpted_volume)
            || file.bytes.len() > MAX_AUTHOR_VOLUME_GLB_BYTES
            || !Arc::ptr_eq(&file.bytes, &proof.bytes)
        {
            return Err("三维输出身份、作者类型或字节预算无效".into());
        }
        if file.path != format!("{}/volume_sculpt/v-{}.glb", draft.folder, proof.hash) {
            return Err("三维输出必须位于当前地图volume_sculpt私有目录，并绑定完整文件摘要".into());
        }
        let patch = record.terrain_patch.as_ref().unwrap();
        if patch.streamed_volume != proof.paged.is_some() {
            return Err("流送版本与作者声明不一致".into());
        }
        if let Some(paged) = &proof.paged {
            if patch.volume_cell_size != Some(paged.cell_size) {
                return Err("流送作者格距与版本不一致".into());
            }
            validate_roots(
                &safe_asset_path(root, &format!("{}/volume_sculpt/objects", draft.folder))?,
                paged,
            )?;
        }
        let actual: Vec<_> = patch
            .cutout
            .iter()
            .chain(&patch.cutouts)
            .map(|h| [h.min, h.max])
            .collect();
        if actual != proof.footprint {
            return Err("三维源与自动地表替换区域尺寸不一致".into());
        }
        let path = safe_asset_path(root, &file.path)?;
        if path.exists() {
            let existing = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            if existing.metadata().map_err(|e| e.to_string())?.len() != file.bytes.len() as u64 {
                return Err("三维摘要路径已经存在不同内容，禁止覆盖外部资产".into());
            }
            let mut bytes = Vec::new();
            existing
                .take(MAX_AUTHOR_VOLUME_GLB_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.as_slice() != file.bytes.as_slice() {
                return Err("三维摘要路径已经存在不同内容，禁止覆盖外部资产".into());
            }
            continue; // 完全相同的历史版本复用磁盘文件，避免无效热更。
        }
        outputs.push(PreparedWrite::shared(path, file.bytes.clone()));
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::save::{
        tests::SaveTransactionTestDirectory, transaction::commit_with_replace_hook,
    };
    use feathrise_map_terrain::volume::*;
    use std::sync::Arc;

    fn fixture() -> (EditorMapDraft, VolumeSaveFile) {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let source = volume_from_heights(16, 0.5, vec![4.; 17 * 17]).unwrap();
        let mesh = build_volume_mesh(&source).unwrap();
        let bytes = Arc::new(encode_volume_glb(&source, &mesh).unwrap());
        let hash: String = Sha256::digest(bytes.as_slice())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = format!("{}/volume_sculpt/v-{hash}.glb", draft.folder);
        let record = serde_json::from_value(serde_json::json!({"id":"volume-save","asset_path":path,"translation":[0,0,0],"yaw_degrees":0,"scale":[1,1,1],
            "terrain_patch":{"sculpted_volume":true,"cutout":{"min":[0,0],"max":[8,8]}}})).unwrap();
        draft.placements.placements.push(record);
        (
            draft,
            VolumeSaveFile {
                placement_id: "volume-save".into(),
                path,
                bytes,
            },
        )
    }
    #[test]
    fn volume_glb_and_placements_commit_together_and_source_can_be_decoded() {
        let temp = SaveTransactionTestDirectory::new();
        let (draft, file) = fixture();
        let mut writes = prepare(&temp.path, &draft, std::slice::from_ref(&file)).unwrap();
        let placement = temp.path.join(&draft.folder).join("placements.json");
        writes.push(PreparedWrite::json(placement.clone(), &draft.placements));
        commit_with_replace_hook(writes, |_, _| Ok(()))
            .unwrap_or_else(|e| panic!("体积联合事务失败：{}", e.message()));
        let bytes = std::fs::read(temp.path.join(&file.path)).unwrap();
        assert!(decode_volume_glb(&bytes).is_ok());
        let saved: feathrise_map_content::MapPlacementsAsset =
            serde_json::from_slice(&std::fs::read(placement).unwrap()).unwrap();
        assert_eq!(saved.placements[0].asset_path, file.path);
        assert!(prepare(&temp.path, &draft, &[file]).unwrap().is_empty()); // 不变的历史文件不重新触发热更。
    }
    #[test]
    fn volume_placement_failure_rolls_back_new_glb_without_leaving_ghost_reference() {
        let temp = SaveTransactionTestDirectory::new();
        let (draft, file) = fixture();
        let placement = temp.path.join(&draft.folder).join("placements.json");
        std::fs::create_dir_all(placement.parent().unwrap()).unwrap();
        std::fs::write(&placement, b"old-placements").unwrap();
        let mut writes = prepare(&temp.path, &draft, std::slice::from_ref(&file)).unwrap();
        writes.push(PreparedWrite::json(placement.clone(), &draft.placements));
        let error = commit_with_replace_hook(writes, |index, _| {
            if index == 2 {
                Err("模拟placement替换失败".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.rollback_complete());
        assert!(!temp.path.join(&file.path).exists());
        assert_eq!(std::fs::read(placement).unwrap(), b"old-placements");
    }
    #[test]
    fn volume_foreign_or_corrupt_source_fails_preflight_before_any_file_write() {
        let temp = SaveTransactionTestDirectory::new();
        let (mut draft, mut file) = fixture();
        file.path = "maps/other/volume_sculpt/v.glb".into();
        draft.placements.placements[0].asset_path = file.path.clone();
        assert!(prepare(&temp.path, &draft, &[file]).is_err());
        assert_eq!(std::fs::read_dir(&temp.path).unwrap().count(), 0);
        let (mut draft, mut file) = fixture();
        let proof = verify(std::slice::from_ref(&file)).unwrap();
        file.bytes = Arc::new((*file.bytes).clone());
        assert!(prepare_verified(&temp.path, &draft, std::slice::from_ref(&file), &proof).is_err());
        assert_eq!(std::fs::read_dir(&temp.path).unwrap().count(), 0); // 另一个字节身份不能借用前一CPU证明，哪怕路径相同。
        file.bytes = Arc::new(vec![0; 20]);
        let hash: String = Sha256::digest(file.bytes.as_slice())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        file.path = format!("{}/volume_sculpt/v-{hash}.glb", draft.folder);
        draft.placements.placements[0].asset_path = file.path.clone();
        assert!(prepare(&temp.path, &draft, &[file]).is_err());
        assert_eq!(std::fs::read_dir(&temp.path).unwrap().count(), 0);
    }

    #[test]
    fn volume_streamed_version_and_placements_save_rollback_reuse_objects_and_check_root_proof() {
        use feathrise_map_terrain::volume::sparse::{paged::*, *};
        let temp = SaveTransactionTestDirectory::new();
        let (mut draft, mut file) = fixture();
        draft.manifest.world_min = [-64.; 2];
        draft.manifest.world_max = [64.; 2];
        let source = apply_sparse_brush(
            &new_sparse_volume(0.25, Arc::default()).unwrap(),
            VolumeBrush {
                kind: VolumeBrushKind::Dig,
                from: [4., 0., 4.],
                to: [4., 0., 4.],
                radius: 2.,
                strength: 1.,
                ..Default::default()
            },
            bevy::math::Rect::from_corners(
                bevy::math::Vec2::splat(-64.),
                bevy::math::Vec2::splat(64.),
            ),
            |_| Ok(0.07),
        )
        .unwrap();
        let objects = temp.path.join(&draft.folder).join("volume_sculpt/objects");
        let paged = write_delta(
            &objects,
            &PagedVolume::new(0.25).unwrap(),
            prepare_delta(None, &source).unwrap(),
        )
        .unwrap();
        file.bytes = Arc::new(encode_paged_volume(&paged).unwrap());
        let hash: String = Sha256::digest(file.bytes.as_slice())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        file.path = format!("{}/volume_sculpt/v-{hash}.glb", draft.folder);
        let record = &mut draft.placements.placements[0];
        record.asset_path = file.path.clone();
        let patch = record.terrain_patch.as_mut().unwrap();
        patch.cutout = None;
        patch.cutouts = paged
            .footprint()
            .into_iter()
            .map(|h| feathrise_map_content::MapTerrainCutout {
                min: h[0],
                max: h[1],
            })
            .collect();
        patch.streamed_volume = true;
        patch.volume_cell_size = Some(0.25);
        let proof = verify(std::slice::from_ref(&file)).unwrap();
        let placement = temp.path.join(&draft.folder).join("placements.json");
        std::fs::write(&placement, b"old").unwrap();
        let mut writes =
            prepare_verified(&temp.path, &draft, std::slice::from_ref(&file), &proof).unwrap();
        writes.push(PreparedWrite::json(placement.clone(), &draft.placements));
        assert!(
            commit_with_replace_hook(writes, |i, _| if i == 2 {
                Err("模拟v3引用提交失败".into())
            } else {
                Ok(())
            })
            .unwrap_err()
            .rollback_complete()
        );
        assert_eq!(std::fs::read(&placement).unwrap(), b"old");
        assert!(!temp.path.join(&file.path).exists());
        assert!(
            build_resident_volume(load_resident_volume(&objects, &paged, [4., 0., 4.], 4).unwrap())
                .unwrap()
                .mesh
                .is_some()
        );
        let mut writes =
            prepare_verified(&temp.path, &draft, std::slice::from_ref(&file), &proof).unwrap();
        writes.push(PreparedWrite::json(placement.clone(), &draft.placements));
        commit_with_replace_hook(writes, |_, _| Ok(()))
            .unwrap_or_else(|e| panic!("{}", e.message()));
        assert!(
            prepare(&temp.path, &draft, std::slice::from_ref(&file))
                .unwrap()
                .is_empty()
        );
        draft.placements.placements[0]
            .terrain_patch
            .as_mut()
            .unwrap()
            .volume_cell_size = Some(0.125);
        assert!(prepare_verified(&temp.path, &draft, std::slice::from_ref(&file), &proof).is_err());
        let blocked = temp.path.join("missing");
        assert!(validate_roots(&blocked, &paged).is_err());
    }
    #[test]
    fn volume_sparse_glb_multiple_cutouts_save_reopen_and_transaction_rollback_are_atomic() {
        use feathrise_map_terrain::volume::sparse::*;
        let mut source = new_sparse_volume(0.25, Arc::default()).unwrap();
        let bounds = bevy::math::Rect::from_corners(
            bevy::math::Vec2::splat(-128.),
            bevy::math::Vec2::splat(128.),
        );
        for point in [[0.; 3], [40., 0., 0.]] {
            source = apply_sparse_brush(
                &source,
                VolumeBrush {
                    kind: VolumeBrushKind::Dig,
                    from: point,
                    to: point,
                    radius: 2.,
                    strength: 1.,
                    ..Default::default()
                },
                bounds,
                |_| Ok(0.),
            )
            .unwrap();
        }
        let source = VolumeSource::Sparse(source);
        let mesh = build_author_volume_mesh(&source).unwrap();
        let bytes = Arc::new(encode_author_volume_glb(&source, &mesh).unwrap());
        let hash: String = Sha256::digest(bytes.as_slice())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let (mut draft, mut file) = fixture();
        draft.manifest.world_min = [-128.; 2];
        draft.manifest.world_max = [128.; 2];
        file.path = format!("{}/volume_sculpt/v-{hash}.glb", draft.folder);
        file.bytes = bytes;
        let record = &mut draft.placements.placements[0];
        record.asset_path = file.path.clone();
        let patch = record.terrain_patch.as_mut().unwrap();
        patch.cutout = None;
        patch.cutouts = source
            .footprint()
            .into_iter()
            .map(|r| feathrise_map_content::MapTerrainCutout {
                min: r[0],
                max: r[1],
            })
            .collect();
        let temp = SaveTransactionTestDirectory::new();
        let mut stale = draft.clone();
        stale.placements.placements[0]
            .terrain_patch
            .as_mut()
            .unwrap()
            .cutouts
            .pop();
        assert!(prepare(&temp.path, &stale, std::slice::from_ref(&file)).is_err());
        assert_eq!(std::fs::read_dir(&temp.path).unwrap().count(), 0);
        let placement = temp.path.join(&draft.folder).join("placements.json");
        std::fs::create_dir_all(placement.parent().unwrap()).unwrap();
        std::fs::write(&placement, b"previous").unwrap();
        let mut writes = prepare(&temp.path, &draft, std::slice::from_ref(&file)).unwrap();
        writes.push(PreparedWrite::json(placement.clone(), &draft.placements));
        let error = commit_with_replace_hook(writes, |index, _| {
            if index == 2 {
                Err("模拟分块记录替换失败".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.rollback_complete());
        assert!(!temp.path.join(&file.path).exists());
        assert_eq!(std::fs::read(&placement).unwrap(), b"previous");
        let mut writes = prepare(&temp.path, &draft, std::slice::from_ref(&file)).unwrap();
        writes.push(PreparedWrite::json(placement.clone(), &draft.placements));
        commit_with_replace_hook(writes, |_, _| Ok(()))
            .unwrap_or_else(|e| panic!("{}", e.message()));
        let reopened = decode_author_volume_glb(
            &std::fs::read(temp.path.join(&file.path)).unwrap(),
            Arc::default(),
        )
        .unwrap();
        assert_eq!(source, reopened);
        let records: feathrise_map_content::MapPlacementsAsset =
            serde_json::from_slice(&std::fs::read(&placement).unwrap()).unwrap();
        assert_eq!(records.placements, draft.placements.placements);
        assert!(prepare(&temp.path, &draft, &[file]).unwrap().is_empty());
    }
}
