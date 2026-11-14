//! 读原模型、BVH命中、局部连续采样及稀疏样本写时复制只在后台执行。

use super::resources::{Sample, Work};
use bevy::prelude::*;
use feathrise_scene::paint_surface::{PaintSurfaceHit, PaintSurfaceSnapshot};
use feathrise_scene_content::ModelSurfaceVegetation;
use std::sync::Arc;

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    surface: PaintSurfaceSnapshot,
    mut data: Arc<ModelSurfaceVegetation>,
    layer: &str,
    samples: Vec<Sample>,
    mut last: Option<(Ray3d, PaintSurfaceHit)>,
    mut targets: Arc<Vec<Arc<ModelSurfaceVegetation>>>,
    index: usize,
) -> Result<Work, String> {
    let before_bytes = heap_bytes(&data);
    let mut changed = false;
    for sample in samples {
        let Some(hit) = surface.ray_surface(
            sample.ray.origin,
            *sample.ray.direction,
            1_000_000.0,
        ) else {
            last = None; // 真实空白切断拖动；不能用前次命中跨过洞口补画。
            continue;
        };
        let steps = last.as_ref().map_or(1, |(_, previous)| {
            super::super::paint::interpolated_dab_count(
                previous.position,
                hit.position,
                sample.radius,
            )
            .max(1)
        });
        for step in 1..=steps {
            let current = if step == steps {
                Some(hit.clone())
            } else {
                let Some((from, _)) = last.as_ref() else {
                    continue;
                };
                let t = step as f32 / steps as f32;
                let direction = (*from.direction)
                    .lerp(*sample.ray.direction, t)
                    .try_normalize();
                direction.and_then(|direction| {
                    surface.ray_surface(
                        from.origin.lerp(sample.ray.origin, t),
                        direction,
                        1_000_000.0,
                    )
                })
            };
            let Some(current) = current else {
                continue;
            };
            changed |= surface.paint_model_vegetation(
                Arc::make_mut(&mut data),
                &current,
                layer,
                sample.radius,
                sample.amount,
                sample.exposure / steps as f32,
                sample.erase,
            )?;
        }
        last = Some((sample.ray, hit));
    }
    let after_bytes = heap_bytes(&data);
    if changed {
        let values = Arc::make_mut(&mut targets); // 目标索引的写时复制同样在后台，主帧只交换完整Arc。
        if index == values.len() {
            values.push(data);
        } else if let Some(value) = values.get_mut(index) {
            *value = data;
        } else {
            return Err("模型稀疏密度的目标下标已变化".into());
        }
    }
    Ok(Work::Paint {
        targets,
        changed,
        last,
        before_bytes,
        after_bytes,
    })
}

pub(super) fn heap_bytes(data: &ModelSurfaceVegetation) -> usize {
    data.samples.capacity()
        * std::mem::size_of::<feathrise_scene_content::ModelSurfaceVegetationSample>()
        + data
            .samples
            .iter()
            .map(|sample| {
                sample.layers.capacity() * std::mem::size_of::<(String, u8)>()
                    + sample
                        .layers
                        .iter()
                        .map(|(id, _)| id.capacity())
                        .sum::<usize>()
            })
            .sum::<usize>()
        + data.sample_blocks.capacity() * std::mem::size_of::<String>()
        + data
            .sample_blocks
            .iter()
            .map(String::capacity)
            .sum::<usize>()
}
