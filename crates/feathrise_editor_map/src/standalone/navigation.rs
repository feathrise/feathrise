//! 工程入口通过原地图进入事务定位作者地图；不自行装配场景、读取文件或生成替代预览。

use crate::{
    authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState},
    resources::{MapBrowserState, MapDeletionTask, MapEditorState, MapEditorViewportState},
    types::MapBrowserItem,
};
use bevy::prelude::*;
use feathrise_map_content::{ActiveDynamicMap, MapCatalogAsset, MapContentRegistry, MapId};

/// 与原目录双击/“进入地图”调用同一受检请求；Ok仅表示已受理，不冒充场景或GPU就绪。
pub fn request_open_map(world: &mut World, map_id: &str) -> Result<(), String> {
    let map_id = MapId::parse(map_id)?;
    let editor = world
        .get_resource::<MapEditorState>()
        .ok_or("原编辑器尚未安装")?;
    if !editor.open || editor.new_map_dialog_open || editor.close_requested {
        return Err("请在原编辑器可编辑且没有地图弹窗时进入地图".into());
    }
    if world
        .get_resource::<crate::resources::EditorTextInputGuard>()
        .is_some_and(|guard| guard.block_shortcuts)
        || world
            .get_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
            .is_some_and(|guard| guard.blocked())
        || world
            .get_resource::<MapBrowserState>()
            .is_some_and(|browser| browser.dialog.is_some())
    {
        return Err("原作者输入或目录事务尚未收口".into());
    }
    if !world.contains_resource::<MapDeletionTask>()
        || !world.contains_resource::<EditorAuthoringSession>()
        || !world.contains_resource::<GlobalEnvironmentAuthoringState>()
    {
        return Err("原地图作者事务尚未初始化".into());
    }
    let accepted = world.resource_scope(|world, mut editor: Mut<MapEditorState>| {
        let registry = world
            .get_resource::<MapContentRegistry>()
            .ok_or("工程地图目录尚未初始化")?;
        let catalog = world
            .get_resource::<Assets<MapCatalogAsset>>()
            .and_then(|assets| assets.get(&registry.catalog))
            .ok_or("工程地图目录尚未成功加载")?;
        if !catalog.maps.iter().any(|entry| entry.id == map_id) {
            return Err(format!("当前工程不存在地图{map_id}"));
        }
        editor.map_list_selection_id = Some(map_id.clone());
        crate::ui::rows::request_selected_map_entry(
            &mut editor,
            world.resource::<MapDeletionTask>(),
            Some(catalog),
            world.resource::<EditorAuthoringSession>(),
            world.resource::<GlobalEnvironmentAuthoringState>(),
            world.contains_resource::<crate::ui::map_documents::MapDocuments>(),
        );
        if editor.pending_map_transition.as_ref() == Some(&map_id)
            || editor.selected_map_id.as_ref() == Some(&map_id)
        {
            Ok(())
        } else {
            Err(editor.status.clone())
        }
    });
    if accepted.is_ok() {
        if let Some(mut browser) = world.get_resource_mut::<MapBrowserState>() {
            browser.select_only(Some(MapBrowserItem::Map(map_id)));
        }
    }
    accepted
}

/// 固定资源与当前相机核对真实原作者画布；不以目录选择或任意Camera3d存在当作3D就绪。
pub fn preview_ready_for(world: &World, map_id: &str) -> bool {
    let Ok(map_id) = MapId::parse(map_id) else {
        return false;
    };
    let Some(editor) = world.get_resource::<MapEditorState>() else {
        return false;
    };
    if !editor.open
        || editor.map_transition_in_progress
        || editor.pending_map_transition.is_some()
        || editor.selected_map_id.as_ref() != Some(&map_id)
    {
        return false;
    }
    if world
        .get_resource::<State<feathrise_scene_core::state::ScenePhase>>()
        .is_none_or(|scene| *scene.get() != feathrise_scene_core::state::ScenePhase::Active)
        || world
            .get_resource::<ActiveDynamicMap>()
            .is_none_or(|active| active.map_id.as_ref() != Some(&map_id))
        || world.contains_resource::<feathrise_scene_core::state::LoadingProgress>()
        || world
            .get_resource::<feathrise_scene_core::state::SceneStartupGate>()
            .is_some_and(|gate| !gate.ready)
    {
        return false;
    }
    if world
        .get_resource::<EditorAuthoringSession>()
        .and_then(|session| session.draft.as_ref())
        .is_none_or(|draft| draft.map_id != map_id)
    {
        return false;
    }
    let Some(viewport) = world.get_resource::<MapEditorViewportState>() else {
        return false;
    };
    viewport.framed_map_id.as_ref() == Some(&map_id)
        && viewport.render_size.x > 1
        && viewport.render_size.y > 1
        && world
            .get::<crate::components::MapEditorCamera>(viewport.camera)
            .is_some()
        && world.get::<Camera3d>(viewport.camera).is_some()
        && world
            .get::<Camera>(viewport.camera)
            .is_some_and(|camera| camera.is_active)
        && world
            .get::<bevy::camera::RenderTarget>(viewport.camera)
            .and_then(|target| target.as_image())
            .is_some_and(|image| image.id() == viewport._image.id())
        && world
            .get_resource::<Assets<Image>>()
            .is_some_and(|images| images.contains(viewport._image.id()))
}

/// 失败和等待沿用原状态栏原因，调用方不必解析或持有私有作者资源。
pub fn author_status(world: &World) -> Option<&str> {
    world
        .get_resource::<MapEditorState>()
        .map(|editor| editor.status.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_navigation_uses_original_guard_and_only_queues_a_real_map_request() {
        let mut world = World::new();
        world.insert_resource(MapEditorState {
            open: true,
            ..default()
        });
        world.init_resource::<MapDeletionTask>();
        world.init_resource::<EditorAuthoringSession>();
        world.init_resource::<GlobalEnvironmentAuthoringState>();
        let catalog: MapCatalogAsset = serde_json::from_value(serde_json::json!({
            "schema_version":1,"maps":[{"id":"MAP-native-navigation","display_name":"正式地图","folder":"maps/MAP-native-navigation","published":true}]
        })).unwrap();
        let mut assets = Assets::<MapCatalogAsset>::default();
        let catalog = assets.add(catalog);
        world.insert_resource(assets);
        world.insert_resource(MapContentRegistry {
            catalog,
            revision: 1,
            ..default()
        });
        assert!(request_open_map(&mut world, "MAP-missing").is_err());
        assert!(
            world
                .resource::<MapEditorState>()
                .pending_map_transition
                .is_none()
        );
        request_open_map(&mut world, "MAP-native-navigation").unwrap();
        assert_eq!(
            world
                .resource::<MapEditorState>()
                .pending_map_transition
                .as_ref()
                .unwrap()
                .as_str(),
            "MAP-native-navigation"
        );
        assert!(
            !world.contains_resource::<State<feathrise_scene_core::state::ScenePhase>>(),
            "受理请求不能伪造场景或就绪状态"
        );
        assert!(!preview_ready_for(&world, "MAP-native-navigation"));
    }

    #[test]
    fn closed_or_ime_busy_editor_cannot_request_a_different_map() {
        let mut world = World::new();
        world.init_resource::<MapEditorState>();
        assert!(
            request_open_map(&mut world, "MAP-existing")
                .unwrap_err()
                .contains("可编辑")
        );
        world.resource_mut::<MapEditorState>().open = true;
        world.insert_resource(crate::resources::EditorTextInputGuard {
            block_shortcuts: true,
        });
        assert!(
            request_open_map(&mut world, "MAP-existing")
                .unwrap_err()
                .contains("输入")
        );
        assert!(
            world
                .resource::<MapEditorState>()
                .map_list_selection_id
                .is_none()
        );
    }
}
