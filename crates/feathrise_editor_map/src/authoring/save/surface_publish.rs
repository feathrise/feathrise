//! 保存完成后的地表定义与权重纹理定向发布。
//!
//! 【功能设计】后台批次分别证明定义JSON与RGBA权重是否真实变化；主线程只冻结对应小型
//! 快照和路径，并在terrain.glb换代前沿唯一正式句柄发布，避免无变化高度保存制造地表修订。

use bevy::prelude::{AssetServer, Assets}; // 主线程只执行小型资产替换与显式异步重载。
use feathrise_map_content::{MAP_SURFACE_FILE_NAME, MapContentRegistry, MapId, MapSurfaceAsset}; // 正式文件名、唯一目录句柄与冻结地表定义。

use super::super::resources::EditorMapDraft; // 保存完成清脏前读取本批同源草稿快照。
use super::publish::publish_saved_surface_asset; // 定义真实变化时原位替换唯一正式资产。

/// 本次成功保存允许执行的全部地表发布动作；全部字段为空表示严格无操作。
#[derive(Default)]
pub(super) struct SavedSurfacePublish {
    definition_path: Option<String>, // 唯一句柄未就绪时才使用的权威JSON兜底路径。
    definition: Option<MapSurfaceAsset>, // 与磁盘事务同源的小型完整定义快照。
    weight_texture_path: Option<String>, // 只有RGBA作者权重真实改变时才存在。
    index_texture_path: Option<String>, // 索引与权重始终属于同一个发布世代。
    source_paths: Option<[String; 2]>, // 技能CPU响应读取raw，必须与KTX2共同显式热更。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_save_publishes_raw_pair_only_when_author_pixels_were_written() {
        let mut draft =
            crate::authoring::terrain::tests::test_heightfield_draft(std::sync::Arc::new(vec![
                0;
                121
            ]));
        draft.surface = Some(MapSurfaceAsset {
            schema_version: feathrise_map_content::MAP_SURFACE_SCHEMA_VERSION,
            map_id: draft.map_id.clone(),
            resolution: [1; 2],
            layers: Vec::new(),
            index_source_path: "maps/surface/indices.rgba32".into(),
            weight_source_path: "maps/surface/weights.rgba8".into(),
            index_texture_path: "maps/surface/indices.ktx2".into(),
            weight_texture_path: "maps/surface/weights.ktx2".into(),
        });
        assert!(
            SavedSurfacePublish::from_draft(&draft, false, false)
                .source_paths
                .is_none()
        );
        assert!(
            SavedSurfacePublish::from_draft(&draft, true, false)
                .source_paths
                .is_none()
        );
        assert_eq!(
            SavedSurfacePublish::from_draft(&draft, false, true).source_paths,
            Some([
                "maps/surface/indices.rgba32".into(),
                "maps/surface/weights.rgba8".into()
            ])
        );
    }
}

impl SavedSurfacePublish {
    /// 在草稿清脏前按后台返回的两项真实写入证据冻结发布输入。
    pub(super) fn from_draft(
        draft: &EditorMapDraft,
        definition_changed: bool,
        weights_changed: bool,
    ) -> Self {
        let definition = if definition_changed {
            draft.surface.clone() // 只克隆后台事务已经证明真实写盘的小型定义。
        } else {
            None // 高度保存不得克隆或发布未变化定义。
        };
        let definition_path = definition.as_ref().map(|_| {
            format!(
                "{}/{}",
                draft.folder.trim_end_matches('/'),
                MAP_SURFACE_FILE_NAME
            )
        }); // 只有事务确实写过JSON时才保留兜底重载路径。
        let weight_texture_path = if weights_changed {
            draft
                .surface
                .as_ref()
                .map(|surface| surface.weight_texture_path.clone()) // 权重脏证据只对应运行时KTX2。
        } else {
            None // 高度保存不得重载未变化权重纹理。
        };
        let index_texture_path = if weights_changed {
            draft
                .surface
                .as_ref()
                .map(|surface| surface.index_texture_path.clone())
        } else {
            None
        };
        let source_paths = weights_changed
            .then(|| {
                draft.surface.as_ref().map(|surface| {
                    [
                        surface.index_source_path.clone(),
                        surface.weight_source_path.clone(),
                    ]
                })
            })
            .flatten();
        Self {
            definition_path,     // 与定义快照成对存在。
            definition,          // 只含本批真正落盘的定义。
            weight_texture_path, // 只含本批真正重建的权重纹理。
            index_texture_path,  // 与权重完整同步热更。
            source_paths,
        }
    }

    /// 先发布定义再发布其权重纹理；调用方随后才允许重载terrain.glb。
    pub(super) fn publish(
        &self,
        registry: Option<&MapContentRegistry>,
        map_id: &MapId,
        surfaces: &mut Assets<MapSurfaceAsset>,
        asset_server: &AssetServer,
    ) {
        if let Some(snapshot) = self.definition.as_ref()
            && !publish_saved_surface_asset(registry, map_id, snapshot, surfaces)
            && let Some(path) = self.definition_path.as_ref()
        {
            asset_server.reload(path.clone()); // 唯一句柄尚未就绪时明确重新请求刚写完的定义。
        }
        if let Some(path) = self.weight_texture_path.as_ref() {
            asset_server.reload(path.clone()); // 只有真实地表笔划才显式发布新权重KTX2。
        }
        if let Some(path) = self.index_texture_path.as_ref() {
            asset_server.reload(path.clone()); // 权重不能与旧索引混用，同帧显式请求新派生图。
        }
        if let Some(paths) = self.source_paths.as_ref() {
            for path in paths {
                asset_server.reload(path.clone());
            } // 已武装保存票据；不用文件监听偶然次序完成CPU/GPU联合交接。
        }
    }
}
