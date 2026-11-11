//! 中央植被预设标签条与活动详细分区的增量构造。
//!
//! 【功能设计】只有会话 `ui_revision` 变化时才替换动态实体；稳定帧只切换两个页面的 Display。
//! 活动正文一次仅构造一个分区、一个 LOD 和一个部件，作者目录与预设总量不进入每帧复杂度。

mod document; // 植被文档的页头和表单，不混入共享标签条重建逻辑。
mod notes;
mod sections; // Blade/Mesh各分区字段清单与结构动作。
mod widgets; // 动态标题、按钮、字段和只读行的统一视觉构件。 // 基础页可选多行备注与有限滚动视区，不参与资产引用。
#[cfg(test)]
mod tab_snapshot;
#[cfg(test)]
pub(crate) use tab_snapshot::{install_workspace_tab_fixture, set_workspace_tab_fixture_context};

pub(super) use self::document::draft_badge;
use self::document::spawn_editor;
use super::types::{
    PresetEditorAction, PresetEditorButton, PresetWorkspaceDynamicRoot, PresetWorkspaceEditorHost,
    PresetWorkspaceEditorPage, PresetWorkspaceScenePage, PresetWorkspaceTab, PresetWorkspaceTabHost,
    VegetationPresetEditorState,
}; // 工作区状态与静态挂载 marker。
use crate::ui::scene::FONT_PATH; // 动态实体复用同一中文字体资产。
use crate::ui::surface_preset_editor::{
    SurfacePresetEditorState, spawn_active_surface_editor, spawn_surface_tabs,
};
use bevy::prelude::*; // Commands、Local、Query 与 UI 组件。 // 地表与植被共用同一浏览器式中央标签条和正文页。
// 水体预设在同一中央区域提供四层波浪和两层微法线完整表单。
use crate::ui::building_presets::{
    BuildingPresetEditorState, spawn_active_building_editor, spawn_building_tabs,
};
use crate::ui::water_preset_editor::{
    WaterPresetEditorState, spawn_active_water_editor, spawn_water_tabs,
}; // 建筑空间语义与其他全局预设共享同一中央标签条和正文页。
// 三类预设共用文字自适应标签，不再分别维护固定宽按钮。
use crate::ui::workspace_tabs::{spawn_preset_tab, tab_list_node};
use feathrise_editor_core::WorkspaceDomainState;

/// 上一次已构造的工作区修订。
#[derive(Default)]
pub(super) struct WorkspaceUiCache {
    revision: u64, // 零值保证首次 Presentation 必定构造标签与正文。
    active_tab: Option<PresetWorkspaceTab>, // 区分同页轻量刷新和真实换页。
}

/// 同步场景/编辑页显隐，并在显式修订变化时原子替换动态 UI 子树。
#[allow(clippy::too_many_arguments)] // 四类查询分别证明页面、挂载点和动态根互不混淆。
pub(super) fn sync_workspace(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>, // 候选名称只读取当前根的八个可见网格。
    gltf_meshes: Res<Assets<bevy::gltf::GltfMesh>>, // 真实GLTF Mesh名称用于明确区分子资产。
    state: Res<VegetationPresetEditorState>,
    surface_state: Res<SurfacePresetEditorState>,
    water_state: Res<WaterPresetEditorState>,
    shared_content: (
        Res<BuildingPresetEditorState>, // 建筑草稿通过共享工作区修订触发原子正文换代。
        Option<Res<feathrise_editor_surface_model::SurfaceEditorExtension>>,
        ResMut<WorkspaceDomainState>,
    ), // 应用只读取中性重建事实，不把领域草稿带回普通编辑器。
    mut cache: Local<WorkspaceUiCache>,
    text_focus: (
        // 聚合焦点与树查询，保持Bevy系统参数数量在支持范围内。
        Res<bevy::input_focus::InputFocus>, // 全部预设共享官方焦点。
        Query<&ChildOf>,                    // 只保护中央正文后代，不冻结侧栏搜索。
        Query<(), With<crate::ui::text_input::EditorTextInput>>, // 只检查当前真实文本目标。
        crate::resources::SceneEditorWorldSession, // 空世界仍可编辑预设，但不展示场景页。
        Option<Res<crate::ui::scene_documents::SceneDocuments>>,
        Option<Res<crate::resources::SceneEditorState>>,
    ), // 同一SystemParam元组不增加额外实体查询。
    mut scene_pages: Query<
        &mut Node,
        (
            With<PresetWorkspaceScenePage>,
            Without<PresetWorkspaceEditorPage>,
        ),
    >,
    mut editor_pages: Query<
        &mut Node,
        (
            With<PresetWorkspaceEditorPage>,
            Without<PresetWorkspaceScenePage>,
        ),
    >,
    tab_hosts: Query<Entity, With<PresetWorkspaceTabHost>>,
    editor_hosts: Query<Entity, With<PresetWorkspaceEditorHost>>,
    dynamic_roots: Query<Entity, With<PresetWorkspaceDynamicRoot>>,
) {
    let (building_state, surface_extension, mut domains) = shared_content;
    let (focus, parents, text_inputs, world, scene_documents, scene_editor) = text_focus;
    let scene_active = state.active_tab == PresetWorkspaceTab::Scene;
    for mut node in &mut scene_pages {
        node.display = if scene_active && world.ready() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut editor_pages {
        node.display = if scene_active
            || matches!(
                state.active_tab,
                PresetWorkspaceTab::Document | PresetWorkspaceTab::AssetPreview
            ) {
            Display::None
        } else {
            Display::Flex
        };
    }
    let (Ok(tab_host), Ok(editor_host)) = (tab_hosts.single(), editor_hosts.single()) else {
        return; // 静态BSN壳尚未应用时不消费修订，下一帧仍会完成首次构造。
    };
    if cache.revision == state.ui_revision {
        return; // 稳定帧不重建任何标签、文字或字段实体。
    }
    if cache.active_tab == Some(state.active_tab)
        && focus.get().is_some_and(|entity| {
            text_inputs.contains(entity)
                && parents
                    .iter_ancestors(entity)
                    .any(|parent| parent == editor_host)
        })
    {
        // 同页输入期间保留真实编辑器实体。
        return; // 延缓标签/摘要修订，组合、选区、光标与在途粘贴均不被销毁。
    } // 换页或明确失焦后再采用最新修订。
    cache.active_tab = Some(state.active_tab); // 新动态页的身份。
    cache.revision = state.ui_revision;
    domains.presented_revision = state.ui_revision;
    for entity in &dynamic_roots {
        commands.entity(entity).despawn(); // Children 关系会递归释放本次动态子树。
    }
    let font = feathrise_editor_core::editor_font(&asset_server, FONT_PATH);
    spawn_tabs(
        &mut commands,
        tab_host,
        &font,
        &state,
        &surface_state,
        &water_state,
        &building_state,
        scene_documents.as_deref().map(|docs| {
            (
                docs,
                scene_editor
                    .as_ref()
                    .and_then(|editor| editor.selected_scene_id.as_ref()),
            )
        }),
    );
    if let Some(draft) = state.active_draft() {
        spawn_editor(
            &mut commands,
            editor_host,
            &font,
            draft,
            &state,
            &gltfs,
            &gltf_meshes,
        ); // 名称只为实际可见子资产读取，不遍历整个模型。
    } else {
        let spawned_surface = spawn_active_surface_editor(
            &mut commands,
            editor_host,
            &font,
            &surface_state,
            state.active_tab,
            surface_extension.as_deref(),
        );
        if !spawned_surface {
            let spawned_water = spawn_active_water_editor(
                &mut commands,
                editor_host,
                &font,
                &water_state,
                state.active_tab,
            );
            if !spawned_water {
                spawn_active_building_editor(
                    &mut commands,
                    editor_host,
                    &font,
                    &building_state,
                    state.active_tab,
                );
            } // 建筑表单不会被错误地交给水体编辑器。
        }
    }
}

/// 构造全部当前打开的预设标签；标签数量只等于作者实际打开的草稿数。
fn spawn_tabs(
    commands: &mut Commands,
    host: Entity,
    font: &Handle<Font>,
    state: &VegetationPresetEditorState,
    surface_state: &SurfacePresetEditorState,
    water_state: &WaterPresetEditorState,
    building_state: &BuildingPresetEditorState, // 独立建筑语义草稿列表。
    scene_documents: Option<(
        &crate::ui::scene_documents::SceneDocuments,
        Option<&feathrise_scene_content::SceneId>,
    )>,
) {
    commands.entity(host).with_children(|parent| {
        parent
            .spawn((
                PresetWorkspaceDynamicRoot,
                tab_list_node(), // 标签根保留自然宽度，超宽统一由外层横滚处理。
            ))
            .with_children(|tabs| {
                if let Some((documents, selected)) = scene_documents {
                    crate::ui::scene_documents::spawn_scene_tabs(
                        tabs,
                        font,
                        documents,
                        selected,
                        state.active_tab,
                    );
                }
                for tab in &state.tabs {
                    let active = state.active_tab == PresetWorkspaceTab::Vegetation(tab.id);
                    let dirty = if tab.draft.dirty() { "*" } else { "" };
                    spawn_preset_tab(
                        tabs,                                                // 继续按当前打开顺序加入中央标签条。
                        font, // 与编辑正文共用项目字体。
                        &format!("{}{}", tab.draft.descriptor.label, dirty), // 名称与未保存标记共同决定宽度。
                        PresetEditorButton(PresetEditorAction::ActivatePresetTab(tab.id)), // 原有激活动作不变。
                        PresetEditorButton(PresetEditorAction::ClosePresetTab(tab.id)), // 原有关闭保护不变。
                        active,                 // 当前活动标签使用统一亮铜外框。
                        tab.close_confirmation, // 未保存草稿仍须再次点击确认。
                    );
                }
                spawn_surface_tabs(tabs, font, surface_state, state.active_tab);
                spawn_water_tabs(tabs, font, water_state, state.active_tab);
                spawn_building_tabs(tabs, font, building_state, state.active_tab); // 建筑标签使用同一脏标记和关闭确认。
            });
    });
}

#[cfg(test)]
mod workbench_tests;

#[cfg(test)]
mod tests {
    use bevy::ecs::system::{IntoSystem, System};
    use bevy::prelude::World;

    use super::sync_workspace;

    #[test]
    fn workspace_sync_initializes_with_disjoint_page_queries() {
        let mut world = World::new();
        let mut system = IntoSystem::into_system(sync_workspace);
        system.initialize(&mut world); // 页面和挂载点查询若可能重叠，此处会以Bevy B0001失败。
    }
}
