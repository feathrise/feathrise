//! 植被连续刷段的有界背压：每帧只推进32个邻域，真实折线尾段与整笔撤销一起收口。
use super::*;
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub from: Vec3,
    pub to: Vec3,
    pub radius: f32,
    pub erase: bool,
    pub amount: f32,
    pub exposure: f32,
}

#[derive(Default)]
pub(crate) struct VegetationPaintPath {
    pub(super) pending: Option<Sample>,
    pub(super) tail: VecDeque<Sample>,
    pub(super) release: bool,
    pub(super) disconnected: bool,
    pub(super) press: Option<ObjectVegetationPress>,
    pub(super) revision: Option<(u64, u64, u64)>,
}

impl VegetationPaintPath {
    pub(super) fn record_revision(&mut self, session: &EditorAuthoringSession) {
        self.revision = Some((
            session.revision,
            session.terrain_revision,
            session.vegetation_revision,
        ));
    }

    pub(super) fn current(&self, session: &EditorAuthoringSession) -> bool {
        self.revision
            == Some((
                session.revision,
                session.terrain_revision,
                session.vegetation_revision,
            ))
    }

    pub(super) fn queue(&mut self, sample: Sample) {
        if let Some(tail) = self.tail.back_mut()
            && tail.from == tail.to
            && sample.from == sample.to
            && tail.to == sample.from
            && tail.erase == sample.erase
            && tail.radius == sample.radius
            && tail.amount == sample.amount
        {
            tail.exposure += sample.exposure;
        } else if self.tail.len() < 64 {
            self.tail.push_back(sample); // 转弯和停留分别保留，不能把折线拉成斜线或把终点停留涂满旧段。
            self.disconnected = false;
        } else {
            self.disconnected = true; // 有界队列满时背压，不覆盖已经受理的真实轨迹。
        }
    }
}

/// 沿真实世界弧长推进前缀，禁止为满足预算把整段拉成32枚相离的圆印。
pub(super) fn apply(
    sample: Sample,
    layer: &feathrise_map_content::MapVegetationAuthorLayer,
    session: &mut EditorAuthoringSession,
    editor: &mut MapEditorState,
    stroke: &mut VegetationBrushStroke,
) -> Option<Sample> {
    let draft = session.draft.as_mut().expect("已验证当前植被画布");
    let metric = super::super::terrain_documents::PaintMetricSpace::from_draft(draft);
    let from = metric.point(sample.from);
    let to = metric.point(sample.to);
    let count = super::super::paint::brush_path_dab_count(from, to, sample.radius);
    let fraction =
        (super::super::paint::MAX_INTERPOLATED_DABS_PER_FRAME as f32 / count as f32).min(1.0);
    let endpoint = sample.from.lerp(sample.to, fraction);
    let remaining = (fraction < 1.0).then_some(Sample {
        from: endpoint,
        ..sample
    });
    let endpoint_metric = metric.point(endpoint);
    let dab_count = interpolated_dab_count(from, endpoint_metric, sample.radius);
    let world_min = metric.bounds(draft.terrain_manifest().world_min);
    let world_max = metric.bounds(draft.terrain_manifest().world_max);
    let Some(density) = draft.vegetation_author_densities.get_mut(&layer.id) else {
        return None;
    };
    let density = Arc::make_mut(density); // 仍只写当前作者层，沿用局部首次值差分。
    let mut changed = false;
    for dab in 1..=dab_count {
        changed |= paint_density_neighborhood_with_flow(
            density,
            layer.resolution,
            world_min,
            world_max,
            from.lerp(endpoint_metric, dab as f32 / dab_count as f32),
            sample.radius,
            sample.erase,
            sample.amount,
            sample.exposure,
            &mut stroke.before,
        );
    }
    stroke.segment_start = Some(sample.from);
    stroke.last_world = Some(endpoint); // GPU即时覆盖也只接管本帧真正修改的前缀。
    if changed {
        session.mark_vegetation_changed(editor, &layer.id);
        editor.status = if sample.erase {
            format!("正在擦除作者植被图层“{}”", layer.label)
        } else {
            format!("正在绘制作者植被图层“{}”", layer.label)
        };
    }
    remaining
}
