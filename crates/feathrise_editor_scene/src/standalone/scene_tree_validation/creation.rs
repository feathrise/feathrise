//! 用原窗口右键、名称输入、后台文件写入与撤销验收场景树新建和空目录删除。

use super::*;
use crate::{
    types::EditorAction,
    ui::{file_manager::{FileAction, FileManagerButton}, gltf_browser::SceneTreeContents, scene_inspector::InspectorTree},
};

pub(super) const ROOT_FOLDER: &str = "验收根目录";
pub(super) const ROOT_GLB: &str = "验收根空模型";
pub(super) const NESTED_GLB: &str = "验收夹内空模型";
const SOURCE_FOLDER: &str = "验收源节点";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Case {
    RootFolder, RootGlb, NestedGlb, SourceFolder, RemoveSourceFolder, RemoveEmptyFolder, RemoveGlb, RemoveCube,
    ChooseGlb, NewMotion, RemoveMotion,
}

impl Case {
    fn action(self) -> SceneMenuAction {
        match self {
            Self::RootFolder => SceneMenuAction::NewPlacementFolder,
            Self::RootGlb | Self::NestedGlb => SceneMenuAction::NewEmptyGlb,
            Self::ChooseGlb => SceneMenuAction::ChooseExistingGlb,
            Self::SourceFolder => SceneMenuAction::NewSourceFolder,
            Self::RemoveSourceFolder => SceneMenuAction::DeleteSourceFolder,
            Self::NewMotion => SceneMenuAction::NewSceneAnimation,
            Self::RemoveEmptyFolder | Self::RemoveGlb | Self::RemoveCube | Self::RemoveMotion => SceneMenuAction::Delete,
        }
    }

    fn new_name(self) -> Option<&'static str> {
        match self {
            Self::RootFolder => Some(ROOT_FOLDER),
            Self::RootGlb => Some(ROOT_GLB),
            Self::NestedGlb => Some(NESTED_GLB),
            Self::SourceFolder => Some(SOURCE_FOLDER),
            _ => None,
        }
    }

    fn node(self, world: &mut World, cube: &str) -> Option<Entity> {
        match self {
            Self::RootFolder | Self::RootGlb => world.query_filtered::<Entity, With<InspectorTree>>().single(world).ok(),
            Self::NestedGlb | Self::ChooseGlb => folder_row(world, ROOT_FOLDER),
            Self::SourceFolder | Self::RemoveGlb => model_row(world),
            Self::RemoveSourceFolder => world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
                let target = row.0.as_ref()?;
                matches!(&target.key, SceneObjectKey::Placement { mesh: Some(mesh), .. } if mesh.is_source_folder()
                    && mesh.node_path.last().is_some_and(|name| name == SOURCE_FOLDER) && ui_visible(world, entity))
                    .then_some(entity)
            }),
            Self::RemoveEmptyFolder => folder_row(world, CREATED_CHILD),
            Self::RemoveCube => scene_row(world, cube, false),
            Self::NewMotion => main_scene_row(world),
            Self::RemoveMotion => world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
                (matches!(row.0.as_ref()?.key, SceneObjectKey::MainSceneAnimation(_)) && ui_visible(world, entity))
                    .then_some(entity)
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Stage {
    CollapseMain(Case, u8), Menu(Case, u8), Action(Case), Name(Case), Applied(Case),
    RootVisible(Case), ExpandSource(u8), Mode(ViewportMode), Undo(Case), Restored(Case),
    PickDirectory(u8, u8), PickFile(u8), ConfirmFile(u8),
    MoveMesh(u8), MeshMoved, MeshRestored,
    MotionLayout, MotionScreenshot, SelectParent(u8),
}

pub(super) fn begin() -> Stage { Stage::CollapseMain(Case::RootFolder, 0) }

fn model_row(world: &mut World) -> Option<Entity> {
    named_model_row(world, ROOT_GLB)
}

fn named_model_row(world: &mut World, name: &str) -> Option<Entity> {
    world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        (target.name == name && matches!(target.key, SceneObjectKey::Placement { mesh: None, .. })
            && ui_visible(world, entity)).then_some(entity)
    })
}

pub(super) fn advance(world: &mut World, stage: &mut Stage, cube: &str, revision: &mut u64, pulse: &mut Option<Entity>) -> Result<bool, String> {
    use Case::*;
    *stage = match *stage {
        Stage::CollapseMain(case, frame) => {
            let Some(root) = main_scene_row(world) else { return Ok(false); };
            let arrow = row_arrow(world, root)?;
            click_control(world, arrow, frame)?.map_or(Stage::Menu(case, 0), |next| Stage::CollapseMain(case, next))
        }
        Stage::Menu(case, frame) => {
            let root_case = matches!(case, RootFolder | RootGlb);
            if root_case && frame == 0 {
                let (contents, layout) = world.query_filtered::<(&Node, &ComputedNode), With<SceneTreeContents>>()
                    .single(world).map_err(|_| "验收缺少场景树正文")?;
                if contents.display != Display::None { return Err("创建前应通过根箭头收起主 GLB".into()); }
                if !layout.is_empty() { return Ok(false); }
            }
            let Some(node) = case.node(world, cube) else { return Ok(false); };
            let (window, mut position) = pointer_context(world, node)?;
            if root_case {
                // 先找真实滚动视口，再点其末尾留白，避免借用任何对象行。
                position.y += f64::from(world.get::<ComputedNode>(node).ok_or("场景树尚未布局")?.size().y) * 0.5 - 14.0;
            }
            let mut click = Some((position, frame));
            if right_click(world, &mut click, window, position) { Stage::Action(case) }
            else { Stage::Menu(case, click.unwrap().1) }
        }
        Stage::Action(case) => {
            let Some(button) = visible_action_for_validation(world, case.action()) else { return Ok(false); };
            if matches!(case, SourceFolder | RemoveSourceFolder) && visible_action_for_validation(world, SceneMenuAction::NewEmptyGlb).is_some() {
                return Err("源 GLB 内部菜单不能创建另一份 GLB".into());
            }
            if case == RemoveCube && visible_action_for_validation(world, SceneMenuAction::EditInBlender).is_some() {
                return Err("主场景内嵌网格应从主 GLB 根行打开 Blender".into());
            }
            if matches!(case, NewMotion | SourceFolder) && visible_action_for_validation(world, SceneMenuAction::EditInBlender).is_none() {
                return Err("主场景和独立 GLB 都应在文件根提供 Blender 入口".into());
            }
            world.entity_mut(button).insert(EditorButtonClick::triggered());
            *pulse = Some(button);
            if case == ChooseGlb { Stage::PickDirectory(0, 0) }
            else if case.new_name().is_some() { Stage::Name(case) } else { Stage::Applied(case) }
        }
        Stage::Name(case) => {
            let Some((input, button)) = visible_rename_controls_for_validation(world) else { return Ok(false); };
            set_test_name(world, input, case.new_name().ok_or("新建入口缺少名称")?)?;
            world.entity_mut(button).insert(EditorButtonClick::triggered());
            *pulse = Some(button);
            Stage::Applied(case)
        }
        Stage::Applied(case) => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.revision == *revision { return Ok(false); }
            if session.revision != revision.wrapping_add(1) { return Err("一次右键操作应只采用一笔作者变更".into()); }
            let draft = session.draft.as_ref().ok_or("缺少场景草稿")?;
            let source = format!("{}/models/{ROOT_GLB}.glb", draft.folder.trim_end_matches('/'));
            let source_folder_exists = || draft.source_glb_edits.version(&source).is_some_and(|version|
                version.nodes.iter().any(|node| node.folder && node.path.last().is_some_and(|name| name == SOURCE_FOLDER)));
            let model_exists = |name, group| draft.placements.placements.iter().any(|item|
                item.editor.name == name && item.editor.group == group && item.asset_target.is_scene0());
            let expected = match case {
                RootFolder => draft.object_folders.contains(ROOT_FOLDER) && draft.object_folders.parent(ROOT_FOLDER).is_none(),
                RootGlb => model_exists(ROOT_GLB, ""),
                NestedGlb => model_exists(NESTED_GLB, ROOT_FOLDER),
                ChooseGlb => draft.placements.placements.iter().any(|item| {
                    item.asset_path == ASSET_PATH && item.editor.group == ROOT_FOLDER && item.asset_target.is_scene0()
                }),
                SourceFolder => source_folder_exists(),
                RemoveSourceFolder => !source_folder_exists(),
                RemoveEmptyFolder => !draft.object_folders.contains(CREATED_CHILD),
                RemoveGlb => !model_exists(ROOT_GLB, ""),
                RemoveCube => !draft.placements.placements.iter().any(|item| item.id == cube),
                NewMotion => draft.motion.clip_name().is_some(),
                RemoveMotion => draft.motion.clip_name().is_none(),
            };
            if !expected { return Err(format!("场景树操作落在了错误的目标下：{case:?}")); }
            *revision = session.revision;
            match case {
                RootFolder | RootGlb => Stage::RootVisible(case),
                NestedGlb => Stage::Menu(ChooseGlb, 0),
                ChooseGlb => Stage::MoveMesh(0),
                SourceFolder => Stage::ExpandSource(0),
                RemoveSourceFolder => Stage::Menu(RemoveEmptyFolder, 0),
                RemoveEmptyFolder | RemoveGlb | RemoveCube => Stage::Undo(case),
                NewMotion => Stage::MotionLayout,
                RemoveMotion => Stage::Mode(ViewportMode::Move),
            }
        }
        Stage::RootVisible(case) => {
            // 创建后等真实树行布局可见，期间不向根箭头补发任何输入。
            if case == RootFolder {
                if folder_row(world, ROOT_FOLDER).is_none() { return Ok(false); }
                Stage::CollapseMain(RootGlb, 0)
            } else {
                if model_row(world).is_none() { return Ok(false); }
                Stage::Menu(NestedGlb, 0)
            }
        }
        Stage::PickDirectory(press, frame) => {
            let Some((id, directory)) = ready_picker(world) else { return Ok(false); };
            if directory.ends_with("scene") {
                Stage::PickFile(0)
            } else {
                let Some(row) = picker_row(world, id, "scene") else { return Ok(false); };
                if let Some(next) = click_control(world, row, frame)? { Stage::PickDirectory(press, next) }
                else if press == 0 { Stage::PickDirectory(1, 0) }
                else { Stage::PickFile(0) }
            }
        }
        Stage::PickFile(frame) => {
            let Some((id, _)) = ready_picker(world) else { return Ok(false); };
            let Some(row) = picker_row(world, id, ASSET_PATH) else { return Ok(false); };
            click_control(world, row, frame)?.map_or(Stage::ConfirmFile(0), Stage::PickFile)
        }
        Stage::ConfirmFile(frame) => {
            let Some((id, _)) = ready_picker(world) else { return Ok(false); };
            let Some(button) = world.query::<(Entity, &FileManagerButton)>().iter(world).find_map(|(entity, button)| {
                (button.window == Some(id) && button.action == FileAction::ChooseClip && ui_visible(world, entity)
                    && world.get::<bevy::ui::InteractionDisabled>(entity).is_none()).then_some(entity)
            }) else { return Ok(false); };
            // 确认后由活动文件窗和正常资源加载完成摆放，验收不直接创建记录。
            click_control(world, button, frame)?.filter(|next| *next < 3)
                .map_or(Stage::Applied(ChooseGlb), Stage::ConfirmFile)
        }
        Stage::MoveMesh(frame) => {
            let Some(source) = scene_row(world, MODEL, true) else { return Ok(false); };
            let Some(target) = named_model_row(world, NESTED_GLB) else { return Ok(false); };
            if frame == 0 {
                let target = world.get::<SceneObjectRow>(target).and_then(|row| row.0.as_ref());
                let Some(SceneObjectKey::Placement { id, .. }) = target.map(|target| &target.key) else { return Ok(false); };
                if !branch_collapsed(world, id)? { return Err("网格拖放应以折叠的空 GLB 为目标".into()); }
            }
            drag_tree_rows(world, source, target, frame)?.map_or(Stage::MeshMoved, Stage::MoveMesh)
        }
        Stage::MeshMoved | Stage::MeshRestored => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.revision == *revision { return Ok(false); }
            let draft = session.draft.as_ref().ok_or("跨 GLB 拖放缺少场景草稿")?;
            let contains_mesh = |path| draft.source_glb_edits.version(path).is_some_and(|version|
                version.nodes.iter().any(|node| node.mesh_name == MESH && node.path.last().is_some_and(|name| name == RENAMED_NODE)));
            let target = format!("{}/models/{NESTED_GLB}.glb", draft.folder.trim_end_matches('/'));
            let moved = *stage == Stage::MeshMoved;
            if session.revision != revision.wrapping_add(1) || contains_mesh(ASSET_PATH) == moved
                || contains_mesh(&target) != moved
            { return Err("跨 GLB 拖放和撤销应一次迁移完整网格并保留源身份".into()); }
            *revision = session.revision;
            if moved {
                let Some(button) = crate::standalone::scene_format_validation::action_button(world, EditorAction::Undo)
                else { return Err("跨 GLB 拖放缺少撤销入口".into()); };
                world.entity_mut(button).insert(EditorButtonClick::triggered());
                *pulse = Some(button);
                Stage::MeshRestored
            } else { Stage::Menu(SourceFolder, 0) }
        }
        Stage::ExpandSource(frame) => {
            let Some(row) = model_row(world) else { return Ok(false); };
            let arrow = row_arrow(world, row)?;
            match click_control(world, arrow, frame)? {
                Some(frame) => Stage::ExpandSource(frame),
                None => Stage::Menu(RemoveSourceFolder, 0),
            }
        }
        Stage::Mode(mode) => {
            // 普通选择模式会同步派生选集，覆盖曾使右键删除丢失的真实调度顺序。
            if world.resource::<ViewportModeState>().mode != mode {
                *pulse = Some(modes::validation_press_mode(world, mode)?);
                return Ok(false);
            }
            if mode == ViewportMode::Select { Stage::Menu(RemoveGlb, 0) } else { Stage::SelectParent(0) }
        }
        Stage::Undo(case) => {
            if case.node(world, cube).is_some() { return Ok(false); }
            let Some(button) = crate::standalone::scene_format_validation::action_button(world, EditorAction::Undo) else { return Ok(false); };
            world.entity_mut(button).insert(EditorButtonClick::triggered());
            *pulse = Some(button);
            Stage::Restored(case)
        }
        Stage::Restored(case) => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.revision == *revision { return Ok(false); }
            let draft = session.draft.as_ref().ok_or("撤销后缺少场景草稿")?;
            let restored = match case {
                RemoveEmptyFolder => draft.object_folders.parent(CREATED_CHILD) == Some(RENAMED_FOLDER),
                RemoveGlb => draft.placements.placements.iter().any(|item| item.editor.name == ROOT_GLB && item.editor.group.is_empty()),
                RemoveCube => draft.placements.placements.iter().any(|item| item.id == cube && item.asset_target.is_builtin()),
                _ => false,
            };
            if !restored || session.revision != revision.wrapping_add(1) {
                return Err(format!("撤销删除未恢复该行：{case:?}"));
            }
            if case.node(world, cube).is_none() { return Ok(false); }
            *revision = world.resource::<EditorAuthoringSession>().revision;
            match case {
                RemoveEmptyFolder => Stage::Mode(ViewportMode::Select),
                RemoveGlb => Stage::Menu(RemoveCube, 0),
                _ => Stage::Menu(NewMotion, 0),
            }
        }
        Stage::MotionLayout => {
            let Some(row) = RemoveMotion.node(world, cube) else { return Ok(false); };
            let Some(cube) = scene_row(world, cube, false) else { return Ok(false); };
            let Some(animation_icon) = row_icon(world, row, "icons/editor/tree_file.png") else { return Ok(false); };
            let Some(cube_icon) = row_icon(world, cube, "icons/editor/tree_mesh.png") else { return Ok(false); };
            let x = |entity| world.get::<UiGlobalTransform>(entity).unwrap().translation.x;
            if (x(animation_icon) - x(cube_icon)).abs() >= 1.1 {
                return Err("同级场景动画和内嵌网格图标应对齐".into());
            }
            request_screenshot(world, "scene-tree-animation-alignment.png")?;
            Stage::MotionScreenshot
        }
        Stage::MotionScreenshot => {
            let Some(result) = world.get_resource_mut::<StartupScreenshot>().and_then(|mut task| check_ready(&mut task.0))
            else { return Ok(false); };
            result?;
            world.remove_resource::<StartupScreenshot>();
            Stage::Menu(RemoveMotion, 0)
        }
        Stage::SelectParent(frame) => {
            let Some(row) = folder_row(world, RENAMED_FOLDER) else { return Ok(false); };
            if let Some(next) = click_control(world, row, frame)? {
                Stage::SelectParent(next)
            } else {
                return if world.resource::<EditorGltfBrowserState>().selected_group.as_deref() == Some(RENAMED_FOLDER) {
                    Ok(true)
                } else {
                    Err("删除验证结束后未恢复父文件夹选择".into())
                };
            }
        }
    };
    Ok(false)
}

fn ready_picker(world: &World) -> Option<(u64, PathBuf)> {
    let state = world.resource::<FileManagerState>();
    let window = state.windows.iter().find(|window| {
        !window.loading && state.focused == Some(window.id) && state.glb_picker_open(window.id)
    })?;
    Some((window.id, window.path.clone()))
}

fn picker_row(world: &mut World, id: u64, path: &str) -> Option<Entity> {
    world.query::<(Entity, &FileRow)>().iter(world).find_map(|(entity, row)| {
        (row.window == id && row.path.as_ref().is_some_and(|source| source.ends_with(path))
            && ui_visible(world, entity)).then_some(entity)
    })
}
