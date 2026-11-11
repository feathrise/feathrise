//! 从已显示的动作容器分批采集关节矩阵，后台按真实变形拾取原图元顶点。

use super::{
    super::{
        animation::{AnimationEditorState, AnimationPreviewRuntime, WeightBinding, WeightFrame},
        rigging::files::active,
        view_math::AssetPreviewView,
    },
    asset::{Document, Geometry},
    edit::{Operation, Selection},
    influences::VertexWeights,
    preview::WeightPreview,
    resources::{Reason, Session, WeightEditor},
    spatial::{Occlusion, Points},
    systems::{Target, queue},
};
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState};
use bevy::{
    mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    prelude::*,
};
use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

// 采集只读当前显示容器。关节和刚性实例合计共享一个逐帧预算。
const MATRICES_PER_FRAME: usize = 256;

#[derive(Clone, Copy)]
pub(super) struct Bookmark {
    pub frame: WeightFrame,
    pub view: AssetPreviewView,
}

#[derive(Clone, Copy)]
pub(super) struct Gesture {
    pub bookmark: Bookmark,
    pub root: Entity,
}

pub(super) struct Capture {
    gesture: Gesture,
    selection: Selection,
    started: Instant,
    next: usize,
    epoch: Option<u64>,
    skins: HashMap<usize, Arc<[Mat4]>>,
    geometry: PosedGeometry,
}

impl Capture {
    pub fn new(gesture: Gesture, selection: Selection) -> Self {
        Self {
            gesture,
            selection,
            started: Instant::now(),
            next: 0,
            epoch: None,
            skins: HashMap::new(),
            geometry: PosedGeometry::default(),
        }
    }
}

enum Sample {
    Skin {
        surface: usize,
        matrices: Arc<[Mat4]>,
        weights: Arc<[VertexWeights]>,
    },
    Rigid(usize, Mat4),
}

#[derive(Default)]
pub(super) struct PosedGeometry(Vec<Sample>);

impl PosedGeometry {
    pub fn deform(
        self,
        doc: &Document,
        selected_surface: usize,
        visible: bool,
        cancel: &AtomicBool,
    ) -> Result<(Points, Option<Occlusion>), String> {
        let mut selected = None;
        let mut triangles = Vec::new();
        for sample in self.0 {
            active(cancel)?;
            let (surface, positions, indices) = match &sample {
                Sample::Skin { surface, .. } => {
                    let (positions, indices) = doc.surfaces[*surface].geometry();
                    (Some(*surface), positions, indices)
                }
                Sample::Rigid(index, _) => {
                    let shape = &doc.rigid[*index];
                    (None, shape.positions.as_slice(), shape.indices.as_slice())
                }
            };
            if surface != Some(selected_surface) && !visible {
                continue;
            }
            let mut points = Vec::with_capacity(positions.len());
            for (group, block) in positions.chunks(2048).enumerate() {
                active(cancel)?;
                for (offset, &position) in block.iter().enumerate() {
                    let point = Vec3::from_array(position);
                    points.push(match &sample {
                        Sample::Skin {
                            matrices, weights, ..
                        } => skin_point(point, weights[group * 2048 + offset], matrices),
                        Sample::Rigid(_, matrix) => matrix.transform_point3(point),
                    });
                }
            }
            if points.iter().any(|point| !point.is_finite()) {
                return Err("问题帧的蒙皮坐标无效".into());
            }
            if visible {
                triangles.extend(indices.chunks_exact(3).map(|face| {
                    [
                        points[face[0] as usize],
                        points[face[1] as usize],
                        points[face[2] as usize],
                    ]
                }));
            }
            if surface == Some(selected_surface) {
                if selected.is_some() {
                    return Err("动作容器复用了当前蒙皮图元".into());
                }
                selected = Some(points);
            }
        }
        active(cancel)?;
        let points = selected.ok_or("动作容器没有当前蒙皮图元")?;
        Ok((
            Points::for_selection(points),
            visible.then(|| Occlusion::new(triangles)),
        ))
    }
}

// 与 Bevy 蒙皮一致，使用关节世界矩阵乘逆绑定矩阵，保持 GLB 顶点下标。
fn skin_point(point: Vec3, row: VertexWeights, matrices: &[Mat4]) -> Vec3 {
    row.joints
        .into_iter()
        .zip(row.weights)
        .filter(|(_, weight)| *weight > 0.0)
        .map(|(joint, weight)| matrices[joint as usize].transform_point3(point) * weight)
        .sum()
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Readback<'w, 's> {
    transforms: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            Option<&'static SkinnedMesh>,
            Option<&'static InheritedVisibility>,
        ),
    >,
    inverse_binds: Option<Res<'w, Assets<SkinnedMeshInverseBindposes>>>,
}

impl Readback<'_, '_> {
    fn advance(
        &self,
        capture: &mut Capture,
        parts: &[(Entity, Geometry)],
        session: &Session,
    ) -> Result<bool, String> {
        let mut budget = MATRICES_PER_FRAME;
        while budget > 0 {
            let Some(&(entity, geometry)) = parts.get(capture.next) else {
                return Ok(true);
            };
            let relevant = capture.selection.visible || geometry == Geometry::Skin(session.surface);
            if relevant {
                let (world, skin, visibility) =
                    self.transforms.get(entity).map_err(|_| "动作网格已换代")?;
                if visibility.is_none_or(|visibility| visibility.get()) {
                    match geometry {
                        Geometry::Skin(surface) => {
                            let source = &session.doc.surfaces[surface];
                            let matrices =
                                if let Some(matrices) = capture.skins.get(&source.skin.index) {
                                    matrices.clone()
                                } else {
                                    let skin = skin.ok_or("动作网格缺少蒙皮组件")?;
                                    let inverse = self
                                        .inverse_binds
                                        .as_ref()
                                        .and_then(|assets| assets.get(&skin.inverse_bindposes))
                                        .ok_or("动作网格缺少逆绑定矩阵")?;
                                    let count = skin.joints.len();
                                    if count != source.skin.joints.len()
                                        || count != inverse.len()
                                        || count > MATRICES_PER_FRAME
                                    {
                                        return Err("动作容器关节与原蒙皮不匹配".into());
                                    }
                                    if count > budget {
                                        return Ok(false);
                                    }
                                    let values: Arc<[Mat4]> = skin
                                        .joints
                                        .iter()
                                        .zip(inverse.iter())
                                        .map(|(joint, inverse)| {
                                            self.transforms
                                                .get(*joint)
                                                .map(|(global, _, _)| global.to_matrix() * *inverse)
                                                .map_err(|_| "动作关节已换代")
                                        })
                                        .collect::<Result<Vec<_>, _>>()?
                                        .into();
                                    budget -= count.saturating_sub(1);
                                    capture.skins.insert(source.skin.index, values.clone());
                                    values
                                };
                            let weights = if session.original {
                                source.original.clone()
                            } else {
                                session.drafts[surface].weights.clone()
                            };
                            capture.geometry.0.push(Sample::Skin {
                                surface,
                                matrices,
                                weights,
                            });
                        }
                        Geometry::Rigid(index) => capture
                            .geometry
                            .0
                            .push(Sample::Rigid(index, world.to_matrix())),
                    }
                }
            }
            capture.next += 1;
            budget -= 1;
        }
        Ok(capture.next == parts.len())
    }
}

pub(super) fn capture(
    mut state: ResMut<WeightEditor>,
    target: Target,
    sidebar: Res<SceneEditorSidebarState>,
    animation: Option<Res<AnimationEditorState>>,
    motion: Option<Res<AnimationPreviewRuntime>>,
    binding: Option<Res<WeightBinding>>,
    draw: Res<WeightPreview>,
    readback: Readback,
) {
    let Some(mut capture) = state.capture.take() else {
        return;
    };
    let result = (|| {
        let session = state
            .session
            .as_ref()
            .filter(|session| target.matches(&session.key) && !session.stale)
            .ok_or("调权模型已切换")?;
        let (Some(animation), Some(motion), Some(binding)) =
            (animation.as_deref(), motion.as_deref(), binding.as_deref())
        else {
            return Err("当前工程动作已经结束".into());
        };
        if sidebar.right != EditorRightSidebarTab::Weights
            || !animation.matches_weight_frame(capture.gesture.bookmark.frame)
            || motion.weight_frame_root(binding, &session.key.path) != Some(capture.gesture.root)
        {
            return Err("动作帧已切换，请重新框选".into());
        }
        if capture.started.elapsed() > Duration::from_secs(8) {
            return Err("读取问题帧超时，请重新框选".into());
        }
        let Some((epoch, parts)) = draw.pose_parts(&session.key, capture.gesture.root) else {
            return Ok(false);
        };
        if capture
            .epoch
            .replace(epoch)
            .is_some_and(|previous| previous != epoch)
        {
            return Err("动作网格已重新加载，请重新框选".into());
        }
        readback.advance(&mut capture, parts, session)
    })();
    match result {
        Ok(true) => queue(
            &mut state,
            Operation::Select(capture.selection, Some(capture.geometry)),
            Reason::Problem(capture.gesture.bookmark),
        ),
        Ok(false) => state.capture = Some(capture),
        Err(error) => {
            state.notice = error;
            state.bump();
        }
    }
}
