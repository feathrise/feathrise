//! 对象画布加载仅一份在途任务；完成值冻结对象定义、场景修订与请求票据。
use super::TerrainCanvas;
use bevy::{prelude::*, tasks::Task};
use feathrise_scene_content::{SceneId, SceneTerrainObject, TerrainId};
use std::sync::Arc;
pub(super) struct Pending {
    pub scene_id: SceneId,
    pub revision: u64,
    pub index: usize,
    pub object: Arc<SceneTerrainObject>,
    pub task: Task<Result<TerrainCanvas, String>>,
}
#[derive(Resource, Default)]
pub(crate) struct TerrainDocumentTask {
    pub reloads: std::collections::VecDeque<String>,
    pub requested: Option<TerrainId>,
    pub(super) pending: Option<Pending>,
    /// 大画布的迟到结果在后台退役；真实释放完成前仍占用同一载入槽。
    pub(super) retirement: Option<Task<()>>,
}

impl TerrainDocumentTask {
    /// 单槽可能仍在结束上一网格的载入；等待选择只能观察属于自己这张画布的开始/结束。
    pub(crate) fn loading_for(&self, map: &SceneId, id: &TerrainId, index: usize, revision: u64) -> bool {
        self.pending.as_ref().is_some_and(|pending| pending.scene_id == *map
            && pending.object.id == *id && pending.index == index && pending.revision == revision)
    }

    #[cfg(test)]
    pub(crate) fn pause_pending_for_tests(&mut self) -> Task<Result<TerrainCanvas, String>> {
        let pending = self.pending.as_mut().expect("先启动真实画布载入再延后采用");
        // 挂起future不占计算/IO线程；测试保留真实Task及全部来源身份，只控制它何时回到采用槽。
        std::mem::replace(&mut pending.task,
            bevy::tasks::IoTaskPool::get().spawn(std::future::pending::<Result<TerrainCanvas, String>>()))
    }

    #[cfg(test)]
    pub(crate) fn resume_pending_for_tests(&mut self, work: Task<Result<TerrainCanvas, String>>) {
        self.pending.as_mut().expect("延迟期间原画布槽必须仍在途").task = work;
    }
}

/// 完整笔划和删层必须先收口，不能在载入完成帧交换它们仍在修改的像素槽位。
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CanvasIdle<'w> {
    surface: Option<Res<'w, super::super::resources::SurfaceBrushStroke>>,
    vegetation: Option<Res<'w, super::super::resources::VegetationBrushStroke>>,
    vegetation_cross: Option<Res<'w, super::super::vegetation_paint_scope::VegetationPaintState>>,
    vegetation_layers: Option<Res<'w, crate::ui::vegetation_layers::VegetationBlankLayerAllocation>>,
    water: Option<Res<'w, super::super::resources::WaterBrushStroke>>,
    terrain: Option<Res<'w, super::super::resources::TerrainBrushStroke>>,
    puddles: Option<Res<'w, super::super::PuddleAuthoringState>>,
    surface_task: Option<Res<'w, super::super::surface_layer_edit::SurfaceLayerEditTask>>,
    sparse: Option<Res<'w, super::super::sparse_terrain::SparseTerrainAuthoringState>>,
}
impl CanvasIdle<'_> {
    pub fn busy(&self, session: &super::super::EditorAuthoringSession) -> bool {
        session.water_preset_delete_pending
            || session.model_vegetation_busy
            || self.vegetation_layers.as_ref().is_some_and(|state| state.busy())
            || self.surface.as_ref().is_some_and(|stroke| stroke.active)
            || self.vegetation_cross.as_ref().is_some_and(|state| state.busy())
            || self.vegetation.as_ref().is_some_and(|stroke| stroke.active)
            || self.water.as_ref().is_some_and(|stroke| stroke.active)
            || self.terrain.as_ref().is_some_and(|stroke| stroke.active)
            || self.puddles.as_ref().is_some_and(|puddles| puddles.busy())
            || self
                .surface_task
                .as_ref()
                .is_some_and(|task| task.is_deleting())
            || self
                .sparse
                .as_ref()
                .is_some_and(|sparse| !sparse.document_can_park())
            || vegetation_compile_pending(session)
    }
}

fn vegetation_compile_pending(session: &super::super::EditorAuthoringSession) -> bool {
    // 停放前必须保存“同一修订的作者图和正式输出”。干净载入的初始化编译不阻止切换，
    // 删除末作者层仍由结构脏标记覆盖，不能因作者脏集合为空而把旧正式密度一起保存。
    session.vegetation_compiled_revision != session.vegetation_revision
        && session.draft.as_ref().is_some_and(|draft| {
            super::super::vegetation_compile::density_canvas_is_active(draft)
                && draft
                .vegetation
                .as_ref()
                .is_some_and(|vegetation| vegetation.authoring.is_some())
                && (draft.terrain_documents.active_dirty
                    || !draft.vegetation_author_dirty_layers.is_empty())
        })
}
