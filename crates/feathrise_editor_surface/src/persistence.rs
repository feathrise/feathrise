//! 全局 PBR 地表目录、内容寻址纹理数组和场景引用保护事务。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::IoTaskPool;
use bevy::tasks::futures::check_ready;
use image::imageops::FilterType;
use feathrise_scene_content::{
    SCENE_CATALOG_ASSET_PATH, SCENE_SCHEMA_VERSION, SCENE_SURFACE_SCHEMA_VERSION, SceneCatalogAsset,
    SceneSurfaceAsset, ResolvedSurfacePresetCatalog, SURFACE_PRESET_CATALOG_ASSET_PATH,
    SURFACE_PRESET_SCHEMA_VERSION, SurfacePresetCatalogAsset, SurfacePresetDescriptor,
    SurfacePresetRegistry,
};
use sha2::{Digest, Sha256};

use super::types::{
    SurfaceCatalogAdoption, SurfaceCatalogWriteResult, SurfaceDraftMode, SurfacePresetEditorState,
};
use crate::helpers::{
    Rgba8ArrayKind, commit_owned_files, encode_rgba8_array, safe_asset_path, workspace_asset_root,
};
use feathrise_editor_core::PresetWorkspaceState;
use feathrise_editor_surface_model::{host::SurfaceEditorHost, system_params::*};

#[derive(Clone, Debug)]
enum SurfaceCatalogMutation {
    Upsert {
        original_id: Option<String>,
        descriptor: SurfacePresetDescriptor,
    },
    Delete {
        id: String,
    },
}

pub(super) fn start_upsert(
    state: &mut SurfacePresetEditorState,
    registry: &SurfacePresetRegistry,
    mode: &SurfaceDraftMode,
    descriptor: SurfacePresetDescriptor,
    tab_id: u64,
) -> Result<(), String> {
    ensure_writable(state, registry)?;
    let feedback_target =
        feathrise_editor_core::EditorStatusTarget::new(&descriptor.id, &descriptor.label);
    let mutation = SurfaceCatalogMutation::Upsert {
        original_id: match mode {
            SurfaceDraftMode::Create => None,
            SurfaceDraftMode::Edit { original_id } => Some(original_id.clone()),
        },
        descriptor,
    };
    let baseline = baseline_catalog(registry);
    mutate_catalog(baseline.clone(), &mutation)?; // 提前验证结构；即使字段未变也允许重建同路径下已被美术替换的源贴图。
    let baseline_revision = registry.revision;
    let root = workspace_asset_root();
    state.status = "正在后台重采样贴图并生成内容寻址 PBR 数组…".to_owned();
    state.save_feedback_target = Some(feedback_target); // 所有原预检通过后才登记反馈，不改变任务输入。
    state.save_feedback_error = None;
    state.write_task = Some(IoTaskPool::get().spawn(async move {
        write_catalog_transaction(root, baseline, mutation, baseline_revision, Some(tab_id))
    }));
    Ok(())
}

pub(super) fn start_delete(
    state: &mut SurfacePresetEditorState,
    registry: &SurfacePresetRegistry,
    editor: &dyn SurfaceEditorHost,
    preset: &SurfacePresetDescriptor,
) -> Result<(), String> {
    ensure_writable(state, registry)?;
    let names = preset_reference_names(preset);
    if editor.draft_references(&names) {
        return Err("当前未保存场景仍引用该地表预设；请先完成该场景层的替换或迁移".to_owned());
    }
    let baseline = baseline_catalog(registry);
    let mutation = SurfaceCatalogMutation::Delete {
        id: preset.id.clone(),
    };
    mutate_catalog(baseline.clone(), &mutation)?;
    let baseline_revision = registry.revision;
    let root = workspace_asset_root();
    state.status = "正在后台扫描全部场景引用；无引用后才会重建数组并删除…".to_owned();
    state.save_feedback_target = None; // 删除沿用原提示，不占用本批保存回执。
    state.write_task = Some(IoTaskPool::get().spawn(async move {
        write_catalog_transaction(root, baseline, mutation, baseline_revision, None)
    }));
    Ok(())
}

fn ensure_writable(
    state: &SurfacePresetEditorState,
    registry: &SurfacePresetRegistry,
) -> Result<(), String> {
    if state.busy() {
        return Err("已有地表目录保存或热采用任务正在进行".to_owned());
    }
    if registry.revision == 0 {
        return Err("地表预设目录尚未形成第一份有效快照".to_owned());
    }
    if let Some(error) = registry.last_error.as_deref() {
        return Err(format!("当前地表目录加载失败，修复后才能写入：{error}"));
    }
    Ok(())
}

fn baseline_catalog(registry: &SurfacePresetRegistry) -> SurfacePresetCatalogAsset {
    let (base, normal, arm_height) = registry.resolved.array_paths();
    SurfacePresetCatalogAsset {
        schema_version: SURFACE_PRESET_SCHEMA_VERSION,
        texture_resolution: registry.resolved.texture_resolution(),
        base_color_array_path: base.to_owned(),
        normal_array_path: normal.to_owned(),
        arm_height_array_path: arm_height.to_owned(),
        presets: registry.presets().to_vec(),
    }
}

fn mutate_catalog(
    mut catalog: SurfacePresetCatalogAsset,
    mutation: &SurfaceCatalogMutation,
) -> Result<SurfacePresetCatalogAsset, String> {
    match mutation {
        SurfaceCatalogMutation::Upsert {
            original_id: None,
            descriptor,
        } => catalog.presets.push(descriptor.clone()),
        SurfaceCatalogMutation::Upsert {
            original_id: Some(original_id),
            descriptor,
        } => {
            let existing = catalog
                .presets
                .iter_mut()
                .find(|preset| preset.id == *original_id)
                .ok_or_else(|| format!("待编辑地表预设{original_id}已不存在"))?;
            if descriptor.id != existing.id {
                return Err("编辑不能修改稳定 ID；场景始终按该身份引用预设".to_owned());
            }
            *existing = descriptor.clone();
        }
        SurfaceCatalogMutation::Delete { id } => {
            let index = catalog
                .presets
                .iter()
                .position(|preset| preset.id == *id)
                .ok_or_else(|| format!("待删除地表预设{id}已不存在"))?;
            catalog.presets.remove(index);
        }
    }
    ResolvedSurfacePresetCatalog::build(&catalog)?;
    Ok(catalog)
}

fn write_catalog_transaction(
    root: PathBuf,
    baseline: SurfacePresetCatalogAsset,
    mutation: SurfaceCatalogMutation,
    baseline_revision: u64,
    tab_id: Option<u64>,
) -> Result<SurfaceCatalogWriteResult, String> {
    let catalog_path = safe_asset_path(&root, SURFACE_PRESET_CATALOG_ASSET_PATH)?;
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取地表预设目录{}失败：{error}", catalog_path.display()))?;
    let disk: SurfacePresetCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析地表预设目录{}失败：{error}", catalog_path.display()))?;
    ResolvedSurfacePresetCatalog::build(&disk)?;
    if disk != baseline {
        return Err("地表预设目录已被外部修改；已拒绝覆盖，请等待热加载后重试".to_owned());
    }
    if let SurfaceCatalogMutation::Delete { id } = &mutation {
        let preset = disk
            .presets
            .iter()
            .find(|preset| preset.id == *id)
            .ok_or_else(|| format!("待删除地表预设{id}已不存在"))?;
        let references = scan_scene_references(&root, preset)?;
        if !references.is_empty() {
            return Err(format!(
                "不能删除地表预设{}：仍被{}引用",
                preset.label,
                references.join("、")
            ));
        }
    }
    let mut updated = mutate_catalog(disk, &mutation)?;
    let (base_array, normal_array, arm_height_array) = build_texture_arrays(&root, &updated)?;
    let digest = array_digest(&base_array, &normal_array, &arm_height_array);
    updated.base_color_array_path = format!("config/surface_presets/base-{digest}.ktx2");
    updated.normal_array_path = format!("config/surface_presets/normal-{digest}.ktx2");
    updated.arm_height_array_path = format!("config/surface_presets/arm-height-{digest}.ktx2");
    ResolvedSurfacePresetCatalog::build(&updated)?;

    let mut catalog_bytes = serde_json::to_vec_pretty(&updated)
        .map_err(|error| format!("序列化地表预设目录失败：{error}"))?;
    catalog_bytes.push(b'\n');
    // 三张不可变内容寻址数组先于目录发布；若任一步失败，公共事务会恢复全部旧目标。
    commit_owned_files(vec![
        (
            safe_asset_path(&root, &updated.base_color_array_path)?,
            base_array,
        ),
        (
            safe_asset_path(&root, &updated.normal_array_path)?,
            normal_array,
        ),
        (
            safe_asset_path(&root, &updated.arm_height_array_path)?,
            arm_height_array,
        ),
        (catalog_path, catalog_bytes),
    ])?;
    let (preset_id, deleted, message) = match mutation {
        SurfaceCatalogMutation::Upsert {
            original_id: None,
            descriptor,
        } => (
            descriptor.id,
            false,
            format!("已新建 PBR 地表预设“{}”", descriptor.label),
        ),
        SurfaceCatalogMutation::Upsert {
            original_id: Some(_),
            descriptor,
        } => (
            descriptor.id,
            false,
            format!("已更新 PBR 地表预设“{}”", descriptor.label),
        ),
        SurfaceCatalogMutation::Delete { id } => {
            (id, true, "已安全删除 PBR 地表预设并重建数组".to_owned())
        }
    };
    Ok(SurfaceCatalogWriteResult {
        baseline_revision,
        preset_id,
        deleted,
        tab_id,
        message,
    })
}

fn build_texture_arrays(
    root: &Path,
    catalog: &SurfacePresetCatalogAsset,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let resolution = catalog.texture_resolution;
    // 同一路径常被多个仅乘色或参数不同的预设复用；一次事务只解码、缩放一次，
    // 后续切片克隆已经统一尺寸的 RGBA 数据，避免重复读取 4K 源图拖慢开发者保存。
    let bases = build_texture_slices(
        root,
        catalog,
        |preset| preset.base_color_texture_path.as_deref(),
        [255, 255, 255, 255],
        false,
        false,
    )?;
    let normals = build_texture_slices(
        root,
        catalog,
        |preset| preset.normal_texture_path.as_deref(),
        [128, 128, 255, 255],
        false,
        true,
    )?;
    let arm_heights = build_texture_slices(
        root,
        catalog,
        |preset| preset.arm_height_texture_path.as_deref(),
        [255, 255, 255, 128],
        true,
        false,
    )?;
    Ok((
        encode_rgba8_array(&bases, resolution, Rgba8ArrayKind::SrgbColor)?,
        encode_rgba8_array(&normals, resolution, Rgba8ArrayKind::TangentNormal)?,
        encode_rgba8_array(&arm_heights, resolution, Rgba8ArrayKind::LinearData)?,
    ))
}

fn build_texture_slices<'a>(
    root: &Path,
    catalog: &'a SurfacePresetCatalogAsset,
    source_path: impl Fn(&'a SurfacePresetDescriptor) -> Option<&'a str>,
    fallback: [u8; 4],
    neutral_alpha_without_source_alpha: bool,
    normalize_tangent_normals: bool,
) -> Result<Vec<Vec<u8>>, String> {
    let mut source_to_slice = BTreeMap::<Option<String>, usize>::new();
    let mut slices = Vec::<Vec<u8>>::with_capacity(catalog.presets.len());
    for preset in &catalog.presets {
        let key = source_path(preset).map(str::to_owned);
        let slice = if let Some(index) = source_to_slice.get(&key).copied() {
            slices[index].clone() // 数组编码需要每层连续数据，但无需再次解码和Lanczos重采样。
        } else {
            let slice = load_rgba_slice(
                root,
                key.as_deref(),
                catalog.texture_resolution,
                fallback,
                neutral_alpha_without_source_alpha,
                normalize_tangent_normals,
            )?;
            source_to_slice.insert(key, slices.len());
            slice
        };
        slices.push(slice);
    }
    Ok(slices)
}

fn load_rgba_slice(
    root: &Path,
    relative: Option<&str>,
    resolution: [u32; 2],
    fallback: [u8; 4],
    neutral_alpha_without_source_alpha: bool,
    normalize_tangent_normals: bool,
) -> Result<Vec<u8>, String> {
    let Some(relative) = relative else {
        let pixel_count = usize::try_from(resolution[0])
            .ok()
            .and_then(|width| {
                usize::try_from(resolution[1])
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| "PBR 回退切片尺寸超出当前平台可表达范围".to_owned())?;
        let byte_count = pixel_count
            .checked_mul(fallback.len())
            .ok_or_else(|| "PBR 回退切片字节数溢出".to_owned())?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(byte_count)
            .map_err(|error| format!("无法为 PBR 回退切片分配{byte_count}字节：{error}"))?;
        for _ in 0..pixel_count {
            output.extend_from_slice(&fallback);
        }
        return Ok(output);
    };
    let path = safe_asset_path(root, relative)?;
    let image = image::open(&path)
        .map_err(|error| format!("解码 PBR 源图{}失败：{error}", path.display()))?;
    let had_alpha = image.color().has_alpha();
    let rgba = image.to_rgba8();
    let mut resized =
        image::imageops::resize(&rgba, resolution[0], resolution[1], FilterType::Lanczos3)
            .into_raw();
    if neutral_alpha_without_source_alpha && !had_alpha {
        for pixel in resized.chunks_exact_mut(4) {
            pixel[3] = 128; // 普通RGB ARM没有高度通道，使用中性高度而非错误的全白凸起。
        }
    }
    if normalize_tangent_normals {
        for pixel in resized.chunks_exact_mut(4) {
            let x = f32::from(pixel[0]) / 127.5 - 1.0;
            let y = f32::from(pixel[1]) / 127.5 - 1.0;
            let z = f32::from(pixel[2]) / 127.5 - 1.0;
            let length = (x * x + y * y + z * z).sqrt();
            let normal = if length > 0.000_001 {
                [x / length, y / length, z / length]
            } else {
                [0.0, 0.0, 1.0]
            };
            for (target, value) in pixel[..3].iter_mut().zip(normal) {
                *target = ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(resized)
}

fn array_digest(base: &[u8], normal: &[u8], arm_height: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(base);
    hasher.update(normal);
    hasher.update(arm_height);
    hasher
        .finalize()
        .iter()
        .take(10)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn scan_scene_references(
    root: &Path,
    preset: &SurfacePresetDescriptor,
) -> Result<Vec<String>, String> {
    let catalog_path = safe_asset_path(root, SCENE_CATALOG_ASSET_PATH)?;
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取场景目录{}失败：{error}", catalog_path.display()))?;
    let catalog: SceneCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析场景目录{}失败：{error}", catalog_path.display()))?;
    if catalog.schema_version != SCENE_SCHEMA_VERSION {
        return Err("场景目录版本不一致，不能安全删除地表预设".to_owned());
    }
    let names = preset_reference_names(preset);
    let mut references = Vec::new();
    for entry in catalog.scenes {
        let Some(relative) = entry.surface_path.as_deref() else {
            continue;
        };
        let path = safe_asset_path(root, relative)?;
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("读取场景{}地表文件失败：{error}", entry.id))?;
        let surface: SceneSurfaceAsset = serde_json::from_slice(&bytes)
            .map_err(|error| format!("解析场景{}地表文件失败：{error}", entry.id))?;
        if surface.schema_version != SCENE_SURFACE_SCHEMA_VERSION || surface.scene_id != entry.id {
            return Err(format!(
                "场景{}地表版本或身份不一致，不能安全删除预设",
                entry.id
            ));
        }
        if surface
            .layers
            .iter()
            .any(|layer| names.contains(layer.preset_id.as_str()))
        {
            references.push(format!("{}({})", entry.display_name, entry.id));
        }
    }
    Ok(references)
}

fn preset_reference_names(preset: &SurfacePresetDescriptor) -> BTreeSet<&str> {
    std::iter::once(preset.id.as_str())
        .chain(preset.aliases.iter().map(String::as_str))
        .collect()
}

/// 文件发布后只重载目录；内容寻址数组由新目录句柄首次加载，不会与旧切片表混用。
pub(crate) fn poll_catalog_write(
    editor: &mut dyn SurfaceEditorHost,
    workspace: &mut PresetWorkspaceState,
    params: PersistenceParams,
) {
    let PersistenceParams {
        mut state,
        registry,
        asset_server,
    } = params;
    if let Some(task) = state.write_task.as_mut()
        && let Some(result) = check_ready(task)
    {
        state.write_task = None;
        match result {
            Ok(result) => {
                asset_server.reload(SURFACE_PRESET_CATALOG_ASSET_PATH);
                state.status = "文件批次已发布，正在等待运行时原子采用新目录…".to_owned();
                state.adoption = Some(SurfaceCatalogAdoption {
                    baseline_revision: result.baseline_revision,
                    preset_id: result.preset_id,
                    deleted: result.deleted,
                    tab_id: result.tab_id,
                    message: result.message,
                });
            }
            Err(error) => {
                editor.set_status(error.clone());
                state.status = error;
                if let Some(target) = state.save_feedback_target.take() {
                    editor.record_operation_result(
                        feathrise_editor_core::EditorStatusOperation::SurfacePresetSave,
                        &target,
                        feathrise_editor_core::EditorStatusKind::Error,
                    );
                }
            }
        }
    }
    let Some(adoption) = state.adoption.as_ref() else {
        return;
    };
    let registry_changed = registry
        .as_ref()
        .is_some_and(|registry| registry.is_changed());
    let Some(registry) = registry.as_deref() else {
        return;
    };
    if registry.revision <= adoption.baseline_revision {
        if registry_changed || state.save_feedback_error.is_none() {
            state.observe_save_adoption_error(editor, registry.last_error.as_deref()); // 只观察失败证据；原等待、重试和采用资格不变。
        }
        return;
    }
    let adopted = registry.resolve(&adoption.preset_id).is_some();
    if adopted == adoption.deleted {
        let message = registry.last_error.as_deref().map_or_else(
            || "目录修订已变化，但目标预设状态不匹配；可能又被外部修改".to_owned(),
            |error| format!("文件已发布，但新目录热采用失败：{error}"),
        );
        editor.set_status(message.clone());
        state.status = message;
        if let Some(target) = state.save_feedback_target.take() {
            editor.record_operation_result(
                feathrise_editor_core::EditorStatusOperation::SurfacePresetSave,
                &target,
                feathrise_editor_core::EditorStatusKind::Error,
            );
        }
        state.adoption = None;
        return;
    }
    let adoption = state.adoption.take().expect("上方已确认采用状态");
    if adoption.deleted {
        if editor.selected_preset() == Some(&adoption.preset_id) {
            editor.set_selected_preset(None);
        }
    } else {
        editor.set_selected_preset(Some(adoption.preset_id.clone()));
    }
    if let Some(tab_id) = adoption.tab_id
        && let Some(saved) = registry.resolve(&adoption.preset_id).cloned()
        && let Some(draft) = state.draft_mut(tab_id)
    {
        draft.mode = SurfaceDraftMode::Edit {
            original_id: saved.id.clone(),
        };
        draft.descriptor = saved.clone();
        draft.baseline = saved;
    }
    state.active_field = None;
    state.input_buffer.clear();
    state.touch_ui();
    workspace.touch_ui();
    editor.set_status(adoption.message.clone());
    state.status = adoption.message;
    if let Some(target) = state.save_feedback_target.take() {
        editor.record_operation_result(
            feathrise_editor_core::EditorStatusOperation::SurfacePresetSave,
            &target,
            feathrise_editor_core::EditorStatusKind::Success,
        ); // 只在原有效目录采用分支发布最终成功，落盘阶段保持处理中。
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::SurfaceProjection;

    fn preset(id: &str) -> SurfacePresetDescriptor {
        SurfacePresetDescriptor {
            id: id.to_owned(),
            aliases: Vec::new(),
            label: id.to_owned(),
            base_color_srgba: [1.0; 4],
            base_color_texture_path: None,
            normal_texture_path: None,
            arm_height_texture_path: None,
            tile_meters: 2.0,
            normal_strength: 1.0,
            normal_y_flip: false,
            height_blend_strength: 0.0,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            crater_response: 1.0, // 测试作者预设保持旧默认。
            projection: SurfaceProjection::WorldXz,
        }
    }

    #[test]
    fn editing_preserves_stable_identity_and_deleting_last_fallback_is_rejected() {
        let baseline = SurfacePresetCatalogAsset {
            schema_version: SURFACE_PRESET_SCHEMA_VERSION,
            texture_resolution: [4, 4],
            base_color_array_path: "config/surface/base.ktx2".to_owned(),
            normal_array_path: "config/surface/normal.ktx2".to_owned(),
            arm_height_array_path: "config/surface/arm.ktx2".to_owned(),
            presets: vec![preset("soil")],
        };
        let mut renamed = preset("other");
        renamed.label = "新名称".to_owned();
        assert!(
            mutate_catalog(
                baseline.clone(),
                &SurfaceCatalogMutation::Upsert {
                    original_id: Some("soil".to_owned()),
                    descriptor: renamed,
                },
            )
            .is_err()
        );
        assert!(
            mutate_catalog(
                baseline,
                &SurfaceCatalogMutation::Delete {
                    id: "soil".to_owned()
                },
            )
            .is_err()
        );
    }
}
