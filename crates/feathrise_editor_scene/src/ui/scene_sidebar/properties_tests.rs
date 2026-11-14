//! 场景属性按目录身份读取、稳定帧不重写，以及失效目标不借当前场景托底的回归。

use super::*;
use crate::ui::scene_sidebar::tests::{initialize, manifest, scene_item, register_manifest};

fn fixture() -> (App, [(ScenePropertiesField, Entity, Entity); 8], Entity) {
    let mut app = App::new();
    initialize(&mut app);
    register_manifest(&mut app, manifest("MAP-1", "已经进入的场景"));
    let mut second = manifest("MAP-2", "目录所选场景");
    second.notes = "第二张图的备注".into();
    second.default_spawn = [21.0, 3.0, 45.0];
    second.terrain.glb_path = "scenes/selected/source.glb".into();
    register_manifest(&mut app, second);
    app.world_mut().resource_mut::<SceneEditorState>().selected_scene_id = Some(feathrise_scene_content::SceneId::parse("MAP-1").unwrap());
    app.world_mut().resource_mut::<SceneBrowserState>().select_only(Some(scene_item("MAP-2")));
    app.world_mut().resource_mut::<SceneEditorSidebarState>().reveal_scene_entry_properties();
    let fields = ScenePropertiesField::ALL.map(|field| {
        let text = app.world_mut().spawn((ScenePropertiesText(field), Text::new(""))).id();
        let row = app.world_mut().spawn((ScenePropertiesRow(field), Node::default())).id();
        (field, text, row)
    });
    let button = app.world_mut().spawn((ScenePropertiesEditBinding::default(), Node::default())).id();
    app.add_systems(Update, sync_scene_properties);
    app.update();
    (app, fields, button)
}

fn value(app: &App, fields: &[(ScenePropertiesField, Entity, Entity); 8], field: ScenePropertiesField) -> String {
    app.world().get::<Text>(fields[field.index()].1).unwrap().0.clone()
}

#[test]
fn directory_scene_properties_show_selected_metadata_without_using_entered_scene() {
    let (mut app, fields, button) = fixture();
    assert_eq!(value(&app, &fields, ScenePropertiesField::Name), "目录所选场景");
    assert_eq!(value(&app, &fields, ScenePropertiesField::Id), "MAP-2");
    assert_eq!(value(&app, &fields, ScenePropertiesField::Kind), "GLB场景"); // 目录夹具使用GLB来源，独立Heightfield作者类型已移除。
    assert_eq!(value(&app, &fields, ScenePropertiesField::Notes), "第二张图的备注");
    assert_eq!(value(&app, &fields, ScenePropertiesField::Source), "scenes/selected/source.glb");
    assert_eq!(value(&app, &fields, ScenePropertiesField::Spawn), "X 21  Y 3  Z 45");
    assert!(app.world().get::<ScenePropertiesEditBinding>(button).unwrap().enabled);
    let pointer = app.world().get::<Text>(fields[ScenePropertiesField::Notes.index()].1).unwrap().0.as_ptr();
    app.update();
    assert_eq!(app.world().get::<Text>(fields[ScenePropertiesField::Notes.index()].1).unwrap().0.as_ptr(), pointer,
        "稳定帧不重写完整备注，避免破坏文本缓存");
    assert_eq!(app.world().resource::<SceneEditorState>().selected_scene_id.as_ref().unwrap().as_str(), "MAP-1");
    assert!(app.world().resource::<SceneEditorState>().pending_scene_transition.is_none());
}

#[test]
fn stale_or_multiple_scene_selection_clears_properties_and_edit_target() {
    let (mut app, fields, button) = fixture();
    let id = feathrise_scene_content::SceneId::parse("MAP-2").unwrap();
    app.world_mut().resource_mut::<SceneContentRegistry>().scenes.remove(&id);
    app.update();
    assert_eq!(value(&app, &fields, ScenePropertiesField::Name), "场景清单尚未就绪");
    assert_eq!(value(&app, &fields, ScenePropertiesField::Id), "MAP-2");
    assert_eq!(value(&app, &fields, ScenePropertiesField::Notes), "");
    assert_eq!(app.world().get::<Node>(fields[ScenePropertiesField::Notes.index()].2).unwrap().display, Display::None);
    assert!(!app.world().get::<ScenePropertiesEditBinding>(button).unwrap().enabled);
    app.world_mut().resource_mut::<SceneBrowserState>().toggle_selection(scene_item("MAP-1"));
    app.update();
    assert_eq!(value(&app, &fields, ScenePropertiesField::Name), "未选择单个场景");
    assert!(app.world().get::<ScenePropertiesEditBinding>(button).unwrap().scene_id.is_none());
}

#[test]
fn scene_properties_assets_refresh_only_the_same_selected_identity() {
    let (mut app, fields, _) = fixture();
    let id = feathrise_scene_content::SceneId::parse("MAP-2").unwrap();
    let handle = app.world().resource::<SceneContentRegistry>().handles(&id).unwrap().manifest.clone();
    app.world_mut().resource_mut::<Assets<SceneManifestAsset>>().get_mut(&handle).unwrap().notes = "外部更新后的备注".into();
    app.update();
    assert_eq!(value(&app, &fields, ScenePropertiesField::Notes), "外部更新后的备注");
    app.world_mut().resource_mut::<Assets<SceneManifestAsset>>().get_mut(&handle).unwrap().id = feathrise_scene_content::SceneId::parse("MAP-1").unwrap();
    app.update();
    assert_eq!(value(&app, &fields, ScenePropertiesField::Name), "场景清单尚未就绪");
    assert_eq!(value(&app, &fields, ScenePropertiesField::Notes), "");
}
