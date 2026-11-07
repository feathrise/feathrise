//! 公共数值拖动、资产预览、资产右键菜单与地图编辑器其他鼠标入口之间的统一所有权仲裁。

use bevy::prelude::*; // 鼠标按钮、资源和Bevy系统条件。
use feathrise_editor_ui::ActiveNumericInputScrub; // 共享 UI 控件提供当前专业数字拖动的唯一实体身份。

use super::asset_context_menu::AssetContextMenuState; // 菜单活动和关闭帧也不得向下透传鼠标。
use super::asset_preview::AssetPreviewNavigation;
use super::water_preset_editor::WaterPresetLivePreviewNavigation; // 水体样品拖出画布后仍独占当前手势。
use crate::resources::{MapBrowserDragState, MapEditorLayoutState, MapEditorNumericScrubMouseGuard, MapEditorState}; // 所有作者鼠标手势共享静默帧释放门。
use crate::workbench::{WorkbenchFloatingPanel, WorkbenchFloatingPanels};

/// 与详情一致，原始鼠标和滚轮读者运行前先认领上一帧实际呈现的浮层区域。
pub(crate) fn track_floating_panel_pointer(
    panels: WorkbenchFloatingPanels,
    host_panels: Query<(), With<WorkbenchFloatingPanel>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<MapEditorNumericScrubMouseGuard>,
) {
    guard.floating_panel_hovered = panels.hovered().is_some_and(|entity| host_panels.contains(entity));
    if guard.floating_panel_hovered && mouse.get_just_pressed().next().is_some() {
        guard.advance(true, false, false); // 只为从面板开始的手势延续到松键和静默帧。
    }
}

/// 在其余地图编辑器输入系统之前推进公共数值拖动鼠标门。
pub(crate) fn update_map_editor_numeric_scrub_mouse_guard(
    active: Res<ActiveNumericInputScrub>, // 共享 UI 控件本帧完成拖动起止判定后的最终身份。
    mouse: Res<ButtonInput<MouseButton>>, // 和弦与release边沿必须覆盖左、右、中三枚作者鼠标键。
    mut guard: ResMut<MapEditorNumericScrubMouseGuard>, // 唯一地图编辑器级鼠标所有权状态。
    preview: (Option<Res<AssetPreviewNavigation>>, Option<Res<super::asset_preview::AudioPreviewState>>, Option<Res<super::asset_preview::AnimationEditorState>>), // 图片/模型、音频和动画时间轴共用释放门。
    minimap: Option<Res<super::asset_preview::DocumentMinimapState>>,
    water_preview: Option<Res<WaterPresetLivePreviewNavigation>>, // 独立测试可不安装水体预览插件。
    asset_menu: (Option<Res<AssetContextMenuState>>, Option<Res<super::scene_context_menu::SceneContextMenuState>>, Option<Res<super::map_sidebar::MapDirectoryMenuState>>), // 目录、场景和资产菜单共用关闭点击及松键帧隔离。
    tab_menu: Option<Res<super::asset_preview::AssetTabMenuState>>, // 文件标签菜单与关闭帧共用同一释放门。
    layout: Option<Res<MapEditorLayoutState>>, // 分隔条与布局按钮不能把同次点击穿透到刚移动的地图画布。
    settings: Option<Res<crate::resources::MapEditorSettingsState>>,
    editor: Option<Res<MapEditorState>>,
    map_drag: Option<Res<MapBrowserDragState>>,
    placement_move: Option<Res<crate::viewport::placement_move::PlacementMoveState>>,
    selection_owners: (
        Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
        Option<Res<crate::authoring::placement_batch::PlacementBatchState>>,
        Option<Res<super::status_bar::EditorStatusBarState>>,
        Option<Res<super::scene_objects::SceneObjectState>>,
        Option<Res<super::terrain_objects::TerrainObjectsEditorState>>,
        Option<Res<super::context_help::EditorContextHelpState>>,
        Option<Res<crate::authoring::basic_shapes::BasicShapeState>>,
        Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
    ),
    sub_map_move: Option<Res<crate::viewport::sub_maps::SubMapTransformState>>,
    spatial: (Option<Res<crate::viewport::spatial_brush::SpatialBrushState>>, Option<Res<feathrise_editor_core::EditorHostOverlay>>),
) {
    let any_button_pressed = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
        .into_iter()
        .any(|button| mouse.pressed(button)); // 主键松开但第二键仍按住时继续阻塞。
    let any_button_released = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
        .into_iter()
        .any(|button| mouse.just_released(button)); // 最后一枚键的release帧仍丢弃UI与世界输入。
    guard.advance(
        active.owns_mouse() // 同帧按下并Esc时事务已清空，取消后的原按压仍必须先取得释放门。
            // 网格编辑空闲时允许观察相机；移动与退出释放帧才占公共门，活动手势自行收口，新会话等待本门归还。
            || selection_owners.7.as_ref().is_some_and(|state| state.owns_mouse() && (state.moving() || state.releasing_mouse()))
            || sub_map_move.as_ref().is_some_and(|state| state.owns_mouse())
            || placement_move.as_ref().is_some_and(|state| state.owns_mouse()) // 手柄优先于相机、世界拾取和笔刷；release 后同样等待静默帧。
            || selection_owners.0.as_ref().is_some_and(|state| state.owns_mouse()) // 拖框与释放和弦也归入同一输入门。
            || selection_owners.1.as_ref().is_some_and(|state| state.owns_mouse()) // 粘贴/批量任务和取消释放帧不穿透相机与笔刷。
            || selection_owners.2.as_ref().is_some_and(|state| state.owns_mouse()) // 诊断阅读及关闭帧也等待三键静默，不把复制点击或滚轮传给地图。
            || selection_owners.3.as_ref().is_some_and(|state| state.busy()) // 完整组捕获完成前保持唯一作者版本。
            || selection_owners.4.as_ref().is_some_and(|state| state.owns_mouse()) // 地形手柄、创建确认与最终release帧共享静默释放门。
            || selection_owners.5.as_ref().is_some_and(|state| state.owns_mouse()) // 帮助正文滚动及关闭点击必须等静默帧再归还地图。
            || selection_owners.6.as_ref().is_some_and(|state| state.owns_mouse()) // 网格创建确认与取消释放帧独占鼠标，不把面板点击解释为放置。
            || spatial.0.as_ref().is_some_and(|state| state.owns_mouse()) // 空间笔刷定位与手柄拖动等到三键静默后归还。
            || spatial.1.as_ref().is_some_and(|overlay| overlay.active) // 宿主工程菜单与关闭点击复用同一静默释放门。
            || editor.as_ref().is_some_and(|editor| editor.map_switch_confirmation.is_some())
            || map_drag.as_ref().is_some_and(|drag| drag.blocks_other_mouse()) // 切图模态及取消 release 同时阻止并列工作区输入，不只隔离地图笔刷。
            || preview.0.as_ref().is_some_and(|preview| preview.dragging())
            || preview.1.as_ref().is_some_and(|audio| audio.owns_mouse())
            || preview.2.as_ref().is_some_and(|animation| animation.owns_mouse())
            || minimap.as_ref().is_some_and(|minimap| minimap.owns_mouse()) // 小地图拖出代码栏也不能改地图。
            || water_preview
                .as_ref()
                .is_some_and(|preview| preview.dragging())
            || asset_menu.0.as_ref().is_some_and(|menu| menu.owns_mouse())
            || asset_menu.1.as_ref().is_some_and(|menu| menu.owns_mouse())
            || asset_menu.2.as_ref().is_some_and(|menu| menu.owns_mouse())
            || tab_menu.as_ref().is_some_and(|menu| menu.owns_mouse())
            || layout
                .as_ref()
                .is_some_and(|layout| layout.owns_mouse_this_frame || layout.drag.is_some())
            || settings
                .as_ref()
                .is_some_and(|settings| settings.open || settings.owns_mouse_this_frame), // 设置窗口及关闭点击也经过同一静默帧门，避免缩放后点透新的画布。
        any_button_pressed,  // 和弦未全部松开时不归还所有权。
        any_button_released, // release边沿消失后的下一帧才允许新动作。
    );
}

/// 仅供没有`Changed`查询或`MessageReader`的世界作者系统使用的轻量运行条件。
pub(crate) fn map_editor_numeric_scrub_mouse_available(
    guard: Res<MapEditorNumericScrubMouseGuard>, // 读取本帧已经推进完成的统一门。
    host_page: Option<Res<crate::workbench::WorkbenchHostPageState>>,
) -> bool {
    !guard.blocked() && !host_page.as_ref().is_some_and(|page| page.active || page.menu_open)
    // 开始页只阻断世界作者输入；不占模态鼠标门，原设置入口仍能正常打开。
}

#[cfg(test)]
mod tests {
    use feathrise_map_content::MapAssetKind; // 构造一个真实可选择的资产目录行。

    use super::*; // 直接测试资源状态机，不启动窗口或渲染插件。
    use crate::components::MapEditorAssetRow; // 固定资产行提供Changed<Interaction>回归入口。
    use crate::resources::{EditorAssetCatalog, MapEditorState}; // 验证阻塞帧不修改目录与编辑会话。
    use crate::types::EditorAssetEntry; // 最小资产记录允许点击系统产生可观察副作用。

    #[test]
    fn host_overlay_uses_the_existing_mouse_release_boundary() {
        let mut app = App::new();
        app.init_resource::<ActiveNumericInputScrub>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<MapEditorNumericScrubMouseGuard>()
            .insert_resource(feathrise_editor_core::EditorHostOverlay { active: true })
            .add_systems(Update, update_map_editor_numeric_scrub_mouse_guard);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
        app.world_mut().resource_mut::<feathrise_editor_core::EditorHostOverlay>().active = false;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.update();
        assert!(!app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
    }

    #[test]
    fn floating_panel_only_owns_hover_and_the_captured_mouse_release() {
        use crate::workbench::WorkbenchFloatingPanel;
        use bevy::ui::RelativeCursorPosition;

        let mut app = App::new();
        app.init_resource::<ActiveNumericInputScrub>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<MapEditorNumericScrubMouseGuard>()
            .add_systems(PreUpdate, track_floating_panel_pointer)
            .add_systems(Update, update_map_editor_numeric_scrub_mouse_guard);
        let panel = app.world_mut().spawn((WorkbenchFloatingPanel, GlobalZIndex(426),
            ComputedNode { size: Vec2::splat(120.), ..default() }, InheritedVisibility::VISIBLE)).id();
        let blocked = |app: &App| app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked();

        app.update();
        assert!(!blocked(&app));
        app.world_mut().get_mut::<RelativeCursorPosition>(panel).unwrap().cursor_over = true;
        app.update();
        assert!(blocked(&app), "浮窗内滚轮不得落到地图");

        app.world_mut().get_mut::<RelativeCursorPosition>(panel).unwrap().cursor_over = false;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert!(!blocked(&app), "离开浮窗同帧开始的外部点击应正常受理");

        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        app.world_mut().get_mut::<RelativeCursorPosition>(panel).unwrap().cursor_over = true;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        app.world_mut().get_mut::<RelativeCursorPosition>(panel).unwrap().cursor_over = false;
        app.update();
        assert!(blocked(&app), "拖选越出面板仍保持鼠标所有权");
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert!(blocked(&app), "释放帧不得落到地图");
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.update();
        assert!(!blocked(&app), "静默帧后恢复浮窗外的地图输入");
    }

    #[test]
    fn scrub_mouse_ownership_releases_only_after_the_chord_and_release_frame_clear() {
        let mut guard = MapEditorNumericScrubMouseGuard::default(); // 初始普通作者输入可用。

        guard.advance(true, true, false); // 左键或右键数值拖动开始并持有主键。
        assert!(guard.blocked()); // 拖动开始的同帧必须立即锁住其他鼠标入口。

        guard.advance(false, true, true); // 主拖动键松开，但和弦中的第二枚键仍按住。
        assert!(guard.blocked()); // Active实体消失也不能让第二枚键补触发按钮或世界编辑。

        guard.advance(false, false, true); // 最后一枚鼠标键在本帧松开。
        assert!(guard.blocked()); // 最终release边沿所在帧仍由数值拖动拥有，不能同帧穿透。

        guard.advance(false, false, false); // 下一帧确认三键均未按下且没有release边沿。
        assert!(!guard.blocked()); // 只有完整静默帧才把鼠标归还给地图编辑器。
    }

    #[test]
    fn ordinary_mouse_buttons_do_not_activate_the_guard_without_a_numeric_scrub() {
        let mut guard = MapEditorNumericScrubMouseGuard::default(); // 初始没有公共数值拖动事务。
        guard.advance(false, true, false); // 普通视口或按钮按键不应自行取得数值拖动所有权。
        assert!(!guard.blocked()); // 非数值操作继续按原有地图编辑器路径执行。
    }

    #[test]
    fn idle_mesh_edit_keeps_camera_navigation_available_without_unlocking_object_inputs() {
        let mut app = App::new();
        app.init_resource::<ActiveNumericInputScrub>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<MapEditorNumericScrubMouseGuard>()
            .init_resource::<crate::authoring::mesh_edit::MeshEditState>()
            .add_systems(Update, update_map_editor_numeric_scrub_mouse_guard);
        app.world_mut().resource_mut::<crate::authoring::mesh_edit::MeshEditState>().activate_for_test();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Middle);
        app.update();
        assert!(!app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked(), "空闲编辑允许作者环绕观察；普通对象入口另由active门阻止");
    }

    #[test]
    fn blocked_asset_row_press_is_consumed_instead_of_replayed_after_unlock() {
        let mut app = App::new(); // 最小ECS应用足够验证Changed tick推进，不加载窗口或渲染。
        let mut catalog = EditorAssetCatalog::default(); // 目录初始停留在assets根。
        catalog.entries.push(EditorAssetEntry {
            asset_path: "gltf/blocked.glb".to_owned(), // 若点击泄漏会被写入selected_asset。
            display_name: "阻塞测试模型".to_owned(),   // UI名称不参与业务判定。
            is_directory: false,                       // 文件点击走选择分支而非目录切换。
            kind: MapAssetKind::Scene,                 // 合法场景资产会产生明确可观察副作用。
        });
        app.insert_resource(catalog) // 提供资产点击系统的目录数据。
            .init_resource::<MapEditorState>() // 提供选中资产和状态文本。
            .init_resource::<MapEditorNumericScrubMouseGuard>() // 提供统一所有权状态。
            .add_systems(Update, crate::asset_catalog::handle_asset_row_clicks); // 真实输入系统必须每帧运行。
        app.world_mut()
            .resource_mut::<MapEditorNumericScrubMouseGuard>()
            .advance(true, true, false); // 数值拖动先取得鼠标所有权。
        app.world_mut().spawn((
            feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed, // 明确的完成点击仍须被数值门消费。
            MapEditorAssetRow {
                slot: 0,              // 固定行池第一个物理槽位。
                entry_index: Some(0), // 指向唯一合法测试资产。
            },
        ));

        app.update(); // 输入系统运行并推进Changed tick，但guard要求立即丢弃按下。
        assert!(
            app.world()
                .resource::<MapEditorState>()
                .selected_asset
                .is_none()
        ); // 阻塞帧没有选择资产。

        app.world_mut()
            .resource_mut::<MapEditorNumericScrubMouseGuard>()
            .advance(false, false, true); // 最终release帧仍保持所有权。
        app.update(); // 没有新的Interaction变化，仍不可产生动作。
        app.world_mut()
            .resource_mut::<MapEditorNumericScrubMouseGuard>()
            .advance(false, false, false); // 下一静默帧才解除所有权。
        app.update(); // 若曾用run_if暂停系统，旧Changed按下会在此处错误重放。

        assert!(
            app.world()
                .resource::<MapEditorState>()
                .selected_asset
                .is_none()
        ); // 已丢弃的按下不会在解锁后补执行。
    }
}
