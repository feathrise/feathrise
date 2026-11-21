//! 无活动画布时仍从真实定义投影植被树；树选或图标命令才会加载盘上的独立作者像素。

use super::{SceneLayerSelection, components, systems, tests::load_author_layers_fixture, types::LayerList};
use crate::{
    SceneEditorSystemSet,
    authoring::{EditorAuthoringSession, VegetationBrushStroke, terrain_documents::{self, TerrainCanvas, TerrainDocumentIdentity, TerrainDocumentTask}},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{scene_context_menu::{SceneContextMenuState, SceneObjectKey, SceneObjectRow, select_target_for_tests},
        scene_sidebar::TreeRowToggle, terrain_objects::TerrainObjectsEditorState, vegetation_layers::handle_scene_menu},
    viewport::{LayerCandidateState, LayerTarget},
};
use bevy::{prelude::*, window::PrimaryWindow};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};
use feathrise_scene_content::{SCENE_VEGETATION_FILE_NAME, SceneContentPlugin, TerrainId};
use std::{path::{Path, PathBuf}, sync::Arc, time::{Duration, Instant}};

#[test]
fn scene_layers_lists_cold_private_vegetation_then_loads_its_author_canvas() {
    const TEST: &str = "FEATHRISE_COLD_VEGETATION_FIXTURE";
    // 与真实网格创建的隔离测试同理：后台读盘的工作区根是进程级，不能改写并行测试的根。
    if std::env::var_os(TEST).is_none() {
        let test_name = format!("{}::scene_layers_lists_cold_private_vegetation_then_loads_its_author_canvas",
            module_path!().split_once("::").unwrap().1);
        let run = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_name, "--nocapture"]).env(TEST, "1").output().unwrap();
        assert!(run.status.success() && String::from_utf8_lossy(&run.stdout).contains("1 passed"), "{}\n{}",
            String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
        return;
    }
    struct FixtureRoot(PathBuf);
    impl Drop for FixtureRoot {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }
    let root = FixtureRoot(std::env::temp_dir().join(format!("feathrise-cold-vegetation-{}", std::process::id())));
    std::fs::create_dir(&root.0).unwrap();
    feathrise_editor_core::set_runtime_test_asset_root(Some(root.0.clone()));
    for action in [FirstAction::Select, FirstAction::Lock, FirstAction::Visibility, FirstAction::StaleVisibility] {
        run_case(&root.0, action);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FirstAction { Select, Lock, Visibility, StaleVisibility }

fn run_case(root: &Path, action: FirstAction) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin { file_path: root.display().to_string(), ..default() }, SceneContentPlugin))
        .init_asset::<Image>()
        .insert_resource(SceneEditorState { open: true, ..default() })
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<TerrainObjectsEditorState>()
        .init_resource::<LayerCandidateState>()
        .init_resource::<SceneContextMenuState>()
        .init_resource::<SceneLayerSelection>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<VegetationBrushStroke>()
        .configure_sets(Update, (SceneEditorSystemSet::Input, SceneEditorSystemSet::Tasks, SceneEditorSystemSet::Presentation).chain())
        .add_systems(Update, (systems::input, select_target_for_tests, handle_scene_menu).chain().in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, (systems::sync, systems::sync_vegetation_toggles).chain().in_set(SceneEditorSystemSet::Presentation));
    terrain_documents::install(&mut app);
    load_author_layers_fixture(&mut app);
    app.world_mut().spawn((Window { focused: true, ..default() }, PrimaryWindow));
    let (id, json, json_before) = seed_private_source(app.world_mut(), root, action);
    let author_id = "fixture-author-0";
    let use_eye = matches!(action, FirstAction::Visibility | FirstAction::StaleVisibility);
    if use_eye { seed_partial_edit(app.world_mut(), &id, author_id); }
    let row = app.world_mut().spawn((components::Row { list: LayerList::Vegetation, slot: 0 },
        Node::default(), SceneObjectRow(None), UiInteraction::None, BackgroundColor::default(), BorderColor::default())).id();
    let [participation, visibility, lock] = TreeRowToggle::ALL.map(|toggle| {
        let button = app.world_mut().spawn((ChildOf(row), components::VegetationRowToggle(toggle), UiInteraction::None,
            Node::default(), BackgroundColor::default(), BorderColor::default())).id();
        app.world_mut().spawn((ChildOf(button), ImageNode::default()));
        button
    });
    settle(&mut app, |world| world.get::<SceneObjectRow>(row).unwrap().0.is_some());
    let session = app.world().resource::<EditorAuthoringSession>();
    let initial = session.draft.as_ref().unwrap();
    assert!(initial.terrain_documents.active.is_none() && initial.vegetation.is_none());
    let target = app.world().get::<SceneObjectRow>(row).unwrap().0.as_ref().unwrap();
    assert!(matches!(&target.key, SceneObjectKey::Vegetation { id: layer, terrain: Some(owner), cold: Some(_), .. }
        if owner == &id && layer == "fixture-author-0"));
    for button in [participation, visibility, lock] {
        assert_eq!(app.world().get::<Node>(button).unwrap().display, Display::Flex);
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(button).is_none());
    }

    let clicked = match action {
        FirstAction::Select => row, FirstAction::Lock => lock, _ => visibility,
    };
    let session_revision = session.revision;
    let scene = initial.scene_id.clone();
    app.world_mut().entity_mut(clicked).insert(EditorButtonClick::triggered());
    if action == FirstAction::StaleVisibility {
        app.update();
        let world = app.world_mut();
        let work = world.resource_mut::<TerrainDocumentTask>().pause_pending_for_tests();
        {
            let mut session = world.resource_mut::<EditorAuthoringSession>();
            let parked = session.draft.as_mut().unwrap().terrain_documents.values.get_mut(&id).unwrap();
            // 水交接失败可这样恢复原停放槽的脏责任，且不推进正式作者修订。
            Arc::make_mut(parked).water_dirty_layers.insert("recovered-water".into());
        }
        world.resource_mut::<TerrainDocumentTask>().resume_pending_for_tests(work);
    }
    settle(&mut app, |world| {
        let session = world.resource::<EditorAuthoringSession>();
        match action {
            FirstAction::Select => world.resource::<LayerCandidateState>().selected == Some(LayerTarget::Vegetation(author_id.into())),
            FirstAction::Lock => session.undo.len() == 1,
            FirstAction::Visibility => world.resource::<SceneEditorState>().vegetation_visibility.hidden_in(&scene, Some(&id))
                .is_some_and(|layers| layers.contains(author_id)),
            FirstAction::StaleVisibility => !session.draft.as_ref().unwrap().terrain_documents.loading,
        }
    });
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    if use_eye {
        assert!(draft.terrain_documents.active.is_none() && draft.vegetation.is_none());
        assert!(app.world().resource::<LayerCandidateState>().selected.is_none());
        assert_eq!(session.revision, session_revision);
        let cached = &draft.terrain_documents.values[&id];
        assert_eq!(cached.vegetation_author_densities[author_id].as_slice(), &[231; 4]);
        assert!(draft.terrain_documents.dirty_stored.contains(&id));
        assert!(cached.vegetation_author_dirty_layers.contains(author_id));
        let stale = action == FirstAction::StaleVisibility;
        assert_eq!(cached.water_dirty_layers.contains("recovered-water"), stale);
        assert_eq!(cached.vegetation_only, stale);
        assert_eq!(app.world().resource::<SceneEditorState>().vegetation_visibility.hidden_in(&scene, Some(&id)).is_some(), !stale);
    } else {
        assert_eq!(draft.terrain_documents.active.as_ref().map(|canvas| &canvas.definition.id), Some(&id));
        assert_eq!(draft.vegetation_author_densities[author_id].as_slice(), &[173; 4]);
    }
    let use_lock = action == FirstAction::Lock;
    let definition = if use_eye { draft.terrain_documents.values[&id].vegetation.as_ref() } else { draft.vegetation.as_ref() };
    assert_eq!(definition.unwrap().authoring.as_ref().unwrap().layers[0].locked, use_lock);
    assert_eq!(session.undo.len(), usize::from(use_lock));
    assert_eq!(app.world().resource::<SceneEditorState>().dirty, use_lock || use_eye);
    assert_eq!(std::fs::read(root.join(json)).unwrap(), json_before); // 选择与会话编辑都不改写保存文件。
}

fn seed_private_source(world: &mut World, root: &Path, action: FirstAction) -> (TerrainId, String, Vec<u8>) {
    let mut session = world.resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    let mut source = draft.manifest.terrains.as_ref().unwrap()[0].clone();
    source.id = TerrainId::parse(match action {
        FirstAction::Select => "cold-selection", FirstAction::Lock => "cold-lock",
        FirstAction::Visibility => "cold-visibility", FirstAction::StaleVisibility => "cold-stale-visibility",
    }).unwrap();
    let folder = source.private_folder(&draft.scene_id);
    let path = format!("{folder}/{SCENE_VEGETATION_FILE_NAME}");
    source.vegetation_path = Some(path.clone());
    source.surface_path = None;
    source.water_path = None;
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([source.clone()]));
    let mut definition = draft.vegetation.take().unwrap();
    TerrainCanvas::default().swap(draft);
    std::fs::create_dir_all(root.join(&folder).join("vegetation")).unwrap();
    for layer in &mut definition.layers {
        layer.density_source_path = format!("{folder}/vegetation/{}.r8", layer.id);
        layer.density_texture_path = format!("{folder}/vegetation/{}.ktx2", layer.id);
        std::fs::write(root.join(&layer.density_source_path), [63; 4]).unwrap();
    }
    for layer in &mut definition.authoring.as_mut().unwrap().layers {
        layer.density_source_path = format!("{folder}/vegetation/{}.r8", layer.id);
        std::fs::write(root.join(&layer.density_source_path), [173; 4]).unwrap();
    }
    let bytes = serde_json::to_vec(&definition).unwrap();
    std::fs::write(root.join(&path), &bytes).unwrap();
    (source.id, path, bytes)
}

fn seed_partial_edit(world: &mut World, id: &TerrainId, author_id: &str) {
    let mut session = world.resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    let source = &draft.manifest.terrains.as_ref().unwrap()[0];
    let mut canvas = terrain_documents::load_vegetation_canvas(TerrainDocumentIdentity {
        definition: Arc::new(source.clone()), manifest: source.source_manifest(&draft.manifest),
        folder: source.private_folder(&draft.scene_id),
    }, None, false).unwrap();
    canvas.vegetation_author_densities.insert(author_id.into(), Arc::new(vec![231; 4]));
    canvas.vegetation_author_dirty_layers.insert(author_id.into());
    draft.terrain_documents.values.insert(id.clone(), Arc::new(canvas));
    draft.terrain_documents.dirty_stored.insert(id.clone());
    world.resource_mut::<SceneEditorState>().dirty = true;
}

fn settle(app: &mut App, ready: impl Fn(&World) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready(app.world()) {
        app.update();
        assert!(Instant::now() < deadline, "{}", app.world().resource::<SceneEditorState>().status);
        std::thread::sleep(Duration::from_millis(1));
    }
}
