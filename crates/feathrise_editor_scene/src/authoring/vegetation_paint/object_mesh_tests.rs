//! 独立Clipmap的正式树木预设真实拖刷、完整Mesh快照和整笔历史；不读写场景或加载模型。
use super::*;
use crate::authoring::{
    live_density::{LiveBladeDensityState, sync_live_blade_density},
    vegetation_compile::{VegetationCompileState, poll_vegetation_compile, start_vegetation_compile},
};
use bevy::ecs::system::RunSystemOnce;
use feathrise_scene::{DynamicBladeDraft, DynamicBladeDraftSnapshot};

struct TreeLayers {
    scene_id: SceneId,
    grass_output: String,
    grass_author: String,
    tree_output: String,
    tree_author: String,
    tree_texture: String,
}

fn tree_fixture() -> (App, TreeLayers) {
    let mut app = fixture(); // 共用真实相机、显式Clipmap、Collider及异步按压来源校验。
    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    ))).unwrap();
    let catalog = ResolvedVegetationPresetCatalog::build(&catalog).unwrap();
    let ids = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let folder = draft.terrain_documents.active.as_ref().unwrap().folder.clone();
        let mut grass = catalog.resolve("hn01_ordinary_grass").unwrap().create_scene_layer(&folder);
        let mut tree = catalog.resolve("hn01_tree").unwrap().create_scene_layer(&folder);
        assert_eq!(grass.kind, SceneVegetationKind::Blade);
        assert_eq!(tree.kind, SceneVegetationKind::Mesh, "必须来自真实Mesh目录项，不能仅给Blade改名称");
        assert!(tree.prototype_id.is_some());
        // 只缩小作者精度，保留正式树木profile、原型、种子及散布参数。
        grass.resolution = [129; 2]; tree.resolution = [129; 2];
        let mut vegetation = SceneVegetationAsset {
            schema_version: SCENE_SCHEMA_VERSION, scene_id: draft.scene_id.clone(),
            layers: vec![grass.clone(), tree.clone()], authoring: None,
        };
        crate::authoring::vegetation_defaults::ensure_vegetation_authoring(&mut vegetation, &folder);
        let authors = &vegetation.authoring.as_ref().unwrap().layers;
        let ids = TreeLayers {
            scene_id: draft.scene_id.clone(), grass_output: grass.id.clone(), grass_author: authors[0].id.clone(),
            tree_output: tree.id.clone(), tree_author: authors[1].id.clone(), tree_texture: tree.density_texture_path.clone(),
        };
        assert_eq!(authors[1].output_layer_id, tree.id, "选中的作者层必须编译回树木正式输出");
        vegetation.authoring.as_ref().unwrap().validate(&vegetation.layers).unwrap();
        draft.vegetation = Some(vegetation);
        draft.vegetation_author_densities.clear(); draft.vegetation_densities.clear();
        for (author, output, value) in [(&ids.grass_author, &ids.grass_output, 37), (&ids.tree_author, &ids.tree_output, 0)] {
            draft.vegetation_author_densities.insert(author.clone(), Arc::new(vec![value; 129 * 129]));
            draft.vegetation_densities.insert(output.clone(), Arc::new(vec![value; 129 * 129]));
        }
        // 显式Clipmap的射线与分页作者链不能意外借用测试原始GLB或整图高度。
        draft.heights = None; draft.terrain_mesh = None; draft.terrain_editing_index = None;
        session.vegetation_revision = 1; // 由真实后台编译建立第一份空树木/非空草地基线。
        ids
    };
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.selected_vegetation_layer = 1; // 正式树木对应的一对一作者层，草地保留为未选中对照。
        editor.brush_radius = 0.75;
    }
    app.init_asset::<Image>()
        .init_resource::<VegetationCompileState>()
        .init_resource::<LiveBladeDensityState>()
        .init_resource::<DynamicBladeDraft>()
        .insert_resource(ActiveDynamicScene { scene_id: Some(ids.scene_id.clone()) })
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
        .add_systems(Update, (poll_vegetation_compile, start_vegetation_compile, sync_live_blade_density)
            .chain().after(apply_vegetation_brush));
    (app, ids)
}

fn current_snapshot(app: &mut App, ids: &TreeLayers) -> Arc<DynamicBladeDraftSnapshot> {
    until(app, |app| {
        let session = app.world().resource::<EditorAuthoringSession>();
        session.vegetation_compiled_revision == session.vegetation_revision
            && app.world().resource::<DynamicBladeDraft>().for_scene(&ids.scene_id)
                .is_some_and(|snapshot| snapshot.source_revision == session.vegetation_revision)
    });
    app.world().resource::<DynamicBladeDraft>().frozen_for_scene(&ids.scene_id).unwrap()
}

fn assert_snapshot(app: &App, ids: &TreeLayers, snapshot: &DynamicBladeDraftSnapshot, tree: &[u8]) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(snapshot.definition.layers.len(), 2, "完整快照必须同时保留草地与树木");
    let output = snapshot.definition.layers.iter().find(|layer| layer.id == ids.tree_output).unwrap();
    assert_eq!(output.kind, SceneVegetationKind::Mesh);
    assert_eq!(output.runtime_profile.as_deref(), Some("hn01_tree"));
    assert!(output.prototype_id.is_some());
    assert_eq!(snapshot.sources.len(), 2);
    assert_eq!(snapshot.densities.len(), 2);
    assert_eq!(snapshot.sources[&ids.tree_output].as_ref(), tree);
    assert_eq!(draft.vegetation_densities[&ids.tree_output].as_slice(), tree);
    assert_eq!(pixels(app, &ids.tree_author), tree);
    assert_eq!(pixels(app, &ids.grass_author), vec![37; 129 * 129], "树木笔刷不能写入草地作者R8");
    assert_eq!(snapshot.sources[&ids.grass_output].as_ref(), &[37; 129 * 129]);
    assert_eq!(draft.vegetation_densities[&ids.grass_output].as_slice(), &[37; 129 * 129]);
    assert!(snapshot.nonzero_layers.contains(&ids.grass_output));
    assert_eq!(snapshot.nonzero_layers.contains(&ids.tree_output), tree.iter().any(|value| *value > 0));
    for output in [&ids.grass_output, &ids.tree_output] {
        let image = app.world().resource::<Assets<Image>>().get(&snapshot.densities[output]).unwrap();
        assert_eq!(image.data.as_deref(), Some(snapshot.sources[output].as_ref()), "实际Image与同代R8应逐字节一致");
    }
    assert!(app.world().resource::<AssetServer>().get_handle::<Image>(ids.tree_texture.clone()).is_none(),
        "未保存树木必须使用内存快照，不能等待不存在的KTX2");
}

fn install_tree_history(app: &mut App) {
    use crate::authoring::*;
    // 公共历史入口的固定轻量资源；本回归不加载门户、地表或水域资产。
    app.insert_resource(EditorPreviewAssets {
        portal_base_mesh: default(), portal_arrow_head_mesh: default(), portal_arrow_shaft_mesh: default(),
        portal_edge_beam_mesh: default(), portal_edge_ring_mesh: default(), portal_base_material: default(),
        portal_arrow_material: default(), portal_edge_beam_material: default(), portal_edge_ring_material: default(),
        portal_label_font: default(),
    })
        .init_resource::<courtyards::CourtyardAuthoringState>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<WaterMaskMaterializeTask>()
        .init_resource::<SurfaceLayerEditTask>()
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<WaterFileHandoff>();
}

#[test]
fn explicit_clipmap_tree_drag_compiles_mesh_snapshot_and_full_publish_with_one_undo_redo() {
    let (mut app, ids) = tree_fixture();
    let empty = vec![0; 129 * 129];
    let initial = current_snapshot(&mut app, &ids);
    assert_snapshot(&app, &ids, &initial, &empty);
    let initial_layout = app.world().resource::<DynamicBladeDraft>().layout_revision_for_scene(&ids.scene_id).unwrap();

    begin(&mut app).send(()).unwrap(); // 真按压先作废旧悬停，原次按住等待新Collider命中后起笔。
    until(&mut app, |app| app.world().resource::<VegetationBrushStroke>().active);
    assert_eq!(app.world().resource::<VegetationBrushStroke>().layer_id.as_deref(), Some(ids.tree_author.as_str()));
    let start = app.world().resource::<VegetationBrushStroke>().last_world.unwrap();
    move_pointer(&mut app, 0.35);
    step(&mut app);
    until(&mut app, |app| app.world().resource::<VegetationBrushStroke>().last_world
        .is_some_and(|point| point.x > start.x + 2.0));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<VegetationBrushStroke>().active);
    let painted = pixels(&app, &ids.tree_author);
    // 顶视从画布中心向正X拖动；检查中间多列，防止只画首尾圆而没有真实连续刷段。
    for x in 64..=68 { assert!(painted[64 * 129 + x] > 0, "树木拖刷的第{x}列不得漏写"); }
    let published = current_snapshot(&mut app, &ids);
    assert_snapshot(&app, &ids, &published, &painted);
    let painted_layout = app.world().resource::<DynamicBladeDraft>().layout_revision_for_scene(&ids.scene_id).unwrap();
    assert!(painted_layout > initial_layout,
        "Mesh密度变化必须完整publish以重建树木散布，不能误走Blade的density-only更新");
    assert_eq!(initial.sources[&ids.tree_output].as_ref(), empty.as_slice(), "旧完整快照应保持不可变");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1, "树木拖刷与所有分帧完成值只组成一笔历史");
    assert!(matches!(&session.undo[0], EditorCommand::VegetationChanged { layer_id, changes }
        if layer_id == &ids.tree_author && !changes.is_empty()));

    // 与刷草后返回游戏一致：关闭后保留完整Mesh快照，后续历史编译仍使用相同发布路径。
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    step(&mut app);
    assert_snapshot(&app, &ids, &published, &painted);
    install_tree_history(&mut app);
    app.world_mut().resource_mut::<SceneEditorState>().undo_requested = true;
    app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
    assert_eq!(pixels(&app, &ids.tree_author), empty);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().redo.len(), 1);
    let undone = current_snapshot(&mut app, &ids);
    assert_snapshot(&app, &ids, &undone, &empty);
    let undone_layout = app.world().resource::<DynamicBladeDraft>().layout_revision_for_scene(&ids.scene_id).unwrap();
    assert!(undone_layout > painted_layout, "撤销树木也应完整发布，移除之前的Mesh散布");

    app.world_mut().resource_mut::<SceneEditorState>().redo_requested = true;
    app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
    assert_eq!(pixels(&app, &ids.tree_author), painted);
    let redone = current_snapshot(&mut app, &ids);
    assert_snapshot(&app, &ids, &redone, &painted);
    assert!(app.world().resource::<DynamicBladeDraft>().layout_revision_for_scene(&ids.scene_id).unwrap() > undone_layout,
        "重做后Mesh散布必须再次收到完整发布");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(session.redo.is_empty());
}
