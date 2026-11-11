//! 工程原生插件装配：工具行、中央文档、右侧唯一预览与原保存事务，领域实现全部在DLL/SO。
mod adapter;
mod components;
mod loader;
pub(crate) mod notifications;
pub(crate) mod management;
mod local_prefab;
mod scene_domain;
mod resources;
pub(crate) mod runtime_preview;
#[cfg(test)]
mod sidebar_tests;
mod systems;
#[cfg(test)]
mod tests;
mod types;
mod timeline;
mod ui;
#[cfg(feature = "validation")]
pub(crate) mod validation;
mod workbench;
use bevy::prelude::*;
use feathrise_editor_core::{SceneEditorSystemSet, PresetWorkspaceRebuildSet};
pub(crate) fn install(app: &mut App, project: &crate::project::Project, target: &std::path::Path) {
    management::install(app, project.root.clone());
    notifications::install(app);
    app.init_resource::<feathrise_editor_scene::game_authoring::DomainToolLabels>()
        .insert_resource(resources::NativeTools::start(project))
        .init_resource::<timeline::Gestures>()
        .add_systems(Update, systems::input.in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, timeline::input.after(systems::input).in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, timeline::tick.after(timeline::input).in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, systems::poll.in_set(SceneEditorSystemSet::Tasks))
        .add_systems(
            Update,
            (
                ui::mount,
                ui::append,
                workbench::append,
                workbench::scroll,
                ui::sync_buttons,
                timeline::sync,
            )
                .chain()
                .in_set(SceneEditorSystemSet::Presentation)
                .after(PresetWorkspaceRebuildSet),
        )
        .add_systems(
            Update,
            systems::preview.before(feathrise_editor_scene::game_authoring::AssetPreviewPrepare),
        );
    adapter::install(app);
    scene_domain::install(app, project);
    runtime_preview::install(app, project, target);
    local_prefab::install(app, project);
    #[cfg(feature = "validation")]
    validation::install(app, project);
}

/// 原关闭事务授权后确认当前工程的真实预览进程和通信线程全部收口。
pub(crate) fn ready_to_exit(world: &mut World) -> Result<bool, String> {
    if !management::ready_to_exit(world) { return Ok(false); }
    if !local_prefab::ready_to_exit(world) { return Ok(false); }
    if !scene_domain::ready_to_exit(world)? { return Ok(false); }
    runtime_preview::ready_to_exit(world)
}
