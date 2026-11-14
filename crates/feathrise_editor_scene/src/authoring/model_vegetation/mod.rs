//! 静态模型的稀疏表面植被：复用根图层，单飞后台采样，一次按压只形成一条历史。

mod resources;
mod systems;
mod worker;

pub(crate) use resources::ModelVegetationState;

use super::{EditorAuthoringSession, EditorCommand, EditorSceneDraft};
use crate::resources::SceneEditorState;
use bevy::prelude::*;
use feathrise_scene_content::{ModelSurfaceVegetation, SurfaceTarget};
use std::sync::Arc;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<ModelVegetationState>()
        .add_systems(Update, (systems::hot_reload, systems::focus_root).chain()
            .in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, (
            systems::capture
                .run_if(crate::ui::numeric_scrub_mouse_guard::scene_editor_numeric_scrub_mouse_available)
                .run_if(crate::scene_browser::scene_browser_general_input_available),
            systems::advance,
        ).chain().after(super::vegetation_paint::apply_vegetation_brush)
            .before(super::start_vegetation_compile)
            .in_set(crate::SceneEditorSystemSet::Presentation));
}

/// 仅推进模型数据修订；它没有二维R8差分，保存责任属于根植被JSON及私有块。
pub(crate) fn mark_changed(session: &mut EditorAuthoringSession, editor: &mut SceneEditorState) {
    session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
    session.mark_changed(editor);
    if let Some(draft) = &mut session.draft {
        draft.terrain_documents.active_dirty = true;
        super::terrain_documents::root_vegetation::mark_registration(draft);
    }
}

/// 历史先恢复根槽，再按冻结下标和稳定ID复验；不在撤销输入帧扫描或复制样本。
pub(crate) fn apply_history(
    draft: &mut EditorSceneDraft,
    command: &EditorCommand,
    undoing: bool,
) -> Result<(), String> {
    let EditorCommand::ModelVegetationChanged {
        index,
        object_id,
        before,
        after,
        ..
    } = command
    else {
        return Err("请求的历史不是模型植被笔划".into());
    };
    let data = &mut draft
        .vegetation
        .as_mut()
        .ok_or("根植被文档尚未就绪")?
        .model_surfaces;
    let expected = SurfaceTarget::Model {
        object_id: object_id.clone(),
    };
    let (from, to) = if undoing {
        (after, before)
    } else {
        (before, after)
    };
    if !Arc::ptr_eq(data, from)
        || after
            .get(*index)
            .is_none_or(|value| value.target != expected)
    {
        return Err("模型植被历史目标已变化，未覆盖其他对象的数据".into());
    }
    *data = to.clone(); // 整个目标表也浅共享，撤销不在输入帧克隆全部模型索引。
    draft.terrain_documents.active_dirty = true;
    Ok(())
}

/// 模型复制允许在编辑其他地形时修改停放根文档；只交换完整目标表，不重归属当前二维画布。
pub(crate) fn root_targets(draft: &EditorSceneDraft) -> Arc<Vec<Arc<ModelSurfaceVegetation>>> {
    super::terrain_documents::root_vegetation::source(draft)
        .and_then(|source| {
            source
                .vegetation
                .map(|vegetation| vegetation.model_surfaces.clone())
        })
        .unwrap_or_default()
}

pub(crate) fn validate_transfer(
    draft: &EditorSceneDraft,
    before: &Arc<Vec<Arc<ModelSurfaceVegetation>>>,
) -> Result<(), String> {
    let current = root_targets(draft);
    if !Arc::ptr_eq(&current, before) && !(current.is_empty() && before.is_empty()) {
        return Err("模型植被复制的根目标表已换代，整批未应用".into());
    }
    Ok(())
}

pub(crate) fn transfer(
    draft: &mut EditorSceneDraft,
    targets: Arc<Vec<Arc<ModelSurfaceVegetation>>>,
) -> Result<(), String> {
    if let Some(root) = &mut draft.terrain_documents.root_vegetation {
        let root = Arc::make_mut(root);
        root.vegetation
            .as_mut()
            .ok_or("停放根植被文档已失效")?
            .model_surfaces = targets;
        root.structural_dirty = true;
    } else if draft.terrain_documents.active.is_none() {
        draft
            .vegetation
            .as_mut()
            .ok_or("根植被文档已失效")?
            .model_surfaces = targets;
        draft.terrain_documents.active_dirty = true;
        super::terrain_documents::root_vegetation::mark_registration(draft);
    } else {
        return Err("模型植被复制找不到根作者文档".into());
    }
    Ok(())
}

pub(crate) fn sample_heap_bytes(data: &ModelSurfaceVegetation) -> usize {
    worker::heap_bytes(data)
}

pub(crate) fn model_selected(
    session: &EditorAuthoringSession,
    browser: &crate::ui::gltf_browser::EditorGltfBrowserState,
) -> bool {
    systems::selected(session, browser).is_some()
}

pub(super) fn target(data: &ModelSurfaceVegetation, id: &str) -> bool {
    matches!(&data.target, SurfaceTarget::Model { object_id } if object_id == id)
}

#[cfg(test)]
mod tests;
