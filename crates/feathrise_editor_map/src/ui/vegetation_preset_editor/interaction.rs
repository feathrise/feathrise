//! 植被预设多标签工作区按钮、IME 与单字段字符缓冲输入。

mod focus;
mod input; // 官方可见字段和焦点同步。
pub(super) use focus::{sync_text_focus, text_bundle}; // 本域构造和注册入口。

pub(super) use input::handle_text_input;

pub(super) use input::{clear_input, commit_pending_input}; // 显式赋值接管当前路径输入；只读定位不得提交或清除输入。

use std::time::{SystemTime, UNIX_EPOCH}; // 新建稳定 ID 包含真实时间戳，删除后不正常复用。

use bevy::prelude::*; // Interaction、资源与查询。
use bevy::window::{PrimaryWindow, Window}; // 字段切换时同步主窗口 IME 开关。
use feathrise_map_content::{VegetationPresetRegistry, VegetationPresetRuntime}; // 编辑入口克隆已校验正式描述。
use feathrise_scene::DynamicMeshProfileRequests; // 选择已有Mesh作者层时继续触发正式多LOD原型按需加载。

use super::fields::field_value; // 聚焦时从强类型描述生成无损字符缓冲。
use super::inspection::begin_asset_inspection; // Asset 按钮只登记异步加载，不同步解析 GLB。
use super::mutations; // 枚举切换和 Mesh 结构动作独立维护。
use super::persistence::{start_delete, start_upsert}; // 保存与删除统一进入乐观并发 IO 事务。
use super::types::{
    InspectionPurpose, PresetDraft, PresetDraftMode, PresetEditorAction, PresetEditorButton,
    PresetField, PresetWorkspaceTab, VegetationPresetEditorState,
}; // 局部动作、字段、标签与唯一会话状态。
use crate::authoring::EditorAuthoringSession; // 删除前检查当前未保存地图引用。
use crate::resources::MapEditorState; // 当前预设与右侧 Asset 选择来自编辑器会话。
use crate::ui::actions::{select_vegetation_preset, set_ime_enabled}; // 目录选择与旧作者层按钮复用同一稳定ID激活语义。

/// 处理左栏和中央动态工作区的全部预设按钮。
#[allow(clippy::too_many_arguments)] // 每个参数属于明确作者门，集中分派保证一次 Interaction 只消费一次。
pub(super) fn handle_buttons(
    buttons: Query<(&feathrise_editor_ui::EditorButtonClick, &PresetEditorButton), Changed<feathrise_editor_ui::EditorButtonClick>>,
    guard: Res<crate::ui::text_input::EditorTextInputGuard>, // IME同帧确认不能顺带触发按钮事务。
    mut state: ResMut<VegetationPresetEditorState>,
    mut editor: ResMut<MapEditorState>,
    registry: Option<Res<VegetationPresetRegistry>>,
    session: Res<EditorAuthoringSession>,
    asset_server: Res<AssetServer>,
    mut mesh_profile_requests: Option<ResMut<DynamicMeshProfileRequests>>, // 数据Mesh预设选择可幂等请求当前正式原型。
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !editor.open {
        return;
    }
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        if state.active_field.is_some_and(PresetField::accepts_text) && guard.block_shortcuts {
            continue;
        } // 候选确认只交官方输入法。
        let action = button.0;
        if matches!(action, PresetEditorAction::AssetReference(_)) {
            continue; // 专用资产系统区分只读定位和写入，不能在这里先提交字符缓冲。
        }
        if matches!(action, PresetEditorAction::Focus(field) if state.active_field == Some(field)) {
            continue;
        } // 点击当前备注只保持编辑，不提交重建并把滚动位置弹回。
        if !matches!(action, PresetEditorAction::DeleteSelected) {
            state.delete_confirmation_id = None; // 任意其他动作解除左栏危险删除确认。
        }
        if !matches!(
            action,
            PresetEditorAction::Cancel | PresetEditorAction::ClosePresetTab(_)
        ) && let Err(error) = commit_pending_input(&mut state, &mut windows)
        {
            state.status = error;
            editor.status = state.status.clone();
            if action == PresetEditorAction::Save {
                state.record_save_warning(&mut editor);
            }
            continue;
        }
        let result = dispatch_action(
            action,
            &mut state,
            &mut editor,
            registry.as_deref(),
            &session,
            &asset_server,
            mesh_profile_requests.as_deref_mut(),
            &mut windows,
        );
        if let Err(error) = result {
            state.status = error;
            if action == PresetEditorAction::Save {
                editor.status = state.status.clone();
                state.record_save_warning(&mut editor);
            }
        }
        editor.status = state.status.clone(); // 底栏与预设工作区始终回显同一结果。
    }
}

#[allow(clippy::too_many_arguments)] // 分派边界显式携带各项授权资源，不隐藏全局访问。
fn dispatch_action(
    action: PresetEditorAction,
    state: &mut VegetationPresetEditorState,
    editor: &mut MapEditorState,
    registry: Option<&VegetationPresetRegistry>,
    session: &EditorAuthoringSession,
    asset_server: &AssetServer,
    mesh_profile_requests: Option<&mut DynamicMeshProfileRequests>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    match action {
        PresetEditorAction::SelectCatalogPreset(index) => {
            let registry = registry.ok_or_else(|| "植被预设目录尚未加载完成".to_owned())?; // 不用旧按钮文本或数组缓存猜测目录内容。
            let preset = registry
                .presets()
                .get(index)
                .ok_or_else(|| "植被预设列表已热更，请等待界面同步".to_owned())?; // 目录换代期间失败关闭，避免索引错选。
            select_vegetation_preset(editor, session, preset, mesh_profile_requests); // 选择规范ID，并在已有作者层时切到对应画布。
            state.status = editor.status.clone(); // 专用面板与底栏共同回显选择或地图缺失诊断。
            Ok(()) // 目录选择本身不修改预设草稿或磁盘文件。
        }
        PresetEditorAction::CreateBlade => {
            begin_create(state, editor, asset_server, InspectionPurpose::CreateBlade)
        }
        PresetEditorAction::CreateMesh => {
            begin_create(state, editor, asset_server, InspectionPurpose::CreateMesh)
        }
        PresetEditorAction::EditSelected => begin_edit(state, editor, registry),
        PresetEditorAction::DeleteSelected => begin_delete(state, editor, registry, session),
        PresetEditorAction::ActivateMapTab => activate_tab(state, PresetWorkspaceTab::Map, windows),
        PresetEditorAction::ActivatePresetTab(id) => {
            activate_tab(state, PresetWorkspaceTab::Vegetation(id), windows)
        }
        PresetEditorAction::ClosePresetTab(id) => close_tab(state, id, windows),
        PresetEditorAction::SelectSection(section) => select_section(state, section),
        PresetEditorAction::SelectLod(index) => select_lod(state, index),
        PresetEditorAction::SelectPart(index) => select_part(state, index),
        PresetEditorAction::Focus(field) => focus_field(state, field, windows),
        PresetEditorAction::AssetReference(_) => Ok(()), // 已由带页拥有者校验的专用系统处理。
        PresetEditorAction::ApplySelectedAsset => begin_apply_asset(state, editor, asset_server),
        PresetEditorAction::AppendMeshLod => begin_append_mesh(state, editor, asset_server),
        PresetEditorAction::DuplicateMeshLod => mutations::duplicate_mesh_lod(state),
        PresetEditorAction::RemoveMeshLod => mutations::remove_mesh_lod(state),
        PresetEditorAction::AppendBladeLod => mutations::append_blade_lod(state),
        PresetEditorAction::DuplicateBladeLod => mutations::duplicate_blade_lod(state),
        PresetEditorAction::RemoveBladeLod => mutations::remove_blade_lod(state),
        PresetEditorAction::DuplicateMeshPart => mutations::duplicate_mesh_part(state),
        PresetEditorAction::RemoveMeshPart => mutations::remove_mesh_part(state),
        PresetEditorAction::ToggleMotion => mutations::toggle_motion(state),
        PresetEditorAction::CycleCollision => mutations::cycle_collision(state),
        PresetEditorAction::CycleUltraBudget => mutations::cycle_ultra_budget(state),
        PresetEditorAction::CycleBillboard => mutations::cycle_billboard(state),
        PresetEditorAction::CycleMeshDerivation => mutations::cycle_mesh_derivation(state),
        PresetEditorAction::CycleAlphaMode => mutations::cycle_alpha_mode(state),
        PresetEditorAction::ToggleDoubleSided => mutations::toggle_double_sided(state),
        PresetEditorAction::ToggleShadow => mutations::toggle_shadow(state),
        PresetEditorAction::ToggleWind => mutations::toggle_wind(state),
        PresetEditorAction::CycleWindPart => mutations::cycle_wind_part(state),
        PresetEditorAction::ToggleCollisionScale => mutations::toggle_collision_scale(state),
        PresetEditorAction::Save => save_draft(state, registry, windows),
        PresetEditorAction::Cancel => {
            let id = state
                .active_preset_tab_id()
                .ok_or_else(|| "当前是地图视口标签".to_owned())?;
            close_tab(state, id, windows)
        }
    }
}

/// 新建允许与其他草稿标签并存，但全局资产/IO 任务仍保持唯一。
fn begin_create(
    state: &mut VegetationPresetEditorState,
    editor: &MapEditorState,
    asset_server: &AssetServer,
    purpose: InspectionPurpose,
) -> Result<(), String> {
    begin_asset_inspection(
        state,
        editor.selected_asset.as_deref(),
        asset_server,
        purpose,
    )
}

/// 从连续目录选择克隆正式描述；重复编辑同一 ID 直接聚焦已有标签。
fn begin_edit(
    state: &mut VegetationPresetEditorState,
    editor: &MapEditorState,
    registry: Option<&VegetationPresetRegistry>,
) -> Result<(), String> {
    if state.busy() {
        return Err("已有预设任务正在进行".to_owned());
    }
    let registry = registry.ok_or_else(|| "植被预设目录尚未加载完成".to_owned())?;
    let id = editor
        .vegetation_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先点击左栏一个预设，再选择编辑".to_owned())?;
    let preset = registry
        .resolve(id)
        .ok_or_else(|| format!("所选预设 {id} 已不存在"))?
        .clone();
    let label = preset.label.clone();
    state.open_draft(PresetDraft::new(
        PresetDraftMode::Edit {
            original_id: preset.id.clone(),
        },
        preset,
    ));
    state.status = format!("已打开“{label}”。在中央标签编辑参数，完成后保存预设。"); // 成功提示说明下一步和保存入口。
    Ok(())
}

/// 删除只阻止目标自身仍在编辑；其他独立草稿标签可以继续保留。
fn begin_delete(
    state: &mut VegetationPresetEditorState,
    editor: &MapEditorState,
    registry: Option<&VegetationPresetRegistry>,
    session: &EditorAuthoringSession,
) -> Result<(), String> {
    if state.busy() {
        return Err("已有预设任务正在进行".to_owned());
    }
    let registry = registry.ok_or_else(|| "植被预设目录尚未加载完成".to_owned())?;
    let id = editor
        .vegetation_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先点击左栏一个预设，再选择删除".to_owned())?;
    let preset = registry
        .resolve(id)
        .ok_or_else(|| format!("所选预设 {id} 已不存在"))?;
    if state.tabs.iter().any(|tab| {
        matches!(&tab.draft.mode, PresetDraftMode::Edit { original_id } if original_id == &preset.id)
    }) {
        return Err("该预设仍在中央编辑标签中；请先关闭该标签再删除".to_owned());
    }
    if state.delete_confirmation_id.as_deref() != Some(preset.id.as_str()) {
        state.delete_confirmation_id = Some(preset.id.clone());
        state.status = format!(
            "再次点击“删除所选预设”确认删除“{}”。仍被地图引用的预设无法删除。", // 与按钮实际名称及引用保护一致。
            preset.label
        );
        return Ok(());
    }
    state.delete_confirmation_id = None;
    start_delete(state, registry, session, preset)
}

/// 激活地图或预设标签并清理旧字符焦点。
fn activate_tab(
    state: &mut VegetationPresetEditorState,
    tab: PresetWorkspaceTab,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    if let PresetWorkspaceTab::Vegetation(id) = tab
        && !state.tabs.iter().any(|candidate| candidate.id == id)
    {
        return Err("目标预设标签已关闭".to_owned());
    }
    clear_input(state, windows);
    state.active_tab = tab;
    for candidate in &mut state.tabs {
        candidate.close_confirmation = false;
    }
    state.touch_ui();
    Ok(())
}

/// 关闭标签；脏草稿必须对同一标签连续点击两次。
fn close_tab(
    state: &mut VegetationPresetEditorState,
    tab_id: u64,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    if state.busy() {
        return Err("资产检查或目录保存期间不能关闭预设标签".to_owned());
    }
    let index = state
        .tabs
        .iter()
        .position(|tab| tab.id == tab_id)
        .ok_or_else(|| "目标预设标签已关闭".to_owned())?;
    let pending_dirty = state.active_preset_tab_id() == Some(tab_id)
        && state.active_field.is_some_and(|field| {
            field_value(&state.tabs[index].draft.descriptor, field)
                .is_ok_and(|value| value != state.input_buffer)
        }); // 未按提交的备注也属于待丢弃修改，不能绕过二次关闭确认。
    if (state.tabs[index].draft.dirty() || pending_dirty) && !state.tabs[index].close_confirmation {
        // 已提交草稿和未提交字符共同决定关闭保护。
        for tab in &mut state.tabs {
            tab.close_confirmation = tab.id == tab_id;
        }
        state.status = format!(
            "“{}”有未保存修改；再次点击标签关闭按钮确认丢弃",
            state.tabs[index].draft.descriptor.label
        );
        state.touch_ui();
        return Ok(());
    }
    clear_input(state, windows);
    let was_active = state.active_tab == PresetWorkspaceTab::Vegetation(tab_id);
    let closed = state.tabs.remove(index);
    if was_active {
        state.active_tab = state
            .tabs
            .get(index.saturating_sub(1))
            .map(|tab| PresetWorkspaceTab::Vegetation(tab.id))
            .unwrap_or(PresetWorkspaceTab::Map);
    }
    state.status = format!("已关闭“{}”预设标签", closed.draft.descriptor.label);
    state.touch_ui();
    Ok(())
}

/// 切换活动标签内部的大分区。
fn select_section(
    state: &mut VegetationPresetEditorState,
    section: super::types::PresetEditorSection,
) -> Result<(), String> {
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    draft.section = section;
    state.touch_ui();
    Ok(())
}

/// 选择当前预设的一档 LOD，并把部件索引钳制到真实范围。
fn select_lod(state: &mut VegetationPresetEditorState, index: usize) -> Result<(), String> {
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let count = match &draft.descriptor.runtime {
        VegetationPresetRuntime::Blade { lods, .. } => lods.len(),
        VegetationPresetRuntime::Mesh { lods, .. } => lods.len(),
    };
    if index >= count {
        return Err("目标 LOD 已不存在".to_owned());
    }
    draft.selected_lod = index;
    draft.selected_part = 0;
    state.touch_ui();
    Ok(())
}

/// 选择当前 Mesh LOD 的一个部件。
fn select_part(state: &mut VegetationPresetEditorState, index: usize) -> Result<(), String> {
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let VegetationPresetRuntime::Mesh { lods, .. } = &draft.descriptor.runtime else {
        return Err("当前预设不是数据 Mesh".to_owned());
    };
    let parts = lods
        .get(draft.selected_lod)
        .ok_or_else(|| "当前 LOD 已不存在".to_owned())?;
    if index >= parts.parts.len() {
        return Err("目标部件已不存在".to_owned());
    }
    draft.selected_part = index;
    state.touch_ui();
    Ok(())
}

/// 聚焦一个详细字段，并从强类型草稿取得唯一字符缓冲初值。
fn focus_field(
    state: &mut VegetationPresetEditorState,
    field: PresetField,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    if state.busy() {
        return Err("预设任务进行期间不能修改表单".to_owned());
    }
    let value = field_value(
        &state
            .active_draft()
            .ok_or_else(|| "请先打开一个预设标签".to_owned())?
            .descriptor,
        field,
    )?;
    state.active_field = Some(field);
    state.input_buffer = value;
    set_ime_enabled(windows, field.accepts_text());
    state.status = if field == PresetField::Notes {
        "备注已聚焦；Enter换行，Ctrl+Enter/Tab提交，Ctrl+A全选，Ctrl+V粘贴，Esc放弃本次输入"
    } else {
        "字段已聚焦；Enter/Tab 提交，Esc 放弃本次字符修改"
    }
    .to_owned(); // 明确备注Enter语义，其他字段保持原有操作。
    Ok(())
}

/// 用右侧 Asset 替换活动预设当前 LOD；实际 primitive 检查分帧完成。
fn begin_apply_asset(
    state: &mut VegetationPresetEditorState,
    editor: &MapEditorState,
    asset_server: &AssetServer,
) -> Result<(), String> {
    let tab_id = state
        .active_preset_tab_id()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    let lod = state
        .active_draft()
        .ok_or_else(|| "活动预设标签已不存在".to_owned())?
        .selected_lod;
    begin_asset_inspection(
        state,
        editor.selected_asset.as_deref(),
        asset_server,
        InspectionPurpose::ReplaceLod { tab_id, lod },
    )
}

/// 把右侧 Asset 作为新的 Mesh LOD 追加到活动标签。
fn begin_append_mesh(
    state: &mut VegetationPresetEditorState,
    editor: &MapEditorState,
    asset_server: &AssetServer,
) -> Result<(), String> {
    let tab_id = state
        .active_preset_tab_id()
        .ok_or_else(|| "请先打开一个预设标签".to_owned())?;
    if !matches!(
        state.active_draft().map(|draft| &draft.descriptor.runtime),
        Some(VegetationPresetRuntime::Mesh { .. })
    ) {
        return Err("只有数据 Mesh 预设可以追加模型 LOD".to_owned());
    }
    begin_asset_inspection(
        state,
        editor.selected_asset.as_deref(),
        asset_server,
        InspectionPurpose::AppendMesh { tab_id },
    )
}

/// 启动活动标签的全局目录原子保存。
fn save_draft(
    state: &mut VegetationPresetEditorState,
    registry: Option<&VegetationPresetRegistry>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    let registry = registry.ok_or_else(|| "植被预设目录尚未加载完成".to_owned())?;
    let tab_id = state
        .active_preset_tab_id()
        .ok_or_else(|| "当前没有可保存的预设标签".to_owned())?;
    let draft = state
        .active_draft()
        .ok_or_else(|| "活动预设标签已不存在".to_owned())?;
    let mode = draft.mode.clone();
    let descriptor = draft.descriptor.clone();
    start_upsert(state, registry, &mode, descriptor, tab_id)?;
    clear_input(state, windows);
    state.delete_confirmation_id = None;
    Ok(())
}

/// 返回当前毫秒；系统时钟异常时用零并由目录碰撞后缀保持唯一。
pub(super) fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin; // 专用输入系统需要真实AssetServer资源但不执行任何文件加载。
    use feathrise_map_content::{
        ResolvedVegetationPresetCatalog, VEGETATION_PRESET_SCHEMA_VERSION,
        VegetationPresetCatalogAsset,
    }; // 以正式公共校验建立随包目录注册表。

    use super::*; // 测试直接运行生产Interaction系统。

    /// 构造只保留随包普通草的完整有效目录快照。
    fn ordinary_grass_registry() -> VegetationPresetRegistry {
        let source: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"), // 从map_editor crate定位工作区资产目录。
            "/tests/fixtures/vegetation_presets.vegetation_presets.json" // 使用开发者实际编辑的数据格式。
        )))
        .expect("随包植被预设目录必须可解析"); // JSON语法回归直接使测试失败。
        let ordinary_grass = source
            .presets
            .into_iter()
            .find(|preset| preset.id == "hn01_ordinary_grass")
            .expect("随包目录必须登记普通草"); // 第一个用户报告目标必须真实存在。
        let catalog = VegetationPresetCatalogAsset {
            schema_version: VEGETATION_PRESET_SCHEMA_VERSION, // 使用当前完整数据协议。
            presets: vec![ordinary_grass],                    // 单项目录使按钮零索引语义明确。
        };
        VegetationPresetRegistry {
            catalog: Handle::default(), // 测试不触发AssetServer热加载。
            revision: 1,                // 非零修订代表已经原子采用的目录。
            resolved: ResolvedVegetationPresetCatalog::build(&catalog)
                .expect("普通草必须通过运行时同一份目录校验"), // 不绕过正式数据约束。
            last_error: None,           // 成功目录没有沿用上版诊断。
        }
    }

    #[test]
    fn dedicated_preset_button_press_selects_ordinary_grass() {
        let mut editor = MapEditorState::default(); // 初始没有目录管理选择和地图草稿。
        editor.open = true; // F12打开时专用输入系统才消费Interaction。
        let mut app = App::new(); // 最小ECS应用足以验证已确认点击的业务分派。
        app.add_plugins((MinimalPlugins, AssetPlugin::default())) // 提供时钟、任务池和AssetServer。
            .insert_resource(editor) // 选择结果写回真实编辑器会话。
            .init_resource::<crate::ui::text_input::EditorTextInputGuard>() // 无候选时允许真实预设按钮动作。
            .init_resource::<VegetationPresetEditorState>() // 面板状态接收同一条状态消息。
            .init_resource::<EditorAuthoringSession>() // 无草稿正是本次回归场景。
            .insert_resource(ordinary_grass_registry()) // 按钮索引解析当前完整目录。
            .add_systems(Update, handle_buttons); // 安装生产专用预设点击系统。
        app.world_mut().spawn((
            feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed, // 显式发布普通草按钮的已确认点击。
            PresetEditorButton(PresetEditorAction::SelectCatalogPreset(0)), // 零索引必须是合法真实动作。
        ));

        app.update(); // 点击查询的Changed tick只允许生产系统消费一次。

        assert_eq!(
            app.world()
                .resource::<MapEditorState>()
                .vegetation_selected_preset_id
                .as_deref(), // 读取专用输入链最终采用的稳定ID。
            Some("hn01_ordinary_grass"), // 不依赖通用地图按钮链或已加载作者草稿。
        ); // 用户点击后编辑、删除和后续新空白层都能读取同一选择。
    }
}
