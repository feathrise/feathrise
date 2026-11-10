//! 多场景、多文件公共标签的内存表现夹具；调用正式同步系统，不加载或保存作者内容。

use bevy::prelude::*;
use super::*;

pub(crate) fn install_workspace_tab_fixture(app: &mut App) {
    app.init_resource::<VegetationPresetEditorState>()
        .init_resource::<SurfacePresetEditorState>()
        .init_resource::<WaterPresetEditorState>()
        .init_resource::<BuildingPresetEditorState>()
        .init_resource::<feathrise_editor_core::WorkspaceDomainState>()
        .init_resource::<crate::resources::SceneEditorState>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<Assets<bevy::gltf::Gltf>>()
        .init_resource::<Assets<bevy::gltf::GltfMesh>>();
    let first = app.world().resource::<crate::resources::SceneEditorState>().selected_scene_id.clone()
        .unwrap_or_else(|| feathrise_scene_content::SceneId::parse("MAP-workspace-snapshot-a").unwrap());
    let mut documents = crate::ui::scene_documents::SceneDocuments::default();
    documents.upsert(first.clone(), "测试场景 a".into(), false);
    documents.upsert(feathrise_scene_content::SceneId::parse("MAP-workspace-snapshot-b").unwrap(), "测试场景 b".into(), true);
    // 只建立UI所需的内存就绪门，不安装状态迁移或场景加载系统。
    app.insert_resource(documents)
        .insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(first.clone()) })
        .add_systems(Update, sync_workspace.in_set(crate::SceneEditorSystemSet::Presentation));
    app.world_mut().resource_mut::<crate::resources::SceneEditorState>().selected_scene_id = Some(first);
    crate::ui::asset_preview::install_snapshot_document_tabs(app);
    set_workspace_tab_fixture_context(app.world_mut(), false);
}

pub(crate) fn set_workspace_tab_fixture_context(world: &mut World, document_active: bool) {
    {
        let mut state = world.resource_mut::<VegetationPresetEditorState>();
        state.active_tab = if document_active { PresetWorkspaceTab::Document } else { PresetWorkspaceTab::Scene };
        state.touch_ui(); // 活动上下文通过正式换代门更新场景高亮和正文显隐。
    }
    crate::ui::asset_preview::set_snapshot_document_tabs_active(world, document_active);
}
