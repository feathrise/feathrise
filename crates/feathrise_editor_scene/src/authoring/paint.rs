//! 地表索引/权重与植被R8局部笔刷；植被层只能引用已登记正式profile。

use std::collections::BTreeMap; // 单笔划只保存首次触及字节。

use super::resources::ByteChange; // 原局部差分算法继续供地表、植被与水域共用。
mod surface_brush; // 地表共享写侧等待、输入采样与历史收口单独维护。
use crate::components::SceneEditorViewport;
use bevy::prelude::*; // 输入、时间、ECS资源和世界坐标。
use bevy::ui::RelativeCursorPosition; // 引入地表作者层事务所需类型和纯助手。
pub(crate) use surface_brush::apply_surface_brush; // 保持原插件注册路径和四槽笔刷算法边界。 // 左键只在中央编辑视口内绘制。

pub(super) const PAINT_RESPONSE_PER_SECOND: f32 = 4.0;
/// 返回首次刷印与持续曝光；后台只能累加曝光，不能把已饱和的中心强度乘到笔刷边缘。
pub(super) fn vegetation_paint_exposure(
    strength: f32,
    delta_seconds: f32,
    starting: bool,
) -> (f32, f32) {
    if starting {
        (strength.clamp(0.0, 1.0), 0.0)
    } else {
        (
            0.0,
            strength.max(0.0) * delta_seconds.clamp(0.0, 0.05) * PAINT_RESPONSE_PER_SECOND,
        )
    }
}
pub(super) const MAX_INTERPOLATED_DABS_PER_FRAME: u32 = 32; // 极端鼠标跳跃仍保持有界，不让一帧复制无界笔刷工作。

pub(super) fn interpolated_dab_count(from: Vec3, to: Vec3, radius: f32) -> u32 {
    brush_path_dab_count(from, to, radius).min(MAX_INTERPOLATED_DABS_PER_FRAME as usize) as u32
}

/// 共用圆刷轨迹采样距离；后台点位散布消费完整路径，不继承R8每帧工作预算。
pub fn brush_path_dab_count(from: Vec3, to: Vec3, radius: f32) -> usize {
    // 小半径也按刷宽补点；0.25米下限会让0.1米细刷自身断成圆斑。毫厘托底仅防非法零半径除零。
    feathrise_author_tools::brush_path_dab_count(from.distance(to), radius)
}

/// 一次笔刷换算后的离散包围盒与世界步长；具名字段避免调用侧依赖长元组顺序。
struct BrushBounds {
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
    step_x: f32,
    step_z: f32,
    center_x: f32,
    center_z: f32,
}

pub(super) fn hovered(viewport: &Query<&RelativeCursorPosition, With<SceneEditorViewport>>) -> bool {
    viewport.single().is_ok_and(|cursor| cursor.cursor_over())
}

/// 判断世界X/Z中的圆形笔刷是否与场景矩形相交；圆心允许位于场景外。
///
/// 【功能设计】视口、地表、植被、地形和水域必须共用同一条边缘契约：只要可见圆仍覆盖
/// 场景的一部分，就裁剪到场景内继续绘制；圆与矩形完全分离时才拒绝本次dab。
pub(crate) fn brush_overlaps_world_bounds(
    world: Vec3,
    radius: f32,
    world_min: [f32; 2],
    world_max: [f32; 2],
) -> bool {
    if !world.x.is_finite()
        || !world.z.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
        || !world_min.iter().all(|value| value.is_finite())
        || !world_max.iter().all(|value| value.is_finite())
        || world_min[0] >= world_max[0]
        || world_min[1] >= world_max[1]
    {
        return false; // 坏范围、非有限圆心和空半径不得进入坐标夹紧或像素索引。
    }
    let nearest_x = world.x.clamp(world_min[0], world_max[0]); // 场景矩形上距圆心最近点的X。
    let nearest_z = world.z.clamp(world_min[1], world_max[1]); // 场景矩形上距圆心最近点的Z。
    let dx = world.x - nearest_x; // 圆心在矩形轴投影内时该轴距离为零。
    let dz = world.z - nearest_z; // 角部越界时两个轴距离共同参与精确圆角判定。
    dx * dx + dz * dz <= radius * radius // 最近点位于圆内或圆周上即存在几何交集。
}

fn brush_bounds(
    world: Vec3,
    radius: f32,
    resolution: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
) -> Option<BrushBounds> {
    if resolution[0] < 2
        || resolution[1] < 2
        || !brush_overlaps_world_bounds(world, radius, world_min, world_max)
    {
        return None;
    }
    let step_x = (world_max[0] - world_min[0]) / (resolution[0] - 1) as f32;
    let step_z = (world_max[1] - world_min[1]) / (resolution[1] - 1) as f32;
    if !step_x.is_finite() || !step_z.is_finite() || step_x <= 0.0 || step_z <= 0.0 {
        return None; // 非法世界跨度不能换算到规则作者网格。
    }
    let center_x = (world.x - world_min[0]) / step_x; // 保留亚格圆心，避免绘制位置吸附到最近纹素。
    let center_z = (world.z - world_min[1]) / step_z; // Z轴同样保留连续网格坐标。
    let min_x = (center_x - radius / step_x).floor().max(0.0) as i32;
    let max_x = (center_x + radius / step_x)
        .ceil()
        .min(resolution[0] as f32 - 1.0) as i32;
    let min_z = (center_z - radius / step_z).floor().max(0.0) as i32;
    let max_z = (center_z + radius / step_z)
        .ceil()
        .min(resolution[1] as f32 - 1.0) as i32;
    if min_x > max_x || min_z > max_z {
        return None; // 连续圆虽接近场景，但没有可访问的离散样本时保持零修改。
    }
    Some(BrushBounds {
        min_x,
        max_x,
        min_z,
        max_z,
        step_x,
        step_z,
        center_x,
        center_z,
    })
}

fn normalize_four_weights(values: [f32; 4]) -> [u8; 4] {
    let positive = values.map(|value| value.max(0.0));
    let total = positive.iter().sum::<f32>();
    if !total.is_finite() || total <= f32::EPSILON {
        return [255, 0, 0, 0];
    }
    let mut output = [0u8; 4];
    let mut fractions = [0.0f32; 4];
    let mut assigned = 0u16;
    for index in 0..4 {
        let exact = positive[index] / total * 255.0;
        output[index] = exact.floor().clamp(0.0, 255.0) as u8;
        fractions[index] = exact - output[index] as f32;
        assigned += output[index] as u16;
    }
    for _ in assigned..255 {
        let winner = fractions
            .iter()
            .enumerate()
            .max_by(|(left_index, left), (right_index, right)| {
                left.total_cmp(right)
                    .then_with(|| right_index.cmp(left_index))
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
        output[winner] = output[winner].saturating_add(1);
        fractions[winner] = -1.0;
    }
    output
}

/// 把一个纹素的四槽候选规范化、编辑并重新选出贡献最高的四层。
///
/// 【功能设计】场景可以登记任意数量的层，但单个纹素只携带实际可见的四个局部索引；
/// 新刷入第五种材质时稳定淘汰最低贡献者，因此 GPU 成本不随预设库或场景层数增长。
fn paint_indexed_weights(
    current_indices: [u32; 4],
    current_weights: [u8; 4],
    layer: u32,
    layer_count: u32,
    erase: bool,
    amount: f32,
) -> ([u32; 4], [u8; 4]) {
    if layer >= layer_count || layer_count == 0 {
        return (current_indices, current_weights);
    }
    let mut entries = [(0_u32, 0.0_f32); 5];
    let mut entry_count = 0usize;
    for (id, weight) in current_indices.into_iter().zip(current_weights) {
        if id >= layer_count || weight == 0 {
            continue;
        }
        if let Some((_, value)) = entries[..entry_count]
            .iter_mut()
            .find(|(candidate, _)| *candidate == id)
        {
            *value += f32::from(weight); // 损坏或旧Mip中的重复索引先聚合为唯一候选。
        } else {
            entries[entry_count] = (id, f32::from(weight));
            entry_count += 1;
        }
    }
    if entry_count == 0 {
        if erase || amount <= 0.0 {
            // 裸地没有可擦除贡献；笔刷边缘零强度也不能制造覆盖。
            return (current_indices, current_weights); // 保持原空画布，不隐式刷入第一层。
        } // 真实非擦除落笔才建立目标材质覆盖。
        entries[0] = (layer, 255.0); // 从空白开始必须使用用户选择的层，而不是固定局部零层。
        entry_count = 1;
    }
    let total = entries[..entry_count]
        .iter()
        .map(|(_, value)| *value)
        .sum::<f32>();
    for (_, value) in &mut entries[..entry_count] {
        *value = *value / total * 255.0;
    }

    let amount = amount.clamp(0.0, 1.0);
    let selected = entries[..entry_count]
        .iter()
        .position(|(candidate, _)| *candidate == layer);
    if erase {
        if let Some(selected) = selected {
            let old = entries[selected].1;
            let next = old * (1.0 - amount);
            let other_total = 255.0 - old;
            if other_total > f32::EPSILON {
                let scale = (255.0 - next) / other_total;
                for (index, (_, value)) in entries[..entry_count].iter_mut().enumerate() {
                    if index != selected {
                        *value *= scale;
                    }
                }
                entries[selected].1 = next;
            } else if layer_count > 1 {
                entries[selected].1 = next;
                let fallback = if layer == 0 { 1 } else { 0 };
                entries[entry_count] = (fallback, 255.0 - next);
                entry_count += 1;
            } // 单层场景没有可接收权重的其他材质，擦除严格保持唯一层全覆盖。
        }
    } else {
        let selected = selected.unwrap_or_else(|| {
            entries[entry_count] = (layer, 0.0);
            entry_count += 1;
            entry_count - 1
        });
        let old = entries[selected].1;
        let next = old + (255.0 - old) * amount;
        let other_total = (255.0 - old).max(0.0);
        let scale = if other_total <= f32::EPSILON {
            0.0
        } else {
            (255.0 - next) / other_total
        };
        for (index, (_, value)) in entries[..entry_count].iter_mut().enumerate() {
            if index != selected {
                *value *= scale;
            }
        }
        entries[selected].1 = next;
    }

    entries[..entry_count].sort_by(|(left_id, left), (right_id, right)| {
        right.total_cmp(left).then_with(|| left_id.cmp(right_id)) // 同权重按局部ID稳定选择，保存和撤销结果可复现。
    });
    let mut indices = [0_u32; 4];
    let mut values = [0.0_f32; 4];
    for (slot, (id, value)) in entries[..entry_count.min(4)].iter().copied().enumerate() {
        indices[slot] = id;
        values[slot] = value;
    }
    (indices, normalize_four_weights(values))
}

#[allow(clippy::too_many_arguments)]
fn paint_surface_neighborhood(
    indices: &mut [u8],
    weights: &mut [u8],
    resolution: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
    world: Vec3,
    radius: f32,
    layer: u32,
    layer_count: u32,
    erase: bool,
    amount: f32,
    before_indices: &mut BTreeMap<usize, u8>,
    before_weights: &mut BTreeMap<usize, u8>,
) -> bool {
    let Some(bounds) = brush_bounds(world, radius, resolution, world_min, world_max) else {
        return false;
    };
    let mut changed = false;
    for z in bounds.min_z..=bounds.max_z {
        for x in bounds.min_x..=bounds.max_x {
            let dx = (x as f32 - bounds.center_x) * bounds.step_x;
            let dz = (z as f32 - bounds.center_z) * bounds.step_z;
            let distance = (dx * dx + dz * dz).sqrt();
            if distance > radius {
                continue;
            }
            let texel = (z as u32 * resolution[0] + x as u32) as usize;
            let weight_offset = texel * 4;
            let index_offset = texel * 16;
            if weight_offset + 4 > weights.len() || index_offset + 16 > indices.len() {
                return changed; // 损坏缓冲不越界；加载预检会给出可操作错误。
            }
            let current_weights = weights[weight_offset..weight_offset + 4]
                .try_into()
                .expect("已验证四字节权重切片");
            let current_indices = std::array::from_fn(|slot| {
                let start = index_offset + slot * 4;
                u32::from_le_bytes(
                    indices[start..start + 4]
                        .try_into()
                        .expect("已验证四字节索引切片"),
                )
            });
            let falloff = 1.0 - distance / radius;
            let (next_indices, next_weights) = paint_indexed_weights(
                current_indices,
                current_weights,
                layer,
                layer_count,
                erase,
                amount * falloff,
            );
            for (channel, next_value) in next_weights.into_iter().enumerate() {
                let index = weight_offset + channel;
                if weights[index] != next_value {
                    before_weights.entry(index).or_insert(weights[index]);
                    weights[index] = next_value;
                    changed = true;
                }
            }
            for (slot, next_index) in next_indices.into_iter().enumerate() {
                for (byte, next_value) in next_index.to_le_bytes().into_iter().enumerate() {
                    let index = index_offset + slot * 4 + byte;
                    if indices[index] != next_value {
                        before_indices.entry(index).or_insert(indices[index]);
                        indices[index] = next_value;
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_density_neighborhood(
    density: &mut [u8],
    resolution: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
    world: Vec3,
    radius: f32,
    erase: bool,
    amount: f32,
    before: &mut BTreeMap<usize, u8>,
) -> bool {
    paint_density_neighborhood_with_flow(
        density, resolution, world_min, world_max, world, radius, erase, amount, 0.0, before,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_density_neighborhood_with_flow(
    density: &mut [u8],
    resolution: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
    world: Vec3,
    radius: f32,
    erase: bool,
    amount: f32,
    flow_exposure: f32,
    before: &mut BTreeMap<usize, u8>,
) -> bool {
    let Some(bounds) = brush_bounds(world, radius, resolution, world_min, world_max) else {
        return false;
    };
    let mut changed = false;
    for z in bounds.min_z..=bounds.max_z {
        for x in bounds.min_x..=bounds.max_x {
            let dx = (x as f32 - bounds.center_x) * bounds.step_x;
            let dz = (z as f32 - bounds.center_z) * bounds.step_z;
            let distance = (dx * dx + dz * dz).sqrt();
            if distance > radius {
                continue;
            }
            let index = (z as u32 * resolution[0] + x as u32) as usize;
            let old = density[index];
            let target = if erase { 0.0 } else { 255.0 };
            let factor = feathrise_terrain::volume::sparse::paged::field_paint_response(
                amount,
                flow_exposure,
                1.0 - distance / radius,
            );
            let next = (old as f32 + (target - old as f32) * factor).round() as u8;
            if next != old {
                before.entry(index).or_insert(old);
                density[index] = next;
                changed = true;
            }
        }
    }
    changed
}

pub(super) fn byte_changes(before: &BTreeMap<usize, u8>, current: &[u8]) -> Vec<ByteChange> {
    before
        .iter()
        .filter_map(|(&index, &before)| {
            let after = *current.get(index)?;
            (before != after).then_some(ByteChange {
                index,
                before,
                after,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "paint_tests.rs"]
mod tests;
