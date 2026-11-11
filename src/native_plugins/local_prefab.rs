//! SDK中性作者外观的本地接入；只发布完整根给唯一资产预览，不构建工程或解释领域正文。
use super::resources::NativeTools;
use bevy::{camera::visibility::RenderLayers, prelude::*, tasks::{IoTaskPool, Task, futures::check_ready}};
use feathrise_runtime::project::author_visuals::{self as render, AuthorPrefabError, AuthorPrefabReady};
use feathrise_scene_content::author_visuals::{AuthorPrefabPreview, AUTHOR_PREFAB_PREVIEW_KIND};
use feathrise_editor_core::{AssetPreviewFocus, PresetWorkspaceTab};
use feathrise_editor_scene::game_authoring::scene_api::ui::{sidebars::*, vegetation_preset_editor::VegetationPresetEditorState};
use feathrise_editor_scene::game_authoring::scene_api::ui::asset_preview::AssetPreviewState;
use feathrise_editor_scene::game_authoring::scene_api::ui::asset_preview::types::AssetPreviewRetiredRoot;
use feathrise_editor_scene::game_authoring::scene_api::resources::SceneEditorState;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key { project: String, index: usize, document: u32, revision: u64 }
#[derive(Resource)]
struct Settings { project: String, root: PathBuf, assets: PathBuf }

#[derive(Resource, Default)]
pub(super) struct State {
    current: Option<Key>,
    attempted: Option<Key>,
    task: Option<Task<(Key, Result<Arc<AuthorPrefabPreview>, String>)>>,
    cached: Option<(Key, Arc<AuthorPrefabPreview>)>,
    root: Option<Entity>,
    scanned: bool,
    error: Option<String>,
}
impl State {
    pub(super) fn root(&self, index: usize, document: u32, revision: u64) -> Option<Entity> {
        self.current.as_ref().filter(|key| key.index == index && key.document == document && key.revision == revision)
            .and(self.root)
    }
    pub(super) fn caption(&self) -> String {
        if let Some(error) = &self.error { format!("本地外观预览失败：{error}") }
        else if self.scanned { "编辑器3D · 右键旋转 / 滚轮缩放".into() }
        else { "正在本地加载模型、骨架、附件与动画…".into() }
    }
    // 这些观测接口只供外部验收扩展使用，扩展的测试构建也必须保留。
    #[cfg(all(feature = "validation", feathrise_external_validation))]
    pub(super) fn validation_source(&self) -> Option<(usize, u32, u64, Entity, &AuthorPrefabPreview)> {
        let current = self.current.as_ref()?;
        let (key, source) = self.cached.as_ref().filter(|(key, _)| key == current)?;
        Some((key.index, key.document, key.revision, self.root?, source.as_ref()))
    }
    #[cfg(all(feature = "validation", feathrise_external_validation))]
    pub(super) fn validation_error(&self) -> Option<&str> { self.error.as_deref() }
}
pub(super) fn is_local(preview: &feathrise_plugin_api::RuntimePreview) -> bool {
    preview.kind == AUTHOR_PREFAB_PREVIEW_KIND
}
pub(super) fn install(app: &mut App, project: &crate::project::Project) {
    app.insert_resource(Settings { project: project.id.clone(), root: project.root.clone(), assets: project.context.asset_roots[0].clone() })
        .init_resource::<State>()
        .add_systems(Update, drive.before(super::systems::preview));
}
pub(super) fn ready_to_exit(world: &World) -> bool {
    // 关闭门先让drive撤回采用权；IO验证真实返回后才释放唯一后台槽。
    world.get_resource::<State>().is_none_or(|state| state.task.is_none() && state.root.is_none())
}
fn wanted(world: &World) -> Option<Key> {
    let settings = world.resource::<Settings>();
    let editor = world.resource::<SceneEditorState>();
    if !editor.open || editor.close_requested
        || feathrise_editor_core::editor_project_paths().is_some_and(|paths|
        paths.project_root() != settings.root || paths.asset_root() != settings.assets)
        || world.get_resource::<feathrise_scene_content::RuntimeAssetRoot>()
            .is_some_and(|root| root.asset_root() != settings.assets)
        || world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|close| close.active())
    { return None; }
    let sidebar = world.resource::<SceneEditorSidebarState>();
    let workspace = world.resource::<VegetationPresetEditorState>();
    if world.resource::<AssetPreviewFocus>().browsing(workspace.active_tab) { return None; }
    world.resource::<NativeTools>().tools.iter().find_map(|(&index, tool)| {
        if !sidebar.tool_page_visible(EditorLeftSidebarTab::Domain(tool.domain))
            || tool.snapshot.active == 0
            || tool.snapshot.workbench.is_some() && workspace.active_tab != PresetWorkspaceTab::Domain(tool.domain, tool.snapshot.active.into())
        { return None; }
        let preview = tool.snapshot.preview.as_ref()?;
        let local = preview.runtime.as_ref().filter(|preview| is_local(preview))?;
        (local.revision > 0 && local.revision == preview.revision).then(|| Key {
            project: settings.project.clone(), index, document: tool.snapshot.active, revision: local.revision,
        })
    })
}
fn prepare(assets: PathBuf, payload: serde_json::Value) -> Result<Arc<AuthorPrefabPreview>, String> {
    let preview: AuthorPrefabPreview = serde_json::from_value(payload).map_err(|error| error.to_string())?;
    preview.validate()?;
    // 最多一个身体、32个附件和一个动作；IO与链接包含关系检查均留在唯一后台槽。
    for path in preview.prefab.model.iter()
        .chain(preview.prefab.attachments.iter().map(|attachment| &attachment.model))
        .chain(preview.prefab.idle.iter().map(|animation| &animation.asset))
    {
        let source = feathrise_scene_content::authoring_io::safe_content_path(&assets, path)?;
        if !source.is_file() { return Err(format!("作者预览资产不存在：{path}")); }
    }
    Ok(Arc::new(preview))
}
fn withdraw(world: &mut World, state: &mut State) {
    if let Some(root) = state.root.take() {
        if let Some(mut visibility) = world.get_mut::<Visibility>(root) { *visibility = Visibility::Hidden; }
        if let Ok(mut entity) = world.get_entity_mut(root) {
            // 已交付的公共根与尚未交付的隐藏候选都只撤发布权，再交公共Last回收。
            entity.insert(AssetPreviewRetiredRoot);
        }
    }
    state.scanned = false;
    state.error = None;
}
fn drive(world: &mut World) {
    let current = wanted(world);
    world.resource_scope(|world, mut state: Mut<State>| {
        if state.current != current {
            withdraw(world, &mut state);
            state.current = current.clone();
            state.attempted = None;
        }
        // 被取消的后台准备仍占槽，直到真实完成；迟到结果不能产生实体或Assets副作用。
        if let Some((key, result)) = state.task.as_mut().and_then(check_ready) {
            state.task = None;
            if Some(&key) == current.as_ref() {
                match result {
                    Ok(prefab) => state.cached = Some((key, prefab)),
                    Err(error) => state.error = Some(error),
                }
            }
        }
        let Some(key) = current else { return; };
        if state.root.is_some_and(|root| world.get_entity(root).is_err()) { state.root = None; state.scanned = false; }
        if state.root.is_none() {
            if let Some((_, cached)) = state.cached.as_ref().filter(|(identity, _)| *identity == key) {
                let prefab = Arc::new(cached.prefab.clone());
                state.root = Some(render::spawn_prefab(world, prefab, RenderLayers::layer(
                    feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ASSET_PREVIEW_LAYER,
                )));
            } else if state.task.is_none() && state.attempted.as_ref() != Some(&key) {
                state.attempted = Some(key.clone());
                let payload = world.resource_mut::<NativeTools>().tools.get_mut(&key.index)
                    .and_then(|tool| tool.snapshot.preview.as_mut())
                    .and_then(|preview| preview.runtime.as_mut())
                    .map(|preview| std::mem::take(&mut preview.payload)).unwrap_or_default();
                let assets = world.resource::<Settings>().assets.clone();
                state.task = Some(IoTaskPool::get().spawn(async move { let result = prepare(assets, payload); (key, result) }));
            }
        }
        if let Some(root) = state.root {
            if let Some(error) = world.get::<AuthorPrefabError>(root) {
                state.error = Some(error.0.clone()); state.scanned = false;
            } else if world.get::<AuthorPrefabReady>(root).is_none() {
                state.scanned = false;
            } else if !state.scanned {
                state.error = None;
                let playback = state.cached.as_ref().map(|(_, preview)| (preview.playing, preview.speed));
                if let Some((playing, speed)) = playback {
                    render::set_prefab_playback(world, root, playing, speed);
                }
                // 身体、跨GLB动画与所有附件Ready后才复用公共联合范围扫描和唯一RTT揭示门。
                state.scanned = world.resource_mut::<AssetPreviewState>().begin_external_bounds_scan(root);
            }
        }
    });
}
pub(super) fn model(state: &State) -> Option<String> {
    state.cached.as_ref().filter(|(key, _)| Some(key) == state.current.as_ref())
        .and_then(|(_, preview)| preview.prefab.model.clone())
}

#[cfg(test)]
#[path = "local_prefab_tests.rs"]
mod tests;
