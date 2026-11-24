//! 从已呈现场景树冻结源 GLB 和节点，再由单飞后台任务生成完整源文件草稿。

use crate::{
    SceneEditorSystemSet,
    authoring::{
        EditorAuthoringSession, EditorCommand, EditorSceneDraft,
        source_glbs::{
            self, SourceGlbEdits, SourceGlbOperation, SourceGlbTarget, SourceNodeAddress,
            SourceMeshProperties,
        },
    },
    resources::{SceneEditorState, SceneEditorViewportState},
    ui::scene_context_menu::{
        SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectTarget,
    },
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_scene_content::SceneId;
mod pose;
mod properties;
mod spaces; // 源 GLB、所在场景和世界坐标的矩阵换算与引用核对。
mod actions;
pub(crate) use pose::Inspector as SourceNodeInspector;
pub(super) use pose::panel;
#[cfg(feature = "validation")]
pub(crate) use pose::source_position_field_for_validation;
#[cfg(feature = "validation")]
pub(crate) use actions::source_name_input_for_validation;

#[derive(Message, Clone)]
pub(crate) enum SourceGlbRequest {
    Move {
        source: SceneObjectTarget,
        destination: SceneObjectTarget,
    },
    NodeEdit {
        target: SceneObjectTarget,
        expected_hash: String,
        pose: Option<Transform>,
        properties: Option<SourceMeshProperties>,
    },
    Rename { target: SceneObjectTarget, expected_hash: String, name: String },
    ApplyTransform { target: SceneObjectTarget, expected_hash: String, remaining: Transform },
}

type Prepared = Result<Option<(SourceGlbEdits, usize)>, String>;

struct Pending {
    scene: SceneId,
    revision: u64,
    before: SourceGlbEdits,
    task: Task<Prepared>,
}

#[derive(Resource, Default)]
struct Editing(Option<Pending>);

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Editing>()
        .add_message::<SourceGlbRequest>()
        .add_systems(Update, advance.in_set(SceneEditorSystemSet::Tasks));
    pose::install(app);
    actions::install(app);
}

pub(crate) fn owns_selection(browser: &crate::ui::gltf_browser::EditorGltfBrowserState) -> bool {
    browser.selection.as_ref().is_some_and(|node| {
        node.node_index.is_some()
            && !node.orphaned_water_binding
            && browser.selected_placement_id.as_ref() == Some(&node.placement_id)
    })
}

fn target(
    object: &SceneObjectTarget,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    hierarchy_changed: bool,
) -> Result<SourceGlbTarget, String> {
    if !object.current(editor, session) || object.hidden || object.locked {
        return Err("GLB 目标已变化、隐藏或锁定".into());
    }
    let draft = session.draft.as_ref().ok_or("场景尚未打开")?;
    resolve_source_target(object, draft, hierarchy_changed)
}

/// 属性读取也使用同一节点地址校验，锁定或隐藏仅限制写入。
fn resolve_source_target(
    object: &SceneObjectTarget,
    draft: &EditorSceneDraft,
    hierarchy_changed: bool,
) -> Result<SourceGlbTarget, String> {
    let SceneObjectKey::Placement { index, id, mesh } = &object.key else {
        return Err("请选择一个外部 GLB 或其中的节点".into());
    };
    let record = draft.placements.placements.get(*index).filter(|record| &record.id == id)
        .ok_or("源 GLB 摆放已变化")?;
    if !record.asset_target.is_scene0() || record.asset_path == draft.placements_path() {
        return Err("此节点不属于可直接编辑的源 GLB".into());
    }
    if hierarchy_changed && !record.water_surfaces.is_empty() {
        return Err("当前实例包含按节点路径引用的水域，请先解除相关网格水绑定".into());
    }
    let node = mesh
        .as_ref()
        .map(|mesh| {
            Ok::<_, String>(SourceNodeAddress {
                index: mesh.node_index.ok_or("源 GLB 节点索引尚未就绪")?,
                path: mesh.node_path.clone(),
                mesh: mesh.mesh_index,
            })
        })
        .transpose()?;
    let world = placement_world(draft, record)?;
    Ok(SourceGlbTarget {
        path: record.asset_path.clone(),
        node,
        world,
    })
}

fn placement_world(
    draft: &EditorSceneDraft,
    record: &feathrise_scene_content::ScenePlacementRecord,
) -> Result<Mat4, String> {
    use crate::authoring::placement_pose::{placement_record_frame, placement_rotation};
    let frame = placement_record_frame(draft, record)?
        .as_transform()
        .to_matrix();
    let local = Mat4::from_scale_rotation_translation(
        record.scale.into(),
        placement_rotation(record),
        record.translation.into(),
    );
    Ok(frame * local) // 跨源时在当前场景的两个摆放坐标系之间换算。
}

fn advance(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editing: ResMut<Editing>,
    menu: Option<Res<SceneContextMenuState>>,
    viewport: Option<Res<SceneEditorViewportState>>,
    mut requests: MessageReader<SourceGlbRequest>,
    mut retirement: ResMut<crate::authoring::AuthoringRetirement>,
) {
    let request = requests.read().last().cloned();
    if let Some(pending) = editing.0.as_mut() {
        let Some(result) = check_ready(&mut pending.task) else {
            return;
        };
        let pending = editing.0.take().expect("已就绪的任务仍由该槽持有");
        session.source_glb_busy = false;
        let current = editor.open
            && !session.saving
            && session.revision == pending.revision
            && session.draft.as_ref().is_some_and(|draft| {
                draft.scene_id == pending.scene
                    && draft
                        .source_glb_edits
                        .verify_current(&pending.before)
                        .is_ok()
            });
        if !current {
            retirement.push(result);
            editor.status = "场景或源 GLB 草稿已变化，已取消这次源编辑".into();
        } else {
            match result {
                Ok(Some((after, bytes))) => {
                    session.draft.as_mut().unwrap().source_glb_edits = after.clone();
                    let command = EditorCommand::SourceGlbsChanged {
                        before: pending.before,
                        after,
                        bytes,
                    };
                    let mut retired = Vec::new();
                    session.push_bounded_history_retiring(command, &mut retired);
                    retirement.push(retired);
                    session.mark_changed(&mut editor);
                    editor.status = "源 GLB 已修改，保存场景后更新所有引用实例".into();
                }
                Ok(None) => editor.status = "源 GLB 保持原状".into(),
                Err(error) => editor.status = error,
            }
        }
        return;
    }
    let menu_request = menu
        .as_ref()
        .and_then(|menu| menu.ready.as_ref())
        .filter(|request| {
            let source_action = match request.action {
                SceneMenuAction::NewGlbCuboid | SceneMenuAction::NewGlbPlane =>
                    matches!(request.target.key, SceneObjectKey::Placement { mesh: None, .. }),
                SceneMenuAction::RenameSourceNode | SceneMenuAction::NewSourceFolder | SceneMenuAction::DeleteSourceFolder => true,
                _ => false,
            };
            source_action && request.target.allows(request.action)
        });
    let resolved = match (request, menu_request) {
        (
            Some(SourceGlbRequest::Move {
                source,
                destination,
            }),
            _,
        ) => target(&source, &editor, &session, true).and_then(|source| {
            Ok((
                source,
                SourceGlbOperation::Move(target(&destination, &editor, &session, false)?),
            ))
        }),
        (
            Some(SourceGlbRequest::NodeEdit {
                target: object,
                expected_hash,
                pose,
                properties,
            }),
            _,
        ) => target(&object, &editor, &session, false).map(|target| {
            (target, SourceGlbOperation::NodeEdit { expected_hash, pose, properties })
        }),
        (Some(SourceGlbRequest::Rename { target: object, expected_hash, name }), _) => {
            target(&object, &editor, &session, true).map(|target| {
                (target, SourceGlbOperation::Rename { name, expected_hash: Some(expected_hash) })
            })
        }
        (Some(SourceGlbRequest::ApplyTransform { target: object, expected_hash, remaining }), _) =>
            target(&object, &editor, &session, false).map(|target| {
                (target, SourceGlbOperation::ApplyTransform { expected_hash, remaining })
            }),
        (None, Some(request)) => target(
            &request.target,
            &editor,
            &session,
            request.action == SceneMenuAction::RenameSourceNode,
        )
        .and_then(|target| {
            let operation = match request.action {
                SceneMenuAction::RenameSourceNode => {
                    SourceGlbOperation::Rename { name: request.name.clone(), expected_hash: None }
                }
                SceneMenuAction::DeleteSourceFolder => SourceGlbOperation::DeleteFolder,
                SceneMenuAction::NewGlbCuboid | SceneMenuAction::NewGlbPlane => {
                    let viewport = viewport.as_deref().ok_or("场景中心尚未就绪")?;
                    SourceGlbOperation::AddPrimitive {
                        shape: crate::ui::terrain_objects::tree_shape_target(request.action == SceneMenuAction::NewGlbPlane)?,
                        center: viewport.scene_center,
                    }
                }
                _ => SourceGlbOperation::Folder(request.name.clone()),
            };
            Ok((target, operation))
        }),
        _ => return,
    };
    let (source, operation) = match resolved {
        Ok(source) => source,
        Err(error) => {
            editor.status = error;
            return;
        }
    };
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    let before = draft.source_glb_edits.clone();
    let original = before.clone();
    let root = feathrise_editor_core::workspace_asset_root();
    let task = IoTaskPool::get()
        .spawn(async move { source_glbs::edit(&root, &original, source, operation) });
    editing.0 = Some(Pending {
        scene: draft.scene_id.clone(),
        revision: session.revision,
        before,
        task,
    });
    session.source_glb_busy = true;
    editor.status = "正在准备源 GLB 修改".into();
}
