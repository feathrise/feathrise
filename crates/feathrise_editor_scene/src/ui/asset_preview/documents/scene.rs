//! 原路径只借用根应用提供的场景工厂；业务回显继续留在原调度。
use bevy::prelude::*;
use feathrise_editor_documents_model::DocumentScenes;
pub(crate) fn panel(scenes: &DocumentScenes) -> Box<dyn Scene> {
    (scenes.panel)()
}
pub(crate) fn center_host(scenes: &DocumentScenes) -> Box<dyn Scene> {
    (scenes.center_host)()
}
pub(crate) fn sidebar_launch(scenes: &DocumentScenes) -> Box<dyn Scene> {
    (scenes.sidebar_launch)()
}
pub(crate) fn open_selected_button(scenes: &DocumentScenes) -> Box<dyn Scene> {
    (scenes.open_selected_button)()
}
