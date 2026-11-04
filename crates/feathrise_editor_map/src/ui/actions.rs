//! 编辑器工具按钮、快捷键与选中态回显。

use bevy::prelude::*; // ECS、按钮交互与键盘输入。
use bevy::ecs::system::SystemParam;
use bevy::input_focus::InputFocus;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorShortcutAction;
use bevy::ui::InteractionDisabled; // 不支持样条水的地形仍保留按钮并明确禁用。
use bevy::window::{PrimaryWindow, Window}; // 关闭弹框时同步中文IME状态。
use feathrise_map_content::{MapSkyMode, MapVegetationKind, VegetationPresetDescriptor}; // 天空按钮读取草稿模式；植被选择读取开发者目录项并只保存稳定预设身份。
use feathrise_scene::DynamicMeshProfileRequests; // 动态地图Mesh画布选择时按需请求既有正式四级原型。
use crate::ui::theme as colors;
use feathrise_editor_ui::{ActiveNumericInput}; // 字符焦点与地图编辑器鼠标门共同隔离快捷键。

use crate::authoring::{EditorAuthoringSession, WaterNetworkEditState, selected_vegetation_layer}; // 作者层到固定正式profile的唯一映射。
use crate::components::MapEditorActionButton; // 静态与动态作者按钮共享统一视觉回显。
use crate::resources::{
    MapDeletionTask, MapEditorNumericScrubMouseGuard, MapEditorState, MapEditorViewportState,
}; // 快捷键在删除模态或数值拖动和弦期间停用。
use crate::types::{EditorAction, EditorTool, EditorView, NewMapInputField};

use super::brush_controls::{
    brush_radius_bounds, BRUSH_STRENGTH_MAX, BRUSH_STRENGTH_MIN,
}; // 键盘快捷键和中央滑杆必须共享完全相同的正式范围。
use super::surface_preset_editor::SurfacePresetEditorState;
use super::action_availability::{EditorActionAvailability, EditorActionInputs};
use super::vegetation_preset_editor::VegetationPresetEditorState; // 预设名称或数值聚焦时必须独占字符快捷键。 // PBR预设输入与植被表单具有同等键盘所有权。
use super::water_preset_editor::WaterPresetEditorState; // 水体波浪表单同样拥有当前字符输入。
mod buttons; // 静态BSN按钮的一帧结构化动作分派。
#[cfg(test)]
mod shortcut_tests;

pub(crate) use buttons::handle_editor_action_buttons; // 保持UI模块既有系统注册路径不变。

/// 侧栏页签和雕刻按钮复用同一工具请求；真正写入资格仍由视口选择/编辑模式门决定。
pub(crate) fn request_terrain_tool(action: EditorAction, editor: &mut MapEditorState) {
    let previous_volume = editor.tool.is_volume_sculpt();
    editor.tool = match action {
        EditorAction::ToolRaise => EditorTool::Raise,
        EditorAction::ToolLower => EditorTool::Lower,
        EditorAction::ToolSmooth => EditorTool::Smooth,
        EditorAction::ToolFlatten => EditorTool::Flatten,
        EditorAction::ToolVolumeDig => EditorTool::VolumeDig,
        EditorAction::ToolVolumeFill => EditorTool::VolumeFill,
        EditorAction::ToolVolumeSmooth => EditorTool::VolumeSmooth,
        EditorAction::ToolVolumeFlatten => EditorTool::VolumeFlatten,
        EditorAction::ToolSurface => EditorTool::Surface,
        EditorAction::ToolPuddles => {
            editor.status = "积水潜力：左键绘制，Shift擦除；首次落笔创建独立层。屋内、桥下保持零值；切换工具查看实际天气效果".into();
            EditorTool::Puddles // 选择不创建画布，首次真实落笔仍由原积水域受理。
        }
        _ => return,
    };
    editor.domain_brush_active = false; // 地形入口退出外部刷子上下文，参数工具条恢复通用状态。
    if editor.tool.is_volume_sculpt() || previous_volume {
        let (minimum, maximum) = brush_radius_bounds(editor);
        editor.brush_radius = editor.brush_radius.clamp(minimum, maximum);
    }
    if editor.tool.is_volume_sculpt() {
        editor.status = if editor.tool == EditorTool::VolumeFlatten {
            "三维整平：按下锁定首个落点的水平高度，拖动削高补低；方盒适合规则地面，竖直半高控制修整厚度".into()
        } else {
            "三维雕刻：左键挖掘/填充/平滑；可选球形、方盒、圆柱，规则刷支持独立竖直半高".into()
        };
    }
}

/// 名称和备注共用官方文本焦点与中文IME。
pub(super) fn focus_new_map_field(
    editor: &mut MapEditorState,
    field: NewMapInputField,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) {
    editor.new_map_input_field = field;
    set_ime_enabled(windows, true);
}

/// 激活一张现有正式密度画布；动态Mesh层同时发出幂等原型预加载请求。
pub(crate) fn activate_vegetation_layer(
    editor: &mut MapEditorState,
    session: &EditorAuthoringSession,
    index: usize,
    mesh_profile_requests: Option<&mut DynamicMeshProfileRequests>,
) -> bool {
    let Some((kind, profile, dynamic_map)) = session.draft.as_ref().and_then(|draft| {
        let layer = selected_vegetation_layer(draft, index)?; // UI索引始终指独立作者画布。
        Some((
            layer.output.kind,                    // 预览后端仍由固定正式层决定。
            layer.output.runtime_profile.clone(), // 稳定预设 ID 成为后续新建层的默认目标。
            draft.manifest.is_data_driven(),
        ))
    }) else {
        editor.status = "选择的正式植被密度层不存在".to_owned();
        return false;
    };
    editor.selected_vegetation_layer = index;
    editor.vegetation_selected_preset_id = profile.clone(); // 选择作者层时同步预设，而不是保存易冲突的地图输出 ID。
    editor.tool = EditorTool::Vegetation;
    if kind == MapVegetationKind::Mesh && dynamic_map {
        let Some(profile) = profile.as_deref() else {
            editor.status = "当前Mesh密度层没有正式运行时profile".to_owned();
            return false;
        };
        let Some(requests) = mesh_profile_requests else {
            editor.status = "正式Mesh原型加载器未安装，当前密度画布无法预览".to_owned();
            return false;
        };
        match requests.request(profile) {
            Ok(_) => {
                editor.status =
                    "已选择正式Mesh密度画布；四级LOD与风材质正在按需加载，左键绘制".to_owned();
            }
            Err(error) => {
                editor.status = error;
                return false;
            }
        }
    } else {
        editor.status = "已选择正式植被密度画布；左键绘制，Shift+左键擦除".to_owned();
    }
    true // 调用方据此判断能否用图层操作状态覆盖原型加载错误。
}

/// 选择开发者预设；已有作者层时激活首层，未使用过时只保存稳定 ID 等待延迟创建。
pub(crate) fn select_vegetation_preset(
    editor: &mut MapEditorState,
    session: &EditorAuthoringSession,
    preset: &VegetationPresetDescriptor,
    mesh_profile_requests: Option<&mut DynamicMeshProfileRequests>,
) {
    // 【修复】目录选择是全局预设管理状态，不能因为当前地图草稿尚未加载或没有植被文件而拒绝选择。
    editor.vegetation_selected_preset_id = Some(preset.id.clone()); // 先保存规范稳定ID，编辑/删除入口与地图作者数据彻底解耦。
    editor.tool = EditorTool::Vegetation; // 有地图时立即进入笔刷；无地图时也保持用户明确选择，待进入地图后继续使用。
    let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| Some(&draft.map_id) == editor.selected_map_id.as_ref())
    else {
        editor.status = format!(
            "已选择预设“{}”；等待当前地图作者数据，全局植被预设仍可管理",
            preset.label
        ); // 选择成功必须有明确回显，地图缺失只限制落笔而不撤销全局目录选择。
        return; // 没有地图作者数据时不尝试解析正式输出或作者层。
    };
    if let Some(message) = super::vegetation_layers::vegetation_canvas_unavailable(editor, draft) {
        editor.status = format!("已选择预设“{}”；{message}", preset.label);
        return; // 载入前只选择全局预设，绝不创建默认文件或沿用上一网格图层。
    }
    let vegetation = draft.vegetation.as_ref().expect("已核对当前网格植被作者数据");
    let output = vegetation.layers.iter().find(|output| {
        output.runtime_profile.as_deref().is_some_and(|profile| {
            profile == preset.id || preset.aliases.iter().any(|alias| alias == profile)
        })
    }); // 历史别名地图仍映射到当前规范预设，下一次创建只写当前 ID。
    let selected = output.and_then(|output| {
        vegetation.authoring.as_ref().and_then(|authoring| {
            authoring
                .layers
                .iter()
                .position(|layer| layer.output_layer_id == output.id)
        })
    });
    if let Some(index) = selected {
        activate_vegetation_layer(editor, session, index, mesh_profile_requests);
        editor.vegetation_selected_preset_id = Some(preset.id.clone()); // 作者层可能保存历史别名，UI始终回显规范 ID。
    } else {
        editor.status = format!(
            "已选择预设“{}”；点击“新空白层”后才为本地图创建正式输出",
            preset.label
        );
    }
}

/// 当前工具按钮保持高亮；植被目录选择由专用预设按钮样式系统负责。
pub(crate) fn sync_editor_action_button_styles(
    mut commands: Commands,               // 按当前地形能力增删Bevy正式禁用标记。
    editor: Res<MapEditorState>,          // 视图、工具与预览按钮仍读取编辑器交互状态。
    session: Res<EditorAuthoringSession>, // 天空二选一必须直接回显当前地图草稿，而不是一次点击后的临时状态。
    availability: EditorActionInputs,
    viewport: Option<Res<MapEditorViewportState>>,
    mut captions: Query<&mut TextColor>,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &MapEditorActionButton,
            Has<InteractionDisabled>,
            &mut BackgroundColor,
            Option<&Children>,
        ),
        Without<crate::components::MapEditorCustomActionStyle>,
    >, // 地形域自行显示预备工具，避免选择模式下两套系统交替改色。
) {
    let spline_unsupported = session
        .draft
        .as_ref()
        .is_some_and(|draft| !draft.manifest.terrain.kind.supports_spline_water());
    let availability = availability.snapshot(&editor, viewport.as_deref());
    for (entity, interaction, button, disabled, mut background, children) in &mut buttons {
        let managed = EditorActionAvailability::supports(button.0);
        let should_disable = availability.reason(button.0).is_some()
            || (spline_unsupported && super::water_actions::spline_edit_action(button.0, editor.tool));
        if should_disable != disabled {
            if should_disable {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
        if managed && let Some(children) = children {
            let color = if should_disable {
                colors::TEXT_MUTED
            } else {
                colors::TEXT_PRIMARY
            };
            for child in children {
                if let Ok(mut caption) = captions.get_mut(*child) {
                    caption.0 = color; // 禁用仍可命中悬停，但不显示按下/选中态；不改字号、尺寸或层级。
                }
            }
        }
        let active = match button.0 {
            EditorAction::ViewPerspective => editor.view == EditorView::Perspective,
            EditorAction::ViewTop => editor.view == EditorView::Top,
            EditorAction::ViewFront => editor.view == EditorView::Front,
            EditorAction::ViewSide => editor.view == EditorView::Side,
            EditorAction::ToggleOrthographic => editor.orthographic,
            EditorAction::ToolRaise => editor.tool == EditorTool::Raise,
            EditorAction::ToolVolumeDig => editor.tool == EditorTool::VolumeDig,
            EditorAction::ToolVolumeFill => editor.tool == EditorTool::VolumeFill,
            EditorAction::ToolVolumeSmooth => editor.tool == EditorTool::VolumeSmooth,
            EditorAction::ToolVolumeFlatten => editor.tool == EditorTool::VolumeFlatten,
            EditorAction::ToolLower => editor.tool == EditorTool::Lower,
            EditorAction::ToolSmooth => editor.tool == EditorTool::Smooth,
            EditorAction::ToolFlatten => editor.tool == EditorTool::Flatten,
            EditorAction::ToolPlaceAsset => editor.tool == EditorTool::PlaceAsset,
            EditorAction::ToolPortal => editor.tool == EditorTool::Portal,
            EditorAction::ToolSurface => editor.tool == EditorTool::Surface,
            EditorAction::ToolPuddles => editor.tool == EditorTool::Puddles, // 积水按钮使用当前工具状态统一高亮。
            EditorAction::ToolBoundary => editor.tool == EditorTool::Boundary,
            EditorAction::VegetationPreviewNext => {
                editor.vegetation_preview_mode != crate::types::VegetationPreviewMode::Source
            }
            EditorAction::SkyModeDynamic => session.draft.as_ref().is_some_and(|draft| {
                draft.manifest.environment.sky_mode == MapSkyMode::Dynamic // 旧地图反序列化后也会稳定回显兼容默认值。
            }),
            EditorAction::SkyModeSkybox => session.draft.as_ref().is_some_and(|draft| {
                draft.manifest.environment.sky_mode == MapSkyMode::Skybox // 只有已通过作者校验的天空盒草稿才显示选中态。
            }),
            EditorAction::FocusNewMapName => editor.new_map_input_field == NewMapInputField::Name,
            _ => false,
        }; // 水域独立工具按钮移除后，通用按钮样式只需处理仍存在的可选入口。
        let input_field = button.0 == EditorAction::FocusNewMapName; // 仅名称仍使用旧表单按钮；长宽公共控件自行管理视觉。
        background.0 = if should_disable {
            colors::SLOT_BG_DISABLED
        } else if active {
            colors::SELECT_BG
        } else if input_field {
            colors::INPUT_BG
        } else {
            match interaction {
                Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
                Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
                Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
            }
        };
    }
}

/// 原生文本焦点与IME候选的保护集中在一个参数，避免漏掉传送名称等静态文本槽。
#[derive(SystemParam)]
pub(crate) struct EditorShortcutInputContext<'w, 's> {
    tool_labels: Option<Res<'w, crate::game_authoring::DomainToolLabels>>,
    sidebar: Option<Res<'w, super::sidebars::MapEditorSidebarState>>,
    settings: ResMut<'w, EditorSettings>,
    focus: Option<Res<'w, InputFocus>>,
    fields: Query<'w, 's, (), With<super::text_input::EditorTextInput>>,
    native_text: Query<'w, 's, (), With<bevy::text::EditableText>>,
    text_guard: Option<Res<'w, super::text_input::EditorTextInputGuard>>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    mode: Option<ResMut<'w, crate::viewport::modes::ViewportModeState>>,
    availability: EditorActionInputs<'w>,
    viewport: Option<Res<'w, MapEditorViewportState>>,
    shapes: Option<ResMut<'w, crate::authoring::basic_shapes::BasicShapeState>>,
    mesh_edit: Option<Res<'w, crate::authoring::mesh_edit::MeshEditState>>,
}

/// 编辑状态下提供可配置的地图快捷键；文本和其他中央文档拥有各自的输入。
pub(crate) fn handle_editor_shortcuts(
    map_characters: Option<Res<crate::game_authoring::map_interaction::MapInteractionState>>, // 地图角色自由文字独占快捷键。
    keyboard: Res<ButtonInput<KeyCode>>,
    active_numeric_input: Res<ActiveNumericInput>, // 延迟取消的一帧内仍必须阻止负号等按键泄漏给笔刷。
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>, // 拖动和弦release帧仍不得保存、撤销或改笔刷。
    mut editor: ResMut<MapEditorState>,
    deletion: Res<MapDeletionTask>, // 删除二次确认属于模态步骤，不能同时保存、撤销或改笔刷。
    water_edit: Res<WaterNetworkEditState>, // 控制点拖动途中不得并发保存、撤销或改通用参数。
    preset_editor: Res<VegetationPresetEditorState>, // 全局预设字段输入期间屏蔽方括号、负号和Ctrl快捷键泄漏。
    surface_preset_editor: Res<SurfacePresetEditorState>, // 地表名称或PBR数值输入同样独占字符键。
    water_preset_editor: Res<WaterPresetEditorState>, // 水体名称、波浪与法线数值输入期间屏蔽编辑器快捷键。
    building_preset_editor: Res<super::building_presets::BuildingPresetEditorState>, // 建筑名称与空间数值输入不能泄漏到底层地图工具。
    documents: Option<Res<super::asset_preview::DocumentState>>, // 文档预览/编辑页的Ctrl+S只能保存当前文档，不能保存隐藏地图。
    mut input: EditorShortcutInputContext,
) {
    if !editor.open
        || editor.close_requested
        || editor.map_transition_in_progress
        || editor.pending_map_transition.is_some()
        || editor.map_switch_confirmation.is_some()
        || input.focus.as_ref().and_then(|focus| focus.get()).is_some_and(|entity| input.fields.contains(entity))
        || input.focus.as_ref().and_then(|focus| focus.get()).is_some_and(|entity| input.native_text.contains(entity))
        || input.text_guard.as_ref().is_some_and(|guard| guard.block_shortcuts)
        || map_characters.as_ref().is_some_and(|state|state.text_input_active)
        || editor.new_map_dialog_open
        || active_numeric_input.entity.is_some()
        || numeric_scrub_mouse.blocked()
        || editor.water_level_input_target.is_some()
        || deletion.busy()
        || water_edit.dragging()
        || preset_editor.text_input_active()
        || surface_preset_editor.text_input_active()
        || water_preset_editor.text_input_active()
        || building_preset_editor.text_input_active()
    // 三类独立预设的Ctrl+S不能保存隐藏地图。
    {
        return;
    } // 文本输入、确认、切图等待和回收任务期间都不登记迟到的保存或编辑快捷键。
    let (radius_min, radius_max) = brush_radius_bounds(&editor);
    for action in EditorShortcutAction::ALL {
        if !input.settings.editor_keybindings.pressed(action, &keyboard) { continue; }
        // 右侧原生工具拥有自己的保存/历史，中央仍为地图时也不能同时撤销地图。
        if matches!(action, EditorShortcutAction::Save | EditorShortcutAction::Undo | EditorShortcutAction::Redo)
            && input.sidebar.as_ref().zip(input.tool_labels.as_ref()).is_some_and(|(sidebar, labels)| {
                matches!(sidebar.left, super::sidebars::EditorLeftSidebarTab::Domain(id)
                    if labels.0.contains_key(id) && sidebar.tool_page_visible(sidebar.left))
            }) { continue; }
        if input.mesh_edit.as_ref().is_some_and(|state| state.active())
            && !matches!(action, EditorShortcutAction::Save | EditorShortcutAction::Undo | EditorShortcutAction::Redo)
        { continue; } // 元素快捷键由网格编辑域消费；已确认几何仍可沿原事务保存或撤销，完整对象工具不得串线。
        if action != EditorShortcutAction::AddObjectMenu
            && (preset_editor.active_tab != super::vegetation_preset_editor::PresetWorkspaceTab::Map
                || documents.as_ref().is_some_and(|state| state.owns_shortcuts())) {
            continue; // 添加网格是跨中央标签的工具入口，其他快捷键仍归当前文档所有。
        }
        if let Some(command) = EditorActionAvailability::shortcut_action(action)
            && input.availability
                .snapshot(&editor, input.viewport.as_deref())
                .reason(command)
                .is_some()
        {
            continue; // 与鼠标入口和悬停说明共用实时资格；不影响文字、拖动等原有输入所有权门。
        }
        if matches!(action, EditorShortcutAction::FocusSelected | EditorShortcutAction::FrameMap | EditorShortcutAction::MoveSelected | EditorShortcutAction::RotateSelected | EditorShortcutAction::ScaleSelected | EditorShortcutAction::ToggleSnapping | EditorShortcutAction::AddObjectMenu)
            && input.mouse.as_ref().is_some_and(|mouse| mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])) {
            continue; // 拖动时取景会跳变落点；笔刷快捷调节仍沿用原有行为。
        }
        match action {
            EditorShortcutAction::AddObjectMenu => {
                if let Some(shapes) = input.shapes.as_deref_mut() { shapes.request_menu = true; }
            }
            EditorShortcutAction::FocusSelected => editor.viewport_framing_requested = Some(crate::types::EditorViewportFraming::Selected),
            EditorShortcutAction::FrameMap => editor.viewport_framing_requested = Some(crate::types::EditorViewportFraming::Map),
            EditorShortcutAction::MoveSelected => crate::viewport::placement_move::toggle_placement_move(&mut editor, input.mode.as_deref_mut()),
            EditorShortcutAction::RotateSelected => crate::viewport::placement_move::toggle_placement_mode(&mut editor, input.mode.as_deref_mut(), crate::viewport::modes::ViewportMode::Rotate),
            EditorShortcutAction::ScaleSelected => crate::viewport::placement_move::toggle_placement_mode(&mut editor, input.mode.as_deref_mut(), crate::viewport::modes::ViewportMode::Scale),
            EditorShortcutAction::ToggleSnapping => {
                input.settings.editor_snapping.enabled = !input.settings.editor_snapping.enabled;
                editor.status = if input.settings.editor_snapping.enabled { "吸附常开；按住Ctrl临时关闭" } else { "吸附已关闭；按住Ctrl临时启用" }.into();
            }
            EditorShortcutAction::Save => editor.save_requested = true,
            EditorShortcutAction::Undo => editor.undo_requested = true,
            EditorShortcutAction::Redo => editor.redo_requested = true,
            EditorShortcutAction::DecreaseRadius => editor.brush_radius =
                (editor.brush_radius / 1.25).clamp(radius_min, radius_max),
            EditorShortcutAction::IncreaseRadius => editor.brush_radius =
                (editor.brush_radius * 1.25).clamp(radius_min, radius_max),
            EditorShortcutAction::DecreaseStrength => editor.brush_strength =
                (editor.brush_strength - 0.05).clamp(BRUSH_STRENGTH_MIN, BRUSH_STRENGTH_MAX),
            EditorShortcutAction::IncreaseStrength => editor.brush_strength =
                (editor.brush_strength + 0.05).clamp(BRUSH_STRENGTH_MIN, BRUSH_STRENGTH_MAX),
        } // 快捷键和原按钮登记同一种一帧请求，导航不进入作者事务。
    }
}

pub(crate) fn set_ime_enabled(
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    enabled: bool,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    // 候选位置由官方 EditableText 布局按文字光标更新，不能用鼠标位置覆盖。
    if window.ime_enabled != enabled {
        window.ime_enabled = enabled; // 最终仲裁每帧运行，但稳定焦点不反复标记Window变化。
    }
}

#[cfg(test)]
mod tests {
    use feathrise_map_content::VegetationPresetCatalogAsset; // 使用随包真实普通草描述覆盖完整数据Blade结构。

    use super::*; // 直接验证生产选择入口，不复制弱化版目录状态逻辑。

    #[test]
    fn global_vegetation_preset_selection_does_not_require_a_loaded_map() {
        let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"), // 测试从当前map_editor crate定位工作区资产根。
            "/tests/fixtures/vegetation_presets.vegetation_presets.json" // 随包目录第一项就是用户报告的普通草。
        )))
        .expect("随包植被预设目录必须可解析"); // 真实目录损坏应由本测试直接暴露。
        let preset = catalog.presets.first().expect("随包目录必须保留普通草预设"); // 不用伪造不完整Blade描述。
        let session = EditorAuthoringSession::default(); // 明确模拟草稿尚未加载或当前地图没有植被文件。
        let mut editor = MapEditorState::default(); // 初始全局预设选择为空。

        select_vegetation_preset(&mut editor, &session, preset, None); // 点击仍必须先采用稳定预设ID。

        assert_eq!(
            editor.vegetation_selected_preset_id.as_deref(), // 读取管理选择而不是作者层回退值。
            Some("hn01_ordinary_grass"), // 第一项零索引和无地图都不能再被当成未选择。
        ); // 编辑与删除入口现在能取得同一个规范ID。
        assert_eq!(editor.tool, EditorTool::Vegetation); // 进入地图后可以直接延续植被作者工具。
        assert!(editor.status.contains("已选择预设“普通草”")); // UI必须向用户确认点击已经生效。
        assert!(editor.status.contains("全局植被预设仍可管理"));
        assert!(!editor.status.contains("进入含植被作者文件的地图"));
        assert!(session.draft.is_none() && session.undo.is_empty()); // 全局选择不能替用户创建地图文件。
    }
}
