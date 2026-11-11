//! 关闭F12保留玩家当前位置与速度；空场景、地形空隙和地下位置都交给正式玩法处理。

use super::*;
use crate::authoring::{terrain_ground::TerrainGroundSnapshot, test_heightfield_draft};
use avian3d::prelude::LinearVelocity;
use feathrise_scene_content::ActiveDynamicScene;
use std::sync::Arc;

#[derive(Component)]
struct WorldActor; // 普通关闭门只保证世界实体原位保留，不绑定原游戏的玩家组件。

#[test]
fn unsupported_player_position_closes_f12_without_teleport_or_confirmation() {
    for has_ground_snapshot in [false, true] {
        let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
        let map = draft.scene_id.clone();
        draft.manifest.terrains = Some(Default::default());
        if has_ground_snapshot {
            draft.terrain_ground = Some(TerrainGroundSnapshot::empty(map.clone(), 0));
        }
        let mut app = App::new();
        app.insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
            .insert_resource(SceneEditorState { open: true, close_requested: true, ..default() })
            .insert_resource(ActiveDynamicScene { scene_id: Some(map) })
            .insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
            .init_resource::<TerrainBrushStroke>()
            .init_resource::<WaterBrushStroke>()
            .init_resource::<TerrainLiveApplyState>()
            .init_resource::<WaterPreviewState>()
            .init_resource::<WaterFileHandoff>()
            .init_resource::<BoundaryEditState>()
            .init_resource::<WaterNetworkEditState>()
            .init_resource::<SceneCreationTask>()
            .init_resource::<SceneDeletionTask>()
            .add_systems(Update, finish_scene_editor_close);
        let position = Transform::from_xyz(6.98, -30.52, -1.24);
        let velocity = LinearVelocity(Vec3::new(0.0, -7.0, 0.0));
        let player = app.world_mut().spawn((WorldActor, position, velocity)).id();
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert!(!editor.open && !editor.close_requested);
        assert!(!editor.status.contains("没有真实承托面"));
        assert_eq!(*app.world().get::<Transform>(player).unwrap(), position);
        assert_eq!(app.world().get::<LinearVelocity>(player).unwrap().0, velocity.0);
        assert!(app.world().resource::<EditorAuthoringSession>().draft.is_some());
    }
}
