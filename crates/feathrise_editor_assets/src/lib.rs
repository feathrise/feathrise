//! 编辑器共享资产路径、原子写入与纹理派生；不拥有作者会话或主线程任务。
pub mod files;
pub mod glb_metadata;
pub mod ktx2;
pub use feathrise_scene_content::authoring_transaction::commit_owned_files;
