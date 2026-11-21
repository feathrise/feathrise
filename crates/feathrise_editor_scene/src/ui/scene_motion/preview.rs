//! 同一播放头驱动场景中的已插帧对象。预览会在切页时按原根身份还原临时变换。

use bevy::prelude::*;
use feathrise_scene::ScenePlacementInstance;
use feathrise_scene_content::{SceneId, ScenePlacementAssetTarget, ScenePlacementRecord};
use crate::{
    authoring::{EditorAuthoringSession, scene_motion::{SceneMotionCollider, SceneMotionPlayback, SceneMotionTrack}},
    resources::SceneEditorState,
    ui::placement_water::PlacementInstanceIndex,
    viewport::SelectedPlacementViewportState,
};
use super::{Context, SceneAnimationFramePose, Target, Workbench};

#[derive(Clone, Copy)]
pub(super) struct PreviewPose {
    pub(super) entity: Entity,
    parent: Option<Entity>,
    source: Transform,
    applied: Transform,
}

type Roots<'w, 's> = Query<'w, 's, (
    &'static ScenePlacementInstance, &'static mut Transform, Option<&'static ChildOf>,
    &'static InheritedVisibility,
)>;

impl PreviewPose {
    fn restore(self, roots: &mut Roots) {
        if let Ok((_, mut transform, parent, _)) = roots.get_mut(self.entity)
            && parent.map(ChildOf::parent) == self.parent
        {
            // 各分量独立归还，保留协作热更新或作者系统已经接管的值。
            if transform.translation == self.applied.translation {
                transform.translation = self.source.translation;
            }
            if transform.rotation == self.applied.rotation {
                transform.rotation = self.source.rotation;
            }
        }
    }
}

/// 保留原有网格碰撞的参与意图。需要承托的普通立方体用实际尺寸，其余资产复用作者世界包围盒。
pub(super) fn initial_collision(
    record: &ScenePlacementRecord, scene: &SceneId, bounds: &SelectedPlacementViewportState,
) -> Option<Option<SceneMotionCollider>> {
    let collision = match record.asset_target {
        ScenePlacementAssetTarget::Cuboid { collision, .. }
        | ScenePlacementAssetTarget::EditableCuboid { collision, .. }
        | ScenePlacementAssetTarget::EditableMesh { collision, .. } => collision,
        _ => record.asset_target.has_gltf_collision(),
    };
    if !collision { return Some(None); }
    let (center, half_extents) = if let ScenePlacementAssetTarget::Cuboid { size_mm, .. } = record.asset_target {
        let size = Vec3::from_array(size_mm.map(|size| size as f32 / 1_000.0));
        (Vec3::ZERO, size * Vec3::from_array(record.scale).abs() * 0.5)
    } else {
        let (world_center, size) = bounds.whole_instance_bounds(scene, &record.id)?;
        let inverse = Quat::from_rotation_y(-record.yaw_degrees.to_radians());
        let half = size * 0.5;
        let extent = (inverse * Vec3::X).abs() * half.x
            + (inverse * Vec3::Y).abs() * half.y + (inverse * Vec3::Z).abs() * half.z;
        (inverse * (world_center - Vec3::from_array(record.translation)), extent.max(Vec3::splat(0.001)))
    };
    Some(Some(SceneMotionCollider { center: center.to_array(), half_extents: half_extents.to_array() }))
}

fn sample(track: Option<&SceneMotionTrack>, seconds: f64, record: &ScenePlacementRecord) -> SceneAnimationFramePose {
    let base = SceneAnimationFramePose::base(record);
    let Some(track) = track else { return base; };
    let (translation, rotation_degrees) = track.sample(seconds, base.translation, base.rotation_degrees);
    SceneAnimationFramePose { translation, rotation_degrees }
}

fn publish(
    record: &ScenePlacementRecord, scene: &SceneId, pose: SceneAnimationFramePose,
    session: &EditorAuthoringSession, index: &PlacementInstanceIndex, roots: &mut Roots,
    state: &mut Workbench,
) -> Option<PreviewPose> {
    let root = session.placement_previews.get(&record.id).copied().into_iter()
        .chain(index.roots_for(scene, &record.id).iter().copied()).find(|&entity| roots.get(entity).is_ok_and(
            |(instance, _, _, inherited)| instance.is_active() && instance.scene_id() == scene
                && instance.id() == record.id && inherited.get()))?;
    let (_, mut transform, parent, _) = roots.get_mut(root).ok()?;
    let parent = parent.map(ChildOf::parent);
    let previous = state.previews.remove(&root).filter(|pose| pose.parent == parent);
    let mut source = *transform;
    if let Some(previous) = previous {
        if source.translation == previous.applied.translation { source.translation = previous.source.translation; }
        if source.rotation == previous.applied.rotation { source.rotation = previous.source.rotation; }
    }
    let base = SceneAnimationFramePose::base(record);
    let [x, y, z] = pose.rotation_degrees.map(f32::to_radians);
    let rotation = Quat::from_euler(EulerRot::XYZ, x, y, z);
    let base_yaw = Quat::from_rotation_y(record.yaw_degrees.to_radians());
    // 正式根和作者预览根可以有各自的父变换。只叠加相对作者记录的变化，保留现有层级来源。
    let applied = Transform {
        translation: source.translation + (Vec3::from_array(pose.translation) - Vec3::from_array(base.translation)),
        rotation: (source.rotation * base_yaw.inverse() * rotation).normalize(),
        scale: source.scale,
    };
    if !applied.translation.is_finite() || !applied.rotation.is_finite() {
        if let Some(previous) = previous { state.previews.insert(root, previous); }
        return None;
    }
    if transform.translation != applied.translation { transform.translation = applied.translation; }
    if transform.rotation != applied.rotation { transform.rotation = applied.rotation; }
    let pose = PreviewPose { entity: root, parent, source, applied };
    state.current_previews.insert(root, pose);
    Some(pose)
}

pub(super) fn apply(
    context: Context, editor: Res<SceneEditorState>, session: Res<EditorAuthoringSession>,
    mut workbench: ResMut<Workbench>, time: Res<Time<Real>>, index: Res<PlacementInstanceIndex>,
    mut roots: Roots,
) {
    let scene = context.scene(&editor, &session).filter(|scene| workbench.scene.as_ref() == Some(*scene));
    let Some((scene, motion)) = scene.and_then(|scene| Some((scene, &session.draft.as_ref()?.motion))) else {
        workbench.close();
        for (_, pose) in workbench.previews.drain() { pose.restore(&mut roots); }
        workbench.pose = None;
        return;
    };
    let selected = context.record(&editor, &session).map(|(_, record)| record);
    let motion_ready = motion.tracks().next().is_some();
    if !context.moving.owns_mouse() { workbench.target(selected.map(|record| Target::new(scene, record))); }

    if motion_ready && workbench.playing {
        // 离屏编辑器暂停玩法时钟，作者播放沿用角色时间线的真实帧钟。
        workbench.elapsed += time.delta_secs_f64();
        let timing = motion.timing();
        workbench.seconds = timing.playback.time(workbench.elapsed, timing.duration_seconds);
        if timing.playback == SceneMotionPlayback::Once && workbench.seconds >= f64::from(timing.duration_seconds) {
            workbench.playing = false;
        }
    }
    let result = selected.filter(|_| motion_ready).map(|record| sample(motion.get(&record.id), workbench.seconds, record));
    workbench.sampled = result;
    let selected_id = selected.map(|record| record.id.as_str());
    for track in motion.tracks() {
        if track.position.is_empty() && track.rotation.is_empty() || selected_id == Some(track.placement_id.as_str()) { continue; }
        let Some(record) = context.placement(&session, &track.placement_id) else { continue; };
        let pose = sample(Some(track), workbench.seconds, record);
        publish(record, scene, pose, &session, &index, &mut roots, &mut workbench);
    }
    workbench.pose = selected.and_then(|record| {
        let pose = context.moving.animation_pose(scene, &record.id).or_else(|| workbench.pose_for(record, scene))?;
        publish(record, scene, pose, &session, &index, &mut roots, &mut workbench)
    });
    for (_, pose) in workbench.previews.drain() { pose.restore(&mut roots); }
    let state = &mut *workbench;
    std::mem::swap(&mut state.previews, &mut state.current_previews);
}
