//! 所选目录场景的只读属性同步；固定字段只借用内存清单，不扫描目录或进入场景。

use super::{components::*, types::{ScenePropertiesField, ScenePropertiesObservation}};
use crate::authoring::EditorAuthoringSession;
use crate::resources::{SceneBrowserState, SceneEditorState};
use crate::types::SceneBrowserItem;
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState};
use bevy::prelude::*;
use feathrise_scene_content::{SceneContentRegistry, SceneManifestAsset, SceneTerrainKind};

pub(super) fn sync_scene_properties(
    editor: Res<SceneEditorState>,
    browser: Res<SceneBrowserState>,
    sidebar: Res<SceneEditorSidebarState>,
    session: Res<EditorAuthoringSession>,
    registry: Option<Res<SceneContentRegistry>>,
    manifests: Option<Res<Assets<SceneManifestAsset>>>,
    added: Query<(), Added<ScenePropertiesText>>,
    mut texts: Query<(&ScenePropertiesText, &mut Text)>,
    mut rows: Query<(&ScenePropertiesRow, &mut Node)>,
    mut buttons: Query<(&mut ScenePropertiesEditBinding, &mut Node), Without<ScenePropertiesRow>>,
    mut scrolls: Query<(&crate::ui::sidebars::SceneEditorSidebarPage, &mut ScrollPosition)>,
    mut previous: Local<Option<ScenePropertiesObservation>>,
) {
    if !editor.open || sidebar.right != EditorRightSidebarTab::SceneEntry {
        return;
    }
    let selected = match browser.selected_item.as_ref() {
        Some(SceneBrowserItem::Scene(id)) if browser.selection_count() == 1 => Some(id),
        _ => None,
    };
    let handles = selected.and_then(|id| registry.as_ref()?.handles(id));
    let asset_id = handles.map(|handles| handles.manifest.id());
    let asset_tick = manifests.as_ref().map_or(0, |assets| assets.last_changed().get());
    let registry_revision = registry.as_ref().map_or(0, |registry| registry.revision);
    let draft_scene = session.draft.as_ref().map(|draft| &draft.scene_id);
    if previous.as_ref().is_some_and(|old| old.scene_id.as_ref() == selected
        && old.current_scene == editor.selected_scene_id && old.selection_pending == browser.selection_pending
        && old.manifest == asset_id && old.asset_tick == asset_tick && old.registry_revision == registry_revision
        && old.draft_revision == session.revision && old.draft_scene.as_ref() == draft_scene
        && old.loading == editor.scene_transition_in_progress) && added.is_empty() {
        return;
    }
    if previous.as_ref().is_none_or(|old| old.scene_id.as_ref() != selected) {
        for (page, mut scroll) in &mut scrolls {
            if page.0 == crate::ui::sidebars::EditorSidebarTab::Right(EditorRightSidebarTab::SceneEntry) {
                scroll.0 = Vec2::ZERO;
            }
        }
    }
    *previous = Some(ScenePropertiesObservation { scene_id: selected.cloned(), current_scene: editor.selected_scene_id.clone(),
        selection_pending: browser.selection_pending, manifest: asset_id, asset_tick, registry_revision,
        draft_revision: session.revision, draft_scene: draft_scene.cloned(), loading: editor.scene_transition_in_progress });
    let saved = handles.and_then(|handles| manifests.as_ref()?.get(&handles.manifest))
        .filter(|manifest| Some(&manifest.id) == selected);
    // 当前场景显示同身份草稿；浏览另一个场景时只能读取那个场景的正式资产。
    let manifest = session.draft.as_ref()
        .filter(|draft| Some(&draft.scene_id) == selected && editor.selected_scene_id.as_ref() == selected)
        .map(|draft| &draft.manifest)
        .or(saved);
    let manifest_path = handles.and_then(|handles| handles.manifest.path()).map(ToString::to_string);
    for (mut binding, mut node) in &mut buttons {
        if binding.scene_id.as_ref() != selected { binding.scene_id = selected.cloned(); }
        let enabled = saved.is_some() && !editor.scene_transition_in_progress && !browser.selection_pending;
        if binding.enabled != enabled { binding.enabled = enabled; }
        let display = if enabled { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    // 值以固定八个字段表达，独立网格只读取len而不展开/复制整个作者集合。
    let value = |field: ScenePropertiesField| -> String {
        let Some(manifest) = manifest else {
            return match field {
                ScenePropertiesField::Name if selected.is_none() => "未选择单个场景".into(),
                ScenePropertiesField::Name => "场景清单尚未就绪".into(),
                ScenePropertiesField::Id => selected.map(ToString::to_string).unwrap_or_default(),
                _ => String::new(),
            };
        };
        match field {
            ScenePropertiesField::Name => manifest.display_name.clone(),
            ScenePropertiesField::Id => manifest.id.to_string(),
            ScenePropertiesField::Kind => if manifest.terrains.is_some() { "独立网格场景" } else {
                match manifest.terrain.kind {
                    SceneTerrainKind::Empty => "空白场景",
                    SceneTerrainKind::Gltf => "GLB场景",
                    SceneTerrainKind::SparseClipmap => "Clipmap场景",
                    SceneTerrainKind::Voxel => "体素场景",
                }
            }.into(),
            ScenePropertiesField::Notes => if manifest.notes.trim().is_empty() { "未填写".into() } else { manifest.notes.clone() },
            ScenePropertiesField::Runtime => manifest.runtime_scene.clone(),
            ScenePropertiesField::Spawn => format!("X {}  Y {}  Z {}", manifest.default_spawn[0], manifest.default_spawn[1], manifest.default_spawn[2]),
            ScenePropertiesField::Manifest => manifest_path.clone().unwrap_or_default(),
            ScenePropertiesField::Source => if let Some(terrains) = &manifest.terrains {
                format!("{} 个独立网格", terrains.len())
            } else if let Some(path) = &manifest.terrain.height_path {
                path.clone()
            } else { manifest.terrain.glb_path.clone() },
        }
    };
    let values = ScenePropertiesField::ALL.map(value);
    for (marker, mut text) in &mut texts {
        let next = &values[marker.0.index()];
        if text.as_str() != next { **text = next.clone(); }
    }
    for (marker, mut node) in &mut rows {
        let display = if values[marker.0.index()].is_empty() { Display::None } else { Display::Grid };
        if node.display != display { node.display = display; }
    }
}

#[cfg(test)]
#[path = "properties_tests.rs"]
mod tests;
