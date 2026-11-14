//! 在官方命中之后冻结按下来源，只有原按钮内释放才产生一次普通动作。

use crate::UiInteraction;
use crate::InteractionButton;
use super::{components::EditorButtonClick, resources::*};
use bevy::{ecs::system::SystemParam, prelude::*, ui::ComputedStackIndex, window::PrimaryWindow};

#[derive(SystemParam)]
pub struct EditorButtonInput<'w, 's> {
    capture: ResMut<'w, ButtonCapture>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    keyboard: Option<Res<'w, ButtonInput<KeyCode>>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    buttons: Query<
        'w,
        's,
        (
            Entity,
            &'static UiInteraction,
            &'static mut EditorButtonClick,
            Option<&'static ChildOf>,
            Option<&'static ComputedUiTargetCamera>,
            Option<&'static ComputedStackIndex>,
            Has<bevy::ui::InteractionDisabled>,
            Option<&'static bevy::ui::RelativeCursorPosition>,
        ),
        With<InteractionButton>,
    >,
    nodes: Query<
        'w,
        's,
        (
            Option<&'static Node>,
            Option<&'static ComputedNode>,
            Option<&'static InheritedVisibility>,
            Option<&'static ChildOf>,
        ),
    >,
}

/// 固定深度的祖先检查仅用于当前按下目标；不逐帧遍历整棵工作台或场景。
fn visible(
    entity: Entity,
    nodes: &Query<(
        Option<&Node>,
        Option<&ComputedNode>,
        Option<&InheritedVisibility>,
        Option<&ChildOf>,
    )>,
) -> bool {
    let mut cursor = Some(entity);
    for depth in 0..128 {
        let Some(entity) = cursor else {
            return true;
        };
        let Ok((node, computed, visibility, parent)) = nodes.get(entity) else {
            return false;
        };
        // 非Node包装祖先或零尺寸溢出容器都可能合法；只有按钮本身必须具有实际命中面积。
        if node.is_some_and(|node| node.display == Display::None)
            || (depth == 0 && computed.is_none_or(ComputedNode::is_empty))
            || visibility.is_some_and(|visibility| !visibility.get())
        {
            return false;
        }
        cursor = parent.map(ChildOf::parent);
    }
    false // 非法深层或循环失败关闭，不给隐藏按钮补发动作。
}

pub fn update_editor_button_clicks(mut input: EditorButtonInput) {
    if let Some(entity) = input.capture.released.take()
        && let Ok((_, _, mut click, ..)) = input.buttons.get_mut(entity)
    {
        click.clicked = false;
    }
    let focused = input.windows.iter().all(|window| window.focused);
    let canceled = input
        .keyboard
        .as_ref()
        .is_some_and(|keys| keys.just_pressed(KeyCode::Escape));
    let Some(mouse) = input.mouse.as_deref() else {
        input.capture.held = None;
        return;
    };
    if !focused || canceled {
        input.capture.held = None;
        return;
    }

    if mouse.just_pressed(MouseButton::Left) {
        // 只在真实按下帧找顶层候选；其余帧仅读取唯一捕获实体。按在空白处后移入不会获权。
        input.capture.held = input
            .buttons
            .iter_mut()
            .filter(|(entity, interaction, _, _, _, _, disabled, cursor)| {
                !*disabled
                    && **interaction == UiInteraction::Pressed
                    && cursor.is_none_or(|cursor| cursor.cursor_over)
                    && visible(*entity, &input.nodes)
            })
            .max_by_key(|(_, _, _, _, _, stack, _, _)| stack.map_or(0, |stack| stack.0))
            .map(
                |(entity, _, click, parent, camera, _, _, _)| CapturedButton {
                    entity,
                    parent: parent.map(ChildOf::parent),
                    camera: camera.and_then(ComputedUiTargetCamera::get),
                    generation: click.generation,
                },
            );
    }
    let Some(captured) = input.capture.held else {
        return;
    };
    let Ok((_, interaction, mut click, parent, camera, _, disabled, cursor)) =
        input.buttons.get_mut(captured.entity)
    else {
        input.capture.held = None;
        return;
    };
    let valid = !disabled
        && visible(captured.entity, &input.nodes)
        && captured.parent == parent.map(ChildOf::parent)
        && captured.camera == camera.and_then(ComputedUiTargetCamera::get)
        && captured.generation == click.generation;
    if !valid {
        input.capture.held = None;
        return;
    }

    if mouse.just_released(MouseButton::Left) && !mouse.pressed(MouseButton::Left) {
        // Bevy同帧快点仍保留Pressed；普通释放必须真实命中原按钮，移出释放与遮挡均取消。
        // 若同帧先松开旧键再按下新键，终态仍按住，必须等新一轮真实释放。
        if cursor.is_none_or(|cursor| cursor.cursor_over)
            && (*interaction == UiInteraction::Hovered
                || (mouse.just_pressed(MouseButton::Left) && *interaction == UiInteraction::Pressed))
        {
            click.clicked = true;
            input.capture.released = Some(captured.entity);
        }
        input.capture.held = None;
    } else if !mouse.pressed(MouseButton::Left)
        || (*interaction != UiInteraction::Pressed && cursor.is_none_or(|cursor| cursor.cursor_over))
    {
        // 现代拾取在移出时投影None：仍按住且确实位于原按钮外只保留原捕获，移回后可合法松开。
        // 原按钮内的业务reset仍取消；换绑generation、隐藏、禁用和失焦已由上方身份门拒收。
        input.capture.held = None;
    }
}
