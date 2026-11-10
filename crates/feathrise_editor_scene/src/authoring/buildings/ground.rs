//! 建筑五点、院落成员和保存审计共享的有界精确高度批次；缺页进度只在后台推进。
use super::types::BuildingGroundQuery;
use crate::authoring::placement_grounding::{PlacementTerrainSample, PlacementTerrainSampler};
use bevy::prelude::*;

pub(crate) fn controls_identity(
    controls: &crate::ui::building_presets::BuildingPlacementControls,
) -> [u32; 4] {
    [
        controls.yaw_degrees.to_bits(),
        controls.grid_step_meters.to_bits(),
        u32::from(controls.connection_snap),
        u32::from(controls.level_terrain),
    ]
}

/// 查询只冻结落点用途；最终根Y仍需同一次五点与占用校验。
pub(crate) fn cursor_target(
    editor: &crate::resources::SceneEditorState,
    controls: &crate::ui::building_presets::BuildingPlacementControls,
    catalog_revision: u64,
) -> Option<crate::viewport::vegetation_ground::GroundCursorTarget> {
    Some(
        crate::viewport::vegetation_ground::GroundCursorTarget::Building {
            preset_id: editor.building_selected_preset_id.clone()?,
            controls: controls_identity(controls),
            catalog_revision,
        },
    )
}

impl BuildingGroundQuery {
    /// 点集由既有CPU工作槽构造，主线程只移动容器，不能为整图建筑逐帧收集五点。
    pub(in crate::authoring) fn new(
        sampler: &PlacementTerrainSampler,
        points: impl IntoIterator<Item = Vec2>,
    ) -> Self {
        Self {
            content: sampler.clipmap_content_revision(),
            points: points
                .into_iter()
                .take(5 * feathrise_scene_content::MAX_SCENE_PLACEMENTS_PER_SCENE + 1)
                .collect(),
            ..default()
        }
    }

    pub(in crate::authoring) fn content(&self) -> Option<[u64; 7]> {
        self.content
    }

    /// 失败点序号用于院落指出成员；诊断也在原worker中完成。
    pub(in crate::authoring) fn next_point_index(&self) -> usize {
        self.cursor
    }

    /// 页到达或淘汰才重试；内容改变则直接拒收已取得的前缀。
    pub(in crate::authoring) fn can_retry(
        &self,
        sampler: &PlacementTerrainSampler,
    ) -> Result<bool, String> {
        self.can_retry_for(
            sampler.clipmap_content_revision(),
            sampler.authoritative_page_revision(),
        )
    }

    fn can_retry_for(
        &self,
        content: Option<[u64; 7]>,
        page: Option<(Option<u64>, bool)>,
    ) -> Result<bool, String> {
        if self.content != content {
            return Err("建筑校验期间地形内容已换代，请重新操作".into());
        }
        Ok(self.waiting_page != page)
    }

    /// 完整批次只保存精确点，不要求所有基础页同时驻留在64页缓存中。
    pub(in crate::authoring) fn prepare(
        &mut self,
        sampler: &PlacementTerrainSampler,
    ) -> Result<Option<PlacementTerrainSampler>, String> {
        if sampler.is_volume_layer() || sampler.is_object_surface(){return Ok(Some(sampler.clone()));} // 有界五点BVH查询由校验worker执行。
        if self.points.len() > 5 * feathrise_scene_content::MAX_SCENE_PLACEMENTS_PER_SCENE {
            return Err("建筑精确高度点集超过单图摆放预算".into());
        }
        if self.content.is_none() {
            return Ok(Some(sampler.clone()));
        } // GLB保留完整高度场协议。
        self.advance(
            sampler.clipmap_content_revision(),
            sampler.authoritative_page_revision(),
            |point| sampler.sample_status(point.x, point.y),
        )?;
        if self.cursor != self.points.len() {
            return Ok(None);
        }
        Ok(Some(PlacementTerrainSampler::from_clipmap_samples(
            self.content.unwrap(),
            std::mem::take(&mut self.heights),
        ).retain_volume_ground(sampler)))
    }

    fn advance(
        &mut self,
        content: Option<[u64; 7]>,
        page: Option<(Option<u64>, bool)>,
        mut sample: impl FnMut(Vec2) -> Result<PlacementTerrainSample, String>,
    ) -> Result<(), String> {
        if self.content != content {
            return Err("建筑校验期间地形内容已换代，请重新操作".into());
        }
        while let Some(point) = self.points.get(self.cursor).copied() {
            let key = (point.x.to_bits(), point.y.to_bits());
            if !self.heights.contains_key(&key) {
                match sample(point)? {
                    PlacementTerrainSample::Height(height) if height.is_finite() => {
                        self.heights.insert(key, height);
                    }
                    PlacementTerrainSample::Height(_) => return Err("建筑地基高度无效".into()),
                    PlacementTerrainSample::WaitingForHeight => {
                        self.waiting_page = page;
                        return Ok(());
                    }
                }
            }
            self.cursor += 1; // 已完成的精确点不因LRU淘汰重采，跨大范围院落也不会永久等待。
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests; // 分页进度和预算回归不启动GPU、不改变用户场景文件。

/// 完成值可能暂时等不到地表快照；确定换代与暂时等待必须分开处理。
pub(in crate::authoring) fn content_is_current(
    expected: Option<[u64; 7]>,
    draft: &crate::authoring::EditorSceneDraft,
    surface: Option<&feathrise_scene::DynamicClipmapGroundSurface>,
    terrain_revision: u64,
) -> Result<bool, String> {
    if expected.is_none() {
        return Ok(true);
    }
    if draft.manifest.terrains.is_some() {
        let Some(ground) = draft.terrain_ground.as_ref() else {
            return Ok(false);
        };
        if ground.scene_id != draft.scene_id {
            return Err("建筑校验期间场景或关联地形来源已换代".into()); // 同修订号不能跨场景借用审计信用。
        }
        if ground.revision != terrain_revision {
            return Ok(false);
        }
        return if Some([ground.revision, ground.generation, 0, 0, 0, 0, 0]) == expected {
            Ok(true)
        } else {
            Err("建筑校验期间关联地形集合已换代".into())
        };
    }
    let Some(snapshot) = surface.and_then(|surface| surface.snapshot.as_ref()) else {
        return Ok(false);
    };
    if snapshot.scene_id != draft.scene_id || Some(snapshot.content_revision_words()) != expected {
        return Err("建筑校验期间场景或地形来源已换代".into());
    }
    Ok(snapshot.ready && snapshot.authoring_height_revision() == Some(terrain_revision))
}
