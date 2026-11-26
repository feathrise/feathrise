//! 复用原场景树文件选择后的作者记录、可见树行和实际网格，核对新进程恢复同一个 GLB。

use super::{
    ASSET_PATH, EditorGltfBrowserEntryKind, EditorGltfBrowserState, GLB_ICON, MESH,
    branch_collapsed, click_control, creation::ROOT_FOLDER, folder_row, open_scene_tree, preflight,
    row_has_icon, scene_row, tree_branch_arrow,
};
use crate::authoring::EditorAuthoringSession;
use bevy::{gltf::GltfMeshName, prelude::*};
use feathrise_scene::ScenePlacementInstance;
use feathrise_scene_content::{SceneId, ScenePlacementRecord};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Resource, Default)]
struct ReopenTree(u8);

pub(in crate::standalone) fn record(world: &World) -> Option<&ScenePlacementRecord> {
    let draft = world
        .get_resource::<EditorAuthoringSession>()?
        .draft
        .as_ref()?;
    if draft.scene_id.as_str() != "M1-SAMPLE"
        || !draft.object_folders.contains(ROOT_FOLDER)
        || draft.object_folders.parent(ROOT_FOLDER).is_some()
    {
        return None;
    }
    let mut members = draft
        .placements
        .placements
        .iter()
        .filter(|record| record.editor.group == ROOT_FOLDER);
    let record = members.next()?;
    (members.next().is_none() && record.asset_path == ASSET_PATH && record.asset_target.is_scene0())
        .then_some(record)
}

pub(super) fn visible_proof(world: &mut World, frame: &mut u8) -> Result<Option<Value>, String> {
    let Some(folder) = folder_row(world, ROOT_FOLDER) else {
        return Ok(None);
    };
    // 新进程可能默认收起文件夹。经真实展开箭头发送鼠标移动、按下和释放，等布局发布再取证。
    if *frame < 3 && (*frame > 0 || branch_collapsed(world, ROOT_FOLDER)?) {
        let Some(arrow) = tree_branch_arrow(world, folder) else {
            return Ok(None);
        };
        *frame = click_control(world, arrow, *frame)?.unwrap_or(3);
        return Ok(None);
    }

    let Some(record) = record(world) else {
        return Ok(None);
    };
    let id = record.id.clone();
    let nested = world.resource::<EditorGltfBrowserState>().entries.windows(2).any(|rows| {
        rows[0].depth == 0 && rows[1].depth == 1
            && matches!(&rows[0].kind, EditorGltfBrowserEntryKind::Group { name, count: 1, collapsed: false, .. }
                if name == ROOT_FOLDER)
            && matches!(&rows[1].kind, EditorGltfBrowserEntryKind::Placement { placement_id, glb: true, .. }
                if placement_id == &id)
    });
    let visible =
        scene_row(world, &id, false).is_some_and(|row| row_has_icon(world, row, GLB_ICON));
    if !nested || !visible {
        return Ok(None);
    }

    // 读取实际生成的活动实例及原 GLB 的网格资产，避免将已读入的作者记录当成视口采用。
    let mut instances = world.query::<(Entity, &ScenePlacementInstance)>();
    let mut children = world.query::<&Children>();
    let meshes = world.resource::<Assets<Mesh>>();
    if !instances.iter(world).any(|(root, instance)| {
        instance.scene_id().as_str() == "M1-SAMPLE"
            && instance.id() == id
            && instance.asset_path() == ASSET_PATH
            && instance.asset_target().is_scene0()
            && instance.is_active()
            && children.query(world).iter_descendants(root).any(|entity| {
                world
                    .get::<GltfMeshName>(entity)
                    .is_some_and(|name| name.0 == MESH)
                    && world
                        .get::<Mesh3d>(entity)
                        .is_some_and(|mesh| meshes.contains(mesh.0.id()))
            })
    }) {
        return Ok(None);
    }

    Ok(Some(json!({
        "chosen_glb": {"id": id, "asset_path": ASSET_PATH, "group": ROOT_FOLDER},
        "visible_tree_row": true,
        "rendered_instance": true
    })))
}

pub(super) fn reopened(world: &mut World, project: &Path, scene: &SceneId) -> Option<Value> {
    if !world.contains_resource::<ReopenTree>() {
        preflight(project, scene).ok()?;
        world.init_resource::<ReopenTree>();
    }
    if !open_scene_tree(world).ok()? {
        return None;
    }
    world.resource_scope(|world, mut reopen: Mut<ReopenTree>| {
        visible_proof(world, &mut reopen.0).ok().flatten()
    })
}
