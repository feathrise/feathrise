//! 正式水域文件交接身份的原子建立。

use std::time::Duration;

use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::prelude::*;
use feathrise_map_content::{
    MapId, MapManifestAsset, MapWaterAsset, MapWaterMaskAsset,
};

use super::state::PlacementFileHandoffGate;
use super::{
    PendingPlacementFileHandoff, PendingWaterAsset, PendingWaterFileHandoff, WaterFileHandoff,
    WaterFileHandoffGate, WaterFileHandoffPurpose,
}; // 摆放与水域共用同一资源和关闭门，但保留各自公开证据。

impl WaterFileHandoff {
    /// 在正式摆放发布或显式重建请求之前登记严格的新Ready/Failed修订基线。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_placement(
        &mut self,
        map_id: MapId,
        author_revision: u64,
        baseline_revision: u64,
        baseline_attempt_revision: u64,
        baseline_progress_revision: u64,
        editor_previews: &mut Vec<super::PlacementPreviewHandoffSnapshot>,
        deleted: &mut Vec<super::PlacementDeletionSnapshot>,
        started_at: Duration,
    ) -> Result<(), String> {
        if self.placement.is_some() || self.preview_cleanup.is_some() {
            return Err("上一笔正式摆放交接尚未完成，不能被新保存事务覆盖".to_owned());
        }
        if let Some(water) = self.pending.as_ref()
            && (water.gate.map_id != map_id
                || water.gate.purpose != WaterFileHandoffPurpose::SavePublish)
        {
            return Err("当前正式水域恢复门与摆放保存身份不一致".to_owned());
        }
        if self
            .unresolved
            .as_ref()
            .is_some_and(|unresolved| unresolved.map_id != map_id)
        {
            return Err("另一地图仍有未解决的正式缓存，不能丢弃其恢复责任".to_owned());
        }
        self.placement = Some(PendingPlacementFileHandoff {
            gate: PlacementFileHandoffGate {
                map_id,                                             // 当前玩法地图的唯一正式摆放目标。
                author_revision,           // 成功提示区分保存后继续编辑的作者修订。
                baseline_revision,         // 新Ready必须严格越过建门时已采用值。
                baseline_attempt_revision, // 旧Failed不能冒充本次发布失败。
                last_progress_revision: baseline_progress_revision, // IO期间旧候选进度不能给发布后的门续命。
                last_progress_at: started_at, // 从建门时开始计算第一段无进展时间。
            },
            editor_previews: std::mem::take(editor_previews), // 所有失败检查后才移动，调用方错误路径仍保有原批次。
            deleted: std::mem::take(deleted), // 同一门接管完整删除责任；旧故障弱快照可释放而不撤销逻辑删除。
        });
        self.unresolved = None; // 合法新摆放门已完整冻结严格基线与当前预览，才接管旧摆放故障责任。
        Ok(())
    }

    /// 保存成功后等待manifest、water JSON、本批脏R8与可选兼容高度源全部进入新正式代次。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin(
        &mut self,
        asset_server: &AssetServer,
        map_id: MapId,
        water_revision: u64,
        terrain_revision: u64,
        baseline_generation: u64,
        manifest_asset_path: &str,
        water_asset_path: &str,
        dirty_layer_ids: &[String],
        dirty_mask_paths: &[String],
        dirty_texture_paths: &[String],
        _dirty_height_path: Option<&str>, // 仅保留调用兼容；动态地图高度不再属于水域文件交接。
        catalog_registration_required: bool,
        started_at: Duration,
    ) -> Result<(), String> {
        if dirty_layer_ids.len() != dirty_texture_paths.len() {
            return Err("待交接静水层的raw R8与KTX2身份数量不一致".to_owned());
        }
        self.begin_with_purpose(
            asset_server,
            WaterFileHandoffPurpose::SavePublish,
            map_id,
            water_revision,
            terrain_revision,
            baseline_generation,
            manifest_asset_path,
            water_asset_path,
            dirty_layer_ids,
            dirty_mask_paths,
            dirty_texture_paths,
            None,
            catalog_registration_required,
            started_at,
        )
    }

    /// 保存失败且磁盘完整回滚后，等待旧manifest/水输入重新覆盖可能的中间缓存。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_rollback(
        &mut self,
        asset_server: &AssetServer,
        map_id: MapId,
        water_revision: u64,
        terrain_revision: u64,
        baseline_generation: u64,
        manifest_asset_path: &str,
        water_asset_path: &str,
        mask_paths: &[String],
        texture_paths: &[String],
        _height_path: Option<&str>, // 旧回滚调用兼容；动态地图没有R16正式资产。
        started_at: Duration,
    ) -> Result<(), String> {
        let empty_layer_ids = vec![String::new(); mask_paths.len()]; // 回滚从未清脏，不承担作者层恢复责任。
        self.begin_with_purpose(
            asset_server,
            WaterFileHandoffPurpose::RollbackRestore,
            map_id,
            water_revision,
            terrain_revision,
            baseline_generation,
            manifest_asset_path,
            water_asset_path,
            &empty_layer_ids,
            mask_paths,
            texture_paths,
            None,
            false,
            started_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_with_purpose(
        &mut self,
        asset_server: &AssetServer,
        purpose: WaterFileHandoffPurpose,
        map_id: MapId,
        water_revision: u64,
        terrain_revision: u64,
        baseline_generation: u64,
        manifest_asset_path: &str,
        water_asset_path: &str,
        dirty_layer_ids: &[String],
        dirty_mask_paths: &[String],
        dirty_texture_paths: &[String],
        _dirty_height_path: Option<&str>,
        catalog_registration_required: bool,
        started_at: Duration,
    ) -> Result<(), String> {
        if self.pending.is_some() || self.placement.is_some() || self.preview_cleanup.is_some() {
            return Err("上一笔正式水域文件交接尚未完成，不能被新保存事务覆盖".to_owned());
        }
        if self
            .unresolved
            .as_ref()
            .is_some_and(|unresolved| unresolved.map_id != map_id)
        {
            return Err("另一地图仍有未解决的正式水域缓存，不能丢弃其恢复责任".to_owned());
        }
        if manifest_asset_path.is_empty() || water_asset_path.is_empty() {
            return Err("正式地图清单或水域JSON路径为空".to_owned());
        }

        let manifest = pending_asset::<MapManifestAsset>(asset_server, manifest_asset_path);
        let water = pending_asset::<MapWaterAsset>(asset_server, water_asset_path);
        let masks = dirty_mask_paths
            .iter()
            .map(|path| pending_asset::<MapWaterMaskAsset>(asset_server, path))
            .collect();
        let textures = dirty_texture_paths
            .iter()
            .map(|path| pending_numeric_image(asset_server, path))
            .collect();
        self.pending = Some(PendingWaterFileHandoff {
            gate: WaterFileHandoffGate {
                purpose,
                map_id,
                water_revision,
                terrain_revision,
                baseline_generation,
                started_at,
                all_inputs_succeeded_frame: None,
            },
            manifest,
            water: Some(water),
            masks,
            textures,
            dirty_layer_ids: dirty_layer_ids.to_vec(),
            catalog_registration_required,
            manifest_asset_path: manifest_asset_path.to_owned(),
            root_pending: true,
            terrains: None,
        });
        if purpose == WaterFileHandoffPurpose::SavePublish {
            self.adopt_water_save_handoff();
        } // 回滚只证明旧水缓存恢复；即使Ready，也不能接管先前失败保存的摆放或作者恢复责任。
        Ok(())
    }
}

pub(super) fn pending_numeric_image(asset_server: &AssetServer, path: &str) -> PendingWaterAsset<Image> {
    let handle = asset_server
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = ImageSampler::linear();
        })
        .load(path.to_owned()); // 与Clipmap运行时保持同一数值纹理设置和资产身份。
    PendingWaterAsset {
        id: handle.id(),
        _handle: handle,
        succeeded: false,
    }
}

pub(super) fn pending_asset<A: Asset>(asset_server: &AssetServer, path: &str) -> PendingWaterAsset<A> {
    let handle = asset_server.load::<A>(path.to_owned());
    PendingWaterAsset {
        id: handle.id(),
        _handle: handle,
        succeeded: false,
    }
}
