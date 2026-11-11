//! 左侧场景树的场景显示预览；折叠状态与场景网格选择独立，不修改作者源或保存配置。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN按钮、交互与会话资源。
use bevy::text::FontSourceTemplate; // 与编辑器其他静态按钮使用相同中文字体。
use feathrise_scene_content::ActiveDynamicScene; // 关闭F12后仍按真正运行的场景恢复着色。
use feathrise_scene::{TerrainGridPreview, TerrainShadingPreview}; // 场景渲染读取的两个独立临时显示状态。
use feathrise_scene_core::state::ScenePhase; // 菜单和加载页不得继承上一个场景的平直着色。
use crate::ui::theme as colors; // 复用编辑器铜边与选中态配色。

use crate::authoring::EditorAuthoringSession; // 场景页只允许选择已载入的当前基础地形。
use crate::components::{TerrainGridButton, TerrainShadingButton, TerrainCollisionButton}; // 三角线、着色与实际碰撞显示使用独立输入身份。
use crate::scene_identity::active_gameplay_scene_id; // 玩法场景身份与F12场景目录保持一致。
use crate::resources::SceneEditorNumericScrubMouseGuard;
use crate::resources::{SceneEditorState, TerrainShadingSession}; // 切换场景恢复场景选择，关闭仍保留玩法模式。

use super::gltf_browser::SceneBaseGltfRow; // 点击右侧场景树的基础地形才展开网格设置。
use super::scene::FONT_PATH; // 保持工具栏中文字体一致。
use super::sidebars::SceneEditorSidebarState; // 场景显示开关跟随左侧场景树，不受右侧属性页影响。

/// 构造只控制当前地形预览的互斥模式按钮。
fn shading_button(label: &str, flat: bool) -> impl Scene {
    bsn! {
        InteractionButton
        TerrainShadingButton(flat) // 仅切换材质的受光法线，不进入场景撤销栈。
        Node {
            width: px(58), // 两档总宽度在最窄中央工作区内仍可完整显示。
            height: px(22), // 与现有视图按钮保持紧凑行高。
            border: px(1), // 铜边表示独立的预览选项。
            border_radius: BorderRadius::all(px(3)), // 与编辑器静态按钮相同。
            align_items: AlignItems::Center, // 文字垂直居中。
            justify_content: JustifyContent::Center, // 文字水平居中。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) // Presentation按实际模式回显选中态。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 不把调试开关伪装成地形作者属性。
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 与中央工具条共享中文字体。
                font_size: px(ui_metrics::FONT_CONTROL), // 两字按钮在窄工作区仍清晰。
            }
            TextColor(colors::TEXT_PRIMARY) // 保持和其他工具按钮相同的文字颜色。
        ]
    }
}

/// 构造可与任一着色模式同时选中的真实三角网格按钮。
fn grid_button() -> impl Scene {
    bsn! {
        InteractionButton
        TerrainGridButton // 独立切换TerrainGridPreview，不进入平滑/平直互斥组。
        Node {
            flex_grow: 1.0, flex_basis: px(0), min_width: px(0), // 与碰撞按钮平分窄栏，不挤出属性区。
            height: px(22), // 保持场景属性卡片的紧凑行高。
            border: px(1), // 铜边延续编辑器调试按钮语言。
            border_radius: BorderRadius::all(px(3)), // 与平滑/平直按钮一致。
            align_items: AlignItems::Center, // 文字垂直居中。
            justify_content: JustifyContent::Center, // 文字水平居中。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) // Presentation按独立开关回显选中态。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 不把可视调试开关伪装成作者数据。
        Children [
            Text("显示三角线") @feathrise_editor_ui::localization::label_scene("显示三角线") // 真实拓扑预览与视口参考网格线分开，避免误认几何创建入口。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 与上方模式按钮共享中文字体。
                font_size: px(ui_metrics::FONT_CONTROL), // 四字标签在窄右栏保持清晰。
            }
            TextColor(colors::TEXT_PRIMARY) // 复用同卡片主要按钮文字色。
        ]
    }
}

fn collision_button() -> impl Scene {
    bsn! { InteractionButton TerrainCollisionButton
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), height: px(22), border: px(1),
            border_radius: BorderRadius::all(px(3)), align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [Text("显示碰撞") @feathrise_editor_ui::localization::label_scene("显示碰撞") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)] }
}

/// 与F1使用同一偏好和Avian绘制组；固定配置同步不扫描碰撞体，也不修改物理模拟。
pub(crate) fn install_collision_preview(app: &mut App) {
    app.add_systems(Update, handle_collision_button.in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, sync_collision_preview.in_set(crate::SceneEditorSystemSet::Presentation));
}

fn handle_collision_button(
    editor: Res<SceneEditorState>,
    session: Res<TerrainShadingSession>,
    sidebar: Res<SceneEditorSidebarState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    gate: super::terrain_objects::PanelWorkspaceGate,
    buttons: Query<&EditorButtonClick, (With<TerrainCollisionButton>, Changed<EditorButtonClick>)>,
    mut settings: ResMut<feathrise_editor_core::EditorSettings>,
) {
    if !editor.open || editor.close_requested || editor.new_scene_dialog_open || editor.scene_transition_in_progress
        || editor.pending_scene_transition.is_some() || editor.scene_switch_confirmation.is_some()
        || guard.blocked() || !gate.buttons_allowed() || !sidebar.scene_tree_visible()
        || !session.preview_expanded || session.scene_id.is_none() || session.scene_id != editor.selected_scene_id { return; }
    if buttons.iter().any(|interaction| interaction.is_clicked()) {
        settings.scene.show_physics_debug = !settings.scene.show_physics_debug;
    }
}

fn sync_collision_preview(
    settings: Res<feathrise_editor_core::EditorSettings>,
    store: Option<ResMut<bevy::gizmos::config::GizmoConfigStore>>,
    mut buttons: Query<(&UiInteraction, &mut BackgroundColor), With<TerrainCollisionButton>>,
) {
    if let Some(mut store) = store {
        if store.get_config::<avian3d::debug_render::PhysicsGizmos>()
            .is_some_and(|(config, _)| config.enabled != settings.scene.show_physics_debug) {
            store.config_mut::<avian3d::debug_render::PhysicsGizmos>().0.enabled = settings.scene.show_physics_debug;
        }
    }
    for (interaction, mut background) in &mut buttons {
        let color = if settings.scene.show_physics_debug { colors::SELECT_BG } else { match interaction {
            UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
            UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
        }};
        background.set_if_neq(BackgroundColor(color));
    }
}

/// 基础地形选中后显示独立着色与真实三角网格设置。
pub(crate) fn terrain_shading_controls() -> impl Scene {
    bsn! {
        super::gltf_browser::SceneBaseTerrainDetails
        Node {
            display: Display::None, // 初始未选择基础地形时不占场景列表空间。
            width: percent(100), // 与基础地形场景行对齐。
            flex_shrink: 0.0, // 展开后不得被摆放实例列表压缩。
            flex_direction: FlexDirection::Column, // 标题、按钮与会话提示依次排列。
            row_gap: px(4), // 紧凑场景属性行距。
            padding: UiRect::all(px(6)), // 网格设置与树行保持可读间距。
            border: UiRect::all(px(1)), // 铜边区分属性区域与普通实例。
        }
        BackgroundColor(colors::SLOT_BG) // 沿用场景树底色。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 调试属性不冒充作者网格操作。
        Children [

                Text("场景显示预览") @feathrise_editor_ui::localization::label_scene("场景显示预览") // 场景级显示辅助不属于选中网格的持久属性。
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 沿用编辑器字体。
                    font_size: px(ui_metrics::FONT_HINT), // 属性标题弱于可操作按钮。
                }
                TextColor(colors::TEXT_HINT) // 预览设置不与正式作者工具争视觉重心。
            --

                Node {
                    width: percent(100), // 两档着色模式保持同一属性行。
                    column_gap: px(4), // 模式之间保留明确点击间距。
                }
                Children [
                    @shading_button("平滑", false)-- // 默认恢复作者平滑法线。
                    @shading_button("平直", true) // GPU片元逐三角面法线，仅作调试。
                ]
            --

                Node {
                    width: percent(100), column_gap: px(4), // 三角线与碰撞各自切换，与着色模式互不覆盖。
                }
                Children [
                    @grid_button()-- // 可与平滑或平直同时保持选中。
                    @collision_button()
                ]
            --

                Text("着色/三角线：可塑形网格；碰撞：实际物理线条") @feathrise_editor_ui::localization::label_scene("着色/三角线：可塑形网格；碰撞：实际物理线条") // 明确场景着色范围与全局物理辅助显示的区别。
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 与属性标题使用同一字体。
                    font_size: px(ui_metrics::FONT_HINT), // 提示弱于模式按钮。
                }
                TextColor(colors::TEXT_HINT) // 说明调试设置的生命周期。

        ]
    }
}

/// 点击场景页当前基础地形才展开设置，不占用摆放实例选择身份。
pub(crate) fn handle_scene_base_terrain_row(
    editor: Res<SceneEditorState>, // 未进入场景或编辑器关闭时不得选择隐藏行。
    authoring: Res<EditorAuthoringSession>, // 当前基础地形必须已对应真实草稿。
    sidebar: Res<SceneEditorSidebarState>, // 切离场景页后旧交互不应改变选择。
    numeric_scrub: Res<SceneEditorNumericScrubMouseGuard>, // 数值拖动和弦不能误选地形。
    row: Query<&EditorButtonClick, (With<SceneBaseGltfRow>, Changed<EditorButtonClick>)>, // 固定一个基础地形按钮。
    mut session: ResMut<TerrainShadingSession>, // 仅记录面板展开态，不创建作者实例。
) {
    if !editor.open
        || editor.close_requested
        || numeric_scrub.blocked()
        || !sidebar.scene_tree_visible()
        || session.scene_id.as_ref() != editor.selected_scene_id.as_ref()
        || !authoring
            .draft
            .as_ref()
            .is_some_and(|draft| Some(&draft.scene_id) == session.scene_id.as_ref()
                && draft.manifest.terrains.as_ref().map_or_else(
                    || !draft.manifest.terrain.is_empty(), |objects| !objects.is_empty()))
    {
        return; // 草稿尚未到达或切换场景期间不可展开旧地形。
    }
    for interaction in &row {
        if interaction.is_clicked() {
            session.preview_expanded = !session.preview_expanded; // 再次点击同一地形收起属性。
        }
    }
}

/// 仅接受场景页选中地形的着色按钮；选择不进入保存链。
pub(crate) fn handle_terrain_shading_buttons(
    editor: Res<SceneEditorState>, // 避免没有活动场景时设置无法预览的模式。
    buttons: Query<(&EditorButtonClick, &TerrainShadingButton), Changed<EditorButtonClick>>, // 两个静态按钮保持独立输入身份。
    numeric_scrub: Res<SceneEditorNumericScrubMouseGuard>, // 右拖/中键和弦不触发普通按钮。
    sidebar: Res<SceneEditorSidebarState>,                 // 属性隐藏时不能消费旧按钮交互。
    mut session: ResMut<TerrainShadingSession>,          // 本进程按场景保留调试选择，不写作者文件。
    mut preview: ResMut<TerrainShadingPreview>,          // 只在模式真实改变时写材质同步资源。
) {
    if !editor.open
        || editor.close_requested
        || numeric_scrub.blocked()
        || !sidebar.scene_tree_visible()
        || !session.preview_expanded
        || session.scene_id.as_ref() != editor.selected_scene_id.as_ref()
        || session.scene_id.is_none()
    {
        return; // 仅选中当前基础地形时允许切换玩法和编辑器共用的模式。
    }
    for (interaction, button) in &buttons {
        if interaction.is_clicked() && preview.flat != button.0 {
            let scene_id = session.scene_id.as_ref().unwrap().clone(); // 只为一次明确点击冻结当前场景身份。
            if button.0 {
                session.flat_scenes.insert(scene_id); // 只记录选过平直的场景。
            } else {
                session.flat_scenes.remove(&scene_id); // 恢复平滑时不保留多余缓存。
            }
            preview.flat = button.0; // 玩法视角与编辑器共用临时材质uniform，关闭不撤销。
        }
    }
}

/// 仅接受场景页已选地形的网格按钮；它与平滑/平直互不改写。
pub(crate) fn handle_terrain_grid_button(
    editor: Res<SceneEditorState>, // 避免没有活动场景时保留无法显示的网格状态。
    buttons: Query<&EditorButtonClick, (With<TerrainGridButton>, Changed<EditorButtonClick>)>, // 固定一个独立切换按钮。
    numeric_scrub: Res<SceneEditorNumericScrubMouseGuard>, // 数值拖动和弦不得误触显示网格。
    sidebar: Res<SceneEditorSidebarState>,                 // 属性卡片隐藏时不能消费旧按钮交互。
    mut session: ResMut<TerrainShadingSession>, // 本进程按场景记录开启集合，不写持久化数据。
    mut preview: ResMut<TerrainGridPreview>,    // 正式场景与作者预览共享唯一显示状态。
) {
    if !editor.open // 关闭F12时保留现有选择，但隐藏按钮不能继续响应。
        || editor.close_requested // 安全关闭等待阶段也拒绝新输入。
        || numeric_scrub.blocked() // 公共数值控件拥有当前鼠标和弦。
        || !sidebar.scene_tree_visible() // 只允许左侧场景导航操作。
        || !session.preview_expanded // 基础地形属性必须真实展开。
        || session.scene_id.as_ref() != editor.selected_scene_id.as_ref() // 切换场景期间禁止写旧场景集合。
        || session.scene_id.is_none()
    // 菜单与加载页没有可显示基础地形。
    {
        return; // 隐藏控件、未选地形或无场景时完全无效。
    } // 完成网格按钮输入门。
    for interaction in &buttons {
        // 固定一个按钮，Changed过滤稳定帧为空。
        if !interaction.is_clicked() {
            // 悬停与释放只影响样式。
            continue; // 等待明确按下边界。
        } // 完成按下判定。
        let scene_id = session.scene_id.as_ref().unwrap().clone(); // 冻结本次点击对应的当前场景身份。
        let visible = !preview.visible; // 独立按钮按下翻转网格，不读取着色状态。
        if visible {
            // 仅为开启网格的场景保留集合项。
            session.grid_scenes.insert(scene_id); // 关闭F12后玩法视角继续显示当前网格。
        } else {
            // 明确关闭时删除该图缓存。
            session.grid_scenes.remove(&scene_id); // 切回该图将恢复为关闭。
        } // 完成当前场景集合更新。
        preview.visible = visible; // 同帧唤醒普通Wireframe与Clipmap专用覆盖层。
    } // 完成网格按钮交互处理。
} // 结束独立网格按钮系统。

/// 按真实玩法场景身份恢复临时模式；关闭F12只收起属性，不撤销玩家视角的着色。
pub(crate) fn sync_terrain_shading_session(
    editor: Res<SceneEditorState>, // 编辑器开关只决定属性面板选择的可见性。
    scene: Option<Res<State<ScenePhase>>>, // 正式场景或加载页决定当前玩法场景身份。
    active_dynamic: Option<Res<ActiveDynamicScene>>, // 同一动态场景下必须区分真实SceneId。
    mut session: ResMut<TerrainShadingSession>, // 缓存本进程每个场景的调试选择。
    mut preview: ResMut<TerrainShadingPreview>, // 切换场景或返回菜单时更新GPU地形受光。
    mut grid: ResMut<TerrainGridPreview>, // 切换场景或返回菜单时更新真实三角网格覆盖。
) {
    let scene_id = if let Some(scene) = scene.as_deref() {
        active_dynamic
            .as_deref()
            .and_then(|active| active_gameplay_scene_id(*scene.get(), active)) // 传送和F12关闭期间仍跟随正式世界。
    } else {
        editor.selected_scene_id.clone() // 无状态插件的独立UI测试沿用已选场景。
    };
    if session.editor_open == editor.open && session.scene_id == scene_id {
        return; // 无切换场景或开关变化时不访问任何地形资产。
    }
    let flat = scene_id
        .as_ref()
        .is_some_and(|id| session.flat_scenes.contains(id)); // 新图与非玩法阶段默认平滑。
    let grid_visible = scene_id
        .as_ref()
        .is_some_and(|id| session.grid_scenes.contains(id)); // 新图与非玩法阶段默认关闭网格。
    session.preview_expanded = false; // 关闭或切换场景只撤销场景属性选择，不丢失每图着色模式。
    session.editor_open = editor.open; // 保存本次最终可见性。
    session.scene_id = scene_id; // 玩法视角仍保留当前场景身份。
    if preview.flat != flat {
        preview.flat = flat; // 玩家视角与F12始终共享本进程该场景的当前模式。
    }
    if grid.visible != grid_visible {
        // 着色与网格分别比较，任一状态可独立恢复。
        grid.visible = grid_visible; // 关闭F12保留当前图选择，切换场景或加载页按集合恢复/清零。
    } // 完成当前场景网格状态发布。
}

/// 两个按钮随模式原位高亮，不改变同一工具栏的视图/笔刷状态。
pub(crate) fn sync_terrain_shading_button_styles(
    preview: Res<TerrainShadingPreview>, // 显示真实地表材质所用模式。
    mut buttons: Query<(&UiInteraction, &TerrainShadingButton, &mut BackgroundColor)>, // 固定两个按钮，成本不随场景规模增长。
) {
    for (interaction, button, mut background) in &mut buttons {
        let color = if preview.flat == button.0 {
            colors::SELECT_BG // 当前模式用既有编辑器选中态。
        } else {
            match interaction {
                UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
                UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
                UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
            }
        };
        if background.0 != color {
            background.0 = color; // 仅视觉变化时写UI组件。
        }
    }
}

/// 独立网格按钮按开关原位高亮，可与任一着色按钮同时显示铜金选中态。
pub(crate) fn sync_terrain_grid_button_style(
    preview: Res<TerrainGridPreview>, // 读取正式场景当前实际网格开关。
    mut buttons: Query<(&UiInteraction, &mut BackgroundColor), With<TerrainGridButton>>, // 固定一个按钮，成本恒定。
) {
    for (interaction, mut background) in &mut buttons {
        // Presentation只遍历静态单按钮。
        let color = if preview.visible {
            // 开启网格时保持铜金选中态，不受悬停覆盖。
            colors::SELECT_BG // 复用平滑/平直选中颜色。
        } else {
            // 关闭时显示常规按压与悬停反馈。
            match interaction {
                UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED, // 按下即时反馈。
                UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED, // 悬停强化铜边暗底。
                UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,     // 稳定关闭底色。
            } // 完成关闭状态交互配色。
        }; // 完成当前按钮颜色选择。
        if background.0 != color {
            // 稳定帧不重复写UI组件。
            background.0 = color; // 只在开关或交互真实变化时更新。
        } // 完成颜色变化检查。
    } // 完成独立网格按钮样式同步。
} // 结束网格按钮Presentation系统。

#[cfg(test)]
mod tests {
    use super::*; // 使用生产交互与会话重置系统。
    use feathrise_scene_content::SceneId; // 两个场景验证临时预览不会串图。

    #[test]
    fn collision_preview_uses_shared_config_and_rejects_hidden_or_stale_controls() {
        use avian3d::debug_render::PhysicsGizmos;
        use bevy::gizmos::config::{GizmoConfig, GizmoConfigStore};
        let map = SceneId::parse("MAP-preview").unwrap();
        let mut store = GizmoConfigStore::default();
        store.insert(GizmoConfig { enabled: false, ..default() }, PhysicsGizmos::default());
        let mut app = App::new();
        app.insert_resource(store)
            .insert_resource(SceneEditorState { open: true, selected_scene_id: Some(map.clone()), ..default() })
            .insert_resource(TerrainShadingSession { scene_id: Some(map), preview_expanded: true, ..default() })
            .insert_resource({ let mut state = SceneEditorSidebarState::default(); state.navigator = crate::ui::sidebars::EditorNavigatorTab::SceneTree; state })
            .init_resource::<SceneEditorNumericScrubMouseGuard>()
            .init_resource::<feathrise_editor_core::EditorSettings>()
            .init_resource::<EditorAuthoringSession>()
            .add_systems(Update, (handle_collision_button, sync_collision_preview).chain());
        let button = app.world_mut().spawn((TerrainCollisionButton, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, BackgroundColor::default())).id();
        let collider = app.world_mut().spawn(avian3d::prelude::Collider::cuboid(2.0, 1.0, 3.0)).id();
        app.update();
        assert!(app.world().resource::<feathrise_editor_core::EditorSettings>().scene.show_physics_debug);
        assert!(app.world().resource::<GizmoConfigStore>().config::<PhysicsGizmos>().0.enabled);
        assert!(app.world().get::<avian3d::prelude::Collider>(collider).is_some());
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        for (expanded, selected_scene) in [(false, "MAP-preview"), (true, "MAP-other")] {
            *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
            app.update();
            app.world_mut().resource_mut::<TerrainShadingSession>().preview_expanded = expanded;
            app.world_mut().resource_mut::<SceneEditorState>().selected_scene_id = Some(SceneId::parse(selected_scene).unwrap());
            app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
            app.update();
            assert!(app.world().resource::<feathrise_editor_core::EditorSettings>().scene.show_physics_debug);
        }
        // F1或偏好加载改动同一源后，F12不会用自己的副本覆盖它。
        app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>().scene.show_physics_debug = false;
        app.update();
        assert!(!app.world().resource::<GizmoConfigStore>().config::<PhysicsGizmos>().0.enabled);
    }

    #[test] // 基础地形的选择和网格模式只响应当前场景页的真实草稿。
    fn clicking_base_terrain_opens_shading_properties_without_placement_selection() {
        let draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121])); // 小型完整GLB地形草稿。
        let scene_id = draft.scene_id.clone(); // UI和场景会话必须属于同一真实场景。
        let mut app = App::new(); // 只运行固定交互系统，不启动实际GPU。
        app.init_resource::<SceneEditorState>()
            .init_resource::<EditorAuthoringSession>()
            .insert_resource({ let mut state = SceneEditorSidebarState::default(); state.navigator = crate::ui::sidebars::EditorNavigatorTab::SceneTree; state })
            .init_resource::<SceneEditorNumericScrubMouseGuard>()
            .init_resource::<TerrainShadingSession>()
            .init_resource::<TerrainShadingPreview>()
            .init_resource::<TerrainGridPreview>() // 独立网格开关从关闭开始。
            .add_systems(
                Update,
                (
                    handle_scene_base_terrain_row,    // 同帧先展开属性。
                    handle_terrain_shading_buttons, // 再接受互斥着色输入。
                    handle_terrain_grid_button,     // 最后接受独立网格输入。
                )
                    .chain(),
            );
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft = Some(draft); // 草稿已加载。
        let editor = &mut app.world_mut().resource_mut::<SceneEditorState>();
        editor.open = true;
        editor.selected_scene_id = Some(scene_id.clone());
        app.world_mut()
            .resource_mut::<TerrainShadingSession>()
            .scene_id = Some(scene_id); // 正式场景身份已同步。
        let row = app
            .world_mut()
            .spawn((SceneBaseGltfRow, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
            .id(); // 点击树内唯一基础地形。
        app.world_mut()
            .spawn((TerrainShadingButton(true), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)); // 同帧点击模式也只由选择门授权。
        app.world_mut()
            .spawn((TerrainGridButton, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)); // 同帧独立开启真实三角网格。
        app.update();
        assert!(
            app.world()
                .resource::<TerrainShadingSession>()
                .preview_expanded
        ); // 场景节点确实展开属性。
        assert!(app.world().resource::<TerrainShadingPreview>().flat); // 选择后模式写入当前地形。
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 网格与平直可同时启用。
        *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
        app.update();
        app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered())); // 再次点击收起地形属性。
        app.world_mut()
            .spawn((TerrainShadingButton(false), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)); // 隐藏的模式按钮不得生效。
        app.world_mut()
            .spawn((TerrainGridButton, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)); // 隐藏的网格按钮同样不得生效。
        app.update();
        assert!(
            !app.world()
                .resource::<TerrainShadingSession>()
                .preview_expanded
        ); // 不删除地形或摆放实例。
        assert!(app.world().resource::<TerrainShadingPreview>().flat); // 收起面板不重置本次运行的选择。
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 隐藏控件不能翻转网格状态。
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
            .manifest.terrains = Some(Vec::new().into());
        *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
        app.update();
        app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update();
        assert!(!app.world().resource::<TerrainShadingSession>().preview_expanded);
        // 隐藏预览条的迟到Pressed不能把显式空容器当作原基础网格重新展开。
    }

    #[test] // 着色和网格彼此独立，且都在关闭F12后保留并按场景恢复。
    fn terrain_display_settings_survive_editor_close_and_recover_per_scene() {
        let mut app = App::new(); // 无窗口运行UI状态与场景身份测试。
        app.init_resource::<SceneEditorState>()
            .init_resource::<TerrainShadingSession>()
            .init_resource::<TerrainShadingPreview>()
            .init_resource::<TerrainGridPreview>() // 重启语义由默认false覆盖。
            .init_resource::<SceneEditorNumericScrubMouseGuard>()
            .insert_resource({ let mut state = SceneEditorSidebarState::default(); state.navigator = crate::ui::sidebars::EditorNavigatorTab::SceneTree; state }) // 默认显示右侧场景页。
            .insert_resource(State::new(ScenePhase::Active)) // 正式玩法身份不受F12开关影响。
            .init_resource::<ActiveDynamicScene>() // 动态场景通过SceneId区分调试状态。
            .add_systems(
                Update,
                (
                    sync_terrain_shading_session,   // 先按当前玩法场景恢复两个独立状态。
                    handle_terrain_shading_buttons, // 再处理本帧着色点击。
                    handle_terrain_grid_button,     // 最后处理本帧网格点击。
                )
                    .chain(),
            );
        let first = SceneId::parse("MAP-shading-first").unwrap();
        let second = SceneId::parse("MAP-shading-second").unwrap();
        app.world_mut().resource_mut::<ActiveDynamicScene>().select(first.clone());
        let editor = &mut app.world_mut().resource_mut::<SceneEditorState>();
        editor.open = true;
        editor.selected_scene_id = Some(first.clone());
        app.update(); // 进入首张图后地形属性默认收起、模式默认平滑。
        assert!(
            !app.world()
                .resource::<TerrainShadingSession>()
                .preview_expanded
        );
        app.world_mut()
            .resource_mut::<TerrainShadingSession>()
            .preview_expanded = true; // 模拟点击基础地形行。
        let button = app
            .world_mut()
            .spawn((TerrainShadingButton(true), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
            .id();
        let grid_button = app
            .world_mut()
            .spawn((TerrainGridButton, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
            .id(); // 同一场景独立开启网格。
        app.update();
        assert!(app.world().resource::<TerrainShadingPreview>().flat); // 点击只对当前场景生效。
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 平直与网格同时保持开启。
        app.world_mut().resource_mut::<SceneEditorState>().open = false;
        app.update();
        assert!(app.world().resource::<TerrainShadingPreview>().flat); // 关闭F12后玩家视角仍使用平直。
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 关闭F12后玩家视角仍显示网格。
        assert!(
            !app.world()
                .resource::<TerrainShadingSession>()
                .preview_expanded
        ); // 隐藏面板不保留展开状态。
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .selected_scene_id = Some(second.clone()); // 玩家在编辑器关闭时进入另一个场景。
        app.world_mut().resource_mut::<ActiveDynamicScene>().select(second); // 同枚举内切到第二个场景。
        app.update();
        assert!(!app.world().resource::<TerrainShadingPreview>().flat); // 新图默认平滑。
        assert!(!app.world().resource::<TerrainGridPreview>().visible); // 新图默认关闭网格。
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
        app.update();
        app.world_mut()
            .resource_mut::<TerrainShadingSession>()
            .preview_expanded = true; // 新图需要重新选择地形。
        *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
        *app.world_mut().get_mut::<UiInteraction>(grid_button).unwrap() = UiInteraction::None; // 两按钮分别经历释放边界。
        app.update();
        app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.world_mut().entity_mut(grid_button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered())); // 第二张图也独立开启网格。
        app.update();
        assert!(app.world().resource::<TerrainShadingPreview>().flat);
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 第二张图记录自己的网格选择。
        app.world_mut().resource_mut::<SceneEditorState>().open = false;
        app.update();
        assert!(app.world().resource::<TerrainShadingPreview>().flat); // 第二张图在游戏里也保留平直。
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 第二张图在游戏里也保留网格。
        app.world_mut().resource_mut::<ActiveDynamicScene>().select(first.clone());
        let editor = &mut app.world_mut().resource_mut::<SceneEditorState>();
        editor.open = true;
        editor.selected_scene_id = Some(first); // 切回首图恢复其独立调试选择。
        app.update();
        assert!(app.world().resource::<TerrainShadingPreview>().flat);
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 切回首图恢复其独立网格选择。
        app.world_mut()
            .resource_mut::<TerrainShadingSession>()
            .preview_expanded = true; // 展开首图网格设置。
        app.world_mut()
            .spawn((TerrainGridButton, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)); // 先只关闭网格，验证不会改写平直着色。
        app.update();
        assert!(!app.world().resource::<TerrainGridPreview>().visible); // 首图网格已独立关闭。
        assert!(app.world().resource::<TerrainShadingPreview>().flat); // 平直模式不受网格按钮影响。
        app.world_mut()
            .spawn((TerrainShadingButton(false), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)); // 明确选择平滑应清除该图缓存。
        app.update();
        assert!(!app.world().resource::<TerrainShadingPreview>().flat);
        assert!(!app.world().resource::<TerrainGridPreview>().visible); // 选择平滑不会重新打开网格。
        assert!(
            app.world()
                .resource::<TerrainShadingSession>()
                .flat_scenes
                .len()
                == 1
        ); // 第二张图仍保持平直选择。
        assert_eq!(
            app.world()
                .resource::<TerrainShadingSession>()
                .grid_scenes
                .len(),
            1
        ); // 第二张图仍保持网格选择。
    }

    #[test] // 共用动态场景必须在F12关闭时也区分每个真实SceneId。
    fn gameplay_scene_identity_resets_on_loading_and_restores_dynamic_scene_mode() {
        let first = SceneId::parse("MAP-a").unwrap(); // 第一张动态场景已在本进程选择平直。
        let second = SceneId::parse("MAP-b").unwrap(); // 第二个场景保持默认平滑。
        let mut app = App::new(); // 最小正式状态与渲染预览资源。
        app.init_resource::<SceneEditorState>()
            .init_resource::<TerrainShadingSession>()
            .init_resource::<TerrainShadingPreview>()
            .init_resource::<TerrainGridPreview>() // 动态场景同样使用进程内默认关闭。
            .insert_resource(State::new(ScenePhase::Active))
            .insert_resource(ActiveDynamicScene {
                scene_id: Some(first.clone()),
            })
            .add_systems(Update, sync_terrain_shading_session);
        app.world_mut()
            .resource_mut::<TerrainShadingSession>()
            .flat_scenes
            .insert(first.clone()); // 仅改变本进程缓存。
        app.world_mut()
            .resource_mut::<TerrainShadingSession>()
            .grid_scenes
            .insert(first.clone()); // 第一张动态场景同时记忆开启网格。
        app.update();
        assert!(app.world().resource::<TerrainShadingPreview>().flat); // 不打开F12也应用当前图模式。
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 不打开F12也恢复当前图网格。
        app.world_mut().resource_mut::<ActiveDynamicScene>().scene_id = Some(second); // 同一GameScene切到另一张图。
        app.update();
        assert!(!app.world().resource::<TerrainShadingPreview>().flat); // 不能把旧图模式泄漏给新图。
        assert!(!app.world().resource::<TerrainGridPreview>().visible); // 不能把旧图网格泄漏给新图。
        *app.world_mut().resource_mut::<State<ScenePhase>>() = State::new(ScenePhase::Loading); // 切换场景加载中无实际地形。
        app.update();
        assert!(!app.world().resource::<TerrainShadingPreview>().flat); // 不继承上张图的渲染状态。
        assert!(!app.world().resource::<TerrainGridPreview>().visible); // 加载页强制关闭所有地形网格。
        app.world_mut().resource_mut::<ActiveDynamicScene>().scene_id = Some(first); // 玩家重新进入首图。
        *app.world_mut().resource_mut::<State<ScenePhase>>() =
            State::new(ScenePhase::Active);
        app.update();
        assert!(app.world().resource::<TerrainShadingPreview>().flat); // 本次进程的首图选择仍生效。
        assert!(app.world().resource::<TerrainGridPreview>().visible); // 返回首图恢复本进程网格选择。
    }
}
