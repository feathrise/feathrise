//! 从受检Asset构造可立即通过正式目录校验的Blade与Mesh默认规格。

use std::path::Path; // 显示名称与稳定英文ID基名来自所选资产文件名。

use feathrise_map_content::{
    BladePresetAppearance, BladePresetBounds, BladePresetLod, BladePresetMotion,
    BladePresetUltraLodBudget, BladePresetViewLod, MeshPresetLod, ResolvedVegetationPresetCatalog,
    VEGETATION_PRESET_SCHEMA_VERSION, VegetationPresetCatalogAsset, VegetationPresetCollision,
    VegetationPresetDescriptor, VegetationPresetRuntime, stable_vegetation_identity,
}; // 默认值最终仍进入公共强类型校验，不在编辑器维护第二套宽松格式。

use super::types::InspectedAsset; // GLTF分帧检查得到的真实primitive与材质标签。

/// 从所选资产和当前目录构造一个稳定且不碰撞的技术ID。
pub(super) fn unique_preset_id(
    asset_path: &str,
    existing: &[VegetationPresetDescriptor],
    timestamp_millis: u128,
) -> String {
    let stem = Path::new(asset_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("preset"); // 非Unicode文件名在Asset浏览器之前已不可选，此处仍提供稳定兜底。
    let mut slug = String::new();
    let mut separator_pending = false;
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            separator_pending = false;
        } else if !slug.is_empty() {
            separator_pending = true;
        }
    }
    if slug.is_empty() {
        slug.push_str("preset"); // 中文文件名继续使用时间身份，显示名仍完整保留中文。
    }
    let base = format!("{slug}-{timestamp_millis}");
    let mut candidate = base.clone();
    let mut suffix = 2_u64;
    while name_is_occupied(&candidate, existing) {
        candidate = format!("{base}-{suffix}");
        suffix = suffix.saturating_add(1); // 不设置预设数量上限；极端溢出仍保持最后值而不回绕。
    }
    candidate
}

/// 返回规范ID或历史别名是否已经占用名称。
fn name_is_occupied(candidate: &str, existing: &[VegetationPresetDescriptor]) -> bool {
    existing.iter().any(|preset| {
        preset.id == candidate || preset.aliases.iter().any(|alias| alias == candidate)
    })
}

/// 取面向作者的默认名称；中文文件名无需转写。
fn display_name(asset_path: &str) -> String {
    Path::new(asset_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("新植被")
        .to_owned()
}

/// 生成新数据Blade预设；一份GLB少于四个primitive时安全复用最后一档。
pub(super) fn new_blade_preset(
    inspected: &InspectedAsset,
    existing: &[VegetationPresetDescriptor],
    timestamp_millis: u128,
) -> Result<VegetationPresetDescriptor, String> {
    if let Some(error) = inspected.blade_error.as_deref() {
        return Err(error.to_owned());
    }
    let first = inspected
        .blade_mesh_paths
        .first()
        .ok_or_else(|| "所选GLB没有可供Blade使用的零变换Mesh primitive".to_owned())?;
    let id = unique_preset_id(&inspected.source_path, existing, timestamp_millis);
    let spacing = 0.1_f32;
    let mesh_path = |index: usize| {
        inspected
            .blade_mesh_paths
            .get(index)
            .or_else(|| inspected.blade_mesh_paths.last())
            .unwrap_or(first)
            .clone()
    }; // 四档不足时复用而非伪造不存在的GLTF标签。
    let seed = stable_vegetation_identity("blade-lod", &id) as u32;
    let lods = vec![
        blade_lod("near", 0, mesh_path(0), 1.0, 0.0, 46.0, 1.0, seed, 224),
        blade_lod("middle", 1, mesh_path(1), 2.0, 38.0, 104.0, 8.0, seed, 112),
        blade_lod("far", 2, mesh_path(2), 4.0, 88.0, 220.0, 60.0, seed, 56),
        blade_lod(
            "ultra",
            3,
            mesh_path(3),
            8.0,
            196.0,
            1_220.0,
            220.0,
            seed,
            28,
        ),
    ]; // 默认四窗沿用项目已验收的屏幕空间层级与稳定窗口尺度，作者可增删。
    let descriptor = VegetationPresetDescriptor {
        id: id.clone(),
        aliases: Vec::new(),
        label: display_name(&inspected.source_path),
        output_id: id,
        author_asset_path: inspected.source_path.clone(),
        notes: Some(inspected.source_path.clone()), // 新建时把真实来源作为可自由修改、清空的备注初值。
        runtime_layer_id: None,
        prototype_id: None,
        resolution: [512, 512],
        spacing_meters: spacing,
        scale_range: [0.85, 1.15],
        seed: None,
        runtime: VegetationPresetRuntime::Blade {
            lods,
            appearance: default_blade_appearance(),
            base_color_texture_path: None,
            motion: BladePresetMotion::Wind,
            bounds: BladePresetBounds {
                max_blade_height: 0.9,
                max_horizontal_sweep: 0.78,
                reference_height: 0.7,
                root_penetration: 0.03,
            },
            view_lod: BladePresetViewLod {
                // 细→粗：中近、远中、超远转远。
                pixel_thresholds: vec![[40.0, 56.0], [12.0, 20.0], [3.5, 5.0]],
                camera_fade_distances: [0.18, 0.55],
                reference_grass_height: 0.7,
                finest_grid_spacing: spacing,
                residency_radii: vec![46.0, 104.0, 220.0, 1_220.0],
                residency_margin: 2.0,
                residency_transition_widths: vec![8.0, 16.0, 64.0],
            },
            density_gain: 1.0,
            ultra_lod_budget: BladePresetUltraLodBudget::Medium,
            ultra_horizontal_sweep: 0.0,
        },
    };
    validate_single(&descriptor)?;
    Ok(descriptor)
}

/// 构造一档Blade窗口，格距倍率与作者默认间距共享同一基准。
#[allow(clippy::too_many_arguments)] // 字段与正式BladePresetLod一一对应，调用点更容易审查四窗差异。
fn blade_lod(
    name: &str,
    level: u32,
    mesh_path: String,
    spacing_scale: f32,
    inner_radius: f32,
    outer_radius: f32,
    outer_fade_width: f32,
    seed: u32,
    snap_cells: u32,
) -> BladePresetLod {
    BladePresetLod {
        name: name.to_owned(),
        level,
        mesh_path,
        grid_side: if level == 3 { 3_521 } else { 1_345 },
        spacing: 0.1 * spacing_scale,
        inner_radius,
        outer_radius,
        inner_fade_width: if level == 0 { 0.0 } else { 1.0 },
        outer_fade_width,
        seed: seed.wrapping_add(level).max(1),
        snap_cells,
    }
}

/// 普通草本的中性综合色与柔韧度默认值。
fn default_blade_appearance() -> BladePresetAppearance {
    BladePresetAppearance {
        geometry: [0.50, 0.90, 0.03, 0.085],
        width_flexibility: [0.82, 1.48, 0.64, 1.0],
        bending: [0.40, 0.48, 1.0, 1.0],
        atlas_uv: [0.5, 1.0, 0.0, 0.0],
        root_color: [0.0048, 0.0097, 0.0030, 0.0],
        tip_color: [0.0482, 0.1248, 0.0212, 0.0],
        cool_tint: [0.91, 1.01, 1.08, 0.0],
        warm_tint: [1.12, 0.98, 0.80, 0.0],
    }
}

/// 生成新通用Mesh预设；所选GLB的全部primitive共同组成近档。
pub(super) fn new_mesh_preset(
    inspected: &InspectedAsset,
    existing: &[VegetationPresetDescriptor],
    timestamp_millis: u128,
) -> Result<VegetationPresetDescriptor, String> {
    if inspected.mesh_parts.is_empty() {
        return Err("所选GLB没有可用Mesh primitive".to_owned());
    }
    let id = unique_preset_id(&inspected.source_path, existing, timestamp_millis);
    let descriptor = VegetationPresetDescriptor {
        id: id.clone(),
        aliases: Vec::new(),
        label: display_name(&inspected.source_path),
        output_id: id,
        author_asset_path: inspected.source_path.clone(),
        notes: Some(inspected.source_path.clone()), // 来源仅提供作者说明初值，不再占用必填模型配置行。
        runtime_layer_id: None,
        prototype_id: None,
        resolution: [512, 512],
        spacing_meters: 2.0,
        scale_range: [0.85, 1.15],
        seed: None,
        runtime: VegetationPresetRuntime::Mesh {
            lods: vec![MeshPresetLod {
                level: 0,
                start: [0.0, 8.0],
                end: [80.0, 100.0],
                parts: inspected.mesh_parts.clone(),
            }],
            ground_embed_depth: 0.0,
            collision: VegetationPresetCollision::None,
        },
    };
    validate_single(&descriptor)?;
    Ok(descriptor)
}

/// 新建默认值也必须通过运行时同一份完整校验。
fn validate_single(descriptor: &VegetationPresetDescriptor) -> Result<(), String> {
    ResolvedVegetationPresetCatalog::build(&VegetationPresetCatalogAsset {
        schema_version: VEGETATION_PRESET_SCHEMA_VERSION,
        presets: vec![descriptor.clone()],
    })
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_map_content::{
        MeshPresetMaterialOverrides, MeshPresetMeshDerivation, MeshPresetPart,
        MeshPresetPartTransform, VegetationPresetBillboard,
    };

    /// 构造一个只含单primitive的受检资产。
    fn inspected() -> InspectedAsset {
        InspectedAsset {
            source_path: "gltf/blender/植被/杨树.glb".to_owned(),
            blade_mesh_paths: vec!["gltf/blender/植被/杨树.glb#Mesh0/Primitive0".to_owned()],
            blade_error: None,
            mesh_parts: vec![MeshPresetPart {
                part_id: 0,
                mesh_path: "gltf/blender/植被/杨树.glb#Mesh0/Primitive0".to_owned(),
                mesh_derivation: MeshPresetMeshDerivation::Direct, // 新建默认直接使用受检源网格。
                material_path: "gltf/blender/植被/杨树.glb#Material0/std".to_owned(),
                material_overrides: MeshPresetMaterialOverrides::default(), // 不猜作者材质覆盖。
                transform: MeshPresetPartTransform::default(),
                casts_shadow: true,
                billboard: VegetationPresetBillboard::Fixed,
                wind: None,
            }],
        }
    }

    #[test]
    fn chinese_asset_gets_non_reused_stable_identity() {
        let first = new_mesh_preset(&inspected(), &[], 42).expect("默认Mesh应通过正式校验");
        let second = new_mesh_preset(&inspected(), std::slice::from_ref(&first), 42)
            .expect("同毫秒碰撞应继续寻找后缀");
        assert_eq!(first.id, "preset-42");
        assert_eq!(second.id, "preset-42-2");
        assert_eq!(first.label, "杨树");
    }

    #[test]
    fn generated_blade_and_mesh_defaults_use_public_validation() {
        let blade = new_blade_preset(&inspected(), &[], 100).expect("Blade默认值应合法");
        let mesh = new_mesh_preset(&inspected(), std::slice::from_ref(&blade), 101)
            .expect("Mesh默认值应合法");
        ResolvedVegetationPresetCatalog::build(&VegetationPresetCatalogAsset {
            schema_version: VEGETATION_PRESET_SCHEMA_VERSION,
            presets: vec![blade, mesh],
        })
        .expect("两个自动预设应可共同进入目录");
    }
}
