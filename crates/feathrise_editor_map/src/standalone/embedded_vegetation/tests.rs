//! 运行真实投影系统与正式DynamicBladeDraft发布入口；不写文件、不修改作者历史。

use super::*;
#[path = "target_credit_tests.rs"]
mod target_credit_tests;
use target_credit_tests::terrain_credit;
use crate::authoring::{EditorAuthoringSession, VegetationBrushStroke};
use feathrise_map_content::{MapId, MapVegetationAsset, embedded_preview::*};
use feathrise_scene::DynamicBladeDraft;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc, time::{Duration, Instant}};

fn setup() -> (App, MapId) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(VegetationPreviewPlugin);
    let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.map_id.clone();
    app.insert_resource(EditorAuthoringSession { draft: Some(draft), vegetation_structure_revision: 1,
        vegetation_compiled_structure_revision: 1, ..default() });
    app.insert_resource(VegetationBrushStroke { active: true, ..default() });
    app.init_resource::<DynamicBladeDraft>();
    let mut state = app.world_mut().resource_mut::<EmbeddedVegetationState>();
    state.enabled = true; state.session = 7; state.generation = 1;
    (app, map)
}

fn publish(world: &mut World, map: &MapId, revision: u64) {
    world.resource_mut::<DynamicBladeDraft>().publish(feathrise_scene::DynamicBladeDraftSnapshot {
        source_revision: revision, definition: MapVegetationAsset { map_id: map.clone(), ..default() },
        sources: BTreeMap::new(), densities: BTreeMap::new(), nonzero_layers: BTreeSet::new(),
    });
}

fn until(app: &mut App, predicate: impl Fn(&World) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        if predicate(app.world()) { break; }
        assert!(Instant::now() < deadline, "后台植被投影没有在有限预算内完成");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn held_stroke_projects_completed_versions_and_ack_preserves_newer_publication() {
    let (mut app, map) = setup();
    publish(app.world_mut(), &map, 3);
    until(&mut app, |world| world.resource::<EmbeddedVegetationState>().ready.is_some());
    let first = app.world().resource::<EmbeddedVegetationState>().ready.clone().unwrap();
    assert!(app.world().resource::<VegetationBrushStroke>().active, "不得等松手才发布");
    assert_eq!(decode_vegetation_preview(&first.bytes).unwrap().canvases[0].source_revision, 3);
    publish(app.world_mut(), &map, 5);
    for _ in 0..10 { app.update(); }
    assert_eq!(app.world().resource::<EmbeddedVegetationState>().ready.as_ref().unwrap().sequence, first.sequence);
    app.world_mut().resource_mut::<EmbeddedVegetationState>().acknowledged_sequence = first.sequence;
    until(&mut app, |world| world.resource::<EmbeddedVegetationState>().ready.as_ref()
        .is_some_and(|ready| ready.sequence > first.sequence));
    let second = app.world().resource::<EmbeddedVegetationState>().ready.as_ref().unwrap();
    assert_eq!(decode_vegetation_preview(&second.bytes).unwrap().canvases[0].source_revision, 5);
    let author = app.world().resource::<EditorAuthoringSession>();
    assert!(author.undo.is_empty()); assert_eq!(author.successful_save_sequence, 0);
}

#[test]
fn reconnect_drops_old_ack_and_structure_change_rejects_inflight_completion() {
    let (mut app, map) = setup();
    publish(app.world_mut(), &map, 3);
    app.update(); // 启动旧来源后台编码。
    app.world_mut().resource_mut::<EditorAuthoringSession>().vegetation_structure_revision = 2;
    app.world_mut().resource_mut::<EditorAuthoringSession>().vegetation_compiled_structure_revision = 2;
    publish(app.world_mut(), &map, 4);
    until(&mut app, |world| world.resource::<EmbeddedVegetationState>().ready.as_ref()
        .is_some_and(|ready| ready.structure_revision == 2));
    let old = app.world().resource::<EmbeddedVegetationState>().ready.clone().unwrap();
    assert_eq!(old.source_revision, 4);
    {
        let mut state = app.world_mut().resource_mut::<EmbeddedVegetationState>();
        state.acknowledged_sequence = old.sequence;
        state.session = 8;
    }
    until(&mut app, |world| world.resource::<EmbeddedVegetationState>().ready.as_ref()
        .is_some_and(|ready| ready.session == 8));
    let ready = app.world().resource::<EmbeddedVegetationState>().ready.as_ref().unwrap();
    assert!(ready.sequence > old.sequence); assert_eq!(ready.source_revision, 4);
}

fn explicit_canvases(app:&mut App,map:&MapId,with_instances:bool)->(Vec<feathrise_map_content::MapTerrainObject>,Vec<Entity>) {
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    use feathrise_scene::TerrainObjectAuthoringTarget;
    use feathrise_scene_core::residency::InstanceResourceTable;
    let objects={
        let mut session=app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft=session.draft.as_mut().unwrap();
        let mut first=draft.manifest.terrain_objects_for_edit()[0].clone();
        first.id=feathrise_map_content::TerrainId::parse("grass-a").unwrap();
        let mut second=first.clone();second.id=feathrise_map_content::TerrainId::parse("grass-b").unwrap();
        let objects=vec![first.clone(),second];
        draft.manifest.terrains=Some(Arc::from(objects.clone()));
        draft.manifest.terrain=feathrise_map_content::MapTerrainDescriptor::empty();
        draft.terrain_documents.active=Some(TerrainDocumentIdentity{manifest:first.source_manifest(&draft.manifest),
            folder:first.private_folder(map),definition:Arc::new(first)});
        objects
    };
    let parents=feathrise_scene::runtime_test_support::install_active_terrain_instances(
        app.world_mut(),map,if with_instances {&objects}else{&[]});
    let mut table=InstanceResourceTable::<DynamicBladeDraft>::default();
    for entity in &parents {table.values.insert(*entity,default());}
    app.insert_resource(table).init_resource::<TerrainObjectAuthoringTarget>();
    {
        let mut target=app.world_mut().resource_mut::<TerrainObjectAuthoringTarget>();
        target.map=Some(map.clone());target.terrain=Some(objects[0].id.clone());
    }
    feathrise_scene::runtime_test_support::apply_authoring_target(app.world_mut());
    (objects,parents)
}

#[test]
fn embedded_vegetation_uses_real_batch_activity_for_sibling_and_logical_root_ownership() {
    use feathrise_scene::{SubMapActive,SubMapInstance};
    use feathrise_scene_core::residency::InstanceResourceTable;
    let (mut app,map)=setup();
    let (objects,parents)=explicit_canvases(&mut app,&map,true);
    terrain_credit(&mut app, &map, &objects);
    assert!(app.world().get::<SubMapActive>(parents[0]).is_none(),"真实terrain节点不冒充批次根");
    let batch=app.world().get::<SubMapInstance>(parents[1]).unwrap().batch;
    assert!(app.world().get::<SubMapActive>(batch).is_some());
    publish(app.world_mut(),&map,7);
    let sibling=|world:&mut World,revision|world.resource_mut::<InstanceResourceTable<DynamicBladeDraft>>().values.get_mut(&parents[1]).unwrap()
        .publish(feathrise_scene::DynamicBladeDraftSnapshot{source_revision:revision,
            definition:MapVegetationAsset{map_id:map.clone(),..default()},sources:BTreeMap::new(),densities:BTreeMap::new(),nonzero_layers:BTreeSet::new()});
    sibling(app.world_mut(),8);
    until(&mut app,|world|world.resource::<EmbeddedVegetationState>().ready.is_some());
    let first=app.world().resource::<EmbeddedVegetationState>().ready.clone().unwrap();
    let canvases=decode_vegetation_preview(&first.bytes).unwrap().canvases;
    assert_eq!(canvases.len(),2);
    assert!(canvases.iter().any(|canvas|canvas.terrain_id.as_ref()==Some(&objects[0].id)&&canvas.source_revision==7));
    assert!(canvases.iter().any(|canvas|canvas.terrain_id.as_ref()==Some(&objects[1].id)&&canvas.source_revision==8));
    app.world_mut().resource_mut::<EmbeddedVegetationState>().acknowledged_sequence=first.sequence;
    app.update();
    app.world_mut().entity_mut(batch).remove::<SubMapActive>();
    sibling(app.world_mut(),9);
    for _ in 0..8 {app.update();}
    assert!(app.world().resource::<EmbeddedVegetationState>().ready.is_none(),"退役批次的兄弟密度不能发给当前游戏");
    app.world_mut().entity_mut(batch).insert(SubMapActive);
    until(&mut app,|world|world.resource::<EmbeddedVegetationState>().ready.is_some());
    let next=decode_vegetation_preview(&app.world().resource::<EmbeddedVegetationState>().ready.as_ref().unwrap().bytes).unwrap();
    assert_eq!(next.canvases.len(),1);assert_eq!(next.canvases[0].source_revision,9);
    assert_eq!(next.canvases[0].terrain_id.as_ref(),Some(&objects[1].id));
}

#[test]
fn embedded_vegetation_applied_author_canvas_publishes_before_local_physical_instance_exists() {
    use feathrise_scene::TerrainObjectAuthoringTarget;
    let (mut app,map)=setup();
    let (objects,_)=explicit_canvases(&mut app,&map,false);
    terrain_credit(&mut app, &map, &objects);
    let target=app.world().resource::<TerrainObjectAuthoringTarget>();
    assert!(target.instance().is_none());assert!(target.applied_to(&map,&objects[0].id));
    publish(app.world_mut(),&map,7);
    until(&mut app,|world|world.resource::<EmbeddedVegetationState>().ready.is_some());
    let ready=app.world().resource::<EmbeddedVegetationState>().ready.clone().unwrap();
    let canvas=&decode_vegetation_preview(&ready.bytes).unwrap().canvases[0];
    assert_eq!(canvas.terrain_id.as_ref(),Some(&objects[0].id));assert_eq!(canvas.source_revision,7);
    assert!(app.world().resource::<VegetationBrushStroke>().active);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}
