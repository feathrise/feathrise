use super::*;
use bevy::text::FontSourceTemplate;
use feathrise_editor_ui::{InteractionButton, colors, localization::label_scene, metrics};

#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    Wall,
    Gate,
    Start,
    UndoPoint,
    ChooseGate,
    RemoveGate,
    Confirm,
    Cancel,
}
#[derive(Component, Clone, Default)]
pub(super) struct Summary;

fn button(label: &'static str, action: Action) -> impl Scene {
    bsn! { InteractionButton ~{action}
    Node { height: px(24), padding: UiRect::horizontal(px(4)), align_items: AlignItems::Center, border: UiRect::all(px(1)) }
    BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [ Text(label) @label_scene(label) Pickable::IGNORE
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY) ] }
}

pub(crate) fn panel() -> impl Scene {
    bsn! { Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(5) }
    Children [
        @crate::ui::context_help::section_header("沿线铺设围墙", "依次点选起点、转角和终点。墙段按连接点和实际长度对齐，门楼用金色标出。确认后可整笔撤销。")--
        Summary Text("") TextColor(colors::TEXT_VALUE)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }--
        Node { width: percent(100), flex_wrap: FlexWrap::Wrap, column_gap: px(3), row_gap: px(3) }
            Children [@button("切换墙段", Action::Wall)-- @button("切换门楼", Action::Gate)--
                @button("开始绘制", Action::Start)-- @button("撤回上一点", Action::UndoPoint)--
                @button("指定门楼位置", Action::ChooseGate)-- @button("移除门楼", Action::RemoveGate)--
                @button("确认铺设", Action::Confirm)-- @button("取消铺设", Action::Cancel)]
    ] }
}

pub(super) fn present(
    tool: Res<WallTool>,
    page: Res<BuildingSidebarPage>,
    locale: Res<feathrise_editor_ui::localization::EditorLocale>,
    mut text: Query<&mut Text, With<Summary>>,
) {
    if *page != BuildingSidebarPage::Wall
        || !(tool.is_changed() || page.is_changed() || locale.is_changed())
    {
        return;
    }
    let english = *locale == feathrise_editor_ui::localization::EditorLocale::English;
    let wall = tool
        .wall
        .as_ref()
        .map_or(if english { "None" } else { "无可用预设" }, |m| {
            m.preset.label.as_str()
        });
    let gate = tool
        .door
        .as_ref()
        .map_or(if english { "None" } else { "无匹配门楼" }, |m| {
            m.preset.label.as_str()
        });
    let points = tool.points.len();
    let count = tool.plan.count;
    let summary = if english {
        format!("Section: {wall}   Gate: {gate}\n{points} points · {count} modules")
    } else {
        format!("墙段：{wall}　门楼：{gate}\n{points} 个控制点 · {count} 个建筑模块")
    };
    for mut label in &mut text {
        if label.as_str() != summary {
            **label = summary.clone();
        }
    }
}
