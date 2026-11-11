//! 目录筛选与动作选择真窗口验收。仅在通过宿主归属核对的自建工程启用。

mod evidence;

use super::{
    AssetPreviewState, AssetPreviewWorkspace,
    animation::{active_project_clip, project_clip_properties, rig_animation_identity, rig_animation_probe},
    rigging_validation::{click_window_entity, fixture_focus, move_cursor, window_event},
    types::{AssetPreviewContent, AssetPreviewPhase},
};
use crate::{
    asset_catalog::{AssetTypeFilter, EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::{SceneEditorActionButton, SceneEditorAssetRow, SceneEditorPane, SceneEditorPaneSplitter},
    constants::EDITOR_CONTENT_MIN_WIDTH,
    resources::{SceneEditorLayoutState, SceneEditorState},
    types::{EditorAction, EditorPane},
    ui::{
        asset_thumbnails::{ViewModeButton, validation_visible_thumbnail},
        project_browser::{
            ProjectAnimationClip, ProjectAnimationExpand, ProjectBrowserState, ProjectBrowserView,
            ProjectFileRow, ProjectTypeFilterButton, ProjectTypeFilterOption,
        },
        sidebars::{EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarState, SceneEditorSidebarTabButton},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{
    app::AppExit,
    input::{ButtonState, mouse::MouseButtonInput},
    math::DVec2,
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::{InteractionDisabled, UiGlobalTransform},
    window::PrimaryWindow,
};
use feathrise_editor_core::{AssetBrowserViewMode, EditorSettings};
use feathrise_editor_ui::UiInteraction;
use serde_json::{Map, Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const HERO: &str = "roles/nested/hero.glb";
const STATIC: &str = "roles/nested/static.glb";
const MOTIONS: &str = "motions.glb";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    OpenDirectory,
    FilterSkeleton,
    OpenAssets,
    OpenRoles,
    OpenNested,
    SkeletonCapture,
    SelectHero,
    OpenHero,
    HeroReady,
    RestoreDirectory,
    FilterAll,
    UpToRoles,
    UpToAssets,
    Narrow,
    OpenNarrowMenu,
    NarrowCapture,
    ChooseAnimation,
    UseThumbnails,
    Expand,
    ClipMenuCapture,
    ChooseSecond,
    ObservePlayback,
    PlaybackCapture,
    Replace,
    AwaitReplacement,
    ChooseModel,
    ReplacementCapture,
    Complete,
}

#[derive(Resource)]
struct Run {
    work: PathBuf,
    assets: PathBuf,
    original_motion: Vec<u8>,
    replacement_size: u64,
    stage: Stage,
    started: Instant,
    stage_started: Instant,
    click: Option<(Entity, bool)>,
    splitter_x: Option<DVec2>,
    tab: Option<(u64, usize)>,
    sample: Option<(f32, Quat)>,
    animation_revision: Option<u64>,
    capture: Option<evidence::CaptureTask>,
    replacement: Option<Task<Result<(), String>>>,
    replacement_revision: Option<u64>,
    steps: Map<String, Value>,
    screenshots: Map<String, Value>,
}

pub fn install(app: &mut App, work: PathBuf, assets: PathBuf) -> Result<(), String> {
    let source = assets.join(MOTIONS);
    if source.canonicalize().ok().as_ref() != Some(&source) {
        return Err("动作验收夹具必须位于自有资产根".into());
    }
    let original_motion = fs::read(source).map_err(|error| error.to_string())?;
    let replacement_size = fs::metadata(assets.join(STATIC)).map_err(|error| error.to_string())?.len();
    let now = Instant::now();
    app.insert_resource(Run {
        work, assets, original_motion, replacement_size, stage: Stage::OpenDirectory, started: now, stage_started: now,
        click: None, splitter_x: None, tab: None, sample: None, animation_revision: None,
        capture: None, replacement: None,
        replacement_revision: None, steps: Map::new(), screenshots: Map::new(),
    })
    .add_systems(Update, fixture_focus.before(crate::SceneEditorSystemSet::Input))
    .add_systems(Last, drive);
    Ok(())
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        let result = if run.started.elapsed() > Duration::from_secs(110)
            || run.stage_started.elapsed() > Duration::from_secs(23)
        {
            Err(format!("等待 {:?} 超时", run.stage))
        } else {
            advance(world, &mut run)
        };
        let error = match result {
            Ok(Some(Stage::Complete)) => {
                run.stage = Stage::Complete;
                None
            }
            Ok(Some(next)) => {
                run.stage = next;
                run.stage_started = Instant::now();
                return;
            }
            Ok(None) => return,
            Err(message) => Some(message),
        };
        let directory = world.get_resource::<EditorAssetCatalog>().map(|catalog| json!({
            "folder":catalog.current_folder,
            "requested":catalog.requested_revision,
            "applied":catalog.applied_revision,
            "filter":format!("{:?}",catalog.adopted_type_filter),
            "entries":catalog.entries.iter().map(|entry| &entry.asset_path).collect::<Vec<_>>(),
        }));
        let project = world.get_resource::<EditorProjectFileCatalog>().map(|catalog| json!({
            "folder":catalog.current_folder,
            "loading":catalog.is_loading(),
            "filter":format!("{:?}",catalog.adopted_type_filter),
            "entries":catalog.entries.iter().map(|entry| &entry.asset_path).collect::<Vec<_>>(),
        }));
        let navigation = world.get_resource::<SceneEditorSidebarState>()
            .map(|sidebar| format!("{:?}", sidebar.navigator));
        let preview = world.get_resource::<AssetPreviewState>().map(|preview| json!({
            "path":preview.selected_path,
            "model_ready":matches!(preview.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. }),
            "active_hero_tab":active_hero(world),
        }));
        let report = json!({
            "passed":error.is_none(),"error":error,"stage":format!("{:?}",run.stage),
            "pid":std::process::id(),"elapsed_seconds":run.started.elapsed().as_secs_f32(),
            "source":"真实 Feathrise 编辑器 + Bevy WindowEvent",
            "os_input_verified":false,
            "steps":run.steps,"screenshots":run.screenshots,
            "last_asset_directory":directory,"last_project_directory":project,
            "navigator":navigation,"preview":preview,
            "animation_diagnostic":super::animation::rig_animation_diagnostic(world),
        });
        let written = evidence::report(&run.work.join("window.json"), &report);
        if let Err(error) = &written { error!("写入工程目录验收报告失败：{error}"); }
        world.write_message(if error.is_none() && written.is_ok() { AppExit::Success } else { AppExit::error() });
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<Option<Stage>, String> {
    let Some(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next() else {
        return Ok(None);
    };
    let next = match run.stage {
        Stage::OpenDirectory => {
            if !world.get_resource::<SceneEditorState>().is_some_and(|editor| {
                editor.open && editor.pending_scene_transition.is_none()
            }) || !click(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
            }) {
                return Ok(None);
            }
            Stage::FilterSkeleton
        }
        Stage::FilterSkeleton => {
            if !project_paths(world).is_some_and(|paths| paths.contains(&"src".into()) && paths.contains(&"assets".into()))
                && run.click.is_none()
            {
                return Ok(None);
            }
            if !choose_filter(world, run, window, AssetTypeFilter::Skeleton) { return Ok(None); }
            Stage::OpenAssets
        }
        Stage::OpenAssets => {
            let Some(paths) = project_paths(world).filter(|paths| paths == &["assets"]) else { return Ok(None); };
            let row = project_row(world, "assets");
            if !click_entity(world, run, window, row) { return Ok(None); }
            run.steps.insert("skeleton_project_root".into(), json!(paths));
            Stage::OpenRoles
        }
        Stage::OpenRoles => {
            let Some(paths) = asset_paths(world, "").filter(|paths| paths == &["roles"]) else { return Ok(None); };
            let row = asset_row(world, "roles");
            if !click_entity(world, run, window, row) { return Ok(None); }
            run.steps.insert("skeleton_asset_root".into(), json!(paths));
            Stage::OpenNested
        }
        Stage::OpenNested => {
            let Some(paths) = asset_paths(world, "roles").filter(|paths| paths == &["roles/nested"]) else {
                return Ok(None);
            };
            let row = asset_row(world, "roles/nested");
            if !click_entity(world, run, window, row) { return Ok(None); }
            run.steps.insert("skeleton_roles".into(), json!(paths));
            Stage::SkeletonCapture
        }
        Stage::SkeletonCapture => {
            let Some(paths) = asset_paths(world, "roles/nested").filter(|paths| paths == &[HERO]) else { return Ok(None); };
            if asset_row(world, HERO).is_none() || !capture(world, run, "skeleton.png")? {
                return Ok(None);
            }
            run.steps.insert("skeleton_nested".into(), json!(paths));
            Stage::SelectHero
        }
        Stage::SelectHero | Stage::OpenHero => {
            let row = asset_row(world, HERO);
            if !click_entity(world, run, window, row) { return Ok(None); }
            if run.stage == Stage::SelectHero { Stage::OpenHero } else { Stage::HeroReady }
        }
        Stage::HeroReady => {
            let Some(tab) = active_hero(world) else {
                // 真窗口在编译着色器时首组双击可能超过正常双击窗口。仍从原两次点击重试。
                return Ok((run.stage_started.elapsed() > Duration::from_millis(650)).then_some(Stage::SelectHero));
            };
            if !world.get_resource::<AssetPreviewState>().is_some_and(|preview| {
                matches!(preview.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. })
            }) { return Ok(None); }
            run.tab = Some(tab);
            Stage::RestoreDirectory
        }
        Stage::RestoreDirectory => {
            // 原模型标签打开后显示自己的场景树。通过原工程页签返回目录，保留中央角色。
            if !world.resource::<SceneEditorSidebarState>().assets_browser_visible()
                && !click(world, run, window, |tab: &SceneEditorSidebarTabButton| {
                    tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
                })
            {
                return Ok(None);
            }
            Stage::FilterAll
        }
        Stage::FilterAll => {
            if !choose_filter(world, run, window, AssetTypeFilter::All) { return Ok(None); }
            Stage::UpToRoles
        }
        Stage::UpToRoles => {
            let Some(paths) = asset_paths(world, "roles/nested").filter(|paths| paths == &[HERO, STATIC]) else {
                return Ok(None);
            };
            if !click(world, run, window, |button: &SceneEditorActionButton| button.0 == EditorAction::AssetFolderUp) {
                return Ok(None);
            }
            run.steps.insert("all_nested".into(), json!(paths));
            Stage::UpToAssets
        }
        Stage::UpToAssets => {
            let Some(paths) = asset_paths(world, "roles").filter(|paths| paths.contains(&"roles/empty".into())) else {
                return Ok(None);
            };
            if !click(world, run, window, |button: &SceneEditorActionButton| button.0 == EditorAction::AssetFolderUp) {
                return Ok(None);
            }
            run.steps.insert("all_roles".into(), json!(paths));
            Stage::Narrow
        }
        Stage::Narrow => {
            let Some(paths) = asset_paths(world, "").filter(|paths| {
                [MOTIONS, "roles", "empty", "swatch.png", "beep.wav", "sample.avi"]
                    .iter().all(|path| paths.iter().any(|entry| entry == path))
            }) else { return Ok(None); };
            if !narrow(world, run, window) { return Ok(None); }
            run.steps.insert("all_asset_root".into(), json!(paths));
            Stage::OpenNarrowMenu
        }
        Stage::OpenNarrowMenu => {
            if !click(world, run, window, |_: &ProjectTypeFilterButton| true) { return Ok(None); }
            Stage::NarrowCapture
        }
        Stage::NarrowCapture => {
            let Some(layout) = narrow_menu(world, window) else { return Ok(None); };
            if !capture(world, run, "narrow-menu.png")? { return Ok(None); }
            run.steps.insert("narrow_menu".into(), layout);
            Stage::ChooseAnimation
        }
        Stage::ChooseAnimation => {
            if !click(world, run, window, |option: &ProjectTypeFilterOption| option.0 == AssetTypeFilter::Animation) {
                return Ok(None);
            }
            Stage::UseThumbnails
        }
        Stage::UseThumbnails => {
            let Some(paths) = asset_paths(world, "").filter(|paths| paths == &[MOTIONS]) else { return Ok(None); };
            let Some(expander) = animation_expander(world) else { return Ok(None); };
            if world.get::<Node>(expander).is_none_or(|node| node.position_type != PositionType::Relative)
                || !click(world, run, window, |button: &ViewModeButton| button.0 == AssetBrowserViewMode::Thumbnails)
            {
                return Ok(None);
            }
            let revision = world.resource::<EditorAssetCatalog>().applied_revision;
            run.animation_revision = Some(revision);
            run.steps.insert("animation_list".into(), json!({"entries":paths,"revision":revision}));
            Stage::Expand
        }
        Stage::Expand => {
            let Some(paths) = asset_paths(world, "").filter(|paths| paths == &[MOTIONS]) else { return Ok(None); };
            let Some(expander) = animation_expander(world) else { return Ok(None); };
            if world.resource::<EditorSettings>().asset_browser_view_mode != AssetBrowserViewMode::Thumbnails
                || world.get::<Node>(expander).is_none_or(|node| node.position_type != PositionType::Absolute)
            {
                return Ok(None);
            }
            let revision = world.resource::<EditorAssetCatalog>().applied_revision;
            if Some(revision) != run.animation_revision {
                return Err("列表与缩略图切换改变了已采用的动画筛选结果".into());
            }
            if !click_entity(world, run, window, Some(expander)) { return Ok(None); }
            run.steps.insert("animation_thumbnails".into(), json!({"entries":paths,"revision":revision}));
            Stage::ClipMenuCapture
        }
        Stage::ClipMenuCapture => {
            // 展开菜单须真正布局两个原片段行，再记录选择前的窗口画面。
            if visible(world, |clip: &ProjectAnimationClip| clip.clip_index == Some(0)).is_none()
                || visible(world, |clip: &ProjectAnimationClip| clip.clip_index == Some(1)).is_none()
                || !capture(world, run, "clip-menu.png")?
            {
                return Ok(None);
            }
            Stage::ChooseSecond
        }
        Stage::ChooseSecond => {
            if !click(world, run, window, |clip: &ProjectAnimationClip| clip.clip_index == Some(1)) {
                return Ok(None);
            }
            Stage::ObservePlayback
        }
        Stage::ObservePlayback => {
            let Some(proof) = observe_motion(world, run)? else { return Ok(None); };
            run.steps.insert("second_clip".into(), proof);
            Stage::PlaybackCapture
        }
        Stage::PlaybackCapture => {
            if !capture(world, run, "second-clip.png")? { return Ok(None); }
            Stage::Replace
        }
        Stage::Replace => {
            if run.replacement.is_none() {
                run.replacement_revision = Some(world.resource::<EditorAssetCatalog>().applied_revision);
                let (work, assets, original) = (run.work.clone(), run.assets.clone(), run.original_motion.clone());
                run.replacement = Some(IoTaskPool::get().spawn(async move {
                    evidence::replace_clip_file(&work, &assets, &original)
                }));
            }
            let Some(result) = run.replacement.as_mut().and_then(check_ready) else { return Ok(None); };
            result?;
            Stage::AwaitReplacement
        }
        Stage::AwaitReplacement => {
            let catalog = world.resource::<EditorAssetCatalog>();
            let before = run.replacement_revision.unwrap_or(0);
            if catalog.applied_revision <= before || !asset_paths(world, "").is_some_and(|paths| paths.is_empty()) {
                return Ok(None);
            }
            run.steps.insert("replacement".into(), json!({
                "previous_revision":before,"applied_revision":catalog.applied_revision,"entries":[],"manual_refresh":false,
            }));
            Stage::ChooseModel
        }
        Stage::ChooseModel => {
            if !choose_filter(world, run, window, AssetTypeFilter::Model) { return Ok(None); }
            Stage::ReplacementCapture
        }
        Stage::ReplacementCapture => {
            let Some(paths) = asset_paths(world, "").filter(|paths| paths == &["roles", MOTIONS]) else {
                return Ok(None);
            };
            let Some(row) = asset_row(world, MOTIONS) else { return Ok(None); };
            let Some((size, texture)) = validation_visible_thumbnail(world, row, MOTIONS) else { return Ok(None); };
            if size != run.replacement_size || size == run.original_motion.len() as u64 {
                return Err("缩略图就绪后仍指向替换前的源文件".into());
            }
            if !capture(world, run, "replaced.png")? { return Ok(None); }
            run.steps.insert("reclassified_model".into(), json!(paths));
            run.steps.insert("replacement_thumbnail".into(), json!({
                "asset":MOTIONS,"source_bytes":size,"texture_size":texture.to_array(),
            }));
            Stage::Complete
        }
        Stage::Complete => return Ok(None),
    };
    Ok(Some(next))
}

fn project_paths(world: &World) -> Option<Vec<String>> {
    let browser = world.get_resource::<ProjectBrowserState>()?;
    let catalog = world.get_resource::<EditorProjectFileCatalog>()?;
    (browser.view == ProjectBrowserView::ProjectFiles && catalog.current_folder.is_empty()
        && !catalog.is_loading() && catalog.feedback.as_ref().is_some_and(|feedback| feedback.result.is_ok()))
        .then(|| catalog.entries.iter().map(|entry| entry.asset_path.clone()).collect())
}

fn asset_paths(world: &World, folder: &str) -> Option<Vec<String>> {
    let browser = world.get_resource::<ProjectBrowserState>()?;
    let catalog = world.get_resource::<EditorAssetCatalog>()?;
    (browser.assets_visible() && catalog.current_folder == folder && catalog.applied_revision == catalog.requested_revision
        && catalog.feedback.as_ref().is_some_and(|feedback| feedback.result.is_ok()))
        .then(|| catalog.entries.iter().map(|entry| entry.asset_path.clone()).collect())
}

fn project_row(world: &mut World, path: &str) -> Option<Entity> {
    let index = world.resource::<EditorProjectFileCatalog>().entries.iter().position(|entry| entry.asset_path == path)?;
    visible(world, |row: &ProjectFileRow| row.entry_index == Some(index))
}

fn asset_row(world: &mut World, path: &str) -> Option<Entity> {
    let index = world.resource::<EditorAssetCatalog>().entries.iter().position(|entry| entry.asset_path == path)?;
    visible(world, |row: &SceneEditorAssetRow| row.entry_index == Some(index))
}

fn animation_expander(world: &mut World) -> Option<Entity> {
    let index = world.resource::<EditorAssetCatalog>().entries.iter().position(|entry| entry.asset_path == MOTIONS)?;
    visible(world, |expand: &ProjectAnimationExpand| expand.0.is_some_and(|binding| binding.index == Some(index)))
}

fn active_hero(world: &World) -> Option<(u64, usize)> {
    let media = world.get_resource::<AssetPreviewWorkspace>()?;
    let workspace = world.get_resource::<VegetationPresetEditorState>()?;
    let tab = media.expanded.as_ref()?;
    (media.active(workspace.active_tab) && tab.path == HERO).then(|| (tab.id, media.tabs().count()))
}

fn observe_motion(world: &mut World, run: &mut Run) -> Result<Option<Value>, String> {
    let Some((path, index, _, true)) = active_project_clip(world) else { return Ok(None); };
    if path != run.assets.join(MOTIONS) || index != 1 || active_hero(world) != run.tab {
        return Err("目录第二个片段没有在原角色标签中播放".into());
    }
    let Some((tab, host, model, motion, index)) = rig_animation_identity(world) else { return Ok(None); };
    if Some(tab) != run.tab.map(|(id, _)| id) || host != HERO || model != HERO || motion != MOTIONS || index != 1 {
        return Err("工程动作预览请求采用了错误的源、目标或片段".into());
    }
    let Some(pose) = rig_animation_probe(world) else { return Ok(None); };
    if pose.model_path != HERO || pose.clip_index != 1 || pose.clip_name != "抬臂" {
        return Err("已显示角色发布的片段不是第二段抬臂".into());
    }
    let Some((_, rotation)) = pose.joints.iter().find(|(name, _)| name == "mixamorig:LeftArm") else {
        return Err("实际蒙皮模型缺少用于比对的左臂关节".into());
    };
    let Some((time, before)) = run.sample else {
        run.sample = Some((pose.time, *rotation));
        return Ok(None);
    };
    if (pose.time - time).abs() < 0.08 || rotation.dot(before).abs() >= 0.9999 { return Ok(None); }
    let Some(properties) = project_clip_properties(world) else { return Ok(None); };
    let (label, parameters) = (&properties.label, &properties.parameters);
    if !label.contains("2/2") || !label.contains("抬臂") || !parameters.contains("30 fps")
        || !parameters.contains("1×") || !parameters.contains("循环")
    {
        return Err(format!("右侧属性缺少当前动作与参数：{label} / {parameters}"));
    }
    if properties.title_lines != 1 || (properties.title_width - properties.section_width).abs() > 1.0 {
        return Err(format!(
            "属性动作短标题应使用整栏宽度并保持一行，实际 {} 行，标题宽度 {:.1} / {:.1} 像素",
            properties.title_lines, properties.title_width, properties.section_width,
        ));
    }
    Ok(Some(json!({
        "model_tab_id":tab,"host":HERO,"source":MOTIONS,"clip_index":1,"clip_name":pose.clip_name,
        "first_sample_seconds":time,"second_sample_seconds":pose.time,"left_arm_rotation_changed":true,
        "bound_joints":pose.joints.len(),"visible_model_root":format!("{:?}",pose.root),
        "property_label":label,"property_parameters":parameters,
        "property_title_lines":properties.title_lines,
        "property_title_width_px":properties.title_width,"property_section_width_px":properties.section_width,
    })))
}

fn visible<T: Component>(world: &mut World, matches: impl Fn(&T) -> bool) -> Option<Entity> {
    world.query::<(Entity, &T, &Node, &ComputedNode, Option<&InteractionDisabled>, Option<&InheritedVisibility>)>()
        .iter(world).find_map(|(entity, marker, node, layout, disabled, visible)| {
            (matches(marker) && node.display != Display::None && layout.size().min_element() > 1.0
                && disabled.is_none() && visible.is_none_or(|value| value.get())).then_some(entity)
        })
}

fn click<T: Component>(world: &mut World, run: &mut Run, window: Entity, matches: impl Fn(&T) -> bool) -> bool {
    let entity = run.click.map(|(entity, _)| entity).or_else(|| visible(world, matches));
    click_entity(world, run, window, entity)
}

fn click_entity(world: &mut World, run: &mut Run, window: Entity, entity: Option<Entity>) -> bool {
    let target = run.click.map(|(entity, _)| entity).or(entity);
    click_window_entity(world, &mut run.click, window, target)
}

fn mouse(world: &mut World, window: Entity, state: ButtonState) {
    window_event(world, MouseButtonInput { window, button: MouseButton::Left, state });
}

fn choose_filter(world: &mut World, run: &mut Run, window: Entity, target: AssetTypeFilter) -> bool {
    let option = visible(world, |option: &ProjectTypeFilterOption| option.0 == target);
    let opening = run.click.is_some_and(|(entity, _)| world.get::<ProjectTypeFilterButton>(entity).is_some());
    if opening || option.is_none() {
        click(world, run, window, |_: &ProjectTypeFilterButton| true);
        false
    } else {
        click_entity(world, run, window, option)
    }
}

fn narrow(world: &mut World, run: &mut Run, window: Entity) -> bool {
    if let Some(origin) = run.splitter_x {
        let physical_scale = world.get::<Window>(window).unwrap().scale_factor() * world.resource::<UiScale>().0;
        // 真实分隔条的下限由生产布局代码裁剪，验收只提供鼠标移动量。
        move_cursor(world, window, origin - DVec2::new(180.0 * f64::from(physical_scale), 0.0));
        if world.resource::<SceneEditorLayoutState>().content_width > EDITOR_CONTENT_MIN_WIDTH + 1.0 { return false; }
        mouse(world, window, ButtonState::Released);
        run.splitter_x = None;
        return true;
    }
    let Some(entity) = visible(world, |splitter: &SceneEditorPaneSplitter| splitter.0 == EditorPane::Content) else {
        return false;
    };
    let center = world.get::<UiGlobalTransform>(entity).unwrap().affine().translation.as_dvec2();
    move_cursor(world, window, center);
    if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Hovered) {
        mouse(world, window, ButtonState::Pressed);
        run.splitter_x = Some(center);
    }
    false
}

fn narrow_menu(world: &mut World, window: Entity) -> Option<Value> {
    let pane = visible(world, |pane: &SceneEditorPane| pane.0 == EditorPane::Content)?;
    let width = world.get::<ComputedNode>(pane)?.size().x;
    let window = world.get::<Window>(window)?;
    let scale = window.scale_factor() * world.resource::<UiScale>().0;
    let bounds = Vec2::new(window.physical_width() as f32, window.physical_height() as f32);
    if width / scale > EDITOR_CONTENT_MIN_WIDTH + 2.0 { return None; }
    let options = world.query::<(&ProjectTypeFilterOption, &ComputedNode, &UiGlobalTransform, &Node)>()
        .iter(world)
        .filter(|(_, size, _, node)| size.size().min_element() > 1.0 && node.display != Display::None)
        .map(|(option, size, transform, _)| (option.0, Rect::from_center_size(transform.affine().translation, size.size())))
        .collect::<Vec<_>>();
    if options.len() != 9 || options.iter().any(|(_, rect)| {
        rect.min.cmplt(Vec2::ZERO).any() || rect.max.cmpgt(bounds + Vec2::ONE).any()
    }) { return None; }
    Some(json!({
        "sidebar_logical_width":width / scale, "window_physical_size":bounds.to_array(),
        "options":options.iter().map(|(filter, rect)| json!({
            "filter":format!("{filter:?}"),"min":rect.min.to_array(),"max":rect.max.to_array()
        })).collect::<Vec<_>>(),
    }))
}

fn capture(world: &mut World, run: &mut Run, filename: &str) -> Result<bool, String> {
    if let Some(proof) = evidence::capture(world, &mut run.capture, &run.work.join(filename))? {
        run.screenshots.insert(filename.into(), proof);
        return Ok(true);
    }
    Ok(false)
}
