//! 小场景只写原生滚动偏移；不移动光标、选区或文档草稿。

use feathrise_editor_ui::UiInteraction;
use bevy::{
    ecs::system::SystemParam,
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    text::{EditableText, TextLayoutInfo},
    ui::RelativeCursorPosition,
    window::PrimaryWindow,
};

use super::{
    super::{components::*, resources::*, types::*},
    math::OverviewMetrics,
};
use crate::{
    resources::*,
    ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
};

#[derive(SystemParam)]
pub(in super::super) struct InputContext<'w, 's> {
    editor: Res<'w, SceneEditorState>,
    workspace: Res<'w, VegetationPresetEditorState>,
    guard: Res<'w, SceneEditorNumericScrubMouseGuard>,
    numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
    settings: Option<Res<'w, SceneEditorSettingsState>>,
    browser: Option<Res<'w, SceneBrowserState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    external: Option<Res<'w, crate::TerrainExternalChangeState>>,
    scene_drag: Option<Res<'w, SceneBrowserDragState>>,
    windows: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>,
}

pub(in super::super) fn handle_input(
    mut wheels: MessageReader<MouseWheel>,
    mouse: Res<ButtonInput<MouseButton>>,
    documents: Res<DocumentState>,
    mut state: ResMut<DocumentMinimapState>,
    context: InputContext,
    mut sources: Query<(
        Entity,
        &DocumentSource,
        &ComputedNode,
        &TextLayoutInfo,
        &mut EditableText,
    )>,
    rails: Query<(&ComputedNode, &RelativeCursorPosition, &UiInteraction), With<DocumentMinimapRail>>,
) {
    state.mouse_owned = false;
    state.wheel_owned = false;
    let window = context.windows.single().ok();
    // 即使被模态或鼠标门阻塞也消费消息，恢复时不重放旧滚轮。
    let delta: f32 = wheels
        .read()
        .filter(|event| {
            window.is_some_and(|(entity, _)| event.window == entity) && event.y.is_finite()
        })
        .map(|event| {
            -event.y
                * match event.unit {
                    MouseScrollUnit::Line => 27.0,
                    MouseScrollUnit::Pixel => 1.0,
                }
        })
        .sum();
    let enabled = state.shown
        && documents.visible
        && documents.in_center()
        && context.workspace.active_tab == PresetWorkspaceTab::Document
        && context.editor.open
        && !context.editor.close_requested
        && !context.editor.new_scene_dialog_open
        && context.editor.pending_scene_transition.is_none()
        && context.settings.as_ref().is_none_or(|item| !item.open)
        && context
            .browser
            .as_ref()
            .is_none_or(|item| item.dialog.is_none())
        && context
            .deletion
            .as_ref()
            .is_none_or(|item| item.confirmation.is_none())
        && context
            .external
            .as_ref()
            .is_none_or(|item| !item.modal_open())
        && context
            .numeric
            .as_ref()
            .is_none_or(|item| item.entity.is_none())
        && context
            .scene_drag
            .as_ref()
            .is_none_or(|item| !item.blocks_other_mouse())
        && window.is_some_and(|(_, window)| window.focused)
        && (state.drag.is_some() || !context.guard.blocked());
    let source = sources.single_mut().ok();
    let rail = rails.single().ok();
    let doc = documents.current().filter(|doc| doc.mode == ViewMode::Edit);
    let valid = enabled
        && doc.is_some()
        && source.as_ref().is_some_and(|(_, source, _, _, _)| {
            source.key.as_ref().is_some_and(|key| {
                let doc = doc.unwrap();
                key.path == doc.path && key.generation == doc.generation
            })
        });
    if !valid || rail.is_none() {
        state.mouse_owned = state.drag.take().is_some(); // 取消帧仍经过公共静默释放门。
        return;
    }
    let doc = doc.unwrap();
    let (entity, _, node, layout, mut scroll) = source.unwrap();
    let (rail, cursor, interaction) = rail.unwrap();
    let scale = node.inverse_scale_factor().recip();
    let Some(metrics) = OverviewMetrics::new(
        layout.size,
        node.content_box().size(),
        rail.size(),
        scroll.viewport.offset.y,
        scale,
    ) else {
        state.mouse_owned = state.drag.take().is_some();
        return;
    };
    if state.drag.as_ref().is_some_and(|drag| {
        drag.source != entity
            || drag.document.path != doc.path
            || drag.document.generation != doc.generation
            || drag.revision != doc.revision
    }) {
        state.drag = None;
        state.mouse_owned = true;
        return;
    }
    // Bevy相对坐标以轨道中心为0、顶部为-0.5；先转成距顶部的物理像素。
    // 越界拖动保留原坐标，只在全文滚动范围内夹取，避免上半段点击和拖动被锁在开头。
    let pointer = cursor
        .normalized
        .map(|position| (position.y + 0.5) * rail.size().y)
        .filter(|y| y.is_finite());
    if let Some(grab_ratio) = state.drag.as_ref().map(|drag| drag.grab_ratio) {
        state.mouse_owned = true;
        if mouse.pressed(MouseButton::Left) {
            if let Some(y) = pointer {
                scroll.viewport.offset.y = metrics.scroll_for_top(y - grab_ratio * metrics.thumb_height);
            }
        } else {
            state.drag = None;
        }
        // 拖动期间不再叠加滚轮，主键及release帧均由同一手势消费。
        state.wheel_owned = delta != 0.0;
        return;
    }
    if !cursor.cursor_over() || *interaction == UiInteraction::None {
        return;
    }
    if delta.is_finite() && delta != 0.0 {
        scroll.viewport.offset.y = (scroll.viewport.offset.y + delta * scale).clamp(0.0, metrics.maximum);
        state.wheel_owned = true; // 到首尾也消费，不能继续滚动资产外栏。
    }
    if !mouse.just_pressed(MouseButton::Left)
        || [MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .any(|button| mouse.pressed(button) || mouse.just_released(button))
    {
        return;
    }
    let Some(y) = pointer else {
        return;
    };
    let grab_ratio = if (metrics.thumb_top..=metrics.thumb_top + metrics.thumb_height).contains(&y)
    {
        ((y - metrics.thumb_top) / metrics.thumb_height).clamp(0.0, 1.0)
    } else {
        scroll.viewport.offset.y = metrics.scroll_for_top(y - metrics.thumb_height * 0.5);
        0.5
    };
    state.drag = Some(MinimapDrag {
        document: doc.source_key(),
        source: entity,
        revision: doc.revision,
        grab_ratio,
    });
    state.mouse_owned = true;
}
