//! 地表保存反馈仅适配原目录采用门；注入纯结果，不写正式目录或纹理数组。

use super::*;
use bevy::prelude::*;
use crate::resources::SceneEditorState;
use feathrise_editor_surface_model::types::*;
use feathrise_editor_surface_model::SurfacePresetEditorState;
use feathrise_scene_content::{SurfacePresetCatalogAsset, SurfacePresetRegistry, ResolvedSurfacePresetCatalog};
use crate::types::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget};
use crate::ui::status_bar::feedback_tests::{acknowledge, asset_app, ready_result, snapshot};
use bevy::ecs::system::RunSystemOnce;

#[test]
fn feedback_keeps_disk_publication_pending_and_observes_adoption_failure_without_releasing_gate() {
    let mut app = asset_app();
    app.add_plugins(feathrise_editor_surface::SurfaceEditorPlugin);
    let catalog: SurfacePresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/surface_presets.surface_presets.json"
    )))
    .unwrap();
    let id = catalog.presets[0].id.clone();
    let target = EditorStatusTarget::new(&id, "冻结地表名称");
    let slot = EditorStatusOperation::SurfacePresetSave.slot();
    app.world_mut()
        .resource_mut::<SurfacePresetRegistry>()
        .revision = 1;
    app.insert_resource(SurfacePresetEditorState {
        save_feedback_target: Some(target.clone()),
        write_task: Some(ready_result(Ok(SurfaceCatalogWriteResult {
            baseline_revision: 1,
            preset_id: id.clone(),
            deleted: false,
            tab_id: None,
            message: "已完整采用地表目录".into(),
        }))),
        ..default()
    });
    assert_eq!(snapshot(&mut app).0, "处理中");
    assert!(snapshot(&mut app).1.contains("后台处理中"));
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert!(
        app.world()
            .resource::<SurfacePresetEditorState>()
            .adoption
            .is_some()
    );
    assert!(app.world().resource::<SceneEditorState>().status_results[slot].is_none());
    assert!(snapshot(&mut app).1.contains("等待运行时目录采用"));
    app.world_mut()
        .resource_mut::<SurfacePresetRegistry>()
        .last_error = Some("贴图路径/中文错误\n完整尾部".into());
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert_eq!(snapshot(&mut app).0, "失败");
    assert!(snapshot(&mut app).2.contains("完整尾部"));
    let sequence = app.world().resource::<SceneEditorState>().status_sequence;
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert_eq!(
        app.world().resource::<SceneEditorState>().status_sequence,
        sequence
    );
    assert!(
        app.world()
            .resource::<SurfacePresetEditorState>()
            .adoption
            .is_some()
    );
    acknowledge(&mut app);
    assert_eq!(snapshot(&mut app).0, "失败"); // 清除回执只确认阅读，仍等待的权威加载故障必须继续可见。
    {
        let mut registry = app.world_mut().resource_mut::<SurfacePresetRegistry>();
        registry.resolved = ResolvedSurfacePresetCatalog::build(&catalog).unwrap();
        registry.revision = 2;
        registry.last_error = None;
    }
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(feathrise_scene_content::SceneId::parse("MAP-another").unwrap());
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    let editor = app.world().resource::<SceneEditorState>();
    let receipt = editor.status_results[slot].as_ref().unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Success);
    assert_eq!(receipt.target.id, target.id);
    assert_eq!(receipt.target.label.as_ref(), "冻结地表名称");
    assert!(editor.dirty);
    assert_eq!(snapshot(&mut app).0, "已完成");
    assert!(
        app.world()
            .resource::<SurfacePresetEditorState>()
            .adoption
            .is_none()
    );
    app.insert_resource(SurfacePresetEditorState {
        save_feedback_target: Some(target),
        write_task: Some(ready_result(Err("文件原子发布失败".into()))),
        ..default()
    });
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    assert_eq!(
        app.world().resource::<SceneEditorState>().status_results[slot]
            .as_ref()
            .unwrap()
            .kind,
        EditorStatusKind::Error
    );
    assert!(
        app.world()
            .resource::<SurfacePresetEditorState>()
            .save_feedback_task()
            .is_none()
    );
}

#[test]
fn feedback_reports_original_target_mismatch_and_does_not_classify_preset_delete_as_save() {
    let mut app = asset_app();
    app.add_plugins(feathrise_editor_surface::SurfaceEditorPlugin);
    app.world_mut()
        .resource_mut::<SurfacePresetRegistry>()
        .revision = 2;
    app.insert_resource(SurfacePresetEditorState {
        save_feedback_target: Some(EditorStatusTarget::new("missing", "缺失地表")),
        adoption: Some(SurfaceCatalogAdoption {
            baseline_revision: 1,
            preset_id: "missing".into(),
            deleted: false,
            tab_id: None,
            message: "不能采用".into(),
        }),
        ..default()
    });
    app.world_mut().run_system_once(poll_catalog_write).unwrap();
    let slot = EditorStatusOperation::SurfacePresetSave.slot();
    let receipt = app.world().resource::<SceneEditorState>().status_results[slot]
        .as_ref()
        .unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Error);
    assert!(receipt.detail.contains("目标预设状态不匹配"));
    let sequence = receipt.sequence;
    app.insert_resource(SurfacePresetEditorState {
        adoption: Some(SurfaceCatalogAdoption {
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
        app.world().resource::<SceneEditorState>().status_results[slot]
            .as_ref()
            .unwrap()
            .sequence,
        sequence
    );
}
