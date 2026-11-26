//! 只读检查器各段文字的排版身份，与原实例/网格选择协议分离。

use crate::types::EditorTool;
use crate::ui::gltf_browser::EditorGltfMeshSelection;
use feathrise_scene_content::SceneId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SelectedToolPropertiesKind {
    #[default]
    Surface,
    Portal,
    Boundary,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SceneInspectorField {
    #[default]
    Name,
    Details,
    Hint,
}

/// 只缓存一份小选择身份与状态；浏览器的有界构建进度Changed不应逐帧重排完整路径。
#[derive(Default)]
pub(crate) struct SceneInspectorObservation {
    pub(super) target: Option<crate::viewport::LayerTarget>,
    pub(super) multi_count: usize,
    pub(super) group: Option<String>,
    pub(super) main_glb: bool,
    pub(super) animation: bool,
    pub(super) author_category: Option<crate::ui::scene_layers::AuthorLayerCategory>,
    pub(super) scene_id: Option<SceneId>,
    pub(super) draft_scene_id: Option<SceneId>,
    pub(super) revision: u64,
    pub(super) structure_revision: u64,
    pub(super) saving: bool,
    pub(super) tool: EditorTool,
    pub(super) loading: bool,
    pub(super) placement_id: Option<String>,
    pub(super) mesh: Option<EditorGltfMeshSelection>,
    pub(super) record_index: Option<usize>,
    pub(super) relocation_armed: bool,
}
