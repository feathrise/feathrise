//! 用与蒙皮权重相同的轮廓显示已发布的动作姿态，并复用轮廓完成选骨。

use super::preview::{Pose, SkeletonFrame, SkeletonView};
use bevy::{camera::visibility::RenderLayers, ecs::system::SystemParam, prelude::*};
use feathrise_editor_animation_model::{
    gizmo::GizmoCamera,
    host::AnimationHost,
    media::{
        AssetPreviewCamera, AssetPreviewCanvas, AssetPreviewModelTarget, AssetPreviewState,
        SceneEditorOwnedEntity,
    },
    skeleton::{
        BONE_COLOR, DEPTH_BIAS, JOINT_COLOR, JointGlyph, LINE_WIDTH, PICK_PADDING_PX,
        SELECTED_COLOR, SELECTED_LINE_WIDTH, fit_connected_joints, triangle_distance_squared,
    },
};
use feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ANIMATION_SKELETON_LAYERS;
use std::collections::VecDeque;

// 沿用权重骨架一帧的关节量级；每关节至多两条闭合轮廓，上传约 77 KiB/批。
const JOINTS_PER_BATCH: usize = 256;

#[derive(Component)]
struct AnimationSkeleton;

struct LineBatch {
    entity: Entity,
    handle: Handle<GizmoAsset>,
}

#[derive(Default)]
struct SkeletonBank {
    batches: Vec<[Option<LineBatch>; 2]>,
    retiring: bool,
}

#[derive(Default)]
struct Display {
    banks: [SkeletonBank; 2],
    front: usize,
    written: usize,
    camera: Option<Entity>,
    view: Option<SkeletonView>,
}

#[derive(SystemParam)]
pub(super) struct SkeletonPresentation<'w, 's> {
    host: Option<Res<'w, AnimationHost>>,
    canvases: Query<'w, 's, &'static ComputedNode, With<AssetPreviewCanvas>>,
    cameras: Query<
        'w,
        's,
        (Entity, &'static Projection, &'static GlobalTransform),
        With<AssetPreviewCamera>,
    >,
    assets: Option<ResMut<'w, Assets<GizmoAsset>>>,
    display: Local<'s, Display>,
}

impl SkeletonPresentation<'_, '_> {
    pub(super) fn view(
        &self,
        preview: &AssetPreviewState,
        selected: Option<usize>,
    ) -> Option<SkeletonView> {
        if !preview.render_visible
            || preview.model.as_ref()?.target != AssetPreviewModelTarget::Scene0
            || self
                .host
                .as_ref()
                .is_some_and(|host| host.weights_own_viewport)
            || self.assets.is_none()
        {
            return None;
        }
        let canvas = self.canvases.single().ok()?;
        let (camera, projection, view) = self.cameras.single().ok()?;
        if preview.camera != Some(camera) {
            return None;
        }
        Some(SkeletonView {
            camera: GizmoCamera::from_projection(
                projection,
                view,
                canvas.size(),
                canvas.inverse_scale_factor(),
            )?,
            selected,
        })
    }

    pub(super) fn needs_view(&self, view: SkeletonView) -> bool {
        self.display.view != Some(view)
            || self.cameras.single().ok().map(|(entity, _, _)| entity) != self.display.camera
    }

    /// 清理不再显示的槽位，退出或频繁换源也只回收一批。
    pub(super) fn maintain(&mut self, commands: &mut Commands) {
        if let Some(bank) = self
            .display
            .banks
            .iter_mut()
            .find(|bank| bank.retiring && !bank.batches.is_empty())
        {
            despawn_batch(commands, bank.batches.pop().unwrap());
        }
    }

    pub(super) fn discard_candidate(&mut self) {
        self.display.written = 0;
        let back = 1 - self.display.front;
        self.display.banks[back].retiring = true;
    }

    pub(super) fn clear(&mut self, commands: &mut Commands) {
        self.display.view = None;
        if let Some(camera) = self.display.camera.take() {
            set_camera_layer(commands, camera, None);
        }
        self.display.written = 0;
        for bank in &mut self.display.banks {
            bank.retiring = true;
        }
    }

    /// 每个 Update 只登记一批待上传的轮廓。相机仍使用旧层，直到隐藏层完整就绪。
    pub(super) fn stage(&mut self, commands: &mut Commands, frame: &mut SkeletonFrame) -> bool {
        let Some(assets) = &mut self.assets else {
            return false;
        };
        let display = &mut *self.display;
        let back = 1 - display.front;
        let bank = &mut display.banks[back];
        bank.retiring = false;
        if let Some(batch) = frame.batches.pop_front() {
            let index = display.written;
            if index == bank.batches.len() {
                bank.batches.push([None, None]);
            }
            let layer = SCENE_EDITOR_ANIMATION_SKELETON_LAYERS[back];
            for ((slot, lines), width) in bank.batches[index]
                .iter_mut()
                .zip(batch)
                .zip([LINE_WIDTH, SELECTED_LINE_WIDTH])
            {
                if has_lines(&lines) {
                    if let Some(batch) = slot.as_ref()
                        && commands.get_entity(batch.entity).is_ok()
                        && let Some(mut asset) = assets.get_mut(&batch.handle)
                    {
                        *asset = lines;
                        continue;
                    }
                    if let Some(batch) = slot.take() {
                        commands.entity(batch.entity).try_despawn();
                    }
                    let handle = assets.add(lines);
                    let entity = commands
                        .spawn((
                            AnimationSkeleton,
                            SceneEditorOwnedEntity,
                            Gizmo {
                                handle: handle.clone(),
                                depth_bias: DEPTH_BIAS,
                                line_config: GizmoLineConfig { width, ..default() },
                            },
                            RenderLayers::layer(layer),
                        ))
                        .id();
                    *slot = Some(LineBatch { entity, handle });
                } else if let Some(batch) = slot.take() {
                    commands.entity(batch.entity).try_despawn();
                }
            }
            display.written += 1;
        } else if bank.batches.len() > display.written {
            // 骨架缩短时先清掉旧尾块，再准许新层整体显示。
            despawn_batch(commands, bank.batches.pop().unwrap());
        }
        frame.batches.is_empty() && bank.batches.len() == display.written
    }

    /// 只在最后一批准备完成后调用。保留另一层的槽位供下一帧复用。
    pub(super) fn publish(&mut self, commands: &mut Commands, view: SkeletonView) {
        let Ok((camera, _, _)) = self.cameras.single() else {
            self.clear(commands);
            return;
        };
        if let Some(previous) = self.display.camera.replace(camera)
            && previous != camera
        {
            set_camera_layer(commands, previous, None);
        }
        self.display.front = 1 - self.display.front;
        self.display.written = 0;
        self.display.view = Some(view);
        set_camera_layer(
            commands,
            camera,
            Some(SCENE_EDITOR_ANIMATION_SKELETON_LAYERS[self.display.front]),
        );
    }
}

fn despawn_batch(commands: &mut Commands, batch: [Option<LineBatch>; 2]) {
    for lines in batch.into_iter().flatten() {
        commands.entity(lines.entity).try_despawn();
    }
}

fn has_lines(asset: &GizmoAsset) -> bool {
    !asset.buffer().buffer().strip_positions.is_empty()
}

fn set_camera_layer(commands: &mut Commands, camera: Entity, active: Option<usize>) {
    commands.queue(move |world: &mut World| {
        let Ok(mut camera) = world.get_entity_mut(camera) else {
            return;
        };
        let original = camera.get::<RenderLayers>().cloned().unwrap_or_default();
        let [front, back] = SCENE_EDITOR_ANIMATION_SKELETON_LAYERS;
        let mut layers = original.clone().without(front).without(back);
        if let Some(active) = active {
            layers = layers.with(active);
        }
        if layers != original {
            camera.insert(layers);
        }
    });
}

fn glyphs(pose: &Pose, view: SkeletonView) -> Vec<Option<JointGlyph>> {
    let mut selected = None;
    let mut glyphs: Vec<_> = pose
        .skeleton
        .iter()
        .enumerate()
        .map(|(index, joint)| {
            let active = Some(joint.node) == view.selected;
            if active {
                selected = Some(index);
            }
            JointGlyph::project(
                view.camera,
                pose.world[joint.node].transform_point3(Vec3::ZERO),
                active,
            )
        })
        .collect();
    for (index, joint) in pose.skeleton.iter().enumerate() {
        if let Some(parent) = joint.parent {
            fit_connected_joints(&mut glyphs, parent, index, selected);
        }
    }
    glyphs
}

/// 与姿态使用同一后台任务。投影完整骨架后分批编码线带，跨批连接共用同一帧的关节点。
pub(super) fn build(pose: &Pose, view: SkeletonView) -> SkeletonFrame {
    let glyphs = glyphs(pose, view);
    let mut batches = VecDeque::new();
    for (batch, joints) in pose.skeleton.chunks(JOINTS_PER_BATCH).enumerate() {
        let mut lines = [GizmoAsset::default(), GizmoAsset::default()];
        for (joint, glyph) in joints.iter().zip(&glyphs[batch * JOINTS_PER_BATCH..]) {
            let Some(glyph) = *glyph else { continue };
            let active = Some(joint.node) == view.selected;
            lines[usize::from(active)].linestrip(
                glyph.diamond(),
                if active { SELECTED_COLOR } else { JOINT_COLOR },
            );
            if let Some(parent) = joint.parent
                && let Some(outline) = glyphs[parent].and_then(|head| head.bone_to(glyph))
            {
                let active = Some(pose.skeleton[parent].node) == view.selected;
                lines[usize::from(active)]
                    .linestrip(outline, if active { SELECTED_COLOR } else { BONE_COLOR });
            }
        }
        if lines.iter().any(has_lines) {
            batches.push_back(lines);
        }
    }
    SkeletonFrame { view, batches }
}

pub(super) fn pick(pose: &Pose, view: SkeletonView, at: Vec2) -> Option<usize> {
    let point = at * view.camera.size * view.camera.inverse_scale;
    let glyphs = glyphs(pose, view);
    let joint = glyphs
        .iter()
        .enumerate()
        .filter_map(|(i, glyph)| {
            let glyph = (*glyph)?;
            let distance = glyph.screen.distance(point);
            (distance <= glyph.radius + PICK_PADDING_PX)
                .then_some((pose.skeleton[i].node, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    joint.map(|(node, _)| node).or_else(|| {
        pose.skeleton
            .iter()
            .enumerate()
            .filter_map(|(i, joint)| {
                let parent = joint.parent?;
                let head = glyphs[parent]?;
                let distance = triangle_distance_squared(point, head.screen_bone_to(glyphs[i]?)?);
                // 宽端属于起点骨骼，关节点始终优先，和权重工具的选择规则一致。
                (distance <= PICK_PADDING_PX.powi(2))
                    .then_some((pose.skeleton[parent].node, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(node, _)| node)
    })
}

#[cfg(any(test, feature = "validation"))]
pub(super) fn submitted(world: &World) -> bool {
    let Some(assets) = world.get_resource::<Assets<GizmoAsset>>() else {
        return false;
    };
    let Some(camera) = world
        .get_resource::<AssetPreviewState>()
        .and_then(|preview| preview.camera)
    else {
        return false;
    };
    let Some(layers) = world.get::<RenderLayers>(camera) else {
        return false;
    };
    world.iter_entities().any(|entity| {
        entity.contains::<AnimationSkeleton>()
            && entity
                .get::<RenderLayers>()
                .is_some_and(|layer| layers.intersects(layer))
            && entity
                .get::<Gizmo>()
                .and_then(|gizmo| assets.get(&gizmo.handle))
                .is_some_and(has_lines)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::SystemState;
    use feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ASSET_PREVIEW_LAYER;

    #[test]
    fn skeleton_layer_switches_only_after_all_batches_are_staged() {
        let mut world = World::new();
        world.init_resource::<Assets<GizmoAsset>>();
        let projection = Projection::Perspective(PerspectiveProjection::default());
        let transform = GlobalTransform::IDENTITY;
        let view = SkeletonView {
            camera: GizmoCamera::from_projection(&projection, &transform, Vec2::splat(400.0), 1.0)
                .unwrap(),
            selected: None,
        };
        let model_layers = RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER);
        let camera = world
            .spawn((
                AssetPreviewCamera,
                projection,
                transform,
                model_layers.clone(),
            ))
            .id();
        world.insert_resource(AssetPreviewState {
            camera: Some(camera),
            ..default()
        });

        let batch = || {
            let mut normal = GizmoAsset::default();
            normal.linestrip([Vec3::ZERO, Vec3::X], JOINT_COLOR);
            [normal, GizmoAsset::default()]
        };
        let count_on_layer = |world: &World, layer| {
            let filter = RenderLayers::layer(layer);
            world
                .iter_entities()
                .filter(|entity| {
                    entity.contains::<AnimationSkeleton>()
                        && entity
                            .get::<RenderLayers>()
                            .is_some_and(|layers| layers.intersects(&filter))
                })
                .count()
        };
        let mut state = SystemState::<(Commands, SkeletonPresentation)>::new(&mut world);
        let mut stage_frame = |world: &mut World, frame: &mut SkeletonFrame| {
            let (mut commands, mut skeleton) = state.get_mut(world).unwrap();
            let ready = skeleton.stage(&mut commands, frame);
            if ready {
                skeleton.publish(&mut commands, frame.view);
            }
            state.apply(world);
            ready
        };

        let [candidate_layer, previous_layer] = SCENE_EDITOR_ANIMATION_SKELETON_LAYERS;
        let mut previous = SkeletonFrame {
            view,
            batches: [batch()].into(),
        };
        assert!(stage_frame(&mut world, &mut previous));
        assert!(submitted(&world));

        // 完整的旧骨架仍可见，刚登记的一批只进入另一层。
        let mut frame = SkeletonFrame {
            view,
            batches: [batch(), batch()].into(),
        };
        assert!(!stage_frame(&mut world, &mut frame));
        assert_eq!(count_on_layer(&world, candidate_layer), 1);
        assert_eq!(
            world.get::<RenderLayers>(camera),
            Some(&model_layers.clone().with(previous_layer))
        );

        assert!(stage_frame(&mut world, &mut frame));
        assert_eq!(count_on_layer(&world, candidate_layer), 2);
        assert_eq!(
            world.get::<RenderLayers>(camera),
            Some(&model_layers.clone().with(candidate_layer))
        );
        assert!(submitted(&world));

        let (mut commands, mut skeleton) = state.get_mut(&mut world).unwrap();
        skeleton.clear(&mut commands);
        state.apply(&mut world);
        assert_eq!(world.get::<RenderLayers>(camera), Some(&model_layers));
        assert!(!submitted(&world));
        assert_eq!(
            count_on_layer(&world, candidate_layer),
            2,
            "立即撤下显示，分批回收实体"
        );
    }
}
