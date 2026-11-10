//! 植被侧栏真实中文布局、任务身份、原六行池与作者事务回归；不写场景、目录或设置。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use std::sync::Arc;

use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::prelude::*;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_ui::{NumericInputRoot, NumericInputSetValue};
use feathrise_scene_content::{
    SceneVegetationAsset, SceneVegetationAuthorGroup, SceneVegetationAuthorGroupMode,
    SceneVegetationAuthorLayer, SceneVegetationAuthoring, SceneVegetationOutputMergeMode,
    ResolvedVegetationPresetCatalog, VegetationPresetCatalogAsset, VegetationPresetRegistry,
};
use feathrise_scene::{DynamicBladeDraft, DynamicBladeDraftSnapshot};

use super::*;
use crate::SceneEditorSystemSet;
use crate::authoring::{
    EditorAuthoringSession, VegetationBrushStroke, VegetationCompileState, test_heightfield_draft,
};
use crate::components::{
    SceneEditorActionButton, SceneEditorRightPanelScroll, SceneEditorVegetationLayerRow,
};
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState};
use crate::types::EditorAction;
use crate::ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState};
use crate::ui::vegetation_layers::*;

fn registry() -> VegetationPresetRegistry {
    let mut catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    )))
    .unwrap();
    for preset in &mut catalog.presets {
        preset.resolution = [2, 2]; // 真实校验目录保留LOD、风、碰撞，只缩小测试画布。
        preset.label = format!("{}用于窄侧栏完整回显的长中文名称", preset.label);
    }
    VegetationPresetRegistry {
        catalog: Handle::default(),
        revision: 1,
        resolved: ResolvedVegetationPresetCatalog::build(&catalog).unwrap(),
        last_error: None,
    }
}

fn page(shell: bool) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
    ));
    app.init_resource::<UiSurface>()
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::ui::gltf_browser::EditorGltfBrowserState>()
        .init_resource::<VegetationBlankLayerAllocation>()
        .init_resource::<VegetationBrushStroke>()
        .init_resource::<VegetationCompileState>()
        .init_resource::<VegetationNumericInputState>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<DynamicBladeDraft>()
        .init_resource::<crate::viewport::modes::ViewportModeState>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .add_message::<NumericInputSetValue>()
        .insert_resource(UiScale(1.0));
    let registry = registry();
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.runtime_scene = "data_driven".into();
    let output = registry
        .resolve("hn01_ordinary_grass")
        .unwrap()
        .create_scene_layer(&draft.folder);
    let author = SceneVegetationAuthorLayer {
        id: "stable-author".into(),
        label: "当前场景图层的完整中文名称不等于新建所选预设".into(),
        group_id: "stable-group".into(),
        output_layer_id: output.id.clone(),
        density_source_path: format!("{}/vegetation/author/stable-author.r8", draft.folder),
        resolution: [2, 2],
        enabled: true,
        locked: false,
        opacity: 1.0,
        priority: -3,
        weight: 1.0,
    };
    draft
        .vegetation_author_densities
        .insert(author.id.clone(), Arc::new(vec![0; 4]));
    draft.vegetation = Some(SceneVegetationAsset {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        layers: vec![output],
        authoring: Some(SceneVegetationAuthoring {
            groups: vec![SceneVegetationAuthorGroup {
                id: "stable-group".into(),
                label: "基础分组的完整名称".into(),
                mode: SceneVegetationAuthorGroupMode::Additive,
                output_merge: SceneVegetationOutputMergeMode::Maximum,
                capacity: 1.0,
                claim_threshold: 0.0,
                selection_seed: 1,
                selection_cell_meters: 8.0,
                enabled: true,
            }],
            layers: vec![author],
        }),
    });
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(draft.scene_id.clone()),
        vegetation_selected_preset_id: Some("hn01_ordinary_grass".into()),
        ..default()
    });
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        vegetation_revision: 4,
        vegetation_compiled_revision: 4,
        ..default()
    });
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
    sidebar.left = EditorLeftSidebarTab::Vegetation;
    app.insert_resource(sidebar);
    app.insert_resource(registry);
    app.configure_sets(
        Update,
        (
            SceneEditorSystemSet::Input,
            SceneEditorSystemSet::Tasks,
            SceneEditorSystemSet::Presentation,
        )
            .chain(),
    );
    super::install(&mut app);
    crate::ui::vegetation_preset_editor::install_sidebar_presentation_for_tests(&mut app);
    app.add_systems(
        Update,
        (
            handle_vegetation_layer_row_clicks,
            handle_vegetation_authoring_buttons,
        )
            .chain()
            .in_set(SceneEditorSystemSet::Input),
    );
    app.add_systems(
        Update,
        poll_vegetation_blank_layer_allocation.in_set(SceneEditorSystemSet::Tasks),
    );
    app.add_systems(
        Update,
        (
            setup_vegetation_numeric_inputs,
            spawn_vegetation_layer_rows,
            crate::ui::vegetation_presets::sync_vegetation_preset_list,
            sync_vegetation_layer_rows,
            sync_vegetation_numeric_inputs,
            crate::ui::layer_status::sync_editor_layer_status,
            crate::ui::sidebars::sync_editor_sidebar_context,
        )
            .chain()
            .in_set(SceneEditorSystemSet::Presentation),
    );
    app.add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
        PostUpdate,
    ));
    app.add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
        PostUpdate,
    ));
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiTargetCamera>::default()
            .after(propagate_ui_target_cameras)
            .before(bevy::ui::widget::measure_text_system),
    );
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(bevy::ui::widget::measure_text_system),
    );
    app.add_systems(
        PostUpdate,
        (
            propagate_ui_target_cameras,
            bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
            ui_layout_system,
        )
            .chain(),
    );
    if shell {
        app.add_systems(Startup, crate::ui::sidebars::test_right_sidebar.spawn());
        app.add_systems(
            Update,
            crate::ui::sidebars::sync_editor_sidebar_presentation
                .in_set(SceneEditorSystemSet::Presentation),
        );
    } else {
        app.add_systems(
            Startup,
            crate::ui::vegetation_scene::vegetation_authoring_panel.spawn(),
        );
    }
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::splat(4096),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::splat(4096),
                ..default()
            }),
            ..default()
        },
    ));
    let face = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    if !shell { app.add_systems(Startup, crate::ui::vegetation_scene::vegetation_properties_panel.spawn()); }
    app.update();
    let world = app.world_mut();
    let root = if shell {
        let root = world
            .query_filtered::<Entity, With<crate::components::SceneEditorPane>>()
            .single(world)
            .unwrap();
        world.get_mut::<Node>(root).unwrap().width = percent(100);
        root
    } else {
        world
            .query_filtered::<Entity, With<VegetationSidebarRoot>>()
            .single(world)
            .unwrap()
    };
    let host = world
        .spawn(Node {
            width: px(240),
            height: px(350),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    let scroll = if shell {
        world.entity_mut(root).insert(ChildOf(host));
    // 单域布局夹具同时挂载迁出的属性；完整左右栏分区由检查器集成测试验证。
    let properties = world.query::<(Entity, &crate::ui::scene_inspector::InspectorSection)>().iter(world)
        .find(|(_, section)| **section == crate::ui::scene_inspector::InspectorSection::Vegetation).unwrap().0;
    world.entity_mut(properties).insert(ChildOf(host));
    world.get_mut::<Node>(properties).unwrap().display = Display::Flex;
        world
            .query::<(Entity, &crate::ui::sidebars::SceneEditorSidebarPage)>()
            .iter(world)
            .find(|(_, page)| {
                page.0
                    == crate::ui::sidebars::EditorSidebarTab::Left(EditorLeftSidebarTab::Vegetation)
            })
            .unwrap()
            .0
    } else {
        let scroll = world
            .spawn((
                SceneEditorRightPanelScroll,
                bevy::ui::RelativeCursorPosition::default(),
                ScrollPosition::default(),
                Node {
                    width: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ChildOf(host),
            ))
            .id();
        world.entity_mut(root).insert(ChildOf(scroll));
        scroll
    };
    for mut font in world.query::<&mut TextFont>().iter_mut(world) {
        font.font = face.clone().into();
    }
    for _ in 0..6 {
        app.update();
    }
    (app, host, scroll)
}

fn feedback(app: &mut App, kind: VegetationSidebarFeedbackKind) -> String {
    let world = app.world_mut();
    world
        .query::<(&VegetationSidebarFeedbackText, &Text)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == kind)
        .unwrap()
        .1
        .to_string()
}

fn press(app: &mut App, action: EditorAction) {
    let world = app.world_mut();
    let entity = world
        .query::<(Entity, &SceneEditorActionButton)>()
        .iter(world)
        .find(|(_, button)| button.0 == action)
        .unwrap()
        .0;
    world.entity_mut(entity).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    app.world_mut().entity_mut(entity).insert(UiInteraction::None);
    app.update();
}

/// 模拟已由画布加载器完整采用的显式Clipmap；不读取或补造作者文件。
fn use_explicit_empty_canvas(app: &mut App) -> feathrise_scene_content::TerrainId {
    let id = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
        let identity = crate::authoring::terrain_documents::TerrainDocumentIdentity {
            definition: Arc::new(object.clone()),
            manifest: object.source_manifest(&draft.manifest),
            folder: draft.folder.clone(),
        };
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
        draft.terrain_documents.active = Some(identity);
        draft.heights = None;
        draft.terrain_mesh = None;
        draft.terrain_editing_index = None;
        let vegetation = draft.vegetation.as_mut().unwrap();
        vegetation.layers.clear();
        let authoring = vegetation.authoring.as_mut().unwrap();
        authoring.groups[0].id = "base".into();
        authoring.layers.clear(); // 保留原基础组，作者明确保存的空列表不做兼容补回。
        authoring.validate(&vegetation.layers).unwrap();
        draft.vegetation_author_densities.clear();
        draft.vegetation_densities.clear();
        object.id
    };
    app.world_mut().resource_mut::<SceneEditorState>().terrain_canvas_target = Some(id.clone());
    id
}

fn choose_ordinary_grass(app: &mut App) {
    let preset = app.world().resource::<VegetationPresetRegistry>()
        .resolve("hn01_ordinary_grass").unwrap().clone();
    app.world_mut().resource_scope(|world, session: Mut<EditorAuthoringSession>| {
        crate::ui::actions::select_vegetation_preset(
            &mut world.resource_mut::<SceneEditorState>(), &session, &preset, None,
        );
    });
}

#[test]
fn vegetation_feedback_distinguishes_unselected_loading_and_loaded_empty_canvas() {
    let (mut app, _, _) = page(false);
    let id = use_explicit_empty_canvas(&mut app);
    let (identity, vegetation) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        (draft.terrain_documents.active.take().unwrap(), draft.vegetation.take().unwrap())
    };
    app.world_mut().resource_mut::<SceneEditorState>().terrain_canvas_target = None;
    choose_ordinary_grass(&mut app);
    app.update();
    for kind in [VegetationSidebarFeedbackKind::Availability, VegetationSidebarFeedbackKind::Compile,
        VegetationSidebarFeedbackKind::Preview] {
        assert!(feedback(&mut app, kind).contains("先在场景树选择一个网格"));
    }
    assert!(app.world().resource::<SceneEditorState>().status.contains("先在场景树选择一个网格"));
    press(&mut app, EditorAction::VegetationAddLayer);
    assert!(!app.world().resource::<VegetationBlankLayerAllocation>().busy());
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().vegetation.is_none());

    app.world_mut().resource_mut::<SceneEditorState>().terrain_canvas_target = Some(id);
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
        .terrain_documents.loading = true;
    choose_ordinary_grass(&mut app);
    app.update();
    assert!(app.world().resource::<SceneEditorState>().status.contains("正在载入网格作者画布"));
    for kind in [VegetationSidebarFeedbackKind::Availability, VegetationSidebarFeedbackKind::Compile,
        VegetationSidebarFeedbackKind::Preview] {
        assert!(feedback(&mut app, kind).contains("正在载入网格作者画布"));
    }
    press(&mut app, EditorAction::VegetationAddLayer);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().vegetation.is_none());
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(!app.world().resource::<VegetationBlankLayerAllocation>().busy());

    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_documents.loading = false;
        draft.terrain_documents.active = Some(identity);
    }
    // 只有活动网格身份还不代表文件已读完；None不能被首次建层入口默认初始化。
    press(&mut app, EditorAction::VegetationAddLayer);
    assert!(app.world().resource::<SceneEditorState>().status.contains("作者数据尚未就绪"));
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().vegetation.is_none());
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
        .vegetation = Some(vegetation);
    choose_ordinary_grass(&mut app);
    app.update();
    assert!(feedback(&mut app, VegetationSidebarFeedbackKind::Availability).contains("第一张空白画布"));
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.status.contains("点击“新空白层”"));
    assert!(!editor.status.contains("进入含植被作者文件的场景"));
    assert_eq!(editor.vegetation_selected_preset_id.as_deref(), Some("hn01_ordinary_grass"));
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn explicit_empty_vegetation_file_first_layer_uses_background_compile_and_full_undo_redo() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::authoring::*;
    let (mut app, _, _) = page(false);
    let terrain = use_explicit_empty_canvas(&mut app);
    let original = serde_json::to_string(app.world().resource::<EditorAuthoringSession>()
        .draft.as_ref().unwrap().vegetation.as_ref().unwrap()).unwrap();
    app.add_systems(Update, (poll_vegetation_compile, start_vegetation_compile).chain()
        .after(poll_vegetation_blank_layer_allocation).in_set(SceneEditorSystemSet::Tasks));
    choose_ordinary_grass(&mut app);
    press(&mut app, EditorAction::VegetationAddLayer);
    let wait_for_compile = |app: &mut App, expected_undo| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.update();
            let session = app.world().resource::<EditorAuthoringSession>();
            if session.undo.len() == expected_undo && session.vegetation_compiled_revision == session.vegetation_revision
                && !app.world().resource::<VegetationBlankLayerAllocation>().busy() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "首层后台分配、编译或历史未收敛：{}", app.world().resource::<SceneEditorState>().status);
            std::thread::yield_now();
        }
    };
    wait_for_compile(&mut app, 1);
    let (author_id, output_id, pixels, created) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.terrain_documents.active.as_ref().unwrap().definition.id, terrain);
        assert!(draft.heights.is_none(), "Clipmap首层创建不能借用整图高度缓存");
        let vegetation = draft.vegetation.as_ref().unwrap();
        let authoring = vegetation.authoring.as_ref().unwrap();
        assert_eq!(vegetation.layers.len(), 1);
        assert_eq!(authoring.layers.len(), 1);
        assert_eq!(authoring.groups.len(), 1);
        assert_eq!(authoring.groups[0].id, "base");
        assert_eq!(authoring.layers[0].group_id, "base");
        assert_eq!(vegetation.layers[0].runtime_profile.as_deref(), Some("hn01_ordinary_grass"));
        let author_id = authoring.layers[0].id.clone();
        let output_id = vegetation.layers[0].id.clone();
        let pixels = draft.vegetation_author_densities[&author_id].clone();
        assert_eq!(pixels.as_slice(), &[0; 4]);
        assert_eq!(draft.vegetation_densities[&output_id].as_slice(), &[0; 4]);
        assert!(draft.vegetation_author_dirty_layers.contains(&author_id));
        assert!(matches!(session.undo[0], EditorCommand::VegetationAuthoringChanged(_)));
        (author_id, output_id, pixels, serde_json::to_string(vegetation).unwrap())
    };
    // 使用生产统一历史入口；首次正式输出、作者描述与R8必须共同撤销和重做。
    app.insert_resource(EditorPreviewAssets {
        portal_base_mesh: default(), portal_arrow_head_mesh: default(), portal_arrow_shaft_mesh: default(),
        portal_edge_beam_mesh: default(), portal_edge_ring_mesh: default(), portal_base_material: default(),
        portal_arrow_material: default(), portal_edge_beam_material: default(), portal_edge_ring_material: default(),
        portal_label_font: default(),
    }).init_resource::<courtyards::CourtyardAuthoringState>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<WaterMaskMaterializeTask>()
        .init_resource::<SurfaceLayerEditTask>()
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<WaterFileHandoff>();
    app.world_mut().resource_mut::<SceneEditorState>().undo_requested = true;
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    wait_for_compile(&mut app, 0);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(serde_json::to_string(draft.vegetation.as_ref().unwrap()).unwrap(), original);
    assert!(draft.vegetation_author_densities.is_empty());
    assert!(!draft.vegetation_densities.contains_key(&output_id));
    assert_eq!(session.redo.len(), 1);
    app.world_mut().resource_mut::<SceneEditorState>().redo_requested = true;
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    wait_for_compile(&mut app, 1);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(serde_json::to_string(draft.vegetation.as_ref().unwrap()).unwrap(), created);
    assert!(Arc::ptr_eq(&pixels, &draft.vegetation_author_densities[&author_id]));
    assert_eq!(draft.vegetation_densities[&output_id].as_slice(), &[0; 4]);
    assert!(session.redo.is_empty());
}

#[test]
fn library_selection_and_scene_creation_target_remain_distinct_and_read_only() {
    let (mut app, _, _) = page(false);
    let before = serde_json::to_string(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .vegetation
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .vegetation_selected_preset_id = Some("hn01_rock".into());
    app.update();
    assert!(feedback(&mut app, VegetationSidebarFeedbackKind::CreationTarget).contains("岩石"));
    assert!(
        !feedback(&mut app, VegetationSidebarFeedbackKind::CreationTarget).contains("当前场景图层")
    );
    let world = app.world_mut();
    let texts = world
        .query::<&Text>()
        .iter(world)
        .map(|text| text.to_string())
        .collect::<Vec<_>>();
    for label in [
        "全局预设库",
        "当前场景图层",
        "所选层属性",
        "用此预设新建空白层",
        "删除图层",
        "删除所选预设",
    ] {
        assert!(
            texts.iter().any(|text| text == label),
            "缺少明确作用域：{label}"
        );
    }
    assert_eq!(
        world
            .query_filtered::<&Text, With<feathrise_editor_ui::PanelStatusText>>()
            .iter(world)
            .filter(|text| text.starts_with("使用已有预设："))
            .count(),
        0
    );
    let session = world.resource::<EditorAuthoringSession>();
    assert_eq!(
        before,
        serde_json::to_string(session.draft.as_ref().unwrap().vegetation.as_ref().unwrap())
            .unwrap()
    );
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert_eq!(session.vegetation_revision, 4);
}

#[test]
fn feedback_rejects_old_scene_and_revision_and_never_claims_gpu_readiness() {
    let (mut app, _, _) = page(false);
    let definition = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .vegetation
        .as_ref()
        .unwrap()
        .clone();
    let scene_id = definition.scene_id.clone();
    app.world_mut()
        .resource_mut::<DynamicBladeDraft>()
        .publish(DynamicBladeDraftSnapshot {
            source_revision: 3,
            definition: definition.clone(),
            sources: default(),
            densities: default(),
            nonzero_layers: default(),
        });
    app.update();
    assert!(
        feedback(&mut app, VegetationSidebarFeedbackKind::Preview).contains("等待当前完整密度")
    );
    app.world_mut()
        .resource_mut::<DynamicBladeDraft>()
        .publish(DynamicBladeDraftSnapshot {
            source_revision: 4,
            definition,
            sources: default(),
            densities: default(),
            nonzero_layers: default(),
        });
    app.update();
    assert!(
        feedback(&mut app, VegetationSidebarFeedbackKind::Preview).contains("当前完整密度已发布")
    );
    assert!(
        feedback(&mut app, VegetationSidebarFeedbackKind::Preview).contains("仍按原加载链就绪")
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .vegetation_revision = 5;
    app.world_mut()
        .resource_mut::<VegetationCompileState>()
        .failed_scene_id = Some(scene_id);
    app.world_mut()
        .resource_mut::<VegetationCompileState>()
        .failed_revision = Some(4);
    app.update();
    assert!(!feedback(&mut app, VegetationSidebarFeedbackKind::Compile).contains("失败"));
    app.world_mut()
        .resource_mut::<VegetationCompileState>()
        .failed_revision = Some(5);
    app.update();
    assert!(feedback(&mut app, VegetationSidebarFeedbackKind::Compile).contains("当前修订失败"));
    app.world_mut()
        .resource_mut::<VegetationPresetRegistry>()
        .last_error = Some("目录验证失败的真实诊断".into());
    app.update();
    assert!(
        feedback(&mut app, VegetationSidebarFeedbackKind::Catalog)
            .contains("目录验证失败的真实诊断")
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = None;
    app.update();
    assert!(
        feedback(&mut app, VegetationSidebarFeedbackKind::Availability).contains("等待当前场景")
    );
    assert!(!feedback(&mut app, VegetationSidebarFeedbackKind::Compile).contains("当前修订失败"));
    assert!(feedback(&mut app, VegetationSidebarFeedbackKind::Preview).contains("等待当前场景"));
}

#[test]
fn existing_six_row_pool_copy_on_write_and_delete_transactions_are_preserved() {
    let (mut app, _, _) = page(false);
    let original = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .vegetation_author_densities["stable-author"]
        .clone();
    press(&mut app, EditorAction::VegetationDuplicateLayer);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let authoring = draft
        .vegetation
        .as_ref()
        .unwrap()
        .authoring
        .as_ref()
        .unwrap();
    assert_eq!(authoring.layers.len(), 2);
    assert!(Arc::ptr_eq(
        &original,
        &draft.vegetation_author_densities[&authoring.layers[1].id]
    ));
    assert_eq!(session.undo.len(), 1);
    press(&mut app, EditorAction::VegetationDeleteSelected);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .vegetation
            .as_ref()
            .unwrap()
            .authoring
            .as_ref()
            .unwrap()
            .layers
            .len(),
        1
    );
    assert_eq!(session.undo.len(), 2);
    press(&mut app, EditorAction::VegetationAddLayer);
    // 等待真实后台分配收口；固定跑100帧会在并行回归争用线程池时过早检查。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.world().resource::<VegetationBlankLayerAllocation>().busy() {
        app.update();
        assert!(std::time::Instant::now() < deadline,
            "空白植被层后台分配未收敛：{}", app.world().resource::<SceneEditorState>().status);
        std::thread::yield_now();
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .vegetation
            .as_ref()
            .unwrap()
            .authoring
            .as_ref()
            .unwrap()
            .layers
            .len(),
        2
    );
    assert_eq!(session.undo.len(), 3);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorVegetationLayerRow>>()
            .iter(world)
            .count(),
        6
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<NumericInputRoot>>()
            .iter(world)
            .count(),
        5
    );
}

#[test]
fn real_cjk_layout_fits_narrow_sidebar_scales_dpi_and_long_values() {
    let (mut app, host, scroll) = page(false);
    let world = app.world_mut();
    let numeric_entities = world
        .query_filtered::<Entity, With<NumericInputRoot>>()
        .iter(world)
        .collect::<Vec<_>>();
    for mut text in world
        .query_filtered::<&mut Text, With<feathrise_editor_ui::NumericInputDisplayText>>()
        .iter_mut(world)
    {
        text.0 = "-2147483648".into();
    }
    for width in [200.0, 240.0, 320.0, 420.0] {
        for scale in [0.6, 1.0, 1.5] {
            for dpi in [1.0, 2.0] {
                app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                for mut camera in app
                    .world_mut()
                    .query::<&mut Camera>()
                    .iter_mut(app.world_mut())
                {
                    camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                }
                app.update();
                let world = app.world_mut();
                let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
                let center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                for (node, transform) in world
                    .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<InteractionButton>>()
                    .iter(world)
                {
                    if node.size().x == 0.0 {
                        continue;
                    }
                    assert!(
                        transform.translation.x - node.size().x * 0.5
                            >= center - host_width * 0.5 - 1.1,
                        "按钮左溢出{width}/{scale}/{dpi}"
                    );
                    assert!(
                        transform.translation.x + node.size().x * 0.5
                            <= center + host_width * 0.5 + 1.1,
                        "按钮右溢出{width}/{scale}/{dpi}"
                    );
                }
                for (node, transform, children) in world.query_filtered::<(&ComputedNode, &UiGlobalTransform, &Children), With<NumericInputRoot>>().iter(world) {
                    for child in children.iter() {
                        if world.get::<Text>(child).is_none() { continue; }
                        let child_node = world.get::<ComputedNode>(child).unwrap();
                        let child_center = world.get::<UiGlobalTransform>(child).unwrap().translation.x;
                        assert!(child_center - child_node.size().x * 0.5 >= transform.translation.x - node.size().x * 0.5 - 1.1, "数值左溢出{width}/{scale}/{dpi}");
                        assert!(child_center + child_node.size().x * 0.5 <= transform.translation.x + node.size().x * 0.5 + 1.1, "数值右溢出{width}/{scale}/{dpi}");
                    }
                }
                assert!(
                    world.get::<ComputedNode>(scroll).unwrap().content_size().y
                        > world.get::<ComputedNode>(scroll).unwrap().size().y
                );
            }
        }
    }
    let world = app.world_mut();
    assert_eq!(
        numeric_entities,
        world
            .query_filtered::<Entity, With<NumericInputRoot>>()
            .iter(world)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
            .iter(world)
            .count(),
        1
    );
}

#[test]
fn stable_feedback_does_not_rewrite_text_or_color_and_inactive_page_stays_untouched() {
    #[derive(Resource, Default)]
    struct Writes(usize);
    fn probe(
        mut writes: ResMut<Writes>,
        texts: Query<
            (),
            (
                With<VegetationSidebarFeedbackText>,
                Or<(Changed<Text>, Changed<TextColor>)>,
            ),
        >,
    ) {
        writes.0 = texts.iter().count();
    }
    let (mut app, _, _) = page(false);
    app.init_resource::<Writes>().add_systems(
        Update,
        probe
            .after(super::systems::sync_feedback)
            .in_set(SceneEditorSystemSet::Presentation),
    );
    app.update();
    for _ in 0..5 {
        app.update();
        assert_eq!(app.world().resource::<Writes>().0, 0);
    }
    let before = feedback(&mut app, VegetationSidebarFeedbackKind::Message);
    app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Water;
    app.world_mut().resource_mut::<SceneEditorState>().status = "其他模块的新操作".into();
    app.update();
    assert_eq!(
        feedback(&mut app, VegetationSidebarFeedbackKind::Message),
        before
    );
    app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Vegetation;
    app.update();
    assert_eq!(
        feedback(&mut app, VegetationSidebarFeedbackKind::Message),
        "其他模块的新操作"
    );
}

#[test]
fn full_low_sidebar_scrolls_long_feedback_and_empty_scene_retains_creation_and_help() {
    let (mut app, host, scroll) = page(true);
    app.world_mut().get_mut::<Node>(host).unwrap().width = px(200);
    app.world_mut().resource_mut::<SceneEditorState>().status =
        "操作诊断保留完整中文，不扩大侧栏，也不挤压场景视口。".repeat(12);
    app.update();
    let world = app.world_mut();
    assert!(
        world.get::<ComputedNode>(scroll).unwrap().content_size().y
            > world.get::<ComputedNode>(scroll).unwrap().size().y
    );
    let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
    let center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
    for (node, transform) in world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<NumericInputRoot>>()
        .iter(world)
    {
        assert!(node.size().x > 0.0);
        assert!(transform.translation.x + node.size().x * 0.5 <= center + host_width * 0.5 + 1.1);
    }
    let inputs = world
        .query_filtered::<Entity, With<NumericInputRoot>>()
        .iter(world)
        .collect::<Vec<_>>();
    world
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .vegetation
        .as_mut()
        .unwrap()
        .authoring
        .as_mut()
        .unwrap()
        .layers
        .clear();
    world
        .resource_mut::<SceneEditorState>()
        .vegetation_selected_preset_id = None;
    app.update();
    assert!(
        feedback(&mut app, VegetationSidebarFeedbackKind::CreationTarget)
            .contains("先在上方全局库选择")
    );
    let world = app.world_mut();
    let properties = world
        .query_filtered::<&Node, With<crate::ui::sidebars::SceneEditorVegetationSelectionProperties>>(
        )
        .single(world)
        .unwrap();
    assert_eq!(properties.display, Display::None);
    assert_eq!(
        inputs,
        world
            .query_filtered::<Entity, With<NumericInputRoot>>()
            .iter(world)
            .collect::<Vec<_>>()
    );
    let labels = world
        .query::<&Text>()
        .iter(world)
        .map(|text| text.to_string())
        .collect::<Vec<_>>();
    assert!(labels.iter().any(|text| text == "用此预设新建空白层"));
    assert!(
        labels
            .iter()
            .any(|text| text.starts_with("绘制：在全局库选预设"))
    );
}
