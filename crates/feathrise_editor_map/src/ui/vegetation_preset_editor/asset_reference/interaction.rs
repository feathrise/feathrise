//! 资产引用按钮分派；定位是纯导航，赋值必须属于按钮实际所在草稿和字段。

use super::super::interaction::{clear_input, commit_pending_input}; // 显式赋值只接管本行输入，其他字段沿用提交规则。
use super::super::types::{
    AssetReferenceAction, AssetReferenceRequest, PresetEditorAction, PresetEditorButton,
    PresetField, PresetReferenceOwner, ReferenceCursor, ReferenceKind, ReferencePhase,
    VegetationPresetEditorState,
}; // 完整会话身份和强类型字段。
use super::helpers::{freeze_target, reference_kind, target_is_current}; // 目标校验不依赖最后焦点。
use super::loading::begin_validation; // 唯一候选和人工选择使用同一验证流程。
use super::widgets::CHOICES_PER_PAGE; // 只显示固定数量候选但允许访问全部页。
use crate::asset_catalog::{normalize_asset_reference, request_asset_reveal}; // 定位与安全相对路径由资产目录统一管理。
use crate::resources::{EditorAssetCatalog, MapEditorNumericScrubMouseGuard, MapEditorState}; // 右侧选中项及统一鼠标所有权。
use crate::ui::sidebars::MapEditorSidebarState;
use bevy::gltf::Gltf; // 模型与材质从根资产发现真实子资产。
use bevy::prelude::*; // 查询、输入、资源与异步资产句柄。
use bevy::window::PrimaryWindow; // 沿用现有IME输入收口。
use std::time::Instant; // 冻结本次加载的真实时间。 // 定位时切换到右侧资产页。

/// 路径按钮独立于普通表单按钮处理，定位不能意外调用“提交当前输入”。
#[allow(clippy::too_many_arguments)] // 明确列出目录、会话和IME资源，避免隐藏全局状态。
pub(in super::super) fn handle_reference_buttons(
    buttons: Query<(Entity, &feathrise_editor_ui::EditorButtonClick, &PresetEditorButton), Changed<feathrise_editor_ui::EditorButtonClick>>, // 只消费真实按下边沿。
    parents: Query<&ChildOf>,             // 从按钮向上找到真正拥有者。
    owners: Query<&PresetReferenceOwner>, // 由中央详细页创建时写入标签ID。
    mut state: ResMut<VegetationPresetEditorState>, // 请求与草稿。
    mut editor: ResMut<MapEditorState>,   // 冻结右侧选择并显示状态。
    mut catalog: ResMut<EditorAssetCatalog>, // 异步反向定位。
    mut sidebar: ResMut<MapEditorSidebarState>, // 自动展示资产浏览器。
    text_guard: Res<crate::ui::text_input::EditorTextInputGuard>, // 官方组合事务未结束时不提交或重定位字段。
    guard: Res<MapEditorNumericScrubMouseGuard>, // 预览或数值拖动期间不得偷取点击。
    server: Res<AssetServer>,                    // 仅登记异步加载。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // 赋值时正确结束IME。
) {
    if !editor.open
        || editor.close_requested
        || editor.pending_map_transition.is_some()
        || guard.blocked()
        || (state.active_field.is_some_and(PresetField::accepts_text) && text_guard.block_shortcuts)
    // 原生组合状态由公共门唯一维护。
    {
        return;
    } // 关闭或鼠标已被其他操作捕获时忽略动作。
    for (entity, interaction, marker) in &buttons {
        let PresetEditorAction::AssetReference(action) = marker.0 else {
            continue;
        }; // 普通按钮交回原系统。
        if !interaction.is_clicked() {
            continue;
        } // 不把悬停当点击。
        let Some(tab_id) = parents
            .iter_ancestors(entity)
            .find_map(|parent| owners.get(parent).ok().map(|owner| owner.0))
        else {
            continue;
        }; // 没有真实页拥有者的孤立按钮不能写草稿。
        state.delete_confirmation_id = None; // 路径操作也打断此前的危险删除连续确认。
        let result = match action {
            AssetReferenceAction::Locate(field) => {
                freeze_target(&state, tab_id, field)
                    .and_then(|target| {
                        let path = if state.active_field == Some(field) {
                            state.input_buffer.as_str()
                        } else {
                            &target.original_path
                        }; // 定位当前可见字符，但不提交它。
                        request_asset_reveal(&mut catalog, &mut editor, &mut sidebar, path) // 后台定位成功才切目录、页和选中项。
                    })
                    .map(|()| {
                        state.status = editor.status.clone();
                    }) // 明确这是导航而不是赋值。
            }
            AssetReferenceAction::UseSelected(field) => {
                prepare_source(field, editor.selected_asset.as_deref()).and_then(|(kind, path)| {
                    if state.busy() {
                        return Err("请先完成或取消当前资产选择/保存，再设置另一条引用".to_owned());
                    } // 保持single-flight。
                    freeze_target(&state, tab_id, field)?; // 先拒绝旧页面按钮，不能连带提交另一个标签的输入。
                    if state.active_field == Some(field) {
                        clear_input(&mut state, &mut windows); // 显式选择接管本行未提交字符，允许从空路径直接选资产；正式原值仍保持不变。
                    } else {
                        commit_pending_input(&mut state, &mut windows)?; // 仅其他字段沿用原有离焦提交语义。
                    }
                    begin_request(&mut state, tab_id, field, kind, path, &server) // 冻结具体标签和原值。
                })
            }
            _ => selection_action(&mut state, tab_id, action, &server), // 票据保护的选择、翻页或取消。
        };
        if let Err(error) = result {
            state.status = error;
        } // 失败不改字段值。
        editor.status = state.status.clone(); // 全局状态栏与预设面板显示相同结果。
    }
}

/// 先进行便宜的类型/路径检查，错误选中项不能顺带提交正在输入的字段。
pub(super) fn prepare_source(
    field: PresetField,
    selected: Option<&str>,
) -> Result<(ReferenceKind, String), String> {
    let kind = reference_kind(field).ok_or_else(|| "当前字段不是资产引用".to_owned())?; // 数值字段不接收文件。
    let selected = selected.ok_or_else(|| "请先在左侧资产页选中文件".to_owned())?; // 不猜最后一个目录。
    let normalized = normalize_asset_reference(selected)?; // 拒绝根外/绝对/无效路径。
    let source = normalized
        .split_once('#')
        .map_or(normalized.as_str(), |(owner, _)| owner); // 子资产所属文件才是异步根。
    let extension = source
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default(); // 后缀只作早期提示，最终仍验证资产类型。
    let allowed = match kind {
        ReferenceKind::Image => matches!(
            extension.as_str(),
            "png" | "jpg" | "jpeg" | "ktx2" | "dds" | "tga" | "bmp" | "webp" | "hdr" | "exr"
        ), // 二维纹理文件。
        _ => matches!(extension.as_str(), "glb" | "gltf"), // 模型和材质均从真实GLTF取得。
    };
    if !allowed {
        return Err(if kind == ReferenceKind::Image {
            "此字段需要二维图片，不能使用当前选中的模型或数据文件"
        } else {
            "此字段需要GLB/GLTF，不能使用当前选中的图片或数据文件"
        }
        .to_owned()); // 保持字段原值。
    }
    Ok((kind, source.to_owned())) // 只复制一条合法资产路径。
}

/// 登记一个字段请求，尚不写任何草稿值。
fn begin_request(
    state: &mut VegetationPresetEditorState,
    tab_id: u64,
    field: PresetField,
    kind: ReferenceKind,
    source: String,
    server: &AssetServer,
) -> Result<(), String> {
    let target = freeze_target(state, tab_id, field)?; // 在加载前冻结真正页与字段。
    let ticket = state.next_reference_ticket; // 旧按钮不能匹配后续请求。
    state.next_reference_ticket = ticket.wrapping_add(1).max(1); // 保留零号无效身份。
    state.status = format!(
        "正在检查 {source} → {}；成功前原字段保持不变",
        target.description
    ); // 说明目标及非即时采用。
    state.asset_reference = Some(AssetReferenceRequest {
        ticket, // 当前请求唯一票据。
        target, // 完整字段身份。
        kind,   // 字段要求的真正类型。
        gltf: (kind != ReferenceKind::Image).then(|| server.load::<Gltf>(source.clone())), // 异步加载根，复用已有缓存。
        image: (kind == ReferenceKind::Image).then(|| server.load::<Image>(source.clone())), // 图片只加载Image。
        source_path: source,                // 右侧后续选择不会改变这次请求。
        phase: ReferencePhase::Loading,     // 所有结果先经过真实加载门。
        cursor: ReferenceCursor::default(), // 固定预算枚举从零开始。
        choices: Vec::new(),                // 不预先按最大资产数分配。
        page: 0,                            // 首次从第一组实际候选开始。
        started: Instant::now(),            // 真实时间超时。
    });
    state.touch_ui(); // 立即显示可取消的内嵌状态，不遮挡资产浏览器。
    Ok(())
}

/// 旧UI票据或已经改变的目标永远不能采用候选。
pub(super) fn selection_action(
    state: &mut VegetationPresetEditorState,
    owner: u64,
    action: AssetReferenceAction,
    server: &AssetServer,
) -> Result<(), String> {
    let ticket = match action {
        AssetReferenceAction::Choose { ticket, .. }
        | AssetReferenceAction::Previous { ticket }
        | AssetReferenceAction::Next { ticket }
        | AssetReferenceAction::Cancel { ticket } => ticket, // 明确携带源请求。
        _ => return Err("不是子资产选择动作".to_owned()), // 单独的定位和开始请求不进入这里。
    };
    let request = state
        .asset_reference
        .as_ref()
        .ok_or_else(|| "该资产选择已经结束".to_owned())?; // 没有隐式恢复旧请求。
    if request.ticket != ticket
        || request.target.tab_id != owner
        || !target_is_current(state, &request.target)
    {
        return Err("子资产选择已过期，请在当前字段重新操作".to_owned()); // 保留后续新请求，不被旧按钮取消。
    }
    if matches!(action, AssetReferenceAction::Cancel { .. }) {
        state.asset_reference = None; // 所有原字段仍未被修改。
        state.status = "已取消资产选择，原引用保持不变".to_owned(); // 明确没有落值。
        state.touch_ui(); // 移除内嵌面板。
        return Ok(());
    }
    let request = state.asset_reference.as_mut().unwrap(); // 上方已校验同一请求身份。
    if !matches!(request.phase, ReferencePhase::Choosing) {
        return Err("子资产仍在检查，请稍候或取消".to_owned());
    } // 不能选部分候选。
    match action {
        AssetReferenceAction::Choose { index, .. } => {
            let choice = *request
                .choices
                .get(index)
                .ok_or_else(|| "子资产选项已不存在".to_owned())?; // 使用真实完整列表的绝对索引。
            begin_validation(request, choice, server); // 真实类型验证完成后才赋值。
        }
        AssetReferenceAction::Previous { .. } => request.page = request.page.saturating_sub(1), // 不越过首组。
        AssetReferenceAction::Next { .. } => {
            request.page = request
                .page
                .saturating_add(1)
                .min(request.choices.len().saturating_sub(1) / CHOICES_PER_PAGE)
        } // 只限制页码合法性。
        _ => unreachable!("开始请求和取消已经在上方分派"), // 所有其他动作已排除。
    }
    state.touch_ui(); // 只重建当前有限候选行。
    Ok(())
}
