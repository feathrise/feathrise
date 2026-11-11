//! 普通关闭夹具的实际文档闭环；失败和重试均经过生产后台写者与工程保存回执。

use super::super::{batch_save::DocumentBatchSaveState, validation};
use super::*;
use crate::standalone::{
    ProjectSavePlugin, ProjectSaveProgress, project_save_progress, request_save_all,
};

fn finish(app: &mut App) -> ProjectSaveProgress {
    for _ in 0..1000 {
        app.update();
        let progress = project_save_progress(app.world());
        if !matches!(progress, ProjectSaveProgress::Running { .. }) {
            return progress;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("普通文档的有界后台保存未完成");
}

#[test]
fn close_document_conflict_retry_keeps_real_source_and_undo_redo() {
    let fixture = Fixture::new();
    let original = "普通文档原始内容\n";
    std::fs::write(fixture.root.join(validation::PATH), original).unwrap();
    let mut documents = DocumentState::default();
    documents.root = fixture.root.clone();
    documents
        .documents
        .push(fixture.document_at(validation::PATH));
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .add_plugins(ProjectSavePlugin)
        .insert_resource(documents)
        .init_resource::<DocumentBatchSaveState>()
        .add_systems(Update, session::drive_tasks);
    let before = validation::edit(app.world_mut()).unwrap();
    assert_eq!(before["dirty"], true);
    assert_eq!(before["can_undo"], true);
    assert_eq!(before["can_redo"], true);

    std::fs::write(fixture.root.join(validation::PATH), "外部冲突内容\n").unwrap();
    request_save_all(app.world_mut()).unwrap();
    assert!(
        matches!(finish(&mut app), ProjectSaveProgress::Failed { detail } if detail.contains("外部修改"))
    );
    assert_eq!(validation::snapshot(app.world()).unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(validation::PATH)).unwrap(),
        "外部冲突内容\n"
    );

    std::fs::write(fixture.root.join(validation::PATH), original).unwrap();
    request_save_all(app.world_mut()).unwrap();
    assert_eq!(finish(&mut app), ProjectSaveProgress::Succeeded);
    let after = validation::snapshot(app.world()).unwrap();
    assert_eq!(after["source"], before["source"]);
    assert_eq!(after["generation"], before["generation"]);
    assert_eq!(after["dirty"], false);
    assert_eq!(after["can_undo"], true);
    assert_eq!(after["can_redo"], true);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(validation::PATH)).unwrap(),
        before["source"].as_str().unwrap()
    );

    let mut documents = app.world_mut().resource_mut::<DocumentState>();
    let doc = documents.documents.first_mut().unwrap();
    doc.travel(true).unwrap();
    assert!(doc.source.contains("第二步临时修改"));
    doc.travel(false).unwrap();
    assert!(!doc.dirty());
    doc.travel(false).unwrap();
    assert_eq!(doc.source_text(), original);
}
