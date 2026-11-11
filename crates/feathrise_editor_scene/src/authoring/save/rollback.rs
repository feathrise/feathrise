//! 失败事务的正式水域缓存恢复输入选择。

use super::super::resources::WaterSaveHandoffSnapshot;

/// 只等待失败事务确实发布后回滚的重型输入；manifest和water JSON总是显式重载作联合屏障。
pub(super) struct RollbackWaterInputs {
    pub(super) manifest_path: String,
    pub(super) water_path: String,
    pub(super) mask_paths: Vec<String>,
    pub(super) texture_paths: Vec<String>,
    pub(super) height_path: Option<String>,
}

pub(super) fn select_rollback_water_inputs(
    snapshot: &WaterSaveHandoffSnapshot,
    rollback_paths: &[String],
) -> Option<RollbackWaterInputs> {
    if snapshot.water_asset_path.is_empty() || snapshot.catalog_registration_required || rollback_paths.is_empty() {
        return None; // 首次新增水体未进入旧正式目录；失败批次的最后catalog目标不可能先发布再失败。
    }
    let contains = |candidate: &str| rollback_paths.iter().any(|path| path == candidate);
    let mask_paths = snapshot
        .dirty_layers
        .iter()
        .filter(|(_, path, _)| contains(path))
        .map(|(_, path, _)| path.clone())
        .collect::<Vec<_>>();
    let texture_paths = snapshot
        .dirty_layers
        .iter()
        .filter(|(_, _, path)| contains(path))
        .map(|(_, _, path)| path.clone())
        .collect::<Vec<_>>();
    let height_path = snapshot
        .dirty_height_path
        .as_ref()
        .filter(|path| contains(path))
        .cloned();
    let affects_water = contains(&snapshot.manifest_asset_path)
        || contains(&snapshot.water_asset_path)
        || !mask_paths.is_empty()
        || !texture_paths.is_empty()
        || height_path.is_some();
    affects_water.then(|| RollbackWaterInputs {
        manifest_path: snapshot.manifest_asset_path.clone(),
        water_path: snapshot.water_asset_path.clone(),
        mask_paths,
        texture_paths,
        height_path,
    })
}

#[cfg(test)]
mod tests {
    use feathrise_scene_content::SceneId;

    use super::*;

    fn snapshot() -> WaterSaveHandoffSnapshot {
        WaterSaveHandoffSnapshot {
            scene_id: SceneId::parse("MAP-rollback-water").expect("测试场景ID合法"),
            water_revision: 4,
            root_water_revision: 4,
            terrain_revision: 3,
            baseline_generation: 8,
            manifest_asset_path: "scenes/MAP-rollback-water/scene.scene_manifest.json".to_owned(),
            water_asset_path: "scenes/MAP-rollback-water/custom.water.json".to_owned(),
            dirty_layers: vec![
                (
                    "lake".to_owned(),
                    "scenes/MAP-rollback-water/lake.r8".to_owned(),
                    "scenes/MAP-rollback-water/lake.ktx2".to_owned(),
                ),
                (
                    "pond".to_owned(),
                    "scenes/MAP-rollback-water/pond.r8".to_owned(),
                    "scenes/MAP-rollback-water/pond.ktx2".to_owned(),
                ),
            ],
            dirty_height_path: Some("scenes/MAP-rollback-water/height.r16".to_owned()),
            catalog_registration_required: false,
            terrains: Vec::new(),
        }
    }

    #[test]
    fn rollback_waits_only_published_masks_but_always_fences_manifest_and_water_json() {
        let selected = select_rollback_water_inputs(
            &snapshot(),
            &[
                "scenes/MAP-rollback-water/scene.scene_manifest.json".to_owned(),
                "scenes/MAP-rollback-water/lake.r8".to_owned(),
                "scenes/MAP-rollback-water/lake.ktx2".to_owned(),
            ],
        )
        .expect("正式水域的manifest回滚必须建立恢复门");

        assert_eq!(selected.mask_paths, ["scenes/MAP-rollback-water/lake.r8"]);
        assert_eq!(
            selected.texture_paths,
            ["scenes/MAP-rollback-water/lake.ktx2"]
        );
        assert!(selected.height_path.is_none());
        assert_eq!(
            selected.water_path,
            "scenes/MAP-rollback-water/custom.water.json"
        );
    }

    #[test]
    fn first_catalog_registration_does_not_expect_an_old_ready_water_batch() {
        let mut new_water = snapshot();
        new_water.catalog_registration_required = true;
        assert!(
            select_rollback_water_inputs(
                &new_water,
                &["scenes/MAP-rollback-water/scene.scene_manifest.json".to_owned()]
            )
            .is_none()
        );
    }
}
