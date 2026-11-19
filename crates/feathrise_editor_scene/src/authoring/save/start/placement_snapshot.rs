//! 保存前分帧冻结摆放记录和精确预览所有权。

use std::ops::Bound::{Excluded, Unbounded};

use bevy::prelude::*;
use feathrise_scene::{DynamicWaterMeshOverrides, ScenePlacementInstance};
use feathrise_scene_content::{SceneId, ScenePlacementRecord, ScenePlacementsAsset};

use super::placement_water::PLACEMENT_WATER_RECORD_SCAN_BUDGET_PER_FRAME as RECORD_BUDGET;
use crate::authoring::{AuthoringRetirement, EditorAuthoringSession, resources::PlacementPreviewHandoffSnapshot};
use crate::components::EditorPlacedAsset;

pub(super) type PlacementPreviewRoots<'w, 's> = Query<'w, 's, (
    Option<&'static ScenePlacementInstance>,
    Option<&'static Transform>,
    Option<&'static ChildOf>,
    Option<&'static DynamicWaterMeshOverrides>,
), With<EditorPlacedAsset>>;

#[derive(Default)]
pub(super) struct PlacementSaveCapture {
    identity: Option<(SceneId, u64, u64, u64)>,
    records: Vec<ScenePlacementRecord>,
    previews: Vec<PlacementPreviewHandoffSnapshot>,
    preview_after: Option<String>,
}

impl PlacementSaveCapture {
    pub(super) fn poll(&mut self, session: &EditorAuthoringSession, requested: bool,
        retirement: Option<&mut AuthoringRetirement>) {
        if self.identity.as_ref().is_some_and(|(scene_id, revision, structure, pose)| {
            !requested || session.draft.as_ref().is_none_or(|draft| &draft.scene_id != scene_id)
                || (*revision, *structure, *pose) != (session.revision, session.placement_structure_revision,
                    session.placement_derived_pose_revision)
        }) {
            self.identity = None;
            self.preview_after = None;
        }
        if self.identity.is_none() && (!self.records.is_empty() || !self.previews.is_empty()) {
            if let Some(retirement) = retirement {
                retirement.push(std::mem::take(self)); // 取消或换代时将整份旧输入移交原后台回收槽。
            } else {
                // 独立保存夹具没有后台回收资源时也分帧释放。
                let records = self.records.len().min(RECORD_BUDGET);
                self.records.truncate(self.records.len() - records);
                self.previews.truncate(self.previews.len().saturating_sub(RECORD_BUDGET - records));
            }
        }
    }

    pub(super) fn step(&mut self, session: &EditorAuthoringSession, roots: &PlacementPreviewRoots<'_, '_>) -> bool {
        let Some(draft) = session.draft.as_ref() else { return true; };
        let source = &draft.placements.placements;
        if self.identity.is_none() {
            if !self.records.is_empty() || !self.previews.is_empty() { return false; }
            self.identity = Some((draft.scene_id.clone(), session.revision, session.placement_structure_revision,
                session.placement_derived_pose_revision));
            self.records.reserve_exact(source.len());
            self.previews.reserve_exact(session.placement_previews.len());
        }
        let start = self.records.len();
        let copied = (source.len() - start).min(RECORD_BUDGET);
        self.records.extend(source[start..start + copied].iter().cloned());
        if self.records.len() < source.len() { return false; }

        // 预览按稳定ID从上次位置继续，每项保留实体版本、姿态、父子和水版号的原有交接证据。
        let bound = self.preview_after.as_deref().map_or(Unbounded, Excluded);
        let mut previews = session.placement_previews.range::<str, _>((bound, Unbounded));
        let mut last = None;
        for (id, &root) in previews.by_ref().take(RECORD_BUDGET - copied) {
            last = Some(id);
            if let Ok((instance, transform, parent, water)) = roots.get(root)
                && let Some(snapshot) = PlacementPreviewHandoffSnapshot::from_components(
                    &draft.scene_id, id, root, instance, transform, parent, water,
                )
            {
                self.previews.push(snapshot);
            }
        }
        if let Some(last) = last {
            self.preview_after = Some(last.clone());
        }
        previews.next().is_none()
    }

    pub(super) fn take(&mut self, source: &ScenePlacementsAsset)
        -> (ScenePlacementsAsset, Vec<PlacementPreviewHandoffSnapshot>) {
        let capture = std::mem::take(self);
        (ScenePlacementsAsset {
            schema_version: source.schema_version,
            scene_id: source.scene_id.clone(),
            placements: capture.records,
        }, capture.previews)
    }
}
