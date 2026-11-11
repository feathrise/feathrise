//! 工程入口通过原场景进入事务定位作者场景；不自行装配场景、读取文件或生成替代预览。

use crate::{
    authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState},
    resources::{SceneBrowserState, SceneDeletionTask, SceneEditorState, SceneEditorViewportState},
    types::SceneBrowserItem,
};
use bevy::prelude::*;
use feathrise_scene_content::{ActiveDynamicScene, SceneCatalogAsset, SceneContentRegistry, SceneId};

/// 与原目录双击/“进入场景”调用同一受检请求；Ok仅表示已受理，不冒充场景或GPU就绪。
pub fn request_open_scene(world: &mut World, scene_id: &str) -> Result<(), String> {
    let scene_id = SceneId::parse(scene_id)?;
    let editor = world
        .get_resource::<SceneEditorState>()
        .ok_or("原编辑器尚未安装")?;
    if !editor.open || editor.new_scene_dialog_open || editor.close_requested {
        return Err("请在原编辑器可编辑且没有场景弹窗时进入场景".into());
    }
    if world
        .get_resource::<crate::resources::EditorTextInputGuard>()
        .is_some_and(|guard| guard.block_shortcuts)
        || world
            .get_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
            .is_some_and(|guard| guard.blocked())
        || world
            .get_resource::<SceneBrowserState>()
            .is_some_and(|browser| browser.dialog.is_some())
    {
        return Err("原作者输入或目录事务尚未收口".into());
    }
    if !world.contains_resource::<SceneDeletionTask>()
        || !world.contains_resource::<EditorAuthoringSession>()
        || !world.contains_resource::<GlobalEnvironmentAuthoringState>()
    {
        return Err("原场景作者事务尚未初始化".into());
    }
    let accepted = world.resource_scope(|world, mut editor: Mut<SceneEditorState>| {
        let registry = world
            .get_resource::<SceneContentRegistry>()
            .ok_or("工程场景目录尚未初始化")?;
        let catalog = world
            .get_resource::<Assets<SceneCatalogAsset>>()
            .and_then(|assets| assets.get(&registry.catalog))
            .ok_or("工程场景目录尚未成功加载")?;
        if !catalog.scenes.iter().any(|entry| entry.id == scene_id) {
            return Err(format!("当前工程不存在场景{scene_id}"));
        }
        editor.scene_list_selection_id = Some(scene_id.clone());
        crate::ui::rows::request_selected_scene_entry(
            &mut editor,
            world.resource::<SceneDeletionTask>(),
            Some(catalog),
            world.resource::<EditorAuthoringSession>(),
            world.resource::<GlobalEnvironmentAuthoringState>(),
            world.contains_resource::<crate::ui::scene_documents::SceneDocuments>(),
        );
        if editor.pending_scene_transition.as_ref() == Some(&scene_id)
            || editor.selected_scene_id.as_ref() == Some(&scene_id)
        {
            Ok(())
        } else {
            Err(editor.status.clone())
        }
    });
    if accepted.is_ok() {
        if let Some(mut browser) = world.get_resource_mut::<SceneBrowserState>() {
            browser.select_only(Some(SceneBrowserItem::Scene(scene_id)));
        }
    }
    accepted
}

/// 固定资源与当前相机核对真实原作者画布；不以目录选择或任意Camera3d存在当作3D就绪。
pub fn preview_ready_for(world: &World, scene_id: &str) -> bool {
    let Ok(scene_id) = SceneId::parse(scene_id) else {
        return false;
    };
    let Some(editor) = world.get_resource::<SceneEditorState>() else {
        return false;
    };
    if !editor.open
        || editor.scene_transition_in_progress
        || editor.pending_scene_transition.is_some()
        || editor.selected_scene_id.as_ref() != Some(&scene_id)
    {
        return false;
    }
    if world
        .get_resource::<State<feathrise_scene_core::state::ScenePhase>>()
        .is_none_or(|scene| *scene.get() != feathrise_scene_core::state::ScenePhase::Active)
        || world
            .get_resource::<ActiveDynamicScene>()
            .is_none_or(|active| active.scene_id.as_ref() != Some(&scene_id))
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
        .is_none_or(|draft| draft.scene_id != scene_id)
    {
        return false;
    }
    let Some(viewport) = world.get_resource::<SceneEditorViewportState>() else {
        return false;
    };
    viewport.framed_scene_id.as_ref() == Some(&scene_id)
        && viewport.render_size.x > 1
        && viewport.render_size.y > 1
        && world
            .get::<crate::components::SceneEditorCamera>(viewport.camera)
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
        .get_resource::<SceneEditorState>()
        .map(|editor| editor.status.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_navigation_uses_original_guard_and_only_queues_a_real_scene_request() {
        let mut world = World::new();
        world.insert_resource(SceneEditorState {
            open: true,
            ..default()
        });
        world.init_resource::<SceneDeletionTask>();
        world.init_resource::<EditorAuthoringSession>();
        world.init_resource::<GlobalEnvironmentAuthoringState>();
        let catalog: SceneCatalogAsset = serde_json::from_value(serde_json::json!({
            "schema_version":2,"scenes":[{"id":"MAP-native-navigation","display_name":"正式场景","folder":"scenes/MAP-native-navigation","published":true}]
        })).unwrap();
        let mut assets = Assets::<SceneCatalogAsset>::default();
        let catalog = assets.add(catalog);
        world.insert_resource(assets);
        world.insert_resource(SceneContentRegistry {
            catalog,
            revision: 1,
            ..default()
        });
        assert!(request_open_scene(&mut world, "MAP-missing").is_err());
        assert!(
            world
                .resource::<SceneEditorState>()
                .pending_scene_transition
                .is_none()
        );
        request_open_scene(&mut world, "MAP-native-navigation").unwrap();
        assert_eq!(
            world
                .resource::<SceneEditorState>()
                .pending_scene_transition
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
    fn closed_or_ime_busy_editor_cannot_request_a_different_scene() {
        let mut world = World::new();
        world.init_resource::<SceneEditorState>();
        assert!(
            request_open_scene(&mut world, "MAP-existing")
                .unwrap_err()
                .contains("可编辑")
        );
        world.resource_mut::<SceneEditorState>().open = true;
        world.insert_resource(crate::resources::EditorTextInputGuard {
            block_shortcuts: true,
        });
        assert!(
            request_open_scene(&mut world, "MAP-existing")
                .unwrap_err()
                .contains("输入")
        );
        assert!(
            world
                .resource::<SceneEditorState>()
                .scene_list_selection_id
                .is_none()
        );
    }
}
