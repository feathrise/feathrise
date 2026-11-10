//! 水保存元数据的分帧预算、修订失效与本批对象资格冻结回归。
use super::*;
use crate::authoring::terrain_documents::{TerrainCanvas, TerrainDocumentIdentity};
use feathrise_scene_content::{SCENE_SCHEMA_VERSION, SceneTerrainDescriptor};

fn fixture(layers: usize) -> EditorAuthoringSession {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let template = draft.manifest.terrain_objects_for_edit()[0].clone();
    let mut objects = Vec::new();
    for name in ["c", "a", "b"] {
        let mut object = template.clone(); object.id = TerrainId::parse(name).unwrap();
        let folder = object.private_folder(&draft.scene_id);
        object.water_path = Some(format!("{folder}/water.scene_water.json"));
        object.enabled = name != "b";
        let mut water = SceneWaterAsset { schema_version: SCENE_SCHEMA_VERSION, scene_id: draft.scene_id.clone(), ..Default::default() };
        for index in 0..layers {
            let mut layer = crate::authoring::water_defaults::default_water_layer(&folder);
            layer.id = format!("lake-{index:03}");
            layer.mask_source_path = format!("{folder}/water/{index}.r8");
            layer.mask_texture_path = format!("{folder}/water/{index}.ktx2");
            water.layers.push(layer);
        }
        let mut canvas = TerrainCanvas { identity: Some(TerrainDocumentIdentity {
            definition: Arc::new(object.clone()), manifest: object.source_manifest(&draft.manifest), folder }),
            water_dirty_layers: water.layers.iter().map(|layer| layer.id.clone()).collect(),
            water_asset_path: object.water_path.clone().unwrap(), water: Some(water), ..Default::default() };
        if name == "c" { canvas.swap(&mut draft); }
        else { draft.terrain_documents.values.insert(object.id.clone(), Arc::new(canvas)); }
        objects.push(object);
    }
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(objects.into());
    EditorAuthoringSession { draft: Some(draft), ..Default::default() }
}
fn finish(capture: &mut WaterSaveCapture, session: &EditorAuthoringSession) {
    for _ in 0..100 {
        if capture.step(session, &WaterFileHandoff::default()) { return; }
    }
    panic!("固定小型夹具应在有界步数内捕获完成");
}

#[test]
fn metadata_capture_is_layer_bounded_sorted_and_freezes_owner_enabled_or_deleted_state() {
    // 即使预检前存在超层数输入，捕获本身每帧也只能读取64层，不能先扫描整层表再拒绝。
    let mut session = fixture(70);
    let mut capture = WaterSaveCapture::default();
    assert!(!capture.step(&session, &WaterFileHandoff::default())); // 本批清单8对象索引预算。
    assert!(!capture.step(&session, &WaterFileHandoff::default()));
    assert_eq!(capture.current.as_ref().unwrap().0.dirty_layers.len(), 64);
    assert!(capture.targets.is_empty());
    finish(&mut capture, &session);
    let targets = capture.take();
    assert_eq!(targets.iter().map(|target| target.terrain_id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert!(targets.iter().all(|target| target.dirty_layers.len() == 70));
    assert!(targets[0].runtime_required);
    assert!(!targets[1].runtime_required, "停放槽的旧enabled=true也不能覆盖owner禁用值");
    assert!(targets[2].runtime_required);
    // 停放脏文件为撤销删除继续保存，但不等运行图中不存在的对象。
    let draft = session.draft.as_mut().unwrap();
    let objects: Vec<_> = draft.manifest.terrains.as_ref().unwrap().iter().filter(|object| object.id.as_str() != "b").cloned().collect();
    draft.manifest.terrains = Some(objects.into());
    finish(&mut capture, &session);
    assert!(!capture.take()[1].runtime_required);
}

#[test]
fn author_revision_object_cow_and_active_switch_each_restart_partial_capture() {
    let mut session = fixture(70);
    let mut capture = WaterSaveCapture::default();
    capture.step(&session, &WaterFileHandoff::default());
    capture.step(&session, &WaterFileHandoff::default());
    assert!(capture.current.is_some());
    session.water_revision += 1;
    capture.step(&session, &WaterFileHandoff::default());
    assert!(capture.current.is_none(), "新修订必须丢弃前一批半层表");
    capture.step(&session, &WaterFileHandoff::default());
    let draft = session.draft.as_mut().unwrap();
    let before = draft.manifest.terrains.as_ref().unwrap().clone();
    Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[1].enabled = false;
    capture.step(&session, &WaterFileHandoff::default());
    assert!(capture.current.is_none(), "结构COW即使尚未推进作者修订，也必须使已捕获资格失效");
    assert!(!Arc::ptr_eq(&before, session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()));
    capture.step(&session, &WaterFileHandoff::default());
    let draft = session.draft.as_mut().unwrap();
    let mut incoming = (*draft.terrain_documents.values.remove(&TerrainId::parse("a").unwrap()).unwrap()).clone();
    incoming.swap(draft);
    let old_id = incoming.identity.as_ref().unwrap().definition.id.clone();
    draft.terrain_documents.values.insert(old_id, Arc::new(incoming));
    capture.step(&session, &WaterFileHandoff::default());
    assert!(capture.current.is_none(), "切换画布不能沿用旧活动身份的半截捕获");
    finish(&mut capture, &session);
    assert_eq!(capture.take().len(), 3);
}

#[test]
fn empty_explicit_container_produces_no_implicit_water_target() {
    let mut session = fixture(1);
    let draft = session.draft.as_mut().unwrap();
    draft.manifest.terrains = Some(Arc::from([]));
    draft.terrain_documents = Default::default();
    draft.water = None; // 移除原private活动描述，空容器自身没有隐式根水。
    let mut capture = WaterSaveCapture::default();
    assert!(capture.step(&session, &WaterFileHandoff::default()));
    assert!(capture.take().is_empty());
}
