//! 工程文件打开桥：消费有工程身份的双击意图，复用中央文本会话。

use bevy::prelude::*;
use crate::{
    asset_catalog::{document_format, project_files::{EditorProjectFileCatalog, ProjectFileIdentity}},
    resources::MapEditorState,
    ui::vegetation_preset_editor::VegetationPresetEditorState,
    workbench::WorkbenchModel,
};
use super::{resources::DocumentState, source};

#[derive(Message, Clone)]
pub(crate) struct OpenProjectDocument { pub(crate) identity: ProjectFileIdentity }

pub(super) fn install(app: &mut App) {
    app.init_resource::<EditorProjectFileCatalog>();
    app.add_message::<OpenProjectDocument>()
        .add_systems(Update, open.after(crate::ui::project_browser::ProjectBrowserTaskSet)
            .after(crate::ui::project_browser::ProjectBrowserRoutingSet)
            .before(super::session::sync_selection).in_set(crate::MapEditorSystemSet::Tasks));
}

fn current(identity: &ProjectFileIdentity, catalog: &EditorProjectFileCatalog) -> bool {
    catalog.active && catalog.selected_file.as_ref() == Some(identity)
        && catalog.project_root() == Some(identity.project_root.as_path())
}

fn open(
    mut requests: MessageReader<OpenProjectDocument>,
    catalog: Option<Res<EditorProjectFileCatalog>>,
    model: Option<Res<WorkbenchModel>>,
    mut documents: ResMut<DocumentState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut editor: ResMut<MapEditorState>,
    sources: Query<&bevy::text::EditableText, With<super::components::DocumentSource>>,
) {
    let Some(identity) = requests.read().last().map(|request| request.identity.clone()) else { return; };
    let Some(catalog) = catalog else { return; };
    if !editor.open || editor.close_requested || editor.new_map_dialog_open
            || model.as_ref().is_some_and(|model| model.modal || model.busy)
            || !current(&identity, &catalog) { return; }
    if sources.iter().any(|text| text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()) {
        editor.status = "请先确认中文输入或等待粘贴完成，再打开工程文件".into();
        return;
    }
    let bound_paths = feathrise_editor_core::editor_project_paths();
    let (expected, assets) = if let Some(model) = &model {
        if model.context != identity.context { return; }
        (model.project_root.as_deref(), model.asset_root.as_deref())
    } else {
        (bound_paths.map(|paths| paths.project_root()), bound_paths.map(|paths| paths.asset_root()))
    };
    if expected != Some(identity.project_root.as_path()) { return; }
    let result = documents.bind_project_source(&identity.project_root, assets).and_then(|_| {
        source::project_key(documents.asset_root(), &identity.project_root, &identity.relative_path)
    });
    let key = match result { Ok(key) => key, Err(error) => { editor.status = error; return; } };
    if document_format(&identity.relative_path).is_some() {
        if let Err(error) = documents.expand(key, &mut workspace) { editor.status = error; }
    } else {
        editor.status = "此文件不支持内置文本编辑，可使用外部IDE打开".into();
    }
}
