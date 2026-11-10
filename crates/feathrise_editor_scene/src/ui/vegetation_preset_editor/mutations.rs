//! 预设详细编辑器的枚举切换与 Mesh LOD/部件结构修改。
//!
//! 【功能设计】结构动作只修改活动标签的强类型草稿，并立即推进 UI 修订；
//! 保存前仍由公共目录校验执行跨字段原子验证，不在按钮层复制完整规则。

use std::collections::BTreeSet; // 新 LOD 与部件身份从当前草稿的真实占用集合中选择。

use feathrise_scene_content::{
    BladePresetLod, BladePresetMotion, BladePresetUltraLodBudget, MAX_BLADE_PRESET_LODS,
    MeshPresetAlphaMode, MeshPresetMeshDerivation, MeshPresetWind, MeshPresetWindPart,
    VegetationPresetBillboard, VegetationPresetCollision, VegetationPresetRuntime,
}; // 直接修改正式目录枚举，避免 UI 私有替身。

use super::types::VegetationPresetEditorState; // 所有动作只作用于活动标签。

/// Blade 在世界风与水流间切换。
pub(super) fn toggle_motion(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Blade { motion, .. } = &mut draft.descriptor.runtime else {
        return Err("Mesh 预设没有 Blade 运动来源".to_owned());
    };
    *motion = if matches!(motion, BladePresetMotion::WaterFlow { .. }) {
        BladePresetMotion::Wind
    } else {
        BladePresetMotion::WaterFlow {
            direction: [1.0, 0.0],
            speed: 0.65,
            tip_strength: 0.12,
            spatial_scale: 0.75,
        }
    };
    let label = if matches!(motion, BladePresetMotion::Wind) {
        "世界风"
    } else {
        "水流"
    };
    state.status = format!("运动来源已切换为{label}");
    state.touch_ui();
    Ok(())
}

/// Mesh 在无碰撞、圆柱和长方体间循环。
pub(super) fn cycle_collision(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Mesh { collision, .. } = &mut draft.descriptor.runtime else {
        return Err("Blade 预设没有 Mesh 近场碰撞".to_owned());
    };
    *collision = match collision {
        VegetationPresetCollision::None => VegetationPresetCollision::Cylinder {
            radius: 0.5,
            height: 2.0,
            center_y: 1.0,
            scale_with_instance: true,
            enter_distance: 72.0,
            exit_distance: 96.0,
        },
        VegetationPresetCollision::Cylinder { .. } => VegetationPresetCollision::Cuboid {
            size: [1.0, 1.0, 1.0],
            center: [0.0, 0.5, 0.0],
            scale_with_instance: true,
            enter_distance: 72.0,
            exit_distance: 96.0,
        },
        VegetationPresetCollision::Cuboid { .. } => VegetationPresetCollision::None,
    };
    state.status = "碰撞策略已切换；本页可继续精确编辑尺寸与驻留距离".to_owned();
    state.touch_ui();
    Ok(())
}

/// 循环 Blade 第四档世界稳定预算。
pub(super) fn cycle_ultra_budget(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Blade {
        ultra_lod_budget, ..
    } = &mut draft.descriptor.runtime
    else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    *ultra_lod_budget = match ultra_lod_budget {
        BladePresetUltraLodBudget::High => BladePresetUltraLodBudget::Dense,
        BladePresetUltraLodBudget::Dense => BladePresetUltraLodBudget::Medium,
        BladePresetUltraLodBudget::Medium => BladePresetUltraLodBudget::Low,
        BladePresetUltraLodBudget::Low => BladePresetUltraLodBudget::High,
    };
    state.status = "已切换 Blade 超远固定预算".to_owned();
    state.touch_ui();
    Ok(())
}

/// 追加一档 Blade LOD，并同步扩展视图阈值与常驻数组。
pub(super) fn append_blade_lod(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Blade { lods, view_lod, .. } = &mut draft.descriptor.runtime
    else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    if lods.len() >= MAX_BLADE_PRESET_LODS {
        return Err(format!(
            "Blade LOD 已达到上限 {MAX_BLADE_PRESET_LODS}，不能继续新增"
        ));
    }
    let used = lods.iter().map(|lod| lod.level).collect::<BTreeSet<_>>();
    let level = (0..MAX_BLADE_PRESET_LODS as u32)
        .find(|candidate| !used.contains(candidate))
        .ok_or_else(|| "Blade LOD level 身份已经全部占用".to_owned())?;
    let source = lods.last().cloned().unwrap_or_else(|| BladePresetLod {
        name: format!("LOD{level}"),
        level,
        mesh_path: String::new(),
        grid_side: 2,
        spacing: 0.1,
        inner_radius: 0.0,
        outer_radius: 8.0,
        inner_fade_width: 0.0,
        outer_fade_width: 1.0,
        seed: 1,
        snap_cells: 8,
    });
    let mut cloned = source;
    cloned.level = level;
    cloned.name = format!("LOD{level}");
    cloned.seed = cloned.seed.wrapping_add(1).max(1);
    if let Some(finest) = lods.iter().min_by_key(|lod| lod.level) {
        align_blade_lod_spacing(&mut cloned, finest.spacing * 2.0_f32.powi(level as i32));
    }
    lods.push(cloned);
    grow_blade_view_for_lod(view_lod, lods.len());
    draft.selected_lod = lods.len() - 1;
    state.status = format!("已追加 Blade LOD {level}；请精调窗口与模型路径");
    state.touch_ui();
    Ok(())
}

/// 复制当前 Blade LOD，并为新档选择尚未使用的 level。
pub(super) fn duplicate_blade_lod(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Blade { lods, view_lod, .. } = &mut draft.descriptor.runtime
    else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    if lods.len() >= MAX_BLADE_PRESET_LODS {
        return Err(format!(
            "Blade LOD 已达到上限 {MAX_BLADE_PRESET_LODS}，不能继续复制"
        ));
    }
    let source = lods
        .get(draft.selected_lod)
        .cloned()
        .ok_or_else(|| "当前 LOD 已不存在".to_owned())?;
    let used = lods.iter().map(|lod| lod.level).collect::<BTreeSet<_>>();
    let level = (0..MAX_BLADE_PRESET_LODS as u32)
        .find(|candidate| !used.contains(candidate))
        .ok_or_else(|| "Blade LOD level 身份已经全部占用".to_owned())?;
    let mut cloned = source;
    cloned.level = level;
    cloned.name = format!("LOD{level}");
    cloned.seed = cloned.seed.wrapping_add(1).max(1);
    if let Some(finest) = lods.iter().min_by_key(|lod| lod.level) {
        align_blade_lod_spacing(&mut cloned, finest.spacing * 2.0_f32.powi(level as i32));
    }
    lods.push(cloned);
    grow_blade_view_for_lod(view_lod, lods.len());
    draft.selected_lod = lods.len() - 1;
    state.status = format!("已复制为 Blade LOD {level}；请精调窗口与模型路径");
    state.touch_ui();
    Ok(())
}

/// 删除当前 Blade LOD；至少保留一档，并收缩视图数组。
pub(super) fn remove_blade_lod(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Blade { lods, view_lod, .. } = &mut draft.descriptor.runtime
    else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    if lods.len() <= 1 {
        return Err("Blade 预设至少保留一个 LOD".to_owned());
    }
    let index = draft.selected_lod.min(lods.len() - 1);
    let removed = lods.remove(index);
    // 档位是细→粗的数组身份；删除中间档后，模型顺序、视图数组和世界母格必须一起重排。
    lods.sort_by_key(|lod| lod.level);
    let finest_spacing = lods[0].spacing;
    for (level, lod) in lods.iter_mut().enumerate() {
        lod.level = level as u32;
        align_blade_lod_spacing(lod, finest_spacing * 2.0_f32.powi(level as i32));
    }
    view_lod.finest_grid_spacing = finest_spacing;
    shrink_blade_view_for_lod(view_lod, removed.level as usize);
    draft.selected_lod = (removed.level as usize).min(lods.len() - 1);
    state.status = format!("已从草稿删除 Blade LOD {}", removed.level);
    state.touch_ui();
    Ok(())
}

/// 扩展视图数组以匹配新的 Blade 档数；新阈值沿用上一档倍率外推。
fn grow_blade_view_for_lod(
    view_lod: &mut feathrise_scene_content::BladePresetViewLod,
    lod_count: usize,
) {
    while view_lod.residency_radii.len() < lod_count {
        let last = view_lod.residency_radii.last().copied().unwrap_or(8.0);
        view_lod.residency_radii.push((last * 2.0).max(last + 1.0));
    }
    while view_lod.residency_transition_widths.len() + 1 < lod_count {
        let last = view_lod
            .residency_transition_widths
            .last()
            .copied()
            .unwrap_or(4.0);
        view_lod.residency_transition_widths.push(last.max(1.0));
    }
    while view_lod.pixel_thresholds.len() + 1 < lod_count {
        // 细→粗追加更粗交接；若已有阈值则向外侧更小像素区间外推。
        let last = view_lod
            .pixel_thresholds
            .last()
            .copied()
            .unwrap_or([8.0, 12.0]);
        let start = (last[0] * 0.5).max(0.5);
        // 起点达到下限后，终点仍需跟随钳制后的起点，避免第八档生成反向区间。
        let next = [start, (last[1] * 0.5).max(start * 1.25)];
        view_lod.pixel_thresholds.push(next);
    }
}

/// 删除真实档位对应的驻留项；中间档合并时保留左侧交接参数，末档删除则移除最后一次交接。
fn shrink_blade_view_for_lod(
    view_lod: &mut feathrise_scene_content::BladePresetViewLod,
    removed_level: usize,
) {
    if removed_level < view_lod.residency_radii.len() {
        view_lod.residency_radii.remove(removed_level);
    }
    if !view_lod.residency_transition_widths.is_empty() {
        let index = removed_level.min(view_lod.residency_transition_widths.len() - 1);
        view_lod.residency_transition_widths.remove(index);
    }
    if !view_lod.pixel_thresholds.is_empty() {
        let index = removed_level.min(view_lod.pixel_thresholds.len() - 1);
        view_lod.pixel_thresholds.remove(index);
    }
}

/// 结构动作保持二倍格距层级，同时保留复制或删除前的世界窗口范围。
fn align_blade_lod_spacing(lod: &mut BladePresetLod, spacing: f32) {
    if spacing.is_finite() && spacing > 0.0 && spacing != lod.spacing {
        let extent = lod.grid_side.saturating_sub(1) as f64 * lod.spacing as f64;
        lod.grid_side = ((extent / spacing as f64).ceil() as u32)
            .saturating_add(1)
            .max(2);
        lod.spacing = spacing;
    }
}

/// 复制当前 Mesh LOD，并为新档选择尚未使用的 u8 稳定级别。
pub(super) fn duplicate_mesh_lod(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut draft.descriptor.runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let source = lods
        .get(draft.selected_lod)
        .cloned()
        .ok_or_else(|| "当前 LOD 已不存在".to_owned())?;
    let used = lods.iter().map(|lod| lod.level).collect::<BTreeSet<_>>();
    let level = (0..=u8::MAX)
        .find(|candidate| !used.contains(candidate))
        .ok_or_else(|| "Mesh LOD 的 u8 稳定身份已经全部占用".to_owned())?;
    let base = lods.iter().map(|lod| lod.end[1]).fold(0.0_f32, f32::max);
    let mut cloned = source;
    cloned.level = level;
    cloned.start = [(base - 20.0).max(0.0), base];
    cloned.end = [base + 80.0, base + 100.0];
    lods.push(cloned);
    draft.selected_lod = lods.len() - 1;
    draft.selected_part = 0;
    state.status = format!("已复制为 Mesh LOD {level}；请精调距离与资产路径");
    state.touch_ui();
    Ok(())
}

/// 删除当前 Mesh LOD；至少保留一个可绘制档。
pub(super) fn remove_mesh_lod(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut draft.descriptor.runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    if lods.len() <= 1 {
        return Err("Mesh 预设至少保留一个 LOD".to_owned());
    }
    let index = draft.selected_lod.min(lods.len() - 1);
    let removed = lods.remove(index);
    draft.selected_lod = index.min(lods.len() - 1);
    draft.selected_part = 0;
    state.status = format!("已从草稿删除 Mesh LOD {}", removed.level);
    state.touch_ui();
    Ok(())
}

/// 复制当前 Mesh 部件并选择同档未占用的 u32 身份。
pub(super) fn duplicate_mesh_part(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut draft.descriptor.runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let lod = lods
        .get_mut(draft.selected_lod)
        .ok_or_else(|| "当前 LOD 已不存在".to_owned())?;
    let mut cloned = lod
        .parts
        .get(draft.selected_part)
        .cloned()
        .ok_or_else(|| "当前部件已不存在".to_owned())?;
    let used = lod
        .parts
        .iter()
        .map(|part| part.part_id)
        .collect::<BTreeSet<_>>();
    cloned.part_id = (0..=u32::MAX)
        .find(|candidate| !used.contains(candidate))
        .ok_or_else(|| "当前 LOD 的 u32 部件身份已经全部占用".to_owned())?;
    lod.parts.push(cloned);
    draft.selected_part = lod.parts.len() - 1;
    state.status = "已复制当前 Mesh 部件；可分别修改模型和材质路径".to_owned();
    state.touch_ui();
    Ok(())
}

/// 删除当前 Mesh 部件；每档至少保留一个真实部件。
pub(super) fn remove_mesh_part(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut draft.descriptor.runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let lod = lods
        .get_mut(draft.selected_lod)
        .ok_or_else(|| "当前 LOD 已不存在".to_owned())?;
    if lod.parts.len() <= 1 {
        return Err("每个 Mesh LOD 至少保留一个部件".to_owned());
    }
    let index = draft.selected_part.min(lod.parts.len() - 1);
    let removed = lod.parts.remove(index);
    draft.selected_part = index.min(lod.parts.len() - 1);
    state.status = format!("已从草稿删除部件 {}", removed.part_id);
    state.touch_ui();
    Ok(())
}

/// 循环当前部件的公告板模式；动态公告板会关闭互斥的风材质。
pub(super) fn cycle_billboard(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    mutate_current_part(
        state,
        |part| {
            part.billboard = match part.billboard {
                VegetationPresetBillboard::Fixed => VegetationPresetBillboard::Cylindrical,
                VegetationPresetBillboard::Cylindrical => VegetationPresetBillboard::Spherical,
                VegetationPresetBillboard::Spherical => VegetationPresetBillboard::Fixed,
            };
            if !matches!(part.billboard, VegetationPresetBillboard::Fixed) {
                part.wind = None; // 两种顶点扩展管线不能叠加，按钮动作保持草稿立即可解释。
            }
        },
        "已切换当前部件公告板模式",
    )
}

/// 循环直接网格、分散薄片和凸包三种运行时取得方式。
pub(super) fn cycle_mesh_derivation(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    mutate_current_part(
        state,
        |part| {
            part.mesh_derivation = match part.mesh_derivation {
                MeshPresetMeshDerivation::Direct => {
                    MeshPresetMeshDerivation::DispersedTriangles { max_triangles: 40 }
                }
                MeshPresetMeshDerivation::DispersedTriangles { .. } => {
                    MeshPresetMeshDerivation::ConvexHull {
                        max_sample_points: 384,
                    }
                }
                MeshPresetMeshDerivation::ConvexHull { .. } => MeshPresetMeshDerivation::Direct,
            };
        },
        "已切换当前部件网格取得方式",
    )
}

/// 循环继承、不透明、Alpha Mask 与透明混合覆盖。
pub(super) fn cycle_alpha_mode(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    mutate_current_part(
        state,
        |part| {
            part.material_overrides.alpha_mode = match part.material_overrides.alpha_mode {
                None => Some(MeshPresetAlphaMode::Opaque),
                Some(MeshPresetAlphaMode::Opaque) => {
                    Some(MeshPresetAlphaMode::Mask { cutoff: 0.45 })
                }
                Some(MeshPresetAlphaMode::Mask { .. }) => Some(MeshPresetAlphaMode::Blend),
                Some(MeshPresetAlphaMode::Blend) => None,
            };
        },
        "已切换当前部件 Alpha 覆盖",
    )
}

/// 循环继承、强制双面和强制单面。
pub(super) fn toggle_double_sided(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    mutate_current_part(
        state,
        |part| {
            part.material_overrides.double_sided = match part.material_overrides.double_sided {
                None => Some(true),
                Some(true) => Some(false),
                Some(false) => None,
            };
        },
        "已切换当前部件双面覆盖",
    )
}

/// 切换当前部件阴影。
pub(super) fn toggle_shadow(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    mutate_current_part(
        state,
        |part| part.casts_shadow = !part.casts_shadow,
        "已切换当前部件投影",
    )
}

/// 开关当前部件高度权重风；启用时恢复固定几何以保持管线互斥。
pub(super) fn toggle_wind(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    mutate_current_part(
        state,
        |part| {
            part.wind = if part.wind.is_some() {
                None
            } else {
                part.billboard = VegetationPresetBillboard::Fixed;
                Some(MeshPresetWind {
                    root_height: 0.0,
                    tip_height: 1.0,
                    strength: 0.08,
                    frequency: 1.0,
                    part: MeshPresetWindPart::Foliage,
                })
            };
        },
        "已切换当前部件高度权重风",
    )
}

/// 在树干与叶簇风角色间切换。
pub(super) fn cycle_wind_part(state: &mut VegetationPresetEditorState) -> Result<(), String> {
    mutate_current_part(
        state,
        |part| {
            if let Some(wind) = part.wind.as_mut() {
                wind.part = match wind.part {
                    MeshPresetWindPart::Stem => MeshPresetWindPart::Foliage,
                    MeshPresetWindPart::Foliage => MeshPresetWindPart::Stem,
                };
            }
        },
        "已切换当前部件风角色",
    )
}

/// 切换碰撞尺寸是否跟随实例统一缩放。
pub(super) fn toggle_collision_scale(
    state: &mut VegetationPresetEditorState,
) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Mesh { collision, .. } = &mut draft.descriptor.runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    match collision {
        VegetationPresetCollision::None => return Err("无碰撞策略没有实例缩放开关".to_owned()),
        VegetationPresetCollision::Cylinder {
            scale_with_instance,
            ..
        }
        | VegetationPresetCollision::Cuboid {
            scale_with_instance,
            ..
        } => {
            *scale_with_instance = !*scale_with_instance;
        }
    }
    state.status = "已切换碰撞随实例缩放".to_owned();
    state.touch_ui();
    Ok(())
}

fn mutate_current_part(
    state: &mut VegetationPresetEditorState,
    mutate: impl FnOnce(&mut feathrise_scene_content::MeshPresetPart),
    status: &str,
) -> Result<(), String> {
    ensure_idle(state)?;
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut draft.descriptor.runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let part = lods
        .get_mut(draft.selected_lod)
        .and_then(|lod| lod.parts.get_mut(draft.selected_part))
        .ok_or_else(|| "当前 Mesh 部件已不存在".to_owned())?;
    mutate(part);
    state.status = status.to_owned();
    state.touch_ui();
    Ok(())
}

fn ensure_idle(state: &VegetationPresetEditorState) -> Result<(), String> {
    if state.busy() {
        Err("预设任务进行期间不能修改草稿".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
