//! GLB预览替代权限的显隐回归：同图独立根、兄弟/子实例、空水网和切源均保留各自所有权。

use bevy::prelude::*;
use feathrise_map_content::{ActiveDynamicMap, MapId, MapTerrainDescriptor, MapTerrainKind, TerrainId};
use feathrise_scene::{
    DynamicWaterMaterial, DynamicWaterStatus, PaintedWaterPurpose, PaintedWaterSurface,
    PaintedWaterSurfaceSpawn, WaterGpuUploadQueue, WaterGpuUploadReceipts, WaterSurfaceKind,
};
use feathrise_scene_core::residency::MapInstanceOwner;
use std::sync::Arc;

use super::super::resources::{
    EditorAuthoringSession, PendingWaterPreviewBatch, WaterPreviewRuntimeTarget,
    WaterPreviewSource, WaterPreviewState,
};
use super::*;

fn surface(map: &MapId, purpose: PaintedWaterPurpose) -> PaintedWaterSurface {
    PaintedWaterSurface { map_id: map.clone(), layer_id: "lake".into(), kind: WaterSurfaceKind::Still, purpose }
}

#[derive(Resource)]
struct ResolvedScope(WaterPreviewSource, WaterPreviewRuntimeTarget);

/// 真实初次采用与Added系统都调用此助手；测试只注入已解析owner，不扩张场景私有状态接口。
fn apply_resolved_scope(
    mut commands: Commands,
    scope: Res<ResolvedScope>,
    mut preview: ResMut<WaterPreviewState>,
    surfaces: Query<(Entity, &PaintedWaterSurface, &Visibility, Option<&MapInstanceOwner>)>,
) {
    for surface in &surfaces {
        hide_replaced_surface(&mut commands, &mut preview, &scope.0, scope.1, surface);
    }
}

#[test]
fn glb_replacement_hides_only_exact_owner_and_preserves_same_map_root_siblings_and_submaps() {
    let mut app = App::new();
    let map = MapId::parse("preview-owned-water").unwrap();
    let selected = app.world_mut().spawn_empty().id();
    let sibling = app.world_mut().spawn_empty().id();
    let same_source_child = app.world_mut().spawn_empty().id();
    app.insert_resource(ResolvedScope(WaterPreviewSource { map_id: map.clone(),
        terrain_id: Some(TerrainId::parse("glb-canvas").unwrap()) }, WaterPreviewRuntimeTarget::Instance(selected)))
        .init_resource::<WaterPreviewState>().add_systems(Update, apply_resolved_scope);
    let own = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Visible, MapInstanceOwner(selected))).id();
    let root = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Inherited)).id();
    let sibling_water = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Inherited, MapInstanceOwner(sibling))).id();
    let child_water = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Inherited, MapInstanceOwner(same_source_child))).id();
    let foreign_map = MapId::parse("preview-other-map").unwrap();
    let foreign = app.world_mut().spawn((surface(&foreign_map, PaintedWaterPurpose::Runtime), Visibility::Inherited, MapInstanceOwner(selected))).id();
    let author = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::EditorPreview), Visibility::Inherited, MapInstanceOwner(selected))).id();
    app.update();
    app.update(); // 同一正式实体不能把已隐藏状态再次登记为“原始可见性”。
    assert_eq!(app.world().get::<Visibility>(own), Some(&Visibility::Hidden));
    for entity in [root, sibling_water, child_water, foreign, author] {
        assert_eq!(app.world().get::<Visibility>(entity), Some(&Visibility::Inherited));
    }
    assert_eq!(app.world().resource::<WaterPreviewState>().hidden_runtime_entities, vec![(own, Visibility::Visible)]);
}

fn glb_fixture() -> (App, MapId) {
    let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.map_id.clone();
    let source = WaterPreviewSource { map_id: map.clone(), terrain_id: None };
    let mut preview = WaterPreviewState::default();
    preview.retarget(Some(map.clone()), 8, 0);
    preview.pending = Some(PendingWaterPreviewBatch {
        source, water_revision: 8, terrain_revision: 0, preset_revision: 0,
        remaining: default(), staged: Vec::new(), warmup_entities: Vec::new(), triangle_count: 0, ticket: None,
    }); // 空完整批次代表删除全部水，仍须真正替代这一GLB来源的旧正式水。
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(State::new(ScenePhase::Active))
        .insert_resource(MapEditorState { open: true, selected_map_id: Some(map.clone()), ..default() })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), water_revision: 8, ..default() })
        .insert_resource(ActiveDynamicMap { map_id: Some(map.clone()) })
        .insert_resource(preview)
        .init_resource::<DynamicWaterStatus>()
        .init_resource::<Assets<Mesh>>().init_resource::<Assets<DynamicWaterMaterial>>()
        .init_resource::<WaterGpuUploadQueue>().init_resource::<WaterGpuUploadReceipts>()
        .add_systems(Update, (poll_water_preview_rebuild, suppress_new_runtime_water_surfaces).chain());
    (app, map)
}

#[test]
fn empty_glb_preview_replaces_legacy_root_and_its_later_formal_hot_reload() {
    let (mut app, map) = glb_fixture();
    let root = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Inherited)).id();
    let owner = app.world_mut().spawn_empty().id();
    let child = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Inherited, MapInstanceOwner(owner))).id();
    app.update();
    let preview = app.world().resource::<WaterPreviewState>();
    assert!(preview.ready_for(&map, 8, 0));
    assert!(preview.spawns.is_empty());
    assert!(preview.replacement_source.is_some(), "零三角形的合法删除预览也要拥有明确替代来源");
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    assert_eq!(app.world().get::<Visibility>(child), Some(&Visibility::Inherited));
    let reloaded = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Visible)).id();
    app.update();
    assert_eq!(app.world().get::<Visibility>(reloaded), Some(&Visibility::Hidden));
    assert_eq!(app.world().resource::<WaterPreviewState>().hidden_runtime_entities.len(), 2);
}

#[test]
fn leaving_glb_source_restores_formal_visibility_and_retires_author_preview() {
    for destination in [MapTerrainKind::Empty, MapTerrainKind::SparseClipmap, MapTerrainKind::Gltf] {
        let (mut app, map) = glb_fixture();
        let root = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Visible)).id();
        app.update();
        let author = app.world_mut().spawn((EditorWaterPreview,
            surface(&map, PaintedWaterPurpose::EditorPreview), Visibility::Inherited)).id();
        app.world_mut().resource_mut::<WaterPreviewState>().spawns.push(PaintedWaterSurfaceSpawn {
            entity: author, mesh: default(), material: default(), additional_meshes: Vec::new(), additional_entities: Vec::new(),
        });
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            if destination == MapTerrainKind::Gltf {
                let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
                object.id = TerrainId::parse("next-glb-canvas").unwrap();
                object.terrain.kind = destination;
                let manifest = object.source_manifest(&draft.manifest);
                draft.terrain_documents.active = Some(super::super::terrain_documents::TerrainDocumentIdentity {
                    definition: Arc::new(object.clone()), manifest, folder: draft.folder.clone(),
                });
                draft.manifest.terrains = Some(Arc::from([object]));
                draft.manifest.terrain = MapTerrainDescriptor::empty();
            } else {
                draft.manifest.terrain.kind = destination;
            }
        }
        app.update();
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Visible), "{destination:?}");
        let preview = app.world().resource::<WaterPreviewState>();
        assert!(preview.replacement_source.is_none());
        assert!(preview.replacement_target.is_none());
        assert!(preview.hidden_runtime_entities.is_empty());
        assert!(preview.spawns.is_empty());
        assert_eq!(app.world().get::<Visibility>(author), Some(&Visibility::Hidden));
        assert!(app.world().get::<PaintedWaterSurface>(author).is_none(), "退役预览不能继续参与互动或被再次显示");
    }
}

#[test]
fn late_glb_batch_cannot_adopt_on_another_canvas_with_identical_revision_numbers() {
    let (mut app, map) = glb_fixture();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("other-canvas").unwrap();
        let manifest = object.source_manifest(&draft.manifest);
        draft.terrain_documents.active = Some(super::super::terrain_documents::TerrainDocumentIdentity {
            definition: Arc::new(object.clone()), manifest, folder: draft.folder.clone(),
        });
        draft.manifest.terrains = Some(Arc::from([object]));
    }
    let root = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Inherited)).id();
    app.update();
    let preview = app.world().resource::<WaterPreviewState>();
    assert!(!preview.ready_for(&map, 8, 0));
    assert!(preview.pending.is_none());
    assert!(preview.replacement_source.is_none());
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
}

#[test]
fn private_glb_replacement_never_falls_back_to_the_ready_map_root() {
    let (mut app, map) = glb_fixture();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("private-glb").unwrap();
        let manifest = object.source_manifest(&draft.manifest);
        draft.terrain_documents.active = Some(super::super::terrain_documents::TerrainDocumentIdentity {
            definition: Arc::new(object.clone()), manifest, folder: draft.folder.clone(),
        });
        draft.manifest.terrains = Some(Arc::from([object]));
        draft.manifest.terrain = MapTerrainDescriptor::empty();
    }
    let source = current_source(app.world().resource::<EditorAuthoringSession>()).unwrap();
    app.world_mut().resource_mut::<WaterPreviewState>().pending.as_mut().unwrap().source = source;
    app.insert_resource(DynamicWaterStatus { map_id: Some(map.clone()), phase: feathrise_scene::DynamicWaterPhase::Ready,
        desired_generation: 3, applied_generation: 3, ..default() });
    let root = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Inherited)).id();
    app.update();
    let preview = app.world().resource::<WaterPreviewState>();
    assert!(preview.replacement_source.is_some());
    assert!(preview.replacement_target.is_none(), "私有实例尚不存在时不能回退到同图根owner");
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
    let added_root = app.world_mut().spawn((surface(&map, PaintedWaterPurpose::Runtime), Visibility::Visible)).id();
    app.update();
    assert_eq!(app.world().get::<Visibility>(added_root), Some(&Visibility::Visible));
    assert!(app.world().resource::<WaterPreviewState>().hidden_runtime_entities.is_empty());
}
