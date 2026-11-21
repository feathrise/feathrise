//! 默认正式加载跳过停用摆放。这里在作者改回参与时补齐缺失的可视根，并清理已停用的预览。

use crate::{
    authoring::{
        EditorSceneDraft, attach_patch_preview, placement_preview_ownership, spawn_placement_preview,
    },
    components::EditorPlacedAsset,
    ui::placement_water::PlacementInstanceIndex,
};
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_scene::{ScenePlacementInstance, TerrainObjectQueries};
use feathrise_scene_content::ScenePlacementRecord;
use std::collections::BTreeMap;

use super::visibility::{ObjectVisibilityState, restore_placement_base_visibility};

// 与既有多选作者预览绑定预算一致，元数据仍使用原来的独立写回预算。
pub(super) const PREVIEW_CHANGES_PER_FRAME: usize = 16;

#[derive(SystemParam)]
pub(super) struct ParticipationPreviews<'w, 's> {
    commands: Commands<'w, 's>,
    assets: Option<Res<'w, AssetServer>>,
    instances: Option<Res<'w, PlacementInstanceIndex>>,
    roots: Query<'w, 's, (&'static ScenePlacementInstance, Has<EditorPlacedAsset>)>,
    terrain: TerrainObjectQueries<'w, 's>,
    visibility: ResMut<'w, ObjectVisibilityState>,
}

impl ParticipationPreviews<'_, '_> {
    pub(super) fn apply(
        &mut self,
        draft: &EditorSceneDraft,
        record: &ScenePlacementRecord,
        enabled: bool,
        previews: &mut BTreeMap<String, Entity>,
        remaining: &mut usize,
    ) -> bool {
        let scene = &draft.scene_id;
        let owned = previews.get(&record.id).copied();
        let owns = |root| self.roots.get(root).is_ok_and(|(instance, author)| {
            author && instance.scene_id() == scene && instance.id() == record.id
        });
        let candidates = self.instances.as_ref().map_or(&[][..], |index| index.roots_for(scene, &record.id));
        if !enabled {
            if let Some(root) = owned.filter(|&root| owns(root)) {
                if *remaining == 0 { return false; }
                *remaining -= 1;
                // 保存不会为停用条目生成正式根。先撤回这个准确的作者拥有者，存档便不会等不存在的交接。
                placement_preview_ownership::withdraw_cold_preview(&mut self.commands, root);
                self.commands.entity(root).try_despawn();
            }
            previews.remove(&record.id);
        } else if !owned.is_some_and(owns) && !candidates.iter().any(|&root| {
            self.roots.get(root).is_ok_and(|(instance, author)| !author && instance.scene_id() == scene
                && instance.id() == record.id && (instance.is_active() || instance.is_pending()))
        }) && let (Some(assets), Some(_)) = (self.assets.as_deref(), self.instances.as_deref()) {
            if *remaining == 0 { return false; }
            *remaining -= 1;
            let root = spawn_placement_preview(&mut self.commands, assets, scene, record);
            if attach_patch_preview(&mut self.commands, draft, &self.terrain, root, record).is_ok() {
                previews.insert(record.id.clone(), root);
                placement_preview_ownership::claim_preview(&mut self.commands, scene, &record.id, root);
                if record.editor.hidden {
                    // 重启参与也保留眼睛的选择，根落地时即应用已有的显隐合成。
                    self.commands.queue(move |world: &mut World| {
                        restore_placement_base_visibility(world, root, Visibility::Inherited);
                    });
                }
            } else {
                // 来源还没有可用的地形父级时保留有效的场景记录，由原正式加载应用它。
                self.commands.entity(root).try_despawn();
            }
        }
        self.visibility.prioritize_participation(candidates);
        true
    }
}
