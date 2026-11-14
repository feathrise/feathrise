//! 模型密度通过原保存事务写入主文档和私有分块，再由生产读取完整重开。
use super::*;
use feathrise_scene_content::{ModelSurfaceAnchor, ModelSurfaceVegetation, ModelSurfaceVegetationSample, SceneVegetationAsset};

#[test]
fn model_samples_use_the_real_save_transaction_and_reopen_from_private_blocks() {
    let parent = std::env::temp_dir();
    let root = parent.join(format!("feathrise-model-save-{}-{}",std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0;121]));
    let mut model = ModelSurfaceVegetation::empty("scene-glb-plane",[7;32],0.25);
    model.samples.push(ModelSurfaceVegetationSample { anchor:ModelSurfaceAnchor { primitive:0, triangle:2,barycentric:[0.2,0.3] },
        layers:vec![("grass".into(),200)] });
    let targets = Arc::new(vec![Arc::new(model)]);
    let layer = feathrise_scene_content::SceneVegetationLayer { id:"grass".into(),label:"草".into(),
        kind:feathrise_scene_content::SceneVegetationKind::Blade,runtime_profile:Some("ordinary".into()),runtime_layer_id:Some(1),prototype_id:None,
        asset_path:"grass.glb".into(),density_source_path:format!("{}/vegetation/grass.r8",draft.folder),
        density_texture_path:format!("{}/vegetation/grass.ktx2",draft.folder),resolution:[64,64],spacing_meters:0.25,
        scale_range:[1.,1.],max_instances:4096,seed:1 };
    draft.vegetation = Some(SceneVegetationAsset { scene_id:draft.scene_id.clone(),layers:vec![layer],model_surfaces:targets.clone(), ..Default::default() });
    draft.vegetation_densities.insert("grass".into(),Arc::new(vec![0;4096]));
    draft.vegetation_dirty_layers.insert("grass".into());
    let source = super::super::super::terrain_documents::root_vegetation::VegetationSource::active(&draft);
    let mut writes = Vec::new();
    prepare_vegetation_writes(&root,&draft.folder,&source,&mut writes).unwrap();
    assert_eq!(writes.len(),4, "模型分块、元数据和原R8/KTX2属于同一真实事务");
    if let Err(failure) = commit_prepared_writes(writes) {
        panic!("模型密度与原二维文件应共同保存成功：{}", failure.message());
    }
    let path = root.join(&draft.folder).join(SCENE_VEGETATION_FILE_NAME);
    let mut loaded: SceneVegetationAsset = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(loaded.model_surfaces[0].samples.is_empty());
    assert_eq!(loaded.model_surfaces[0].sample_blocks.len(),1);
    feathrise_scene_content::load_model_vegetation_blocks(&root,&draft.folder,&mut loaded).unwrap();
    assert_eq!(loaded.model_surfaces,targets);
    assert_eq!(root.parent(),Some(parent.as_path()));
    std::fs::remove_dir_all(root).unwrap(); // 只清理本用例独占创建的临时根。
}
