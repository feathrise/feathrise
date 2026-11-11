//! 多地形地面查询：对象索引和快照后台构建，光标单飞查询，摆放采样共用真实承托层。
mod index;
mod paint_cursor;
mod resources;
mod systems;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod display_tests;
pub use index::TerrainGroundSnapshot;
pub(crate) use resources::TerrainGroundCursor;
pub(crate) use resources::CursorRay;
pub(crate) use resources::CursorIdentity;
pub(crate) use resources::CursorGroundSnapshot;
pub(crate) use systems::capture as capture_objects;
/// 验收只读取正式单飞查询事实；不投递射线、不轮询Task，也不授予点击权限。
#[cfg(feature = "validation")]
pub(crate) fn cursor_validation_snapshot(world: &bevy::prelude::World) -> serde_json::Value {
    use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState};
    let Some(cursor) = world.get_resource::<TerrainGroundCursor>() else { return serde_json::Value::Null; };
    let session = world.resource::<EditorAuthoringSession>();
    let editor = world.resource::<SceneEditorState>();
    let ground = session.draft.as_ref().and_then(|draft| draft.terrain_ground.as_ref());
    let ready = cursor.input_available
        && cursor.identity.as_ref().is_some_and(|identity| identity.revision == session.revision
            && identity.terrain_revision == session.terrain_revision
            && ground.is_some_and(|ground| ground.scene_id == identity.scene_id))
        && cursor.accepted.as_ref().is_some_and(|(snapshot, ray, hidden, hit)|
            cursor.ray == Some(*ray) && ground.is_some_and(|ground| std::sync::Arc::ptr_eq(ground, snapshot))
                && std::sync::Arc::ptr_eq(hidden, &editor.terrain_hidden)
                && hit.as_ref().is_some_and(|hit| editor.cursor_world == Some(hit.point) && hit.point.is_finite()));
    serde_json::json!({"ready":ready,"input_available":cursor.input_available,"click_ticket":cursor.click_ticket,
        "pending_click":cursor.pending_click.is_some(),"running":cursor.work.is_some(),
        "running_click":cursor.work.as_ref().is_some_and(|work|work.click),
        "running_ticket":cursor.work.as_ref().map(|work|work.click_ticket),
        "ray_epoch":cursor.ray.map(|ray|ray.epoch),"identity_revision":cursor.identity.as_ref().map(|identity|identity.revision),
        "identity_terrain_revision":cursor.identity.as_ref().map(|identity|identity.terrain_revision),
        "click_terrain":cursor.click_terrain.as_ref().map(|terrain| terrain.as_str())})
}
#[cfg(any(test, feature = "runtime_test_support"))]
pub(crate) fn cursor_for_tests(world: &mut bevy::prelude::World) {
    use bevy::ecs::system::RunSystemOnce;
    world.run_system_once(systems::cursor).unwrap(); // 定向回归消费正式单飞Collider查询与点击采用链。
}
#[cfg(test)]
pub(crate) fn hold_cursor_result_for_tests(world: &mut bevy::prelude::World) -> std::sync::mpsc::Sender<()> {
    let mut cursor = world.resource_mut::<resources::TerrainGroundCursor>();
    let work = cursor.work.take().expect("先启动真实Collider任务再设置完成屏障");
    let original = work.task;
    let (release, receiver) = std::sync::mpsc::channel();
    let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
        let _ = receiver.recv(); // 仅测试屏障延后采用；实际查询输出和冻结身份保持不变。
        original.await
    });
    cursor.work = Some(resources::CursorWork { task, ..work });
    release
}
#[cfg(test)]
pub(crate) fn cursor_running_for_tests(world: &bevy::prelude::World) -> bool {
    world.resource::<resources::TerrainGroundCursor>().work.is_some()
}
pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<resources::TerrainGroundCapture>()
        .init_resource::<super::content_bindings::ContentTerrainFollowState>()
        .init_resource::<TerrainGroundCursor>()
        .add_systems(Update, systems::capture.in_set(crate::SceneEditorSystemSet::Tasks)
            .before(super::placement_grounding::reconcile_following_placements))
        .add_systems(Update,super::content_bindings::reconcile.in_set(crate::SceneEditorSystemSet::Tasks)
            .after(systems::capture).before(crate::ui::close::finish_scene_editor_close))
        .add_systems(Update, systems::cursor.in_set(crate::SceneEditorSystemSet::Cursor)
            .after(crate::viewport::sync_editor_viewport_camera)
            .before(crate::viewport::resolve_vegetation_ground_cursor)
            .run_if(crate::scene_residency::document_active)); // 相机和真实命中同属Cursor阶段，不能反向依赖更早的Input阶段。
}
