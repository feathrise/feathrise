//! F12编辑时请求完整静态网格；显示LOD不能改变拾取、雕刻与保存所用的作者源。
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_scene::{MeshLodAuthoringState, MeshLodRuntimeStatus, MeshChunkLodPreview};
use crate::resources::SceneEditorState;

/// 显示恢复按帧分批，世界输入必须等待完整L0；列表/属性及相机导航不依赖该门。
#[derive(SystemParam)]
pub(crate) struct MeshLodInputGate<'w> {
    authoring: Option<Res<'w, MeshLodAuthoringState>>,
    runtime: Option<Res<'w, MeshLodRuntimeStatus>>,
}

impl MeshLodInputGate<'_> {
    pub(crate) fn ready(&self) -> bool {
        self.runtime.as_ref().is_none_or(|runtime| {
            self.authoring.as_ref().is_some_and(|authoring| authoring.active)
                && runtime.authoring_ready
        }) // 未安装静态LOD的独立作者应用沿用原行为；F12首帧不能把默认ready当作恢复完成。
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<MeshLodAuthoringState>()
        .init_resource::<MeshChunkLodPreview>()
        .add_systems(PostUpdate, sync.before(bevy::transform::TransformSystems::Propagate));
}

fn sync(editor: Res<SceneEditorState>, mut lod: ResMut<MeshLodAuthoringState>,
    mesh_edit: Option<Res<super::mesh_edit::MeshEditState>>,
    shading: Option<Res<crate::viewport::shading::ShadingState>>,
    mut preview: ResMut<MeshChunkLodPreview>) {
    let active = editor.open || editor.close_requested;
    if lod.active != active { lod.active = active; }
    // 只读对象观察允许独立分块显示；精确作者工具和真实作者线框继续等待完整L0。
    preview.active = editor.open && !editor.close_requested
        && editor.tool == crate::types::EditorTool::Inspect
        && mesh_edit.as_ref().is_none_or(|state| !state.owns_mouse())
        && shading.as_ref().is_none_or(|state| state.mode != crate::viewport::shading::ShadingMode::Wireframe);
    // 关闭门完成之前仍保留完整几何；只改作者精度请求，不写场景、全局设置或历史。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoring_and_close_gate_require_original_geometry_without_mutating_preferences() {
        let mut app = App::new();
        app.init_resource::<SceneEditorState>();
        install(&mut app);
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
        app.update();
        assert!(app.world().resource::<MeshLodAuthoringState>().active);
        assert!(app.world().resource::<MeshChunkLodPreview>().active);
        app.world_mut().resource_mut::<SceneEditorState>().open = false;
        app.world_mut().resource_mut::<SceneEditorState>().close_requested = true;
        app.update();
        assert!(app.world().resource::<MeshLodAuthoringState>().active);
        assert!(!app.world().resource::<MeshChunkLodPreview>().active);
        app.world_mut().resource_mut::<SceneEditorState>().close_requested = false;
        app.update();
        assert!(!app.world().resource::<MeshLodAuthoringState>().active);
    }

    #[test]
    fn chunk_preview_stops_for_editing_wireframe_and_brushes() {
        let mut app = App::new();
        app.init_resource::<SceneEditorState>()
            .init_resource::<super::super::mesh_edit::MeshEditState>()
            .init_resource::<crate::viewport::shading::ShadingState>();
        install(&mut app);
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
        app.update();
        assert!(app.world().resource::<MeshChunkLodPreview>().active);
        app.world_mut().resource_mut::<crate::viewport::shading::ShadingState>().mode = crate::viewport::shading::ShadingMode::Wireframe;
        app.update();
        assert!(!app.world().resource::<MeshChunkLodPreview>().active);
        app.world_mut().resource_mut::<crate::viewport::shading::ShadingState>().mode = crate::viewport::shading::ShadingMode::Rendered;
        app.world_mut().resource_mut::<SceneEditorState>().tool = crate::types::EditorTool::Vegetation;
        app.update();
        assert!(!app.world().resource::<MeshChunkLodPreview>().active);
        app.world_mut().resource_mut::<SceneEditorState>().tool = crate::types::EditorTool::Inspect;
        app.world_mut().resource_mut::<super::super::mesh_edit::MeshEditState>().activate_for_test();
        app.update();
        assert!(!app.world().resource::<MeshChunkLodPreview>().active);
    }
}
