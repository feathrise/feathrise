//! 地表作者层列表的稳定选择、分页和后台删除请求。

use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*; // 原生按钮Changed查询与作者资源借用。
use feathrise_map_content::SurfacePresetRegistry; // 未使用预设不能误删旧地图层。

use crate::authoring::{
    EditorAuthoringSession, SurfaceBrushStroke, SurfaceLayerEditTask, start_surface_layer_delete,
}; // 删除整图在作者域的唯一后台槽执行。
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState}; // 复用公共鼠标所有权门。

use super::components::{SurfaceLayerButton, SurfaceLayerRow}; // 只消费当前地图层列表的交互。
use super::helpers::{page_target, select_layer, selected_index, target_is_current}; // 选择与删除共享身份语义。
use super::types::SurfaceLayerAction; // 分页和删除分别处理。

/// 同一鼠标入口先确认显示身份，再发布唯一的作者操作。
pub(super) fn handle_input(
    rows: Query<(Ref<EditorButtonClick>, &SurfaceLayerRow)>, // 固定六行也用于复验删除目标确实已显示。
    buttons: Query<(&EditorButtonClick, &SurfaceLayerButton), Changed<EditorButtonClick>>, // 地图局部操作按钮。
    guard: Res<MapEditorNumericScrubMouseGuard>,       // 数值拖动和释放帧不补触发旧点击。
    mut editor: ResMut<MapEditorState>,                // 更新选中索引、工具与状态反馈。
    mut session: ResMut<EditorAuthoringSession>,       // 显式创建仅追加元数据，删除像素仍在后台。
    stroke: Res<SurfaceBrushStroke>,                   // 不能在尚未收口的笔划中切换或删除目标层。
    registry: Option<Res<SurfacePresetRegistry>>,      // 解析当前选择的规范ID和历史别名。
    mut task: ResMut<SurfaceLayerEditTask>,            // 整图删除共享一个single-flight后台槽。
    mut candidates: Option<ResMut<crate::viewport::LayerCandidateState>>,
    property_context: (Option<Res<crate::ui::sidebars::MapEditorSidebarState>>,
        Query<&crate::ui::scene_inspector::SelectedToolProperties>,
        Option<Res<crate::game_authoring::map_interaction::MapInteractionState>>,
        Option<Res<crate::viewport::modes::ViewportModeState>>),
) {
    if !editor.open || editor.close_requested || editor.new_map_dialog_open || guard.blocked() {
        return; // 系统仍推进Changed tick，阻塞边沿不会在后续补执行。
    } // 只接受当前可操作编辑器的真实点击。
    let pressed = rows
        .iter()
        .any(|(interaction, _)| interaction.is_changed() && interaction.is_clicked())
        || buttons
            .iter()
            .any(|(interaction, _)| interaction.is_clicked()); // 只检查固定数量的交互对象。
    if !pressed {
        return;
    } // 稳定帧不读取地表层或写任何状态。
    let in_properties = property_context.0.as_ref().is_some_and(|sidebar| sidebar.right == crate::ui::sidebars::EditorRightSidebarTab::Scene)
        && property_context.1.iter().any(|form| form.shown_in_inspector(crate::ui::scene_inspector::SelectedToolPropertiesKind::Surface));
    let previous_tool = editor.tool;
    let mut selected = false;
    if session.draft.as_ref().map(|draft| &draft.map_id) != editor.selected_map_id.as_ref() {
        editor.status = "地图作者数据正在切换，请等待地表作者层刷新".to_owned(); // 新地图已选中但旧草稿仍可见时冻结旧列表点击。
        return; // 不把旧地图层索引写入新地图的笔刷选择。
    } // 相同地图的有效操作才继续。
    if stroke.active || task.is_pending() || session.saving {
        // 新建也必须避开冻结中的像素任务与保存快照。
        editor.status = "地表笔划或图层删除正在完成，请稍候再选择图层".to_owned(); // 保留操作归属和完整撤销边界。
        return; // 已在途的删除不会因为另一次点击叠加任务。
    } // 画面仍使用上一个完整结果。
    for (interaction, row) in &rows {
        if !interaction.is_changed() || !interaction.is_clicked() {
            continue;
        } // 持续按下和悬停不重复改变选择。
        let Some(target) = row.target.as_ref() else {
            continue;
        }; // 未使用槽位不可点击。
        if !target_is_current(target, &session) {
            editor.status = "地表作者层已更新，请在列表刷新后重新选择".to_owned(); // 不以新下标猜测作者意图。
            continue; // 换图、撤销、删除后的旧行完全无副作用。
        } // 身份正确才同步笔刷。
        select_layer(target.index, &mut editor, &session); // 点选层本身不创建地图引用。
        selected = true;
    } // 所选高亮在本帧Presentation回显。
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue;
        } // 只消费真实松开点击。
        if button.0 == SurfaceLayerAction::Create {
            // 新建不要求当前表非空或已有选中行。
            match crate::authoring::create_surface_layer(
                &mut editor,
                &mut session,
                registry.as_deref(),
            ) {
                // 后端统一维护唯一ID和撤销历史。
                Ok(_) => { selected = true; editor.status = "已新建并选中地表作者层，可在地图上绘制".to_owned(); }, // 行在本帧Presentation显示。
                Err(error) => editor.status = error, // 未选预设或未加载草稿时明确反馈。
            } // 创建不自动涂满地图。
            continue; // 不把新建误解释为翻页或删除。
        } // 其余操作使用已有作者行。
        let Some(surface) = session
            .draft
            .as_ref()
            .and_then(|draft| draft.surface.as_ref())
        else {
            continue;
        }; // 不对尚未加载地图操作。
        if button.0 != SurfaceLayerAction::DeleteSelected {
            if let Some(index) = page_target(
                surface.layers.len(),
                editor.selected_surface_layer,
                button.0 == SurfaceLayerAction::PreviousPage,
            ) {
                select_layer(index, &mut editor, &session); // 分页明确直选该页首层。
                selected = true;
            } // 空表不制造选择。
            continue; // 分页不触发删除或保存。
        } // 以下只处理明确的删除按钮。
        let Some(index) = selected_index(surface, &editor, registry.as_deref()) else {
            editor.status = "请先在地表作者层列表中选中要删除的层".to_owned(); // 管理选择未引用预设时禁止沿用旧索引。
            continue; // 不把预设库选择误当作地图层选择。
        }; // 有效选择还需与当前显示行一致。
        let layer = &surface.layers[index]; // selected_index已完成范围检查。
        let visible = rows.iter().any(|(_, row)| {
            row.target.as_ref().is_some_and(|target| {
                target.index == index && target_is_current(target, &session) // 按钮只删除当前确实显示并高亮的条目。
            })
        }); // 固定六行扫描不随地图层数增长。
        if !visible {
            editor.status = "地表作者层已更新，请等待列表刷新后再删除".to_owned(); // 旧页或旧修订的高亮不能删除新层。
            continue; // 本次按钮边沿被消费，不在后续帧自动补删。
        } // 后台入口再次冻结同一稳定身份。
        if let Err(error) =
            start_surface_layer_delete(&mut editor, &session, &mut task, index, &layer.id)
        {
            editor.status = error; // 保存冲突和实际资源失败都明确显示。
        } // 后台完成后才原子采用像素、定义与撤销命令。
    } // 一次操作不扫描磁盘或删除源文件。
    if in_properties && selected {
        editor.tool = previous_tool; // 属性中的层选择不等于准备绘制，不改变顶部动态工具页签。
        if let Some(candidates) = candidates.as_mut()
            && let Some(layer) = session.draft.as_ref().and_then(|draft| draft.surface.as_ref())
                .and_then(|surface| surface.layers.get(editor.selected_surface_layer))
        {
            candidates.confirm_tree_author(crate::viewport::LayerTarget::Surface(layer.id.clone()), Some(editor.selected_surface_layer),
                &session, property_context.2.as_ref().map_or(0, |state| state.observed_generation),
                property_context.3.as_ref().map_or_else(Default::default, |state| state.selection_filter(&editor)));
        } // 保留同一表单与完整单选身份，避免“选另一层”被误当作退出属性。
    }
} // 保存和预览复用作者域既有流程。
