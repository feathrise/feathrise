//! 地形页内地表作者层摘要、六槽列表与当前地图删除入口。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // 原生BSN场景沿用编辑器现有布局。
use bevy::text::FontSourceTemplate; // 静态文字使用项目中文字体。
use crate::ui::theme as colors; // 复用植被列表的暗墨底和铜金状态。

use crate::ui::scene::FONT_PATH; // 与其他编辑器页面保持同一字体资源。

use super::components::{
    SurfaceLayerActions, SurfaceLayerButton, SurfaceLayerList, SurfaceLayerListTitle,
}; // 地表列表组件不参与全局预设删除。
use super::types::SurfaceLayerAction; // 局部按钮只发送当前地图的局部动作。

/// 独立动作按钮保留编辑器24像素高度与等宽布局。
fn action_button(label: &str, action: SurfaceLayerAction) -> impl Scene {
    bsn! { // BSN构造固定UI，不随条目数量增加。
        InteractionButton // 原生Interaction消费鼠标。
        SurfaceLayerButton(action) // 分页与地图层删除的明确动作。
        Node { // 紧凑按钮占据操作行的等宽空间。
            height: px(24), // 沿用正式编辑器按钮高度。
            flex_basis: px(0), // 等宽分配而不依赖标签长度。
            flex_grow: 1.0, // 填充操作行可用宽度。
            min_width: px(0), // 窄栏允许收缩。
            justify_content: JustifyContent::Center, // 标签横向居中。
            align_items: AlignItems::Center, // 标签垂直居中。
            border: UiRect::all(px(1)), // 与植被按钮保持铜边。
        } // 尺寸不受地表层总数影响。
        BackgroundColor(colors::SLOT_BG) // 默认使用暗墨槽位背景。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 静态弱铜边。
        Children [( // 每个按钮只有一个短文本子节点。
            Text(label) @feathrise_editor_ui::localization::label_scene(label) // BSN模板直接接收标签，与既有编辑器按钮保持一致。
            TextFont { // 使用已加载的中文字体模板。
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 与全局预设栏同源。
                font_size: px(ui_metrics::FONT_CONTROL), // 三列紧凑按钮完整显示。
            } // 不引入游戏菜单式大字号。
            TextColor(colors::TEXT_PRIMARY) // 默认使用米白文字。
        )] // 子节点数量固定。
    } // 返回可嵌入地形页的静态按钮。
} // 不产生文件写入。

/// 展示当前地图独立作者层，显式新建后立即出现。
pub(crate) fn surface_authoring_panel() -> impl Scene {
    bsn! { // 只增加地表工具必要的作者入口。
        Node { // 摘要、列表与操作行纵向排列。
            width: percent(100), // 与地表预设管理器等宽。
            flex_direction: FlexDirection::Column, // 层列表在摘要下方。
            row_gap: px(3), // 保持编辑器紧凑密度。
            flex_shrink: 0.0, // 超长页面由既有纵向滚动访问。
        } // 容器按实际子内容增高。
        Children [ // 固定三块，不额外创建一套滚动视口。
            ( // 真实数量与页码摘要。
                SurfaceLayerListTitle // 由专用系统更新文字。
                Text("地表作者层 · 等待地图") @feathrise_editor_ui::localization::label_scene("地表作者层 · 等待地图") // 未加载时不伪造地表条目。
                TextFont { // 标题密度与植被作者层一致。
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 项目中文字体。
                    font_size: px(ui_metrics::FONT_CONTROL), // 与相邻地表状态一致。
                } // 字号保持可辨。
                TextColor(colors::TEXT_VALUE) // 摘要沿用米白值颜色。
            ), // 摘要结束。
            ( // 当前页最多六个真实地图层。
                SurfaceLayerList // 系统仅在Added时创建固定行池。
                Node { // 零项不占用空白。
                    display: Display::None, // 就绪后由真实数量启用。
                    width: percent(100), // 完整占据左侧页面宽度。
                    height: px(0), // 共享行池算法决定最终高度。
                    flex_direction: FlexDirection::Column, // 地表层逐行排列。
                    row_gap: px(2), // 与植被六槽行池一致。
                    flex_shrink: 0.0, // 低窗口通过外层滚动访问。
                } // 固定槽数量不受地图规模影响。
            ), // 列表结束。
            ( // 零层时也保留显式新建入口。
                SurfaceLayerActions // 零层时保留新建按钮。
                Node { // 翻页只在超过一页时显示。
                    display: Display::None, // 等待地图作者定义就绪。
                    width: percent(100), // 与列表等宽。
                    column_gap: px(3), // 与植被操作行一致。
                    flex_shrink: 0.0, // 保持固定按钮高度。
                } // 按钮显示由专用回显系统控制。
                Children [ // 全部按钮只影响当前地图。
                    (@action_button("新建层", SurfaceLayerAction::Create)), // 使用已选预设创建独立空白层。
                    (@action_button("前页首项", SurfaceLayerAction::PreviousPage)), // 显式选择前页第一层。
                    (@action_button("后页首项", SurfaceLayerAction::NextPage)), // 显式选择后页第一层。
                    (@action_button("删除所选", SurfaceLayerAction::DeleteSelected)), // 通过后台事务删除完整材质层。
                ] // 不在此处删除全局PBR预设。
            ), // 操作行结束。
        ] // 页面沿用既有左右栏布局。
    } // 列表与预设管理器保持独立职责。
} // 选择、撤销和保存由系统接线。
