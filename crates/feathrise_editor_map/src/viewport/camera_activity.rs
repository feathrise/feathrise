//! 编辑相机启停不依赖作者文档门；关闭F12或切换驻留地图时立即归还玩法LOD观察权。

use bevy::prelude::*;

use crate::{
    authoring::EditorAuthoringSession,
    components::MapEditorCamera,
    resources::{MapEditorState, MapEditorViewportState},
};

pub(crate) fn sync_editor_camera_activity(
    editor: Res<MapEditorState>,
    viewport: Option<Res<MapEditorViewportState>>,
    session: Option<Res<EditorAuthoringSession>>,
    scenes: Option<Res<feathrise_scene::ResidentScenes>>,
    activity: Option<Res<feathrise_scene_core::residency::SceneResidencyActivity>>,
    host_page: Option<Res<crate::workbench::WorkbenchHostPageState>>,
    mut cameras: Query<(Entity, &mut Camera), With<MapEditorCamera>>,
) {
    // 位姿/射线系统受document_active保护，启停必须在该门之外运行；否则旧相机可一直抢占游戏LOD。
    let enabled = editor.open && !host_page.as_ref().is_some_and(|page| page.active)
        && crate::map_residency::document_active(session, scenes, activity);
    let selected = viewport.as_ref().map(|viewport| viewport.camera);
    for (entity, mut camera) in &mut cameras {
        let next = enabled && selected == Some(entity);
        if camera.is_active != next {
            camera.is_active = next;
            if next {
                // 驻留期间可能错过窗口/RTT尺寸事件；重新启用时让正式camera_system重取当前目标与投影。
                camera.computed.target_info = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (App, Entity, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<MapEditorState>()
            .init_resource::<feathrise_scene_core::residency::SceneResidencyActivity>()
            .add_systems(Update, sync_editor_camera_activity);
        let game = app.world_mut().spawn(Camera::default()).id();
        let editor = app.world_mut().spawn((MapEditorCamera, Camera::default())).id();
        let stale = app.world_mut().spawn((MapEditorCamera, Camera::default())).id();
        app.insert_resource(MapEditorViewportState {
            _image: Handle::default(), camera: editor, center: Vec3::ZERO,
            scene_center: Vec3::ZERO, scene_half_extents: Vec3::ONE, extent: 64.0,
            yaw: 0.0, pitch: 0.5, framed_map_id: None, render_size: UVec2::splat(512),
        });
        (app, game, editor, stale)
    }

    #[test]
    fn editor_camera_activity_closes_even_when_document_update_is_blocked() {
        let (mut app, game, editor, stale) = fixture();
        app.world_mut().resource_mut::<MapEditorState>().open = true;
        app.update();
        assert!(app.world().get::<Camera>(editor).unwrap().is_active);
        assert!(!app.world().get::<Camera>(stale).unwrap().is_active);
        app.world_mut().resource_mut::<feathrise_scene_core::residency::SceneResidencyActivity>().switching = true;
        app.world_mut().resource_mut::<MapEditorState>().open = false;
        app.update();
        assert!(!app.world().get::<Camera>(editor).unwrap().is_active);
        assert!(app.world().get::<Camera>(game).unwrap().is_active);
        // 模拟暖恢复还原了旧相机活动值，作者文档仍被恢复门阻止；同一独立系统必须再次撤销它。
        app.world_mut().get_mut::<Camera>(editor).unwrap().is_active = true;
        app.update();
        assert!(!app.world().get::<Camera>(editor).unwrap().is_active);
        app.world_mut().resource_mut::<MapEditorState>().open = true;
        app.update();
        assert!(!app.world().get::<Camera>(editor).unwrap().is_active);
        app.world_mut().resource_mut::<feathrise_scene_core::residency::SceneResidencyActivity>().switching = false;
        app.update();
        assert!(app.world().get::<Camera>(editor).unwrap().is_active);
        assert!(!app.world().get::<Camera>(stale).unwrap().is_active);
    }

    #[test]
    fn editor_camera_activity_without_viewport_cannot_claim_gameplay_lod() {
        let (mut app, game, editor, _) = fixture();
        app.world_mut().resource_mut::<MapEditorState>().open = true;
        app.world_mut().remove_resource::<MapEditorViewportState>();
        app.update();
        assert!(!app.world().get::<Camera>(editor).unwrap().is_active);
        assert!(app.world().get::<Camera>(game).unwrap().is_active);
    }
}
