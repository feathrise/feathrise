//! 地表预设只借用宿主的固定事实和受检操作，不复制场景草稿或持有 World。
use feathrise_editor_core::{
    EditorStatusKind, EditorStatusOperation, EditorStatusTarget, PresetWorkspaceTab,
};
use feathrise_scene_content::{SurfacePresetDescriptor, SurfacePresetRegistry};
pub trait SurfaceEditorHost {
    fn is_open(&self) -> bool;
    fn surface_busy(&self) -> bool;
    fn fallback_tab(&self) -> PresetWorkspaceTab;
    fn selected_asset(&self) -> Option<&str>;
    fn selected_preset(&self) -> Option<&str>;
    fn set_selected_preset(&mut self, value: Option<String>);
    fn set_status(&mut self, value: String);
    fn select_preset(
        &mut self,
        preset: &SurfacePresetDescriptor,
        registry: &SurfacePresetRegistry,
    ) -> Result<String, String>;
    fn draft_references(&self, names: &std::collections::BTreeSet<&str>) -> bool;
    fn record_operation_result(
        &mut self,
        operation: EditorStatusOperation,
        target: &EditorStatusTarget,
        kind: EditorStatusKind,
    );
    fn record_operation_feedback(
        &mut self,
        operation: EditorStatusOperation,
        target: &EditorStatusTarget,
        kind: EditorStatusKind,
        detail: std::sync::Arc<str>,
    );
}
