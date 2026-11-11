//! 文档中央标签与原位挂载；只移动已有UI子树，保留原生编辑器的光标、撤销和滚动。

use super::{components::*, resources::DocumentState};
use crate::components::SceneEditorPresetTabScroll;
use crate::ui::workspace_tabs::{spawn_preset_tab, tab_list_node};
use crate::ui::{scene::FONT_PATH, vegetation_preset_editor::VegetationPresetEditorState};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_editor_ui::{localization::EditorLocale, workspace_tabs::WorkspaceTabButton};

pub(super) fn sync(
    mut commands: Commands,
    state: Res<DocumentState>,
    locale: Option<Res<EditorLocale>>,
    server: Res<AssetServer>,
    workspace: Option<Res<VegetationPresetEditorState>>,
    replacement: Option<Res<super::search::AssetReplaceState>>,
    mut centers: Query<(Entity, &mut Node), With<DocumentCenterHost>>,
    sidebar: Query<Entity, With<super::super::types::AssetPreviewCanvas>>,
    panels: Query<(Entity, &ChildOf), With<DocumentPanel>>,
    hosts: Query<Entity, With<SceneEditorPresetTabScroll>>,
    roots: Query<Entity, With<DocumentTabRoot>>,
    tab_buttons: Query<(&DocumentAction, &Children), With<WorkspaceTabButton>>,
    mut tab_text: Query<&mut Text>,
    added_ui: Query<
        (),
        Or<(
            Added<DocumentPanel>,
            Added<DocumentCenterHost>,
            Added<SceneEditorPresetTabScroll>,
        )>,
    >,
    mut painted: Local<Option<Vec<(u64, String, bool, bool)>>>,
) {
    let locale_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    if !state.is_changed() && !locale_changed && added_ui.is_empty() && replacement.as_ref().is_none_or(|state| !state.is_changed()) {
        return; // 稳定帧不复制路径、不重挂父关系，也不重画标签。
    }
    for (_, mut node) in &mut centers {
        let display = if state.in_center() || replacement.as_ref().is_some_and(|state| state.preview_visible()) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let target = if state.expanded.is_some() {
        centers.single().ok().map(|(entity, _)| entity)
    } else {
        sidebar.single().ok()
    };
    if let Some(target) = target
        && let Ok((panel, parent)) = panels.single()
    {
        if parent.parent() != target {
            commands.entity(panel).insert(ChildOf(target)); // 移位只换父实体，不销毁正文或重置SourceKey。
        }
    }
    if workspace.is_none() {
        return;
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    let mut key: Vec<_> = state.tabs().map(|tab| {
        let dirty = state.documents.iter().any(|doc| doc.path == tab.path && doc.dirty());
        let active = state.central_active && replacement.as_ref().is_none_or(|state| !state.preview_visible())
            && state.expanded.as_ref().is_some_and(|current| current.id == tab.id);
        (tab.id, tab.path.clone(), dirty, active)
    }).collect();
    key.sort_by_key(|tab| tab.0); // 最多8项，切换活动文件不改变标签的打开顺序。
    let locale = locale.as_deref().copied().unwrap_or_default();
    if painted.as_ref() == Some(&key) && (key.is_empty() || !roots.is_empty()) {
        // 语言是呈现修订。原活动标签、关闭动作及其命中实体都继续使用同一身份。
        if locale_changed {
            for (action, children) in &tab_buttons {
                let DocumentAction::ActivateTab(id) = action else { continue };
                let Some((_, path, dirty, _)) = key.iter().find(|tab| tab.0 == *id) else { continue };
                for child in children.iter() {
                    if let Ok(mut text) = tab_text.get_mut(child) {
                        text.set_if_neq(Text::new(tab_caption(locale, path, *dirty)));
                    }
                }
            }
        }
        return;
    } // 只在路径、未保存状态或激活状态变化时重画标签。
    *painted = Some(key.clone());
    for root in &roots {
        commands.entity(root).despawn();
    }
    if key.is_empty() {
        return;
    }
    let font = feathrise_editor_core::editor_font(&server, FONT_PATH);
    commands.entity(host).with_children(|parent| {
        parent
            .spawn((DocumentTabRoot, RelativeCursorPosition::default(), tab_list_node())) // 右键覆盖标题与关闭键，祖先裁剪仍有效。
            .with_children(|row| {
                for (id, path, dirty, active) in key {
                    let label = tab_caption(locale, &path, dirty);
                    spawn_preset_tab(row, &font, &label,
                        (DocumentAction::ActivateTab(id), crate::ui::external_files::ExternalFileTab(path.clone()), RelativeCursorPosition::default()),
                        (DocumentAction::CloseTab(id), crate::ui::external_files::ExternalFileTab(path), RelativeCursorPosition::default()), active, false);
                }
            });
    });
}

fn tab_caption(locale: EditorLocale, path: &str, dirty: bool) -> String {
    let filename = path.rsplit('/').next().unwrap_or(path);
    locale.format("document.tab", &[("file", filename.into()), ("dirty", if dirty { " *" } else { "" }.into())])
}

/// 离屏布局夹具使用真实文件会话和标签同步；不启动加载、解析、保存或文件检查。
#[cfg(test)]
pub(crate) fn install_snapshot_document_tabs(app: &mut App) {
    use super::types::{DiskIdentity, Document, ExpandedDocument, ViewMode};
    use crate::ui::vegetation_preset_editor::PresetWorkspaceTab;
    use std::sync::Arc;
    let mut documents = Vec::new();
    for (path, source, dirty) in [
        ("config/工作台配置.json", "{\"布局\":\"紧凑\"}\n", true),
        ("说明/编辑说明.md", "# 编辑说明\n内存文档布局验证。\n", false),
    ] {
        let mut document = Document {
            path: path.into(), format: crate::asset_catalog::document_format(path).unwrap(),
            generation: 1,
            disk: DiskIdentity { path: std::path::PathBuf::from(path), digest: [0; 32], bom: false, crlf: false },
            pages: super::io::source_pages(source), source: Arc::from(source), revision: 0,
            saved_revision: 0, history: default(), preview: Arc::new(Vec::new()),
            preview_revision: None, preview_clipped: false, mode: ViewMode::Preview,
            preview_scroll: Vec2::ZERO, confirm_reload: false, external: default(), status: String::new(),
        };
        if dirty { document.edit("{\"布局\":\"紧凑\",\"修改\":true}\n"); }
        documents.push(document);
    }
    app.insert_resource(DocumentState {
        active: Some(documents[0].path.clone()), documents,
        expanded: Some(ExpandedDocument { id: 1, path: "config/工作台配置.json".into(), return_tab: PresetWorkspaceTab::Scene }),
        parked: vec![ExpandedDocument { id: 2, path: "说明/编辑说明.md".into(), return_tab: PresetWorkspaceTab::Scene }],
        next_tab: 2, ..default()
    }).add_systems(Update, sync.in_set(crate::SceneEditorSystemSet::Presentation));
}

/// 切换表现夹具的活动上下文，沿用正式会话回显，不直接重建标签实体。
#[cfg(test)]
pub(crate) fn set_snapshot_document_tabs_active(world: &mut World, active: bool) {
    world.resource_mut::<DocumentState>().central_active = active;
}
