//! 选中摆放保存为全局院落模板，并在热采用后建立原位置第一份副本。

use std::collections::BTreeSet; // 选择 ID 必须唯一，不能让一个成员重复进入模板。
use std::time::{SystemTime, UNIX_EPOCH}; // 只在作者显式保存边沿生成跨会话稳定 ID。

use bevy::prelude::*; // 消息、资源和资产热加载。
use bevy::tasks::IoTaskPool; // 磁盘读写和完整目录 JSON 校验都离开主线程。
use bevy::tasks::futures::check_ready; // 每帧非阻塞轮询一次目录事务。
use feathrise_map_content::{
    BUILDING_PRESET_CATALOG_ASSET_PATH, BuildingPresetRegistry,
    COURTYARD_PRESET_CATALOG_ASSET_PATH, CourtyardPresetCatalogAsset, CourtyardPresetDescriptor,
    CourtyardPresetRegistry, MAX_COURTYARD_MEMBERS_PER_PRESET, MapPlacementCourtyardMembership,
}; // 公共目录格式和地图成员身份保持一致。

use super::geometry::preset_from_placements; // 当前所见成员生成局部模板。
use super::state::{CatalogAdoption, CatalogTask, CourtyardAuthoringState}; // 写入与热采用单一状态机。
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, commit_owned_files, safe_asset_path, workspace_asset_root,
}; // 复用全局原子文件事务和地图单步撤销。
use crate::resources::MapEditorState; // 状态栏与保存脏位共用既有协议。
use crate::ui::courtyard::{CourtyardUiRequest, CourtyardUiState}; // UI 只发送稳定 ID 和名称。

/// UI 明确保存时冻结小型选集和作者身份，文件写入在 IO 池进行。
pub(super) fn request_save_selection(
    mut requests: MessageReader<CourtyardUiRequest>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    registry: Option<Res<CourtyardPresetRegistry>>,
    building_registry: Option<Res<BuildingPresetRegistry>>,
    mut ui: ResMut<CourtyardUiState>,
    mut state: ResMut<CourtyardAuthoringState>,
) {
    for request in requests.read() {
        let CourtyardUiRequest::SaveSelection {
            name,
            placement_ids,
        } = request
        else {
            continue;
        }; // 旋转由整组几何域处理。
        let result = (|| -> Result<(), String> {
            if !editor.open || session.saving {
                return Err("地图正在保存或编辑器未打开".to_owned()); // 与地图保存快照串行。
            }
            if state.catalog_task.is_some()
                || state.operation_pending()
                || state.visual_sync_pending()
            {
                return Err("已有院落保存或摆放任务正在完成".to_owned()); // 一个会话内不并发改写相同选集。
            }
            if !state.index.ready_for(&session) {
                return Err("院落成员索引正在分帧准备，请稍后重试".to_owned());
            }
            let registry = registry
                .as_deref()
                .ok_or_else(|| "院落目录尚未加载".to_owned())?;
            if registry.revision == 0 || registry.last_error.is_some() {
                return Err("院落目录尚未形成可写的有效快照".to_owned()); // 坏目录不能由旧快照覆盖。
            }
            let draft = session
                .draft
                .as_ref()
                .ok_or_else(|| "当前地图草稿未就绪".to_owned())?;
            if editor.selected_map_id.as_ref() != Some(&draft.map_id) {
                return Err("正在切换地图，请等待当前作者草稿就绪".to_owned());
            }
            if placement_ids.is_empty() || placement_ids.len() > MAX_COURTYARD_MEMBERS_PER_PRESET {
                return Err(format!(
                    "请选择 1 到 {MAX_COURTYARD_MEMBERS_PER_PRESET} 个摆放成员"
                ));
            }
            let mut seen = BTreeSet::new(); // 单次按钮边沿至多 256 条，无帧循环扫描。
            let mut selected = Vec::with_capacity(placement_ids.len());
            for id in placement_ids {
                if !seen.insert(id.as_str()) {
                    return Err(format!("选集中有重复摆放 ID：{id}"));
                }
                let index = *state
                    .index
                    .by_placement_id
                    .get(id)
                    .ok_or_else(|| format!("选中资产{id}已不存在"))?;
                let record = draft
                    .placements
                    .placements
                    .get(index)
                    .filter(|record| &record.id == id)
                    .ok_or_else(|| format!("选中资产{id}的结构身份已失效"))?;
                if record.courtyard.is_some() {
                    return Err(format!("资产{id}已属于院落；请先选择未编组的房屋与装饰")); // 不暗中拆散旧院落。
                }
                if !record.water_surfaces.is_empty() {
                    return Err(format!("资产{id}带有整网格水语义，暂不能作为院落成员")); // 克隆前拒绝大水体节点列表。
                }
                if let Some(building_id) = record.building_preset_id.as_deref() {
                    let building_registry = building_registry.as_deref().ok_or_else(|| {
                        format!("保存建筑成员需先加载{BUILDING_PRESET_CATALOG_ASSET_PATH}")
                    })?;
                    let preset = building_registry
                        .resolve(building_id)
                        .ok_or_else(|| format!("建筑成员{id}引用的预设{building_id}不存在"))?;
                    if record.asset_path != preset.asset_path
                        || record.asset_target != preset.asset_target
                    {
                        return Err(format!("建筑成员{id}的模型来源与预设不一致")); // 坏来源不能被保存为重复印章。
                    }
                }
                selected.push((index, record.clone()));
            }
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err("请填写院落名称".to_owned());
            }
            state.ticket = state.ticket.wrapping_add(1).max(1);
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| format!("生成院落稳定 ID 失败：{error}"))?
                .as_millis();
            let id = format!("courtyard-{millis}-{}", state.ticket); // 不依赖同名显示标签或数组下标。
            let records = selected
                .iter()
                .map(|(_, record)| record.clone())
                .collect::<Vec<_>>();
            let (descriptor, origin, yaw_degrees) =
                preset_from_placements(id.clone(), trimmed.to_owned(), &records)?;
            let trial = CourtyardPresetCatalogAsset {
                schema_version: feathrise_map_content::COURTYARD_PRESET_SCHEMA_VERSION,
                presets: vec![descriptor.clone()],
            };
            trial.validate()?; // 入口先核对成员路径、变换和 ID，无效目录不排队 IO。
            let adoption = CatalogAdoption {
                baseline_revision: registry.revision,
                preset_id: id,
                map_id: draft.map_id.clone(),
                author_revision: session.revision,
                structure_revision: session.placement_structure_revision,
                selected,
                origin,
                yaw_degrees,
            };
            let root = workspace_asset_root(); // 任务拥有绝对 assets 根，避免进程目录变化。
            state.catalog_task = Some(CatalogTask::Writing {
                task: IoTaskPool::get().spawn(async move { write_catalog(root, descriptor) }),
                adoption,
            });
            Ok(())
        })();
        let message = match result {
            Ok(()) => "正在后台保存院落模板并等待资产热采用…".to_owned(),
            Err(error) => format!("保存院落失败：{error}"),
        };
        ui.status = message.clone();
        editor.status = message;
    }
}

/// 后台重新读取真实目录并追加，不以可能过期的内存快照覆盖外部修改。
fn write_catalog(
    root: std::path::PathBuf,
    descriptor: CourtyardPresetDescriptor,
) -> Result<(), String> {
    let path = safe_asset_path(&root, COURTYARD_PRESET_CATALOG_ASSET_PATH)?;
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("读取院落目录{}失败：{error}", path.display()))?;
    let mut catalog: CourtyardPresetCatalogAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析院落目录{}失败：{error}", path.display()))?;
    catalog.validate()?; // 坏磁盘文件不能被旧内存目录悄悄覆盖。
    catalog.presets.push(descriptor); // 从最新磁盘版本追加，保留外部新增项。
    catalog.validate()?; // 同名 ID 或容量冲突整份拒绝。
    let mut updated = serde_json::to_vec_pretty(&catalog)
        .map_err(|error| format!("序列化院落目录失败：{error}"))?;
    updated.push(b'\n');
    commit_owned_files(vec![(path, updated)])?; // 与地图保存相同的原子发布链。
    Ok(())
}

/// 文件写成功后等待公共目录采用，随后一次建立原选中成员的院落归属。
#[allow(clippy::too_many_arguments)]
pub(super) fn poll_catalog_save(
    asset_server: Res<AssetServer>,
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    registry: Option<Res<CourtyardPresetRegistry>>,
    mut ui: ResMut<CourtyardUiState>,
    mut state: ResMut<CourtyardAuthoringState>,
) {
    let Some(task) = state.catalog_task.take() else {
        return;
    }; // 稳定帧无任何工作。
    let adoption = match task {
        CatalogTask::Writing { mut task, adoption } => {
            let Some(result) = check_ready(&mut task) else {
                state.catalog_task = Some(CatalogTask::Writing { task, adoption });
                return; // 绝不阻塞主线程等待磁盘。
            };
            if let Err(error) = result {
                ui.status = format!("保存院落目录失败：{error}");
                editor.status = ui.status.clone();
                return;
            }
            asset_server.reload(COURTYARD_PRESET_CATALOG_ASSET_PATH); // 文件写完才请求公共严格热采用。
            state.catalog_task = Some(CatalogTask::AwaitAdoption(adoption));
            return;
        }
        CatalogTask::AwaitAdoption(adoption) => adoption,
    };
    let Some(registry) = registry.as_deref() else {
        ui.status = "院落模板已写入，但公共目录注册表不可用".to_owned();
        editor.status = ui.status.clone();
        return;
    };
    if registry.revision <= adoption.baseline_revision {
        if let Some(error) = registry.last_error.as_deref() {
            ui.status = format!("院落模板文件已写入，但热采用失败：{error}");
            editor.status = ui.status.clone();
            return; // 坏目录不会无限占住唯一保存槽，作者修复后可重新操作。
        }
        state.catalog_task = Some(CatalogTask::AwaitAdoption(adoption)); // 热加载仍在途。
        return;
    }
    let Some(preset) = registry.resolve(&adoption.preset_id) else {
        ui.status = "院落模板文件已写入，但热采用未找到新预设，请检查目录诊断".to_owned();
        editor.status = ui.status.clone();
        return;
    };
    let id = preset.id.clone();
    let result = attach_source_members(&adoption, &id, &mut session, &mut editor);
    let message = match result {
        Ok(()) => {
            ui.selected_preset_id = Some(id);
            ui.selected_placement_ids = adoption
                .selected
                .iter()
                .map(|(_, record)| record.id.clone())
                .collect();
            format!(
                "已保存院落“{}”，原位置也已成组；可盖印、移动、旋转或复制",
                preset.label
            )
        }
        Err(error) => format!("院落模板已保存；原选择未成组：{error}。请重新选择成员"),
    };
    ui.status = message.clone();
    editor.status = message;
}

/// 热采用后只在原作者值仍完全一致时写入完整组元数据和一条撤销历史。
fn attach_source_members(
    adoption: &CatalogAdoption,
    preset_id: &str,
    session: &mut EditorAuthoringSession,
    editor: &mut MapEditorState,
) -> Result<(), String> {
    let draft = session
        .draft
        .as_mut()
        .ok_or_else(|| "地图草稿已经卸载".to_owned())?;
    if draft.map_id != adoption.map_id
        || session.revision != adoption.author_revision
        || session.placement_structure_revision != adoption.structure_revision
        || session.saving
    {
        return Err("地图或作者数据在目录写入期间发生变化".to_owned()); // 文件不回滚，绝不串写新草稿。
    }
    for (index, before) in &adoption.selected {
        if draft.placements.placements.get(*index) != Some(before) {
            return Err(format!("原成员{}已改变", before.id)); // 先完整预检，避免半组归属。
        }
    }
    let instance_id = format!("courtyard-instance-{preset_id}"); // 新模板首次保存处形成独立副本。
    let mut changed = Vec::with_capacity(adoption.selected.len());
    for (ordinal, (index, before)) in adoption.selected.iter().enumerate() {
        let mut after = before.clone();
        after.courtyard = Some(MapPlacementCourtyardMembership {
            instance_id: instance_id.clone(),
            preset_id: preset_id.to_owned(),
            member_id: format!("member-{}", ordinal + 1),
            origin: adoption.origin.to_array(),
            yaw_degrees: adoption.yaw_degrees,
        }); // 与模板成员稳定 ID 顺序严格对应。
        changed.push((*index, before.clone(), after));
    }
    let change = crate::authoring::CourtyardBatchChange::new(Vec::new(), changed)?; // 失败必须发生在草稿修改前。
    for (index, _, after) in &change.changed {
        draft.placements.placements[*index] = after.clone(); // 完整历史已构造后才原子写入归属。
    }
    session.push_bounded_water_history(EditorCommand::CourtyardBatch(Box::new(change))); // 成组归属单步撤销，原模型位置不发生变化。
    session.mark_placement_structure_changed(editor);
    Ok(())
}
