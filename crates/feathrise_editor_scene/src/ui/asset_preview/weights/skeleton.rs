//! 在权重画布叠加当前蒙皮实例的骨架与所选骨骼。

use feathrise_editor_glb_model::weights::{Joint, MAX_JOINTS, Tool};
use super::{
    super::types::{AssetPreviewCamera, AssetPreviewCanvas, AssetPreviewState},
    preview::{self, WeightPreview},
    resources::{Session, WeightEditor},
    systems::{Gate, Target},
};
use crate::{
    resources::SceneEditorState,
    ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    viewport::placement_move::MoveCamera,
};
use bevy::{
    camera::visibility::{RenderLayers, VisibilitySystems},
    gizmos::config::GizmoConfigStore,
    mesh::skinning::SkinnedMesh,
    prelude::*,
    transform::TransformSystems,
};
use feathrise_editor_animation_model::skeleton::{
    BONE_COLOR, DEPTH_BIAS, JOINT_COLOR, JointGlyph, LINE_WIDTH, SELECTED_COLOR, SELECTED_LINE_WIDTH,
    PICK_PADDING_PX, fit_connected_joints, triangle_distance_squared,
};
use feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ASSET_PREVIEW_LAYER;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct SkeletonGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct SelectedBoneGizmos;

const LOCK_BODY: [Vec2; 5] = [
    Vec2::new(-4.0, 0.0),
    Vec2::new(4.0, 0.0),
    Vec2::new(4.0, 6.0),
    Vec2::new(-4.0, 6.0),
    Vec2::new(-4.0, 0.0),
];
const LOCK_SHACKLE: [Vec2; 6] = [
    Vec2::new(-2.5, 0.0),
    Vec2::new(-2.5, -2.5),
    Vec2::new(-1.2, -4.0),
    Vec2::new(1.2, -4.0),
    Vec2::new(2.5, -2.5),
    Vec2::new(2.5, 0.0),
];

#[derive(Clone, Copy)]
struct BoneFrame {
    root: Entity,
    generation: u64,
    tab: u64,
    surface: usize,
    camera: MoveCamera,
    camera_origin: Vec3,
    camera_forward: Vec3,
}

impl BoneFrame {
    fn depth(self, origin: Vec3) -> f32 {
        (origin - self.camera_origin).dot(self.camera_forward)
    }
}

/// 发布给下一次输入的是同帧绘制的完整骨架，固定容量与蒙皮上限相同。
#[derive(Resource)]
pub(super) struct BoneTargets {
    frame: Option<BoneFrame>,
    glyphs: [Option<JointGlyph>; MAX_JOINTS],
}

impl Default for BoneTargets {
    fn default() -> Self {
        Self {
            frame: None,
            glyphs: [None; MAX_JOINTS],
        }
    }
}

impl BoneTargets {
    fn current_frame(&self, session: &Session, preview: &AssetPreviewState) -> Option<BoneFrame> {
        let generation = preview.model.as_ref()?.generation;
        self.frame.filter(|frame| {
            preview.render_visible
                && session.tool == Tool::Bone
                && session.show_skeleton
                && !session.stale
                && Some(frame.root) == preview.scene_root()
                && frame.generation == generation
                && frame.tab == session.key.tab
                && frame.surface == session.surface
        })
    }

    pub(super) fn pick(
        &self,
        session: &Session,
        preview: &AssetPreviewState,
        camera: MoveCamera,
        cursor: Vec2,
    ) -> Option<u16> {
        let frame = self.current_frame(session, preview)?;
        if frame.camera != camera || !cursor.is_finite() {
            return None;
        }
        let joints = &session.doc.surface(session.surface).skin.joints;
        let pixels = cursor * camera.size * camera.inverse_scale;
        pick_glyph(&self.glyphs[..joints.len()], joints, frame, pixels)
    }
}

fn pick_glyph(
    glyphs: &[Option<JointGlyph>],
    joints: &[Joint],
    frame: BoneFrame,
    cursor: Vec2,
) -> Option<u16> {
    let order = |a: &(f32, f32, u16), b: &(f32, f32, u16)| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.1.total_cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    };
    // 起点标记优先。相邻短骨仍能在指尖等位置选到真实关节。
    if let Some((_, _, joint)) = glyphs
        .iter()
        .enumerate()
        .filter_map(|(index, glyph)| {
            let glyph = (*glyph)?;
            let delta = (cursor - glyph.screen).abs();
            (delta.element_sum() <= glyph.radius + PICK_PADDING_PX).then(|| {
                (
                    delta.length_squared(),
                    frame.depth(glyph.origin),
                    index as u16,
                )
            })
        })
        .min_by(order)
    {
        return Some(joint);
    }
    joints
        .iter()
        .enumerate()
        .filter_map(|(index, joint)| {
            let parent = joint.parent?;
            let head = glyphs[usize::from(parent)]?;
            let tail = glyphs[index]?;
            let distance = triangle_distance_squared(cursor, head.screen_bone_to(tail)?);
            if distance > PICK_PADDING_PX.powi(2) {
                return None;
            }
            // 点击骨段选中其起点。投影相交时按点击处在观察方向上的深度区分。
            let axis = tail.screen - head.screen;
            let t = ((cursor - head.screen).dot(axis) / axis.length_squared()).clamp(0.0, 1.0);
            Some((
                distance,
                frame.depth(head.origin.lerp(tail.origin, t)),
                parent,
            ))
        })
        .min_by(order)
        .map(|(_, _, joint)| joint)
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<BoneTargets>()
        .init_gizmo_group::<SkeletonGizmos>()
        .init_gizmo_group::<SelectedBoneGizmos>()
        .add_systems(Startup, configure)
        .add_systems(
            PostUpdate,
            draw.after(preview::update)
                .after(TransformSystems::Propagate)
                .after(VisibilitySystems::VisibilityPropagate),
        );
}

fn configure(mut store: ResMut<GizmoConfigStore>) {
    // 权重热力图显示在模型表面，骨骼轮廓和起点在同一预览相机中穿透该表面。
    let (config, _) = store.config_mut::<SkeletonGizmos>();
    config.render_layers = RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER);
    config.depth_bias = DEPTH_BIAS;
    config.line.width = LINE_WIDTH;
    let (config, _) = store.config_mut::<SelectedBoneGizmos>();
    config.render_layers = RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER);
    config.depth_bias = DEPTH_BIAS;
    config.line.width = SELECTED_LINE_WIDTH;
}

fn draw(
    target: Target,
    gate: Gate,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    state: Res<WeightEditor>,
    preview: Res<WeightPreview>,
    skins: Query<(&SkinnedMesh, &InheritedVisibility)>,
    transforms: Query<&GlobalTransform>,
    canvases: Query<&ComputedNode, With<AssetPreviewCanvas>>,
    cameras: Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
    mut skeleton: Gizmos<SkeletonGizmos>,
    mut highlight: Gizmos<SelectedBoneGizmos>,
    mut targets: ResMut<BoneTargets>,
) {
    targets.frame = None;
    if sidebar.right != EditorRightSidebarTab::Weights
        || !target.preview.render_visible
        || !gate.open(&editor)
    {
        return;
    }
    let Some(session) = state
        .session
        .as_ref()
        .filter(|session| session.show_skeleton && !session.stale && target.matches(&session.key))
    else {
        return;
    };
    // 显隐传播后直接选当前已显示的蒙皮实例，静置和动作交换时都使用画面的真实关节。
    let Some(skinned) = preview
        .surface_entities(&session.key, session.surface)
        .filter_map(|entity| skins.get(entity).ok())
        .find_map(|(skin, visibility)| visibility.get().then_some(skin))
    else {
        return;
    };
    let joints = &session.doc.surface(session.surface).skin.joints;
    if skinned.joints.len() != joints.len() {
        return;
    }
    let (Ok(canvas), Some(camera)) = (canvases.single(), target.preview.camera) else {
        return;
    };
    let Ok((projection, view)) = cameras.get(camera) else {
        return;
    };
    let Some(camera) = MoveCamera::from_projection(
        projection,
        view,
        canvas.size(),
        canvas.inverse_scale_factor(),
    ) else {
        return;
    };

    // 已登记关节按固定上限一次取完整姿态，每关节最多一个起点和一个入射骨段。
    let active = usize::from(session.joint);
    let glyphs = &mut targets.glyphs[..joints.len()];
    for (index, (glyph, &joint)) in glyphs.iter_mut().zip(&skinned.joints).enumerate() {
        let Ok(transform) = transforms.get(joint) else {
            return;
        };
        let origin = transform.translation();
        if !origin.is_finite() {
            return;
        }
        *glyph = JointGlyph::project(*camera, origin, index == active);
    }
    for (index, joint) in joints.iter().enumerate() {
        if let Some(parent) = joint.parent.map(usize::from) {
            fit_connected_joints(glyphs, parent, index, Some(active));
        }
    }

    let lock_color = Color::srgb(1.0, 0.68, 0.18);
    let locks = session.current_locks();
    for (index, joint) in joints.iter().enumerate() {
        let Some(glyph) = glyphs[index] else { continue };
        if index == active {
            highlight.linestrip(glyph.diamond(), SELECTED_COLOR);
        } else {
            skeleton.linestrip(glyph.diamond(), JOINT_COLOR);
        }
        if locks[index] {
            // 锁记号与关节共享投影深度，使用单独的像素偏移保留起点命中区。
            let anchor = Vec2::new(glyph.radius + 6.0, -5.0);
            for pair in LOCK_BODY.windows(2).chain(LOCK_SHACKLE.windows(2)) {
                skeleton.line(
                    glyph.offset(anchor + pair[0]),
                    glyph.offset(anchor + pair[1]),
                    lock_color,
                );
            }
        }
        if let Some(parent) = joint.parent.map(usize::from)
            && let Some(outline) = glyphs[parent].and_then(|head| head.bone_to(glyph))
        {
            // 选中左大腿时标出髋到膝，末端关节也有自己的起点标记。
            if parent == active {
                highlight.linestrip(outline, SELECTED_COLOR);
            } else {
                skeleton.linestrip(outline, BONE_COLOR);
            }
        }
    }
    if session.tool == Tool::Bone
        && let Some(root) = target.preview.scene_root()
        && let Some(model) = target.preview.model.as_ref()
    {
        targets.frame = Some(BoneFrame {
            root,
            generation: model.generation,
            tab: session.key.tab,
            surface: session.surface,
            camera,
            camera_origin: view.translation(),
            camera_forward: *view.forward(),
        });
    }
}

#[cfg(feature = "validation")]
fn validation_targets(world: &World) -> Option<(&BoneTargets, BoneFrame, &Session)> {
    let targets = world.get_resource::<BoneTargets>()?;
    let session = world.get_resource::<WeightEditor>()?.session.as_ref()?;
    let frame = targets.current_frame(session, world.get_resource::<AssetPreviewState>()?)?;
    Some((targets, frame, session))
}

#[cfg(feature = "validation")]
pub(super) fn joint_point(world: &World, joint: u16) -> Option<Vec2> {
    let (targets, frame, session) = validation_targets(world)?;
    let joints = &session.doc.surface(frame.surface).skin.joints;
    let index = usize::from(joint);
    if index >= joints.len() {
        return None;
    }
    let glyph = targets.glyphs[index]?;
    Some(glyph.screen / (frame.camera.size * frame.camera.inverse_scale))
}

#[cfg(feature = "validation")]
pub(super) fn bone_point(world: &World, child: u16) -> Option<Vec2> {
    let (targets, frame, session) = validation_targets(world)?;
    let joints = &session.doc.surface(frame.surface).skin.joints;
    let index = usize::from(child);
    let parent = usize::from(joints.get(index)?.parent?);
    let [a, b, c] = targets.glyphs[parent]?.screen_bone_to(targets.glyphs[index]?)?;
    Some((a + b + c) / (3.0 * frame.camera.size * frame.camera.inverse_scale))
}

#[cfg(feature = "validation")]
pub(super) fn submitted(world: &World) -> bool {
    let handles = world.resource::<bevy::gizmos::GizmoHandles>();
    handles
        .handles()
        .get(&std::any::TypeId::of::<SelectedBoneGizmos>())
        .is_some_and(Option::is_some)
}

#[cfg(feature = "validation")]
pub(super) fn joint_markers(world: &World) -> usize {
    use bevy::gizmos::{GizmoAsset, GizmoHandles};
    use std::any::TypeId;

    let handles = world.resource::<GizmoHandles>();
    let assets = world.resource::<Assets<GizmoAsset>>();
    [
        TypeId::of::<SkeletonGizmos>(),
        TypeId::of::<SelectedBoneGizmos>(),
    ]
    .into_iter()
    .filter_map(|group| handles.handles().get(&group)?.as_ref())
    .filter_map(|handle| assets.get(handle))
    .map(|asset| {
        // 菱形是五个顶点的闭合线带，骨段轮廓是四个顶点。
        asset
            .buffer()
            .buffer()
            .strip_positions
            .split(|point| !point.is_finite())
            .filter(|strip| strip.len() == 5)
            .count()
    })
    .sum()
}

#[cfg(feature = "validation")]
pub(super) fn lock_markers(world: &World) -> usize {
    use bevy::gizmos::{GizmoAsset, GizmoHandles};
    use std::any::TypeId;

    let handles = world.resource::<GizmoHandles>();
    let assets = world.resource::<Assets<GizmoAsset>>();
    handles
        .handles()
        .get(&TypeId::of::<SkeletonGizmos>())
        .and_then(Option::as_ref)
        .and_then(|handle| assets.get(handle))
        .map_or(0, |asset| {
            // 同组的关节和骨段使用线带，仅锁记号使用独立线段。
            asset.buffer().buffer().list_positions.len()
                / (2 * (LOCK_BODY.len() + LOCK_SHACKLE.len() - 2))
        })
}
