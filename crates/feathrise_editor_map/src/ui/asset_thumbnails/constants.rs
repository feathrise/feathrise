//! 缩略图的固定容量、离屏尺寸和生命周期预算。

pub(super) const IMAGE_SIZE: u32 = 256;
pub(super) const CACHE_CAPACITY: usize = 64; // RGBA快照最多16MiB，原始模型和纹理不进入缓存。
pub(super) const NODE_STEPS_PER_FRAME: usize = 128;
pub(super) const MAX_NODES: usize = 8192;
pub(super) const MAX_MESHES: usize = 256;
pub(super) const MAX_VERTICES: usize = 500_000; // 计入节点重复实例，256px照片不提交整张高密度地图。
pub(super) const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
pub(super) const RECHECK_SECONDS: f64 = 2.0;
pub(super) const TIMEOUT_SECONDS: f64 = 30.0; // 使用真实时间，F12暂停游戏时也能退出坏资源。
pub(crate) const TILE_HEIGHT: f32 = 118.0; // 目录完整高度、虚拟窗口与定位共用实际卡片几何。
pub(crate) const TILE_GAP: f32 = 5.0;
