//! 非当前玩法场景的空气墙关闭校验：只校验草稿，不等待另一个场景的物理Ready。
use crate::{authoring::EditorSceneDraft, resources::SceneEditorState};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use feathrise_scene_content::{ActiveDynamicScene, SceneBoundaryCapture, SceneId};
use feathrise_scene::DynamicSceneBoundaryOverride;
use feathrise_scene_core::state::ScenePhase;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
struct Identity {
    scene_id: SceneId,
    revision: u64,
    bounds: Rect,
}

#[derive(Default)]
pub(super) struct BoundaryCloseValidation {
    identity: Option<Identity>,
    capture: SceneBoundaryCapture,
    task: Option<Task<(Identity, Result<(), String>)>>, // 取消关闭仍保留CPU槽，直至真正完成。
    ready: Option<(Identity, Result<(), String>)>,
}

pub(super) fn is_runtime_scene(
    active: Option<&ActiveDynamicScene>,
    scene: Option<&ScenePhase>,
    map: &SceneId,
) -> bool {
    scene == Some(&ScenePhase::Active)
        && active.and_then(|active| active.scene_id.as_ref()) == Some(map)
}

impl BoundaryCloseValidation {
    /// 返回true表示仍需等待或已取消关闭；结果身份完全匹配才允许关闭。
    pub(super) fn blocks_close(
        &mut self,
        editor: &mut SceneEditorState,
        draft: &EditorSceneDraft,
        revision: u64,
        source: Option<&DynamicSceneBoundaryOverride>,
    ) -> bool {
        let Some(bounds) = draft.manifest.world_bounds() else {
            editor.cancel_close_request("空气墙场景范围无效，本次关闭已取消");
            return true;
        };
        let identity = Identity {
            scene_id: draft.scene_id.clone(),
            revision,
            bounds,
        };
        if self.identity.as_ref() != Some(&identity) {
            self.identity = Some(identity.clone());
            self.capture = default();
            self.ready = None; // 场景/修订/范围变化都不能复用旧校验。
        }
        if let Some(result) = self.task.as_mut().and_then(check_ready) {
            self.task = None;
            if result.0 == identity {
                self.ready = Some(result);
            }
        }
        if let Some((_, result)) = self.ready.as_ref() {
            return match result {
                Ok(()) => false,
                Err(error) => {
                    editor
                        .cancel_close_request(format!("空气墙校验未通过，本次关闭已取消：{error}"));
                    true
                }
            };
        }
        editor.status = "正在后台校验所编辑场景的空气墙；完成后自动关闭编辑器".to_owned();
        if self.task.is_some() {
            return true;
        }
        let document = if let Some(shared) = source
            .filter(|source| {
                source.scene_id.as_ref() == Some(&draft.scene_id) && source.revision == revision
            })
            .and_then(|source| source.document.as_ref())
        {
            Arc::clone(shared) // 已完整发布的同修订草稿直接共享。
        } else {
            let Some(document) = draft.boundaries.as_ref() else {
                return false;
            };
            if !self.capture.step(document, 4096, 64) {
                return true;
            }
            let Some(document) = self.capture.take() else {
                return true;
            };
            Arc::new(document) // 无覆盖时分帧冻结后交付worker。
        };
        self.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let result = document.validate(&identity.scene_id, identity.bounds);
            (identity, result)
        }));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_scene_background_validation_finishes_without_runtime_status() {
        use feathrise_scene_content::{SceneBoundariesAsset, SceneBoundaryRole};
        use std::time::{Duration, Instant};
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut document = SceneBoundariesAsset::rectangular_default(
            &draft.scene_id,
            draft.manifest.world_min,
            draft.manifest.world_max,
            -10.0,
            100.0,
        )
        .unwrap();
        document.paths[0].role = SceneBoundaryRole::Internal;
        document.paths[0].closed = false;
        document.paths[0].points.truncate(2);
        draft.boundaries = Some(document);
        let mut editor = SceneEditorState {
            open: true,
            ..default()
        };
        editor.request_close();
        let mut validation = BoundaryCloseValidation::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while validation.blocks_close(&mut editor, &draft, 7, None) {
            assert!(editor.close_requested, "合法内部挡墙不能取消关闭");
            assert!(
                Instant::now() < deadline,
                "非当前场景不应永远等待另一个场景的Ready"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        draft.boundaries.as_mut().unwrap().paths[0].role = SceneBoundaryRole::Outer;
        let deadline = Instant::now() + Duration::from_secs(2);
        while editor.close_requested {
            assert!(validation.blocks_close(&mut editor, &draft, 8, None));
            assert!(Instant::now() < deadline, "不完整外框必须返回明确错误");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(editor.open); // 外图仍执行严格校验，错误恢复编辑。
    }
    #[test]
    fn only_matching_active_dynamic_scene_requires_physics_ready() {
        let map = SceneId::parse("MAP-current").unwrap();
        let other = SceneId::parse("MAP-other").unwrap();
        let active = ActiveDynamicScene {
            scene_id: Some(map.clone()),
        };
        assert!(is_runtime_scene(
            Some(&active),
            Some(&ScenePhase::Active),
            &map
        ));
        assert!(!is_runtime_scene(
            Some(&active),
            Some(&ScenePhase::Active),
            &other
        ));
        assert!(!is_runtime_scene(Some(&active), None, &map)); // 菜单里没有可等待的场景物理。
        assert!(!is_runtime_scene(
            None,
            Some(&ScenePhase::Active),
            &map
        ));
    }
}
