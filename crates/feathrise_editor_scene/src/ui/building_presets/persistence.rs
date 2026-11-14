//! 全局建筑预设目录的乐观并发、引用保护、原子发布和热采用门。

use std::collections::BTreeSet; // 规范 ID 与历史别名共同构成引用集合。
use std::path::{Path, PathBuf}; // 后台任务只持有工作区资产根。

use bevy::prelude::*; // AssetServer、Res 与系统参数。
use bevy::tasks::IoTaskPool; // 文件读取、全图引用扫描和写入离开帧关键路径。
use bevy::tasks::futures::check_ready; // 主线程只非阻塞轮询单任务。
use feathrise_scene_content::{
    BUILDING_PRESET_CATALOG_ASSET_PATH, BUILDING_PRESET_SCHEMA_VERSION, BuildingPresetCatalogAsset,
    BuildingPresetDescriptor, BuildingPresetRegistry, SCENE_CATALOG_ASSET_PATH, SCENE_SCHEMA_VERSION,
    SceneCatalogAsset, ResolvedBuildingPresetCatalog,
}; // 目录与场景引用统一复用公共严格格式。

use super::types::{
    BuildingCatalogAdoption, BuildingCatalogWriteResult, BuildingDraftMode,
    BuildingPresetEditorState,
}; // 编辑会话只保存轻量任务和采用状态。
use crate::authoring::{
    EditorAuthoringSession, commit_owned_files, safe_asset_path, workspace_asset_root,
}; // 文件事务沿用场景编辑器经过验证的同卷原子替换。
use crate::resources::SceneEditorState; // 成功采用后规范化当前选择并回显状态。
use crate::ui::vegetation_preset_editor::VegetationPresetEditorState; // 中央共享标签修订。

/// 一次全局目录修改；后台重新读取磁盘基线后再应用。
#[derive(Clone, Debug)]
enum BuildingCatalogMutation {
    Upsert {
        original_id: Option<String>,          // 新建为 None，编辑冻结原规范 ID。
        descriptor: BuildingPresetDescriptor, // 完整强类型草稿。
    },
    Delete {
        id: String, // 只有无任何场景引用时才删除。
    },
}

/// 启动新建或编辑事务。
pub(super) fn start_upsert(
    state: &mut BuildingPresetEditorState, // 唯一写任务槽。
    registry: &BuildingPresetRegistry,     // 当前有效内存基线。
    mode: &BuildingDraftMode,              // 决定追加或替换。
    descriptor: BuildingPresetDescriptor,  // 已提交字段的完整草稿。
    tab_id: u64,                           // 成功后回写对应标签基线。
) -> Result<(), String> {
    ensure_writable(state, registry)?; // 坏目录或在途任务失败关闭。
    let mutation = BuildingCatalogMutation::Upsert {
        original_id: match mode {
            BuildingDraftMode::Create => None, // 新建追加到作者顺序末尾。
            BuildingDraftMode::Edit { original_id } => Some(original_id.clone()), // 编辑只替换原项。
        },
        descriptor, // 完整值移入后台任务。
    };
    let baseline = baseline_catalog(registry); // O(n) 复制只发生在显式保存点击。
    mutate_catalog(baseline.clone(), &mutation)?; // 主线程先纯校验，错误不启动 IO。
    let baseline_revision = registry.revision; // 热采用必须观察到更高修订。
    let root = workspace_asset_root(); // 后台任务不依赖当前进程目录变化。
    state.status = "正在后台校验并原子保存建筑预设目录…".to_owned(); // 明确任务阶段。
    state.write_task = Some(IoTaskPool::get().spawn(async move {
        write_catalog_transaction(root, baseline, mutation, baseline_revision, Some(tab_id))
    })); // single-flight 任务拥有完整输入快照。
    Ok(())
}

/// 启动引用保护删除事务。
pub(super) fn start_delete(
    state: &mut BuildingPresetEditorState, // 唯一任务槽。
    registry: &BuildingPresetRegistry,     // 当前有效目录。
    session: &EditorAuthoringSession,      // 当前未保存场景必须先检查。
    preset: &BuildingPresetDescriptor,     // 删除目标及全部历史别名。
) -> Result<(), String> {
    ensure_writable(state, registry)?; // 不覆盖坏目录或另一个事务。
    let names = preset_reference_names(preset); // 当前 ID 与别名同等视为引用。
    if session.draft.as_ref().is_some_and(|draft| {
        draft.placements.placements.iter().any(|record| {
            record
                .building_preset_id
                .as_deref()
                .is_some_and(|id| names.contains(id))
        })
    }) {
        return Err("当前未保存场景仍引用该建筑预设；请先替换或删除对应建筑".to_owned()); // 不让后台只看旧磁盘漏引用。
    }
    let baseline = baseline_catalog(registry); // 冻结当前完整目录供磁盘乐观比较。
    let mutation = BuildingCatalogMutation::Delete {
        id: preset.id.clone(), // 后台重新定位规范项。
    };
    mutate_catalog(baseline.clone(), &mutation)?; // 提前证明目录删除后仍合法。
    let baseline_revision = registry.revision; // 采用门拒绝旧事件。
    let root = workspace_asset_root(); // 全场景扫描在 IO 池执行。
    state.status = "正在后台扫描全部场景建筑引用；确认无引用后删除…".to_owned();
    state.write_task = Some(IoTaskPool::get().spawn(async move {
        write_catalog_transaction(root, baseline, mutation, baseline_revision, None)
    }));
    Ok(())
}

/// 拒绝在目录无有效基线、已报错或已有任务时写入。
fn ensure_writable(
    state: &BuildingPresetEditorState,
    registry: &BuildingPresetRegistry,
) -> Result<(), String> {
    if state.busy() {
        return Err("已有建筑目录保存或热采用任务正在进行".to_owned()); // 保持单在途。
    }
    if registry.revision == 0 {
        return Err("建筑预设目录尚未形成第一份有效快照".to_owned()); // 禁止用空猜测覆盖磁盘。
    }
    if let Some(error) = registry.last_error.as_deref() {
        return Err(format!("当前建筑目录加载失败，修复后才能写入：{error}")); // 旧快照可读但不可覆盖坏文件。
    }
    Ok(())
}

/// 从当前原子快照构造磁盘比较基线。
fn baseline_catalog(registry: &BuildingPresetRegistry) -> BuildingPresetCatalogAsset {
    BuildingPresetCatalogAsset {
        schema_version: BUILDING_PRESET_SCHEMA_VERSION, // 使用当前公共格式。
        presets: registry.presets().to_vec(),           // 显式点击允许一次完整有界复制。
    }
}

/// 对拥有型目录应用纯修改并执行整份公共校验。
fn mutate_catalog(
    mut catalog: BuildingPresetCatalogAsset,
    mutation: &BuildingCatalogMutation,
) -> Result<BuildingPresetCatalogAsset, String> {
    match mutation {
        BuildingCatalogMutation::Upsert {
            original_id: None,
            descriptor,
        } => catalog.presets.push(descriptor.clone()), // 新项保持创建顺序。
        BuildingCatalogMutation::Upsert {
            original_id: Some(original_id),
            descriptor,
        } => {
            let existing = catalog
                .presets
                .iter_mut()
                .find(|preset| preset.id == *original_id)
                .ok_or_else(|| format!("待编辑建筑预设{original_id}已不存在"))?;
            if descriptor.id != existing.id {
                return Err("编辑不能修改建筑稳定 ID；场景始终按该身份引用".to_owned()); // 重命名应通过 aliases 做显式迁移。
            }
            *existing = descriptor.clone(); // 通过规范身份原位替换且保留目录顺序。
        }
        BuildingCatalogMutation::Delete { id } => {
            let index = catalog
                .presets
                .iter()
                .position(|preset| preset.id == *id)
                .ok_or_else(|| format!("待删除建筑预设{id}已不存在"))?;
            catalog.presets.remove(index); // 引用扫描由后台磁盘阶段完成。
        }
    }
    ResolvedBuildingPresetCatalog::build(&catalog)?; // 任一字段错误拒绝整份新目录。
    Ok(catalog)
}

/// 后台重新读取、比较、引用扫描并原子发布目录。
fn write_catalog_transaction(
    root: PathBuf,
    baseline: BuildingPresetCatalogAsset,
    mutation: BuildingCatalogMutation,
    baseline_revision: u64,
    tab_id: Option<u64>,
) -> Result<BuildingCatalogWriteResult, String> {
    let catalog_path = safe_asset_path(&root, BUILDING_PRESET_CATALOG_ASSET_PATH)?; // 拒绝逃逸 assets 根。
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取建筑预设目录{}失败：{error}", catalog_path.display()))?;
    let disk: BuildingPresetCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析建筑预设目录{}失败：{error}", catalog_path.display()))?;
    ResolvedBuildingPresetCatalog::build(&disk)?; // 坏磁盘文件不能被旧内存快照覆盖。
    if disk != baseline {
        return Err("建筑预设目录已被外部修改；已拒绝覆盖，请等待热加载后重试".to_owned()); // 乐观并发失败。
    }
    if let BuildingCatalogMutation::Delete { id } = &mutation {
        let preset = disk
            .presets
            .iter()
            .find(|preset| preset.id == *id)
            .ok_or_else(|| format!("待删除建筑预设{id}已不存在"))?;
        let references = scan_scene_references(&root, preset)?; // 全图 IO 不进入主线程。
        if !references.is_empty() {
            return Err(format!(
                "不能删除建筑预设{}：仍被{}",
                preset.label,
                references.join("、"),
            )); // 明确列出引用场景。
        }
    }
    let updated = mutate_catalog(disk, &mutation)?; // 对刚读取的相同基线应用唯一修改。
    let mut catalog_bytes = serde_json::to_vec_pretty(&updated)
        .map_err(|error| format!("序列化建筑预设目录失败：{error}"))?;
    catalog_bytes.push(b'\n'); // 仓库 JSON 保持末尾换行。
    commit_owned_files(vec![(catalog_path, catalog_bytes)])?; // 公共事务执行同卷临时文件与原子替换。
    let (preset_id, deleted, message) = match mutation {
        BuildingCatalogMutation::Upsert {
            original_id: None,
            descriptor,
        } => (
            descriptor.id, // 采用后应存在该规范项。
            false,         // 新建不是删除。
            format!("已新建建筑预设“{}”", descriptor.label),
        ),
        BuildingCatalogMutation::Upsert {
            original_id: Some(_),
            descriptor,
        } => (
            descriptor.id, // 编辑保持稳定身份。
            false,         // 编辑后仍应存在。
            format!("已更新建筑预设“{}”", descriptor.label),
        ),
        BuildingCatalogMutation::Delete { id } => (
            id,   // 采用后应不再解析。
            true, // 删除存在条件相反。
            "已安全删除无场景引用的建筑预设".to_owned(),
        ),
    };
    Ok(BuildingCatalogWriteResult {
        baseline_revision, // 主线程等待更高修订。
        preset_id,         // 核对最终存在性。
        deleted,           // 决定选择清理。
        tab_id,            // 保存草稿回写目标。
        message,           // 热采用后回显。
    })
}

/// 后台扫描所有场景 placements 文件中的规范 ID 或历史别名引用。
fn scan_scene_references(
    root: &Path,
    preset: &BuildingPresetDescriptor,
) -> Result<Vec<String>, String> {
    let catalog_path = safe_asset_path(root, SCENE_CATALOG_ASSET_PATH)?; // 全局场景目录是唯一枚举来源。
    let bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取场景目录{}失败：{error}", catalog_path.display()))?;
    let catalog: SceneCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析场景目录{}失败：{error}", catalog_path.display()))?;
    if catalog.schema_version != SCENE_SCHEMA_VERSION {
        return Err("场景目录版本不一致，不能安全删除建筑预设".to_owned()); // 不猜测未来路径协议。
    }
    let names = preset_reference_names(preset); // 旧场景别名仍阻止删除。
    let mut references = Vec::new(); // 只收集有引用场景的短摘要。
    for entry in catalog.scenes {
        let relative = entry.placements_path(); // placements 路径由公共目录规则派生。
        let path = safe_asset_path(root, &relative)?; // 每条仍拒绝目录逃逸。
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("读取场景{}摆放文件失败：{error}", entry.id))?;
        let placements = feathrise_scene_content::read_scene_placements(&bytes, &relative)
            .map_err(|error| format!("解析场景{}摆放文件失败：{error}", entry.id))?;
        if placements.schema_version != SCENE_SCHEMA_VERSION || placements.scene_id != entry.id {
            return Err(format!(
                "场景{}摆放版本或身份不一致，不能安全删除预设",
                entry.id
            )); // 文件串图失败关闭。
        }
        let referenced = placements.placements.iter().any(|record| {
            record
                .building_preset_id
                .as_deref()
                .is_some_and(|id| names.contains(id))
        }); // 普通资产 None 不参与。
        if referenced {
            references.push(format!("{}({})", entry.display_name, entry.id)); // 作者可直接定位场景。
        }
    }
    Ok(references)
}

/// 返回当前规范 ID 与全部历史别名的借用集合。
fn preset_reference_names(preset: &BuildingPresetDescriptor) -> BTreeSet<&str> {
    std::iter::once(preset.id.as_str())
        .chain(preset.aliases.iter().map(String::as_str))
        .collect() // 删除保护不遗漏重命名前场景。
}

/// 非阻塞轮询文件任务，并等待 AssetServer 发布完整有效新快照。
pub(super) fn poll_catalog_write(
    mut state: ResMut<BuildingPresetEditorState>, // 任务和采用状态。
    registry: Option<Res<BuildingPresetRegistry>>, // 新修订来源。
    asset_server: Res<AssetServer>,               // 文件发布后显式请求目录重载。
    mut editor: ResMut<SceneEditorState>,           // 当前选择与底栏状态。
    mut workspace: ResMut<VegetationPresetEditorState>, // 中央标签重建修订。
) {
    if let Some(task) = state.write_task.as_mut()
        && let Some(result) = check_ready(task)
    {
        state.write_task = None; // 完成值只消费一次。
        match result {
            Ok(result) => {
                asset_server.reload(BUILDING_PRESET_CATALOG_ASSET_PATH); // 进入公共加载和严格校验链。
                state.status = "文件已原子发布，正在等待运行时采用新建筑目录…".to_owned();
                state.adoption = Some(BuildingCatalogAdoption {
                    baseline_revision: result.baseline_revision, // 旧事件不算成功。
                    preset_id: result.preset_id,                 // 核对目标存在性。
                    deleted: result.deleted,                     // 删除应消失。
                    tab_id: result.tab_id,                       // 保存应回写草稿。
                    message: result.message,                     // 最终成功文案。
                });
            }
            Err(error) => {
                editor.status = error.clone(); // 底栏显示具体 IO 或引用错误。
                state.status = error; // 左栏同步显示。
            }
        }
    }
    let Some(adoption) = state.adoption.as_ref() else {
        return; // 没有待采用结果保持 O(1)。
    };
    let Some(registry) = registry.as_deref() else {
        return; // SceneContent 暂未提供注册表继续等待。
    };
    if registry.revision <= adoption.baseline_revision {
        return; // 旧有效快照不能证明本次文件已采用。
    }
    let adopted = registry.resolve(&adoption.preset_id).is_some(); // 别名也会解析，但目标写的是规范 ID。
    if adopted == adoption.deleted {
        let message = registry.last_error.as_deref().map_or_else(
            || "目录修订已变化，但目标建筑状态不匹配；可能又被外部修改".to_owned(),
            |error| format!("文件已发布，但新建筑目录热采用失败：{error}"),
        );
        editor.status = message.clone(); // 明确失败而不伪造保存成功。
        state.status = message;
        state.adoption = None; // 等待作者修复后重新保存。
        return;
    }
    let adoption = state.adoption.take().expect("上方已经确认存在待采用状态");
    if adoption.deleted {
        if editor.building_selected_preset_id.as_deref() == Some(&adoption.preset_id) {
            editor.building_selected_preset_id = None; // 删除后清理悬空选择。
        }
    } else {
        editor.building_selected_preset_id = Some(adoption.preset_id.clone()); // 新建和编辑规范化选择。
    }
    if let Some(tab_id) = adoption.tab_id
        && let Some(saved) = registry.resolve(&adoption.preset_id).cloned()
        && let Some(draft) = state.draft_mut(tab_id)
    {
        draft.mode = BuildingDraftMode::Edit {
            original_id: saved.id.clone(), // 后续保存执行原位替换。
        };
        draft.descriptor = saved.clone(); // 采用公共规范化结果。
        draft.baseline = saved; // 清除脏星号。
    }
    state.active_field = None; // 旧输入身份不能跨目录修订。
    state.input_buffer.clear(); // 清除已提交缓冲。
    state.touch_ui(); // 建筑标签标题和字段采用新值。
    workspace.touch_ui(); // 共享工作区实际重建。
    editor.status = adoption.message.clone(); // 顶部作者反馈。
    state.status = adoption.message; // 左栏反馈。
}

#[cfg(test)]
mod tests {
    use super::*; // 纯目录修改回归不触碰真实磁盘。
    use feathrise_scene_content::ScenePlacementAssetTarget; // 合法测试模型目标。

    /// 构造一个完整合法建筑预设。
    fn preset(id: &str) -> BuildingPresetDescriptor {
        BuildingPresetDescriptor::from_asset(
            id.to_owned(),                        // 规范身份由测试控制。
            format!("建筑{id}"),                  // 名称保持非空。
            "gltf/buildings/test.glb".to_owned(), // 公共校验接受的相对 GLB。
            ScenePlacementAssetTarget::Scene0,      // 基线使用完整场景。
        )
    }

    #[test]
    fn editing_preserves_identity_and_delete_removes_only_target() {
        let baseline = BuildingPresetCatalogAsset {
            schema_version: BUILDING_PRESET_SCHEMA_VERSION, // 当前首版协议。
            presets: vec![preset("house-a"), preset("house-b")], // 两项验证原位语义。
        };
        let mut renamed = preset("other"); // 制造非法稳定 ID 变化。
        renamed.label = "新名称".to_owned(); // 显示名本身合法。
        assert!(
            mutate_catalog(
                baseline.clone(),
                &BuildingCatalogMutation::Upsert {
                    original_id: Some("house-a".to_owned()),
                    descriptor: renamed,
                },
            )
            .is_err()
        ); // 编辑不得偷换场景引用身份。
        let deleted = mutate_catalog(
            baseline,
            &BuildingCatalogMutation::Delete {
                id: "house-a".to_owned(),
            },
        )
        .expect("删除一个无引用项后目录仍应合法");
        assert_eq!(deleted.presets.len(), 1); // 只保留另一项。
        assert_eq!(deleted.presets[0].id, "house-b"); // 作者顺序保持稳定。
    }
}
