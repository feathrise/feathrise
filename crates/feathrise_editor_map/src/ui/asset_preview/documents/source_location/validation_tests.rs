//! 使用生产地图标签构造验证只读finder；无窗口布局事实仅用于条件回归，不作为真实GPU验收。

use feathrise_editor_ui::InteractionButton;
use super::*;
use super::super::super::types::DiskIdentity;
use crate::{asset_catalog::document_format, resources::MapEditorState,
    ui::{map_documents::{MapDocumentButton, MapDocuments, spawn_map_tabs}, vegetation_preset_editor::PresetWorkspaceTab}};
use bevy::ecs::system::RunSystemOnce;
use feathrise_map_content::MapId;
use std::sync::Arc;

const NOTE: &str = "src/author_notes.rs";

fn fixture() -> (App, Entity, Entity, Entity) {
    let root = std::env::temp_dir().join("source-location-map-finder-state-only");
    let key = "project://src/author_notes.rs";
    let source: Arc<str> = Arc::from("// 未保存文本草稿\n");
    let mut documents = DocumentState::default();
    documents.root = root.join("assets"); documents.project_root = Some(root.clone());
    documents.active = Some(key.into()); documents.visible = true; documents.central_active = true;
    documents.documents.push(Document {
        path: key.into(), format: document_format(NOTE).unwrap(), generation: 1,
        disk: DiskIdentity { path: root.join(NOTE), digest: [7; 32], bom: false, crlf: false },
        pages: vec![source.clone()], source, revision: 1, saved_revision: 0, history: default(),
        preview: Arc::new(Vec::new()), preview_revision: None, preview_clipped: false, mode: ViewMode::Edit,
        preview_scroll: Vec2::ZERO, confirm_reload: false, external: default(), status: String::new(),
    });
    let a = MapId::parse("MAP-finder-a").unwrap();
    let b = MapId::parse("MAP-finder-b").unwrap();
    let mut maps = MapDocuments::default();
    maps.upsert(a.clone(), "当前作者地图".into(), true);
    maps.upsert(b.clone(), "另一个地图".into(), false);
    let mut app = App::new();
    app.insert_resource(documents).insert_resource(maps)
        .insert_resource(MapEditorState { open: true, selected_map_id: Some(a.clone()), ..default() });
    app.world_mut().run_system_once(|mut commands: Commands, maps: Res<MapDocuments>| {
        commands.spawn(Node::default()).with_children(|tabs| {
            spawn_map_tabs(tabs, &Handle::default(), &maps, None, PresetWorkspaceTab::Document);
        });
    }).unwrap();
    let buttons = app.world_mut().query::<(Entity, &MapDocumentButton)>().iter(app.world())
        .map(|(entity, action)| (entity, action.clone())).collect::<Vec<_>>();
    let mut current = None; let mut other = None; let mut close = None;
    for (entity, action) in buttons {
        app.world_mut().entity_mut(entity).insert((
            ComputedNode { size: Vec2::new(140., 28.), ..default() }, InheritedVisibility::VISIBLE,
        ));
        match action {
            MapDocumentButton::Activate(map) if map == a => current = Some(entity),
            MapDocumentButton::Activate(map) if map == b => other = Some(entity),
            MapDocumentButton::Close(map) if map == a => close = Some(entity),
            _ => (),
        }
    }
    (app, current.unwrap(), other.unwrap(), close.unwrap())
}

#[test]
fn map_finder_returns_the_real_current_map_title_and_keeps_the_document_untouched() {
    let (mut app, current, other, close) = fixture();
    let before = source_location_probe(app.world_mut()).unwrap();
    let found = source_location_action_entity(app.world_mut(), NOTE, SourceLocationAction::Map);
    assert_eq!(found, Some(current));
    assert_ne!(found, Some(other)); assert_ne!(found, Some(close));
    assert!(app.world().get::<InteractionButton>(current).is_some());
    assert_eq!(before, source_location_probe(app.world_mut()).unwrap());
    assert!(app.world().resource::<DocumentState>().central_active, "查找实体本身不能替代实际点击切页");
}

#[test]
fn map_finder_uses_selected_map_identity_and_rejects_a_retired_title() {
    let (mut app, _, other, _) = fixture();
    app.world_mut().resource_mut::<MapEditorState>().selected_map_id = Some(MapId::parse("MAP-finder-b").unwrap());
    assert_eq!(source_location_action_entity(app.world_mut(), NOTE, SourceLocationAction::Map), Some(other));
    app.world_mut().resource_mut::<MapDocuments>().tabs.clear();
    assert!(source_location_action_entity(app.world_mut(), NOTE, SourceLocationAction::Map).is_none(),
        "旧标题实体尚未退役也不能重新激活已经关闭的地图");
}

#[test]
fn map_finder_requires_the_actual_visible_enabled_button_and_nonempty_layout() {
    for gate in ["display", "visibility", "layout", "disabled", "button"] {
        let (mut app, current, _, _) = fixture();
        match gate {
            "display" => app.world_mut().get_mut::<Node>(current).unwrap().display = Display::None,
            "visibility" => { app.world_mut().entity_mut(current).insert(InheritedVisibility::HIDDEN); }
            "layout" => { app.world_mut().entity_mut(current).insert(ComputedNode::default()); }
            "disabled" => { app.world_mut().entity_mut(current).insert(bevy::ui::InteractionDisabled); }
            "button" => { app.world_mut().entity_mut(current).remove::<InteractionButton>(); }
            _ => unreachable!(),
        }
        assert!(source_location_action_entity(app.world_mut(), NOTE, SourceLocationAction::Map).is_none(), "必须保留{gate}门");
    }
}

#[test]
fn map_finder_preserves_expected_document_and_overlay_ownership_gates() {
    let (mut app, _, _, _) = fixture();
    assert!(source_location_action_entity(app.world_mut(), "src/other.rs", SourceLocationAction::Map).is_none());
    app.world_mut().insert_resource(feathrise_editor_core::EditorHostOverlay { active: true });
    assert!(source_location_action_entity(app.world_mut(), NOTE, SourceLocationAction::Map).is_none());
    app.world_mut().resource_mut::<feathrise_editor_core::EditorHostOverlay>().active = false;
    app.world_mut().resource_mut::<DocumentState>().visible = false;
    assert!(source_location_action_entity(app.world_mut(), NOTE, SourceLocationAction::Map).is_none());
}
