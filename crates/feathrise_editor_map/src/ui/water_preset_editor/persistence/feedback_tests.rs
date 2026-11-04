//! 水体保存回执与原目录采用门的适配回归，不运行文件发布或创建水面。

use super::*;
use crate::types::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget};
use crate::ui::status_bar::feedback_tests::{asset_app, ready_result, snapshot};
use bevy::ecs::system::RunSystemOnce;

#[test]
fn feedback_waits_for_catalog_adoption_and_keeps_failure_without_changing_original_gate() {
    let mut app = asset_app();
    let catalog: WaterPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/water_presets.water_presets.json"
    )))
    .unwrap();
    let id = catalog.presets[0].id.clone();
    let target = EditorStatusTarget::new(&id, "冻结水体名称");
    let slot = EditorStatusOperation::WaterPresetSave.slot();
    app.world_mut()
        .resource_mut::<WaterPresetRegistry>()
        .revision = 1;
    app.insert_resource(WaterPresetEditorState {
        save_feedback_target: Some(target.clone()),
        write_task: Some(ready_result(Ok(WaterCatalogWriteResult {
            baseline_revision: 1,
            preset_id: id.clone(),
            deleted: false,
            tab_id: None,
            message: "已完整采用水体目录".into(),
        }))),
        ..default()
    });
    assert_eq!(snapshot(&mut app).0, "处理中");
    assert!(snapshot(&mut app).1.contains("后台处理中"));
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert!(
        app.world()
            .resource::<WaterPresetEditorState>()
            .adoption
            .is_some()
    );
    assert!(app.world().resource::<MapEditorState>().status_results[slot].is_none());
    assert!(snapshot(&mut app).1.contains("等待运行时目录采用"));
    app.world_mut()
        .resource_mut::<WaterPresetRegistry>()
        .last_error = Some("水体目录/中文加载失败\n尾部".into());
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert_eq!(snapshot(&mut app).0, "失败");
    let sequence = app.world().resource::<MapEditorState>().status_sequence;
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert_eq!(
        app.world().resource::<MapEditorState>().status_sequence,
        sequence
    );
    assert!(
        app.world()
            .resource::<WaterPresetEditorState>()
            .adoption
            .is_some()
    );
    {
        let mut registry = app.world_mut().resource_mut::<WaterPresetRegistry>();
        registry.resolved = ResolvedWaterPresetCatalog::build(&catalog).unwrap();
        registry.revision = 2;
        registry.last_error = None;
    }
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    let editor = app.world().resource::<MapEditorState>();
    let receipt = editor.status_results[slot].as_ref().unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Success);
    assert_eq!(receipt.target.id, target.id);
    assert_eq!(receipt.target.label.as_ref(), "冻结水体名称");
    assert!(editor.dirty);
    assert_eq!(snapshot(&mut app).0, "已完成");
    app.insert_resource(WaterPresetEditorState {
        save_feedback_target: Some(target),
        write_task: Some(ready_result(Err("水体目录并发冲突".into()))),
        ..default()
    });
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert_eq!(
        app.world().resource::<MapEditorState>().status_results[slot]
            .as_ref()
            .unwrap()
            .kind,
        EditorStatusKind::Error
    );
    assert!(
        app.world()
            .resource::<WaterPresetEditorState>()
            .save_feedback_task()
            .is_none()
    );
}

#[test]
fn feedback_target_mismatch_is_error_but_original_delete_is_not_a_save_receipt() {
    let mut app = asset_app();
    app.world_mut()
        .resource_mut::<WaterPresetRegistry>()
        .revision = 2;
    app.insert_resource(WaterPresetEditorState {
        save_feedback_target: Some(EditorStatusTarget::new("missing", "缺失水体")),
        adoption: Some(WaterCatalogAdoption {
            baseline_revision: 1,
            preset_id: "missing".into(),
            deleted: false,
            tab_id: None,
            message: "不能采用".into(),
        }),
        ..default()
    });
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    let slot = EditorStatusOperation::WaterPresetSave.slot();
    let receipt = app.world().resource::<MapEditorState>().status_results[slot]
        .as_ref()
        .unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Error);
    let sequence = receipt.sequence;
    app.insert_resource(WaterPresetEditorState {
        adoption: Some(WaterCatalogAdoption {
            baseline_revision: 1,
            preset_id: "missing".into(),
            deleted: true,
            tab_id: None,
            message: "原删除完成".into(),
        }),
        ..default()
    });
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert_eq!(
        app.world().resource::<MapEditorState>().status_results[slot]
            .as_ref()
            .unwrap()
            .sequence,
        sequence
    );
}
