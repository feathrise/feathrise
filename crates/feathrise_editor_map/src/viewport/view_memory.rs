//! 地图视角只保存固定大小的观察元数据，独立于切图时回收的相机和作者草稿。

use crate::resources::{MapEditorState, MapEditorViewMemory, MapEditorViewportState};
use crate::types::MapEditorViewSnapshot;

/// 在RTT回收或真正采用另一张地图前保存旧视角，目录高亮不能改变其所属地图。
pub(super) fn remember_editor_viewport(
    memory: &mut MapEditorViewMemory,
    editor: &MapEditorState,
    viewport: &MapEditorViewportState,
) {
    let Some(map_id) = viewport.framed_map_id.as_ref() else {
        return; // 尚未采用任何地图的默认相机不能创建地图观察记录。
    };
    memory.maps.insert(
        map_id.clone(), // 切图请求已可能把selected_map_id指向目标，源身份只能取当前视口。
        MapEditorViewSnapshot {
            view: editor.view,
            orthographic: editor.orthographic,
            center: viewport.center,
            extent: viewport.extent,
            yaw: viewport.yaw,
            pitch: viewport.pitch,
        },
    ); // 每次离开只更新一个固定大小记录，不扫描或复制地图数据。
}
