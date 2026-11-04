//! 传送双列表动态行实体的生成、增量回显和交互样式。

mod current; // 当前地图源点列表及整个面板显隐。
mod spawn; // 两套列表只在真实条目数量变化时增减行实体。
mod target; // 地图树目标落点列表、关系摘要与加载态。

use bevy::prelude::*; // 共享布局常量与交互配色返回值。

pub(crate) use current::{
    sync_current_portal_row_interactions, sync_current_portal_rows, sync_portal_selection_panel,
};
pub(crate) use spawn::{PortalRowEntityRevision, spawn_portal_list_rows};
pub(crate) use target::{sync_target_portal_row_interactions, sync_target_portal_rows};

pub(super) const PORTAL_ROW_HEIGHT: f32 = feathrise_editor_ui::metrics::ROW_HEIGHT; // 24逻辑像素单行，窄栏不挤占下一标签。
const PORTAL_ROW_GAP: f32 = 2.0; // 两个真实端点之间保留可辨间距。
const PORTAL_LIST_PADDING_Y: f32 = 2.0; // 非空列表上下各保留两像素安全边距。

/// 让传送列表高度覆盖全部真实条目；列表整体溢出由左栏既有滚动视口承接。
pub(super) fn fit_portal_list(node: &mut Node, rows: usize) {
    node.display = if rows == 0 {
        Display::None // 零项不制造父级行距或空白高度。
    } else {
        Display::Flex // 一项起让完整列表参与左栏纵向布局。
    };
    node.height = px(if rows == 0 {
        0.0 // 空列表必须彻底退出布局。
    } else {
        rows as f32 * PORTAL_ROW_HEIGHT
            + rows.saturating_sub(1) as f32 * PORTAL_ROW_GAP
            + PORTAL_LIST_PADDING_Y * 2.0 // 高度随真实条目数增长，不再按四行封顶。
    });
    node.padding = if rows == 0 {
        UiRect::all(px(0)) // 隐藏时清除历史非空内边距。
    } else {
        UiRect::vertical(px(PORTAL_LIST_PADDING_Y)) // 非空列表保留上下点击安全距离。
    };
}

/// 返回行在按下、悬停、选中和普通状态下的统一背景色。
pub(super) fn portal_row_background(interaction: Interaction, selected: bool) -> Color {
    use crate::ui::theme as colors; // 复用项目统一水墨铜金交互层级。

    match (interaction, selected) {
        (Interaction::Pressed, _) => colors::SELECT_BG_PRESSED, // 按下反馈优先覆盖稳定选择。
        (Interaction::Hovered, _) => colors::SELECT_BG_HOVER,   // 悬停明确该行可作为操作对象。
        (Interaction::None, true) => colors::SELECT_BG,         // 已选端点松开后保留金棕底。
        (Interaction::None, false) => colors::SLOT_BG,          // 普通行回到低饱和暗墨槽位。
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin; // 最小测试应用仍通过正式Assets资源初始化路径。
    use feathrise_map_content::{MapManifestAsset, MapPortalsAsset}; // 目标Presentation的两类资产参数。

    use super::*; // 注册产品中的五个真实Presentation系统以捕获Bevy查询冲突。
    use crate::authoring::EditorAuthoringSession; // 空草稿是有效的加载边界测试输入。
    use crate::resources::{MapBrowserState, MapEditorState}; // 提供工具、地图树与双列表选择会话态。
    use crate::types::EditorTool; // 强制系统进入传送工具分支。

    #[test]
    fn portal_list_layout_keeps_every_real_row() {
        let mut node = Node::default(); // 使用真实Bevy节点验证完整列表高度。
        fit_portal_list(&mut node, 43); // 截图中的四十三项不得再被四行容量钳制。
        assert_eq!(node.display, Display::Flex); // 非空列表必须参与左栏滚动布局。
        assert_eq!(node.height, px(1120)); // 43×24px行高、42×2px间距和上下各2px内边距。
    }

    #[test]
    fn portal_presentation_queries_are_disjoint_in_a_real_schedule() {
        let mut app = App::new(); // B0001只会在系统参数真正初始化时暴露，单纯编译不足以覆盖。
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_resource::<MapEditorState>() // 提供面板显隐与两套选择身份。
            .init_resource::<PortalRowEntityRevision>() // Presentation系统读取动态行结构修订。
            .init_resource::<MapBrowserState>() // 空高亮应安全显示目标地图提示。
            .init_resource::<EditorAuthoringSession>() // 空草稿应安全显示当前地图等待态。
            .init_asset::<MapManifestAsset>() // 目标默认出生点读取正式清单集合。
            .init_asset::<MapPortalsAsset>() // 目标具体端点读取正式传送图集合。
            .add_systems(
                Update,
                (
                    sync_portal_selection_panel,
                    sync_current_portal_rows,
                    sync_target_portal_rows,
                    sync_current_portal_row_interactions,
                    sync_target_portal_row_interactions,
                )
                    .chain(), // 使用真实顺序运行一次完整双列表Presentation。
            );
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.open = true; // 允许两个列表进入查询与格式化分支。
            editor.tool = EditorTool::Portal; // 面板必须处于本功能真实激活状态。
        }
        app.update(); // 任一可变Node、Text或Background查询重叠都会在此抛出B0001。
    }
}
