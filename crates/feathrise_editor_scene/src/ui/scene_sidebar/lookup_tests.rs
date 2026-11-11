//! 离屏查名按64项预算推进，只读完整已采用投影，不采用整理中的私有候选。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::ui::scene_sidebar::lookup::{LOOKUP_STEPS, advance_lookup};

#[test]
fn visible_binding_seeds_full_name_and_releases_tree_lease() {
    let index = published_index(vec![entry(scene_item("MAP-1"), "真实名称")]);
    let mut lookup = SelectionLookup::default();
    advance_lookup(
        &mut lookup,
        Some(&index.entries[0].item),
        &index,
        Some(&index.entries[0]),
    );
    assert_eq!(lookup.record, Some(index.entries[0].clone()));
    assert!(lookup.complete);
    assert!(lookup.source.is_none());
    assert_eq!(Arc::strong_count(&index.full_entries), 1);
}

#[test]
fn offscreen_lookup_has_fixed_budget_and_does_not_rescan_after_completion() {
    let entries = (0..205)
        .map(|number| entry(scene_item(&format!("MAP-{number}")), "同名场景"))
        .collect();
    let index = published_index(entries);
    let selected = scene_item("MAP-204");
    let mut lookup = SelectionLookup::default();
    for expected in [LOOKUP_STEPS, LOOKUP_STEPS * 2, LOOKUP_STEPS * 3] {
        advance_lookup(&mut lookup, Some(&selected), &index, None);
        assert_eq!(lookup.cursor, expected);
        assert!(lookup.record.is_none());
        assert!(!lookup.complete);
    }
    advance_lookup(&mut lookup, Some(&selected), &index, None);
    assert_eq!(lookup.record.as_ref().unwrap().item, selected);
    assert!(lookup.complete);
    assert!(lookup.source.is_none());
    let cursor = lookup.cursor;
    advance_lookup(&mut lookup, Some(&selected), &index, None);
    assert_eq!(lookup.cursor, cursor);
    assert_eq!(Arc::strong_count(&index.full_entries), 1);
}

#[test]
fn private_candidate_cannot_replace_old_published_name() {
    let mut index = published_index(vec![entry(scene_item("MAP-1"), "已显示名称")]);
    let selected = scene_item("MAP-1");
    let newer = SceneBrowserIndexSignature {
        folder_revision: 2,
        catalog_revision: 2,
    };
    index.applied_signature = Some(newer.clone());
    index.full_entries = Arc::new(vec![entry(selected.clone(), "尚未公开的候选名称")]);
    let mut lookup = SelectionLookup::default();
    advance_lookup(&mut lookup, Some(&selected), &index, None);
    assert!(lookup.record.is_none());
    assert!(lookup.source.is_none());
    advance_lookup(
        &mut lookup,
        Some(&selected),
        &index,
        Some(&index.entries[0]),
    );
    assert_eq!(lookup.record.as_ref().unwrap().display_name, "已显示名称");
    index.applied_projection = Some((newer, 1));
    advance_lookup(&mut lookup, Some(&selected), &index, None);
    assert_eq!(
        lookup.record.as_ref().unwrap().display_name,
        "尚未公开的候选名称"
    );
}

#[test]
fn selection_or_projection_change_invalidates_record_and_missing_is_explicit() {
    let mut index = published_index(vec![entry(scene_item("MAP-1"), "同名场景")]);
    let mut lookup = SelectionLookup::default();
    advance_lookup(&mut lookup, Some(&scene_item("MAP-1")), &index, None);
    advance_lookup(&mut lookup, Some(&scene_item("MAP-2")), &index, None);
    assert!(lookup.complete);
    assert!(lookup.record.is_none());
    index.full_entries = Arc::new(vec![entry(scene_item("MAP-2"), "热更新后的完整名称")]);
    index.applied_projection.as_mut().unwrap().1 += 1;
    advance_lookup(&mut lookup, Some(&scene_item("MAP-2")), &index, None);
    assert_eq!(
        lookup.record.as_ref().unwrap().display_name,
        "热更新后的完整名称"
    );
    advance_lookup(&mut lookup, Some(&SceneBrowserItem::Root), &index, None);
    assert!(lookup.complete && lookup.source.is_none() && lookup.record.is_none());
}

#[test]
fn real_row_and_root_share_hover_select_and_drop_feedback_without_text_rewrites() {
    use crate::ui::theme as colors;
    let mut app = sidebar_app();
    app.add_systems(
        Update,
        (
            crate::ui::rows::sync_scene_rows,
            crate::scene_browser::sync_scene_browser_root_row,
        )
            .before(sync_sidebar),
    );
    let label = app
        .world_mut()
        .spawn((SceneEditorSceneRowText, Text::new(""), TextColor::default()))
        .id();
    let row = app
        .world_mut()
        .spawn((
            SceneEditorSceneRow {
                slot: 0,
                item: None,
            },
            Node::default(),
            UiInteraction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id();
    app.world_mut().entity_mut(label).insert(ChildOf(row));
    let root = app
        .world_mut()
        .spawn((
            SceneEditorSceneRootRow,
            UiInteraction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id();
    app.update();
    *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::Hovered;
    *app.world_mut().get_mut::<UiInteraction>(root).unwrap() = UiInteraction::Hovered;
    app.update();
    assert_eq!(
        *app.world().get::<BackgroundColor>(row).unwrap(),
        *app.world().get::<BackgroundColor>(root).unwrap()
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(row).unwrap().0,
        colors::SELECT_BG_HOVER
    );
    *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
    select(&mut app, Some(scene_item("SCENE-1")));
    assert_eq!(
        app.world().get::<BackgroundColor>(row).unwrap().0,
        colors::SELECT_BG
    );
    assert_eq!(
        *app.world().get::<BorderColor>(row).unwrap(),
        BorderColor::all(colors::COPPER_BORDER)
    );
    app.world_mut()
        .resource_mut::<SceneBrowserDragState>()
        .drop_target = Some(SceneBrowserDropTarget::Root);
    app.update();
    assert_eq!(
        *app.world().get::<BorderColor>(root).unwrap(),
        BorderColor::all(colors::COPPER_BORDER)
    );
    app.init_resource::<Writes>().add_systems(
        Update,
        (|texts: Query<(), (With<SceneEditorSceneRowText>, Changed<Text>)>,
          mut writes: ResMut<Writes>| {
            writes.0 += texts.iter().count();
        })
        .after(crate::ui::rows::sync_scene_rows),
    );
    app.update();
    app.world_mut().resource_mut::<Writes>().0 = 0;
    app.world_mut().resource_mut::<SceneEditorState>().status = "其他工具的状态".into();
    app.update();
    assert_eq!(app.world().resource::<Writes>().0, 0);
}
