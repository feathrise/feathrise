//! 详细表单字符缓冲到正式植被描述的强类型写入。
//!
//! 【功能设计】这里仅完成当前字段的局部解析与落值；保存时仍由公共目录校验器
//! 对跨字段范围、LOD 顺序、资源协议和碰撞约束做一次完整校验。

use feathrise_scene_content::{
    BladePresetMotion, MeshPresetAlphaMode, MeshPresetMeshDerivation, VegetationPresetCollision,
    VegetationPresetDescriptor, VegetationPresetRuntime,
};

use super::super::types::{
    BladeAppearanceGroup, BladeBoundsField, BladeLodField, BladeMotionField, BladeViewGroup,
    CollisionField, MeshLodField, MeshPartField, PresetField,
};

/// 把当前字符缓冲提交到强类型描述；完整跨字段校验仍由保存事务统一执行。
pub(crate) fn commit_field(
    descriptor: &mut VegetationPresetDescriptor,
    field: PresetField,
    input: &str,
) -> Result<(), String> {
    let trimmed = input.trim();
    match field {
        PresetField::Label => descriptor.label = required_text(trimmed, "显示名称")?,
        PresetField::Notes => descriptor.notes = Some(input.to_owned()), // 自由备注完整保留中文、换行、空白和显式清空，不改来源或运行时字段。
        PresetField::ResolutionX => descriptor.resolution[0] = parse_u32(trimmed, "画布宽度")?,
        PresetField::ResolutionY => descriptor.resolution[1] = parse_u32(trimmed, "画布高度")?,
        PresetField::Spacing => descriptor.spacing_meters = parse_f32(trimmed, "候选间距")?,
        PresetField::ScaleMin => descriptor.scale_range[0] = parse_f32(trimmed, "缩放下限")?,
        PresetField::ScaleMax => descriptor.scale_range[1] = parse_f32(trimmed, "缩放上限")?,
        PresetField::Seed => {
            descriptor.seed = (!trimmed.is_empty())
                .then(|| parse_u64(trimmed, "散布种子"))
                .transpose()?
        }
        PresetField::BladeBaseColorPath => {
            let VegetationPresetRuntime::Blade {
                base_color_texture_path,
                ..
            } = &mut descriptor.runtime
            else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            *base_color_texture_path = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        }
        PresetField::BladeDensityGain => {
            let VegetationPresetRuntime::Blade { density_gain, .. } = &mut descriptor.runtime
            else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            *density_gain = parse_f32(trimmed, "密度增益")?;
        }
        PresetField::BladeUltraSweep => {
            let VegetationPresetRuntime::Blade {
                ultra_horizontal_sweep,
                ..
            } = &mut descriptor.runtime
            else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            *ultra_horizontal_sweep = parse_f32(trimmed, "超远水平覆盖")?;
        }
        PresetField::BladeBounds(field) => {
            commit_blade_bounds(&mut descriptor.runtime, field, trimmed)?
        }
        PresetField::BladeLod { lod, field } => {
            commit_blade_lod(&mut descriptor.runtime, lod, field, trimmed)?
        }
        PresetField::BladeAppearance { group, component } => {
            let VegetationPresetRuntime::Blade { appearance, .. } = &mut descriptor.runtime else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            *appearance_group_mut(appearance, group)
                .get_mut(component)
                .ok_or_else(|| "Blade 外观分量越界".to_owned())? = parse_f32(trimmed, "外观分量")?;
        }
        PresetField::BladeView { group, component } => {
            commit_blade_view(&mut descriptor.runtime, group, component, trimmed)?
        }
        PresetField::BladeMotion(field) => {
            commit_blade_motion(&mut descriptor.runtime, field, trimmed)?
        }
        PresetField::MeshGroundEmbed => {
            let VegetationPresetRuntime::Mesh {
                ground_embed_depth, ..
            } = &mut descriptor.runtime
            else {
                return Err("当前预设不是数据 Mesh".to_owned());
            };
            *ground_embed_depth = parse_f32(trimmed, "贴地深度")?;
        }
        PresetField::MeshLod { lod, field } => {
            commit_mesh_lod(&mut descriptor.runtime, lod, field, trimmed)?
        }
        PresetField::MeshPart { lod, part, field } => {
            commit_mesh_part(&mut descriptor.runtime, lod, part, field, trimmed)?
        }
        PresetField::Collision(field) => commit_collision(&mut descriptor.runtime, field, trimmed)?,
    }
    Ok(())
}

fn appearance_group_mut(
    appearance: &mut feathrise_scene_content::BladePresetAppearance,
    group: BladeAppearanceGroup,
) -> &mut [f32; 4] {
    match group {
        BladeAppearanceGroup::Geometry => &mut appearance.geometry,
        BladeAppearanceGroup::WidthFlexibility => &mut appearance.width_flexibility,
        BladeAppearanceGroup::Bending => &mut appearance.bending,
        BladeAppearanceGroup::AtlasUv => &mut appearance.atlas_uv,
        BladeAppearanceGroup::RootColor => &mut appearance.root_color,
        BladeAppearanceGroup::TipColor => &mut appearance.tip_color,
        BladeAppearanceGroup::CoolTint => &mut appearance.cool_tint,
        BladeAppearanceGroup::WarmTint => &mut appearance.warm_tint,
    }
}

fn commit_blade_bounds(
    runtime: &mut VegetationPresetRuntime,
    field: BladeBoundsField,
    value: &str,
) -> Result<(), String> {
    let VegetationPresetRuntime::Blade { bounds, .. } = runtime else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    let target = match field {
        BladeBoundsField::MaxHeight => &mut bounds.max_blade_height,
        BladeBoundsField::HorizontalSweep => &mut bounds.max_horizontal_sweep,
        BladeBoundsField::ReferenceHeight => &mut bounds.reference_height,
        BladeBoundsField::RootPenetration => &mut bounds.root_penetration,
    };
    *target = parse_f32(value, "Blade 包围")?;
    Ok(())
}

fn commit_blade_lod(
    runtime: &mut VegetationPresetRuntime,
    index: usize,
    field: BladeLodField,
    value: &str,
) -> Result<(), String> {
    let VegetationPresetRuntime::Blade { lods, .. } = runtime else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    let lod = lods
        .get_mut(index)
        .ok_or_else(|| "Blade LOD 已不存在".to_owned())?;
    match field {
        BladeLodField::Name => lod.name = required_text(value, "LOD 名称")?,
        BladeLodField::MeshPath => lod.mesh_path = required_text(value, "Mesh 路径")?,
        BladeLodField::GridSide => lod.grid_side = parse_u32(value, "候选边长")?,
        BladeLodField::Spacing => lod.spacing = parse_f32(value, "LOD 格距")?,
        BladeLodField::InnerRadius => lod.inner_radius = parse_f32(value, "内半径")?,
        BladeLodField::OuterRadius => lod.outer_radius = parse_f32(value, "外半径")?,
        BladeLodField::InnerFade => lod.inner_fade_width = parse_f32(value, "内淡变")?,
        BladeLodField::OuterFade => lod.outer_fade_width = parse_f32(value, "外淡变")?,
        BladeLodField::Seed => lod.seed = parse_u32(value, "LOD 种子")?,
        BladeLodField::SnapCells => lod.snap_cells = parse_u32(value, "吸附格数")?,
    }
    Ok(())
}

fn commit_blade_view(
    runtime: &mut VegetationPresetRuntime,
    group: BladeViewGroup,
    component: usize,
    value: &str,
) -> Result<(), String> {
    let VegetationPresetRuntime::Blade { view_lod, .. } = runtime else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    let parsed = parse_f32(value, "视图 LOD 参数")?;
    // 任意相邻档使用同一字段路径，旧的三个中文组名不再限制可编辑范围。
    let target: &mut [f32] = match group {
        BladeViewGroup::PixelThresholds(index) => view_lod
            .pixel_thresholds
            .get_mut(index)
            .ok_or_else(|| "Blade 相邻档阈值越界".to_owned())?,
        BladeViewGroup::CameraFadeDistances => &mut view_lod.camera_fade_distances,
        BladeViewGroup::ReferenceGrassHeight => {
            std::slice::from_mut(&mut view_lod.reference_grass_height)
        }
        BladeViewGroup::FinestGridSpacing => {
            std::slice::from_mut(&mut view_lod.finest_grid_spacing)
        }
        BladeViewGroup::ResidencyRadii => &mut view_lod.residency_radii,
        BladeViewGroup::ResidencyMargin => std::slice::from_mut(&mut view_lod.residency_margin),
        BladeViewGroup::ResidencyTransitionWidths => &mut view_lod.residency_transition_widths,
    };
    *target
        .get_mut(component)
        .ok_or_else(|| "Blade 视图分量越界".to_owned())? = parsed;
    Ok(())
}

fn commit_blade_motion(
    runtime: &mut VegetationPresetRuntime,
    field: BladeMotionField,
    value: &str,
) -> Result<(), String> {
    let VegetationPresetRuntime::Blade { motion, .. } = runtime else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    let BladePresetMotion::WaterFlow {
        direction,
        speed,
        tip_strength,
        spatial_scale,
    } = motion
    else {
        return Err("世界风模式没有水流标量".to_owned());
    };
    let parsed = parse_f32(value, "水流参数")?;
    match field {
        BladeMotionField::DirectionX => direction[0] = parsed,
        BladeMotionField::DirectionZ => direction[1] = parsed,
        BladeMotionField::Speed => *speed = parsed,
        BladeMotionField::TipStrength => *tip_strength = parsed,
        BladeMotionField::SpatialScale => *spatial_scale = parsed,
    }
    Ok(())
}

fn commit_mesh_lod(
    runtime: &mut VegetationPresetRuntime,
    index: usize,
    field: MeshLodField,
    value: &str,
) -> Result<(), String> {
    let VegetationPresetRuntime::Mesh { lods, .. } = runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let lod = lods
        .get_mut(index)
        .ok_or_else(|| "Mesh LOD 已不存在".to_owned())?;
    match field {
        MeshLodField::StartBegin => lod.start[0] = parse_f32(value, "淡入起点")?,
        MeshLodField::StartFull => lod.start[1] = parse_f32(value, "完全可见")?,
        MeshLodField::EndBegin => lod.end[0] = parse_f32(value, "淡出起点")?,
        MeshLodField::EndHidden => lod.end[1] = parse_f32(value, "完全隐藏")?,
    }
    Ok(())
}

fn commit_mesh_part(
    runtime: &mut VegetationPresetRuntime,
    lod_index: usize,
    part_index: usize,
    field: MeshPartField,
    value: &str,
) -> Result<(), String> {
    let VegetationPresetRuntime::Mesh { lods, .. } = runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let part = lods
        .get_mut(lod_index)
        .and_then(|lod| lod.parts.get_mut(part_index))
        .ok_or_else(|| "Mesh 部件已不存在".to_owned())?;
    match field {
        MeshPartField::PartId => part.part_id = parse_u32(value, "部件 ID")?,
        MeshPartField::MeshPath => part.mesh_path = required_text(value, "Mesh 路径")?,
        MeshPartField::MaterialPath => part.material_path = required_text(value, "材质路径")?,
        MeshPartField::TranslationX => part.transform.translation[0] = parse_f32(value, "平移 X")?,
        MeshPartField::TranslationY => part.transform.translation[1] = parse_f32(value, "平移 Y")?,
        MeshPartField::TranslationZ => part.transform.translation[2] = parse_f32(value, "平移 Z")?,
        MeshPartField::YawDegrees => part.transform.yaw_degrees = parse_f32(value, "Y 旋转")?,
        MeshPartField::ScaleX => part.transform.scale[0] = parse_f32(value, "缩放 X")?,
        MeshPartField::ScaleY => part.transform.scale[1] = parse_f32(value, "缩放 Y")?,
        MeshPartField::ScaleZ => part.transform.scale[2] = parse_f32(value, "缩放 Z")?,
        MeshPartField::DerivationBudget => match &mut part.mesh_derivation {
            MeshPresetMeshDerivation::Direct => return Err("直接网格没有派生预算".to_owned()),
            MeshPresetMeshDerivation::DispersedTriangles { max_triangles } => {
                *max_triangles = parse_usize(value, "最大三角形")?
            }
            MeshPresetMeshDerivation::ConvexHull { max_sample_points } => {
                *max_sample_points = parse_usize(value, "最大采样点")?
            }
        },
        MeshPartField::AlphaCutoff => match &mut part.material_overrides.alpha_mode {
            Some(MeshPresetAlphaMode::Mask { cutoff }) => *cutoff = parse_f32(value, "Alpha 阈值")?,
            _ => return Err("当前材质覆盖不是 Alpha Mask".to_owned()),
        },
        MeshPartField::WindRootHeight => {
            part.wind
                .as_mut()
                .ok_or_else(|| "当前部件未启用风".to_owned())?
                .root_height = parse_f32(value, "风根高度")?
        }
        MeshPartField::WindTipHeight => {
            part.wind
                .as_mut()
                .ok_or_else(|| "当前部件未启用风".to_owned())?
                .tip_height = parse_f32(value, "风梢高度")?
        }
        MeshPartField::WindStrength => {
            part.wind
                .as_mut()
                .ok_or_else(|| "当前部件未启用风".to_owned())?
                .strength = parse_f32(value, "风强度")?
        }
        MeshPartField::WindFrequency => {
            part.wind
                .as_mut()
                .ok_or_else(|| "当前部件未启用风".to_owned())?
                .frequency = parse_f32(value, "风频率")?
        }
    }
    Ok(())
}

fn commit_collision(
    runtime: &mut VegetationPresetRuntime,
    field: CollisionField,
    value: &str,
) -> Result<(), String> {
    let VegetationPresetRuntime::Mesh { collision, .. } = runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let parsed = parse_f32(value, "碰撞参数")?;
    match (collision, field) {
        (VegetationPresetCollision::Cylinder { radius, .. }, CollisionField::CylinderRadius) => {
            *radius = parsed
        }
        (VegetationPresetCollision::Cylinder { height, .. }, CollisionField::CylinderHeight) => {
            *height = parsed
        }
        (VegetationPresetCollision::Cylinder { center_y, .. }, CollisionField::CylinderCenterY) => {
            *center_y = parsed
        }
        (VegetationPresetCollision::Cuboid { size, .. }, CollisionField::CuboidSizeX) => {
            size[0] = parsed
        }
        (VegetationPresetCollision::Cuboid { size, .. }, CollisionField::CuboidSizeY) => {
            size[1] = parsed
        }
        (VegetationPresetCollision::Cuboid { size, .. }, CollisionField::CuboidSizeZ) => {
            size[2] = parsed
        }
        (VegetationPresetCollision::Cuboid { center, .. }, CollisionField::CuboidCenterX) => {
            center[0] = parsed
        }
        (VegetationPresetCollision::Cuboid { center, .. }, CollisionField::CuboidCenterY) => {
            center[1] = parsed
        }
        (VegetationPresetCollision::Cuboid { center, .. }, CollisionField::CuboidCenterZ) => {
            center[2] = parsed
        }
        (
            VegetationPresetCollision::Cylinder { enter_distance, .. }
            | VegetationPresetCollision::Cuboid { enter_distance, .. },
            CollisionField::EnterDistance,
        ) => *enter_distance = parsed,
        (
            VegetationPresetCollision::Cylinder { exit_distance, .. }
            | VegetationPresetCollision::Cuboid { exit_distance, .. },
            CollisionField::ExitDistance,
        ) => *exit_distance = parsed,
        _ => return Err("当前碰撞策略没有该字段".to_owned()),
    }
    Ok(())
}

fn required_text(value: &str, label: &str) -> Result<String, String> {
    (!value.is_empty())
        .then(|| value.to_owned())
        .ok_or_else(|| format!("{label}不能为空"))
}

fn parse_f32(value: &str, label: &str) -> Result<f32, String> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| format!("{label}不是有效数字"))?;
    parsed
        .is_finite()
        .then_some(parsed)
        .ok_or_else(|| format!("{label}必须是有限数字"))
}

fn parse_u32(value: &str, label: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("{label}不是有效 u32 整数"))
}

fn parse_u64(value: &str, label: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{label}不是有效 u64 整数"))
}

fn parse_usize(value: &str, label: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("{label}不是有效整数"))
}
