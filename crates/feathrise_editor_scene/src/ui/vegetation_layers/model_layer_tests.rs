//! 模型样本和R8作者层的联合事务、真实历史与异步迟到结果回归。

use super::*;
use feathrise_scene_content::{ModelSurfaceAnchor, ModelSurfaceVegetation, ModelSurfaceVegetationSample, SceneVegetationAsset};

fn session() -> EditorAuthoringSession {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut authoring = authoring();
    let mut second = authoring.layers[0].clone();
    second.id = "second-grass".into();
    second.density_source_path = "scenes/test/vegetation/author/second-grass.r8".into();
    authoring.layers.push(second);
    let mut data = ModelSurfaceVegetation::empty("bridge", [9; 32], 0.25);
    for (triangle, layers) in [(0, vec![("base-grass".into(), 64), ("second-grass".into(), 31)]),
        (1, vec![("base-grass".into(), 192)])] {
        data.samples.push(ModelSurfaceVegetationSample {
            anchor: ModelSurfaceAnchor { primitive: 0, triangle, barycentric: [0.2, 0.3] }, layers,
        });
    }
    data.sample_blocks.push("scenes/test/vegetation/models/previous.samples".into());
    draft.vegetation = Some(SceneVegetationAsset {
        scene_id: draft.scene_id.clone(), layers: vec![output()], authoring: Some(authoring),
        model_surfaces: Arc::new(vec![Arc::new(data)]), ..Default::default()
    });
    draft.vegetation_author_densities = BTreeMap::from([
        ("base-grass".into(), Arc::new(vec![0, 64, 192, 0])),
        ("second-grass".into(), Arc::new(vec![31, 0, 0, 0])),
    ]);
    EditorAuthoringSession { draft: Some(draft), ..Default::default() }
}

fn models(session: &EditorAuthoringSession) -> Arc<Vec<Arc<ModelSurfaceVegetation>>> {
    session.draft.as_ref().unwrap().vegetation.as_ref().unwrap().model_surfaces.clone()
}

#[test]
fn model_layer_duplicate_and_delete_share_the_original_structure_history() {
    let mut session = session();
    let before = models(&session);
    let mut editor = SceneEditorState::default();
    let prepared = prepare_vegetation_authoring_action(EditorAction::VegetationDuplicateLayer, &editor, &session, None, None).unwrap();
    let prepared = crate::ui::vegetation_layers::model_layers::prepare(prepared, EditorAction::VegetationDuplicateLayer, "base-grass").unwrap();
    let duplicate_id = prepared.change.as_ref().unwrap().after.layers[1].id.clone();
    adopt_vegetation_authoring_action(prepared, &mut editor, &mut session).unwrap();
    let after = models(&session);
    assert!(!Arc::ptr_eq(&before, &after));
    for (source, copied) in before[0].samples.iter().zip(&after[0].samples) {
        assert_eq!(source.anchor, copied.anchor);
        let density = source.layers.iter().find(|(id, _)| id == "base-grass").unwrap().1;
        assert_eq!(copied.layers.iter().find(|(id, _)| id == &duplicate_id).unwrap().1, density);
    }
    assert!(after[0].sample_blocks.is_empty());
    assert_eq!(before[0].samples[0].layers.len(), 2, "后台写时复制必须保留旧样本");
    assert_eq!(session.undo.len(), 1);

    let (mut app, _, _) = crate::viewport::placement_move::interaction_tests::app();
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    editor.open = true;
    editor.selected_scene_id = Some(session.draft.as_ref().unwrap().scene_id.clone());
    app.insert_resource(session).insert_resource(editor);
    for undo in [true, false] {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.undo_requested = undo;
        editor.redo_requested = !undo;
        drop(editor);
        crate::authoring::drive_undo_redo_to_completion(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(Arc::ptr_eq(&models(session), if undo { &before } else { &after }));
        assert_eq!(session.draft.as_ref().unwrap().vegetation.as_ref().unwrap().authoring.as_ref().unwrap().layers.len(), if undo { 2 } else { 3 });
    }

    // 独立删除夹具覆盖：保留其他层密度，且删除无剩余密度的锚点，不能留悬空作者层ID。
    let mut session = self::session();
    let mut editor = SceneEditorState::default();
    let prepared = prepare_vegetation_authoring_action(EditorAction::VegetationDeleteSelected, &editor, &session, None, None).unwrap();
    let prepared = crate::ui::vegetation_layers::model_layers::prepare(prepared, EditorAction::VegetationDeleteSelected, "base-grass").unwrap();
    adopt_vegetation_authoring_action(prepared, &mut editor, &mut session).unwrap();
    assert_eq!(models(&session)[0].samples.len(), 1);
    assert_eq!(models(&session)[0].samples[0].layers, vec![("second-grass".into(), 31)]);
}

#[test]
fn model_layer_single_flight_discards_replaced_document_without_publishing_half_structure() {
    use crate::ui::vegetation_layers::allocation::{request_model_layer, VegetationBlankLayerAllocation, poll_vegetation_blank_layer_allocation};
    use bevy::prelude::*;
    bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let session = session();
    let before = models(&session);
    let editor = SceneEditorState { open: true, selected_scene_id: Some(session.draft.as_ref().unwrap().scene_id.clone()), ..default() };
    let mut app = App::new();
    app.insert_resource(editor).insert_resource(session).init_resource::<VegetationBlankLayerAllocation>()
        .add_systems(Update, poll_vegetation_blank_layer_allocation);
    app.world_mut().resource_scope(|world, mut allocation: Mut<VegetationBlankLayerAllocation>| {
        world.resource_scope(|world, mut editor: Mut<SceneEditorState>| {
            let session = world.resource::<EditorAuthoringSession>();
            assert!(request_model_layer(EditorAction::VegetationDuplicateLayer, &mut editor, session, &mut allocation).unwrap());
            assert!(request_model_layer(EditorAction::VegetationDeleteSelected, &mut editor, session, &mut allocation).is_err());
        });
    });
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    // 同场景、同修订的全新文档必须换弱身份；旧任务不能把新文档当原来源。
    app.world_mut().resource_mut::<EditorAuthoringSession>().placement_deletions = Default::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.world().resource::<VegetationBlankLayerAllocation>().busy() {
        app.update();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert!(Arc::ptr_eq(&models(session), &before));
    assert_eq!(session.draft.as_ref().unwrap().vegetation.as_ref().unwrap().authoring.as_ref().unwrap().layers.len(), 2);
}
