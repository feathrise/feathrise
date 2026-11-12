//! 人形绑骨真窗口验收。只在显式 validation 及本轮自有工程启用，走原按钮、后台和发布流程。

use crate::{
    asset_catalog::{AssetTypeFilter, EditorAssetCatalog, request_asset_reveal},
    components::{SceneEditorAssetRow, SceneEditorOrientationAxisEndpoint},
    resources::SceneEditorState,
    ui::{
        asset_preview::{
            OpenAssetPreview,
            AssetPreviewState,
            AssetPreviewWorkspace,
            DocumentState,
            animation::{
                properties_panel_visible, rig_animation_diagnostic, rig_animation_identity,
                rig_animation_probe, timeline_dock_probe,
                workflow_validation::{animation_probe, timeline_ruler},
            },
            components::AssetPreviewWorkspaceAction,
            gltf_tree::{self, EditorSceneTreePanel},
            gltf_tree_validation,
            rigging::{RigAction, RigLandmarkDot, RiggingState},
            sidebar::validation_details_text,
            types::{AssetPreviewCanvas, AssetPreviewModelViewport},
            view_toolbar,
        },
        project_browser::{ProjectAnimationClip, ProjectAnimationExpand},
        sidebars::{
            EditorNavigatorTab, EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarState,
            SceneEditorSidebarTabButton,
        },
        vegetation_preset_editor::VegetationPresetEditorState,
    },
    viewport::orientation_axis::PreviewOrientationAxis,
};
use bevy::{
    app::AppExit,
    ecs::query::QueryFilter,
    input::{ButtonState, mouse::MouseButtonInput},
    math::DVec2,
    pbr::AtmosphereSettings,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::{InteractionDisabled, UiGlobalTransform},
    window::{CursorMoved, PrimaryWindow, WindowEvent},
};
use feathrise_scene::author_view::{GridView, ShadingView};
use feathrise_editor_ui::UiInteraction;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

const SOURCE: &str = "人形.glb";
const OUTPUT: &str = "人形_已绑骨.glb";
const IDLE: &str = "01-待机.glb";
const WALK: &str = "02-行走.glb";
const FIXTURE_ENV: &str = "FEATHRISE_RIG_VALIDATION_DIR";
const OWNER: &[u8] = b"feathrise-rigging-validation-v1";
const LEFT_WRIST: usize = 5;
const RIGHT_WRIST: usize = 12;

#[derive(Clone, Copy, Debug)]
enum Phase {
    Open,
    Selected,
    NavigateScene,
    GltfTreeImage,
    ToggleGrid,
    RestoreGrid,
    SelectWire,
    RestoreShading,
    OpenShadingMenu,
    CloseShadingMenu,
    ReturnToMapTree,
    MapTreeRestored,
    Launch,
    Inspect,
    SwitchBackend,
    InspectBlender,
    RejectReference,
    ChooseReference,
    ReferenceSelected,
    Attributes,
    RestoreRigging,
    FrontAxis,
    FrontView,
    AimMarker,
    PressMarker,
    DragMarker,
    ReleaseMarker,
    VerifyMarker,
    MarkersImage,
    ToggleMirror,
    IndependentMarkers,
    Bind,
    Published,
    BrowseOtherFile,
    VerifyOtherFile,
    MotionSceneTree,
    SelectModelRoot,
    OpenMotionPicker,
    PickIdle,
    Preview,
    Motion,
    PlayingBrowseOtherFile,
    PlayingSelectOtherFile,
    PlayingInspectOtherFile,
    PlayingRestoreModel,
    PlayingModelRestored,
    OpenSecondPicker,
    SwitchMotion,
    AimDivider,
    PressDivider,
    DragDivider,
    ReleaseDivider,
    VerifyDivider,
    ExpandedTimelineImage,
    MotionImage,
    SeekPlayingRuler,
    VerifyPlayingRuler,
    StandaloneProject,
    StandaloneSelect,
    StandaloneOpen,
    StandaloneTimeline,
    Verify,
}

#[derive(Clone, Copy)]
struct DragWitness {
    cursor: DVec2,
    destination: DVec2,
    left_before: Vec3,
    right_before: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TabWitness {
    active: u64,
    next: u64,
    count: usize,
}

struct DividerWitness {
    entity: Entity,
    destination: DVec2,
    before: Vec2,
    at_release: Option<Vec2>,
    enlarged: Option<Vec2>,
    after: Option<Vec2>,
}

#[derive(Resource)]
struct Run {
    root: PathBuf,
    assets: PathBuf,
    reopen: bool,
    backend: &'static str,
    started: Instant,
    phase_started: Instant,
    phase: Phase,
    source_hashes: [String; 3],
    previous_output: Option<String>,
    click: Option<(Entity, bool)>,
    drag: Option<DragWitness>,
    dragged_marker: bool,
    mirrored_marker: bool,
    independent_marker: bool,
    front_view: bool,
    restored_rigging_tab: bool,
    reopened_motion_picker: bool,
    property_switch_verified: bool,
    gltf_tree_verified: bool,
    capture: Option<Mutex<mpsc::Receiver<Result<bool, String>>>>,
    sample: Option<(f32, Vec<(String, Quat)>)>,
    tabs: Option<TabWitness>,
    second_motion: bool,
    first_motion: Option<Value>,
    playing_other_file_verified: bool,
    divider: Option<DividerWitness>,
    ruler_seek: Option<(f32, Option<Instant>)>,
    standalone_open_requested: bool,
    motion: Option<Value>,
    verify: Option<Task<Result<Value, String>>>,
    launch_snapshot: bool,
}

pub fn install(app: &mut App, project: &Path, assets: &Path) -> Result<(), String> {
    let Some(root) = std::env::var_os(FIXTURE_ENV).map(PathBuf::from) else {
        return Ok(());
    };
    if !root.is_absolute() {
        return Err("绑骨验收必须使用本轮自有绝对目录".into());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let project = project.canonicalize().map_err(|error| error.to_string())?;
    let assets = assets.canonicalize().map_err(|error| error.to_string())?;
    let isolated_preferences = std::env::var_os("FEATHRISE_PREFERENCES_FILE").map(PathBuf::from);
    let isolated_plugins = std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS")
        .and_then(|path| PathBuf::from(path).canonicalize().ok());
    if project != root.join("project")
        || assets != project.join("assets")
        || fs::read(root.join(".rigging-owner")).ok().as_deref() != Some(OWNER)
        || !isolated_preferences.is_some_and(|path| {
            path.file_name()
                .is_some_and(|name| name == "preferences.json")
                && path
                    .parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .as_ref()
                    == Some(&root)
        })
        || isolated_plugins.as_ref() != Some(&root.join("plugins"))
        || fs::read(root.join("plugins/.feathrise-plugin-fixture-owner"))
            .ok()
            .as_deref()
            != Some(b"feathrise-single-binary-validation-v1")
    {
        return Err("绑骨验收的工程、作者偏好或全局插件缺少本轮隔离归属".into());
    }
    if std::env::var_os("FEATHRISE_RIG_VALIDATION_WEIGHTS").is_some() {
        return super::weights::install_validation(app, root, assets);
    }
    let reopen = std::env::var_os("FEATHRISE_RIG_VALIDATION_REOPEN").is_some();
    let report = if reopen {
        "reopen.json"
    } else {
        "exercise.json"
    };
    if root.join(report).exists() {
        return Err("绑骨验收报告已存在，请使用新目录".into());
    }
    let (previous_output, backend) = if reopen {
        let bytes = fs::read(root.join("exercise.json")).map_err(|error| error.to_string())?;
        let exercise: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let expected = exercise["model_sha256"]
            .as_str()
            .ok_or("缺少上一轮已生成模型摘要")?;
        if exercise["passed"] != true || hash(&assets, OUTPUT)? != expected {
            return Err("重开前落盘模型与上次验收不同".into());
        }
        let backend = match exercise["binding_backend"].as_str() {
            Some("native") => "native",
            Some("blender") | None => "blender",
            _ => return Err("上次绑骨验收报告的后端无效".into()),
        };
        (Some(expected.to_owned()), backend)
    } else {
        if fs::symlink_metadata(assets.join(OUTPUT)).is_ok() {
            return Err("绑骨夹具已经存在结果，请使用新工程".into());
        }
        let backend = match std::env::var("FEATHRISE_RIG_VALIDATION_BACKEND").as_deref() {
            Ok("blender") => "blender",
            Ok("native") | Err(std::env::VarError::NotPresent) => "native",
            _ => return Err("绑骨验收后端须为 native 或 blender".into()),
        };
        (None, backend)
    };
    app.insert_resource(Run {
        source_hashes: hashes(&assets)?,
        root,
        assets,
        reopen,
        backend,
        previous_output,
        started: Instant::now(),
        phase_started: Instant::now(),
        phase: Phase::Open,
        click: None,
        capture: None,
        drag: None,
        dragged_marker: false,
        mirrored_marker: false,
        independent_marker: false,
        front_view: false,
        restored_rigging_tab: false,
        reopened_motion_picker: false,
        property_switch_verified: false,
        gltf_tree_verified: false,
        sample: None,
        tabs: None,
        second_motion: false,
        first_motion: None,
        playing_other_file_verified: false,
        divider: None,
        ruler_seek: None,
        standalone_open_requested: false,
        motion: None,
        verify: None,
        launch_snapshot: false,
    })
    .add_systems(PreUpdate, hold_drag_cursor.before(bevy::ui::UiSystems::Focus))
    .add_systems(Update, fixture_focus.before(crate::SceneEditorSystemSet::Input))
    .add_systems(Last, drive);
    Ok(())
}

fn hold_drag_cursor(world: &mut World) {
    let destination = world.get_resource::<Run>().and_then(|run| match run.phase {
        Phase::ReleaseMarker | Phase::VerifyMarker => run.drag.map(|drag| drag.destination),
        Phase::ReleaseDivider | Phase::VerifyDivider => run.divider.as_ref().map(|drag| drag.destination),
        _ => None,
    });
    let Some(destination) = destination else { return; };
    let Some(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next() else {
        return;
    };
    // 真窗口可能在帧间重报系统光标。焦点计算前固定拖动坐标，释放帧也保留同一目标。
    move_cursor(world, window, destination);
}

pub(super) fn fixture_focus(mut window: Query<&mut Window, With<PrimaryWindow>>) {
    // 独占夹具的内部鼠标输入使用原焦点门。验证不抢占操作系统的实际前台。
    if let Ok(mut window) = window.single_mut() && !window.focused {
        window.focused = true;
    }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        let short_phase = matches!(
            run.phase,
            Phase::NavigateScene | Phase::GltfTreeImage | Phase::ToggleGrid | Phase::RestoreGrid
                | Phase::SelectWire | Phase::RestoreShading | Phase::OpenShadingMenu | Phase::CloseShadingMenu
                | Phase::ReturnToMapTree | Phase::MapTreeRestored
                | Phase::Launch | Phase::Inspect | Phase::SwitchBackend | Phase::InspectBlender
                | Phase::RejectReference | Phase::ChooseReference | Phase::ReferenceSelected
                | Phase::Attributes | Phase::RestoreRigging | Phase::FrontAxis | Phase::FrontView
                | Phase::PressMarker | Phase::DragMarker | Phase::ReleaseMarker | Phase::VerifyMarker
                | Phase::ToggleMirror | Phase::IndependentMarkers
                | Phase::MarkersImage
                | Phase::BrowseOtherFile | Phase::VerifyOtherFile | Phase::MotionSceneTree | Phase::SelectModelRoot
                | Phase::OpenMotionPicker | Phase::PickIdle | Phase::Preview | Phase::Motion
                | Phase::PlayingBrowseOtherFile | Phase::PlayingSelectOtherFile
                | Phase::PlayingInspectOtherFile | Phase::PlayingRestoreModel | Phase::PlayingModelRestored
                | Phase::OpenSecondPicker | Phase::SwitchMotion
                | Phase::AimDivider | Phase::PressDivider | Phase::DragDivider
                | Phase::ReleaseDivider | Phase::VerifyDivider | Phase::ExpandedTimelineImage | Phase::MotionImage
                | Phase::SeekPlayingRuler | Phase::VerifyPlayingRuler | Phase::StandaloneProject
                | Phase::StandaloneSelect | Phase::StandaloneOpen | Phase::StandaloneTimeline
        );
        let result = if run.started.elapsed() > Duration::from_secs(250)
            || short_phase && run.phase_started.elapsed() > Duration::from_secs(40)
        {
            Err(format!(
                "等待 {:?} 阶段超过限定时间。{}",
                run.phase, rig_animation_diagnostic(world).unwrap_or_default()
            ))
        } else {
            advance(world, &mut run)
        };
        match result {
            Ok(Some(proof)) => {
                let name = if run.reopen {
                    "reopen.json"
                } else {
                    "exercise.json"
                };
                match report(&run.root.join(name), &proof) {
                    Ok(()) => {
                        world.write_message(AppExit::Success);
                    }
                    Err(error) => {
                        error!("写绑骨验收报告失败：{error}");
                        world.write_message(AppExit::error());
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                let name = if run.reopen {
                    "reopen-failure.json"
                } else {
                    "exercise-failure.json"
                };
                let detail = diagnostics(world, &run);
                let _ = report(&run.root.join(name), &json!({
                    "phase": format!("{:?}", run.phase), "error": error, "detail": detail,
                }));
                error!("绑骨验收 {:?}：{error}", run.phase);
                world.write_message(AppExit::error());
            }
        }
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<Option<Value>, String> {
    let Some(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
    else {
        return Ok(None);
    };
    let model = if run.reopen { OUTPUT } else { SOURCE };
    let action = if run.second_motion { WALK } else { IDLE };
    if !run.standalone_open_requested
        && let Some(before) = run.tabs && active_model_tab(world, model) != Some(before)
    {
        return Err("预览动作期间，活动模型标签、标签数量或分配序号发生变化".into());
    }
    if let Some(state) = world.get_resource::<RiggingState>() {
        if let Some(error) = state.validation_status().error {
            return Err(error.to_owned());
        }
    }
    let phase = run.phase;
    let next = match phase {
        Phase::Open => {
            let Some(editor) = world.get_resource::<SceneEditorState>() else {
                return Ok(None);
            };
            if !editor.open || editor.pending_scene_transition.is_some() {
                return Ok(None);
            }
            // 用正式引用定位事务选择源文件，等待资产目录采用后再走原双击打开命令。
            world.resource_scope(|world, mut catalog: Mut<EditorAssetCatalog>| {
                world.resource_scope(|world, mut editor: Mut<SceneEditorState>| {
                    request_asset_reveal(
                        &mut catalog,
                        &mut editor,
                        &mut world.resource_mut::<SceneEditorSidebarState>(),
                        model,
                    )
                })
            })?;
            Phase::Selected
        }
        Phase::Selected
            if world.resource::<SceneEditorState>().selected_asset.as_deref() == Some(model) =>
        {
            world.write_message(OpenAssetPreview { path: model.into() });
            if run.reopen { Phase::BrowseOtherFile } else { Phase::NavigateScene }
        }
        Phase::NavigateScene if click_matching(world, run, window, |tab: &SceneEditorSidebarTabButton| {
            tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree)
        }) => Phase::GltfTreeImage,
        Phase::GltfTreeImage => {
            let Some((path, rows)) = gltf_tree_validation(world) else { return Ok(None) };
            let Some(camera) = world.resource::<AssetPreviewState>().validation_camera() else { return Ok(None) };
            if !gltf_tree::validation_header(world).is_some_and(|(_, docked)| docked)
                || world.get::<AtmosphereSettings>(camera).is_none()
            {
                return Ok(None);
            }
            if path != SOURCE || !rows.iter().any(|row| row.starts_with("场景 0") || row.starts_with("Scene 0"))
                || !rows.iter().any(|row| row.starts_with("网格 0") || row.starts_with("Mesh 0"))
                || world.query_filtered::<&Node, With<EditorSceneTreePanel>>()
                    .iter(world).any(|node| node.display != Display::None)
            {
                return Err("左侧场景树未切换为当前 GLB 的场景和网格".into());
            }
            let region = motion_region(world);
            if run.capture.is_none() && region.is_none() {
                return Ok(None);
            }
            if !capture(world, run, "gltf-tree.png", region)? {
                return Ok(None);
            }
            Phase::ToggleGrid
        }
        Phase::ToggleGrid if click_matching(world, run, window, |action: &view_toolbar::Action| {
            *action == view_toolbar::Action::Grid
        }) => Phase::RestoreGrid,
        Phase::RestoreGrid => {
            let Some(camera) = world.resource::<AssetPreviewState>().validation_camera() else { return Ok(None) };
            if world.get::<GridView>(camera).is_none()
                || !click_matching(world, run, window, |action: &view_toolbar::Action| *action == view_toolbar::Action::Grid)
            {
                return Ok(None);
            }
            Phase::SelectWire
        }
        Phase::SelectWire => {
            let Some(camera) = world.resource::<AssetPreviewState>().validation_camera() else { return Ok(None) };
            let button = crate::viewport::shading::validation_asset_mode_button(world, 0);
            if world.get::<GridView>(camera).is_some() || !click_entity(world, run, window, button) {
                return Ok(None);
            }
            Phase::RestoreShading
        }
        Phase::RestoreShading => {
            let Some(camera) = world.resource::<AssetPreviewState>().validation_camera() else { return Ok(None) };
            let button = crate::viewport::shading::validation_asset_mode_button(world, 3);
            if !world.get::<ShadingView>(camera).is_some_and(|view| view.options.x == 0.0)
                || !click_entity(world, run, window, button)
            {
                return Ok(None);
            }
            Phase::OpenShadingMenu
        }
        Phase::OpenShadingMenu => {
            let button = crate::viewport::shading::validation_asset_menu_button(world);
            if !click_entity(world, run, window, button) {
                return Ok(None);
            }
            Phase::CloseShadingMenu
        }
        Phase::CloseShadingMenu => {
            use crate::viewport::shading::AssetPreviewShadingState;
            if !world.resource::<AssetPreviewShadingState>().0.menu_open() {
                return Ok(None);
            }
            let button = crate::viewport::shading::validation_asset_menu_button(world);
            if !click_entity(world, run, window, button) {
                return Ok(None);
            }
            Phase::ReturnToMapTree
        }
        Phase::ReturnToMapTree if click_matching(world, run, window, |action: &AssetPreviewWorkspaceAction| {
            *action == AssetPreviewWorkspaceAction::ToggleExpand
        }) => Phase::MapTreeRestored,
        Phase::MapTreeRestored => {
            if gltf_tree_validation(world).is_some()
                || !gltf_tree::validation_header(world).is_some_and(|(_, docked)| !docked)
                || !world.query_filtered::<&Node, With<EditorSceneTreePanel>>()
                .iter(world).any(|node| node.display == Display::Flex)
            {
                return Ok(None);
            }
            run.gltf_tree_verified = true;
            world.write_message(OpenAssetPreview { path: model.into() });
            Phase::Launch
        }
        Phase::BrowseOtherFile if gltf_tree_validation(world).is_some()
            && click_matching(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
            }) => Phase::VerifyOtherFile,
        Phase::VerifyOtherFile if click_asset_row(world, run, window, IDLE) => Phase::MotionSceneTree,
        Phase::MotionSceneTree => {
            if world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(IDLE)
                || world.resource::<SceneEditorSidebarState>().right != EditorRightSidebarTab::Assets
                || !file_counts_visible(world, 0, 1)
            {
                return Ok(None);
            }
            if world.resource::<AssetPreviewState>().selected_path.as_deref() != Some(model)
                || properties_panel_visible(world)
            {
                return Err("选择工程里的其他文件改变了中央 GLB 或串入动作面板".into());
            }
            if !click_matching(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree)
            }) {
                return Ok(None);
            }
            Phase::SelectModelRoot
        }
        Phase::SelectModelRoot => {
            let button = gltf_tree::validation_root(world);
            if !click_entity(world, run, window, button) {
                return Ok(None);
            }
            Phase::OpenMotionPicker
        }
        Phase::OpenMotionPicker => {
            if world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(model)
                || world.resource::<SceneEditorSidebarState>().right != EditorRightSidebarTab::Assets
                || (run.reopen && !file_counts_visible(world, 1, 0))
                || !click(world, run, window, RigAction::ChooseProjectMotion)
            {
                return Ok(None);
            }
            if run.reopen {
                run.property_switch_verified = true;
                run.reopened_motion_picker = true;
            }
            Phase::PickIdle
        }
        Phase::Launch => {
            if !run.reopen && !world.resource::<SceneEditorSidebarState>().scene_tree_visible() {
                return Ok(None);
            }
            if click(world, run, window, RigAction::Launch) {
                Phase::Inspect
            } else {
                if !run.launch_snapshot && run.phase_started.elapsed() > Duration::from_secs(5) {
                    run.launch_snapshot = true;
                    let path = run.root.join("launch.png");
                    world.spawn(Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(path));
                    eprintln!("绑骨入口等待详情 {}", diagnostics(world, run));
                }
                return Ok(None);
            }
        }
        Phase::Inspect | Phase::InspectBlender => {
            let status = world.resource::<RiggingState>().validation_status();
            if !status.open || status.busy || status.source != Some(model) {
                return Ok(None);
            }
            if matches!(phase, Phase::Inspect) && !status.native {
                return Err("编辑器未默认使用原生绑骨".into());
            } else if matches!(phase, Phase::Inspect) && run.backend == "blender" {
                Phase::SwitchBackend
            } else if matches!(phase, Phase::InspectBlender) && status.native {
                return Ok(None);
            } else if status.landmarks == 17 {
                if !world.resource::<SceneEditorSidebarState>().assets_browser_visible()
                    || status.animation.is_some()
                {
                    return Err("启动绑骨应打开左侧工程页并等待显式选择参考".into());
                }
                Phase::RejectReference
            } else {
                return Ok(None);
            }
        }
        Phase::SwitchBackend if click(world, run, window, RigAction::SwitchBackend) => {
            Phase::InspectBlender
        }
        Phase::RejectReference if click_asset_row(world, run, window, SOURCE) => Phase::ChooseReference,
        Phase::ChooseReference => {
            let status = world.resource::<RiggingState>().validation_status();
            if status.reference != Some(SOURCE) {
                return Ok(None);
            }
            if status.animation.is_some() {
                return Err("未蒙皮的原模型被误选为参考骨架".into());
            }
            if !click_asset_row(world, run, window, IDLE) {
                return Ok(None);
            }
            Phase::ReferenceSelected
        }
        Phase::ReferenceSelected => {
            let status = world.resource::<RiggingState>().validation_status();
            if status.reference != Some(IDLE) {
                return Ok(None);
            }
            if status.animation != Some(IDLE) || status.landmarks != 17
                || world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(SOURCE)
                || world.resource::<AssetPreviewState>().selected_path.as_deref() != Some(SOURCE)
            {
                return Err("左侧参考选择未独立于中央模型和属性目标".into());
            }
            Phase::Attributes
        }
        Phase::Attributes if click_right_tab(world, run, window, EditorRightSidebarTab::Scene) => {
            Phase::RestoreRigging
        }
        Phase::RestoreRigging => {
            let sidebar = world.resource::<SceneEditorSidebarState>();
            if !sidebar.tab_active(EditorSidebarTab::Right(EditorRightSidebarTab::Scene))
                || !sidebar.rigging_available
                || !click_right_tab(world, run, window, EditorRightSidebarTab::Tools)
            {
                return Ok(None);
            }
            Phase::FrontAxis
        }
        Phase::FrontAxis => {
            if world.resource::<SceneEditorSidebarState>().right != EditorRightSidebarTab::Rigging {
                return Ok(None);
            }
            let status = world.resource::<RiggingState>().validation_status();
            if !status.open || status.reference != Some(IDLE) || status.landmarks != 17 {
                return Err("从属性返回绑骨页后丢失了当前参考或关节点".into());
            }
            if !click_filtered::<SceneEditorOrientationAxisEndpoint, With<PreviewOrientationAxis>>(
                world, run, window, |axis| axis.world_axis == Vec3::Z && axis.positive,
            ) {
                return Ok(None);
            }
            run.restored_rigging_tab = true;
            Phase::FrontView
        }
        Phase::FrontView => {
            let preview = world.resource::<AssetPreviewState>();
            let Some(camera) = preview.validation_camera() else { return Ok(None); };
            if !matches!(world.get::<Projection>(camera), Some(Projection::Orthographic(_))) {
                return Ok(None);
            }
            let yaw = world.resource::<RiggingState>().front_yaw(
                world.resource::<DocumentState>().asset_root(), preview,
            );
            let transform = world.get::<Transform>(camera).ok_or("资产预览相机没有位姿")?;
            let expected = Quat::from_rotation_y(yaw) * Vec3::Z;
            if (transform.rotation * Vec3::Z).dot(expected) < 0.9999 {
                return Err("点击模型方向球正 Z 后，正交相机没有对准人物正面".into());
            }
            run.front_view = true;
            Phase::AimMarker
        }
        Phase::AimMarker => {
            let dot = world
                .query::<(&RigLandmarkDot, &UiGlobalTransform, &ComputedNode)>()
                .iter(world)
                .find_map(|(dot, transform, node)| {
                    (dot.0 == LEFT_WRIST && node.size().min_element() > 1.0)
                        .then_some(transform.affine().translation.as_dvec2())
                });
            let Some(dot) = dot else { return Ok(None) };
            let state = world.resource::<RiggingState>();
            let mirror_expected = !run.mirrored_marker;
            if state.validation_mirror_enabled() != mirror_expected {
                return Err("关节点镜像开关与本次拖动阶段不符".into());
            }
            let scale = f64::from(world.get::<Window>(window).unwrap().scale_factor());
            let delta = if run.mirrored_marker { DVec2::new(-5.0, 3.0) } else { DVec2::new(6.0, -4.0) };
            run.drag = Some(DragWitness {
                cursor: dot,
                destination: dot + delta * scale,
                left_before: read_marker(state, LEFT_WRIST)?,
                right_before: read_marker(state, RIGHT_WRIST)?,
            });
            move_cursor(world, window, dot);
            Phase::PressMarker
        }
        Phase::PressMarker => {
            let drag = run.drag.ok_or("缺少关节点拖动起点")?;
            move_cursor(world, window, drag.cursor);
            let hover = world
                .query_filtered::<&UiInteraction, With<AssetPreviewCanvas>>()
                .iter(world)
                .any(|interaction| *interaction == UiInteraction::Hovered);
            if !hover {
                return Ok(None);
            }
            window_event(
                world,
                MouseButtonInput {
                    window,
                    button: MouseButton::Left,
                    state: ButtonState::Pressed,
                },
            );
            Phase::DragMarker
        }
        Phase::DragMarker => {
            if !world.resource::<RiggingState>().owns_mouse() {
                return Ok(None);
            }
            let drag = run.drag.ok_or("缺少关节点拖动起点")?;
            move_cursor(world, window, drag.destination);
            Phase::ReleaseMarker
        }
        Phase::ReleaseMarker => {
            let drag = run.drag.ok_or("缺少关节点拖动起点")?;
            let state = world.resource::<RiggingState>();
            if read_marker(state, LEFT_WRIST)?.distance(drag.left_before) < 1e-5 {
                return Ok(None);
            }
            window_event(
                world,
                MouseButtonInput {
                    window,
                    button: MouseButton::Left,
                    state: ButtonState::Released,
                },
            );
            Phase::VerifyMarker
        }
        Phase::VerifyMarker => {
            let state = world.resource::<RiggingState>();
            if state.owns_mouse() {
                return Ok(None);
            }
            let drag = run.drag.take().ok_or("缺少关节点拖动起点")?;
            let left = read_marker(state, LEFT_WRIST)?;
            if left.distance(drag.left_before) < 1e-5 {
                return Err("松开鼠标后，拖动的左腕没有保留最终位置".into());
            }
            let right = read_marker(state, RIGHT_WRIST)?;
            let next = if run.mirrored_marker {
                if right != drag.right_before {
                    return Err("关闭镜像后，拖动左腕仍改动了右腕".into());
                }
                run.independent_marker = true;
                Phase::Bind
            } else {
                // 本轮只拖腕。以仍为原值的双肩重建人物中面，独立核对实际右腕坐标。
                let left_shoulder = read_marker(state, 3)?;
                let right_shoulder = read_marker(state, 10)?;
                let midpoint = left_shoulder.midpoint(right_shoulder);
                let lateral = (left_shoulder - right_shoulder).with_y(0.0)
                    .try_normalize().ok_or("双肩不能建立镜像中面")?;
                let expected = left - 2.0 * (left - midpoint).dot(lateral) * lateral;
                if right.distance(expected) > 1e-4 || right.distance(drag.right_before) < 1e-5 {
                    return Err(format!("拖动左腕后右腕镜像不符：实际 {right:?}，预期 {expected:?}"));
                }
                run.mirrored_marker = true;
                Phase::MarkersImage
            };
            run.dragged_marker = true;
            next
        }
        Phase::MarkersImage => {
            // 检查颈与骨盆之间的躯干部位，排除圆点自身让空画布误通过。
            let region = if run.capture.is_none() {
                let Some(region) = marker_model_region(world) else { return Ok(None) };
                Some(region)
            } else {
                None
            };
            if !capture(world, run, "landmarks.png", region)? {
                return Ok(None);
            }
            Phase::ToggleMirror
        }
        Phase::ToggleMirror if click(world, run, window, RigAction::ToggleMirror) => Phase::IndependentMarkers,
        Phase::IndependentMarkers if !world.resource::<RiggingState>().validation_mirror_enabled() => Phase::AimMarker,
        Phase::Bind if click(world, run, window, RigAction::Bind) => Phase::Published,
        Phase::Published
            if world.resource::<RiggingState>().validation_status().output == Some(OUTPUT) =>
        {
            Phase::OpenMotionPicker
        }
        Phase::PickIdle => {
            let button = run.click.map(|(entity, _)| entity).or_else(|| animation_file_button(world, IDLE));
            if !click_entity(world, run, window, button) {
                return Ok(None);
            }
            Phase::Preview
        }
        Phase::Preview => {
            let Some(tabs) = active_model_tab(world, model) else { return Ok(None) };
            let button = run.click.map(|(entity, _)| entity).or_else(|| animation_clip_button(world, 0));
            if !click_entity(world, run, window, button) {
                return Ok(None);
            }
            // 在物理松手事件被原按钮系统处理前取证。下一帧起每帧复核，包含后台过渡期。
            run.tabs.get_or_insert(tabs);
            Phase::Motion
        }
        Phase::Motion => {
            if !properties_panel_visible(world) || timeline_dock_probe(world).is_none() {
                return Ok(None);
            }
            let Some(evidence) = observe_motion(world, run, model, action)? else {
                return Ok(None);
            };
            if run.reopen && !run.second_motion {
                run.first_motion = Some(evidence);
                Phase::PlayingBrowseOtherFile
            } else {
                run.motion = Some(evidence);
                Phase::AimDivider
            }
        }
        Phase::PlayingBrowseOtherFile if click_matching(world, run, window, |tab: &SceneEditorSidebarTabButton| {
            tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
        }) => Phase::PlayingSelectOtherFile,
        Phase::PlayingSelectOtherFile if click_asset_row(world, run, window, WALK) => Phase::PlayingInspectOtherFile,
        Phase::PlayingInspectOtherFile => {
            if world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(WALK)
                || world.resource::<SceneEditorSidebarState>().right != EditorRightSidebarTab::Assets
                || properties_panel_visible(world)
                || !file_counts_visible(world, 0, 1)
                || timeline_dock_probe(world).is_none()
            {
                return Ok(None);
            }
            if !run.playing_other_file_verified {
                // 两份姿态都在另一文件属性可见后采集，避免把点击前已播放的帧当成持续播放。
                let Some(evidence) = observe_motion(world, run, model, IDLE)? else { return Ok(None) };
                run.first_motion.as_mut().ok_or("缺少重开后的待机证据")?["while_other_file_visible"] = evidence;
                run.playing_other_file_verified = true;
            }
            if !click_matching(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree)
            }) {
                return Ok(None);
            }
            Phase::PlayingRestoreModel
        }
        Phase::PlayingRestoreModel => {
            let button = gltf_tree::validation_root(world);
            if !click_entity(world, run, window, button) {
                return Ok(None);
            }
            Phase::PlayingModelRestored
        }
        Phase::PlayingModelRestored => {
            if world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(model)
                || !properties_panel_visible(world) || !file_counts_visible(world, 1, 0)
                || timeline_dock_probe(world).is_none() || rig_animation_probe(world).is_none()
            {
                return Ok(None);
            }
            let region = motion_region(world);
            if (run.capture.is_none() && region.is_none()) || !capture(world, run, "reopen-idle.png", region)? {
                return Ok(None);
            }
            Phase::OpenSecondPicker
        }
        Phase::OpenSecondPicker if click(world, run, window, RigAction::ChooseProjectMotion) => Phase::SwitchMotion,
        Phase::SwitchMotion => {
            let button = run.click.map(|(entity, _)| entity).or_else(|| animation_file_button(world, WALK));
            if !click_entity(world, run, window, button) {
                return Ok(None);
            }
            run.second_motion = true;
            Phase::Preview
        }
        Phase::AimDivider => {
            let Some(dock) = timeline_dock_probe(world) else { return Ok(None) };
            verify_divider_order(dock.splitter_center.y, dock.canvas_bottom, dock.timeline_top)?;
            let cursor = dock.splitter_center.as_dvec2();
            let witness = run.divider.get_or_insert_with(|| DividerWitness {
                entity: dock.splitter,
                destination: cursor,
                before: Vec2::new(dock.canvas_height, dock.timeline_height),
                at_release: None,
                enlarged: None,
                after: None,
            });
            if witness.entity != dock.splitter || dock.dragging {
                return Err("开始拖动前，时间轴分隔条的身份或鼠标捕获异常".into());
            }
            let scale = f64::from(world.get::<Window>(window).unwrap().scale_factor());
            let delta = if witness.enlarged.is_some() { 40.0 } else { -64.0 };
            witness.destination = cursor + DVec2::Y * delta * scale;
            witness.at_release = None;
            move_cursor(world, window, cursor);
            Phase::PressDivider
        }
        Phase::PressDivider => {
            window_event(world, MouseButtonInput {
                window, button: MouseButton::Left, state: ButtonState::Pressed,
            });
            Phase::DragDivider
        }
        Phase::DragDivider => {
            let Some(dock) = timeline_dock_probe(world).filter(|dock| dock.dragging) else { return Ok(None) };
            let witness = run.divider.as_ref().ok_or("缺少分隔条拖动起点")?;
            if witness.entity != dock.splitter { return Err("拖动中分隔条换代".into()); }
            move_cursor(world, window, witness.destination);
            Phase::ReleaseDivider
        }
        Phase::ReleaseDivider => {
            let Some(dock) = timeline_dock_probe(world).filter(|dock| dock.dragging) else { return Ok(None) };
            let witness = run.divider.as_mut().ok_or("缺少分隔条拖动起点")?;
            let sizes = Vec2::new(dock.canvas_height, dock.timeline_height);
            let direction = if witness.enlarged.is_some() { -1.0 } else { 1.0 };
            let delta = direction * (sizes - witness.enlarged.unwrap_or(witness.before));
            if delta.x > -12.0 || delta.y < 12.0 { return Ok(None); }
            witness.at_release = Some(sizes);
            window_event(world, MouseButtonInput {
                window, button: MouseButton::Left, state: ButtonState::Released,
            });
            Phase::VerifyDivider
        }
        Phase::VerifyDivider => {
            let Some(dock) = timeline_dock_probe(world).filter(|dock| !dock.dragging) else { return Ok(None) };
            verify_divider_order(dock.splitter_center.y, dock.canvas_bottom, dock.timeline_top)?;
            let witness = run.divider.as_mut().ok_or("缺少分隔条拖动记录")?;
            let sizes = Vec2::new(dock.canvas_height, dock.timeline_height);
            if !sizes.abs_diff_eq(witness.at_release.ok_or("未观察到拖动后的布局")?, 2.0) {
                return Err("松开鼠标后，画布和时间轴未保留调整后的高度".into());
            }
            if witness.enlarged.is_none() {
                witness.enlarged = Some(sizes);
                Phase::ExpandedTimelineImage
            } else {
                witness.after = Some(sizes);
                Phase::MotionImage
            }
        }
        Phase::ExpandedTimelineImage => {
            let filename = if run.reopen { "walk-timeline-expanded.png" } else { "idle-timeline-expanded.png" };
            let region = motion_region(world);
            if (run.capture.is_none() && region.is_none()) || !capture(world, run, filename, region)? {
                return Ok(None);
            }
            Phase::AimDivider
        }
        Phase::MotionImage => {
            let filename = if run.reopen { "walk.png" } else { "idle.png" };
            let region = if run.capture.is_none() {
                if rig_animation_probe(world).is_none() || !properties_panel_visible(world)
                    || timeline_dock_probe(world).is_none()
                    || world.resource::<SceneEditorState>().selected_asset.as_deref() != Some(model)
                {
                    return Ok(None);
                }
                let Some(region) = motion_region(world) else { return Ok(None) };
                Some(region)
            } else {
                None
            };
            if !capture(world, run, filename, region)? {
                return Ok(None);
            }
            if run.reopen { Phase::SeekPlayingRuler } else { Phase::Verify }
        }
        Phase::SeekPlayingRuler => {
            let Some((ruler, expected)) = timeline_ruler(world) else { return Ok(None) };
            if !click_entity(world, run, window, Some(ruler)) { return Ok(None); }
            run.ruler_seek = Some((expected, None));
            Phase::VerifyPlayingRuler
        }
        Phase::VerifyPlayingRuler => {
            let tab = run.tabs.ok_or("缺少帧尺所在模型标签")?;
            if rig_animation_identity(world) != Some((tab.active, OUTPUT, OUTPUT, WALK, 0)) {
                return Err("帧尺定位改变了当前模型标签或工程动作源".into());
            }
            let Some(probe) = rig_animation_probe(world) else { return Ok(None) };
            let (expected, since) = run.ruler_seek.as_mut().ok_or("缺少帧尺目标时刻")?;
            if (probe.time - *expected).abs() > 1e-3 {
                *since = None;
                return Ok(None);
            }
            // 连续观察发布姿态，排除自动播放时恰好经过帧尺中点。
            if since.get_or_insert_with(Instant::now).elapsed() < Duration::from_millis(150) {
                return Ok(None);
            }
            run.motion.as_mut().ok_or("缺少行走预览记录")?["ruler_seek"] = json!({
                "expected_seconds": *expected, "published_seconds": probe.time,
                "paused_at_target": true,
            });
            Phase::StandaloneProject
        }
        Phase::StandaloneProject if click_matching(world, run, window, |tab: &SceneEditorSidebarTabButton| {
            tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
        }) => Phase::StandaloneSelect,
        Phase::StandaloneSelect if click_asset_row(world, run, window, WALK) => Phase::StandaloneOpen,
        Phase::StandaloneOpen if world.resource::<SceneEditorState>().selected_asset.as_deref() == Some(WALK) => {
            // 同标签播放证据已经收齐。显式进入原动作文件的作者模式，复用正式打开入口。
            world.write_message(OpenAssetPreview { path: WALK.into() });
            run.standalone_open_requested = true;
            Phase::StandaloneTimeline
        }
        Phase::StandaloneTimeline => {
            let Some(tab) = active_model_tab(world, WALK) else { return Ok(None) };
            let Some(dock) = timeline_dock_probe(world) else { return Ok(None) };
            let source = world.resource::<DocumentState>().asset_root().join(WALK);
            let Some(probe) = animation_probe(world, &source, None)? else { return Ok(None) };
            if rig_animation_identity(world).is_some() || probe.dirty || probe.actual_clip != 0
                || probe.key.value.len() != 3 || run.tabs.is_some_and(|before| before.active == tab.active)
            {
                return Err("独立动作没有进入自己的干净作者会话和可编辑平移通道".into());
            }
            verify_divider_order(dock.splitter_center.y, dock.canvas_bottom, dock.timeline_top)?;
            run.motion.as_mut().ok_or("缺少行走预览记录")?["standalone_author_timeline"] = json!({
                "source": WALK, "tab_id": tab.active, "clip_index": probe.actual_clip,
                "editable_translation_channel": probe.key_identity.channel,
                "canvas_height": dock.canvas_height, "timeline_height": dock.timeline_height,
            });
            Phase::Verify
        }
        Phase::Verify => {
            if let Some(task) = &mut run.verify {
                return match check_ready(task) {
                    Some(result) => result.map(Some),
                    None => Ok(None),
                };
            }
            let assets = run.assets.clone();
            let source_hashes = run.source_hashes.clone();
            let previous_output = run.previous_output.clone();
            let backend = run.backend;
            let mut motion = run.motion.take().ok_or("缺少实际动画证据")?;
            motion["landmark_drag_verified"] = json!(run.dragged_marker);
            motion["mirrored_landmark_verified"] = json!(run.mirrored_marker);
            motion["independent_landmark_verified"] = json!(run.independent_marker);
            motion["front_view_verified"] = json!(run.front_view);
            motion["front_mesh_pixels_verified"] = json!(!run.reopen);
            motion["rigging_tab_restored"] = json!(run.restored_rigging_tab);
            motion["reopened_project_motion_picker"] = json!(run.reopened_motion_picker);
            motion["other_file_and_glb_properties_verified"] = json!(run.property_switch_verified);
            motion["playing_view_survives_inspecting_another_file"] = json!(run.playing_other_file_verified);
            motion["first_motion_after_reopen"] = json!(run.first_motion.take());
            let tabs = run.tabs.ok_or("缺少播放前的中央模型标签身份")?;
            motion["model_tab_id"] = json!(tabs.active);
            motion["media_tab_count"] = json!(tabs.count);
            motion["same_model_tab_verified"] = json!(true);
            let dock = run.divider.as_ref().ok_or("缺少中央时间轴验收记录")?;
            motion["timeline_under_canvas_verified"] = json!(true);
            motion["timeline_resize_pixels"] = json!({
                "before": dock.before.to_array(),
                "dragged_up": dock.enlarged.ok_or("缺少向上拖动证据")?.to_array(),
                "dragged_down": dock.after.ok_or("缺少向下拖动证据")?.to_array(),
            });
            motion["gltf_tree_and_map_restore_verified"] = json!(run.gltf_tree_verified);
            motion["preview_grid_and_shading_controls_verified"] = json!(!run.reopen);
            motion["left_reference_selection_verified"] = json!(!run.reopen);
            motion["viewport_visible_pixels_verified"] = json!(true);
            run.verify = Some(IoTaskPool::get().spawn(async move {
                verify_disk(&assets, &source_hashes, previous_output.as_deref(), backend, motion)
            }));
            return Ok(None);
        }
        _ => return Ok(None),
    };
    eprintln!("人形绑骨验收 {:?} -> {next:?}（{} ms）", run.phase, run.phase_started.elapsed().as_millis());
    run.phase = next;
    run.phase_started = Instant::now();
    Ok(None)
}

fn diagnostics(world: &mut World, run: &Run) -> Value {
    let rigging = world.get_resource::<RiggingState>().map(|state| {
        let status = state.validation_status();
        json!({"open": status.open, "busy": status.busy, "source": status.source,
            "reference": status.reference, "landmarks": status.landmarks, "native": status.native,
            "notice": status.notice, "mirror_enabled": state.validation_mirror_enabled()})
    });
    let preview = world.get_resource::<AssetPreviewState>().map(|preview| json!({
        "path": preview.selected_path, "content": format!("{:?}", preview.content),
        "status": preview.status, "rendering": preview.render_visible,
        "model": preview.model.as_ref().map(|model| json!({
            "generation": model.generation, "scene_ready": model.scene_ready,
            "meshes": model.scene_mesh_count, "target": format!("{:?}", model.target),
        })),
    }));
    let buttons: Vec<_> = world.query::<(Entity, &RigAction, &Node, &ComputedNode, &UiInteraction, Option<&InteractionDisabled>)>()
        .iter(world).filter(|(_, action, _, _, _, _)| matches!(
            action, RigAction::Launch | RigAction::SwitchBackend | RigAction::ToggleMirror | RigAction::ChooseProjectMotion
        ))
        .map(|(entity, action, node, computed, interaction, disabled)| json!({
            "entity": format!("{entity:?}"), "action": format!("{action:?}"), "display": format!("{:?}", node.display),
            "size": computed.size().to_array(), "interaction": format!("{interaction:?}"),
            "disabled": disabled.is_some(),
        })).collect();
    let window = world.query_filtered::<&Window, With<PrimaryWindow>>().iter(world).next()
        .map(|window| json!({"focused": window.focused, "cursor": window.cursor_position().map(|value| value.to_array())}));
    let properties = validation_details_text(world);
    let motion = rig_animation_identity(world).map(|(tab, host, model, source, clip)| json!({
        "tab": tab, "host": host, "model": model, "source": source, "clip": clip,
    }));
    let dock = timeline_dock_probe(world).map(|dock| json!({
        "splitter": format!("{:?}", dock.splitter), "dragging": dock.dragging,
        "canvas_height": dock.canvas_height, "timeline_height": dock.timeline_height,
        "splitter_y": dock.splitter_center.y, "canvas_bottom": dock.canvas_bottom,
        "timeline_top": dock.timeline_top,
    }));
    json!({"preview": preview, "rigging": rigging, "buttons": buttons, "click": format!("{:?}", run.click),
        "window": window, "file_properties": properties, "motion": motion, "timeline": dock,
        "expected_tabs": format!("{:?}", run.tabs)})
}

fn active_model_tab(world: &World, path: &str) -> Option<TabWitness> {
    let media = world.get_resource::<AssetPreviewWorkspace>()?;
    let workspace = world.get_resource::<VegetationPresetEditorState>()?;
    let tab = media.expanded.as_ref()?;
    (media.active(workspace.active_tab) && tab.path == path).then(|| TabWitness {
        active: tab.id,
        next: media.next_tab,
        count: media.tabs().count(),
    })
}

/// 核对外部动作身份和已显示角色关节。等待新动作采用时，允许画布先保留上一完整帧。
fn observe_motion(world: &World, run: &mut Run, host: &str, action: &str) -> Result<Option<Value>, String> {
    let Some((tab, active_file, target, source, clip)) = rig_animation_identity(world) else { return Ok(None) };
    if run.tabs.is_none_or(|witness| witness.active != tab) || active_file != host || target != OUTPUT {
        return Err("工程动作预览的承载标签或目标模型身份不符".into());
    }
    if source != action || clip != 0 {
        return Ok(None);
    }
    let Some(probe) = rig_animation_probe(world) else { return Ok(None) };
    if probe.model_path != OUTPUT || probe.clip_index != clip {
        return Err("画布发布的角色或动作片段身份不符".into());
    }
    if probe.source.file_name() != Some(std::ffi::OsStr::new(action)) {
        return Ok(None);
    }
    let Some((time, joints)) = &run.sample else {
        run.sample = Some((probe.time, probe.joints));
        return Ok(None);
    };
    let changed = probe.joints.iter().zip(joints).filter(|((name, rotation), (before, old))| {
        name == before && rotation.dot(*old).abs() < 0.99999
    }).count();
    if (probe.time - time).abs() < 0.08 || changed == 0 {
        return Ok(None);
    }
    run.sample = None;
    Ok(Some(json!({
        "model_tab_id": tab, "host_file": host, "target_model": target,
        "animation": action, "clip": probe.clip_name, "clip_index": clip, "time": probe.time,
        "changed_joints": changed, "bound_joints": probe.joints.len(),
        "visible_model_root": format!("{:?}", probe.root),
    })))
}

fn verify_divider_order(splitter: f32, canvas_bottom: f32, timeline_top: f32) -> Result<(), String> {
    if ![splitter, canvas_bottom, timeline_top].into_iter().all(f32::is_finite)
        || canvas_bottom > splitter + 2.0 || timeline_top < splitter - 2.0
    {
        return Err("分隔条应位于画布和中央底部时间轴之间".into());
    }
    Ok(())
}

fn read_marker(state: &RiggingState, index: usize) -> Result<Vec3, String> {
    state.validation_landmark(index).map(Vec3::from_array)
        .ok_or_else(|| format!("缺少验收关节点 {index}"))
}

fn click(world: &mut World, run: &mut Run, window: Entity, action: RigAction) -> bool {
    click_matching(world, run, window, |marker: &RigAction| *marker == action)
}

fn click_asset_row(world: &mut World, run: &mut Run, window: Entity, path: &str) -> bool {
    let Some(index) = world.resource::<EditorAssetCatalog>().entries.iter()
        .position(|entry| entry.asset_path == path) else { return false };
    click_matching(world, run, window, |row: &SceneEditorAssetRow| row.entry_index == Some(index))
}

/// 共用实窗驱动只在目录采用动画筛选后命中完整路径对应的展开按钮。
pub(super) fn animation_file_button(world: &mut World, path: &str) -> Option<Entity> {
    let catalog = world.resource::<EditorAssetCatalog>();
    if catalog.is_loading() || catalog.adopted_type_filter != AssetTypeFilter::Animation {
        return None;
    }
    let index = catalog.entries.iter().position(|entry| entry.asset_path == path)?;
    visible_button::<ProjectAnimationExpand, ()>(world, |expand| {
        expand.0.is_some_and(|binding| binding.index == Some(index))
    })
}

pub(super) fn animation_clip_button(world: &mut World, index: usize) -> Option<Entity> {
    visible_button::<ProjectAnimationClip, ()>(world, |clip| clip.clip_index == Some(index))
}

fn file_counts_visible(world: &mut World, meshes: usize, clips: usize) -> bool {
    validation_details_text(world).is_some_and(|text| {
        text.contains(&format!("网格 {meshes}\n蒙皮 1 · 动画 {clips}"))
            || text.contains(&format!("Meshes {meshes}\nSkins 1 · Animations {clips}"))
    })
}

fn click_right_tab(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    tab: EditorRightSidebarTab,
) -> bool {
    click_matching(world, run, window, |button: &SceneEditorSidebarTabButton| {
        button.0 == EditorSidebarTab::Right(tab)
    })
}

fn click_matching<T: Component>(world: &mut World, run: &mut Run, window: Entity, matches: impl Fn(&T) -> bool) -> bool {
    click_filtered::<T, ()>(world, run, window, matches)
}

fn click_filtered<T: Component, F: QueryFilter>(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    matches: impl Fn(&T) -> bool,
) -> bool {
    let entity = match run.click {
        Some((entity, _)) => Some(entity),
        None => visible_button::<T, F>(world, matches),
    };
    let Some(entity) = entity else { return false };
    click_entity(world, run, window, Some(entity))
}

fn visible_button<T: Component, F: QueryFilter>(
    world: &mut World, matches: impl Fn(&T) -> bool,
) -> Option<Entity> {
    world
        .query_filtered::<(
            Entity,
            &T,
            &Node,
            &ComputedNode,
            Option<&InteractionDisabled>,
            Option<&Visibility>,
        ), F>()
        .iter(world)
        .find(|(_, marker, node, computed, disabled, visibility)| {
            matches(marker) && node.display != Display::None
                && *visibility != Some(&Visibility::Hidden)
                && computed.size().min_element() > 1.0 && disabled.is_none()
        })
        .map(|(entity, _, _, _, _, _)| entity)
}

fn click_entity(world: &mut World, run: &mut Run, window: Entity, entity: Option<Entity>) -> bool {
    click_window_entity(world, &mut run.click, window, entity)
}

/// 实窗专项共用逐帧指针输入，只有官方三态确认命中与按下后才发释放。
pub(super) fn click_window_entity(
    world: &mut World,
    click: &mut Option<(Entity, bool)>,
    window: Entity,
    entity: Option<Entity>,
) -> bool {
    let Some(entity) = entity else { return false };
    let Some(transform) = world.get::<UiGlobalTransform>(entity) else {
        return false;
    };
    let physical = transform.affine().translation.as_dvec2();
    move_cursor(world, window, physical);
    match *click {
        None => *click = Some((entity, false)),
        Some((_, false)) if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Hovered) => {
            window_event(
                world,
                MouseButtonInput {
                    window,
                    button: MouseButton::Left,
                    state: ButtonState::Pressed,
                },
            );
            *click = Some((entity, true));
        }
        Some((_, true)) if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Pressed) => {
            window_event(
                world,
                MouseButtonInput {
                    window,
                    button: MouseButton::Left,
                    state: ButtonState::Released,
                },
            );
            *click = None;
            return true;
        }
        _ => {}
    }
    false
}

pub(super) fn move_cursor(world: &mut World, window: Entity, physical: DVec2) {
    let mut target = world.get_mut::<Window>(window).unwrap();
    target.set_physical_cursor_position(Some(physical));
    let position = physical.as_vec2() / target.scale_factor();
    window_event(
        world,
        CursorMoved {
            window,
            position,
            delta: None,
        },
    );
}

pub(super) fn window_event(world: &mut World, event: impl Message + Clone + Into<WindowEvent>) {
    let window_event: WindowEvent = event.clone().into();
    world.write_message(window_event);
    world.write_message(event);
}

fn marker_model_region(world: &mut World) -> Option<URect> {
    let mut points = [None; 4];
    for (dot, node, transform) in world.query::<(&RigLandmarkDot, &ComputedNode, &UiGlobalTransform)>().iter(world) {
        let slot = match dot.0 { 1 => 0, 2 => 1, 3 => 2, 10 => 3, _ => continue };
        if node.size().min_element() > 1.0 {
            points[slot] = Some(transform.affine().translation);
        }
    }
    let [Some(neck), Some(hips), Some(left), Some(right)] = points else { return None };
    let center = neck.midpoint(hips);
    let half = Vec2::new((left.x - right.x).abs() * 0.65, (neck.y - hips.y).abs() * 0.16);
    Some(URect::from_corners((center - half).as_uvec2(), (center + half).as_uvec2()))
}

fn motion_region(world: &mut World) -> Option<URect> {
    let (node, transform, _) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<AssetPreviewModelViewport>>()
        .iter(world)
        .filter(|(node, _, visible)| visible.get() && node.size().min_element() > 16.0)
        .max_by(|(a, _, _), (b, _, _)| a.size().element_product().total_cmp(&b.size().element_product()))?;
    let center = transform.affine().translation;
    // 取中央范围，排除边框和左上角编辑按钮，检测真正合成到画布的非背景像素。
    let half = node.size() * Vec2::new(0.24, 0.39);
    Some(URect::from_corners((center - half).as_uvec2(), (center + half).as_uvec2()))
}

fn capture(world: &mut World, run: &mut Run, filename: &str, region: Option<URect>) -> Result<bool, String> {
    if let Some(receiver) = &mut run.capture {
        return match receiver
            .get_mut()
            .map_err(|error| error.to_string())?
            .try_recv()
        {
            Ok(result) => {
                run.capture = None;
                result
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => Err("实际 GPU 截图没有回执".into()),
        };
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let path = run.root.join(filename);
    let verify_sky = filename == "gltf-tree.png";
    let perspective = world.get_resource::<AssetPreviewState>()
        .and_then(AssetPreviewState::validation_camera)
        .and_then(|camera| world.get::<Projection>(camera))
        .is_some_and(|projection| matches!(projection, Projection::Perspective(_)));
    run.capture = Some(Mutex::new(receiver));
    world
        .spawn(Screenshot::primary_window())
        .observe(move |event: On<ScreenshotCaptured>| {
            let (image, path, sender) = (event.image.clone(), path.clone(), sender.clone());
            IoTaskPool::get()
                .spawn(async move {
                    let result = image
                        .try_into_dynamic()
                        .map_err(|error| error.to_string())
                        .and_then(|image| {
                            if let Some(region) = region {
                                let pixels = image.to_rgb8();
                                let max = region.max.min(UVec2::new(pixels.width(), pixels.height()));
                                if region.min.cmpge(max).any() {
                                    return Err("GPU 截图缺少有效画布范围".into());
                                }
                                if verify_sky {
                                    // 隔离工程的日光在画布左上方应有实际蓝色大气像素。
                                    let sky_width = (max.x - region.min.x).min(80);
                                    let sky_y = (region.min.y + 8).min(max.y - 1);
                                    let blue = (region.min.x..region.min.x + sky_width)
                                        .filter(|&x| {
                                            let [r, _, b] = pixels.get_pixel(x, sky_y).0;
                                            b.saturating_sub(r) > 12
                                        }).count();
                                    if blue < 32 {
                                        return Ok(false);
                                    }
                                }
                                // 透视夹具的地平线在画布中心上方。只在下半区比较模型与背景，
                                // 避免大气先就绪时把地平线误判成已渲染的角色。
                                let model_top = if perspective {
                                    region.min.y + (max.y - region.min.y) / 2
                                } else {
                                    region.min.y
                                };
                                let background = pixels.get_pixel(region.min.x, model_top).0;
                                let foreground = (model_top..max.y)
                                    .flat_map(|y| (region.min.x..max.x).map(move |x| (x, y)))
                                    .filter(|&(x, y)| pixels.get_pixel(x, y).0.iter().zip(background)
                                        .any(|(value, clear)| value.abs_diff(clear) > 16))
                                    .take(128)
                                    .count();
                                if foreground < 128 {
                                    // 渲染准备落后于 CPU 姿态时重取新帧，空画布不得通过验收。
                                    return Ok(false);
                                }
                            }
                            image.save(path).map(|()| true).map_err(|error| error.to_string())
                        });
                    let _ = sender.send(result);
                })
                .detach();
        });
    Ok(false)
}

fn hash(assets: &Path, name: &str) -> Result<String, String> {
    let path = assets.join(name);
    if path.canonicalize().ok().as_ref() != Some(&path) {
        return Err(format!("绑骨验收资产不是本轮普通文件：{name}"));
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn hashes(assets: &Path) -> Result<[String; 3], String> {
    Ok([
        hash(assets, SOURCE)?,
        hash(assets, IDLE)?,
        hash(assets, WALK)?,
    ])
}

fn verify_disk(
    assets: &Path,
    original: &[String; 3],
    previous: Option<&str>,
    backend: &str,
    motion: Value,
) -> Result<Value, String> {
    if &hashes(assets)? != original {
        return Err("绑骨流程改动了原模型或原动作文件".into());
    }
    let model_sha256 = hash(assets, OUTPUT)?;
    if previous.is_some_and(|previous| previous != model_sha256) {
        return Err("重开后已生成模型字节被改写".into());
    }
    let glb = gltf::Gltf::open(assets.join(OUTPUT)).map_err(|error| error.to_string())?;
    let joints = glb.skins().next().map_or(0, |skin| skin.joints().count());
    if joints != 65
        || glb.animations().len() != 0
        || !glb
            .nodes()
            .any(|node| node.mesh().is_some() && node.skin().is_some())
    {
        return Err("新 GLB 未保存预期的独立角色和蒙皮".into());
    }
    Ok(json!({
        "passed": true, "os_input_verified": false, "reopened": previous.is_some(),
        "model": OUTPUT, "model_sha256": model_sha256, "source_sha256": original,
        "binding_backend": backend, "joint_count": joints, "embedded_clips": 0, "motion": motion,
    }))
}

fn report(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|error| error.to_string())
}
