//! 水源换选先完整通过事务门；被拒绝的树点击和菜单命令必须保留旧属性、多选和其他类别缓存。
use super::*;
use super::super::types::SceneObjectRequest;
use crate::{authoring::EditorCommand, types::{EditorTool, WaterAuthoringFeatureKind},
    ui::{scene_layers::SceneLayerSelection, terrain_objects::TerrainObjectsEditorState},
    viewport::multi_select::{PlacementSelectionSet, SelectedMember}};
use bevy::ecs::system::RunSystemOnce;
use std::sync::Arc;

#[cfg(feature = "validation")]
#[test]
fn validation_terrain_focus_dispatch_replaces_a_previous_model_selection() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let object = draft.manifest.terrain_objects_for_edit()[0].clone();
    let id = object.id.clone();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    let target = super::super::SceneObjectTarget { scene_id: draft.scene_id.clone(), revision: 7,
        key: SceneObjectKey::Terrain { index: 0, id: id.clone() }, name: object.display_name,
        hidden: false, locked: false, copyable: true };
    let mut browser = EditorGltfBrowserState::default();
    browser.selected_placement_id = Some("previous-model".into());
    let mut app = App::new();
    app.insert_resource(SceneEditorState { open: true, selected_scene_id: Some(draft.scene_id.clone()), ..default() })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), revision: 7, ..default() })
        .insert_resource(browser).init_resource::<SceneContextMenuState>()
        .init_resource::<TerrainObjectsEditorState>().init_resource::<LayerCandidateState>()
        .init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>();
    app.world_mut().spawn((Window::default(), bevy::window::PrimaryWindow));
    app.world_mut().spawn((super::super::SceneObjectRow(Some(target)), Node::default(), feathrise_editor_ui::UiInteraction::None));
    assert!(super::super::queue_terrain_focus_for_validation(app.world_mut(), &id).unwrap());
    assert_eq!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.as_deref(), Some("previous-model"),
        "排队不能提前清选择或安装作者画布");
    app.world_mut().run_system_once(super::super::systems::dispatch).unwrap();
    app.world_mut().run_system_once(super::select_target).unwrap();
    assert!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.is_none());
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().selected.as_ref(), Some(&id));
    assert_eq!(
        app.world().resource::<LayerCandidateState>().selected.as_ref(),
        Some(&LayerTarget::TerrainObject(id.as_str().to_owned())),
    );
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.is_none(),
        "正式选择只发布目标，独立画布仍须由原后台载入");
}

#[test]
fn rejected_water_source_keeps_confirmed_selection_and_cancels_its_menu_command() {
    for from_menu in [false, true] {
        for reason in ["sparse-dirty", "loading", "unscoped-history", "success"] {
            let (mut session, private, id) = crate::ui::water_feature_list::source_selection_tests::same_id_sources();
            let map = session.draft.as_ref().unwrap().scene_id.clone();
            let mut editor = SceneEditorState { open: true, selected_scene_id: Some(map.clone()),
                tool: EditorTool::Inspect, ..default() };
            crate::ui::water_feature_list::activate_listed_water_feature(&mut editor, &mut session,
                Some(&private), false, WaterAuthoringFeatureKind::StillWater, &id, None, None).unwrap();
            editor.tool = EditorTool::Inspect;
            let target = crate::ui::scene_layers::tests::water_snapshot(&session).into_iter()
                .find(|target| matches!(&target.key, SceneObjectKey::Water { terrain: None, .. })).unwrap();
            let old = LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, id.clone());
            let mut candidates = LayerCandidateState::default();
            candidates.confirm_tree_author(old.clone(), Some(0), &session, 0, default());
            let confirmations = candidates.selection_revision;
            let revision = session.revision;
            let mut multi = PlacementSelectionSet::default();
            multi.publish(vec![SelectedMember { id: "previous-asset".into(), index: 0, position: Vec3::ONE }], &session);
            let members = multi.members.clone();
            let mut browser = EditorGltfBrowserState::default();
            browser.selected_group = Some("previous-group".into());
            browser.selected_placement_id = Some("previous-asset".into());
            let mut legacy = EditorViewportSelectionState::default();
            legacy.selected_vegetation_scene_id = Some(map.clone());
            legacy.selected_vegetation_output_id = Some("previous-grass".into());
            let mut terrains = TerrainObjectsEditorState::default();
            terrains.selected = Some(private.clone());
            terrains.selected_index = Some(0);
            terrains.documents_loading = reason == "loading";
            if reason == "sparse-dirty" { session.sparse_terrain_dirty = true; }
            if reason == "unscoped-history" {
                for _ in 0..130 {
                    session.undo.push(EditorCommand::WaterLevelChanged { layer_id: id.clone(), before: 899.0, after: 900.0 });
                }
            }
            let mut menu = SceneContextMenuState::default();
            let mut tree = SceneLayerSelection::default();
            if from_menu { menu.ready = Some(SceneObjectRequest { target, action: SceneMenuAction::Focus, name: String::new() }); }
            else { tree.0 = Some(target); }
            let mut app = App::new();
            app.insert_resource(editor).insert_resource(session).insert_resource(menu).insert_resource(tree)
                .insert_resource(candidates).insert_resource(browser).insert_resource(legacy)
                .insert_resource(multi).insert_resource(terrains);
            app.world_mut().run_system_once(super::select_target).unwrap();
            let session = app.world().resource::<EditorAuthoringSession>();
            let candidates = app.world().resource::<LayerCandidateState>();
            assert_eq!(candidates.selected.as_ref(), Some(&old));
            assert!(candidates.selected_matches_session(session));
            assert!(candidates.author_canvas_matches(session.draft.as_ref().unwrap()));
            if reason == "success" {
                assert!(session.draft.as_ref().unwrap().terrain_documents.active.is_none());
                assert_eq!(candidates.selection_revision, confirmations + 1);
                assert!(app.world().resource::<PlacementSelectionSet>().members.is_empty());
                assert!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.is_none());
                if from_menu {
                    assert_eq!(app.world().resource::<SceneContextMenuState>().ready.as_ref().unwrap().target.revision, session.revision);
                }
                continue;
            }
            assert_eq!(session.revision, revision, "{reason}");
            assert_eq!(session.draft.as_ref().unwrap().terrain_documents.active.as_ref().unwrap().definition.id, private);
            assert_eq!(candidates.selection_revision, confirmations, "失败不能取消或伪造正式选择：{reason}");
            assert!(Arc::ptr_eq(&app.world().resource::<PlacementSelectionSet>().members, &members));
            let browser = app.world().resource::<EditorGltfBrowserState>();
            assert_eq!(browser.selected_group.as_deref(), Some("previous-group"));
            assert_eq!(browser.selected_placement_id.as_deref(), Some("previous-asset"));
            assert_eq!(app.world().resource::<EditorViewportSelectionState>().selected_vegetation_output_id.as_deref(), Some("previous-grass"));
            assert!(app.world().resource::<SceneContextMenuState>().ready.is_none(), "失败的菜单定位不能继续消费旧水源请求");
        }
    }
}
