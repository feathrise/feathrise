//! 原路径只借用根应用提供的场景工厂；业务回显继续留在原调度。
use bevy::prelude::*;
use feathrise_editor_documents_model::DocumentScenes;
pub(crate) fn panel(scenes: &DocumentScenes) -> Box<dyn Scene> {
    (scenes.search_panel)()
}
