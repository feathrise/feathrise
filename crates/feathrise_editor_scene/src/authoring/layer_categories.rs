//! 场景级分类状态的 SDK 接口，统一用于正文按钮、历史与作者预览。

use bevy::{ecs::system::SystemParam, prelude::{Assets, Local, Mut, Res, Visibility, World}};
use feathrise_scene_content::{SceneContentRegistry, SceneId, SceneManifestAsset, TerrainId};
use std::{collections::BTreeSet, sync::Arc};

use super::{EditorAuthoringSession, EditorSceneDraft};
use crate::{resources::SceneEditorState, ui::scene_layers::{LayerCategoryVisibility, WaterLayerEyes}};

// 公开编辑器可继续编译在旧 SDK 上；固有方法可用时由 Rust 优先解析。
#[allow(dead_code)]
trait LegacyLayerCategories {
    fn supports_layer_categories(&self) -> bool { false }
    fn layer_category_state(&self, _vegetation: bool) -> (bool, bool) { (true, false) }
    fn set_layer_category_state(&mut self, _vegetation: bool, _enabled: bool, _locked: bool) {}
    fn set_layer_preview_state(
        _world: &mut World, _scene: Option<SceneId>, _water: (bool, bool), _vegetation: (bool, bool),
    ) {}
    fn supports_still_water_layers(&self) -> bool { false }
    fn still_water_layer_state(&self, _terrain: Option<&TerrainId>, _id: &str) -> (bool, bool) { (true, false) }
    fn still_water_disabled_layers(&self, _terrain: Option<&TerrainId>) -> Option<&Arc<BTreeSet<String>>> { None }
    fn set_still_water_layer_state(&mut self, _terrain: Option<&TerrainId>, _id: &str, _enabled: bool, _locked: bool) {}
    fn set_still_water_preview_state(_world: &mut World, _draft: Option<&Self>, _hidden: WaterLayerEyes) {}
}

impl LegacyLayerCategories for SceneManifestAsset {}

pub(crate) fn supported(draft: &EditorSceneDraft) -> bool {
    draft.manifest.supports_layer_categories()
}

/// 顺序为参与、锁定。分类覆盖与各成员原有设置独立。
pub(crate) fn category_state(draft: &EditorSceneDraft, vegetation: bool) -> (bool, bool) {
    draft.manifest.layer_category_state(vegetation)
}

pub(crate) fn set_state(draft: &mut EditorSceneDraft, vegetation: bool, state: (bool, bool)) -> Result<(), String> {
    if !supported(draft) { return Err("当前 SDK 不支持场景分类参与和锁定".into()); }
    draft.manifest.set_layer_category_state(vegetation, state.0, state.1);
    Ok(())
}

pub(crate) fn water_layer_supported(draft: &EditorSceneDraft) -> bool {
    draft.manifest.supports_still_water_layers()
}

/// 水体控制属于场景根清单，活动和停放的私有水源都用同一完整身份。
pub(crate) fn water_layer_state(draft: &EditorSceneDraft, terrain: Option<&TerrainId>, id: &str) -> (bool, bool) {
    draft.manifest.still_water_layer_state(terrain, id)
}

pub(crate) fn set_water_layer_state(draft: &mut EditorSceneDraft, terrain: Option<&TerrainId>, id: &str, state: (bool, bool)) {
    draft.manifest.set_still_water_layer_state(terrain, id, state.0, state.1);
}

pub(crate) fn water_layer_edit_error(draft: &EditorSceneDraft, terrain: Option<&TerrainId>, id: &str) -> Option<&'static str> {
    edit_error(draft, false).or_else(|| match water_layer_state(draft, terrain, id) {
        (false, _) => Some("请先启用该静水体再编辑"),
        (true, true) => Some("请先解锁该静水体再编辑"),
        (true, false) => None,
    })
}

/// 参与和锁定属于写侧，眼睛的临时预览状态独立于作者事务。
pub(crate) fn edit_error(draft: &EditorSceneDraft, vegetation: bool) -> Option<&'static str> {
    match (vegetation, category_state(draft, vegetation)) {
        (true, (false, _)) => Some("请先启用植被分类再编辑"),
        (false, (false, _)) => Some("请先启用水域分类再编辑"),
        (true, (true, true)) => Some("请先解锁植被分类再编辑"),
        (false, (true, true)) => Some("请先解锁水域分类再编辑"),
        (_, (true, false)) => None,
    }
}

/// 作者辅助覆盖与正式预览使用同一可见条件，保留图层各自的显隐。
pub(crate) fn preview_visible(
    draft: &EditorSceneDraft, visibility: Option<&LayerCategoryVisibility>, vegetation: bool,
) -> bool {
    category_state(draft, vegetation).0
        && !visibility.is_some_and(|visibility| visibility.hidden(&draft.scene_id, vegetation))
}

pub(crate) fn water_layer_visible(draft: &EditorSceneDraft, visibility: Option<&LayerCategoryVisibility>, terrain: Option<&TerrainId>, id: &str) -> bool {
    water_layer_state(draft, terrain, id).0
        && !visibility.is_some_and(|visibility| visibility.water_hidden(&draft.scene_id, terrain, id))
}

#[derive(SystemParam)]
pub(crate) struct LayerPreviewVisibility<'w> {
    eyes: Option<Res<'w, LayerCategoryVisibility>>,
    catalog: Option<Res<'w, SceneContentRegistry>>,
    manifests: Option<Res<'w, Assets<SceneManifestAsset>>>,
}

impl LayerPreviewVisibility<'_> {
    fn loaded_manifest(&self, scene: &SceneId) -> Option<&SceneManifestAsset> {
        let handles = self.catalog.as_deref()?.handles(scene)?;
        self.manifests.as_deref()?.get(&handles.manifest).filter(|manifest| &manifest.id == scene)
    }

    /// 替换批可能保留到 F12 关闭后，此时与正式运行时一起读取已加载清单。
    pub(crate) fn for_root(&self, editor_open: bool, draft: Option<&EditorSceneDraft>, vegetation: bool) -> Visibility {
        let visible = draft.is_none_or(|draft| {
            if editor_open { return preview_visible(draft, self.eyes.as_deref(), vegetation); }
            self.loaded_manifest(&draft.scene_id)
                .is_none_or(|manifest| manifest.layer_category_state(vegetation).0)
        });
        if visible { Visibility::Inherited } else { Visibility::Hidden }
    }

    /// 编辑器自有替换水面应用同一来源的单层显隐。
    pub(crate) fn still_water_visible(&self, editor_open: bool, draft: Option<&EditorSceneDraft>, terrain: Option<&TerrainId>, id: &str) -> bool {
        draft.is_none_or(|draft| if editor_open {
            water_layer_visible(draft, self.eyes.as_deref(), terrain, id)
        } else {
            self.loaded_manifest(&draft.scene_id).is_none_or(|manifest| manifest.still_water_layer_state(terrain, id).0)
        })
    }

    pub(crate) fn still_water_inputs<'a>(&'a self, editor_open: bool, draft: Option<&'a EditorSceneDraft>, terrain: Option<&TerrainId>)
        -> (Option<&'a Arc<BTreeSet<String>>>, Option<&'a WaterLayerEyes>)
    {
        let Some(draft) = draft else { return (None, None); };
        if editor_open {
            (draft.manifest.still_water_disabled_layers(terrain), self.eyes.as_deref().and_then(|eyes| eyes.water_eyes(&draft.scene_id)))
        } else {
            (self.loaded_manifest(&draft.scene_id).and_then(|manifest| manifest.still_water_disabled_layers(terrain)), None)
        }
    }
}

#[derive(Default)]
pub(crate) struct StillWaterPreviewBridge {
    applied: Option<(Option<SceneId>, u64, u64)>,
    empty: WaterLayerEyes,
}

pub(crate) fn sync_still_water_preview(world: &mut World, mut bridge: Local<StillWaterPreviewBridge>) {
    let (scene, revision) = (|| {
        let editor = world.get_resource::<SceneEditorState>()?;
        let session = world.get_resource::<EditorAuthoringSession>()?;
        let draft = session.draft.as_ref()?;
        (editor.open && !editor.scene_transition_in_progress && editor.selected_scene_id.as_ref() == Some(&draft.scene_id))
            .then_some((Some(&draft.scene_id), session.revision))
    })().unwrap_or((None, 0));
    let visibility = world.get_resource::<LayerCategoryVisibility>();
    let eye_revision = visibility.map_or(0, LayerCategoryVisibility::water_revision);
    if bridge.applied.as_ref().map(|(scene, revision, eyes)| (scene.as_ref(), *revision, *eyes))
        == Some((scene, revision, eye_revision)) { return; }
    let scene = scene.cloned();
    let eyes = scene.as_ref().and_then(|scene| visibility?.water_eyes(scene)).unwrap_or(&bridge.empty).clone();
    if scene.is_some() {
        // 借出草稿资源后直接交接清单的 Arc。清单本身和完整水域数据均无需复制。
        world.resource_scope(|world, session: Mut<EditorAuthoringSession>| {
            SceneManifestAsset::set_still_water_preview_state(world, session.draft.as_ref().map(|draft| &draft.manifest), eyes);
        });
    } else {
        SceneManifestAsset::set_still_water_preview_state(world, None, eyes);
    }
    bridge.applied = Some((scene, revision, eye_revision));
}

type PreviewState = (Option<SceneId>, (bool, bool), (bool, bool));

/// 只发布当前编辑场景的预览覆盖。窗口关闭或场景交接后，由 SDK 恢复正式状态。
pub(crate) fn sync_layer_category_preview(world: &mut World, mut applied: Local<Option<PreviewState>>) {
    let current = (|| {
        let editor = world.get_resource::<SceneEditorState>()?;
        let draft = world.get_resource::<EditorAuthoringSession>()?.draft.as_ref()?;
        if !editor.open || editor.scene_transition_in_progress
            || editor.selected_scene_id.as_ref() != Some(&draft.scene_id) { return None; }
        let visibility = world.get_resource::<LayerCategoryVisibility>();
        let state = |vegetation| (
            category_state(draft, vegetation).0,
            !visibility.is_some_and(|visibility| visibility.hidden(&draft.scene_id, vegetation)),
        );
        Some((Some(&draft.scene_id), state(false), state(true)))
    })().unwrap_or((None, (true, true), (true, true)));

    if applied.as_ref().map(|(scene, water, vegetation)| (scene.as_ref(), *water, *vegetation)) == Some(current) {
        return;
    }
    let next = (current.0.cloned(), current.1, current.2);
    SceneManifestAsset::set_layer_preview_state(world, next.0.clone(), next.1, next.2);
    *applied = Some(next);
}
