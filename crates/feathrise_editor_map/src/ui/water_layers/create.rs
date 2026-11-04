//! 空白水层单任务准备：没有像素总门，失败、换图和过期结果不会留下半个历史操作。
use super::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};

pub(super) struct BlankCreation {
    request: PendingClipmapWaterLayerRequest,
    selected: usize,
    level: Option<f32>,
    task: Task<Result<Arc<Vec<u8>>, String>>,
    retiring: bool,
    cancelled: bool, // 任务尚未完成也永久记住取消，不能恢复旧选择后重新获得采用资格。
}

fn allocate(resolution: [u32; 2]) -> Result<Arc<Vec<u8>>, String> {
    let count = pixel_count(resolution)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(count)
        .map_err(|e| format!("空白R8内存分配失败：{e}"))?;
    bytes.resize(count, 0);
    Ok(Arc::new(bytes))
}

pub(super) fn request_current(request: &PendingClipmapWaterLayerRequest, selected: usize,
    editor: &MapEditorState, session: &EditorAuthoringSession) -> bool {
    editor.open && !editor.close_requested && !editor.save_requested && !session.saving && !session.water_preset_delete_pending
        && !editor.map_transition_in_progress && editor.pending_map_transition.is_none()
        && editor.tool == EditorTool::Water && editor.selected_water_layer == selected
        && editor.selected_map_id.as_ref() == Some(&request.map_id)
        && session.revision == request.authoring_revision
        && crate::authoring::water_canvas::writable(session, editor)
        && session.draft.as_ref().is_some_and(|draft| request.matches_canvas(draft)
            && (draft.terrain_documents.active.is_some() || editor.terrain_canvas_target.is_none())
            && draft.water.as_ref().is_some_and(|water| match &request.source_layer_id {
                Some(id) => water.layers.get(selected).is_some_and(|layer| &layer.id == id),
                None => water.layers.is_empty(),
            }))
}

pub(super) fn begin(
    selected: usize,
    preset: Option<&str>,
    level: Option<f32>,
    pending: &mut PendingClipmapWaterLayerCreate,
    editor: &mut MapEditorState,
    session: &EditorAuthoringSession,
    frozen_request: Option<PendingClipmapWaterLayerRequest>,
) {
    if pending.blank.is_some() {
        editor.status = "静水层遮罩正在后台准备，完成后可继续新建".into();
        return; // 保留一个在途计算槽，不能取消未完成任务后马上启动另一份大分配。
    }
    let Some(draft) = session.draft.as_ref().filter(|_| !session.saving) else {
        return;
    };
    let Some(water) = draft.water.as_ref() else {
        return;
    };
    let source = water.layers.get(selected);
    let resolution = source.map_or_else(
        || default_water_layer(&draft.terrain_folder()).resolution,
        |s| s.resolution,
    );
    let request = frozen_request.unwrap_or_else(|| PendingClipmapWaterLayerRequest {
        map_id: draft.map_id.clone(),
        canvas: crate::authoring::water_canvas::identity(draft),
        source_layer_id: source.map(|s| s.id.clone()),
        preset_id: preset.map(str::to_owned),
        authoring_revision: session.revision,
    });
    if !request_current(&request, selected, editor, session) {
        editor.status = "静水层准备前水域来源已经变化，本次新建已取消".into();
        return;
    }
    pending.blank = Some(BlankCreation {
        request,
        selected,
        level,
        task: AsyncComputeTaskPool::get().spawn(async move { allocate(resolution) }),
        retiring: false,
        cancelled: false,
    });
    editor.status = format!("正在后台准备{}×{}静水层遮罩", resolution[0], resolution[1]);
}

/// 输入暂时被数值控件持有时只记录取消，不轮询完成值，也不创建、采用或丢弃像素任务。
pub(super) fn observe(pending: &mut PendingClipmapWaterLayerCreate,
    editor: &MapEditorState, session: &EditorAuthoringSession) {
    if let Some(creation) = pending.blank.as_mut().filter(|creation| !creation.retiring) {
        // 保存请求在Input已生效，而文件保存可能仍等Tasks预检；取消不能等到saving或像素完成才检查。
        creation.cancelled |= !request_current(&creation.request, creation.selected, editor, session);
    }
}

pub(super) fn poll(
    pending: &mut PendingClipmapWaterLayerCreate,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
) {
    observe(pending, editor, session);
    let Some(result) = pending
        .blank
        .as_mut()
        .and_then(|b| check_ready(&mut b.task))
    else {
        return;
    };
    let creation = pending.blank.take().expect("已完成空白水层任务仍存在");
    if creation.retiring {
        if editor.open {
            editor.status = "静水层准备期间作者身份已变化，本次新建已取消".into();
        }
        return;
    }
    if creation.cancelled {
        // 巨大完成像素不得在Update析构；保持同一个计算槽直到旧缓冲真正释放。
        pending.blank = Some(BlankCreation {
            retiring: true,
            task: AsyncComputeTaskPool::get().spawn(async move {
                drop(result);
                Err("静水层准备期间作者身份已变化，本次新建已取消".into())
            }),
            ..creation
        });
        return;
    }
    match result {
        Ok(mask) => apply_and_present_water_structure_action(
            EditorAction::WaterAddLayer,
            creation.selected,
            creation.request.preset_id.as_deref(),
            creation.level,
            false,
            editor,
            session,
            Some(mask),
        ),
        Err(error) => editor.status = error,
    }
}

#[cfg(test)]
pub(super) fn test_blank(resolution: [u32; 2]) -> Arc<Vec<u8>> {
    allocate(resolution).unwrap()
}

#[cfg(test)]
pub(super) fn pause_for_tests(pending: &mut PendingClipmapWaterLayerCreate) -> Task<Result<Arc<Vec<u8>>, String>> {
    let creation = pending.blank.as_mut().expect("先启动真实空白R8任务");
    std::mem::replace(&mut creation.task, AsyncComputeTaskPool::get().spawn(std::future::pending()))
}

#[cfg(test)]
pub(super) fn resume_for_tests(pending: &mut PendingClipmapWaterLayerCreate, task: Task<Result<Arc<Vec<u8>>, String>>) {
    pending.blank.as_mut().expect("取消后旧任务仍须占用单槽").task = task;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blank_mask_beyond_old_resolution_and_byte_limit_is_complete() {
        let mask = allocate([4097, 4097]).unwrap();
        assert_eq!(mask.len(), 4097 * 4097);
        assert!(mask.iter().all(|p| *p == 0));
    }
}
