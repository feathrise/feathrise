//! 植被目录保存的只读反馈适配回归，不调用真实目录文件事务或模型检查。

use super::*;
use crate::types::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget};
use crate::ui::status_bar::feedback_tests::{asset_app, ready_result, snapshot};
use bevy::ecs::system::RunSystemOnce;

#[test]
fn feedback_keeps_written_catalog_pending_and_only_original_adoption_publishes_success() {
    let mut app = asset_app();
    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    )))
    .unwrap();
    let id = catalog.presets[0].id.clone();
    let target = EditorStatusTarget::new(&id, "冻结植被名称");
    let slot = EditorStatusOperation::VegetationPresetSave.slot();
    app.world_mut()
        .resource_mut::<VegetationPresetRegistry>()
        .revision = 1;
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .save_feedback_target = Some(target.clone());
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .write_task = Some(ready_result(Ok(CatalogWriteResult {
        baseline_revision: 1,
        preset_id: id.clone(),
        deleted: false,
        tab_id: None,
        message: "已完整采用植被目录".into(),
    })));
    assert_eq!(snapshot(&mut app).0, "处理中");
    assert!(snapshot(&mut app).1.contains("后台处理中"));
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .adoption
            .is_some()
    );
    assert!(app.world().resource::<MapEditorState>().status_results[slot].is_none());
    assert!(snapshot(&mut app).1.contains("等待运行时目录采用"));
    app.world_mut()
        .resource_mut::<VegetationPresetRegistry>()
        .last_error = Some("植被目录/中文校验失败\n完整尾部".into());
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
            .resource::<VegetationPresetEditorState>()
            .adoption
            .is_some()
    );
    {
        let mut registry = app.world_mut().resource_mut::<VegetationPresetRegistry>();
        registry.resolved = ResolvedVegetationPresetCatalog::build(&catalog).unwrap();
        registry.revision = 2;
        registry.last_error = None;
    }
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    let editor = app.world().resource::<MapEditorState>();
    let receipt = editor.status_results[slot].as_ref().unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Success);
    assert_eq!(receipt.target.id, target.id);
    assert_eq!(receipt.target.label.as_ref(), "冻结植被名称");
    assert!(editor.dirty);
    assert_eq!(snapshot(&mut app).0, "已完成");
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>();
        state.save_feedback_target = Some(target);
        state.write_task = Some(ready_result(Err("植被目录并发冲突".into())));
    }
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
            .resource::<VegetationPresetEditorState>()
            .save_feedback_task()
            .is_none()
    );
}

#[test]
fn feedback_marks_original_target_mismatch_but_not_preset_delete_or_inspection() {
    let mut app = asset_app();
    app.world_mut()
        .resource_mut::<VegetationPresetRegistry>()
        .revision = 2;
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>();
        state.save_feedback_target = Some(EditorStatusTarget::new("missing", "缺失植被"));
        state.adoption = Some(CatalogAdoption {
            baseline_revision: 1,
            preset_id: "missing".into(),
            deleted: false,
            tab_id: None,
            message: "不能采用".into(),
        });
    }
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    let slot = EditorStatusOperation::VegetationPresetSave.slot();
    let receipt = app.world().resource::<MapEditorState>().status_results[slot]
        .as_ref()
        .unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Error);
    let sequence = receipt.sequence;
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .adoption = Some(CatalogAdoption {
        baseline_revision: 1,
        preset_id: "missing".into(),
        deleted: true,
        tab_id: None,
        message: "原删除完成".into(),
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
