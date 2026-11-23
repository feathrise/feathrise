//! 用原窗口右键、名称输入、后台文件写入与撤销验收场景树新建和空目录删除。

use super::*;
use crate::{
    types::EditorAction,
    ui::{gltf_browser::SceneTreeContents, scene_inspector::InspectorTree},
};

pub(super) const ROOT_FOLDER: &str = "验收根目录";
pub(super) const ROOT_GLB: &str = "验收根空模型";
pub(super) const NESTED_GLB: &str = "验收夹内空模型";
const SOURCE_FOLDER: &str = "验收源节点";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Case { RootFolder, RootGlb, NestedGlb, SourceFolder, RemoveSourceFolder, RemoveEmptyFolder }

impl Case {
    fn action(self) -> SceneMenuAction {
        match self {
            Self::RootFolder => SceneMenuAction::NewPlacementFolder,
            Self::RootGlb | Self::NestedGlb => SceneMenuAction::NewEmptyGlb,
            Self::SourceFolder => SceneMenuAction::NewSourceFolder,
            Self::RemoveSourceFolder => SceneMenuAction::DeleteSourceFolder,
            Self::RemoveEmptyFolder => SceneMenuAction::Delete,
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

    fn node(self, world: &mut World) -> Option<Entity> {
        match self {
            Self::RootFolder | Self::RootGlb => world.query_filtered::<Entity, With<InspectorTree>>().single(world).ok(),
            Self::NestedGlb => folder_row(world, ROOT_FOLDER),
            Self::SourceFolder => model_row(world),
            Self::RemoveSourceFolder => world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
                let target = row.0.as_ref()?;
                matches!(&target.key, SceneObjectKey::Placement { mesh: Some(mesh), .. } if mesh.is_source_folder()
                    && mesh.node_path.last().is_some_and(|name| name == SOURCE_FOLDER) && ui_visible(world, entity))
                    .then_some(entity)
            }),
            Self::RemoveEmptyFolder => folder_row(world, CREATED_CHILD),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Stage {
    CollapseMain(Case, u8), Menu(Case, u8), Action(Case), Name(Case), Applied(Case),
    RootVisible(Case), ExpandSource(u8), Undo, Restored, SelectParent(u8),
}

pub(super) fn begin() -> Stage { Stage::CollapseMain(Case::RootFolder, 0) }

fn model_row(world: &mut World) -> Option<Entity> {
    world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        let target = row.0.as_ref()?;
        (target.name == ROOT_GLB && matches!(target.key, SceneObjectKey::Placement { mesh: None, .. })
            && ui_visible(world, entity)).then_some(entity)
    })
}

pub(super) fn advance(world: &mut World, stage: &mut Stage, revision: &mut u64, pulse: &mut Option<Entity>) -> Result<bool, String> {
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
            let Some(node) = case.node(world) else { return Ok(false); };
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
            world.entity_mut(button).insert(EditorButtonClick::triggered());
            *pulse = Some(button);
            if case.new_name().is_some() { Stage::Name(case) } else { Stage::Applied(case) }
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
                SourceFolder => source_folder_exists(),
                RemoveSourceFolder => !source_folder_exists(),
                RemoveEmptyFolder => !draft.object_folders.contains(CREATED_CHILD),
            };
            if !expected { return Err(format!("场景树操作落在了错误的目标下：{case:?}")); }
            *revision = session.revision;
            match case {
                RootFolder | RootGlb => Stage::RootVisible(case),
                NestedGlb => Stage::Menu(SourceFolder, 0),
                SourceFolder => Stage::ExpandSource(0),
                RemoveSourceFolder => Stage::Menu(RemoveEmptyFolder, 0),
                RemoveEmptyFolder => Stage::Undo,
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
        Stage::ExpandSource(frame) => {
            let Some(row) = model_row(world) else { return Ok(false); };
            let arrow = row_arrow(world, row)?;
            match click_control(world, arrow, frame)? {
                Some(frame) => Stage::ExpandSource(frame),
                None => Stage::Menu(RemoveSourceFolder, 0),
            }
        }
        Stage::Undo => {
            let Some(button) = crate::standalone::scene_format_validation::action_button(world, EditorAction::Undo) else { return Ok(false); };
            world.entity_mut(button).insert(EditorButtonClick::triggered());
            *pulse = Some(button);
            Stage::Restored
        }
        Stage::Restored => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.revision == *revision { return Ok(false); }
            if session.draft.as_ref().and_then(|draft| draft.object_folders.parent(CREATED_CHILD)) != Some(RENAMED_FOLDER) {
                return Err("撤销删除未恢复空文件夹的原父级".into());
            }
            *revision = session.revision;
            Stage::SelectParent(0)
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
