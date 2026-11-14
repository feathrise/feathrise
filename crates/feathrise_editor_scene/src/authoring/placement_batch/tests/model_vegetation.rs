//! 模型复制使用独立稳定目标，局部锚点与密度随同一整批历史恢复；跨场景缺层拒绝整批。

use super::*;
use feathrise_scene_content::{ModelSurfaceAnchor, ModelSurfaceVegetation, ModelSurfaceVegetationSample, SceneVegetationAsset, SceneVegetationAuthoring, SurfaceTarget};

#[test]
fn copy_rebinds_model_vegetation_and_joint_undo_preserves_the_original() {
    use bevy::ecs::system::RunSystemOnce;
    let source = record("source-model", 1.0);
    let mut data = ModelSurfaceVegetation::empty(source.id.clone(), [7; 32], 0.25);
    data.samples.push(ModelSurfaceVegetationSample {
        anchor: ModelSurfaceAnchor { primitive: 2, triangle: 5, barycentric: [0.2, 0.3] },
        layers: vec![("grass-author".into(), 144)],
    });
    data.sample_blocks.push("scenes/source/vegetation/models/old.samples".into());
    let before = Arc::new(vec![Arc::new(data)]);
    let clipboard = Arc::new(ClipboardData {
        model_vegetation: before.clone(), records: vec![source.clone()], patch_frames: Arc::default(),
        origin: Vec3::from_array(source.translation), source_scene: map(),
    });
    let authoring: SceneVegetationAuthoring = serde_json::from_value(serde_json::json!({
        "groups":[{"id":"base","label":"基础","mode":"additive","output_merge":"maximum"}],
        "layers":[{"id":"grass-author","label":"草","group_id":"base","output_layer_id":"grass",
            "density_source_path":"scenes/source/vegetation/author/grass.r8","resolution":[2,2]}]
    })).unwrap();
    let make_work = |authoring| PasteWork {
        model_vegetation: Some(ModelVegetationPaste { source: clipboard.clone(), before: before.clone(), authoring }),
        source: worker::ghost_records(&clipboard, "independent-copy").unwrap(), delta: Vec3::new(2., 0., 2.),
        records: Vec::new(), validator: None, presets: Arc::new(vec![None]), sampler: None,
        work_plane: false, patch: None, query: None, bounds: bounds(), scene_id: map(), building_revision: 0,
    };
    let BatchOutput::Paste(Ok(prepared)) = worker::run_paste(make_work(Some(authoring.clone())), 1) else {
        panic!("带模型密度的普通副本应完整准备成功");
    };
    let transfer = prepared.history.model_vegetation.as_ref().expect("同笔历史必须持有根植被前后Arc");
    let after = transfer.after.clone();
    assert!(Arc::ptr_eq(&transfer.before, &before));
    assert_eq!(after.len(), 2);
    assert!(Arc::ptr_eq(&after[0], &before[0]));
    assert_ne!(prepared.apply[0].id, source.id);
    assert_eq!(after[1].target, SurfaceTarget::Model { object_id: prepared.apply[0].id.clone() });
    assert_eq!(after[1].topology, before[0].topology);
    assert_eq!(after[1].spacing_meters, before[0].spacing_meters);
    assert_eq!(after[1].samples, before[0].samples);
    assert!(after[1].sample_blocks.is_empty(), "副本下次保存必须生成属于新目标的独立块");
    assert_eq!(clipboard.records, vec![source.clone()]);
    assert_eq!(clipboard.model_vegetation[0].sample_blocks.len(), 1);
    let BatchOutput::Paste(Err(error)) = worker::run_paste(make_work(None), 1) else {
        panic!("跨场景缺作者层必须拒绝整个模型副本，不能默默丢植被");
    };
    assert!(error.contains("作者图层"));

    let (mut app, _, _) = crate::viewport::placement_move::interaction_tests::app();
    app.init_asset::<bevy::world_serialization::WorldAsset>(); // 原Undo/Redo恢复普通GLB预览必须使用已登记的正式资产类型。
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    {
        let mut session = app.world_mut().resource_mut::<crate::authoring::EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placements.placements = vec![source];
        draft.placements.placements.extend(prepared.apply.clone());
        draft.placement_water_surface_count = 0;
        draft.vegetation = Some(SceneVegetationAsset {
            scene_id: draft.scene_id.clone(), authoring: Some(authoring), model_surfaces: after.clone(), ..Default::default()
        });
        session.undo.push(crate::authoring::EditorCommand::CourtyardBatch(Box::new(prepared.history)));
    }
    for undo in [true, false] {
        let mut editor = app.world_mut().resource_mut::<crate::resources::SceneEditorState>();
        editor.undo_requested = undo;
        editor.redo_requested = !undo;
        drop(editor);
        crate::authoring::drive_undo_redo_to_completion(&mut app);
        let session = app.world().resource::<crate::authoring::EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.placements.placements.len(), if undo { 1 } else { 2 });
        assert!(Arc::ptr_eq(&draft.vegetation.as_ref().unwrap().model_surfaces, if undo { &before } else { &after }));
        app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
    }
}
