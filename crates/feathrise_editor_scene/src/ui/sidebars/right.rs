//! F12右侧独立属性和动态工具页；文件属性与场景属性互斥复用同一“属性”页签。

use crate::ui::theme as colors;
use bevy::prelude::*; // BSN Scene、UI布局、滚动位置与显示切换。
use bevy::ui::RelativeCursorPosition; // 每页独立判断滚轮悬停。 // 右栏复用水墨铜木配色。

use crate::components::*; // 资产行池、环境摘要和滚动marker。
use crate::constants::EDITOR_INSPECTOR_WIDTH;
use feathrise_editor_weights_model::WeightPanelScenes;
use crate::types::EditorPane; // 布局标记不改变既有作者动作协议。

use super::super::asset_preview::asset_properties_page; // 目录位于左侧；所选文件与唯一预览留在右侧属性。
use super::{
    EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarPage,
    sidebar_tab_button,
}; // 右页身份、帮助折叠和上下文marker。

/// 属性页按任务显示所选对象或独立样品；场景树已移至左侧导航。
pub(crate) fn scene_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Scene))
        Node {
            display: Display::Flex, // 首次打开显示场景页。
            width: percent(100),
            flex_grow: 1.0,
            min_width: px(0),
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::top(px(3)),
            overflow: Overflow::clip(),
        }
        Children [

                SceneEditorSceneContext
                Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column }
                Children [
                    crate::ui::scene_inspector::InspectorBody
                        SceneEditorRightPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
                        Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), overflow: Overflow::scroll_y() }
                        Children [
                    @crate::ui::scene_inspector::scene_inspector_overview()--
                    @crate::ui::terrain_objects::properties_panel()--
                    crate::game_authoring::DomainInspectorHost Node { width:percent(100),min_width:px(0),flex_direction:FlexDirection::Column }--
                    @crate::ui::water_scene::water_properties_panel()--
                    @crate::ui::vegetation_scene::vegetation_properties_panel()--
                    @crate::ui::subscenes::properties_panel()--
                    @crate::ui::scene_objects::properties_panel()--
                    @crate::ui::scene_inspector::scene_inspector_properties()-- // 原变换与网格水树保持实体，只在当前稳定对象下展开。
                    @crate::ui::scene_inspector::scene_inspector_feedback()
                        ]
                ]
            --
            crate::game_authoring::DomainPreviewHost Node { width:percent(100),min_width:px(0),flex_direction:FlexDirection::Column }--
            @crate::ui::water_preset_editor::water_live_preview_host() // 水体编辑使用场景页最下方独立10m×50m样品河面。
        ]
    }
}

/// 属性与动态工具页签独立；点击文件或对象明确切换属性来源，工具表单保留滚动与缓冲。
pub(crate) fn right_sidebar(
    scenes: &feathrise_editor_documents_model::DocumentScenes,
    weight_scenes: &WeightPanelScenes,
    animation_scenes: &feathrise_editor_animation_model::assembly::AnimationScenes,
) -> impl Scene + use<> {
    bsn! {
        SceneEditorPane(EditorPane::Inspector)
        Node {
            width: px(EDITOR_INSPECTOR_WIDTH), // 预览和属性共享可调宽度，不创建第二个视图。
            height: percent(100),
            flex_shrink: 0.0, // 右栏不因视口宽度竞争而缩窄或换行。
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(6)),
            border: UiRect::left(px(1)),
        }
        BackgroundColor(colors::PANEL_BG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [

                Node {
                    width: percent(100), // 一级导航分区占满右栏正文宽度。
                    flex_shrink: 0.0, // 三个工作页签固定在滚动正文之外。
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    padding: UiRect::all(px(2)), // 页签直接表达右栏职责，减少重复标题和留白。
                    margin: UiRect::bottom(px(3)), // 导航与右栏活动页正文分隔。
                    border: UiRect::bottom(px(1)), // 用细分隔线与页面分层，避免导航套多重铜框。
                }
                BackgroundColor(colors::PANEL_BG_STRONG) // 一级页签与页内资产工具形成墨色差异。
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [

                        Node {
                            width: percent(100), // 属性、资产和当前工具严格等宽占满右栏。
                            height: px(24), // 与左侧导航、顶部模块统一紧凑高度。
                            flex_shrink: 0.0, // 正文增长时不压扁固定页签行。
                            column_gap: px(2), // 页签之间使用统一细缝。
                        }
                        Children [
                            @sidebar_tab_button("属性", EditorSidebarTab::Right(EditorRightSidebarTab::Scene))--
                            @sidebar_tab_button("网格", EditorSidebarTab::Right(EditorRightSidebarTab::Tools))
                        ]

                ]
            --
            @scene_page()--
            @super::left::tools_page()--
            @super::super::asset_preview::rigging::scene::sidebar_page()--
            @super::super::asset_preview::weights::panel::sidebar_page(weight_scenes)--
            @asset_properties_page(scenes, weight_scenes, animation_scenes)--
            @crate::ui::scene_sidebar::scene_properties_page()--
            @super::super::environment::environment_page() // 三种环境作用域在唯一滚动页内独立显示。
        ]
    }
}
