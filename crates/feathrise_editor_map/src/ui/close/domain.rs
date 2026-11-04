//! 运行真实关闭系统，确认旧扩展回执不能放行新地图或新文档，当前回执仍可正常收口。

use super::*;
use crate::authoring::test_support::test_heightfield_draft;
use crate::game_authoring::map_interaction::{MapInteractionState, fixtures};
use std::sync::Arc;

#[test]
fn stale_domain_receipt_cannot_close_a_new_map_document_generation_or_revision() {
    for changed_identity in ["map", "document", "hash", "generation", "revision"] {
        let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.domain = Some(fixtures::document("adopted-domain"));
        let mut session = EditorAuthoringSession { draft: Some(draft), revision: 7, ..default() };
        let mut port = fixtures::state("adopted-domain", 1);
        port.observed_generation = 3;
        assert!(!port.prepare_close(&session));
        let old_receipt = port.close_request.take().unwrap();

        // 地图切换可能保留相同修订数字；回执还必须逐项匹配其余来源身份。
        let draft = session.draft.as_mut().unwrap();
        let snapshot = Arc::get_mut(port.snapshot.as_mut().unwrap()).unwrap();
        match changed_identity {
            "map" => {
                let map = MapId("OTHER-TEST".into());
                draft.map_id = map.clone();
                draft.manifest.id = map.clone();
                draft.placements.map_id = map.clone();
                draft.portals.map_id = map.clone();
                snapshot.map = map;
            }
            "document" => {
                draft.domain.as_mut().unwrap().domain = "test.other-document";
                snapshot.document_domain = "test.other-document";
            }
            "hash" => {
                draft.domain = Some(fixtures::document("new-domain"));
                snapshot.hash = "new-domain".into();
            }
            "generation" => port.observed_generation += 1,
            "revision" => session.revision += 1,
            _ => unreachable!(),
        }
        port.close_completed = Some(old_receipt);
        port.status = "扩展域正在收口".into();
        let mut app = domain_close_app(session, port);
        app.update();
        let editor = app.world().resource::<MapEditorState>();
        assert!(editor.open && editor.close_requested, "旧{changed_identity}回执不得关闭当前地图");
        let port = app.world().resource::<MapInteractionState>();
        assert!(port.close_completed.is_none(), "旧回执必须在当前关闭门被清除");
        let current_request = port.close_request.clone().expect("当前完整快照应生成新的关闭请求");
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(current_request.revision, session.revision);
        assert!(current_request.identity.current(session.draft.as_ref().unwrap(), port.observed_generation));

        // 外部域完成当前请求后，仍通过同一真实关闭系统结束F12，防止门只会阻止却不会恢复。
        app.world_mut().resource_mut::<MapInteractionState>().close_completed = Some(current_request);
        app.update();
        let editor = app.world().resource::<MapEditorState>();
        assert!(!editor.open && !editor.close_requested, "当前{changed_identity}回执应允许关闭");
    }
}

fn domain_close_app(session: EditorAuthoringSession, port: MapInteractionState) -> App {
    let mut app = App::new();
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_map_content::ActiveDynamicMap {
            map_id: Some(session.draft.as_ref().unwrap().map_id.clone()),
        })
        .insert_resource(MapEditorState { open: true, close_requested: true, ..default() })
        .insert_resource(session)
        .insert_resource(port)
        .init_resource::<TerrainBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<TerrainLiveApplyState>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<BoundaryEditState>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<MapCreationTask>()
        .init_resource::<MapDeletionTask>()
        .add_systems(Update, finish_map_editor_close);
    app
}
