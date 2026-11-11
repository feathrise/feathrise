//! 水体预设目录的乐观并发、全场景引用保护、原子写入与热采用。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::IoTaskPool;
use bevy::tasks::futures::check_ready;
use feathrise_scene_content::{
    SCENE_CATALOG_ASSET_PATH, SCENE_SCHEMA_VERSION, SCENE_WATER_FILE_NAME, SceneCatalogAsset, SceneManifestAsset,
    SceneTerrainKind, SceneWaterAsset,
    ResolvedWaterPresetCatalog, WATER_PRESET_CATALOG_ASSET_PATH, WATER_PRESET_SCHEMA_VERSION,
    WaterPresetCatalogAsset, WaterPresetDescriptor, WaterPresetRegistry,
};

use super::types::{
    WaterCatalogAdoption, WaterCatalogWriteResult, WaterDraftMode, WaterPresetEditorState,
};
use crate::authoring::{
    EditorAuthoringSession, commit_owned_files, safe_asset_path, workspace_asset_root,
};
use crate::resources::SceneEditorState;
use crate::ui::vegetation_preset_editor::VegetationPresetEditorState;

mod delete_preflight;
pub(super) use delete_preflight::{WaterDeletePreflight, advance_delete_preflight};

#[derive(Clone, Debug)]
enum WaterCatalogMutation {
    Upsert {
        original_id: Option<String>,
        descriptor: WaterPresetDescriptor,
    },
    Delete {
        id: String,
    },
}

pub(super) fn start_upsert(
    state: &mut WaterPresetEditorState,
    registry: &WaterPresetRegistry,
    mode: &WaterDraftMode,
    descriptor: WaterPresetDescriptor,
    tab_id: u64,
) -> Result<(), String> {
    ensure_writable(state, registry)?;
    let feedback_target = crate::types::EditorStatusTarget::new(&descriptor.id, &descriptor.label);
    let mutation = WaterCatalogMutation::Upsert {
        original_id: match mode {
            WaterDraftMode::Create => None,
            WaterDraftMode::Edit { original_id } => Some(original_id.clone()),
        },
        descriptor,
    };
    let baseline = baseline_catalog(registry);
    mutate_catalog(baseline.clone(), &mutation)?;
    let baseline_revision = registry.revision;
    let root = workspace_asset_root();
    state.status = "正在后台校验并原子保存水体预设目录…".to_owned();
    state.save_feedback_target = Some(feedback_target);
    state.save_feedback_error = None;
    state.write_task = Some(IoTaskPool::get().spawn(async move {
        write_catalog_transaction(root, baseline, mutation, baseline_revision, Some(tab_id))
    }));
    Ok(())
}

pub(super) fn start_delete(
    state: &mut WaterPresetEditorState,
    registry: &WaterPresetRegistry,
    session: &mut EditorAuthoringSession,
    preset: &WaterPresetDescriptor,
) -> Result<(), String> {
    ensure_writable(state, registry)?;
    if session.saving || session.draft.as_ref().is_some_and(|draft| draft.terrain_documents.loading) {
        return Err("场景保存或画布加载尚未完成，请稍后删除水体预设".into());
    }
    // 不复制草稿或扫描全部水层；停放画布也由有界游标检查，来源变化直接拒绝。
    state.delete_preflight = Some(WaterDeletePreflight::new(session, registry, preset.id.clone()));
    session.water_preset_delete_pending = true;
    state.status = "正在检查当前与停放草稿的水体引用；再次点击删除可取消尚未写入的检查…".to_owned();
    state.save_feedback_target = None;
    Ok(())
}

fn ensure_writable(
    state: &WaterPresetEditorState,
    registry: &WaterPresetRegistry,
) -> Result<(), String> {
    if state.busy() {
        return Err("已有水体目录保存或热采用任务正在进行".to_owned());
    }
    if registry.revision == 0 {
        return Err("水体预设目录尚未形成第一份有效快照".to_owned());
    }
    if let Some(error) = registry.last_error.as_deref() {
        return Err(format!("当前水体目录加载失败，修复后才能写入：{error}"));
    }
    Ok(())
}

fn baseline_catalog(registry: &WaterPresetRegistry) -> WaterPresetCatalogAsset {
    WaterPresetCatalogAsset {
        schema_version: WATER_PRESET_SCHEMA_VERSION,
        presets: registry.presets().to_vec(),
    }
}

fn mutate_catalog(
    mut catalog: WaterPresetCatalogAsset,
    mutation: &WaterCatalogMutation,
) -> Result<WaterPresetCatalogAsset, String> {
    match mutation {
        WaterCatalogMutation::Upsert {
            original_id: None,
            descriptor,
        } => {
            catalog.presets.push(descriptor.clone());
        }
        WaterCatalogMutation::Upsert {
            original_id: Some(original_id),
            descriptor,
        } => {
            let existing = catalog
                .presets
                .iter_mut()
                .find(|preset| preset.id == *original_id)
                .ok_or_else(|| format!("待编辑水体预设{original_id}已不存在"))?;
            if descriptor.id != existing.id {
                return Err("编辑不能修改稳定 ID；场景始终按该身份引用预设".to_owned());
            }
            *existing = descriptor.clone();
        }
        WaterCatalogMutation::Delete { id } => {
            let index = catalog
                .presets
                .iter()
                .position(|preset| preset.id == *id)
                .ok_or_else(|| format!("待删除水体预设{id}已不存在"))?;
            catalog.presets.remove(index);
        }
    }
    ResolvedWaterPresetCatalog::build(&catalog)?;
    Ok(catalog)
}

fn write_catalog_transaction(
    root: PathBuf,
    baseline: WaterPresetCatalogAsset,
    mutation: WaterCatalogMutation,
    baseline_revision: u64,
    tab_id: Option<u64>,
) -> Result<WaterCatalogWriteResult, String> {
    let catalog_path = safe_asset_path(&root, WATER_PRESET_CATALOG_ASSET_PATH)?;
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取水体预设目录{}失败：{error}", catalog_path.display()))?;
    let disk: WaterPresetCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析水体预设目录{}失败：{error}", catalog_path.display()))?;
    ResolvedWaterPresetCatalog::build(&disk)?;
    if disk != baseline {
        return Err("水体预设目录已被外部修改；已拒绝覆盖，请等待热加载后重试".to_owned());
    }
    if let WaterCatalogMutation::Delete { id } = &mutation {
        let preset = disk
            .presets
            .iter()
            .find(|preset| preset.id == *id)
            .ok_or_else(|| format!("待删除水体预设{id}已不存在"))?;
        let references = scan_scene_references(&root, preset)?;
        if !references.is_empty() {
            return Err(format!(
                "不能删除水体预设{}：仍被{}引用",
                preset.label,
                references.join("、")
            ));
        }
    }
    let updated = mutate_catalog(disk, &mutation)?;
    let mut catalog_bytes = serde_json::to_vec_pretty(&updated)
        .map_err(|error| format!("序列化水体预设目录失败：{error}"))?;
    catalog_bytes.push(b'\n');
    commit_owned_files(vec![(catalog_path, catalog_bytes)])?;
    let (preset_id, deleted, message) = match mutation {
        WaterCatalogMutation::Upsert {
            original_id: None,
            descriptor,
        } => (
            descriptor.id,
            false,
            format!("已新建水体预设“{}”", descriptor.label),
        ),
        WaterCatalogMutation::Upsert {
            original_id: Some(_),
            descriptor,
        } => (
            descriptor.id,
            false,
            format!("已更新水体预设“{}”", descriptor.label),
        ),
        WaterCatalogMutation::Delete { id } => (id, true, "已安全删除水体预设".to_owned()),
    };
    Ok(WaterCatalogWriteResult {
        baseline_revision,
        preset_id,
        deleted,
        tab_id,
        message,
    })
}

fn scan_scene_references(root: &Path, preset: &WaterPresetDescriptor) -> Result<Vec<String>, String> {
    let catalog_path = safe_asset_path(root, SCENE_CATALOG_ASSET_PATH)?;
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取场景目录{}失败：{error}", catalog_path.display()))?;
    let catalog: SceneCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析场景目录{}失败：{error}", catalog_path.display()))?;
    if catalog.schema_version != SCENE_SCHEMA_VERSION {
        return Err("场景目录版本不一致，不能安全删除水体预设".to_owned());
    }
    let names = preset_reference_names(preset);
    let mut references = Vec::new();
    for entry in catalog.scenes {
        // 根水与独立网格私有水都是正式引用，不能仅扫描目录的 water_path。
        // 全部读取在现有 IoTaskPool 事务内执行；任何来源不明都不能作为“无引用”。
        let manifest_path = safe_asset_path(root, &entry.manifest_path())?;
        let bytes = std::fs::read(&manifest_path)
            .map_err(|error| format!("读取场景{}清单失败：{error}", entry.id))?;
        let manifest: SceneManifestAsset = serde_json::from_slice(&bytes)
            .map_err(|error| format!("解析场景{}清单失败：{error}", entry.id))?;
        if manifest.schema_version != SCENE_SCHEMA_VERSION || manifest.id != entry.id
            || !manifest.is_data_driven()
        {
            return Err(format!(
                "场景{}清单版本、身份或场景类型不受支持，不能安全删除预设",
                entry.id
            ));
        }
        let optional_legacy = (entry.water_path.is_none()
            && matches!(manifest.terrain.kind, SceneTerrainKind::Empty | SceneTerrainKind::SparseClipmap))
            .then(|| format!("{}/{}", entry.folder.trim_end_matches('/'), SCENE_WATER_FILE_NAME));
        // 与正式后台装载相同：未登记根水只允许确切 NotFound，当坏文件存在时不能漏过引用。
        let required_paths = entry.water_path.iter().map(String::as_str)
            .chain(manifest.terrains.iter().flat_map(|objects| objects.iter())
                .filter_map(|object| object.water_path.as_deref()))
            .collect::<BTreeSet<_>>();
        let paths = required_paths.iter().copied()
            .chain(optional_legacy.as_deref())
            .collect::<BTreeSet<_>>();
        let mut referenced = false;
        for relative in paths {
            let path = safe_asset_path(root, relative)?;
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if optional_legacy.as_deref() == Some(relative)
                    && !required_paths.contains(relative)
                    && error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("读取场景{}水域文件{relative}失败：{error}", entry.id)),
            };
            let water: SceneWaterAsset = serde_json::from_slice(&bytes)
                .map_err(|error| format!("解析场景{}水域文件{relative}失败：{error}", entry.id))?;
            if water.schema_version != SCENE_SCHEMA_VERSION || water.scene_id != entry.id {
                return Err(format!("场景{}水域版本或身份不一致，不能安全删除预设", entry.id));
            }
            referenced |= water.layers.iter().any(|layer| names.contains(layer.preset_id.as_str()));
        }
        if referenced {
            references.push(format!("{}({})", entry.display_name, entry.id));
        }
    }
    Ok(references)
}

fn preset_reference_names(preset: &WaterPresetDescriptor) -> BTreeSet<&str> {
    std::iter::once(preset.id.as_str())
        .chain(preset.aliases.iter().map(String::as_str))
        .collect()
}

pub(super) fn poll_catalog_write(
    mut state: ResMut<WaterPresetEditorState>,
    registry: Option<Res<WaterPresetRegistry>>,
    asset_server: Res<AssetServer>,
    mut editor: ResMut<SceneEditorState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut session: Option<ResMut<EditorAuthoringSession>>,
) {
    if let Some(task) = state.write_task.as_mut()
        && let Some(result) = check_ready(task)
    {
        state.write_task = None;
        match result {
            Ok(result) => {
                asset_server.reload(WATER_PRESET_CATALOG_ASSET_PATH);
                state.status = "目录已发布，正在等待运行时原子采用并重建水面…".to_owned();
                state.adoption = Some(WaterCatalogAdoption {
                    baseline_revision: result.baseline_revision,
                    preset_id: result.preset_id,
                    deleted: result.deleted,
                    tab_id: result.tab_id,
                    message: result.message,
                });
            }
            Err(error) => {
                if let Some(session) = session.as_deref_mut() { session.water_preset_delete_pending = false; }
                editor.status = error.clone();
                state.status = error;
                if let Some(target) = state.save_feedback_target.take() {
                    editor.record_operation_result(
                        crate::types::EditorStatusOperation::WaterPresetSave,
                        &target,
                        crate::types::EditorStatusKind::Error,
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
        if adoption.deleted && let Some(error) = registry.last_error.as_deref() {
            // 写者已收口，但目录尚未采用：保留引用门和错误回执，不伪装成功。
            // 只取消门后尚未启动的意图；否则它们会令关闭快照永久 pending，连放弃退出也不可用。
            editor.save_requested = false;
            editor.pending_scene_transition = None;
            editor.scene_switch_confirmation = None;
            let message = format!("水体预设已写盘，但新目录采用失败：{error}；已取消尚未开始的保存和切换场景，可修复后等待重载，或关闭工程放弃现场");
            editor.status = message.clone();
            state.status = message;
            state.save_feedback_error = Some(error.into());
            return;
        }
        if registry_changed || state.save_feedback_error.is_none() {
            state.observe_save_adoption_error(&mut editor, registry.last_error.as_deref());
        }
        return;
    }
    let adopted = registry.resolve(&adoption.preset_id).is_some();
    if adopted == adoption.deleted {
        let message = registry.last_error.as_deref().map_or_else(
            || "目录修订已变化，但目标预设状态不匹配；可能又被外部修改".to_owned(),
            |error| format!("文件已发布，但新水体目录热采用失败：{error}"),
        );
        editor.status = message.clone();
        state.status = message;
        if let Some(target) = state.save_feedback_target.take() {
            editor.record_operation_result(
                crate::types::EditorStatusOperation::WaterPresetSave,
                &target,
                crate::types::EditorStatusKind::Error,
            );
        }
        state.adoption = None;
        if let Some(session) = session.as_deref_mut() { session.water_preset_delete_pending = false; }
        return;
    }
    let adoption = state.adoption.take().expect("上方已确认水体目录采用状态");
    if let Some(session) = session.as_deref_mut() { session.water_preset_delete_pending = false; }
    if adoption.deleted {
        if editor.water_selected_preset_id.as_deref() == Some(&adoption.preset_id) {
            editor.water_selected_preset_id = None;
        }
    } else {
        editor.water_selected_preset_id = Some(adoption.preset_id.clone());
    }
    if let Some(tab_id) = adoption.tab_id
        && let Some(saved) = registry.resolve(&adoption.preset_id).cloned()
        && let Some(draft) = state.draft_mut(tab_id)
    {
        draft.mode = WaterDraftMode::Edit {
            original_id: saved.id.clone(),
        };
        draft.descriptor = saved.clone();
        draft.baseline = saved;
    }
    state.active_field = None;
    state.input_buffer.clear();
    state.touch_ui();
    workspace.touch_ui();
    editor.status = adoption.message.clone();
    state.status = adoption.message;
    if let Some(target) = state.save_feedback_target.take() {
        editor.record_operation_result(
            crate::types::EditorStatusOperation::WaterPresetSave,
            &target,
            crate::types::EditorStatusKind::Success,
        );
    }
}

#[cfg(test)]
mod feedback_tests;

#[cfg(test)]
mod tests;
