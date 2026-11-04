//! F12编辑时请求完整静态网格；显示LOD不能改变拾取、雕刻与保存所用的作者源。
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_scene::{MeshLodAuthoringState, MeshLodRuntimeStatus};
use crate::resources::MapEditorState;

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
        .add_systems(PostUpdate, sync.before(bevy::transform::TransformSystems::Propagate));
}

fn sync(editor: Res<MapEditorState>, mut lod: ResMut<MeshLodAuthoringState>) {
    let active = editor.open || editor.close_requested;
    if lod.active != active { lod.active = active; }
    // 关闭门完成之前仍保留完整几何；只改作者精度请求，不写地图、全局设置或历史。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoring_and_close_gate_require_original_geometry_without_mutating_preferences() {
        let mut app = App::new();
        app.init_resource::<MapEditorState>();
        install(&mut app);
        app.world_mut().resource_mut::<MapEditorState>().open = true;
        app.update();
        assert!(app.world().resource::<MeshLodAuthoringState>().active);
        app.world_mut().resource_mut::<MapEditorState>().open = false;
        app.world_mut().resource_mut::<MapEditorState>().close_requested = true;
        app.update();
        assert!(app.world().resource::<MeshLodAuthoringState>().active);
        app.world_mut().resource_mut::<MapEditorState>().close_requested = false;
        app.update();
        assert!(!app.world().resource::<MeshLodAuthoringState>().active);
    }
}
