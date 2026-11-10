//! 建筑预设选择、新建、编辑、删除、标签切换、摆放控制和持久化动作。
//!
//! 【功能设计】按钮动作先完成旧输入收口，再以完整目录、标签和场景身份执行单次事务。

use super::numeric_input::{clear_input, label_from_asset_path, touch, unique_id}; // 草稿动作共享唯一输入清理和身份生成规则。
use super::*; // 复用父模块的建筑草稿、目录、窗口与编辑器协议。

/// 分派一个已经收口旧输入的建筑动作。
#[allow(clippy::too_many_arguments)] // 纯分派显式携带所有身份来源，避免隐藏全局状态。
pub(super) fn dispatch_action(
    action: BuildingPresetAction,
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    editor: &mut SceneEditorState,
    registry: Option<&BuildingPresetRegistry>,
    preview: &AssetPreviewState,
    session: &EditorAuthoringSession,
    controls: &mut BuildingPlacementControls,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    match action {
        BuildingPresetAction::SelectPreset(index) => select_preset(index, state, editor, registry),
        BuildingPresetAction::CreateFromSelectedAsset => {
            create_from_selected_asset(state, workspace, editor, registry, preview)
        }
        BuildingPresetAction::EditSelected => edit_selected(state, workspace, editor, registry),
        BuildingPresetAction::DeleteSelected => delete_selected(state, editor, registry, session),
        BuildingPresetAction::ActivateTab(id) => activate_tab(state, workspace, id, windows),
        BuildingPresetAction::CloseTab(id) => close_tab(state, workspace, id, windows),
        BuildingPresetAction::Focus(field) => focus_field(state, workspace, field, windows),
        BuildingPresetAction::SelectSection(section) => select_section(state, workspace, section),
        BuildingPresetAction::SelectConnection(index) => select_connection(state, workspace, index),
        BuildingPresetAction::CycleCategory => cycle_category(state, workspace),
        BuildingPresetAction::CycleInterior => cycle_interior(state, workspace),
        BuildingPresetAction::PlaceSelected => place_selected(state, editor, registry),
        BuildingPresetAction::RotateLeft => {
            rotate_controls(state, controls, -90.0); // 建筑始终保持竖直，只绕世界 Y 轴旋转。
            Ok(())
        }
        BuildingPresetAction::RotateRight => {
            rotate_controls(state, controls, 90.0); // 九十度步进适合沿街和院落网格。
            Ok(())
        }
        BuildingPresetAction::CycleGrid => {
            cycle_grid(state, controls); // 关闭、0.5、1、2、4 米循环。
            Ok(())
        }
        BuildingPresetAction::ToggleConnectionSnap => {
            controls.connection_snap = !controls.connection_snap; // 普通点放默认保持不变。
            state.status = if controls.connection_snap {
                "已开启模块连接吸附；必须把模块连接点移到两米内的同类型现有连接点".to_owned()
            } else {
                "已关闭模块连接吸附；恢复普通网格点放".to_owned()
            };
            Ok(())
        }
        BuildingPresetAction::ToggleTerrainLeveling => {
            controls.level_terrain = !controls.level_terrain; // 只改变当前F12会话中的明确作者意图。
            state.status = if controls.level_terrain {
                "已开启单栋建筑整平；新建、移动或复制会与地形组成一次撤销事务".to_owned()
            } else {
                "已关闭单栋建筑整平；建筑只校验现有坡度，不修改地形".to_owned()
            };
            Ok(())
        }
        BuildingPresetAction::AddConnectionPoint => add_connection_point(state, workspace),
        BuildingPresetAction::RemoveConnectionPoint(index) => {
            remove_connection_point(state, workspace, index)
        }
        BuildingPresetAction::Save => save_draft(state, workspace, registry),
        BuildingPresetAction::Reset => reset_draft(state, workspace, windows),
        BuildingPresetAction::Cancel => {
            let PresetWorkspaceTab::Building(id) = workspace.active_tab else {
                return Err("当前不是建筑预设标签".to_owned());
            };
            close_tab(state, workspace, id, windows)
        }
    }
}

/// 选择一个目录项，但由“开始摆放”按钮显式进入写入工具。
fn select_preset(
    index: usize,
    state: &mut BuildingPresetEditorState,
    editor: &mut SceneEditorState,
    registry: Option<&BuildingPresetRegistry>,
) -> Result<(), String> {
    let registry = registry.ok_or_else(|| "建筑预设目录尚未加载".to_owned())?;
    let preset = registry
        .presets()
        .get(index)
        .ok_or_else(|| "建筑预设列表已热更，请等待界面同步".to_owned())?;
    editor.building_selected_preset_id = Some(preset.id.clone()); // 别名选择也规范化为当前 ID。
    state.status = format!(
        "已选择“{}” · {} · {}；点击开始摆放进入视口",
        preset.label,
        preset.category.label(),
        preset.interior.label(),
    );
    Ok(())
}

/// 从右侧当前 GLB/GLTF 和其精确预览目标建立安全初始草稿。
fn create_from_selected_asset(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    editor: &SceneEditorState,
    registry: Option<&BuildingPresetRegistry>,
    preview: &AssetPreviewState,
) -> Result<(), String> {
    if state.busy() {
        return Err("建筑目录任务进行中，暂不能新建".to_owned());
    }
    let registry = registry.ok_or_else(|| "建筑预设目录尚未加载".to_owned())?;
    let path = editor
        .selected_asset
        .as_deref()
        .ok_or_else(|| "请先在右侧 Asset 浏览器选择 GLB/GLTF 模型".to_owned())?;
    let lower = path.to_ascii_lowercase(); // 单个有界资产路径只在显式点击时分配小写副本。
    if !(lower.ends_with(".glb") || lower.ends_with(".gltf")) {
        return Err("建筑预设只能从 GLB 或 GLTF 模型新建".to_owned());
    }
    let target = preview.placement_target_for(path)?; // 蒙皮 primitive 会由资产预览层失败关闭。
    let descriptor = BuildingPresetDescriptor::from_asset(
        unique_id(registry),         // 新建身份不依赖数组索引。
        label_from_asset_path(path), // 初始中文名可在中央继续修改。
        path.to_owned(),             // 模型路径独立写入预设，场景实例后续仍会冻结同一路径。
        target,                      // Scene0 或合法静态 primitive 精确持久化。
    );
    let tab_id = state.open_draft(BuildingDraft::new(BuildingDraftMode::Create, descriptor));
    workspace.active_tab = PresetWorkspaceTab::Building(tab_id); // 中央立即显示完整空间语义表单。
    touch(state, workspace);
    state.status = "已从右侧模型建立建筑草稿；请配置占地、正门、地基和植被净空".to_owned();
    Ok(())
}

/// 打开所选规范项；同一项已有标签时直接复用。
fn edit_selected(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    editor: &SceneEditorState,
    registry: Option<&BuildingPresetRegistry>,
) -> Result<(), String> {
    if state.busy() {
        return Err("建筑目录任务进行中，暂不能编辑".to_owned());
    }
    let registry = registry.ok_or_else(|| "建筑预设目录尚未加载".to_owned())?;
    let id = editor
        .building_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先点击左栏一个建筑预设".to_owned())?;
    let preset = registry
        .resolve(id)
        .ok_or_else(|| format!("所选建筑预设{id}已不存在"))?
        .clone();
    let label = preset.label.clone(); // 移入草稿前保留反馈文案。
    let tab_id = state.open_draft(BuildingDraft::new(
        BuildingDraftMode::Edit {
            original_id: preset.id.clone(), // 编辑冻结规范身份。
        },
        preset, // 草稿与正式快照解耦。
    ));
    workspace.active_tab = PresetWorkspaceTab::Building(tab_id);
    touch(state, workspace);
    state.status = format!("已在中央打开“{label}”建筑空间语义标签");
    Ok(())
}

/// 两次点击确认后启动全场景引用保护删除。
fn delete_selected(
    state: &mut BuildingPresetEditorState,
    editor: &SceneEditorState,
    registry: Option<&BuildingPresetRegistry>,
    session: &EditorAuthoringSession,
) -> Result<(), String> {
    if state.busy() {
        return Err("建筑目录任务进行中，暂不能删除".to_owned());
    }
    let registry = registry.ok_or_else(|| "建筑预设目录尚未加载".to_owned())?;
    let id = editor
        .building_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先点击左栏一个建筑预设".to_owned())?;
    let preset = registry
        .resolve(id)
        .ok_or_else(|| "所选建筑预设已不存在".to_owned())?;
    if state.tabs.iter().any(|tab| {
        matches!(&tab.draft.mode, BuildingDraftMode::Edit { original_id } if original_id == &preset.id)
    }) {
        return Err("该建筑预设仍在中央编辑标签中；请先关闭标签".to_owned());
    }
    if state.delete_confirmation_id.as_deref() != Some(preset.id.as_str()) {
        state.delete_confirmation_id = Some(preset.id.clone());
        state.status = format!(
            "再次点击删除确认移除「{}」；当前草稿及所有场景引用都会自动阻止",
            preset.label,
        );
        return Ok(());
    }
    state.delete_confirmation_id = None;
    start_delete(state, registry, session, preset)
}

/// 激活一张仍存在的建筑草稿标签。
fn activate_tab(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    id: u64,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    if !state.tabs.iter().any(|tab| tab.id == id) {
        return Err("目标建筑预设标签已关闭".to_owned());
    }
    clear_input(state, windows); // 输入缓冲不得跨标签身份。
    workspace.active_tab = PresetWorkspaceTab::Building(id);
    for tab in &mut state.tabs {
        tab.close_confirmation = false; // 激活另一标签撤销旧二次确认。
    }
    touch(state, workspace);
    Ok(())
}

/// 关闭建筑标签；脏草稿必须第二次点击同一关闭键。
fn close_tab(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    id: u64,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    if state.busy() {
        return Err("建筑目录保存或热采用期间不能关闭标签".to_owned());
    }
    let index = state
        .tabs
        .iter()
        .position(|tab| tab.id == id)
        .ok_or_else(|| "目标建筑预设标签已关闭".to_owned())?;
    if state.tabs[index].draft.dirty() && !state.tabs[index].close_confirmation {
        for tab in &mut state.tabs {
            tab.close_confirmation = tab.id == id; // 只有目标标签进入确认态。
        }
        state.status = format!(
            "“{}”有未保存修改；再次点击标签关闭按钮确认丢弃",
            state.tabs[index].draft.descriptor.label,
        );
        touch(state, workspace);
        return Ok(());
    }
    clear_input(state, windows);
    let was_active = workspace.active_tab == PresetWorkspaceTab::Building(id);
    let closed = state.tabs.remove(index); // 草稿释放不修改磁盘目录。
    if was_active {
        workspace.active_tab = state
            .tabs
            .get(index.saturating_sub(1))
            .map(|tab| PresetWorkspaceTab::Building(tab.id))
            .or_else(|| {
                workspace
                    .tabs
                    .last()
                    .map(|tab| PresetWorkspaceTab::Vegetation(tab.id))
            })
            .unwrap_or(PresetWorkspaceTab::Scene); // 没有草稿时回到永久场景页。
    }
    state.status = format!("已关闭“{}”建筑预设标签", closed.draft.descriptor.label);
    touch(state, workspace);
    Ok(())
}

/// 建立一个字段输入会话，名称启用 IME，数值使用 ASCII 缓冲。
fn focus_field(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    field: BuildingPresetField,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    let value = state
        .active_draft(workspace.active_tab)
        .map(|draft| field_value(&draft.descriptor, field))
        .ok_or_else(|| "请先打开一个建筑预设标签".to_owned())?;
    state.active_field = Some(field);
    state.input_buffer = value;
    set_ime_enabled(windows, field.accepts_ime());
    if !field.accepts_text() {
        touch(state, workspace); // 数值按钮用缓冲加光标显示活动输入。
    }
    Ok(())
}

/// 按钮入口已收口旧输入；分区选择只推进UI修订，不修改作者描述。
fn select_section(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    section: BuildingPresetSection,
) -> Result<(), String> {
    let draft = state
        .active_draft_mut(workspace.active_tab)
        .ok_or_else(|| "请先打开一个建筑预设标签".to_owned())?;
    if draft.section != section {
        draft.section = section;
        touch(state, workspace);
    }
    Ok(())
}

/// 连接点保持原数组顺序及稳定ID，选择动作只决定哪一组字段可见。
fn select_connection(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    index: usize,
) -> Result<(), String> {
    let draft = state
        .active_draft_mut(workspace.active_tab)
        .ok_or_else(|| "请先打开一个建筑预设标签".to_owned())?;
    if index >= draft.descriptor.connections.len() {
        return Err("目标连接点已经不存在".to_owned());
    }
    if draft.selected_connection != index || draft.section != BuildingPresetSection::Connections {
        draft.selected_connection = index;
        draft.section = BuildingPresetSection::Connections;
        touch(state, workspace);
    }
    Ok(())
}

/// 循环建筑浏览类别，不改变任何摆放几何。
fn cycle_category(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
) -> Result<(), String> {
    let draft = state
        .active_draft_mut(workspace.active_tab)
        .ok_or_else(|| "请先打开一个建筑预设标签".to_owned())?;
    draft.descriptor.category = match draft.descriptor.category {
        BuildingPresetCategory::Residence => BuildingPresetCategory::Inn,
        BuildingPresetCategory::Inn => BuildingPresetCategory::Shop,
        BuildingPresetCategory::Shop => BuildingPresetCategory::Pavilion,
        BuildingPresetCategory::Pavilion => BuildingPresetCategory::Gate,
        BuildingPresetCategory::Gate => BuildingPresetCategory::Wall,
        BuildingPresetCategory::Wall => BuildingPresetCategory::Stall,
        BuildingPresetCategory::Stall => BuildingPresetCategory::Other,
        BuildingPresetCategory::Other => BuildingPresetCategory::Residence,
    };
    touch(state, workspace);
    Ok(())
}

/// 循环室内可达声明；实际碰撞和传送仍由对应系统决定。
fn cycle_interior(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
) -> Result<(), String> {
    let draft = state
        .active_draft_mut(workspace.active_tab)
        .ok_or_else(|| "请先打开一个建筑预设标签".to_owned())?;
    draft.descriptor.interior = match draft.descriptor.interior {
        BuildingInteriorKind::Inaccessible => BuildingInteriorKind::DirectEntry,
        BuildingInteriorKind::DirectEntry => BuildingInteriorKind::PortalEntry,
        BuildingInteriorKind::PortalEntry => BuildingInteriorKind::Inaccessible,
    };
    touch(state, workspace);
    Ok(())
}

/// 显式进入建筑点放工具，并保留当前规范预设选择。
fn place_selected(
    state: &mut BuildingPresetEditorState,
    editor: &mut SceneEditorState,
    registry: Option<&BuildingPresetRegistry>,
) -> Result<(), String> {
    let registry = registry.ok_or_else(|| "建筑预设目录尚未加载".to_owned())?;
    let selected = editor
        .building_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先选择一个建筑预设".to_owned())?;
    let preset = registry
        .resolve(selected)
        .ok_or_else(|| "所选建筑预设已不存在".to_owned())?;
    editor.building_selected_preset_id = Some(preset.id.clone()); // 旧别名在进入工具时迁移为规范 ID。
    editor.tool = EditorTool::Building; // 视口模式门会要求作者显式处于编辑模式。
    state.status = format!(
        "正在摆放“{}”；视口将校验占地、门口、边界和坡差",
        preset.label
    );
    Ok(())
}

/// 九十度调整当前建筑预览朝向。
fn rotate_controls(
    state: &mut BuildingPresetEditorState,
    controls: &mut BuildingPlacementControls,
    delta: f32,
) {
    controls.yaw_degrees = (controls.yaw_degrees + delta).rem_euclid(360.0); // 只保留四个规范朝向。
    state.status = format!("建筑摆放朝向已调整为 {:.0}°", controls.yaw_degrees);
}

/// 循环建筑世界网格尺寸。
fn cycle_grid(state: &mut BuildingPresetEditorState, controls: &mut BuildingPlacementControls) {
    controls.grid_step_meters = if controls.grid_step_meters <= 0.0 {
        0.5 // 关闭后从半米精调开始。
    } else if controls.grid_step_meters < 0.75 {
        1.0 // 半米后进入默认城镇网格。
    } else if controls.grid_step_meters < 1.5 {
        2.0 // 两米适合房屋间距粗调。
    } else if controls.grid_step_meters < 3.0 {
        4.0 // 四米适合街区级对齐。
    } else {
        0.0 // 最大档后关闭吸附。
    };
    state.status = if controls.grid_step_meters <= 0.0 {
        "已关闭建筑网格吸附".to_owned()
    } else {
        format!("建筑网格吸附已切换为 {:.1} 米", controls.grid_step_meters)
    };
}

/// 向当前草稿追加一个拥有唯一默认ID的地面连接点。
fn add_connection_point(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
) -> Result<(), String> {
    let draft = state
        .active_draft_mut(workspace.active_tab)
        .ok_or_else(|| "请先打开一个建筑预设标签".to_owned())?;
    if draft.descriptor.connections.len() >= MAX_BUILDING_CONNECTION_POINTS {
        return Err(format!(
            "单个建筑预设最多只能配置{MAX_BUILDING_CONNECTION_POINTS}个连接点"
        ));
    }
    let id = (1_u32..)
        .map(|index| format!("connection-{index}"))
        .find(|candidate| {
            draft
                .descriptor
                .connections
                .iter()
                .all(|connection| connection.id != *candidate)
        })
        .expect("32个连接点上限内一定存在u32候选ID");
    draft
        .descriptor
        .connections
        .push(BuildingConnectionPointDescriptor { id, ..default() }); // 默认类型和位置可立即在当前点表单继续编辑。
    draft.section = BuildingPresetSection::Connections;
    draft.selected_connection = draft.descriptor.connections.len() - 1;
    touch(state, workspace);
    state.status = "已新增地面连接点；请设置稳定ID、兼容类型、X/Z位置和90°朝外方向".to_owned();
    Ok(())
}

/// 从当前草稿移除一个连接点；正式目录只在随后显式保存时变化。
fn remove_connection_point(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    index: usize,
) -> Result<(), String> {
    let draft = state
        .active_draft_mut(workspace.active_tab)
        .ok_or_else(|| "请先打开一个建筑预设标签".to_owned())?;
    if index >= draft.descriptor.connections.len() {
        return Err("目标连接点已经不存在".to_owned());
    }
    let removed = draft.descriptor.connections.remove(index);
    draft.selected_connection = draft
        .selected_connection
        .saturating_sub(usize::from(index < draft.selected_connection))
        .min(draft.descriptor.connections.len().saturating_sub(1)); // 删除前面的点保留原选择；删当前点则选择相邻点。
    touch(state, workspace);
    state.status = format!("已从草稿移除连接点“{}”", removed.id);
    Ok(())
}

/// 保存当前建筑草稿；目录层会再次执行完整公共校验。
fn save_draft(
    state: &mut BuildingPresetEditorState,
    workspace: &VegetationPresetEditorState,
    registry: Option<&BuildingPresetRegistry>,
) -> Result<(), String> {
    let registry = registry.ok_or_else(|| "建筑预设目录尚未加载".to_owned())?;
    let PresetWorkspaceTab::Building(tab_id) = workspace.active_tab else {
        return Err("当前不是建筑预设标签".to_owned());
    };
    let draft = state
        .active_draft(workspace.active_tab)
        .ok_or_else(|| "活动建筑预设标签已不存在".to_owned())?;
    let mode = draft.mode.clone(); // 先冻结值，随后可变借用任务槽。
    let descriptor = draft.descriptor.clone(); // 显式保存允许一次小型描述复制。
    start_upsert(state, registry, &mode, descriptor, tab_id)
}

/// 恢复当前标签最近一次热采用基线。
fn reset_draft(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    let draft = state
        .active_draft_mut(workspace.active_tab)
        .ok_or_else(|| "活动建筑预设标签已不存在".to_owned())?;
    draft.descriptor = draft.baseline.clone(); // 新建草稿恢复创建时安全默认值，编辑恢复正式快照。
    draft.selected_connection = draft
        .selected_connection
        .min(draft.descriptor.connections.len().saturating_sub(1));
    clear_input(state, windows);
    state.status = "已恢复当前建筑标签的最近基线".to_owned();
    touch(state, workspace);
    Ok(())
}
