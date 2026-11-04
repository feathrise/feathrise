//! 重叠候选保留各自作者来源；换槽前验证原定义和对象，悬停只能读取已经活动的槽。
use super::layers::{LayerCandidateState, LayerTarget};
use crate::{authoring::EditorMapDraft, types::WaterAuthoringFeatureKind};
use feathrise_map_content::TerrainId;

impl LayerCandidateState {
    /// 一次查询只捕获一个普通作者画布；根水始终有独立来源，地图级对象无需借作者槽。
    pub(super) fn candidate_uses_canvas(&self, target: &LayerTarget) -> bool {
        matches!(target, LayerTarget::RootWater(..) | LayerTarget::Water(..)
            | LayerTarget::Surface(_) | LayerTarget::Vegetation(_))
            || matches!(target, LayerTarget::TerrainObject(id)
                if self.candidate_canvas.as_ref().is_some_and(|source| source.id.as_str() == id))
    }

    pub(super) fn candidate_terrain(&self, target: &LayerTarget) -> Option<&TerrainId> {
        self.candidate_definition(target).map(|source| &source.id)
    }

    fn candidate_definition(&self, target: &LayerTarget) -> Option<&feathrise_map_content::MapTerrainObject> {
        match target {
            LayerTarget::RootWater(..) => None,
            LayerTarget::Water(..) => self.candidate_water_canvas.as_deref(),
            _ => self.candidate_canvas.as_deref(),
        }
    }

    /// 停放源只能给行提供悬停反馈，不能把同名层的下标交给当前画布高亮。
    pub(super) fn candidate_canvas_matches(&self, draft: &EditorMapDraft, target: &LayerTarget) -> bool {
        if !self.candidate_uses_canvas(target) { return true; }
        if draft.terrain_documents.loading { return false; }
        match (self.candidate_terrain(target), draft.terrain_documents.active.as_ref()) {
            (None, None) => true,
            (Some(id), Some(active)) => &active.definition.id == id
                && self.candidate_definition(target).is_some_and(|source| source == active.definition.as_ref()),
            _ => false,
        }
    }

    /// 此门必须先于scope_history和换槽：失效候选不能取得新画布或新修订的身份。
    pub(super) fn validate_candidate_source(&self, draft: &EditorMapDraft,
        target: &LayerTarget, index: Option<usize>) -> Result<(), String> {
        if let Some(active) = &draft.terrain_documents.active {
            let known = [&self.candidate_canvas, &self.candidate_water_canvas].into_iter()
                .flatten().any(|source| source.as_ref() == active.definition.as_ref());
            if !known { return Err("当前画布与候选来源已变化，请重新点击视口".into()); }
            crate::authoring::terrain_documents::root_water::validate_cached_focus(draft, Some(&active.definition.id))?;
        } // 根水目标也不能把未知或已换定义的当前画布停放，再借成功切源提升旧列表身份。
        let terrain = self.candidate_terrain(target);
        crate::authoring::terrain_documents::root_water::validate_cached_focus(draft, terrain)?;
        if let Some(source) = self.candidate_definition(target) {
            let current = draft.manifest.terrains.as_ref().map_or_else(||
                draft.terrain_documents.active.as_ref().is_some_and(|active| active.definition.as_ref() == source),
                |objects| objects.iter().any(|object| object == source));
            if !current {
                return Err("候选所属网格已变化，请重新点击视口".into());
            }
        }
        let (surface, vegetation, water) = if matches!(target, LayerTarget::RootWater(..)) {
            (None, None, crate::authoring::terrain_documents::root_water::source(draft).water)
        } else if self.candidate_canvas_matches(draft, target) {
            (draft.surface.as_ref(), draft.vegetation.as_ref(), draft.water.as_ref())
        } else if let Some(canvas) = terrain.and_then(|id| draft.terrain_documents.values.get(id)) {
            (canvas.surface.as_ref(), canvas.vegetation.as_ref(), canvas.water.as_ref())
        } else {
            return Err("候选作者画布已变化，请重新点击视口".into());
        };
        let exists = match target {
            LayerTarget::Surface(id) => index.and_then(|at| surface?.layers.get(at))
                .is_some_and(|layer| layer.id == *id),
            LayerTarget::Vegetation(id) => index.and_then(|at| vegetation?.authoring.as_ref()?.layers.get(at))
                .is_some_and(|layer| layer.id == *id && layer.enabled),
            LayerTarget::Water(kind, id) | LayerTarget::RootWater(kind, id) => index.is_some_and(|at| {
                water.is_some_and(|water| match kind {
                    WaterAuthoringFeatureKind::StillWater => water.layers.get(at).is_some_and(|layer| layer.id == *id),
                    WaterAuthoringFeatureKind::River => water.rivers.get(at).is_some_and(|river| river.id == *id),
                    WaterAuthoringFeatureKind::Waterfall => water.waterfalls.get(at).is_some_and(|fall| fall.id == *id),
                })
            }),
            LayerTarget::TerrainObject(id) => index.and_then(|at| draft.manifest.terrains.as_ref()?.get(at))
                .is_some_and(|object| object.id.as_str() == id),
            _ => true,
        };
        if exists { Ok(()) } else { Err("候选作者对象已变化，请重新点击视口".into()) }
    }
}
