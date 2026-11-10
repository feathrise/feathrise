//! 场景视角只保存固定大小的观察元数据，独立于切换场景时回收的相机和作者草稿。

use crate::resources::{SceneEditorState, SceneEditorViewMemory, SceneEditorViewportState};
use crate::types::SceneEditorViewSnapshot;

/// 在RTT回收或真正采用另一个场景前保存旧视角，目录高亮不能改变其所属场景。
pub(super) fn remember_editor_viewport(
    memory: &mut SceneEditorViewMemory,
    editor: &SceneEditorState,
    viewport: &SceneEditorViewportState,
) {
    let Some(scene_id) = viewport.framed_scene_id.as_ref() else {
        return; // 尚未采用任何场景的默认相机不能创建场景观察记录。
    };
    memory.scenes.insert(
        scene_id.clone(), // 切换场景请求已可能把selected_scene_id指向目标，源身份只能取当前视口。
        SceneEditorViewSnapshot {
            view: editor.view,
            orthographic: editor.orthographic,
            center: viewport.center,
            extent: viewport.extent,
            yaw: viewport.yaw,
            pitch: viewport.pitch,
        },
    ); // 每次离开只更新一个固定大小记录，不扫描或复制场景数据。
}
