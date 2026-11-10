//! 中央详细表单字段与正式强类型植被描述之间的唯一读写映射。
//!
//! 【功能设计】UI 只保存当前一个字符缓冲；提交时按字段路径写回完整描述。
//! 未聚焦字段始终直接回显强类型数据，因此切换标签、LOD 或部件不会维护第二份镜像状态。

mod write;

pub(super) use write::commit_field;

use feathrise_scene_content::{
    BladePresetMotion, MeshPresetAlphaMode, MeshPresetMeshDerivation, VegetationPresetCollision,
    VegetationPresetDescriptor, VegetationPresetRuntime,
}; // 全部表单值直接落入发布目录的正式类型。

use super::types::{
    BladeAppearanceGroup, BladeBoundsField, BladeLodField, BladeMotionField, BladeViewGroup,
    CollisionField, MeshLodField, MeshPartField, PresetField,
}; // 强类型字段路径避免依赖标签文本反查。

/// 返回一个字段当前用于输入框的无损文本表示。
pub(super) fn field_value(
    descriptor: &VegetationPresetDescriptor,
    field: PresetField,
) -> Result<String, String> {
    let value = match field {
        PresetField::Label => descriptor.label.clone(),
        PresetField::Notes => descriptor
            .notes
            .clone()
            .unwrap_or_else(|| descriptor.author_asset_path.clone()), // 只有未设置备注才读取旧来源；Some("")永远保持空白。
        PresetField::ResolutionX => descriptor.resolution[0].to_string(),
        PresetField::ResolutionY => descriptor.resolution[1].to_string(),
        PresetField::Spacing => descriptor.spacing_meters.to_string(),
        PresetField::ScaleMin => descriptor.scale_range[0].to_string(),
        PresetField::ScaleMax => descriptor.scale_range[1].to_string(),
        PresetField::Seed => descriptor
            .seed
            .map(|value| value.to_string())
            .unwrap_or_default(),
        PresetField::BladeBaseColorPath => {
            let VegetationPresetRuntime::Blade {
                base_color_texture_path,
                ..
            } = &descriptor.runtime
            else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            base_color_texture_path.clone().unwrap_or_default()
        }
        PresetField::BladeDensityGain => blade_scalar(&descriptor.runtime, |density, _| density)?,
        PresetField::BladeUltraSweep => blade_scalar(&descriptor.runtime, |_, sweep| sweep)?,
        PresetField::BladeBounds(field) => {
            let VegetationPresetRuntime::Blade { bounds, .. } = &descriptor.runtime else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            match field {
                BladeBoundsField::MaxHeight => bounds.max_blade_height,
                BladeBoundsField::HorizontalSweep => bounds.max_horizontal_sweep,
                BladeBoundsField::ReferenceHeight => bounds.reference_height,
                BladeBoundsField::RootPenetration => bounds.root_penetration,
            }
            .to_string()
        }
        PresetField::BladeLod { lod, field } => {
            let VegetationPresetRuntime::Blade { lods, .. } = &descriptor.runtime else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            let lod = lods
                .get(lod)
                .ok_or_else(|| "Blade LOD 已不存在".to_owned())?;
            match field {
                BladeLodField::Name => lod.name.clone(),
                BladeLodField::MeshPath => lod.mesh_path.clone(),
                BladeLodField::GridSide => lod.grid_side.to_string(),
                BladeLodField::Spacing => lod.spacing.to_string(),
                BladeLodField::InnerRadius => lod.inner_radius.to_string(),
                BladeLodField::OuterRadius => lod.outer_radius.to_string(),
                BladeLodField::InnerFade => lod.inner_fade_width.to_string(),
                BladeLodField::OuterFade => lod.outer_fade_width.to_string(),
                BladeLodField::Seed => lod.seed.to_string(),
                BladeLodField::SnapCells => lod.snap_cells.to_string(),
            }
        }
        PresetField::BladeAppearance { group, component } => {
            let VegetationPresetRuntime::Blade { appearance, .. } = &descriptor.runtime else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            appearance_group(appearance, group)
                .get(component)
                .ok_or_else(|| "Blade 外观分量越界".to_owned())?
                .to_string()
        }
        PresetField::BladeView { group, component } => {
            let VegetationPresetRuntime::Blade { view_lod, .. } = &descriptor.runtime else {
                return Err("当前预设不是数据 Blade".to_owned());
            };
            view_group(view_lod, group)
                .get(component)
                .ok_or_else(|| "Blade 视图分量越界".to_owned())?
                .to_string()
        }
        PresetField::BladeMotion(field) => {
            let VegetationPresetRuntime::Blade { motion, .. } = &descriptor.runtime else {
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
            match field {
                BladeMotionField::DirectionX => direction[0],
                BladeMotionField::DirectionZ => direction[1],
                BladeMotionField::Speed => *speed,
                BladeMotionField::TipStrength => *tip_strength,
                BladeMotionField::SpatialScale => *spatial_scale,
            }
            .to_string()
        }
        PresetField::MeshGroundEmbed => {
            let VegetationPresetRuntime::Mesh {
                ground_embed_depth, ..
            } = &descriptor.runtime
            else {
                return Err("当前预设不是数据 Mesh".to_owned());
            };
            ground_embed_depth.to_string()
        }
        PresetField::MeshLod { lod, field } => {
            let lod = mesh_lod(&descriptor.runtime, lod)?;
            match field {
                MeshLodField::StartBegin => lod.start[0].to_string(),
                MeshLodField::StartFull => lod.start[1].to_string(),
                MeshLodField::EndBegin => lod.end[0].to_string(),
                MeshLodField::EndHidden => lod.end[1].to_string(),
            }
        }
        PresetField::MeshPart { lod, part, field } => {
            let part = mesh_part(&descriptor.runtime, lod, part)?;
            match field {
                MeshPartField::PartId => part.part_id.to_string(),
                MeshPartField::MeshPath => part.mesh_path.clone(),
                MeshPartField::MaterialPath => part.material_path.clone(),
                MeshPartField::TranslationX => part.transform.translation[0].to_string(),
                MeshPartField::TranslationY => part.transform.translation[1].to_string(),
                MeshPartField::TranslationZ => part.transform.translation[2].to_string(),
                MeshPartField::YawDegrees => part.transform.yaw_degrees.to_string(),
                MeshPartField::ScaleX => part.transform.scale[0].to_string(),
                MeshPartField::ScaleY => part.transform.scale[1].to_string(),
                MeshPartField::ScaleZ => part.transform.scale[2].to_string(),
                MeshPartField::DerivationBudget => match part.mesh_derivation {
                    MeshPresetMeshDerivation::Direct => String::new(),
                    MeshPresetMeshDerivation::DispersedTriangles { max_triangles } => {
                        max_triangles.to_string()
                    }
                    MeshPresetMeshDerivation::ConvexHull { max_sample_points } => {
                        max_sample_points.to_string()
                    }
                },
                MeshPartField::AlphaCutoff => match part.material_overrides.alpha_mode {
                    Some(MeshPresetAlphaMode::Mask { cutoff }) => cutoff.to_string(),
                    _ => String::new(),
                },
                MeshPartField::WindRootHeight => part
                    .wind
                    .as_ref()
                    .map(|wind| wind.root_height.to_string())
                    .unwrap_or_default(),
                MeshPartField::WindTipHeight => part
                    .wind
                    .as_ref()
                    .map(|wind| wind.tip_height.to_string())
                    .unwrap_or_default(),
                MeshPartField::WindStrength => part
                    .wind
                    .as_ref()
                    .map(|wind| wind.strength.to_string())
                    .unwrap_or_default(),
                MeshPartField::WindFrequency => part
                    .wind
                    .as_ref()
                    .map(|wind| wind.frequency.to_string())
                    .unwrap_or_default(),
            }
        }
        PresetField::Collision(field) => collision_value(&descriptor.runtime, field)?,
    };
    Ok(value)
}

fn blade_scalar(
    runtime: &VegetationPresetRuntime,
    select: impl FnOnce(f32, f32) -> f32,
) -> Result<String, String> {
    let VegetationPresetRuntime::Blade {
        density_gain,
        ultra_horizontal_sweep,
        ..
    } = runtime
    else {
        return Err("当前预设不是数据 Blade".to_owned());
    };
    Ok(select(*density_gain, *ultra_horizontal_sweep).to_string())
}

fn appearance_group(
    appearance: &feathrise_scene_content::BladePresetAppearance,
    group: BladeAppearanceGroup,
) -> &[f32; 4] {
    match group {
        BladeAppearanceGroup::Geometry => &appearance.geometry,
        BladeAppearanceGroup::WidthFlexibility => &appearance.width_flexibility,
        BladeAppearanceGroup::Bending => &appearance.bending,
        BladeAppearanceGroup::AtlasUv => &appearance.atlas_uv,
        BladeAppearanceGroup::RootColor => &appearance.root_color,
        BladeAppearanceGroup::TipColor => &appearance.tip_color,
        BladeAppearanceGroup::CoolTint => &appearance.cool_tint,
        BladeAppearanceGroup::WarmTint => &appearance.warm_tint,
    }
}

fn view_group(view: &feathrise_scene_content::BladePresetViewLod, group: BladeViewGroup) -> &[f32] {
    match group {
        BladeViewGroup::PixelThresholds(index) => view
            .pixel_thresholds
            .get(index)
            .map(|pair| pair.as_slice())
            .unwrap_or(&[]),
        BladeViewGroup::CameraFadeDistances => &view.camera_fade_distances,
        BladeViewGroup::ReferenceGrassHeight => std::slice::from_ref(&view.reference_grass_height),
        BladeViewGroup::FinestGridSpacing => std::slice::from_ref(&view.finest_grid_spacing),
        BladeViewGroup::ResidencyRadii => &view.residency_radii,
        BladeViewGroup::ResidencyMargin => std::slice::from_ref(&view.residency_margin),
        BladeViewGroup::ResidencyTransitionWidths => &view.residency_transition_widths,
    }
}

fn mesh_lod(
    runtime: &VegetationPresetRuntime,
    lod: usize,
) -> Result<&feathrise_scene_content::MeshPresetLod, String> {
    let VegetationPresetRuntime::Mesh { lods, .. } = runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    lods.get(lod).ok_or_else(|| "Mesh LOD 已不存在".to_owned())
}

fn mesh_part(
    runtime: &VegetationPresetRuntime,
    lod: usize,
    part: usize,
) -> Result<&feathrise_scene_content::MeshPresetPart, String> {
    mesh_lod(runtime, lod)?
        .parts
        .get(part)
        .ok_or_else(|| "Mesh 部件已不存在".to_owned())
}

fn collision_value(
    runtime: &VegetationPresetRuntime,
    field: CollisionField,
) -> Result<String, String> {
    let VegetationPresetRuntime::Mesh { collision, .. } = runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let value = match (collision, field) {
        (VegetationPresetCollision::Cylinder { radius, .. }, CollisionField::CylinderRadius) => {
            *radius
        }
        (VegetationPresetCollision::Cylinder { height, .. }, CollisionField::CylinderHeight) => {
            *height
        }
        (VegetationPresetCollision::Cylinder { center_y, .. }, CollisionField::CylinderCenterY) => {
            *center_y
        }
        (VegetationPresetCollision::Cuboid { size, .. }, CollisionField::CuboidSizeX) => size[0],
        (VegetationPresetCollision::Cuboid { size, .. }, CollisionField::CuboidSizeY) => size[1],
        (VegetationPresetCollision::Cuboid { size, .. }, CollisionField::CuboidSizeZ) => size[2],
        (VegetationPresetCollision::Cuboid { center, .. }, CollisionField::CuboidCenterX) => {
            center[0]
        }
        (VegetationPresetCollision::Cuboid { center, .. }, CollisionField::CuboidCenterY) => {
            center[1]
        }
        (VegetationPresetCollision::Cuboid { center, .. }, CollisionField::CuboidCenterZ) => {
            center[2]
        }
        (
            VegetationPresetCollision::Cylinder { enter_distance, .. }
            | VegetationPresetCollision::Cuboid { enter_distance, .. },
            CollisionField::EnterDistance,
        ) => *enter_distance,
        (
            VegetationPresetCollision::Cylinder { exit_distance, .. }
            | VegetationPresetCollision::Cuboid { exit_distance, .. },
            CollisionField::ExitDistance,
        ) => *exit_distance,
        _ => return Err("当前碰撞策略没有该字段".to_owned()),
    };
    Ok(value.to_string())
}
