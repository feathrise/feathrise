//! 作者工具拥有的固定参数；公共场景不依赖编辑器预览或缩略图通道。

/// 编辑器离屏样品渲染层；保留原编号，避免和共享环境或游戏私有层串画面。
pub mod render_layers {
    /// 右侧资产浏览器的 GLB/GLTF 预览模型、灯光和离屏相机共用层。
    pub const MAP_EDITOR_ASSET_PREVIEW_LAYER: usize = 14;

    /// 静水预设的独立 10m×50m 样品层，不显示当前地图或资产浏览器模型。
    pub const MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER: usize = 18;

    /// 资产缩略图串行烘焙层，仅当前生成静态图片的任务启用。
    pub const MAP_EDITOR_ASSET_THUMBNAIL_LAYER: usize = 20;
}
