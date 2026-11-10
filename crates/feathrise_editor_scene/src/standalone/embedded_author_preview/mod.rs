//! 原工具已采用的局部显示结果：后台编码、每次仅一域、真实ACK后推进来源版本。

mod resources;
mod poses;
mod source;
mod systems;
#[cfg(test)]
mod tests;
use bevy::prelude::*;
pub use resources::{EmbeddedAuthorPreview, EmbeddedAuthorPreviewState};
pub use resources::{EmbeddedExternalAuthorVisuals,ExternalAuthorVisual};

pub struct AuthorPreviewPlugin;
impl Plugin for AuthorPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::authoring::AuthoringRetirement>()
            .init_resource::<EmbeddedExternalAuthorVisuals>()
            .init_resource::<EmbeddedAuthorPreviewState>()
            .add_systems(
                Update,
                systems::advance.after(crate::SceneEditorSystemSet::Tasks),
            );
    }
}

pub fn invalidate(world: &mut World) {
    let retired = if let Some(mut state) = world.get_resource_mut::<EmbeddedAuthorPreviewState>() {
        // 完整快照只撤销来源采用资格，不能重置同一 IPC 会话的域序号。
        // ready 必须当场撤下，避免宿主在本帧再次送出已失效的候选；在途任务仍占唯一槽。
        state.identity = None;
        state.failed = None;
        state.error = None;
        state.pending = state.task.is_some();
        Some((state.offered.take(), state.ready.take()))
    } else {
        None
    };
    if let Some(retired) = retired {
        world
            .resource_mut::<crate::authoring::AuthoringRetirement>()
            .push(retired);
    }
}

/// 登记实际发送的完整快照；最新 ready 不能替另一个在途版本提供对象存在信用。
pub fn snapshot_sent(world: &mut World, session: u64, generation: u64, revision: u64,
    scene_id: feathrise_scene_content::SceneId,
    targets: std::sync::Arc<super::embedded_snapshot::EmbeddedPoseTargets>) {
    let Some(mut state) = world.get_resource_mut::<EmbeddedAuthorPreviewState>() else { return; };
    // 宿主可在本帧发送完整快照时推进场景代次，不能等下一帧 configure 才承认这次在途身份。
    state.session = session;
    state.generation = generation;
    let old = state.pending_pose_targets.replace(resources::PoseTargetCredit {
        session, generation, revision, scene_id, targets,
    });
    world.resource_mut::<crate::authoring::AuthoringRetirement>().push(old);
}

/// 只接受同会话、同代次、同完整版本的真实回执；旧进程和旧图永远不能开放新 ID。
pub fn snapshot_applied(world: &mut World, session: u64, generation: u64, revision: u64) -> bool {
    let Some(mut state) = world.get_resource_mut::<EmbeddedAuthorPreviewState>() else { return false; };
    if !state.enabled || state.session != session || state.generation != generation
        || state.pending_pose_targets.as_ref().is_none_or(|value|
        value.session != session || value.generation != generation || value.revision != revision) {
        return false;
    }
    let pending = state.pending_pose_targets.take();
    let old = std::mem::replace(&mut state.applied_pose_targets, pending);
    world.resource_mut::<crate::authoring::AuthoringRetirement>().push(old);
    true
}

/// 显式验收只读取原可靠槽的实际信用；已排队/已发送的内容不能代替同身份版本ACK。
#[cfg(feature = "validation")]
pub fn external_acknowledgement_for_validation(world: &World) -> Option<serde_json::Value> {
    use resources::{Key, Version};
    let state = world.get_resource::<EmbeddedAuthorPreviewState>()?;
    let identity = state.identity.as_ref()?;
    let source = world.get_resource::<EmbeddedExternalAuthorVisuals>()?;
    let fact = |value: &Option<ExternalAuthorVisual>, slot| {
        value.as_ref().map(|value| {
            let acknowledged = state.enabled
                && state.session == identity.session
                && state.generation == identity.generation
                && value.scene_id == identity.scene_id
                && state.acknowledged.get(&Key::Extension(slot))
                    == Some(&Version {
                        revision: value.revision,
                        present: true,
                    });
            serde_json::json!({"scene_id":value.scene_id.as_str(),"revision":value.revision,
                "domain":value.domain,"acknowledged":acknowledged})
        })
    };
    Some(serde_json::json!({"scene_id":identity.scene_id.as_str(),"session":identity.session,
        "generation":identity.generation,"epoch":identity.epoch,
        "enabled":state.enabled,"pending":state.pending,"error":state.error,
        "acknowledged_sequence":state.acknowledged_sequence,
        "content":fact(&source.content,6),"pose":fact(&source.pose,7)}))
}
