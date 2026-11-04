//! 独立Clipmap整笔复用作者会话；重复刷印只发布局部页，不导出源版本或重建对象集合。
use super::*;
use crate::{
    authoring::{terrain_documents::TerrainDocumentIdentity, test_heightfield_draft},
    viewport::{LayerCandidateState, LayerTarget},
};
use bevy::ecs::system::RunSystemOnce;
use feathrise_map_content::{MapTerrainDescriptor, SparseHeightDocument, SparseHeightSpec, TerrainId};

#[test]
fn repeated_object_dabs_publish_live_tiles_without_rewriting_source_or_reselecting_mesh() {
    let _app = App::new().add_plugins(MinimalPlugins);
    let (root, map) = super::super::tests::test_map();
    let id = TerrainId::parse("live-clipmap").unwrap();
    let spec = SparseHeightSpec::new([-32.; 2], [32.; 2], 0.125, -80., 160.).unwrap();
    SparseHeightDocument::create_for_terrain(&root, map.clone(), Some(id.clone()), spec)
        .unwrap()
        .save_dirty()
        .unwrap();
    let path = format!(
        "maps/{map}/terrains/{}/height_tiles.map_height_manifest.json",
        id.as_str()
    );
    let original = std::fs::read(root.join(&path)).unwrap();
    let author = SparseTerrainAuthorSession::load_source(&root, &map, Some(&id), &path).unwrap();
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.map_id = map.clone();
    draft.manifest.id = map.clone();
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = id.clone();
    object.world_min = [-32.; 2];
    object.world_max = [32.; 2];
    object.terrain.kind = MapTerrainKind::SparseClipmap;
    object.terrain.height_path = Some(path.clone());
    object.terrain.resolution = [513; 2];
    object.terrain.base_height = -80.;
    object.terrain.height_scale = 160.;
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object.clone()),
        manifest: object.source_manifest(&draft.manifest),
        folder: object.private_folder(&map),
    });
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    let objects: Arc<[_]> = Arc::from([object]);
    draft.manifest.terrains = Some(objects.clone());
    let mut live = SparseHeightLiveOverride::default();
    live.retarget(Some(map.clone()), 0);
    let mut world = World::new();
    world.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    });
    world.insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(map.clone()),
        ..default()
    });
    world.insert_resource(GlobalEnvironmentAuthoringState::default());
    world.insert_resource(live);
    world.insert_resource(SparseTerrainAuthoringState {
        desired_map_id: Some(map.clone()),
        desired_source: Some((Some(id.clone()), path.clone())),
        author: Some(author),
        stroke: SparseTerrainStroke {
            active: true,
            tool: Some(EditorTool::Raise),
            ..default()
        },
        ..default()
    });
    let mut selection = LayerCandidateState::default();
    selection.confirm_terrain(&id, 0);
    let selection_revision = selection.selection_revision;
    world.insert_resource(selection);
    let elapsed = std::time::Instant::now();
    for _ in 0..12 {
        start_brush_task(
            &mut world.resource_mut::<SparseTerrainAuthoringState>(),
            SparseTerrainBrushInput {
                kind: SparseTerrainBrushKind::Raise,
                center_xz: [0.; 2],
                radius_meters: 2.,
                strength: 1.,
                speed_meters_per_second: 24.,
                delta_seconds: 1. / 60.,
                flatten_target_meters: None,
            },
            [1.; 2],
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while world
            .resource::<SparseTerrainAuthoringState>()
            .task
            .is_some()
        {
            world.run_system_once(poll_sparse_terrain_task).unwrap();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(Arc::ptr_eq(
            world
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .manifest
                .terrains
                .as_ref()
                .unwrap(),
            &objects
        ));
        assert_eq!(
            world.resource::<LayerCandidateState>().selected,
            Some(LayerTarget::TerrainObject(id.as_str().into()))
        );
        assert_eq!(
            world.resource::<LayerCandidateState>().selection_revision,
            selection_revision
        );
    }
    eprintln!(
        "独立Clipmap 12次局部刷印与实时页采用：{:?}（CPU回归，不含渲染）",
        elapsed.elapsed()
    );
    assert_eq!(
        std::fs::read(root.join(&path)).unwrap(),
        original,
        "刷印只改内存，保存前不改写源清单"
    );
    assert!(
        world
            .resource::<SparseTerrainAuthoringState>()
            .author
            .as_ref()
            .unwrap()
            .sample_world_height(0., 0.)
            .unwrap()
            .unwrap()
            > 4.7
    );
    assert_eq!(world.resource::<SparseHeightLiveOverride>().revision(), 12);
    world
        .resource_mut::<SparseTerrainAuthoringState>()
        .stroke
        .release_requested = true;
    world.resource_scope(|world, mut state: Mut<SparseTerrainAuthoringState>| {
        finish_sparse_stroke_if_ready(
            &mut state,
            &mut world.resource_mut::<EditorAuthoringSession>(),
        );
    });
    assert_eq!(world.resource::<EditorAuthoringSession>().undo.len(), 1);
    drop(world);
    std::fs::remove_dir_all(root).unwrap(); // 只回收本用例创建的独占临时目录。
}
