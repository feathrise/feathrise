//! 水源换选先完整通过事务门；被拒绝的树点击和菜单命令必须保留旧属性、多选和其他类别缓存。
use super::*;
use super::super::types::SceneObjectRequest;
use crate::{authoring::EditorCommand, types::{EditorTool, WaterAuthoringFeatureKind},
    ui::{scene_layers::SceneLayerSelection, terrain_objects::TerrainObjectsEditorState},
    viewport::multi_select::{PlacementSelectionSet, SelectedMember}};
use bevy::ecs::system::RunSystemOnce;
use std::sync::Arc;

#[test]
fn rejected_water_source_keeps_confirmed_selection_and_cancels_its_menu_command() {
    for from_menu in [false, true] {
        for reason in ["sparse-dirty", "loading", "unscoped-history", "success"] {
            let (mut session, private, id) = crate::ui::water_feature_list::source_selection_tests::same_id_sources();
            let map = session.draft.as_ref().unwrap().map_id.clone();
            let mut editor = MapEditorState { open: true, selected_map_id: Some(map.clone()),
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
            legacy.selected_vegetation_map_id = Some(map.clone());
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
