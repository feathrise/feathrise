//! F12离屏相机对正式GPU叶片视图的临时接管。
//!
//! 【功能设计】编辑器打开时把现有玩法相机的`GpuBladeView`原值搬到离屏相机；关闭时原样归还。
//! 全屏期间始终只有一个正式叶片视图，因此HN-01现有窗口跟随、四级LOD、风场和RenderApp路径全部复用。

use bevy::prelude::*; // Commands、Entity、Query和Resource。
use feathrise_environment::vegetation::{VegetationMeshBatchView, blade::GpuBladeView};

use crate::components::SceneEditorCamera;
use crate::resources::SceneEditorState;

/// 保存被F12暂时移除的玩法相机身份与完整LOD配置。
#[derive(Resource, Default)]
pub(crate) struct EditorBladeViewHandoff {
    owner: Option<Entity>,
    value: Option<GpuBladeView>,
    mesh_batch_view: bool, // 动态场景Mesh批次必须与Blade使用完全相同的唯一观察相机。
}

/// 保持唯一`GpuBladeView`并处理切换场景时旧玩法相机被销毁、新玩法相机出现的情况。
pub(crate) fn handoff_gpu_blade_view(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    mut handoff: ResMut<EditorBladeViewHandoff>,
    editor_camera: Query<
        (Entity, Option<&GpuBladeView>, Has<VegetationMeshBatchView>),
        With<SceneEditorCamera>,
    >,
    gameplay_views: Query<
        (Entity, &GpuBladeView, Has<VegetationMeshBatchView>),
        Without<SceneEditorCamera>,
    >,
    all_entities: Query<(), ()>,
) {
    let editor_camera = editor_camera.single().ok();
    if editor.open {
        let Some((editor_entity, editor_view, _)) = editor_camera else { return; };
        // 切换场景后新玩法相机可能在F12重新打开前出现；优先接管它的新场景规格。
        if let Some((owner, value, mesh_batch_view)) = gameplay_views.iter().next() {
            handoff.owner = Some(owner);
            handoff.value = Some(*value);
            handoff.mesh_batch_view = mesh_batch_view;
            commands.entity(owner).remove::<GpuBladeView>();
            commands.entity(owner).remove::<VegetationMeshBatchView>();
            commands.entity(editor_entity).insert(*value);
            if mesh_batch_view {
                commands
                    .entity(editor_entity)
                    .insert(VegetationMeshBatchView);
            }
        } else if editor_view.is_none()
            && let Some(value) = handoff.value
        {
            commands.entity(editor_entity).insert(value);
            if handoff.mesh_batch_view {
                commands
                    .entity(editor_entity)
                    .insert(VegetationMeshBatchView);
            }
        }
        return;
    }
    // 加载/切换场景可先回收RTT相机；关闭时仍必须归还原玩法视图，不能依赖编辑相机继续存在。
    if let Some((editor_entity, editor_view, editor_mesh_batch_view)) = editor_camera {
        if editor_view.is_some() { commands.entity(editor_entity).remove::<GpuBladeView>(); }
        if editor_mesh_batch_view { commands.entity(editor_entity).remove::<VegetationMeshBatchView>(); }
    }
    if let (Some(owner), Some(value)) = (handoff.owner.take(), handoff.value.take())
        && all_entities.get(owner).is_ok()
    {
        commands.entity(owner).insert(value);
        if handoff.mesh_batch_view {
            commands.entity(owner).insert(VegetationMeshBatchView);
        }
    }
    handoff.mesh_batch_view = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_resource_starts_without_fabricated_view() {
        let state = EditorBladeViewHandoff::default();
        assert!(state.owner.is_none());
        assert!(state.value.is_none());
        assert!(!state.mesh_batch_view);
    }

    #[test]
    fn closing_after_editor_camera_retirement_restores_gameplay_vegetation_view() {
        let mut app = App::new();
        app.insert_resource(SceneEditorState { open: true, ..default() })
            .init_resource::<EditorBladeViewHandoff>()
            .add_systems(Update, handoff_gpu_blade_view);
        let view = GpuBladeView::default();
        let gameplay = app.world_mut().spawn((view, VegetationMeshBatchView)).id();
        let editor_camera = app.world_mut().spawn(SceneEditorCamera).id();
        app.update();
        assert!(app.world().get::<GpuBladeView>(gameplay).is_none());
        assert!(app.world().get::<GpuBladeView>(editor_camera).is_some());
        app.world_mut().despawn(editor_camera); // 载入/切换场景期间RTT可以先于F12关闭收口被回收。
        app.world_mut().resource_mut::<SceneEditorState>().open = false;
        app.update();
        assert_eq!(app.world().get::<GpuBladeView>(gameplay), Some(&view), "不能因编辑相机缺席而丢失玩法草地观察视图");
        assert!(app.world().get::<VegetationMeshBatchView>(gameplay).is_some());
    }
}
