//! 删除内存预检的预算、完整来源、取消及目录事务门回归；不复制图层像素或写正式目录。

use super::*;
use super::super::tests::{catalog, registry, water};
use crate::authoring::terrain_documents::{RootWaterCanvas, TerrainCanvas};
use std::sync::Arc;

fn session() -> EditorAuthoringSession {
    EditorAuthoringSession {
        draft: Some(crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]))),
        ..Default::default()
    }
}

#[test]
fn active_root_and_parked_private_alias_references_are_all_protected() {
    let registry = registry(&catalog());
    for source in 0..3 {
        let mut session = session();
        let draft = session.draft.as_mut().unwrap();
        match source {
            0 => draft.water = Some(water("legacy-water")),
            1 => draft.terrain_documents.root_water = Some(RootWaterCanvas {
                water: Some(water("legacy-water")), ..Default::default()
            }),
            _ => { draft.terrain_documents.values.insert(TerrainId::parse("private").unwrap(), Arc::new(TerrainCanvas {
                water: Some(water("legacy-water")), ..Default::default()
            })); }
        }
        let mut pending = WaterDeletePreflight::new(&session, &registry, "water-main".into());
        assert!(pending.advance(&session, &registry, None, CHECKS_PER_FRAME).unwrap_err().contains("仍引用"));
    }
}

#[test]
fn another_open_map_with_unsaved_water_is_not_mistaken_for_its_old_disk_snapshot() {
    let registry = registry(&catalog());
    let current = session();
    let mut other = session();
    let draft = other.draft.as_mut().unwrap();
    let id = MapId::parse("PARKED-MAP").unwrap();
    draft.map_id = id.clone();
    draft.water = Some(water("legacy-water"));
    let mut documents = MapDocuments::default();
    documents.parked.insert(id, crate::ui::map_documents::MapDocumentSnapshot::with_test_session(other));
    let mut pending = WaterDeletePreflight::new(&current, &registry, "water-main".into());
    assert!(pending.advance(&current, &registry, Some(&documents), CHECKS_PER_FRAME).unwrap_err().contains("仍引用"));
}

#[test]
fn undo_and_redo_before_after_water_references_are_protected_without_discarding_history() {
    use crate::authoring::{EditorCommand, WaterLayersChange};
    let registry = registry(&catalog());
    for redo in [false, true] {
        for after_has_reference in [false, true] {
            for definitions in [false, true] {
                let mut session = session();
                let before = water(if after_has_reference { "" } else { "legacy-water" });
                let after = water(if after_has_reference { "water-main" } else { "" });
                let command = if definitions {
                    EditorCommand::WaterDefinitionsChanged { before, after }
                } else {
                    EditorCommand::WaterLayersChanged(Box::new(WaterLayersChange {
                        before, after, before_masks: Default::default(), after_masks: Default::default(),
                    }))
                };
                let scoped = EditorCommand::RootWater(Box::new(command));
                if redo { session.redo.push(scoped); } else { session.undo.push(scoped); }
                let mut pending = WaterDeletePreflight::new(&session, &registry, "water-main".into());
                assert!(pending.advance(&session, &registry, None, CHECKS_PER_FRAME).unwrap_err().contains("撤销/重做历史仍引用"));
                assert_eq!(session.undo.len() + session.redo.len(), 1, "引用保护不能清空作者历史");
            }
        }
    }
}

#[test]
fn each_frame_is_bounded_even_for_large_layers_and_many_empty_parked_canvases() {
    let registry = registry(&catalog());
    let mut session = session();
    let draft = session.draft.as_mut().unwrap();
    let mut large = water("");
    large.layers = vec![large.layers[0].clone(); CHECKS_PER_FRAME * 3];
    draft.water = Some(large);
    for index in 0..CHECKS_PER_FRAME * 2 {
        draft.terrain_documents.values.insert(TerrainId::parse(format!("empty-{index:04}")).unwrap(), Arc::new(TerrainCanvas::default()));
    }
    let pixels = Arc::new(vec![7_u8; 1024]);
    draft.water_masks.insert("held".into(), Arc::clone(&pixels));
    let mut pending = WaterDeletePreflight::new(&session, &registry, "water-main".into());
    let began = std::time::Instant::now();
    assert!(!pending.advance(&session, &registry, None, CHECKS_PER_FRAME).unwrap());
    assert_eq!(pending.layer, CHECKS_PER_FRAME);
    let first = began.elapsed();
    let mut frames = 1;
    let mut peak = first;
    loop {
        let start = std::time::Instant::now();
        let ready = pending.advance(&session, &registry, None, CHECKS_PER_FRAME).unwrap();
        peak = peak.max(start.elapsed());
        frames += 1;
        if ready { break; }
        assert!(frames <= 8, "游标必须前进，不能每帧从首项重扫");
    }
    assert_eq!(frames, 6, "每帧预算包含水层、源切换及空槽检查");
    assert_eq!(Arc::strong_count(&pixels), 2, "预检不得持有额外整图像素副本");
    eprintln!("water_delete_preflight metadata_budget={CHECKS_PER_FRAME}, frames={frames}, first_us={}, peak_us={}", first.as_micros(), peak.as_micros());
}

#[test]
fn author_water_terrain_map_and_catalog_changes_invalidate_the_preflight() {
    for change in 0..5 {
        let mut session = session();
        let mut registry = registry(&catalog());
        let mut pending = WaterDeletePreflight::new(&session, &registry, "water-main".into());
        match change {
            0 => session.revision += 1,
            1 => session.water_revision += 1,
            2 => session.terrain_revision += 1,
            3 => session.draft.as_mut().unwrap().map_id = MapId::parse("OTHER").unwrap(),
            _ => registry.revision += 1,
        }
        assert!(pending.advance(&session, &registry, None, CHECKS_PER_FRAME).unwrap_err().contains("已变化"));
    }
}

#[test]
fn delete_preflight_is_cancelled_before_write_and_releases_only_its_own_author_gate() {
    use bevy::ecs::system::RunSystemOnce;
    let registry = registry(&catalog());
    let mut session = session();
    let mut state = WaterPresetEditorState::default();
    start_delete(&mut state, &registry, &mut session, &catalog().presets[0]).unwrap();
    assert!(session.water_preset_delete_pending && state.saved_play_pending());
    assert!(!session.saving);
    let mut app = App::new();
    app.insert_resource(session).insert_resource(registry).insert_resource(state)
        .insert_resource(MapEditorState { open: false, ..Default::default() });
    app.world_mut().run_system_once(advance_delete_preflight).unwrap();
    let state = app.world().resource::<WaterPresetEditorState>();
    assert!(!state.busy() && state.write_task.is_none());
    assert!(!app.world().resource::<EditorAuthoringSession>().water_preset_delete_pending);
}

#[test]
fn third_delete_click_cancels_preflight_without_starting_file_io() {
    use crate::ui::water_preset_editor::{types::WaterPresetAction, workbench_tests::{interaction_app, press}};
    let mut app = interaction_app(WaterPresetEditorState::default());
    app.insert_resource(registry(&catalog()));
    app.world_mut().resource_mut::<MapEditorState>().water_selected_preset_id = Some("water-main".into());
    press(&mut app, WaterPresetAction::DeleteSelected);
    press(&mut app, WaterPresetAction::DeleteSelected);
    assert!(app.world().resource::<EditorAuthoringSession>().water_preset_delete_pending);
    press(&mut app, WaterPresetAction::DeleteSelected);
    assert!(!app.world().resource::<WaterPresetEditorState>().busy());
    assert!(!app.world().resource::<EditorAuthoringSession>().water_preset_delete_pending);
}

#[test]
fn reference_gate_keeps_the_draft_and_view_but_rejects_applying_a_new_preset() {
    use crate::ui::water_preset_editor::{types::WaterPresetAction, workbench_tests::{interaction_app, press}};
    let mut session = session();
    session.draft.as_mut().unwrap().water = Some(water(""));
    session.water_preset_delete_pending = true;
    let before = session.draft.as_ref().unwrap().water.clone();
    let mut app = interaction_app(WaterPresetEditorState::default());
    app.insert_resource(registry(&catalog())).insert_resource(session);
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.tool = crate::types::EditorTool::Water;
        editor.water_selected_preset_id = Some("water-main".into());
    }
    press(&mut app, WaterPresetAction::ApplyToSelectedStillWater);
    let session = app.world().resource::<EditorAuthoringSession>();
    let editor = app.world().resource::<MapEditorState>();
    assert_eq!(session.draft.as_ref().unwrap().water, before);
    assert!(!crate::authoring::water_canvas::writable(session, editor));
    assert!(crate::authoring::water_canvas::visible(session, editor));
    assert!(editor.status.contains("暂不能新增引用"));
    assert!(session.undo.is_empty() && !session.saving);
}

#[test]
fn file_write_failure_and_successful_empty_adoption_release_the_reference_gate() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::ui::status_bar::feedback_tests::{asset_app, ready_result};
    for failed in [false, true] {
        let mut app = asset_app();
        app.insert_resource(EditorAuthoringSession { water_preset_delete_pending: true, ..Default::default() });
        if failed {
            app.insert_resource(WaterPresetEditorState { write_task: Some(ready_result(Err("引用冲突".into()))), ..Default::default() });
        } else {
            app.world_mut().resource_mut::<WaterPresetRegistry>().revision = 2;
            app.insert_resource(WaterPresetEditorState { adoption: Some(WaterCatalogAdoption {
                baseline_revision: 1, preset_id: "water-main".into(), deleted: true, tab_id: None, message: "已删除".into(),
            }), ..Default::default() });
        }
        app.world_mut().run_system_once(poll_catalog_write).unwrap();
        assert!(!app.world().resource::<EditorAuthoringSession>().water_preset_delete_pending);
        assert!(!app.world().resource::<WaterPresetEditorState>().busy());
    }
}

#[test]
fn failed_delete_adoption_cancels_only_queued_intents_and_allows_close_decision() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::standalone::project_close_snapshot;
    let mut app = crate::ui::status_bar::feedback_tests::asset_app();
    app.insert_resource(EditorAuthoringSession { water_preset_delete_pending: true, ..Default::default() });
    let mut registry = registry(&catalog());
    registry.last_error = Some("目录加载校验失败".into());
    app.insert_resource(registry);
    app.insert_resource(WaterPresetEditorState { adoption: Some(WaterCatalogAdoption {
        baseline_revision: 1, preset_id: "water-main".into(), deleted: true, tab_id: None,
        message: "不应报告成功".into(),
    }), ..Default::default() });
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.dirty = false; // 此夹具没有地图草稿，只验证目录失败与尚未启动的排队意图。
        editor.save_requested = true;
        editor.pending_map_transition = Some(MapId::parse("QUEUED-MAP").unwrap());
    }
    assert!(project_close_snapshot(app.world()).pending.is_some());
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    let state = app.world().resource::<WaterPresetEditorState>();
    assert!(state.adoption.is_some(), "失败回执不能伪装采用成功或提前放开引用写入");
    assert_eq!(state.save_feedback_error.as_deref(), Some("目录加载校验失败"));
    assert!(state.status.contains("取消尚未开始"));
    assert!(app.world().resource::<EditorAuthoringSession>().water_preset_delete_pending);
    let editor = app.world().resource::<MapEditorState>();
    assert!(!editor.save_requested && editor.pending_map_transition.is_none());
    let snapshot = project_close_snapshot(app.world());
    assert!(snapshot.pending.is_none() && snapshot.blocked.is_none(),
        "实际写者已完成，宿主必须能呈现并处理关闭/放弃决定，而非永久等待：{snapshot:?}");
}
