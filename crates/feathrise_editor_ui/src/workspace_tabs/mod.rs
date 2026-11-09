//! 中央工作区共用的文字自适应文档标签；以整块填色和字色标记活动项，动作仍由各预设模块处理。
//!
//! 【功能设计】宽度交给Bevy真实字体测量，不按字符数估算，也不平分可用空间；
//! 标题和关闭键是互不嵌套的两个命中区，保留未保存草稿的二次关闭确认。

use crate::InteractionButton;
use crate::colors;
use bevy::prelude::*; // 标签布局、文字和结构化按钮组件。
use bevy::ui::FocusPolicy; // 标题文字穿透，两个按钮各自独占鼠标命中。 // 沿用F12的暗墨底、铜边与米白文字。

mod components;
pub use components::WorkspaceTabButton; // 共享标签不再被各预设正文的普通按钮配色覆盖。

const TAB_HEIGHT: f32 = 28.0; // 文档标签比普通对象行稍高，给标题与关闭命中区留足空间。
const TAB_FONT_SIZE: f32 = 11.5; // 地图与所有预设使用同一可读文字度量。
const TITLE_PADDING: f32 = 6.0; // 标题左右只保留紧凑点击留白。
const CLOSE_SIZE: f32 = 20.0; // 普通关闭键为标签内部的小方形，而不是独立宽按钮。

#[cfg(test)]
mod tests; // 无窗口布局与真实鼠标命中回归，防止缩放、长名称或关闭键串动作。

/// 整条动态内容保持自然宽度，溢出统一交给外层横向滚动容器。
pub fn tab_list_node() -> Node {
    Node {
        width: Val::Auto,                // 宽度等于实际标签总宽，不再压进视口后自行裁切。
        height: Val::Percent(100.0),     // 继续填充固定高度标签条。
        flex_grow: 0.0,                  // 空闲横向空间不分配给标签。
        flex_shrink: 0.0,                // 标签过多时保持内容宽度，供外层计算滚动范围。
        align_items: AlignItems::Center, // 所有标签围绕同一中心线排列。
        column_gap: Val::Px(3.0),        // 标签之间保留现有紧凑间距。
        ..default()                      // 内层不裁切，唯一外层Scroll负责可见范围。
    }
}

/// 标签不绘制活动底线；整个标题与关闭区域使用同一背景，保持高密度工作台的扁平风格。
fn tab_frame_node() -> Node {
    Node {
        width: Val::Auto,                     // 标题、间距和关闭键共同决定自然宽度。
        height: Val::Px(TAB_HEIGHT),          // 窗口与全屏共享同一逻辑高度。
        flex_grow: 0.0,                       // 短标题不会被拉宽。
        flex_shrink: 0.0,                     // 长标题不挤压相邻标签。
        align_items: AlignItems::Center,      // 标题与关闭键垂直居中对齐。
        column_gap: Val::Px(1.0),             // 小关闭键紧靠所属标题，不像下一个标签的按钮。
        padding: UiRect::right(Val::Px(2.0)), // 关闭键与外边框留少量间隙。
        ..default()                           // 行布局保持默认，不添加绝对坐标或额外缩放。
    }
}

/// 永久地图标签同样按文字适配，但没有关闭槽位。
pub fn map_tab_node() -> Node {
    Node {
        padding: UiRect::horizontal(Val::Px(TITLE_PADDING)), // 不可关闭的标签只保留文字两侧留白。
        justify_content: JustifyContent::Center,             // 单个标题在紧凑框内居中。
        ..tab_frame_node() // 地图与预设共用高度、自然宽度及边框规格。
    }
}

/// 单个标题或关闭命中区；继续携带调用方原有动作marker与交互配色组件。
fn spawn_tab_button<B: Bundle>(
    parent: &mut ChildSpawnerCommands, // 两个命中区挂在同一个非按钮标签框内。
    font: &Handle<Font>,               // 使用当前工作区已经加载的项目字体。
    label: impl Bundle,                // 普通标题或支持原位语言切换的关闭确认文字。
    action: B,                         // 保留植被、地表或水体自己的动作类型。
    node: Node,                        // 仅改变视觉尺寸，不改动作分派。
    background: Color,                 // 标题和关闭键继承同一标签基色，悬停按压仍由共享按钮系统反馈。
    text_color: Color,                 // 活动与非活动项以文字明暗区分。
) {
    parent
        .spawn((
            InteractionButton, // 为原有按钮系统提供Interaction。
            WorkspaceTabButton, // 视觉属于整个共享标签；保留业务action但排除正文样式覆写。
            action, // 调用方的原始结构化动作。
            node,   // 无独立边框的标题或小关闭键布局。
            BackgroundColor(background), // 不把活动态冒充瞬时按压色，避免主题反馈层归一后分裂标签底色。
            BorderColor::all(colors::COPPER_BORDER_WEAK), // 保留既有配色查询契约，零边宽不会画第二道框。
            FocusPolicy::Block, // 命中只交给本按钮，不误触同标签的另一动作。
        ))
        .with_child((
            label, // 不截断标题，不按字符个数猜测宽度。
            TextFont {
                font: font.clone().into(),       // 只克隆句柄，不复制字体资源。
                font_size: TAB_FONT_SIZE.into(), // 使用统一紧凑字号。
                ..default()                      // 其余字体设置与当前工作区一致。
            },
            TextLayout::no_wrap(), // 真实字体按单行测量，长名称通过标签条横滚访问。
            TextColor(text_color), // 标题与关闭符号共享活动项文字层级。
            Pickable::IGNORE,     // FocusPolicy只服务旧输入仲裁；现代拾取须明确穿透到所属按钮。
            FocusPolicy::Pass,     // 点击字形区域也能命中所属按钮。
        ));
}

/// 三类预设共用同一紧凑标签，只传入各自原有的激活/关闭动作。
pub fn spawn_preset_tab<B: Bundle>(
    parent: &mut ChildSpawnerCommands, // 中央横向标签内容根。
    font: &Handle<Font>,               // 同一个工作区中文字体。
    label: &str,                       // 包含类型前缀和未保存星号的完整标题。
    activate: B,                       // 原有切换标签动作，不修改草稿。
    close: B,                          // 原有关闭动作，继续执行忙碌保护与二次确认。
    active: bool,                      // 当前活动标签只使用选中填色与文字层级。
    close_confirmation: bool,          // 等待确认时仍显式显示“确认×”，不静默丢弃草稿。
) {
    let background = if active { colors::SELECT_BG } else { colors::BUTTON_DEV_SMALL_NORMAL };
    let text_color = if active { colors::TEXT_TAB_ACTIVE } else { colors::TEXT_TAB_INACTIVE };
    parent
        .spawn((
            tab_frame_node(), // 用内容自然测量替代原168px固定宽度。
            BackgroundColor(background), // 外框、标题与关闭键基色一致，关闭键普通态不再显出独立方块。
            BorderColor::all(colors::COPPER_BORDER_WEAK), // 保留组件合同但不画活动底线或亮边。
            Pickable::IGNORE, // 外框自身不参与现代拾取，两个子按钮仍各自命中。
            FocusPolicy::Pass, // 外框不是第三个按钮，不参与激活或关闭分派。
        ))
        .with_children(|row| {
            spawn_tab_button(
                row,      // 标题和关闭键是同级节点，点击不会父子联动。
                font,     // 与关闭键共用字体。
                Text::new(label), // 原样显示完整名称。
                activate, // 只触发当前标签激活。
                Node {
                    width: Val::Auto,                                    // 由文字本身决定标题宽度。
                    height: Val::Percent(100.0), // 标题占满外框内高，便于点击。
                    flex_shrink: 0.0,            // 不缩窄字形或产生换行。
                    padding: UiRect::horizontal(Val::Px(TITLE_PADDING)), // 仅保留必要左右留白。
                    align_items: AlignItems::Center, // 标题文字与关闭符号共用中心线。
                    justify_content: JustifyContent::Center, // 文字在自己的自然宽度槽内居中。
                    ..default()                  // 不给标题按钮追加独立边框。
                },
                background, // 标题与所在标签保持同一填色，仍有独立悬停和按压反馈。
                text_color,
            );
            spawn_tab_button(
                row,                                            // 关闭键位于所属标签框内右侧。
                font,                                           // 保证关闭字形可以正常显示。
                crate::localization::editor_label(if close_confirmation { "确认×" } else { "×" }),
                close,                                          // 不改现有关闭事务语义。
                Node {
                    width: Val::Auto,               // 确认文案可按文字增宽，普通×不会占据34px。
                    min_width: Val::Px(CLOSE_SIZE), // 普通关闭键仍有明确可点击面积。
                    height: Val::Px(CLOSE_SIZE),    // 小关闭键上下留白，不再冒充独立标签。
                    flex_shrink: 0.0,               // 不让长标题挤掉关闭命中区。
                    padding: UiRect::horizontal(Val::Px(3.0)), // ×和确认文案都保留少量内边距。
                    align_items: AlignItems::Center, // 符号在方形命中区垂直居中。
                    justify_content: JustifyContent::Center, // 符号水平居中。
                    border_radius: BorderRadius::all(Val::Px(2.0)), // 悬停底色保持小圆角，不画额外铜框。
                    ..default() // 外框已经承担边界，仅保留按钮悬停反馈。
                },
                if close_confirmation { colors::DANGER_BG } else { background }, // 仅显式关闭确认强调危险，普通×融入当前标签。
                if close_confirmation { colors::DANGER_TEXT } else { text_color },
            );
        });
}
