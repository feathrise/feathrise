//! BSN静态按钮的一帧结构化动作分派。

use feathrise_editor_ui::EditorButtonClick;
use bevy::ecs::system::SystemParam;
use bevy::input_focus::{FocusCause, InputFocus}; // 新建地图名称点击立即交给真实官方文本实体。
use bevy::prelude::*; // 按钮交互、资源与资产集合。
use bevy::window::{PrimaryWindow, Window}; // 新建地图字段切换时同步IME。
use feathrise_map_content::{MapCatalogAsset, MapContentRegistry, MapSkyMode}; // 编辑资料时重新验证正式目录身份。

use super::{focus_new_map_field, set_ime_enabled}; // 复用父模块的新建地图输入焦点逻辑。
use crate::authoring::{
    DraftLoadTask, EditorAuthoringSession, WaterBrushStroke,
    WaterNetworkEditState, request_map_creation,
}; // 新建地图事务与水域跨帧输入互斥门。
use crate::components::{MapEditorActionButton, NewMapNameText}; // BSN按钮携带结构化动作，名称使用真实编辑实体。
use crate::resources::{
    EditorAssetCatalog, MapBrowserState, MapCreationTask, MapDeletionTask,
    MapEditorNumericScrubMouseGuard, MapEditorState, MapEditorViewportState, MapFolderPersistence,
}; // 目录、新建、删除和数值拖动鼠标所有权状态。
use crate::types::{
    EditorAction, EditorTool, EditorView, EnvironmentEditAction, NewMapInputField,
}; // 按钮只发布结构化编辑器动作。
use crate::ui::action_availability::{EditorActionAvailability, EditorActionInputs};
use crate::ui::water_actions::handle_water_action; // 水域按钮由独立领域分派器优先消费。
use crate::viewport::set_editor_view; // 快捷方向与投影解耦，共用视口角度采用入口。

/// 通用按钮分派器的输入法门；新地图创建不再等待地表预设目录。
#[derive(SystemParam)]
pub(crate) struct EditorButtonInputGuard<'w> {
    input_guard: Res<'w, crate::ui::text_input::EditorTextInputGuard>, // 候选确认帧禁止按钮提交名称。
    load: Res<'w, DraftLoadTask>, // 在途草稿不得落后于资料编辑结果。
    registry: Option<Res<'w, MapContentRegistry>>, // 正式地图目录稳定身份。
    catalogs: Res<'w, Assets<MapCatalogAsset>>, // 提交时重验目录名称。
    folder_persistence: ResMut<'w, MapFolderPersistence>, // 编辑与归类共享唯一写入互斥。
    viewport: Option<ResMut<'w, MapEditorViewportState>>, // 只对准当前相机；空世界尚无RTT时不创建新视图。
    documents: Option<Res<'w, crate::ui::map_documents::MapDocuments>>,
    availability: EditorActionInputs<'w>,
    sidebar: Option<Res<'w, crate::ui::sidebars::MapEditorSidebarState>>,
    layout: Option<Res<'w, crate::resources::MapEditorLayoutState>>,
    mesh_edit: Option<Res<'w, crate::authoring::mesh_edit::MeshEditState>>,
    standalone_play: crate::standalone::StandalonePlayRequest<'w>, // 与F12共用独立模式试玩入口，一体模式保持原关闭门。
}

#[derive(SystemParam)]
pub(crate) struct NewMapNameFocus<'w, 's> {
    inputs: Query<'w, 's, Entity, With<NewMapNameText>>, // 外层Button只负责动作，官方键盘必须定位到子实体。
    focus: ResMut<'w, InputFocus>, // 名称框本帧取得焦点，不等待下一次自动绑定。
}

impl NewMapNameFocus<'_, '_> {
    fn activate(&mut self) {
        if let Ok(entity) = self.inputs.single() {
            self.focus.set(entity, FocusCause::Navigated);
        } // 静态场景尚未生成时由后续绑定系统补齐焦点。
    }
}

/// 所有BSN静态按钮以结构化动作分派，名称和层级变化不会影响业务。
#[allow(clippy::too_many_arguments)] // 按钮系统同时更新UI会话、目录、创建任务和IME，不把它们折叠成隐藏耦合资源。
pub(crate) fn handle_editor_action_buttons(
    buttons: Query<
        (
            &EditorButtonClick,
            &MapEditorActionButton,
            Has<bevy::ui::InteractionDisabled>,
            Option<&crate::ui::map_sidebar::MapSidebarActionBinding>,
        ),
        Changed<EditorButtonClick>,
    >,
    mut editor: ResMut<MapEditorState>,
    mut catalog: ResMut<EditorAssetCatalog>,
    mut creation: ResMut<MapCreationTask>,
    deletion: Res<MapDeletionTask>, // 回收事务期间禁止发起另一次catalog写入。
    map_browser: Res<MapBrowserState>, // 新地图确认时冻结当前虚拟文件夹和有效文件基线。
    session: Res<EditorAuthoringSession>,
    water_stroke: Res<WaterBrushStroke>,       // 旧水域工具和循环按钮不得在静水笔划中途串切对象。
    water_edit: Res<WaterNetworkEditState>,    // 左键拖点期间阻止按钮制造并发作者分支。
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>, // 拖动及三键release帧都丢弃静态按钮边沿。
    mut input_guard: EditorButtonInputGuard, // IME与资料事务依赖收口，保持Bevy系统参数数量上限。
    mut new_map_focus: NewMapNameFocus,      // 组合官方焦点与静态名称实体，保持按钮系统参数上限。
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !editor.open || numeric_scrub_mouse.blocked() && !input_guard.mesh_edit.as_ref().is_some_and(|state| state.moving()) {
        return; // 关闭和拖动帧仍推进Changed tick，隐藏控件的按下不能在重新打开后延迟执行。
    }
    for (interaction, button, disabled, binding) in &buttons {
        if !interaction.is_clicked()
            || (disabled && !EditorActionAvailability::supports(button.0))
        {
            continue;
        }
        if binding.is_some_and(|binding| !binding.matches(map_browser.selected_item.as_ref())) {
            continue; // 目录菜单只接受已过静默门并冻结原选择的唯一按下，不能让旧新建按钮改用新目录。
        }
        if input_guard.mesh_edit.as_ref().is_some_and(|state| state.active())
            && !matches!(button.0, EditorAction::Close | EditorAction::Save | EditorAction::Undo | EditorAction::Redo
                | EditorAction::ViewPerspective | EditorAction::ViewTop | EditorAction::ViewFront | EditorAction::ViewSide | EditorAction::ToggleOrthographic) {
            editor.status = "请先按 Tab 返回对象模式，再执行地图或对象操作".into();
            continue; // 关闭仍沿正式安全路径；其他按钮不能在点边面会话中改写完整对象或保存预览。
        }
        if matches!(button.0, EditorAction::RefreshAssets | EditorAction::AssetFolderUp | EditorAction::AssetPreviousPage | EditorAction::AssetNextPage)
            && (input_guard.sidebar.as_ref().is_some_and(|sidebar| !sidebar.assets_browser_visible())
                || input_guard.layout.as_ref().is_some_and(|layout| !layout.content_shown()))
        {
            continue; // 左侧浏览器隐藏后丢弃目录按钮旧边沿，不影响右侧属性或其他全局命令。
        }
        // 禁用组件是上一轮表现结果；业务必须读取本帧真实状态，防止刚阻塞或刚恢复时错发/漏发。
        if EditorActionAvailability::supports(button.0)
            && input_guard.availability
                .snapshot(&editor, input_guard.viewport.as_deref())
                .reason(button.0)
                .is_some()
        {
            continue;
        }
        if editor.map_transition_in_progress && button.0 != EditorAction::Close {
            continue; // 外壳继续显示，但加载期间不能新建、保存或修改另一份地图草稿。
        }
        if water_edit.dragging() {
            editor.status = "请先松开正在拖动的水网控制点".to_owned();
            continue; // 拖动事务只在松手形成完整收口，期间不接受任何面板动作。
        }
        if handle_water_action(button.0, &mut editor, &session, &water_stroke, &water_edit) {
            continue; // 水域选择与一帧动作已由独立模块完整消费。
        }
        if editor.new_map_dialog_open
            && !matches!(
                button.0,
                EditorAction::CancelNewMap
                    | EditorAction::ConfirmNewMap
                    | EditorAction::FocusNewMapName
            )
        {
            continue; // 弹框独占期间底层工具/保存/关闭按钮不得改工具或触发切模块。
        }
        match button.0 {
            EditorAction::Close => {
                if input_guard.standalone_play.request(&mut editor) { continue; }
                editor.request_close(); // 与F12共用安全关闭门；空世界收起界面保留空场景。
            }
            EditorAction::NewMap => {
                if deletion.busy() {
                    editor.status = "地图删除确认或回收事务尚未完成".to_owned();
                    continue; // 新建与删除不能并发改写catalog。
                }
                if input_guard.folder_persistence.busy() {
                    editor.status = "地图归类保存完成前不能新建地图".to_owned();
                    continue; // 创建请求必须冻结已经落盘的归类基线。
                }
                if let Some(reason) = map_browser.write_blocked.as_deref() {
                    editor.status = format!("地图归类尚未就绪，不能新建地图：{reason}");
                    continue; // 首载或损坏时不能误用虚拟根目录。
                }
                if editor.close_requested {
                    editor.cancel_close_request("已转入新建地图"); // 显式新建意图继续使用F12，不能让待关闭门阻止名称获得光标。
                }
                editor.new_map_dialog_open = true;
                editor.new_map_edit_target = None; // 新建不会沿用此前编辑地图的冻结目标。
                editor.new_map_name.clear();
                editor.new_map_notes.clear(); // 每次创建从空可选备注开始。
                editor.status = "请输入地图名称和可选备注，创建后可添加网格".to_owned();
                focus_new_map_field(&mut editor, NewMapInputField::Name, &mut windows);
                new_map_focus.activate(); // 弹框本帧打开后，下一键直接路由到名称组件。
            }
            EditorAction::EnterSelectedMap
            | EditorAction::SaveAndSwitchMap
            | EditorAction::DiscardAndSwitchMap
            | EditorAction::CancelMapSwitch
            | EditorAction::DeleteMap
            | EditorAction::ConfirmDeleteMap
            | EditorAction::ConfirmDeleteMapPermanently
            | EditorAction::CancelDeleteMap
            | EditorAction::AdoptBlenderTerrain
            | EditorAction::OverwriteBlenderTerrain
            | EditorAction::MapFolderCreate
            | EditorAction::MapFolderRename
            | EditorAction::ConfirmMapFolderDialog
            | EditorAction::CancelMapFolderDialog
            | EditorAction::FocusMapFolderName => {} // 地图树与文件夹模态由独立map_browser功能域处理。
            EditorAction::ConfirmNewMap => {
                if input_guard.input_guard.block_shortcuts {
                    editor.status = "请先确认输入法候选".to_owned();
                    continue;
                } // 保留官方候选提交与业务创建的先后边界。
                if let Some(target) = editor.new_map_edit_target.clone() {
                    let catalog = input_guard
                        .registry
                        .as_deref()
                        .and_then(|registry| input_guard.catalogs.get(&registry.catalog)); // 确认时读取最新目录，不依赖打开时行索引。
                    let result = if creation.task.is_some()
                        || creation.pending_after_save.is_some()
                        || deletion.busy()
                        || input_guard.load.task.is_some()
                    {
                        Err("地图创建、加载或回收尚未完成，请稍后保存资料".to_owned())
                    } else {
                        crate::map_browser::rename::begin_map_edit(
                            &target,                             // 稳定MapId、名称和备注原值必须一起核对。
                            &editor.new_map_name, // 中文名称支持与新建相同的输入范围。
                            &editor.new_map_notes, // 多行备注在确认帧一次性冻结。
                            &session,             // 当前草稿保存期间禁止资料双文件事务。
                            &mut input_guard.folder_persistence, // 后台写入与其他目录修改互斥。
                            catalog,              // 目录新修订不能被旧弹框覆盖。
                        )
                    };
                    match result {
                        Ok(()) => {
                            // 任务启动不是保存成功：保留草稿与弹框，让后台校验错误可见且可直接修正。
                            editor.status = "正在保存地图资料，请等待结果".to_owned(); // 不提前丢弃输入或显示已保存。
                        }
                        Err(error) => editor.status = error, // 校验失败保持弹框和原输入以便修正。
                    }
                    continue;
                }
                if deletion.busy() {
                    editor.status = "地图回收事务完成前不能新建地图".to_owned();
                } else {
                    request_map_creation(
                        &mut editor,
                        &mut creation,
                        &map_browser,
                        &input_guard.folder_persistence,
                        input_guard.documents.is_some(),
                    );
                }
            }
            EditorAction::CancelNewMap => {
                editor.new_map_dialog_open = false;
                editor.new_map_edit_target = None; // 取消只关闭当前编辑目标，不触及磁盘。
                if creation.cancel_pending_after_save() {
                    editor.save_requested = false; // 取消弹框同时撤销尚未启动的自动保存请求。
                }
                editor.new_map_name.clear();
                editor.new_map_notes.clear(); // 关闭表单后不把旧备注带入下一张地图。
                set_ime_enabled(&mut windows, false);
            }
            EditorAction::FocusNewMapName => {
                focus_new_map_field(&mut editor, NewMapInputField::Name, &mut windows);
                new_map_focus.activate(); // 点击边框也命中真实文本，而不是保留Button焦点。
            }
            EditorAction::RefreshAssets | EditorAction::AssetFolderUp
                | EditorAction::AssetPreviousPage | EditorAction::AssetNextPage => {
                crate::asset_catalog::navigation::navigate(button.0, &mut catalog, &mut editor);
            }
            EditorAction::SkyModeDynamic => {
                editor.environment_action_requested =
                    Some(EnvironmentEditAction::SetSkyMode(MapSkyMode::Dynamic)); // 动态天空不要求主 Cube 已设置。
            }
            EditorAction::SkyModeSkybox => {
                editor.environment_action_requested =
                    Some(EnvironmentEditAction::SetSkyMode(MapSkyMode::Skybox)); // 作者域统一检查主 KTX2 前置条件。
            }
            EditorAction::SetSelectedSkybox => {
                editor.environment_action_requested =
                    Some(EnvironmentEditAction::SetSelectedSkybox);
            }
            EditorAction::ClearSkybox => {
                editor.environment_action_requested = Some(EnvironmentEditAction::ClearSkybox);
            }
            EditorAction::Save => editor.save_requested = true,
            EditorAction::Undo => editor.undo_requested = true,
            EditorAction::Redo => editor.redo_requested = true,
            EditorAction::ViewPerspective => set_editor_view(
                &mut editor,
                input_guard.viewport.as_deref_mut(),
                EditorView::Perspective,
            ),
            EditorAction::ViewTop => set_editor_view(
                &mut editor,
                input_guard.viewport.as_deref_mut(),
                EditorView::Top,
            ),
            EditorAction::ViewFront => set_editor_view(
                &mut editor,
                input_guard.viewport.as_deref_mut(),
                EditorView::Front,
            ),
            EditorAction::ViewSide => set_editor_view(
                &mut editor,
                input_guard.viewport.as_deref_mut(),
                EditorView::Side,
            ),
            EditorAction::ToggleOrthographic => editor.orthographic = !editor.orthographic,
            EditorAction::FocusSelected => {
                editor.viewport_framing_requested =
                    Some(crate::types::EditorViewportFraming::Selected)
            }
            EditorAction::FrameMap => {
                editor.viewport_framing_requested = Some(crate::types::EditorViewportFraming::Map)
            }
            EditorAction::ToolRaise
            | EditorAction::ToolVolumeDig
            | EditorAction::ToolVolumeFill
            | EditorAction::ToolVolumeSmooth
            | EditorAction::ToolVolumeFlatten
            | EditorAction::ToolLower
            | EditorAction::ToolSmooth
            | EditorAction::ToolFlatten
            | EditorAction::ToolSurface
            | EditorAction::ToolPuddles => super::request_terrain_tool(button.0, &mut editor),
            EditorAction::RiverAdd
            | EditorAction::WaterfallAdd
            | EditorAction::WaterPointPrevious
            | EditorAction::WaterPointDelete
            | EditorAction::WaterSegmentKindToggle
            | EditorAction::WaterSourceNext
            | EditorAction::WaterTargetNext
            | EditorAction::WaterAddLayer
            | EditorAction::WaterDuplicateLayer
            | EditorAction::WaterListPreviousPage
            | EditorAction::WaterListNextPage
            | EditorAction::WaterDeleteSelected
            | EditorAction::WaterLevelFromCursor => {
                unreachable!("水域按钮已由water_actions提前消费")
            }
            EditorAction::ToolPlaceAsset => editor.tool = EditorTool::PlaceAsset,
            EditorAction::ToolPortal => editor.tool = EditorTool::Portal,
            EditorAction::PortalDeleteSelected
            | EditorAction::PortalNameFocus
            | EditorAction::PortalNameApply => {} // 传送列表域自行处理当前源点名称与删除。
            EditorAction::ToolBoundary => {
                editor.tool = EditorTool::Boundary; // 进入区域边界控制点工具，不复用米制圆形笔刷。
                editor.status =
                    "区域边界工具：拖动白色控制点；要重新绘制请先点“重画边界”".to_owned(); // 直接说明闭合边界不会盲目追加点。
            }
            EditorAction::BoundaryRedraw => {
                editor.tool = EditorTool::Boundary; // 按钮操作后保持边界Gizmo可见。
                editor.boundary_redraw_requested = true; // 作者域下一阶段原子清空主路径并登记撤销。
            }
            EditorAction::BoundaryRemoveLastPoint => {
                editor.tool = EditorTool::Boundary; // 删除后继续显示开放主路径。
                editor.boundary_remove_last_requested = true; // 作者域执行完整快照撤销。
            }
            EditorAction::BoundaryClose => {
                editor.tool = EditorTool::Boundary; // 闭合结果立即以青色显示。
                editor.boundary_close_requested = true; // 作者域统一校验至少三点和正式文件契约。
            }
            EditorAction::VegetationPreviewNext => {
                editor.vegetation_preview_mode = editor.vegetation_preview_mode.next(); // 原图、最终和冲突循环切换。
                editor.status = format!(
                    "植被覆盖已切换为“{}”；实际模型始终显示最终编译结果",
                    editor.vegetation_preview_mode.label()
                ); // 避免把诊断覆盖误认为运行时内容。
            }
            EditorAction::VegetationListPreviousPage
            | EditorAction::VegetationListNextPage
            | EditorAction::VegetationAddLayer
            | EditorAction::VegetationDuplicateLayer
            | EditorAction::VegetationDeleteSelected
            | EditorAction::VegetationMoveUp
            | EditorAction::VegetationMoveDown
            | EditorAction::VegetationToggleEnabled
            | EditorAction::VegetationToggleLocked
            | EditorAction::VegetationNewGroup
            | EditorAction::VegetationNextGroup
            | EditorAction::VegetationCycleGroupMode
            | EditorAction::VegetationCycleMergeMode => {} // 独立图层系统持有可变作者草稿并统一登记撤销快照。
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::authoring::TerrainPreviewState;
    use crate::ui::action_availability::dynamic_terrain_save_is_pending;

    #[test]
    fn dynamic_terrain_save_waits_for_mesh_and_height_image_of_the_same_revision() {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.terrain_dirty = true;
        let map_id = draft.map_id.clone();
        let session = EditorAuthoringSession {
            draft: Some(draft),
            terrain_revision: 7,
            ..default()
        };
        let mut preview = TerrainPreviewState::default();
        assert!(dynamic_terrain_save_is_pending(&session, &preview));

        preview.applied_map_id = Some(map_id);
        preview.applied_revision = 7;
        preview.mesh = Some(Handle::default());
        assert!(dynamic_terrain_save_is_pending(&session, &preview));
        preview.height_image = Some(Handle::default());
        assert!(!dynamic_terrain_save_is_pending(&session, &preview));
    }
}
