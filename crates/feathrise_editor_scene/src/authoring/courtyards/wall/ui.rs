use super::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::InteractionDisabled;
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
    Node { flex_basis: px(0), flex_grow: 1.0, min_width: px(0), min_height: px(26),
        padding: UiRect::horizontal(px(4)), align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)) }
    BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [ Text(label) @label_scene(label) Pickable::IGNORE
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY) ] }
}

fn pair(left: (&'static str, Action), right: (&'static str, Action)) -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), column_gap: px(4) }
        Children [@button(left.0, left.1)-- @button(right.0, right.1)]
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! { Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(5) }
    Children [
        @crate::ui::context_help::section_header("沿线铺设围墙", "依次点选起点、转角和终点。墙段按连接点和实际长度对齐，门楼用金色标出。确认后可整笔撤销。")--
        Summary Text("") TextColor(colors::TEXT_VALUE)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(feathrise_editor_core::FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }--
        @pair(("切换墙段", Action::Wall), ("切换门楼", Action::Gate))--
        @pair(("开始绘制", Action::Start), ("撤回上一点", Action::UndoPoint))--
        @pair(("指定门楼位置", Action::ChooseGate), ("移除门楼", Action::RemoveGate))--
        @pair(("确认铺设", Action::Confirm), ("取消铺设", Action::Cancel))
    ] }
}

pub(super) fn present(
    mut commands: Commands,
    tool: Res<WallTool>,
    batch: Res<CourtyardAuthoringState>,
    page: Res<BuildingSidebarPage>,
    locale: Res<feathrise_editor_ui::localization::EditorLocale>,
    mut text: Query<&mut Text, With<Summary>>,
    mut buttons: Query<(
        Entity,
        &Action,
        Has<InteractionDisabled>,
        &mut BackgroundColor,
        &Children,
    )>,
    mut captions: Query<&mut TextColor>,
) {
    if *page != BuildingSidebarPage::Wall {
        return;
    }
    // 保留四组动作的位置，路线和门楼达到相应阶段才接受输入。
    let armed = tool.owner.is_some();
    let pending = batch.operation_pending();
    for (entity, action, disabled, mut background, children) in &mut buttons {
        let enabled = (!pending || matches!(action, Action::Cancel))
            && match action {
                Action::Wall => tool.wall.is_some(),
                Action::Gate => tool.door.is_some(),
                Action::Start => tool.wall.is_some() && !armed,
                Action::UndoPoint => !tool.points.is_empty(),
                Action::ChooseGate => tool.plan.count > 0 && tool.door.is_some(),
                Action::RemoveGate => tool.gate.is_some(),
                Action::Confirm => armed && tool.plan.count > 0,
                Action::Cancel => armed,
            };
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let primary = matches!(action, Action::Start | Action::Confirm);
        background.set_if_neq(BackgroundColor(if enabled && primary {
            colors::SELECT_BG
        } else {
            colors::SLOT_BG
        }));
        for child in children.iter() {
            if let Ok(mut caption) = captions.get_mut(child) {
                caption.set_if_neq(TextColor(if enabled {
                    colors::TEXT_PRIMARY
                } else {
                    colors::TEXT_MUTED
                }));
            }
        }
    }
    if !(tool.is_changed() || page.is_changed() || locale.is_changed()) {
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
        format!("Section: {wall}\nGate: {gate}\n{points} points · {count} modules")
    } else {
        format!("墙段 {wall}\n门楼 {gate}\n{points} 个控制点 · {count} 个建筑模块")
    };
    for mut label in &mut text {
        if label.as_str() != summary {
            **label = summary.clone();
        }
    }
}
