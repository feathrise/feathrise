//! JSON语法诊断、保留词元的格式化及保存前校验；共用一个不可抢占的后台槽。

use bevy::prelude::*;

mod components;
mod control;
#[cfg(test)]
mod format;
#[cfg(test)]
mod gpu_tests;
mod resources;
mod scene;
mod systems;
#[cfg(test)]
mod tests;
mod types;
#[cfg(test)]
mod validation;

#[cfg(test)]
pub(super) use gpu_tests::gpu_assert_tools;
pub(super) use resources::JsonTools;
pub(super) use systems::{before_save, drive};

/// 只有显式写入/格式化占用保存门；只读后台诊断不阻止批量快照校验。
pub(super) fn save_busy(documents: &super::resources::DocumentState) -> bool {
    documents.json.save.is_some() || documents.json.format.is_some() || documents.json.ready.is_some()
}

pub(super) fn supported_for_save(path: &str) -> bool { types::supported(path) }

/// 单文件和批量都使用同一流式语法规则；必须由后台CPU任务调用。
pub(super) fn validate_for_save(
    source: &str,
    content: feathrise_editor_documents_model::DocumentContentOperations,
) -> Result<(), String> {
    match (content.process_json)(source, false, false, None).error {
        Some(error) => Err(format!("{}；保存已停止，草稿保持", error.summary())),
        None => Ok(()),
    }
}

/// 跨工具GPU夹具只观察任务是否结清，不公开校验缓存或保存意图的内部结构。
#[cfg(test)]
pub(super) fn gpu_tools_idle(documents: &super::resources::DocumentState) -> bool {
    documents.json.task.is_none() && documents.json.save.is_none() && documents.io.is_none()
        && documents.current().is_some_and(|doc| documents.json.checked(doc).is_some())
}

pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        control::input
            .after(super::history::HistorySystemSet::Input)
            .after(super::navigation::NavigationSystemSet::Input)
            .before(crate::ui::actions::handle_editor_shortcuts)
            .in_set(crate::MapEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        drive
            .after(super::session::drive_tasks)
            .in_set(crate::MapEditorSystemSet::Tasks),
    )
    .add_systems(
        Update,
        (control::apply, scene::sync)
            .chain()
            .after(super::native::bind)
            .after(super::navigation::NavigationSystemSet::Navigate)
            .in_set(crate::MapEditorSystemSet::Presentation),
    )
    .add_systems(
        PostUpdate,
        control::reveal
            .after(super::navigation::NavigationSystemSet::Reveal)
            .before(bevy::ui_widgets::ImeSystems::UpdatePosition)
            .in_set(bevy::ui::UiSystems::PostLayout),
    );
}
