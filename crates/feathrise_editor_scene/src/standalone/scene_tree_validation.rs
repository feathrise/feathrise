//! 原生窗口验收场景树换选与来源定位，沿用原保存和截图。

use super::{open_scene_tree, ui_visible};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand},
    components::{SceneBrowserDragPreview, SceneBrowserDragPreviewText, SceneEditorRightPanelScroll},
    resources::{SceneEditorState, SceneEditorViewportState},
    ui::{
        external_files::platform_path,
        file_manager::{FileManagerState, FileRow},
        file_manager_validation::{mouse, move_cursor, pointer_click, right_click},
        gltf_browser::{EditorGltfBrowserEntryKind, EditorGltfBrowserState, PlacementTreeDrag, PlacementTreeExpander, SceneBaseGltfRow, SceneTreeRootRow},
        scene_context_menu::{SceneMenuAction, SceneObjectKey, SceneObjectRow, press_action_for_validation,
            visible_action_for_validation, visible_rename_controls_for_validation},
        scene_objects,
        scene_source_glb::{source_name_input_for_validation, source_position_field_for_validation},
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
const TERRAIN_ICON: &str = "icons/editor/tree_terrain.png";
const SOURCE_SPACES: [&str; 3] = ["源 GLB", "当前场景", "世界"];
const SOURCE_X_VALUES: [f32; 3] = [1.75, 6.75, 6.75];
const WORLD_X_EDIT: f32 = 7.5;
const COORDINATE_SCREENSHOT: &str = "scene-tree-glb-coordinates.png";
const CUBE_PROPERTIES_SCREENSHOT: &str = "scene-tree-cube-properties.png";
const DRAG_PREVIEW_SCREENSHOT: &str = "scene-tree-drag-preview.png";
const RENAMED_NODE: &str = "验收网格";
const RENAMED_FOLDER: &str = "验收文件夹";
const FOLDER_PARENT: &str = "验收目标";
const CREATED_CHILD: &str = "验收子文件夹";
const NESTED_FOLDER_SCREENSHOT: &str = "scene-tree-nested-folder.png";

#[derive(Resource)]
struct StartupScreenshot(Task<Result<(), String>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Open,
    StartupTreeReady,
    StartupScreenshot,
    SelectCube,
    CubeProperties,
    CubePropertiesScreenshot,
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
    OpenMeshApply(u8),
    ApplyMeshScale(u8),
    MeshScaleApplied,
    RenameMesh(u8),
    MeshRenamed,
    DragPreview(u8),
    DragScreenshot,
    DragFinished,
    GroupName { press: u8, frame: u8, before: [u32; 4] },
    GroupFocused { frame: u8, before: [u32; 4] },
    RenameFolder(u8),
    FolderRenamed,
    ChildFolderMenu(u8),
    ChildFolderAction(u8),
    ChildFolderName(u8),
    ChildFolderCreated,
    DragFolder { drop: FolderDrop, frame: u8 },
    FolderDropped(FolderDrop),
    NestedFolderScreenshot,
    RestoreMesh,
    OpenMenu(u8),
    ChooseLocate,
    Locate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FolderDrop { IntoParent, ToRoot, SaveInsideParent }

impl FolderDrop {
    fn parent(self) -> Option<&'static str> {
        (self != Self::ToRoot).then_some(FOLDER_PARENT)
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::IntoParent => Some(Self::ToRoot),
            Self::ToRoot => Some(Self::SaveInsideParent),
            Self::SaveInsideParent => None,
        }
    }
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
    let folders = &world.resource::<EditorAuthoringSession>().draft.as_ref()?.object_folders;
    let folder_parent = folders.parent(RENAMED_FOLDER)?.to_owned();
    let child_parent = folders.parent(CREATED_CHILD)?.to_owned();
    let folder_members = world.resource::<EditorGltfBrowserState>().entries.iter().find_map(|entry| {
        match &entry.kind {
            EditorGltfBrowserEntryKind::Group { name, count, .. } if name == FOLDER_PARENT && entry.depth == 0 => Some(*count),
            _ => None,
        }
    })?;
    let root = world.query::<(Entity, &ScenePlacementInstance)>().iter(world).find_map(|(entity, instance)| {
        (instance.scene_id() == &scene && instance.id() == MODEL && instance.asset_path() == ASSET_PATH
            && instance.is_active()).then_some(entity)
    })?;
    let mut children = world.query::<&Children>();
    // 针对新进程实际采用的资产实例取数，避免用磁盘回读结果冒充重开后的世界坐标。
    children.query(world).iter_descendants(root).find_map(|entity| {
        if world.get::<EmbeddedNodeIndex>(entity)?.0 != 0 { return None; }
        Some(json!({"placement": MODEL, "local_x": world.get::<Transform>(entity)?.translation.x,
            "world_x": world.get::<GlobalTransform>(entity)?.translation().x,
            "reopened_folder_parent": folder_parent, "reopened_folder_members": folder_members,
            "reopened_created_child_parent": child_parent}))
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
    if session.revision != *revision && !matches!(step, Step::SourceSpaceEdited | Step::MeshScaleApplied | Step::MeshRenamed
        | Step::FolderRenamed | Step::ChildFolderCreated | Step::FolderDropped(_))
    {
        return Err(format!("切换视口工具和树行选择不应修改场景修订：{step:?}，{} → {}", *revision, session.revision));
    }
    let selected = world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref();
    let mode = world.resource::<ViewportModeState>().mode;
    let transform_enabled = world.resource::<SceneEditorState>().placement_transform_enabled;
    match *step {
        Step::Open => {
            if open_scene_tree(world)? {
                *step = Step::StartupTreeReady;
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
                *step = if startup_enabled() { Step::CubeProperties } else { Step::Move };
            }
        }
        Step::CubeProperties if selected == Some(cube) => {
            if source_space_button(world, "应用").is_some() {
                request_screenshot(world, CUBE_PROPERTIES_SCREENSHOT)?;
                *step = Step::CubePropertiesScreenshot;
            }
        }
        Step::CubePropertiesScreenshot => {
            if let Some(result) = world.get_resource_mut::<StartupScreenshot>().and_then(|mut capture| check_ready(&mut capture.0)) {
                result?;
                world.remove_resource::<StartupScreenshot>();
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
                *step = Step::OpenMeshApply(0);
            }
        }
        Step::OpenMeshApply(frame) => {
            if let Some(button) = source_space_button(world, "应用") {
                *step = click_control(world, button, frame)?.map_or(Step::ApplyMeshScale(0), Step::OpenMeshApply);
            }
        }
        Step::ApplyMeshScale(frame) => {
            if let Some(button) = source_space_button(world, "缩放") {
                // 原点击在松手后的更新立即收起菜单，从释放请求起就等待业务回执。
                *step = click_control(world, button, frame)?.filter(|frame| *frame < 3)
                    .map_or(Step::MeshScaleApplied, Step::ApplyMeshScale);
            }
        }
        Step::MeshScaleApplied | Step::MeshRenamed if session.revision != *revision => {
            if session.revision != revision.wrapping_add(1)
                || !matches!(session.undo.last(), Some(EditorCommand::SourceGlbsChanged { .. }))
            { return Err("应用和改名各应形成一笔源 GLB 编辑".into()); }
            *revision = session.revision;
            *step = if *step == Step::MeshScaleApplied { Step::RenameMesh(0) } else { Step::DragPreview(0) };
        }
        Step::RenameMesh(frame) => {
            if let Some(input) = source_name_input_for_validation(world)
                && press_rename(world, input, RENAMED_NODE, frame)?
            {
                *step = if frame == 2 { Step::MeshRenamed } else { Step::RenameMesh(frame + 1) };
            }
        }
        Step::DragPreview(frame) => {
            let Some(source) = scene_row(world, MODEL, false) else { return Ok(None); };
            let Some(target) = folder_row(world, GROUP) else { return Ok(None); };
            let point = if frame < 2 { source } else { target };
            let (window, position) = pointer_context(world, point)?;
            move_cursor(world, window, position);
            if frame < 3 {
                if frame == 1 {
                    if world.get::<feathrise_editor_ui::UiInteraction>(source) != Some(&feathrise_editor_ui::UiInteraction::Hovered) {
                        return Ok(None); // 窗口鼠标事件可能晚一帧到达，收到原生悬停后才按下。
                    }
                    mouse(world, window, bevy::input::ButtonState::Pressed);
                }
                *step = Step::DragPreview(frame + 1);
            } else {
                let (fill, edge) = crate::ui::scene_sidebar::tree_row_colors(false, true, feathrise_editor_ui::UiInteraction::None);
                if world.get::<BackgroundColor>(target) != Some(&BackgroundColor(fill))
                    || world.get::<BorderColor>(target) != Some(&edge)
                    || world.get::<Node>(target).map(|node| node.border) != Some(UiRect::all(px(1)))
                    || !drag_preview_visible(world)
                    || !world.query_filtered::<&Text, With<SceneBrowserDragPreviewText>>().iter(world)
                        .any(|text| text.0.contains(MODEL))
                {
                    return Ok(None); // 持续悬停，等待真实落点样式与浮层采用同一帧的指针输入。
                }
                request_screenshot(world, DRAG_PREVIEW_SCREENSHOT)?;
                *step = Step::DragScreenshot;
            }
        }
        Step::DragScreenshot => {
            if let Some(result) = world.get_resource_mut::<StartupScreenshot>().and_then(|mut task| check_ready(&mut task.0)) {
                result?;
                world.remove_resource::<StartupScreenshot>();
                let source = scene_row(world, MODEL, false).ok_or("拖动源已丢失")?;
                let (window, position) = pointer_context(world, source)?;
                // 在源行释放，验证无效落点撤回浮层且不产生摆放修改。
                move_cursor(world, window, position);
                mouse(world, window, bevy::input::ButtonState::Released);
                *step = Step::DragFinished;
            }
        }
        Step::DragFinished => {
            if drag_preview_visible(world) {
                return Err("松手后拖动浮层应当收起".into());
            }
            *step = Step::GroupName { press: 0, frame: 0, before: camera_pose(world) };
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
            let Some(row) = folder_row(world, GROUP) else { return Ok(None); };
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
                if let Some(row) = folder_row(world, GROUP) {
                    let arrow = row_arrow(world, row)?;
                    let next = if startup_enabled() { Step::RenameFolder(0) } else { Step::RestoreMesh };
                    *step = click_control(world, arrow, frame)?
                        .map_or(next, |frame| Step::GroupFocused { frame, before });
                }
            }
        }
        Step::RenameFolder(frame) => {
            if let Some(input) = scene_objects::name_input_for_validation(world)
                && press_rename(world, input, RENAMED_FOLDER, frame)?
            {
                *step = if frame == 2 { Step::FolderRenamed } else { Step::RenameFolder(frame + 1) };
            }
        }
        Step::FolderRenamed if session.revision != *revision => {
            if session.revision != revision.wrapping_add(1)
                || !matches!(session.undo.last(), Some(EditorCommand::SceneObjectsChanged(_)))
            { return Err("文件夹改名应形成一笔场景对象历史".into()); }
            *revision = session.revision;
            *step = Step::ChildFolderMenu(0);
        }
        Step::ChildFolderMenu(frame) => {
            let Some(row) = folder_row(world, RENAMED_FOLDER) else { return Ok(None); };
            let (window, position) = pointer_context(world, row)?;
            let mut pending = Some((position, frame));
            *step = if right_click(world, &mut pending, window, position) {
                Step::ChildFolderAction(0)
            } else { Step::ChildFolderMenu(pending.unwrap().1) };
        }
        Step::ChildFolderAction(frame) => {
            if let Some(button) = visible_action_for_validation(world, SceneMenuAction::NewPlacementFolder)
                && let Some(next) = click_control(world, button, frame)?
            {
                // 菜单项在松开后换成名称框，下一阶段等待新的控件出现。
                *step = if frame == 2 { Step::ChildFolderName(0) } else { Step::ChildFolderAction(next) };
            }
        }
        Step::ChildFolderName(frame) => {
            if let Some((input, button)) = visible_rename_controls_for_validation(world) {
                if frame == 0 { set_test_name(world, input, CREATED_CHILD)?; }
                if let Some(next) = click_control(world, button, frame)? {
                    *step = if frame == 2 { Step::ChildFolderCreated } else { Step::ChildFolderName(next) };
                }
            }
        }
        Step::ChildFolderCreated if session.revision != *revision => {
            if session.revision != revision.wrapping_add(1)
                || !matches!(session.undo.last(), Some(EditorCommand::PlacementFolderMoved(_)))
                || session.draft.as_ref().and_then(|draft| draft.object_folders.parent(CREATED_CHILD)) != Some(RENAMED_FOLDER)
            { return Err("右键新建的子文件夹应随所在文件夹写入一笔历史".into()); }
            if folder_row(world, CREATED_CHILD).is_some() {
                *revision = world.resource::<EditorAuthoringSession>().revision;
                *step = Step::DragFolder { drop: FolderDrop::IntoParent, frame: 0 };
            }
        }
        Step::DragFolder { drop, frame } => {
            let Some(source) = folder_row(world, RENAMED_FOLDER) else { return Ok(None); };
            let target = match drop.parent() {
                Some(name) => folder_row(world, name),
                None => main_scene_row(world),
            };
            let Some(target) = target else { return Ok(None); };
            let (window, position) = pointer_context(world, if frame < 2 { source } else { target })?;
            move_cursor(world, window, position);
            if frame == 1 {
                if world.get::<feathrise_editor_ui::UiInteraction>(source) != Some(&feathrise_editor_ui::UiInteraction::Hovered) {
                    return Ok(None);
                }
                mouse(world, window, bevy::input::ButtonState::Pressed);
            }
            if frame == 3 {
                let (fill, border) = crate::ui::scene_sidebar::tree_row_colors(false, true, feathrise_editor_ui::UiInteraction::None);
                // 普通悬停也会变色，等实际拖放状态采用该落点后再松开。
                if !world.resource::<PlacementTreeDrag>().highlights(target)
                    || !drag_preview_visible(world)
                    || world.get::<BackgroundColor>(target).map(|color| color.0) != Some(fill)
                    || world.get::<BorderColor>(target) != Some(&border)
                {
                    return Ok(None);
                }
                mouse(world, window, bevy::input::ButtonState::Released);
                *step = Step::FolderDropped(drop);
            } else {
                *step = Step::DragFolder { drop, frame: frame + 1 };
            }
        }
        Step::FolderDropped(drop) => {
            let draft = session.draft.as_ref().ok_or("整夹拖放失去场景草稿")?;
            if session.revision != *revision {
                if session.revision != revision.wrapping_add(1)
                    || !matches!(session.undo.last(), Some(EditorCommand::PlacementFolderMoved(_)))
                    || draft.object_folders.parent(RENAMED_FOLDER) != drop.parent()
                { return Err("整夹拖放需要恰好一笔历史和明确的目标父级".into()); }
                *revision = session.revision;
            }
            if draft.object_folders.parent(RENAMED_FOLDER) == drop.parent() && folder_rows_reparented(world, drop) {
                if let Some(next) = drop.next() {
                    *step = Step::DragFolder { drop: next, frame: 0 };
                } else {
                    request_screenshot(world, NESTED_FOLDER_SCREENSHOT)?;
                    *step = Step::NestedFolderScreenshot;
                }
            }
        }
        Step::NestedFolderScreenshot => {
            if let Some(result) = world.get_resource_mut::<StartupScreenshot>().and_then(|mut task| check_ready(&mut task.0)) {
                result?;
                world.remove_resource::<StartupScreenshot>();
                *step = Step::RestoreMesh;
            }
        }
        Step::RestoreMesh => {
            let folder = if startup_enabled() { RENAMED_FOLDER } else { GROUP };
            if world.resource::<EditorGltfBrowserState>().selected_group.as_deref() != Some(folder) {
                return Err("点击组箭头改变了选中的组".into());
            }
            if branch_collapsed(world, folder) == Ok(false) && let Some(row) = click_row(world, MODEL, true) {
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
                let folder = if startup_enabled() { RENAMED_FOLDER } else { GROUP };
                if branch_collapsed(world, folder)? || branch_collapsed(world, MODEL)? {
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
                        "mesh_rows_below_vegetation": true,
                        "embedded_mesh_icon_aligned": true,
                        "terrain_icon": TERRAIN_ICON,
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
                        "cube_properties_screenshot": CUBE_PROPERTIES_SCREENSHOT,
                        "source_name": RENAMED_NODE,
                        "folder_name": RENAMED_FOLDER,
                        "created_child_folder": CREATED_CHILD,
                        "drag_preview_screenshot": DRAG_PREVIEW_SCREENSHOT,
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
    let Some(terrain) = world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        matches!(&row.0.as_ref()?.key, SceneObjectKey::Terrain { id, .. }
            if id.as_str() == PRIVATE_TERRAIN && ui_visible(world, entity)).then_some(entity)
    }) else { return Ok(false); };
    let Some(display) = world.query_filtered::<Entity, With<SceneBaseGltfRow>>().iter(world)
        .find(|entity| ui_visible(world, *entity)) else { return Ok(false); };
    let Some(root) = main_scene_row(world) else { return Ok(false); };
    let Some(model) = scene_row(world, MODEL, false) else { return Ok(false); };
    let Some(cuboid) = scene_row(world, cube, false) else { return Ok(false); };
    let Some(cube_icon) = row_icon(world, cuboid, "icons/editor/tree_mesh.png") else { return Ok(false); };
    let model_arrow = row_arrow(world, model)?;
    let has_heading = world.query::<(Entity, &Text)>().iter(world)
        .any(|(entity, text)| text.as_str() == "网格对象" && ui_visible(world, entity));
    let position = |entity| world.get::<UiGlobalTransform>(entity).unwrap().translation;
    let left = |entity| position(entity).x - world.get::<ComputedNode>(entity).unwrap().size().x * 0.5;
    let name = world.get::<Children>(root).into_iter().flat_map(|children| children.iter())
        .flat_map(|child| world.get::<Children>(child).into_iter().flat_map(|children| children.iter()))
        .find_map(|child| world.get::<Text>(child)).map(|text| text.as_str());
    Ok(name.is_some_and(|name| name.contains("scene.glb"))
        && !has_heading
        && position(display).y < position(root).y
        && position(root).y < position(plant).y && left(root) < left(cuboid)
        && position(plant).y < position(cuboid).y && position(cuboid).y < position(model).y
        && (position(cube_icon).x - position(model_arrow).x).abs() < 1.1
        && row_has_icon(world, terrain, TERRAIN_ICON)
        && row_has_icon(world, root, GLB_ICON)
        && row_has_icon(world, model, GLB_ICON))
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
    row_icon(world, row, path).is_some()
}

fn row_icon(world: &World, row: Entity, path: &str) -> Option<Entity> {
    let children = |entity| world.get::<Children>(entity).into_iter().flat_map(|children| children.iter());
    children(row).chain(children(row).flat_map(children)).find(|&entity| {
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

fn folder_row(world: &mut World, name: &str) -> Option<Entity> {
    world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        (matches!(target.key, SceneObjectKey::Group { .. } | SceneObjectKey::PlacementFolder)
            && target.name == name && ui_visible(world, entity))
            .then_some(entity)
    })
}

fn folder_rows_reparented(world: &World, drop: FolderDrop) -> bool {
    let browser = world.resource::<EditorGltfBrowserState>();
    let depth = usize::from(drop.parent().is_some());
    // 新发布树的实际缩进和成员一起变化，闭合目标在首次放下后应自动展开。
    browser.entries.iter().any(|row| row.depth == depth && matches!(&row.kind,
        EditorGltfBrowserEntryKind::Group { name, count: 2, collapsed: false, .. } if name == RENAMED_FOLDER))
        && ["scene-tree-group-0", "scene-tree-group-1"].into_iter().all(|id| {
            browser.entries.iter().any(|row| row.depth == depth + 1 && matches!(&row.kind,
                EditorGltfBrowserEntryKind::Placement { placement_id, .. } if placement_id == id))
        })
}

fn row_arrow(world: &World, row: Entity) -> Result<Entity, String> {
    world.get::<Children>(row).and_then(|children| children.iter()
        .find(|child| world.get::<PlacementTreeExpander>(*child).is_some()))
        .ok_or_else(|| "场景树验收没有实际的展开按钮".into())
}

fn click_control(world: &mut World, entity: Entity, frame: u8) -> Result<Option<u8>, String> {
    let (window, position) = pointer_context(world, entity)?;
    let mut pending = Some((position, frame));
    pointer_click(world, &mut pending, window, position, MouseButton::Left);
    Ok(pending.map(|(_, next)| next))
}

fn press_rename(world: &mut World, input: Entity, name: &str, frame: u8) -> Result<bool, String> {
    let Some(button) = source_space_button(world, "确认名称") else { return Ok(false); };
    if frame == 0 { set_test_name(world, input, name)?; }
    click_control(world, button, frame)?;
    Ok(true)
}

fn set_test_name(world: &mut World, input: Entity, name: &str) -> Result<(), String> {
    // 设置实际字段值，名字事务与落盘仍由生产系统执行。
    world.resource_mut::<bevy::input_focus::InputFocus>()
        .set(input, bevy::input_focus::FocusCause::Navigated);
    let mut text = world.get_mut::<bevy::text::EditableText>(input).ok_or("缺少名称输入")?;
    crate::ui::text_input::replace_single_line_value(&mut text, name, 128);
    Ok(())
}

fn pointer_context(world: &mut World, entity: Entity) -> Result<(Entity, bevy::math::DVec2), String> {
    let position = world.get::<UiGlobalTransform>(entity).ok_or("树控件尚未布局")?.translation.as_dvec2();
    let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world)
        .map_err(|_| "没有场景树验收窗口")?;
    Ok((window, position))
}

fn drag_preview_visible(world: &mut World) -> bool {
    world.query_filtered::<&Node, With<SceneBrowserDragPreview>>().iter(world)
        .any(|node| node.display == Display::Flex)
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
    world.query_filtered::<(Entity, &SceneObjectRow), With<SceneTreeRootRow>>().iter(world).find_map(|(entity, row)| {
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
