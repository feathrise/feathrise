//! F12显隐、静态按钮、中文名称输入和BSN回显系统。

use bevy::input::keyboard::{Key, KeyboardInput}; // 读取Unicode文本、Enter和退格。
use bevy::input::mouse::{MouseScrollUnit, MouseWheel}; // 低高度窗口左右生产栏按悬停分别使用同一滚轮入口。
use bevy::prelude::*; // ECS、UI、键盘与状态。
use bevy::ui::{FocusPolicy, RelativeCursorPosition}; // 左右栏只在鼠标悬停范围内接管滚轮。
use bevy::window::{PrimaryWindow, Window}; // 中文IME预编辑、提交和候选框定位。
use feathrise_scene_core::input::{EditorInputState, InputGateState}; // 打开门和统一玩法输入阻塞。
use feathrise_map_content::{ActiveDynamicMap, MapCatalogAsset, MapContentRegistry, MapManifestAsset}; // 标题显示当前编辑地图并从当前场景取得默认选择。
use feathrise_scene_core::state::{EditorStartup, ScenePhase, LoadingProgress}; // 正式进度资源消失后才允许收口编辑器切图。

use crate::authoring::{WaterNetworkEditState, request_map_creation}; // 新建地图事务与水网拖动关闭互斥门。
use crate::components::*; // 根、文本、弹框和动作按钮marker。
use crate::map_identity::active_gameplay_map_id; // 目录高亮和当前作者地图初始化使用同一真实身份。
use crate::resources::{
    EditorAssetCatalog, MapBrowserState, MapCreationTask, MapDeletionTask,
    MapEditorNumericScrubMouseGuard, MapEditorState, MapEditorWorldSession, MapFolderPersistence,
}; // 新建、删除、鼠标所有权与编辑器会话共享明确资源边界。
use crate::types::NewMapInputField; // 名称、X宽度、Z长度与地形格距共享一个明确键盘焦点。
use crate::viewport::{
    map_editor_brush_cursor_visible, map_editor_brush_display_hit, map_editor_brush_follows_cursor,
    spatial_brush::SpatialBrushState,
}; // Gizmo与鼠标显隐共用跟随判定，固定空间轮廓不能占用指针。

use super::actions::{focus_new_map_field, set_ime_enabled}; // F12关闭、字段切换与中文名称共用同一IME入口。
use super::vegetation_preset_editor::PresetNotesScroll; // 多行备注先在自身有限视区内滚动，到边界才交给正文。
use super::viewport_toolbar::ViewportToolbarScroll; // 同排工具条复用标签条横滚与原鼠标所有权门。

const EDITOR_SCROLL_LINE_HEIGHT: f32 = 27.0; // 与24px紧凑工具按钮及行距一致。

fn editor_scroll_delta(unit: MouseScrollUnit, wheel_y: f32) -> f32 {
    -wheel_y
        * match unit {
            MouseScrollUnit::Line => EDITOR_SCROLL_LINE_HEIGHT,
            MouseScrollUnit::Pixel => 1.0,
        }
}

/// 低高度窗口只滚动鼠标所在的左右生产栏；中央视口滚轮仍独立负责相机缩放。
pub(crate) fn scroll_editor_side_panels(
    mut wheels: MessageReader<MouseWheel>,
    editor_context: (Res<MapEditorState>, Option<ResMut<EditorAssetCatalog>>),
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>, // 数值拖动和弦期间滚轮必须消费但不能滚面板。
    map_wheel: super::management_dialogs::ManagementWheelGuard<'_>, // 原地图拖拽/release门加管理模态所有权；保持16个顶层参数以内。
    asset_preview: (Option<Res<super::asset_preview::AssetPreviewNavigation>>, Option<Res<super::asset_preview::AudioPreviewState>>, Option<Res<super::asset_preview::AnimationEditorState>>), // 音频滑杆、动画时间轴和画布手势不能同时滚外栏。
    documents: Option<Res<super::asset_preview::DocumentState>>, // 文档正文已消费的滚轮不再移动资产外栏。
    minimap: Option<Res<super::asset_preview::DocumentMinimapState>>,
    asset_search: Option<Res<super::asset_preview::AssetSearchState>>, // 搜索虚拟结果视区接管后的滚轮不再滚动外层资产页。
    asset_replace: Option<Res<super::asset_preview::AssetReplaceState>>,
    asset_batch_save: Option<Res<super::asset_preview::DocumentBatchSaveState>>,
    water_preview: Option<Res<super::water_preset_editor::WaterPresetLivePreviewNavigation>>, // 水体样品画布悬停时滚轮只改变独立相机距离。
    character_lists: Query<
        &RelativeCursorPosition,
        With<crate::game_authoring::map_interaction::DomainScrollViewport>,
    >, // 角色虚拟列表独占其悬停滚轮。
    mut map_tree_scrolls: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        (Or<(With<MapEditorMapTreeScroll>, With<super::asset_shortcuts::ShortcutList>, With<super::list_layout::SceneObjectListScroll>)>, Without<PresetNotesScroll>), // 目录树与有界收藏列表共同使用局部滚动协议。
    >, // 地图树优先接管其独立滚动视口。
    mut horizontal_scrolls: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        (
            Or<(
                With<MapEditorPresetTabScroll>,
                With<ViewportToolbarScroll>,
                With<crate::game_authoring::map_interaction::DomainHorizontalScroll>,
            )>,
            Without<MapEditorMapTreeScroll>,
            Without<super::list_layout::SceneObjectListScroll>, // 场景对象列表先滚动内部可见窗口。
            Without<super::asset_shortcuts::ShortcutList>,
            Without<MapEditorPresetWorkspaceScroll>,
            Without<PresetNotesScroll>, // 备注Y轴与标签X轴不能借用同一滚动组件。
        ),
    >, // 文档标签与固定工具条悬停时纵向滚轮转换为横滚，不增加控件高度。
    mut notes_scrolls: Query<
        (
            &RelativeCursorPosition,
            &ComputedNode,
            &bevy::text::TextLayoutInfo,
            &mut bevy::ui::widget::TextScroll,
        ), // 滚动官方文字视区，而非已固定高度的父容器。
        With<PresetNotesScroll>, // 所有其他滚动查询均明确排除此标记，防止B0001。
    >, // 不让长备注撑高整个表单，也不把一次滚轮同时交给两个视区。
    mut panels: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition,
            Option<&super::project_browser::ProjectBrowserContent>, Option<&super::sidebars::MapEditorSidebarPage>),
        (
            Or<(
                With<MapEditorLeftPanelScroll>, // 左栏地图与作者工具保持既有滚动行为。
                With<MapEditorRightPanelScroll>, // 右栏场景树、资产与环境使用同一悬停滚轮规则。
                With<MapEditorPresetWorkspaceScroll>, // 中央预设详细页独立滚动，不把滚轮传给隐藏地图视口。
            )>,
            Without<MapEditorMapTreeScroll>, // 与独立树查询显式互斥，避免可变ScrollPosition冲突。
            Without<super::list_layout::SceneObjectListScroll>, // 场景对象列表先滚动内部可见窗口。
            Without<super::asset_shortcuts::ShortcutList>, // 收藏/最近访问内部滚动不重复推动整页。
            Without<MapEditorPresetTabScroll>, // 标签条使用X轴，不能与正文Y轴查询潜在重叠。
            Without<ViewportToolbarScroll>,  // 工具条同样使用X轴，与三类正文显式互斥。
            Without<crate::game_authoring::map_interaction::DomainHorizontalScroll>,      // 状态行独占X轴，不能同时推动所属中央表单。
            Without<PresetNotesScroll>,      // 备注内容先局部滚动，外层只负责整个预设页面。
        ),
    >,
) {
    let (editor, mut catalog) = editor_context;
    let delta = wheels
        .read()
        .map(|event| editor_scroll_delta(event.unit, event.y))
        .sum::<f32>();
    if numeric_scrub_mouse.blocked()
        || documents.as_ref().is_some_and(|state| state.owns_wheel())
        || minimap.as_ref().is_some_and(|state| state.owns_wheel())
        || asset_search.as_ref().is_some_and(|state| state.owns_wheel())
        || asset_replace.as_ref().is_some_and(|state| state.owns_wheel())
        || asset_batch_save.as_ref().is_some_and(|state| state.owns_wheel())
        || map_wheel.blocked()
        || asset_preview.1.as_ref().is_some_and(|audio| audio.owns_wheel())
        || asset_preview.2.as_ref().is_some_and(|animation| animation.owns_wheel())
        || asset_preview.0
            .as_ref()
            .is_some_and(|preview| preview.owns_wheel())
        || water_preview
            .as_ref()
            .is_some_and(|preview| preview.owns_wheel())
    {
        // 当前帧已交给预览的滚轮只消费一次。
        return; // MessageReader已显式读空，解锁后不会补滚数值拖动期间积累的事件。
    }
    if !editor.open || delta == 0.0 {
        return;
    }
    if matches!(editor.tool, crate::types::EditorTool::Domain(_))
        && character_lists.iter().any(|cursor| cursor.cursor_over())
    {
        return;
    }
    for (cursor, computed, mut scroll) in &mut horizontal_scrolls {
        if !cursor.cursor_over() {
            continue;
        }
        let maximum = ((computed.content_size().x - computed.size().x)
            * computed.inverse_scale_factor())
        .max(0.0);
        scroll.x = (scroll.x + delta).clamp(0.0, maximum);
        return; // 标签或工具条命中后不把同一滚轮继续传给中央正文或侧栏。
    }
    for (cursor, computed, mut scroll) in &mut map_tree_scrolls {
        if !cursor.cursor_over() {
            continue; // 根目录行和树外操作区不应滚动内部树。
        }
        let maximum = ((computed.content_size().y - computed.size().y)
            * computed.inverse_scale_factor())
        .max(0.0); // 展开内容没有超过视口时保持零滚动范围。
        let next = (scroll.y + delta).clamp(0.0, maximum); // 树滚动使用与两侧面板一致的逻辑像素手感。
        if next != scroll.y {
            scroll.y = next; // 只有树真实还能滚动时才消费本批滚轮。
            return; // 同一事件批次不能继续推动外层地图页。
        }
        break; // 唯一树已悬停但到达边界，允许外层低高度页面继续滚动。
    }
    for (cursor, computed, layout, mut scroll) in &mut notes_scrolls {
        if !cursor.cursor_over() {
            continue; // 鼠标在备注之外仍保持原有正文及侧栏滚动。
        }
        let maximum = (layout.size.y - computed.content_box().size().y).max(0.0); // 官方排版和TextScroll均使用物理像素。
        let physical_delta = delta / computed.inverse_scale_factor().max(f32::EPSILON); // 逻辑滚轮只转换一次UI缩放。
        let next = (scroll.0.y + physical_delta).clamp(0.0, maximum); // 全文仍保留在官方文本编辑器中。
        if next != scroll.0.y {
            scroll.0.y = next; // 用户滚动保持在TextScroll，不改变真实光标和选区。
            return; // 一次滚轮不同时移动备注与中央工作区。
        }
        break; // 已到首尾时继续允许用户滚动外层页面。
    }
    for (cursor, computed, mut scroll, content, page) in &mut panels {
        if !cursor.cursor_over() {
            continue; // 未悬停侧栏不消费本次位移，另一侧栏仍可成为唯一目标。
        }
        let maximum = ((computed.content_size().y - computed.size().y)
            * computed.inverse_scale_factor())
        .max(0.0); // 内容未超过当前侧栏时保持零滚动范围。
        let next = (scroll.y + delta).clamp(0.0, maximum);
        if next != scroll.y {
            scroll.y = next;
            let assets = content.is_some_and(|content| content.0 == super::project_browser::ProjectBrowserView::Assets)
                || content.is_none() && page.is_some_and(|page| page.0 == super::sidebars::EditorSidebarTab::Navigator(super::sidebars::EditorNavigatorTab::Assets));
            if assets && let Some(catalog) = catalog.as_mut() {
                catalog.scroll_intent_revision = catalog.scroll_intent_revision.wrapping_add(1);
            } // 用户滚动才撤销等待中的定位；实际布局或自动锚点调整不冒充用户意图。
        }
        break; // 左右栏屏幕区域互斥，同一事件批次不得同时滚动两个面板。
    }
}

/// F12切换全屏工作台；空世界关闭后保留空场景，切图期间等待完整就绪再关闭。
pub(crate) fn toggle_map_editor(
    keyboard: Res<ButtonInput<KeyCode>>,
    active_numeric_input: Res<feathrise_editor_ui::ActiveNumericInput>, // 中键字符编辑必须先明确提交或取消，不能留下隐藏焦点。
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>, // 拖动和弦release帧结束前F12仍不得穿透。
    water_edit: Res<WaterNetworkEditState>, // 水网控制点拖动也必须先等左键松开形成唯一撤销。
    scene: Res<State<ScenePhase>>,
    gate: Res<InputGateState>,
    active_dynamic: Res<ActiveDynamicMap>,
    editor_startup: Option<Res<EditorStartup>>,
    loading: Option<Res<LoadingProgress>>,
    world: MapEditorWorldSession,
    mut editor: ResMut<MapEditorState>,
    documents: Option<Res<super::map_documents::MapDocuments>>,
    recovery: Option<Res<crate::resources::MapLoadingRecovery>>,
    mut standalone_play: crate::standalone::StandalonePlayRequest,
) {
    if editor_startup.is_some()
        && *scene.get() == ScenePhase::Idle
        && !editor.open
        && !editor.empty_workspace_closed
        && !standalone_play.embedded_playing()
    {
        // 启动标记只决定无菜单窗口模式；手动关闭意图优先，不能在下一帧抢回工作台。
        editor.open = true;
        editor.close_requested = false;
        editor.selected_map_id = None;
        editor.map_list_selection_id = None;
        editor.status = "编辑器模式：未进入地图；可直接编辑全局预设，或从左侧进入地图".to_owned();
    }
    let preparing = documents
        .as_ref()
        .is_some_and(|docs| docs.preparing_target.is_some())
        || recovery.as_ref().is_some_and(|state| state.request.is_some()); // 恢复须等场景退出/回收，不能被迟到Ready解锁。
    if editor.map_transition_in_progress && !preparing && world.ready() {
        let active_map_id = active_gameplay_map_id(*scene.get(), &active_dynamic); // LoadingScene完成后以真实玩法身份收口。
        editor.map_transition_in_progress = false;
        editor.open = true;
        // 交接期间用户仍可请求关闭；完整进图只解除切图锁，不丢弃待执行的关闭意图。
        editor.selected_map_id = active_map_id.clone(); // 当前作者草稿跟随已真正进入的地图。
        editor.map_list_selection_id = active_map_id; // 恢复时默认高亮刚进入的目录项。
        editor.status = "地图切换完成，可以继续编辑".to_owned();
    } else if editor.map_transition_in_progress
        && !preparing
        && *scene.get() == ScenePhase::Idle
        && loading.is_none()
    {
        // 缺失地图回退到空世界时仍保留工作台，不能留下永远无法完成的切图锁。
        editor.map_transition_in_progress = false;
        editor.selected_map_id = None;
        editor.map_list_selection_id = None;
        editor.status = "目标地图不可用，已返回空工作区；请重新选择地图".to_owned();
    }
    if !keyboard.just_pressed(KeyCode::F12) || !standalone_play.legacy_shortcut_allowed() {
        return;
    }
    if active_numeric_input.entity.is_some()
        || numeric_scrub_mouse.blocked()
        || water_edit.dragging()
    {
        return; // 字符输入和左右拖动都必须先明确收口，F12不能隐藏仍持有输入身份的控件。
    }
    if editor.open || standalone_play.embedded_playing() {
        if standalone_play.request(&mut editor) { return; } // 独立模式保留完整作者World，只请求外部运行已保存工程。
        editor.toggle_close_request(); // 首次请求安全关闭；等待期间再次按F12可取消并同帧恢复笔刷。
        return;
    }
    // 死亡和ESC同帧时可能留下菜单模式；死亡已锁住ESC，F12仍须提供返回作者工作台的入口。
    if !(world.idle_menu() || (world.ready() && gate.scene_focus_available))
        || !gate.editor_entry_allowed // 死亡仍封锁玩法相机，但不能挡住F12返回作者工作台。
    {
        return;
    }
    editor.open = true;
    editor.empty_workspace_closed = false; // 只有新的手动F12入口才解除空工作台关闭状态。
    editor.close_requested = false; // 每次主动打开都从无关闭请求的稳定状态开始。
    let active_map_id = active_gameplay_map_id(*scene.get(), &active_dynamic); // F12关闭期间玩家可能通过传送到另一张图。
    editor.selected_map_id = active_map_id.clone(); // 手工打开时总是重新对齐当前玩法世界。
    editor.map_list_selection_id = active_map_id; // 初始目录高亮与当前图一致，随后单击可独立移动。
    editor.status = "地图编辑器已打开；选择地图或新建白模地图".to_owned();
}

/// 地图模态只处理名称/备注导航；文字、选区和IME仍由官方文本组件处理。
pub(crate) fn handle_new_map_form_input(
    mut keyboard_inputs: MessageReader<KeyboardInput>,
    guard: Res<crate::ui::text_input::EditorTextInputGuard>,
    official_inputs: Query<&bevy::text::EditableText, With<crate::ui::text_input::EditorTextInput>>,
    mut editor: ResMut<MapEditorState>,
    mut creation: ResMut<MapCreationTask>,
    deletion: Res<MapDeletionTask>,
    map_browser: Res<MapBrowserState>,
    folder_persistence: Res<MapFolderPersistence>,
    numeric: Res<feathrise_editor_ui::ActiveNumericInput>,
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    documents: Option<Res<super::map_documents::MapDocuments>>,
) {
    if !editor.open || !editor.new_map_dialog_open || numeric_scrub_mouse.blocked() || numeric.entity.is_some() {
        keyboard_inputs.clear();
        return;
    }
    let ime_owns_keyboard = guard.block_shortcuts || official_inputs.iter().any(|text| text.is_composing());
    let mut confirm = false;
    for input in keyboard_inputs.read() {
        if !input.state.is_pressed() || ime_owns_keyboard { continue; }
        match input.logical_key {
            Key::Enter if editor.new_map_input_field == NewMapInputField::Name => {
                // 名称Enter仅确认创建；已有地图资料仍由保存按钮提交，备注Enter保留换行。
                confirm = editor.new_map_edit_target.is_none();
            }
            Key::Tab => {
                let next = editor.new_map_input_field.next();
                focus_new_map_field(&mut editor, next, &mut windows);
            }
            Key::Escape => {
                editor.new_map_dialog_open = false;
                editor.new_map_edit_target = None;
                if creation.cancel_pending_after_save() { editor.save_requested = false; }
                set_ime_enabled(&mut windows, false);
            }
            _ => {}
        }
    }
    if confirm {
        if deletion.busy() {
            editor.status = "地图回收事务完成前不能新建地图".to_owned();
        } else {
            request_map_creation(&mut editor, &mut creation, &map_browser, &folder_persistence, documents.is_some());
        }
    }
}

/// 将编辑器显隐写入公共输入门，供统一暂停阶段在当前帧应用。
pub(crate) fn sync_map_editor_blocker(
    editor: Res<MapEditorState>,
    standalone: Option<Res<crate::standalone::StandaloneEditorMode>>,
    mut blocks: ResMut<EditorInputState>,
) {
    // 子游戏取得玩法输入，作者World仍冻结；隐藏工作台不能放开背后的本地玩法。
    let blocked = editor.open || standalone.as_ref().is_some_and(|mode| mode.embedded_playing());
    if blocks.map_editor_open != blocked {
        blocks.map_editor_open = blocked;
    }
    if !editor.open && blocks.map_editor_custom_cursor_visible {
        blocks.map_editor_custom_cursor_visible = false; // 关闭帧先防御性清理旧所有权，后续精确发布仍保持false。
    }
}

/// 在本帧最终视口命中完成后发布世界笔刷鼠标所有权。
pub(crate) fn sync_map_editor_custom_cursor(
    editor: Res<MapEditorState>,
    ground_cursor: Option<Res<crate::viewport::vegetation_ground::VegetationGroundCursor>>,
    objects_cursor: Option<Res<crate::authoring::terrain_ground::TerrainGroundCursor>>,
    spatial: Option<Res<SpatialBrushState>>,
    deletion: Res<MapDeletionTask>,
    map_browser: Res<MapBrowserState>, // 文件夹模态需要系统鼠标和中文IME候选定位。
    map_drag: Res<crate::resources::MapBrowserDragState>, // 地图拖拽浮层必须跟随可见系统鼠标。
    asset_preview: Option<Res<super::asset_preview::AssetPreviewNavigation>>, // 预览拖出到地图上仍保持可见系统鼠标。
    water_preview: Option<Res<super::water_preset_editor::WaterPresetLivePreviewNavigation>>, // 水体样品拖出画布后也不能被地图笔刷抢走系统鼠标。
    viewport: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    gate: Res<InputGateState>,
    mut blocks: ResMut<EditorInputState>,
) {
    let viewport_hovered = viewport.single().is_ok_and(|cursor| cursor.cursor_over()); // 侧栏和弹框区域必须继续使用系统鼠标。
    let display = map_editor_brush_display_hit(&editor, objects_cursor.as_deref());
    // 点击定位后在同帧归还指针；后续真实地形命中也不能把固定轮廓误认作鼠标。
    let custom_cursor_visible = map_editor_brush_follows_cursor(&editor, spatial.as_deref())
        && map_editor_brush_cursor_visible(
            &editor,
            viewport_hovered,
            deletion.confirmation.is_some()
                || map_browser.dialog.is_some()
                || map_drag.active
                || asset_preview
                    .as_ref()
                    .is_some_and(|preview| preview.dragging())
                || water_preview
                    .as_ref()
                    .is_some_and(|preview| preview.dragging()), // 不让地图笔刷抢走任一独立预览的拖动光标。
            gate.system_cursor_required,
            ground_cursor
                .as_deref()
                .is_some_and(|cursor| cursor.has_volume_cursor())
                || display.is_some_and(|(_, volume)| volume),
            display.map(|(point, _)| point),
        );
    if blocks.map_editor_custom_cursor_visible != custom_cursor_visible {
        blocks.map_editor_custom_cursor_visible = custom_cursor_visible; // Apply阶段只让实际跟随光标替代系统鼠标。
    }
}

/// 同步根节点、弹框、标题、目录和底部状态，不逐帧生成UI实体。
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy查询显式列出互斥文本marker，避免按名称查找或多组可变Text查询冲突。
pub(crate) fn sync_editor_shell(
    editor: Res<MapEditorState>,
    catalog: Res<EditorAssetCatalog>,
    browser: Option<Res<crate::ui::project_browser::ProjectBrowserState>>,
    registry: Option<Res<MapContentRegistry>>,
    catalogs: Res<Assets<MapCatalogAsset>>,
    manifests: Res<Assets<MapManifestAsset>>,
    mut roots: Query<(&mut Node, &mut Visibility), With<MapEditorRoot>>,
    mut dialogs: Query<
        (&mut Node, &mut Interaction, &mut bevy::ui::FocusPolicy),
        (With<NewMapDialog>, Without<MapEditorRoot>),
    >,
    mut texts: Query<(
        &mut Text,
        Option<&MapEditorCurrentMapText>,
        Option<&MapEditorAssetFolderText>,
    )>,
) {
    if !(editor.is_changed()
        || catalog.is_changed()
        || browser.as_ref().is_some_and(|browser| browser.is_changed())
        || registry.as_ref().is_some_and(|value| value.is_changed()))
    {
        return;
    }
    for (mut node, mut visibility) in &mut roots {
        node.display = if editor.open {
            Display::Flex
        } else {
            Display::None
        };
        *visibility = if editor.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (mut node, mut interaction, mut focus_policy) in &mut dialogs {
        let visible = editor.open && editor.new_map_dialog_open;
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        // 与删除确认框同一命中策略：可见遮罩阻止点透，隐藏当帧释放底层按钮。
        *focus_policy = if visible {
            FocusPolicy::Block
        } else {
            FocusPolicy::Pass
        };
        if !visible {
            *interaction = Interaction::None;
        }
    }
    // 壳体回显不能写入窗口 IME：F12 关闭时该系统仍会因目录变化运行，曾在主菜单姓名框聚焦后写回 false。
    // 编辑中的开关交给 sync_map_editor_ime；F12 关闭和切图由已有 close/map_transition 入口释放。
    let folder = if let Some(browser) = browser.as_ref() {
        browser.asset_folder_label(&catalog.current_folder) // 同一工程树显示真实登记根，不把嵌套资产目录硬写成assets/。
    } else if catalog.current_folder.is_empty() {
        "assets/".to_owned()
    } else {
        format!("assets/{}/", catalog.current_folder)
    };
    let title = editor.selected_map_id.as_ref().map_or_else(
        || "当前地图：未进入地图".to_owned(),
        |map_id| {
            let display_name = registry
                .as_deref()
                .and_then(|registry| registry.handles(map_id))
                .and_then(|handles| manifests.get(&handles.manifest))
                .map(|manifest| manifest.display_name.as_str())
                .or_else(|| {
                    registry
                        .as_deref()
                        .and_then(|registry| catalogs.get(&registry.catalog))
                        .and_then(|catalog| catalog.maps.iter().find(|entry| &entry.id == map_id))
                        .map(|entry| entry.display_name.as_str())
                })
                .unwrap_or("加载中");
            format!("当前地图：{display_name} [{map_id}]")
        },
    );
    // 所有动态文本使用一个可变查询，既避免运行时Query冲突，也不会逐帧重建UI实体。
    for (mut text, current_map, folder_text) in &mut texts {
        let value = if current_map.is_some() {
            &title
        } else if folder_text.is_some() {
            &folder
        } else {
            continue;
        };
        if text.as_str() != value {
            **text = value.clone(); // 长目录路径不因其他状态变化重复标记布局Changed。
        }
    }
}

#[cfg(test)]
#[path = "systems_tests.rs"]
mod tests;
