//! 所选目录地图的只读属性同步；固定字段只借用内存清单，不扫描目录或进入地图。

use super::{components::*, types::{MapPropertiesField, MapPropertiesObservation}};
use crate::authoring::EditorAuthoringSession;
use crate::resources::{MapBrowserState, MapEditorState};
use crate::types::MapBrowserItem;
use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState};
use bevy::prelude::*;
use feathrise_map_content::{MapContentRegistry, MapManifestAsset, MapTerrainKind};

pub(super) fn sync_map_properties(
    editor: Res<MapEditorState>,
    browser: Res<MapBrowserState>,
    sidebar: Res<MapEditorSidebarState>,
    session: Res<EditorAuthoringSession>,
    registry: Option<Res<MapContentRegistry>>,
    manifests: Option<Res<Assets<MapManifestAsset>>>,
    added: Query<(), Added<MapPropertiesText>>,
    mut texts: Query<(&MapPropertiesText, &mut Text)>,
    mut rows: Query<(&MapPropertiesRow, &mut Node)>,
    mut buttons: Query<(&mut MapPropertiesEditBinding, &mut Node), Without<MapPropertiesRow>>,
    mut scrolls: Query<(&crate::ui::sidebars::MapEditorSidebarPage, &mut ScrollPosition)>,
    mut previous: Local<Option<MapPropertiesObservation>>,
) {
    if !editor.open || sidebar.right != EditorRightSidebarTab::Map {
        return;
    }
    let selected = match browser.selected_item.as_ref() {
        Some(MapBrowserItem::Map(id)) if browser.selection_count() == 1 => Some(id),
        _ => None,
    };
    let handles = selected.and_then(|id| registry.as_ref()?.handles(id));
    let asset_id = handles.map(|handles| handles.manifest.id());
    let asset_tick = manifests.as_ref().map_or(0, |assets| assets.last_changed().get());
    let registry_revision = registry.as_ref().map_or(0, |registry| registry.revision);
    let draft_map = session.draft.as_ref().map(|draft| &draft.map_id);
    if previous.as_ref().is_some_and(|old| old.map_id.as_ref() == selected
        && old.current_map == editor.selected_map_id && old.selection_pending == browser.selection_pending
        && old.manifest == asset_id && old.asset_tick == asset_tick && old.registry_revision == registry_revision
        && old.draft_revision == session.revision && old.draft_map.as_ref() == draft_map
        && old.loading == editor.map_transition_in_progress) && added.is_empty() {
        return;
    }
    if previous.as_ref().is_none_or(|old| old.map_id.as_ref() != selected) {
        for (page, mut scroll) in &mut scrolls {
            if page.0 == crate::ui::sidebars::EditorSidebarTab::Right(EditorRightSidebarTab::Map) {
                scroll.0 = Vec2::ZERO;
            }
        }
    }
    *previous = Some(MapPropertiesObservation { map_id: selected.cloned(), current_map: editor.selected_map_id.clone(),
        selection_pending: browser.selection_pending, manifest: asset_id, asset_tick, registry_revision,
        draft_revision: session.revision, draft_map: draft_map.cloned(), loading: editor.map_transition_in_progress });
    let saved = handles.and_then(|handles| manifests.as_ref()?.get(&handles.manifest))
        .filter(|manifest| Some(&manifest.id) == selected);
    // 当前地图显示同身份草稿；浏览另一张地图时只能读取那张地图的正式资产。
    let manifest = session.draft.as_ref()
        .filter(|draft| Some(&draft.map_id) == selected && editor.selected_map_id.as_ref() == selected)
        .map(|draft| &draft.manifest)
        .or(saved);
    let manifest_path = handles.and_then(|handles| handles.manifest.path()).map(ToString::to_string);
    for (mut binding, mut node) in &mut buttons {
        if binding.map_id.as_ref() != selected { binding.map_id = selected.cloned(); }
        let enabled = saved.is_some() && !editor.map_transition_in_progress && !browser.selection_pending;
        if binding.enabled != enabled { binding.enabled = enabled; }
        let display = if enabled { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    // 值以固定八个字段表达，独立网格只读取len而不展开/复制整个作者集合。
    let value = |field: MapPropertiesField| -> String {
        let Some(manifest) = manifest else {
            return match field {
                MapPropertiesField::Name if selected.is_none() => "未选择单张地图".into(),
                MapPropertiesField::Name => "地图清单尚未就绪".into(),
                MapPropertiesField::Id => selected.map(ToString::to_string).unwrap_or_default(),
                _ => String::new(),
            };
        };
        match field {
            MapPropertiesField::Name => manifest.display_name.clone(),
            MapPropertiesField::Id => manifest.id.to_string(),
            MapPropertiesField::Kind => if manifest.terrains.is_some() { "独立网格地图" } else {
                match manifest.terrain.kind {
                    MapTerrainKind::Empty => "空白地图",
                    MapTerrainKind::Gltf => "GLB地图",
                    MapTerrainKind::Heightfield => "高度场地图",
                    MapTerrainKind::SparseClipmap => "Clipmap地图",
                    MapTerrainKind::Voxel => "体素地图",
                }
            }.into(),
            MapPropertiesField::Notes => if manifest.notes.trim().is_empty() { "未填写".into() } else { manifest.notes.clone() },
            MapPropertiesField::Runtime => manifest.runtime_scene.clone(),
            MapPropertiesField::Spawn => format!("X {}  Y {}  Z {}", manifest.default_spawn[0], manifest.default_spawn[1], manifest.default_spawn[2]),
            MapPropertiesField::Manifest => manifest_path.clone().unwrap_or_default(),
            MapPropertiesField::Source => if let Some(terrains) = &manifest.terrains {
                format!("{} 个独立网格", terrains.len())
            } else if let Some(path) = &manifest.terrain.height_path {
                path.clone()
            } else { manifest.terrain.glb_path.clone() },
        }
    };
    let values = MapPropertiesField::ALL.map(value);
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
