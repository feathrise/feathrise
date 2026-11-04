//! 全局植被预设目录的乐观并发、引用保护与原子写入事务。

use std::collections::BTreeSet; // 删除时规范ID与历史别名形成确定性引用集合。

use bevy::prelude::*; // AssetServer、Res与ECS资源。
use bevy::tasks::IoTaskPool; // 跨地图扫描和文件写入绝不占用帧主线程。
use bevy::tasks::futures::check_ready; // Update只做非阻塞任务探测。
use feathrise_map_content::{
    MAP_CATALOG_ASSET_PATH, MAP_SCHEMA_VERSION, MapCatalogAsset, MapVegetationAsset,
    ResolvedVegetationPresetCatalog, VEGETATION_PRESET_CATALOG_ASSET_PATH,
    VEGETATION_PRESET_SCHEMA_VERSION, VegetationPresetCatalogAsset, VegetationPresetDescriptor,
    VegetationPresetRegistry,
}; // 写入前后复用运行时同一目录校验与地图正式格式。

use super::types::{
    CatalogAdoption, CatalogWriteResult, PresetDraftMode, VegetationPresetEditorState,
}; // UI草稿身份和唯一任务槽。
use crate::authoring::{
    EditorAuthoringSession, atomic_write, safe_asset_path, workspace_asset_root,
}; // 当前未保存地图引用与共享安全文件边界。
use crate::resources::MapEditorState; // 热采用完成后更新目录选择和作者状态栏。

/// 一次目录原子修改的纯拥有快照。
#[derive(Clone, Debug)]
enum CatalogMutation {
    Upsert {
        original_id: Option<String>, // None表示新建，Some表示稳定ID原位编辑。
        descriptor: VegetationPresetDescriptor, // 完整强类型新值。
    },
    Delete {
        id: String, // 删除目标在任务启动时冻结。
    },
}

/// 从已加载注册表启动新建或编辑事务。
pub(super) fn start_upsert(
    state: &mut VegetationPresetEditorState,
    registry: &VegetationPresetRegistry,
    mode: &PresetDraftMode,
    mut descriptor: VegetationPresetDescriptor, // 显式保存时才固定未设置备注的来源初值。
    tab_id: u64,
) -> Result<(), String> {
    ensure_writable(state, registry)?;
    initialize_notes(&mut descriptor); // 旧目录只在第一次明确保存时把来源固化为可编辑备注。
    let original_id = match mode {
        PresetDraftMode::Create => None,
        PresetDraftMode::Edit { original_id } => Some(original_id.clone()),
    };
    let mutation = CatalogMutation::Upsert {
        original_id,
        descriptor: descriptor.clone(),
    };
    let baseline = baseline_catalog(registry);
    let preview = mutate_catalog(baseline.clone(), &mutation)?;
    if preview.presets == baseline.presets {
        return Err("预设内容没有变化，无需写入".to_owned());
    }
    let baseline_revision = registry.revision;
    let root = workspace_asset_root();
    state.status = "正在后台校验并原子保存全局植被预设…".to_owned();
    state.save_feedback_target = Some(crate::types::EditorStatusTarget::new(
        &descriptor.id,
        &descriptor.label,
    ));
    state.save_feedback_error = None;
    state.write_task = Some(IoTaskPool::get().spawn(async move {
        write_catalog_transaction(root, baseline, mutation, baseline_revision, Some(tab_id))
    }));
    Ok(())
}

/// 旧目录缺少备注时借用作者来源一次；显式空串及已有说明始终保持原样。
fn initialize_notes(descriptor: &mut VegetationPresetDescriptor) {
    // 单独抽出纯函数锁定迁移边界。
    if descriptor.notes.is_none() {
        // 只有真正缺省才属于旧数据。
        descriptor.notes = Some(descriptor.author_asset_path.clone()); // 保存后备注与内部兼容来源彼此独立。
    } // Some("")不能被误判为空缺并重新复活来源。
} // 不执行目录校验、文件IO或运行时修改。

/// 从已加载注册表启动安全删除；当前未保存地图也必须先通过引用门。
pub(super) fn start_delete(
    state: &mut VegetationPresetEditorState,
    registry: &VegetationPresetRegistry,
    session: &EditorAuthoringSession,
    preset: &VegetationPresetDescriptor,
) -> Result<(), String> {
    ensure_writable(state, registry)?;
    let names = preset_reference_names(preset);
    if let Some(draft) = session.draft.as_ref()
        && let Some(vegetation) = draft.vegetation.as_ref()
        && vegetation.layers.iter().any(|layer| {
            layer
                .runtime_profile
                .as_deref()
                .is_some_and(|profile| names.contains(profile))
        })
    {
        return Err("当前未保存地图仍引用该预设；请先删除对应植被层并保存地图".to_owned());
    }
    let baseline = baseline_catalog(registry);
    let mutation = CatalogMutation::Delete {
        id: preset.id.clone(),
    };
    mutate_catalog(baseline.clone(), &mutation)?; // 启动IO前先确认目标确实存在且目录删除后仍合法。
    let baseline_revision = registry.revision;
    let root = workspace_asset_root();
    state.status = "正在后台检查全部地图引用；确认无引用后才会删除…".to_owned();
    state.save_feedback_target = None;
    state.write_task = Some(IoTaskPool::get().spawn(async move {
        write_catalog_transaction(root, baseline, mutation, baseline_revision, None)
    }));
    Ok(())
}

/// 注册表失败快照、加载中或已有任务时不允许覆盖全局目录。
fn ensure_writable(
    state: &VegetationPresetEditorState,
    registry: &VegetationPresetRegistry,
) -> Result<(), String> {
    if state.busy() {
        return Err("已有预设资产检查、保存或热采用任务正在进行".to_owned());
    }
    if let Some(error) = registry.last_error.as_deref() {
        return Err(format!("当前预设目录加载失败，修复后才能写入：{error}"));
    }
    Ok(())
}

/// 把当前已校验注册表冻结为文件乐观并发基线。
fn baseline_catalog(registry: &VegetationPresetRegistry) -> VegetationPresetCatalogAsset {
    VegetationPresetCatalogAsset {
        schema_version: VEGETATION_PRESET_SCHEMA_VERSION,
        presets: registry.presets().to_vec(),
    }
}

/// 在纯内存目录上应用修改并执行完整公共校验。
fn mutate_catalog(
    mut catalog: VegetationPresetCatalogAsset,
    mutation: &CatalogMutation,
) -> Result<VegetationPresetCatalogAsset, String> {
    match mutation {
        CatalogMutation::Upsert {
            original_id: None,
            descriptor,
        } => catalog.presets.push(descriptor.clone()),
        CatalogMutation::Upsert {
            original_id: Some(original_id),
            descriptor,
        } => {
            let existing = catalog
                .presets
                .iter_mut()
                .find(|preset| preset.id == *original_id)
                .ok_or_else(|| format!("待编辑预设{original_id}已不存在"))?;
            if descriptor.id != existing.id
                || descriptor.output_id != existing.output_id
                || descriptor.runtime_layer_id != existing.runtime_layer_id
                || descriptor.prototype_id != existing.prototype_id
                || std::mem::discriminant(&descriptor.runtime)
                    != std::mem::discriminant(&existing.runtime)
            {
                return Err(
                    "编辑只能修改内容参数；稳定ID、输出ID、运行时身份和后端类型已锁定".to_owned(),
                );
            }
            *existing = descriptor.clone();
        }
        CatalogMutation::Delete { id } => {
            let index = catalog
                .presets
                .iter()
                .position(|preset| preset.id == *id)
                .ok_or_else(|| format!("待删除预设{id}已不存在"))?;
            catalog.presets.remove(index);
        }
    }
    ResolvedVegetationPresetCatalog::build(&catalog)?; // 数量不限，但全部结构、路径和身份必须原子有效。
    Ok(catalog)
}

/// 在IO线程核对磁盘基线、扫描引用并原子替换完整目录。
fn write_catalog_transaction(
    root: std::path::PathBuf,
    baseline: VegetationPresetCatalogAsset,
    mutation: CatalogMutation,
    baseline_revision: u64,
    tab_id: Option<u64>,
) -> Result<CatalogWriteResult, String> {
    let catalog_path = safe_asset_path(&root, VEGETATION_PRESET_CATALOG_ASSET_PATH)?;
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取植被预设目录{}失败：{error}", catalog_path.display()))?;
    let disk: VegetationPresetCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析植被预设目录{}失败：{error}", catalog_path.display()))?;
    ResolvedVegetationPresetCatalog::build(&disk)?;
    if disk.schema_version != baseline.schema_version || disk.presets != baseline.presets {
        return Err("植被预设目录已被外部修改；已拒绝覆盖，请等待热加载后重试".to_owned());
    }
    if let CatalogMutation::Delete { id } = &mutation {
        let preset = disk
            .presets
            .iter()
            .find(|preset| preset.id == *id)
            .ok_or_else(|| format!("待删除预设{id}已不存在"))?;
        let references = scan_map_references(&root, preset)?;
        if !references.is_empty() {
            return Err(format!(
                "不能删除预设{}：仍被{}引用",
                preset.label,
                references.join("、")
            ));
        }
    }
    let updated = mutate_catalog(disk, &mutation)?;
    let mut output = serde_json::to_vec_pretty(&updated)
        .map_err(|error| format!("序列化植被预设目录失败：{error}"))?;
    output.push(b'\n');
    atomic_write(&catalog_path, &output)?;
    let (preset_id, deleted, message) = match mutation {
        CatalogMutation::Upsert {
            original_id: None,
            descriptor,
        } => (
            descriptor.id,
            false,
            format!("已新建全局植被预设“{}”", descriptor.label),
        ),
        CatalogMutation::Upsert {
            original_id: Some(_),
            descriptor,
        } => (
            descriptor.id,
            false,
            format!("已更新全局植被预设“{}”", descriptor.label),
        ),
        CatalogMutation::Delete { id } => (id, true, "已安全删除全局植被预设".to_owned()),
    };
    Ok(CatalogWriteResult {
        baseline_revision,
        preset_id,
        deleted,
        tab_id,
        message,
    })
}

/// 扫描全部目录地图的正式植被层；缺失或损坏文件宁可阻止删除。
fn scan_map_references(
    root: &std::path::Path,
    preset: &VegetationPresetDescriptor,
) -> Result<Vec<String>, String> {
    let catalog_path = safe_asset_path(root, MAP_CATALOG_ASSET_PATH)?;
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取地图目录{}失败：{error}", catalog_path.display()))?;
    let catalog: MapCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析地图目录{}失败：{error}", catalog_path.display()))?;
    if catalog.schema_version != MAP_SCHEMA_VERSION {
        return Err(format!(
            "地图目录版本{}与编辑器版本{}不一致，不能安全删除预设",
            catalog.schema_version, MAP_SCHEMA_VERSION
        ));
    }
    let names = preset_reference_names(preset);
    let mut references = Vec::new();
    for entry in catalog.maps {
        let Some(relative) = entry.vegetation_path.as_deref() else {
            continue;
        };
        let path = safe_asset_path(root, relative)?;
        let bytes = std::fs::read(&path).map_err(|error| {
            format!(
                "读取地图{}植被文件{}失败：{error}",
                entry.id,
                path.display()
            )
        })?;
        let vegetation: MapVegetationAsset = serde_json::from_slice(&bytes)
            .map_err(|error| format!("解析地图{}植被文件失败：{error}", entry.id))?;
        if vegetation.schema_version != MAP_SCHEMA_VERSION || vegetation.map_id != entry.id {
            return Err(format!(
                "地图{}植被文件版本或地图身份不一致，不能安全删除预设",
                entry.id
            ));
        }
        if vegetation.layers.iter().any(|layer| {
            layer
                .runtime_profile
                .as_deref()
                .is_some_and(|profile| names.contains(profile))
        }) {
            references.push(format!("{}({})", entry.display_name, entry.id));
        }
    }
    Ok(references)
}

/// 当前规范ID和全部历史别名都属于同一个不可误删引用集合。
fn preset_reference_names(preset: &VegetationPresetDescriptor) -> BTreeSet<&str> {
    std::iter::once(preset.id.as_str())
        .chain(preset.aliases.iter().map(String::as_str))
        .collect()
}

/// 非阻塞采用写入结果，并等待AssetServer注册表推进后才开放下一次写入。
pub(super) fn poll_catalog_write(
    mut state: ResMut<VegetationPresetEditorState>,
    registry: Option<Res<VegetationPresetRegistry>>,
    asset_server: Res<AssetServer>,
    mut editor: ResMut<MapEditorState>,
) {
    if let Some(task) = state.write_task.as_mut()
        && let Some(result) = check_ready(task)
    {
        state.write_task = None;
        match result {
            Ok(result) => {
                asset_server.reload(VEGETATION_PRESET_CATALOG_ASSET_PATH);
                state.status = "文件已原子保存，正在等待运行时热采用…".to_owned();
                state.adoption = Some(CatalogAdoption {
                    baseline_revision: result.baseline_revision,
                    preset_id: result.preset_id,
                    deleted: result.deleted,
                    tab_id: result.tab_id,
                    message: result.message,
                });
            }
            Err(error) => {
                editor.status = error.clone();
                state.status = error;
                if let Some(target) = state.save_feedback_target.take() {
                    editor.record_operation_result(
                        crate::types::EditorStatusOperation::VegetationPresetSave,
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
        if registry_changed || state.save_feedback_error.is_none() {
            state.observe_save_adoption_error(&mut editor, registry.last_error.as_deref());
        }
        return;
    }
    let adopted = registry.resolve(&adoption.preset_id).is_some();
    if adopted == adoption.deleted {
        let message = if let Some(error) = registry.last_error.as_deref() {
            format!("文件已保存，但新目录热采用失败：{error}")
        } else {
            "目录修订已变化，但目标预设状态不匹配；可能又被外部修改".to_owned()
        };
        editor.status = message.clone();
        state.status = message;
        if let Some(target) = state.save_feedback_target.take() {
            editor.record_operation_result(
                crate::types::EditorStatusOperation::VegetationPresetSave,
                &target,
                crate::types::EditorStatusKind::Error,
            );
        }
        state.adoption = None;
        return;
    }
    let adoption = state.adoption.take().expect("已在上方确认采用状态存在");
    if adoption.deleted {
        if editor.vegetation_selected_preset_id.as_deref() == Some(adoption.preset_id.as_str()) {
            editor.vegetation_selected_preset_id = None;
        }
    } else {
        editor.vegetation_selected_preset_id = Some(adoption.preset_id.clone());
    }
    if let Some(tab_id) = adoption.tab_id
        && let Some(saved) = registry.resolve(&adoption.preset_id).cloned()
        && let Some(draft) = state.draft_mut(tab_id)
    {
        draft.mode = PresetDraftMode::Edit {
            original_id: saved.id.clone(),
        };
        draft.descriptor = saved.clone();
        draft.baseline = saved; // 热采用后的正式值成为新的脏状态基线。
    }
    state.active_field = None;
    state.input_buffer.clear();
    state.touch_ui();
    editor.status = adoption.message.clone();
    state.status = adoption.message;
    if let Some(target) = state.save_feedback_target.take() {
        editor.record_operation_result(
            crate::types::EditorStatusOperation::VegetationPresetSave,
            &target,
            crate::types::EditorStatusKind::Success,
        );
    }
}

#[cfg(test)] // 备注持久化回归使用隔离临时目录，不改作者真实配置。
mod notes_tests; // 覆盖来源初值、自由中文、清空和完整事务往返。

#[cfg(test)]
mod feedback_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// 从随包树木复制合法完整数据Mesh，目录事务测试不再依赖已删除的注册后端。
    fn preset(id: &str) -> VegetationPresetDescriptor {
        let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vegetation_presets.vegetation_presets.json"
        )))
        .expect("随包数据预设目录必须可解析");
        let mut preset = catalog
            .presets
            .into_iter()
            .find(|preset| preset.id == "hn01_tree")
            .expect("随包树木数据预设必须存在");
        preset.id = id.to_owned(); // 测试调用方控制规范身份。
        preset.aliases.clear(); // 基线不携带树木历史别名。
        preset.label = id.to_owned(); // 显示名与测试身份保持直观一致。
        preset.output_id = id.to_owned(); // 目录事务据此验证稳定输出身份。
        preset.runtime_layer_id = None; // 使用测试ID派生新的层身份。
        preset.prototype_id = None; // 使用测试ID派生新的Mesh原型身份。
        preset
    }

    #[test]
    fn edit_cannot_change_stable_identity_or_backend() {
        let baseline = VegetationPresetCatalogAsset {
            schema_version: VEGETATION_PRESET_SCHEMA_VERSION,
            presets: vec![preset("tree")],
        };
        let mut changed_id = preset("tree");
        changed_id.output_id = "other".to_owned();
        assert!(
            mutate_catalog(
                baseline.clone(),
                &CatalogMutation::Upsert {
                    original_id: Some("tree".to_owned()),
                    descriptor: changed_id,
                },
            )
            .is_err()
        );
        let deleted = mutate_catalog(
            baseline,
            &CatalogMutation::Delete {
                id: "tree".to_owned(),
            },
        )
        .expect("无引用纯目录删除应合法");
        assert!(deleted.presets.is_empty());
    }
}
