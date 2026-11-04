//! 地图属性按目录身份读取、稳定帧不重写，以及失效目标不借当前地图托底的回归。

use super::*;
use crate::ui::map_sidebar::tests::{initialize, manifest, map_item, register_manifest};

fn fixture() -> (App, [(MapPropertiesField, Entity, Entity); 8], Entity) {
    let mut app = App::new();
    initialize(&mut app);
    register_manifest(&mut app, manifest("MAP-1", "已经进入的地图"));
    let mut second = manifest("MAP-2", "目录所选地图");
    second.notes = "第二张图的备注".into();
    second.default_spawn = [21.0, 3.0, 45.0];
    second.terrain.glb_path = "maps/selected/source.glb".into();
    register_manifest(&mut app, second);
    app.world_mut().resource_mut::<MapEditorState>().selected_map_id = Some(feathrise_map_content::MapId::parse("MAP-1").unwrap());
    app.world_mut().resource_mut::<MapBrowserState>().select_only(Some(map_item("MAP-2")));
    app.world_mut().resource_mut::<MapEditorSidebarState>().reveal_map_properties();
    let fields = MapPropertiesField::ALL.map(|field| {
        let text = app.world_mut().spawn((MapPropertiesText(field), Text::new(""))).id();
        let row = app.world_mut().spawn((MapPropertiesRow(field), Node::default())).id();
        (field, text, row)
    });
    let button = app.world_mut().spawn((MapPropertiesEditBinding::default(), Node::default())).id();
    app.add_systems(Update, sync_map_properties);
    app.update();
    (app, fields, button)
}

fn value(app: &App, fields: &[(MapPropertiesField, Entity, Entity); 8], field: MapPropertiesField) -> String {
    app.world().get::<Text>(fields[field.index()].1).unwrap().0.clone()
}

#[test]
fn directory_map_properties_show_selected_metadata_without_using_entered_map() {
    let (mut app, fields, button) = fixture();
    assert_eq!(value(&app, &fields, MapPropertiesField::Name), "目录所选地图");
    assert_eq!(value(&app, &fields, MapPropertiesField::Id), "MAP-2");
    assert_eq!(value(&app, &fields, MapPropertiesField::Kind), "高度场地图");
    assert_eq!(value(&app, &fields, MapPropertiesField::Notes), "第二张图的备注");
    assert_eq!(value(&app, &fields, MapPropertiesField::Source), "maps/selected/source.glb");
    assert_eq!(value(&app, &fields, MapPropertiesField::Spawn), "X 21  Y 3  Z 45");
    assert!(app.world().get::<MapPropertiesEditBinding>(button).unwrap().enabled);
    let pointer = app.world().get::<Text>(fields[MapPropertiesField::Notes.index()].1).unwrap().0.as_ptr();
    app.update();
    assert_eq!(app.world().get::<Text>(fields[MapPropertiesField::Notes.index()].1).unwrap().0.as_ptr(), pointer,
        "稳定帧不重写完整备注，避免破坏文本缓存");
    assert_eq!(app.world().resource::<MapEditorState>().selected_map_id.as_ref().unwrap().as_str(), "MAP-1");
    assert!(app.world().resource::<MapEditorState>().pending_map_transition.is_none());
}

#[test]
fn stale_or_multiple_map_selection_clears_properties_and_edit_target() {
    let (mut app, fields, button) = fixture();
    let id = feathrise_map_content::MapId::parse("MAP-2").unwrap();
    app.world_mut().resource_mut::<MapContentRegistry>().maps.remove(&id);
    app.update();
    assert_eq!(value(&app, &fields, MapPropertiesField::Name), "地图清单尚未就绪");
    assert_eq!(value(&app, &fields, MapPropertiesField::Id), "MAP-2");
    assert_eq!(value(&app, &fields, MapPropertiesField::Notes), "");
    assert_eq!(app.world().get::<Node>(fields[MapPropertiesField::Notes.index()].2).unwrap().display, Display::None);
    assert!(!app.world().get::<MapPropertiesEditBinding>(button).unwrap().enabled);
    app.world_mut().resource_mut::<MapBrowserState>().toggle_selection(map_item("MAP-1"));
    app.update();
    assert_eq!(value(&app, &fields, MapPropertiesField::Name), "未选择单张地图");
    assert!(app.world().get::<MapPropertiesEditBinding>(button).unwrap().map_id.is_none());
}

#[test]
fn map_properties_assets_refresh_only_the_same_selected_identity() {
    let (mut app, fields, _) = fixture();
    let id = feathrise_map_content::MapId::parse("MAP-2").unwrap();
    let handle = app.world().resource::<MapContentRegistry>().handles(&id).unwrap().manifest.clone();
    app.world_mut().resource_mut::<Assets<MapManifestAsset>>().get_mut(&handle).unwrap().notes = "外部更新后的备注".into();
    app.update();
    assert_eq!(value(&app, &fields, MapPropertiesField::Notes), "外部更新后的备注");
    app.world_mut().resource_mut::<Assets<MapManifestAsset>>().get_mut(&handle).unwrap().id = feathrise_map_content::MapId::parse("MAP-1").unwrap();
    app.update();
    assert_eq!(value(&app, &fields, MapPropertiesField::Name), "地图清单尚未就绪");
    assert_eq!(value(&app, &fields, MapPropertiesField::Notes), "");
}
