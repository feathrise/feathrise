//! F4 动态场景监控的输入、两级分页、显隐与低频缓存报告刷新。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_atmosphere::{WeatherDebugOverride, WeatherState}; // 环境页复用现成天气状态，不创建第二套监控缓存。

use super::helpers::resource_monitor_page_display;
use super::report::build_editor_scene_report;
use crate::authoring::{EditorAuthoringSession, VegetationPreviewState, WaterPreviewState}; // F4只复用作者会话与现有预览完成值，不启动独立统计任务。
use crate::components::{
    SceneEditorMonitorGroupButton, SceneEditorMonitorPageText, SceneEditorMonitorPanel,
    SceneEditorMonitorSubTabRow, SceneEditorMonitorTabButton,
};
use crate::monitor_resources::EditorSceneMonitorState; // 当前页枚举通过状态字段读取，无需单独导入类型。
use crate::resources::SceneEditorState;

type GroupButtonData = (
    &'static SceneEditorMonitorGroupButton,
    &'static UiInteraction,
    &'static mut BackgroundColor,
    &'static mut BorderColor,
);
type GroupButtonFilter = (With<InteractionButton>, Without<SceneEditorMonitorTabButton>);
type TabButtonData = (
    &'static SceneEditorMonitorTabButton,
    &'static UiInteraction,
    &'static mut BackgroundColor,
    &'static mut BorderColor,
    &'static mut Node,
);
type TabButtonFilter = (With<InteractionButton>, Without<SceneEditorMonitorGroupButton>);
type SubTabRowFilter = (
    With<SceneEditorMonitorSubTabRow>,
    Without<SceneEditorMonitorTabButton>,
    Without<SceneEditorMonitorPageText>,
);
type PageNodeData = (&'static SceneEditorMonitorPageText, &'static mut Node);
type PageNodeFilter = (
    Without<SceneEditorMonitorSubTabRow>,
    Without<SceneEditorMonitorTabButton>,
);

/// 响应F4并同步面板显隐；重新打开时立即刷新当前数据页。
pub(super) fn toggle_editor_scene_monitor(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<EditorSceneMonitorState>,
    mut panels: Query<&mut Visibility, With<SceneEditorMonitorPanel>>,
) {
    if !keyboard.just_pressed(KeyCode::F4) {
        return;
    }
    state.visible = !state.visible;
    state.force_refresh = state.visible;
    if state.visible {
        state.refresh_timer.reset();
    }
    for mut visibility in &mut panels {
        *visibility = if state.visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// 逐项复刻 F3 的顶层组、二级页签、按钮样式和互斥正文切换。
pub(super) fn select_editor_scene_monitor_page(
    mut state: ResMut<EditorSceneMonitorState>,
    bindings: Option<Res<feathrise_fps_display::LodControlPresetBindings>>,
    group_clicks: Query<(&SceneEditorMonitorGroupButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    tab_clicks: Query<(&SceneEditorMonitorTabButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut groups: Query<GroupButtonData, GroupButtonFilter>,
    mut tabs: Query<TabButtonData, TabButtonFilter>,
    mut subtab_rows: Query<&mut Node, SubTabRowFilter>,
    mut pages: Query<PageNodeData, PageNodeFilter>,
    mut labels: Query<(&super::ui::MonitorTabLabel, &mut Text)>,
) {
    if !state.visible {
        return;
    }

    // 映射移除后不能停留在已隐藏的分类页；固定按钮池的其余状态保持原位。
    if !state.page.available(bindings.as_deref()) {
        state.page = state.group.default_page();
        state.force_refresh = true;
    }

    let mut requested_group = None;
    for (button, click) in &group_clicks {
        if click.is_clicked() {
            requested_group = Some(button.group);
        }
    }
    if let Some(group) = requested_group
        && group != state.group
    {
        state.group = group;
        state.page = group.default_page();
        state.force_refresh = true;
    }

    let mut requested_page = None;
    for (tab, click) in &tab_clicks {
        if click.is_clicked() && tab.page.group() == state.group && tab.page.available(bindings.as_deref()) {
            requested_page = Some(tab.page);
        }
    }
    if let Some(page) = requested_page
        && page != state.page
    {
        state.page = page;
        state.force_refresh = true;
    }

    for (button, interaction, mut background, mut border) in &mut groups {
        let selected = button.group == state.group;
        let next_background = BackgroundColor(if selected {
            Color::srgba(0.38, 0.24, 0.10, 0.94)
        } else if *interaction == UiInteraction::Hovered {
            Color::srgba(0.22, 0.16, 0.09, 0.90)
        } else {
            Color::srgba(0.055, 0.044, 0.035, 0.86)
        });
        let next_border = BorderColor::all(if selected {
            Color::srgba(0.88, 0.66, 0.31, 0.92)
        } else {
            Color::srgba(0.52, 0.38, 0.19, 0.52)
        });
        if *background != next_background {
            *background = next_background;
        }
        if *border != next_border {
            *border = next_border;
        }
    }

    for (tab, interaction, mut background, mut border, mut node) in &mut tabs {
        let belongs_to_group = tab.page.group() == state.group && tab.page.available(bindings.as_deref());
        let next_display = resource_monitor_page_display(belongs_to_group);
        if node.display != next_display {
            node.display = next_display;
        }
        let selected = tab.page == state.page;
        let next_background = BackgroundColor(if selected {
            Color::srgba(0.38, 0.24, 0.10, 0.94)
        } else if *interaction == UiInteraction::Hovered {
            Color::srgba(0.22, 0.16, 0.09, 0.90)
        } else {
            Color::srgba(0.055, 0.044, 0.035, 0.86)
        });
        let next_border = BorderColor::all(if selected {
            Color::srgba(0.88, 0.66, 0.31, 0.92)
        } else {
            Color::srgba(0.52, 0.38, 0.19, 0.52)
        });
        if *background != next_background {
            *background = next_background;
        }
        if *border != next_border {
            *border = next_border;
        }
    }

    for (label, mut text) in &mut labels {
        let next = label.0.display_label(bindings.as_deref());
        if text.as_str() != next { **text = next.to_owned(); }
    }

    for mut node in &mut subtab_rows {
        let next_display = resource_monitor_page_display(state.group.has_subtabs());
        if node.display != next_display {
            node.display = next_display;
        }
    }
    for (page, mut node) in &mut pages {
        let next_display = resource_monitor_page_display(page.page == state.page);
        if node.display != next_display {
            node.display = next_display;
        }
    }
}

/// 可见页每半秒读取一次现有缓存与有界元数据；不会扫描R8、R16或世界实体。
pub(super) fn refresh_editor_scene_monitor(
    time: Res<Time<Real>>,
    bindings: Option<Res<feathrise_fps_display::LodControlPresetBindings>>,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    vegetation_preview: Res<VegetationPreviewState>,
    water_preview: Res<WaterPreviewState>,
    weather_state: Res<WeatherState>, // 自动天气当前节点和连续过渡比例。
    weather_override: Res<WeatherDebugOverride>, // F12强制模式与手动预览比例。
    mut state: ResMut<EditorSceneMonitorState>,
    mut texts: Query<(&SceneEditorMonitorPageText, &mut Text)>,
) {
    state.refresh_timer.tick(time.delta());
    if !state.visible {
        return;
    }
    if !state.refresh_timer.just_finished() && !state.force_refresh {
        return;
    }
    state.force_refresh = false;
    let active_page = state.page; // 报告构建期间冻结当前叶子页，避免查询循环重复读取可变状态。
    let report = build_editor_scene_report(
        active_page,
        bindings.as_deref(),
        &editor,
        &session,
        &vegetation_preview,
        &water_preview,
        &weather_state,
        &weather_override,
    ); // 只构建当前页字符串，十七页不会在同一刷新点全部分配。
    for (page, mut text) in &mut texts {
        if page.page == active_page && text.as_str() != report {
            **text = report;
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use feathrise_fps_display::{LodControlContent, LodControlPresetBindings};
    use crate::monitor_resources::{EditorSceneMonitorPage, EditorSceneMonitorGroup};

    #[test]
    fn unbound_pages_hide_and_removed_active_binding_returns_to_generic_page() {
        let mut world = World::new();
        world.insert_resource(EditorSceneMonitorState { visible: true, group: EditorSceneMonitorGroup::Vegetation,
            page: EditorSceneMonitorPage::Wheat, ..Default::default() });
        let mut bindings = LodControlPresetBindings::default();
        bindings.bind(LodControlContent::Wheat, "project_meadow");
        world.insert_resource(bindings);
        for page in EditorSceneMonitorPage::SUBTABS {
            world.spawn((SceneEditorMonitorTabButton { page }, InteractionButton, UiInteraction::None,
                BackgroundColor::default(), BorderColor::default(), Node::default()));
            world.spawn((super::super::ui::MonitorTabLabel(page), Text::new("旧标签")));
        }
        world.run_system_once(select_editor_scene_monitor_page).unwrap();
        assert_eq!(world.resource::<EditorSceneMonitorState>().page, EditorSceneMonitorPage::Wheat);
        world.remove_resource::<LodControlPresetBindings>();
        world.run_system_once(select_editor_scene_monitor_page).unwrap();
        assert_eq!(world.resource::<EditorSceneMonitorState>().page, EditorSceneMonitorPage::OrdinaryGrass);
        let visible = world.query::<(&SceneEditorMonitorTabButton, &Node)>().iter(&world)
            .filter(|(_, node)| node.display != Display::None).map(|(tab, _)| tab.page).collect::<Vec<_>>();
        assert_eq!(visible, vec![EditorSceneMonitorPage::OrdinaryGrass, EditorSceneMonitorPage::Trees]);
        for (label, text) in world.query::<(&super::super::ui::MonitorTabLabel, &Text)>().iter(&world) {
            if label.0 == EditorSceneMonitorPage::OrdinaryGrass { assert_eq!(text.as_str(), "叶片植被"); }
            if label.0 == EditorSceneMonitorPage::Trees { assert_eq!(text.as_str(), "网格植被"); }
        }
    }
}
