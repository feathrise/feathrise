//! 原生窗口验收场景树换选与来源定位，沿用原保存和截图。

use super::{open_scene_tree, ui_visible};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand},
    components::SceneEditorRightPanelScroll,
    resources::{SceneEditorState, SceneEditorViewportState},
    ui::{
        external_files::platform_path,
        file_manager::{FileManagerState, FileRow},
        file_manager_validation::{pointer_click, right_click},
        gltf_browser::{EditorGltfBrowserEntryKind, EditorGltfBrowserState, PlacementTreeExpander, SceneMeshObjectsHeading},
        scene_context_menu::{SceneMenuAction, SceneObjectKey, SceneObjectRow, press_action_for_validation},
        scene_source_glb::source_position_field_for_validation,
    },
    viewport::modes::{self, ViewportMode, ViewportModeState},
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
    window::PrimaryWindow,
};
use feathrise_scene_content::SceneId;
use feathrise_editor_ui::{EditorButtonClick, NumericInputCommitOutcome, NumericInputCommitSource, NumericInputCommitted, NumericInputValue, NumericValue};
use serde_json::{Value, json};
use std::{fs, path::{Path, PathBuf}};

const MODEL: &str = "sample-model";
const ASSET_PATH: &str = "scene/probe.glb";
const MESH: &str = "ProjectAssetQuad";
const GROUP: &str = "验收组";
const PRIVATE_TERRAIN: &str = "starter-ground";
const PRIVATE_LAYER: &str = "scene-tree-private-grass";
const GLB_ICON: &str = "icons/editor/tree_glb.png";
const SOURCE_SPACES: [&str; 3] = ["源 GLB", "当前场景", "世界"];
const SOURCE_X_VALUES: [f32; 3] = [1.75, 6.75, 6.75];
const WORLD_X_EDIT: f32 = 7.5;
const COORDINATE_SCREENSHOT: &str = "scene-tree-glb-coordinates.png";

#[derive(Resource)]
struct StartupScreenshot(Task<Result<(), String>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Open,
    ExpandMain(u8),
    StartupTreeReady,
    StartupScreenshot,
    SelectCube,
    Move,
    SelectRoot { press: u8, frame: u8, before: [u32; 4] },
    RootFocused([u32; 4]),
    ExpandRoot(u8),
    SelectMesh,
    Verify,
    SourceSpace { index: usize, frame: u8 },
    VerifySourceSpace(usize),
    SourceSpaceScreenshot,
    SourceSpaceEdited,
    GroupName { press: u8, frame: u8, before: [u32; 4] },
    GroupFocused { frame: u8, before: [u32; 4] },
    RestoreMesh,
    OpenMenu(u8),
    ChooseLocate,
    Locate,
}

pub(super) fn enabled() -> bool {
    std::env::var("FEATHRISE_SCENE_TREE_VALIDATION").as_deref() == Ok("1")
}

fn startup_enabled() -> bool {
    std::env::var("FEATHRISE_SCENE_TREE_STARTUP_VALIDATION").as_deref() == Ok("1")
}

pub(super) fn preflight(project: &Path, scene: &SceneId) -> Result<(), String> {
    let output = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("场景树验收缺少输出目录")?);
    let work = output.parent().ok_or("场景树验收缺少独占工作目录")?;
    let owner: Value = serde_json::from_slice(&fs::read(work.join(".feathrise-native-validation-owner.json"))
        .map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    if owner["schema"] != 1 || owner["purpose"] != "ordinary-author-boundary-validation"
        || owner["work_root"] != json!(work) || project != work.join("独立工程 A")
        || scene.as_str() != "M1-SAMPLE"
    {
        return Err("场景树验收只接受本轮独占工程 A 的中性样例".into());
    }
    Ok(())
}

pub(super) fn reopened_source_position(world: &mut World) -> Option<Value> {
    use feathrise_editor_animation_model::types::EmbeddedNodeIndex;
    use feathrise_scene::ScenePlacementInstance;

    let paths = feathrise_editor_core::editor_project_paths()?;
    let scene = SceneId::parse("M1-SAMPLE").ok()?;
    preflight(paths.project_root(), &scene).ok()?;
    let root = world.query::<(Entity, &ScenePlacementInstance)>().iter(world).find_map(|(entity, instance)| {
        (instance.scene_id() == &scene && instance.id() == MODEL && instance.asset_path() == ASSET_PATH
            && instance.is_active()).then_some(entity)
    })?;
    let mut children = world.query::<&Children>();
    // 针对新进程实际采用的资产实例取数，避免用磁盘回读结果冒充重开后的世界坐标。
    children.query(world).iter_descendants(root).find_map(|entity| {
        if world.get::<EmbeddedNodeIndex>(entity)?.0 != 0 { return None; }
        Some(json!({"placement": MODEL, "local_x": world.get::<Transform>(entity)?.translation.x,
            "world_x": world.get::<GlobalTransform>(entity)?.translation().x}))
    })
}

pub(super) fn advance(
    world: &mut World,
    step: &mut Step,
    cube: &str,
    revision: &mut u64,
    pulse: &mut Option<Entity>,
) -> Result<Option<Value>, String> {
    let session = world.resource::<EditorAuthoringSession>();
    if session.revision != *revision && *step != Step::SourceSpaceEdited {
        return Err("切换视口工具和树行选择不应修改场景修订".into());
    }
    let selected = world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref();
    let mode = world.resource::<ViewportModeState>().mode;
    let transform_enabled = world.resource::<SceneEditorState>().placement_transform_enabled;
    match *step {
        Step::Open => {
            if open_scene_tree(world)? {
                *step = Step::ExpandMain(0);
            }
        }
        Step::ExpandMain(frame) => {
            if frame == 0 && scene_row(world, cube, false).is_some() {
                *step = Step::StartupTreeReady;
            } else if let Some(row) = main_scene_row(world) {
                // 内置立方体现由主场景 GLB 收纳，先操作真实箭头才能验证它的图标和选择。
                let arrow = row_arrow(world, row)?;
                *step = click_control(world, arrow, frame)?.map_or(Step::StartupTreeReady, Step::ExpandMain);
            }
        }
        Step::StartupTreeReady => {
            if !startup_enabled() {
                *step = Step::SelectCube;
            } else if startup_tree_visible(world, cube)? {
                request_screenshot(world, "scene-tree-startup.png")?;
                *step = Step::StartupScreenshot;
            }
        }
        Step::StartupScreenshot => {
            if let Some(result) = world.get_resource_mut::<StartupScreenshot>()
                .and_then(|mut capture| check_ready(&mut capture.0))
            {
                result?;
                world.remove_resource::<StartupScreenshot>();
                *step = Step::SelectCube;
            }
        }
        Step::SelectCube => {
            if let Some(row) = click_row(world, cube, false) {
                *pulse = Some(row);
                *step = Step::Move;
            }
        }
        Step::Move if selected == Some(cube) => {
            *pulse = Some(modes::validation_press_mode(world, ViewportMode::Move)?);
            *step = Step::SelectRoot { press: 0, frame: 0, before: camera_pose(world) };
        }
        Step::SelectRoot { press, frame, before } if mode == ViewportMode::Move && transform_enabled => {
            if press == 1 && frame == 0
                && (selected != Some(MODEL) || camera_pose(world) != before || !branch_collapsed(world, MODEL)?)
            {
                return Err("首次点击 GLB 名称应只选中，保留相机和折叠状态".into());
            }
            if let Some(row) = scene_row(world, MODEL, false) {
                *step = match click_control(world, row, frame)? {
                    Some(frame) => Step::SelectRoot { press, frame, before },
                    None if press == 0 => Step::SelectRoot { press: 1, frame: 0, before },
                    None => Step::RootFocused(before),
                };
            }
        }
        Step::RootFocused(before) if camera_pose(world) != before => {
            if selected != Some(MODEL) || !branch_collapsed(world, MODEL)? {
                return Err("双击 GLB 名称取景应保留原选择和折叠状态".into());
            }
            *step = Step::ExpandRoot(0);
        }
        Step::ExpandRoot(frame) if selected == Some(MODEL) => {
            if mode != ViewportMode::Move || !transform_enabled {
                return Err("场景树根行选择改变了移动工具".into());
            }
            if frame == 0 && !branch_collapsed(world, MODEL)? {
                return Err("GLB 必须等待独立箭头展开".into());
            }
            if properties_show_path(world) && let Some(row) = scene_row(world, MODEL, false) {
                let arrow = row_arrow(world, row)?;
                *step = click_control(world, arrow, frame)?.map_or(Step::SelectMesh, Step::ExpandRoot);
            }
        }
        Step::SelectMesh => {
            if selected != Some(MODEL) {
                return Err("点击 GLB 展开箭头改变了名称选择".into());
            }
            if let Some(row) = click_row(world, MODEL, true) {
                *pulse = Some(row);
                *step = Step::Verify;
            }
        }
        Step::Verify => {
            if mode != ViewportMode::Move || !transform_enabled {
                return Err("场景树网格选择改变了移动工具".into());
            }
            let browser = world.resource::<EditorGltfBrowserState>();
            if selected == Some(MODEL) && let Some(mesh) = &browser.selection
                && mesh.valid && mesh.placement_id == MODEL && mesh.mesh_name == MESH
            {
                *step = if startup_enabled() {
                    let Some((_, active)) = source_position_field_for_validation(world) else { return Ok(None); };
                    if active != SOURCE_SPACES[0] { return Err("源网格默认坐标应为源 GLB".into()); }
                    Step::SourceSpace { index: 0, frame: 0 }
                } else {
                    Step::GroupName { press: 0, frame: 0, before: camera_pose(world) }
                };
            }
        }
        Step::SourceSpace { index, frame } => {
            if let Some(button) = source_space_button(world, SOURCE_SPACES[index]) {
                *step = click_control(world, button, frame)?.map_or(Step::VerifySourceSpace(index),
                    |frame| Step::SourceSpace { index, frame });
            }
        }
        Step::VerifySourceSpace(index) => {
            // 顶层场景与世界同值。分别核对真实采用的档位，防止两个按钮误走同一分支也通过。
            if let Some((field, active)) = source_position_field_for_validation(world)
                && active == SOURCE_SPACES[index]
                && world.get::<NumericInputValue>(field).and_then(|value| value.0.as_f32()) == Some(SOURCE_X_VALUES[index])
            {
                if index + 1 < SOURCE_SPACES.len() {
                    *step = Step::SourceSpace { index: index + 1, frame: 0 };
                } else {
                    request_screenshot(world, COORDINATE_SCREENSHOT)?;
                    *step = Step::SourceSpaceScreenshot;
                }
            }
        }
        Step::SourceSpaceScreenshot => {
            if let Some(result) = world.get_resource_mut::<StartupScreenshot>()
                .and_then(|mut capture| check_ready(&mut capture.0))
            {
                result?;
                world.remove_resource::<StartupScreenshot>();
                let (entity, label) = source_position_field_for_validation(world).ok_or("真实源网格 X 字段已失效")?;
                if label != "世界" { return Err("世界坐标字段未采用".into()); }
                // 复用实际数字框的正式提交协议，源编辑和随后场景保存仍由原系统执行。
                world.write_message(NumericInputCommitted { entity, value: NumericValue::F32(WORLD_X_EDIT),
                    outcome: NumericInputCommitOutcome::Accepted, source: NumericInputCommitSource::Text,
                    changed: true, edited: true });
                *step = Step::SourceSpaceEdited;
            }
        }
        Step::SourceSpaceEdited => {
            if session.revision != *revision {
                if session.revision != revision.wrapping_add(1)
                    || !matches!(session.undo.last(), Some(EditorCommand::SourceGlbsChanged { .. }))
                {
                    return Err("一次世界 X 提交应形成一笔源 GLB 编辑".into());
                }
                *revision = session.revision;
                *step = Step::GroupName { press: 0, frame: 0, before: camera_pose(world) };
            }
        }
        Step::GroupName { press, frame, before } => {
            if !branch_collapsed(world, GROUP)? || branch_collapsed(world, MODEL)? {
                return Err("选择组名应保留 GLB 与组原有的展开状态".into());
            }
            if press == 1 && frame == 0
                && (world.resource::<EditorGltfBrowserState>().selected_group.as_deref() != Some(GROUP)
                    || camera_pose(world) != before)
            {
                return Err("首次点击组名应只选组并保留相机".into());
            }
            let Some(row) = group_row(world) else { return Ok(None); };
            // 实际按松同一组行两次，形成生产双击并通过原菜单请求取景。
            *step = match click_control(world, row, frame)? {
                Some(frame) => Step::GroupName { press, frame, before },
                None if press == 0 => Step::GroupName { press: 1, frame: 0, before },
                None => Step::GroupFocused { frame: 0, before },
            };
        }
        Step::GroupFocused { frame, before } => {
            let viewport = world.resource::<SceneEditorViewportState>();
            // 两成员位于 X=24 和 X=36；命中中间区域证明使用了合并范围。
            if (27.0..33.0).contains(&viewport.center.x) && camera_pose(world) != before {
                if frame == 0 && (!branch_collapsed(world, GROUP)? || branch_collapsed(world, MODEL)?) {
                    return Err("双击组名定位不应改变两支的展开状态".into());
                }
                if let Some(row) = group_row(world) {
                    let arrow = row_arrow(world, row)?;
                    *step = click_control(world, arrow, frame)?
                        .map_or(Step::RestoreMesh, |frame| Step::GroupFocused { frame, before });
                }
            }
        }
        Step::RestoreMesh => {
            if world.resource::<EditorGltfBrowserState>().selected_group.as_deref() != Some(GROUP) {
                return Err("点击组箭头改变了选中的组".into());
            }
            if !branch_collapsed(world, GROUP)? && let Some(row) = click_row(world, MODEL, true) {
                *pulse = Some(row);
                *step = Step::OpenMenu(0);
            }
        }
        Step::OpenMenu(frame) => {
            let Some(row) = scene_row(world, MODEL, true) else { return Ok(None); };
            let position = world.get::<UiGlobalTransform>(row).ok_or("网格树行尚未布局")?.translation.as_dvec2();
            let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world)
                .map_err(|_| "没有场景树验收窗口")?;
            let mut pending = Some((position, frame));
            // 经原窗口事件右击子网格，原菜单仍须确认冻结身份和实际悬停。
            *step = if right_click(world, &mut pending, window, position) {
                Step::ChooseLocate
            } else {
                Step::OpenMenu(pending.unwrap().1)
            };
        }
        Step::ChooseLocate => {
            if let Some(button) = press_action_for_validation(world, SceneMenuAction::RevealGlb) {
                *pulse = Some(button);
                *step = Step::Locate;
            }
        }
        Step::Locate => {
            if mode != ViewportMode::Move || !transform_enabled || selected != Some(MODEL) {
                return Err("定位 GLB 改变了场景选择或移动工具".into());
            }
            if file_window_shows_source(world)? {
                if branch_collapsed(world, GROUP)? || branch_collapsed(world, MODEL)? {
                    return Err("切回网格和定位 GLB 文件应保留展开的分支".into());
                }
                return Ok(Some(json!({
                    "viewport_mode": format!("{mode:?}"),
                    "previous_placement": cube,
                    "selected_placement": MODEL,
                    "selected_mesh": MESH,
                    "asset_path_shown_in_properties": ASSET_PATH,
                    "file_window_selected": ASSET_PATH,
                    "scene_revision": *revision,
                    "selected_names_without_expansion": true,
                    "model_double_click_focused_while_folded": true,
                    "explicit_arrows_preserved_selection": true,
                    "group_focused_center": world.resource::<SceneEditorViewportState>().center.to_array(),
                    "startup": startup_enabled().then(|| json!({
                        "vegetation_layer": PRIVATE_LAYER,
                        "terrain": PRIVATE_TERRAIN,
                        "terrain_canvas_unloaded": true,
                        "mesh_section_below_vegetation": true,
                        "tree_glb_icon": GLB_ICON,
                        "file_glb_icon": GLB_ICON,
                        "screenshot": "scene-tree-startup.png",
                        "source_coordinates": {
                            "labels": SOURCE_SPACES,
                            "x_meters": SOURCE_X_VALUES,
                            "switch_preserved_author_revision": true,
                            "edited_world_x": WORLD_X_EDIT,
                            "screenshot": COORDINATE_SCREENSHOT,
                        },
                    })),
                })));
            }
        }
        _ => {}
    }
    Ok(None)
}

fn startup_tree_visible(world: &mut World, cube: &str) -> Result<bool, String> {
    let draft = world.resource::<EditorAuthoringSession>().draft.as_ref().ok_or("首屏验收缺少作者草稿")?;
    if draft.terrain_documents.active.is_some() {
        return Err("首屏植被目录依赖已载入的地形编辑画布".into());
    }
    let Some(plant) = world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        matches!(&target.key, SceneObjectKey::Vegetation { terrain: Some(terrain), id, output: false, cold: Some(_), .. }
            if terrain.as_str() == PRIVATE_TERRAIN && id == PRIVATE_LAYER && ui_visible(world, entity))
            .then_some(entity)
    }) else { return Ok(false); };
    let Some(heading) = world.query_filtered::<Entity, With<SceneMeshObjectsHeading>>().iter(world)
        .find(|entity| ui_visible(world, *entity)) else { return Ok(false); };
    let Some(model) = scene_row(world, MODEL, false) else { return Ok(false); };
    let Some(cuboid) = scene_row(world, cube, false) else { return Ok(false); };
    let y = |entity| world.get::<UiGlobalTransform>(entity).unwrap().translation.y;
    Ok(y(plant) < y(heading) && y(heading) < y(model)
        && row_has_icon(world, model, GLB_ICON) && row_has_icon(world, cuboid, "icons/editor/tree_mesh.png"))
}

fn request_screenshot(world: &mut World, name: &str) -> Result<(), String> {
    let output = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("首屏截图缺少独占目录")?);
    let path = output.join(name);
    // 当前作者阶段保持不变，等实际主窗口完成渲染并写证据再推进。
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>, mut commands: Commands| {
        let image = event.image.clone();
        let path = path.clone();
        commands.insert_resource(StartupScreenshot(IoTaskPool::get().spawn(async move {
            image.try_into_dynamic().map_err(|error| error.to_string())?
                .save(path).map_err(|error| error.to_string())
        })));
    });
    Ok(())
}

fn source_space_button(world: &mut World, label: &str) -> Option<Entity> {
    world.query::<(Entity, &Text)>().iter(world).filter(|(_, text)| text.0 == label)
        .find_map(|(entity, _)| {
            std::iter::successors(Some(entity), |entity| world.get::<ChildOf>(*entity).map(ChildOf::parent))
                .find(|entity| world.get::<EditorButtonClick>(*entity).is_some()
                    && world.get::<bevy::ui::InteractionDisabled>(*entity).is_none()
                    && ui_visible(world, *entity))
        })
}

fn row_has_icon(world: &World, row: Entity, path: &str) -> bool {
    let children = |entity| world.get::<Children>(entity).into_iter().flat_map(|children| children.iter());
    children(row).chain(children(row).flat_map(children)).any(|entity| {
        world.get::<ImageNode>(entity).is_some_and(|icon| {
            icon.image.path().is_some_and(|source| source.path() == Path::new(path))
                && world.resource::<Assets<Image>>().get(&icon.image).is_some()
        })
    })
}

fn click_row(world: &mut World, placement: &str, child: bool) -> Option<Entity> {
    let row = scene_row(world, placement, child)?;
    world.entity_mut(row).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Some(row)
}

fn branch_collapsed(world: &World, branch: &str) -> Result<bool, String> {
    world.resource::<EditorGltfBrowserState>().entries.iter().find_map(|row| match &row.kind {
        EditorGltfBrowserEntryKind::Group { name, collapsed, .. } if name == branch => Some(*collapsed),
        EditorGltfBrowserEntryKind::Placement { placement_id, collapsed, .. } if placement_id == branch => Some(*collapsed),
        _ => None,
    }).ok_or_else(|| "场景树验收缺少已发布的 GLB 或组分支".into())
}

fn group_row(world: &mut World) -> Option<Entity> {
    world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        (matches!(target.key, SceneObjectKey::Group { .. }) && target.name == GROUP && ui_visible(world, entity))
            .then_some(entity)
    })
}

fn row_arrow(world: &World, row: Entity) -> Result<Entity, String> {
    world.get::<Children>(row).and_then(|children| children.iter()
        .find(|child| world.get::<PlacementTreeExpander>(*child).is_some()))
        .ok_or_else(|| "场景树验收没有实际的展开按钮".into())
}

fn click_control(world: &mut World, entity: Entity, frame: u8) -> Result<Option<u8>, String> {
    let position = world.get::<UiGlobalTransform>(entity).ok_or("树控件尚未布局")?.translation.as_dvec2();
    let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world)
        .map_err(|_| "没有场景树验收窗口")?;
    let mut pending = Some((position, frame));
    pointer_click(world, &mut pending, window, position, MouseButton::Left);
    Ok(pending.map(|(_, next)| next))
}

fn camera_pose(world: &World) -> [u32; 4] {
    let viewport = world.resource::<SceneEditorViewportState>();
    let [x, y, z] = viewport.center.to_array();
    [x, y, z, viewport.extent].map(f32::to_bits)
}

fn scene_row(world: &mut World, placement: &str, child: bool) -> Option<Entity> {
    world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        matches!(&target.key, SceneObjectKey::Placement { id, mesh, .. }
            if id == placement && mesh.is_some() == child && ui_visible(world, entity)
                && mesh.as_ref().is_none_or(|mesh| mesh.mesh_name == MESH)).then_some(entity)
    })
}

fn main_scene_row(world: &mut World) -> Option<Entity> {
    world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        (matches!(row.0.as_ref()?.key, SceneObjectKey::MainSceneGlb) && ui_visible(world, entity)).then_some(entity)
    })
}

fn file_window_shows_source(world: &mut World) -> Result<bool, String> {
    let paths = feathrise_editor_core::editor_project_paths().ok_or("场景树验收缺少当前资产根")?;
    let path = platform_path(paths.asset_root()).join(ASSET_PATH);
    let files = world.resource::<FileManagerState>();
    let Some(window) = files.windows.iter().find(|window| {
        files.focused == Some(window.id) && !window.minimized && !window.loading
            && Some(window.path.as_path()) == path.parent()
            && window.selection.len() == 1 && window.selection.contains(&path)
    }) else { return Ok(false); };
    let window = window.id;
    Ok(world.query::<(Entity, &FileRow)>().iter(world).any(|(entity, row)| {
        row.window == window && row.path.as_deref() == Some(path.as_path()) && ui_visible(world, entity)
            && (!startup_enabled() || row_has_icon(world, entity, GLB_ICON))
    }))
}

fn properties_show_path(world: &mut World) -> bool {
    world.query::<(Entity, &Text)>().iter(world).any(|(entity, text)| {
        text.0 == ASSET_PATH && ui_visible(world, entity)
            && std::iter::successors(Some(entity), |entity| world.get::<ChildOf>(*entity).map(ChildOf::parent))
                .any(|ancestor| world.get::<SceneEditorRightPanelScroll>(ancestor).is_some())
    })
}
