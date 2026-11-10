//! 超大植被作者空白 R8 的 single-flight 后台分配与身份化采用。

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use feathrise_scene_content::{SceneId, VegetationPresetRegistry};
use feathrise_scene::DynamicMeshProfileRequests;

use crate::authoring::EditorAuthoringSession;
use crate::resources::SceneEditorState;
use crate::types::EditorTool;
use crate::ui::actions::activate_vegetation_layer;

use super::operations::{
    apply_vegetation_authoring_action_with_blank, vegetation_blank_layer_pixel_count,
};

struct PendingVegetationBlankLayer {
    scene_id: SceneId,                       // 切换场景后迟到完成值不得写入另一个场景。
    session_revision: u64,               // 任何中途作者修改都会使当前完成值失效。
    selected_layer: usize,               // 用户换选作者层后不在意外位置插入新层。
    selected_preset_id: Option<String>,  // 用户换选预设后不采用旧尺寸缓冲。
    catalog_revision: u64,               // 目录热更可能改变默认分辨率或输出身份。
    task: Task<Result<Vec<u8>, String>>, // 全局唯一分配槽提供明确内存背压。
}

/// 编辑器内最多只有一张尚未采用的空白植被画布。
#[derive(Resource, Default)]
pub(crate) struct VegetationBlankLayerAllocation {
    pending: Option<PendingVegetationBlankLayer>,
}

impl VegetationBlankLayerAllocation {
    /// 侧栏只观察完整身份匹配的在途画布，不把旧场景或旧预设的分配当作当前进度。
    pub(crate) fn pending_for(
        &self,
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
        registry: Option<&VegetationPresetRegistry>,
    ) -> bool {
        self.pending.as_ref().is_some_and(|pending| {
            editor.open
                && session.draft.as_ref().map(|draft| &draft.scene_id) == Some(&pending.scene_id)
                && editor.selected_scene_id.as_ref() == Some(&pending.scene_id)
                && session.revision == pending.session_revision
                && editor.selected_vegetation_layer == pending.selected_layer
                && editor.vegetation_selected_preset_id == pending.selected_preset_id
                && catalog_revision(registry) == pending.catalog_revision
        })
    }

    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }
}

fn catalog_revision(registry: Option<&VegetationPresetRegistry>) -> u64 {
    registry.map_or(0, |registry| registry.revision)
}

/// 开始后台分配当前新建动作需要的完整零值 R8；不提前修改正式输出或作者描述。
pub(super) fn request_vegetation_blank_layer_allocation(
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    registry: Option<&VegetationPresetRegistry>,
    allocation: &mut VegetationBlankLayerAllocation,
) -> Result<(), String> {
    if allocation.pending.is_some() {
        return Err("已有一张植被空白画布正在后台创建，请稍候".to_owned());
    }
    let pixel_count = vegetation_blank_layer_pixel_count(editor, session, registry)?;
    let scene_id = session
        .draft
        .as_ref()
        .ok_or_else(|| "场景作者数据尚未就绪".to_owned())?
        .scene_id
        .clone();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(pixel_count)
            .map_err(|error| format!("无法为植被作者画布保留{pixel_count}字节：{error}"))?;
        pixels.resize(pixel_count, 0); // 容量已在后台成功保留，清零成本同样不进入输入帧。
        Ok(pixels)
    });
    allocation.pending = Some(PendingVegetationBlankLayer {
        scene_id,
        session_revision: session.revision,
        selected_layer: editor.selected_vegetation_layer,
        selected_preset_id: editor.vegetation_selected_preset_id.clone(),
        catalog_revision: catalog_revision(registry),
        task,
    });
    editor.status = format!("正在后台创建{pixel_count}字节的植被空白作者画布…");
    Ok(())
}

/// 每帧只非阻塞探测唯一分配任务，身份完整匹配后才执行原有撤销与正式输出事务。
pub(crate) fn poll_vegetation_blank_layer_allocation(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    registry: Option<Res<VegetationPresetRegistry>>,
    mut allocation: ResMut<VegetationBlankLayerAllocation>,
    mut mesh_profile_requests: Option<ResMut<DynamicMeshProfileRequests>>,
) {
    let Some(pending) = allocation.pending.as_mut() else {
        return;
    };
    let Some(result) = check_ready(&mut pending.task) else {
        return;
    };
    let pending = allocation
        .pending
        .take()
        .expect("完成探测期间唯一植被画布任务仍存在");
    let current_scene = session.draft.as_ref().map(|draft| &draft.scene_id);
    if !editor.open
        || current_scene != Some(&pending.scene_id)
        || session.revision != pending.session_revision
        || editor.selected_vegetation_layer != pending.selected_layer
        || editor.vegetation_selected_preset_id != pending.selected_preset_id
        || catalog_revision(registry.as_deref()) != pending.catalog_revision
    {
        editor.status =
            "植被空白画布完成时场景、作者修订或预设选择已变化；已丢弃迟到结果".to_owned();
        return;
    }
    let pixels = match result {
        Ok(pixels) => pixels,
        Err(error) => {
            editor.status = error;
            return;
        }
    };
    match apply_vegetation_authoring_action_with_blank(
        &mut editor,
        &mut session,
        registry.as_deref(),
        pixels,
    ) {
        Ok(outcome) => {
            editor.tool = EditorTool::Vegetation;
            if let Some(index) = outcome.selected {
                editor.vegetation_layer_page = index / super::systems::VEGETATION_LAYER_ROW_COUNT;
                if activate_vegetation_layer(
                    &mut editor,
                    &session,
                    index,
                    mesh_profile_requests.as_deref_mut(),
                ) {
                    editor.status = outcome.status;
                }
            } else {
                editor.vegetation_layer_page = 0;
                editor.status = outcome.status;
            }
        }
        Err(error) => editor.status = error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_progress_requires_complete_current_allocation_identity() {
        let draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        let session = EditorAuthoringSession {
            draft: Some(draft),
            revision: 12,
            ..default()
        };
        let mut editor = SceneEditorState {
            open: true,
            selected_scene_id: Some(session.draft.as_ref().unwrap().scene_id.clone()),
            selected_vegetation_layer: 2,
            vegetation_selected_preset_id: Some("hn01_ordinary_grass".into()),
            ..default()
        };
        let pool = AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let mut allocation = VegetationBlankLayerAllocation {
            pending: Some(PendingVegetationBlankLayer {
                scene_id: editor.selected_scene_id.clone().unwrap(),
                session_revision: 12,
                selected_layer: 2,
                selected_preset_id: editor.vegetation_selected_preset_id.clone(),
                catalog_revision: 0,
                task: pool.spawn(async { Ok(vec![0; 4]) }),
            }),
        };
        assert!(allocation.pending_for(&editor, &session, None));
        editor.selected_scene_id = None;
        assert!(!allocation.pending_for(&editor, &session, None));
        editor.selected_scene_id = Some(session.draft.as_ref().unwrap().scene_id.clone());
        editor.selected_vegetation_layer = 3;
        assert!(!allocation.pending_for(&editor, &session, None));
        editor.selected_vegetation_layer = 2;
        editor.vegetation_selected_preset_id = Some("hn01_rock".into());
        assert!(!allocation.pending_for(&editor, &session, None));
        editor.vegetation_selected_preset_id = Some("hn01_ordinary_grass".into());
        allocation.pending.as_mut().unwrap().session_revision = 11;
        assert!(!allocation.pending_for(&editor, &session, None));
        allocation.pending.as_mut().unwrap().session_revision = 12;
        allocation.pending.as_mut().unwrap().catalog_revision = 1;
        assert!(!allocation.pending_for(&editor, &session, None));
        allocation.pending.as_mut().unwrap().catalog_revision = 0;
        editor.open = false;
        assert!(!allocation.pending_for(&editor, &session, None));
    }
}
