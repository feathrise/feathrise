//! 绘制组UI使用固定行池，创建和目标勾选不分配密度、不重建按钮或改变检查器。
use feathrise_editor_ui::UiInteraction;
use super::{components::*, scene, systems};
use crate::{
    authoring::{EditorAuthoringSession, VegetationBrushStroke, vegetation_paint_scope::*},
    resources::*,
};
use bevy::{asset::AssetPlugin, prelude::*};

fn app() -> App {
    let draft = crate::authoring::vegetation_paint_scope::painted_draft();
    let map = draft.map_id.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Font>()
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        })
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map),
            ..default()
        })
        .init_resource::<VegetationPaintSettings>()
        .init_resource::<VegetationPaintState>()
        .init_resource::<VegetationBrushStroke>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_systems(
            Update,
            (systems::handle, scene::setup, systems::sync).chain(),
        );
    app.world_mut().spawn((PaintPanel, Node::default()));
    app.update();
    app
}
fn press(app: &mut App, predicate: impl Fn(&PaintAction) -> bool) {
    let entity = app
        .world_mut()
        .query::<(Entity, &PaintAction)>()
        .iter(app.world())
        .find(|(_, a)| predicate(a))
        .unwrap()
        .0;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().entity_mut(entity).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}
#[test]
fn cross_mesh_group_ui_creates_once_while_held_and_never_allocates_member_pixels() {
    let mut app = app();
    let before = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .vegetation_author_densities
        .clone();
    press(&mut app, |a| matches!(a, PaintAction::New));
    for _ in 0..4 {
        app.update();
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .vegetation_paint_groups
            .len(),
        2
    );
    assert_eq!(session.undo.len(), 1);
    let draft = session.draft.as_ref().unwrap();
    assert!(draft.manifest.vegetation_paint_groups[1].members.is_empty());
    for (id, pixels) in before {
        assert!(std::sync::Arc::ptr_eq(
            &pixels,
            &draft.vegetation_author_densities[&id]
        ));
    }
    assert!(app.world().resource::<VegetationPaintSettings>().scope == PaintScope::Cross);
}
#[test]
fn cross_mesh_target_list_scrolls_large_maps_with_twenty_stable_rows_and_independent_checks() {
    let mut app = app();
    press(&mut app, |a| matches!(a, PaintAction::Cross));
    let rows = app
        .world_mut()
        .query_filtered::<Entity, With<PaintRow>>()
        .iter(app.world())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 20);
    let active = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .terrain_documents
        .active
        .as_ref()
        .unwrap()
        .definition
        .id
        .clone();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let original = draft.manifest.terrains.as_ref().unwrap()[0].clone();
        draft.manifest.terrains = Some(
            (0..10_000)
                .map(|i| {
                    let mut o = original.clone();
                    o.id = feathrise_map_content::TerrainId::parse(format!("large-{i}")).unwrap();
                    o.display_name = format!("网格{i}");
                    o
                })
                .collect::<Vec<_>>()
                .into(),
        );
    }
    app.update();
    let viewport = app
        .world_mut()
        .query::<(Entity, &PaintList)>()
        .iter(app.world())
        .find(|(_, list)| list.0 == ListKind::Targets)
        .unwrap()
        .0;
    app.world_mut()
        .get_mut::<ScrollPosition>(viewport)
        .unwrap()
        .y = 9500. * 27.;
    app.update();
    let target = app
        .world_mut()
        .query::<(&PaintRow, &PaintAction)>()
        .iter(app.world())
        .find_map(|(row, action)| {
            if row.kind == ListKind::Targets && row.slot == 0 {
                if let PaintAction::Target(id) = action {
                    return Some(id.clone());
                }
            }
            None
        })
        .unwrap();
    assert_eq!(target.as_str(), "large-9499");
    press(
        &mut app,
        |a| matches!(a,PaintAction::Target(id) if *id==target),
    );
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert!(
        draft.manifest.vegetation_paint_groups[0]
            .targets
            .contains(&target)
    );
    assert_eq!(
        draft
            .terrain_documents
            .active
            .as_ref()
            .unwrap()
            .definition
            .id,
        active
    );
    assert!(rows.iter().all(|e| app.world().get_entity(*e).is_ok()));
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<PaintRow>>()
            .iter(app.world())
            .count(),
        20
    );
}
