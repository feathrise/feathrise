//! 真实悬停延迟、只读阻塞条件和布局后避让；不注册动作、焦点或鼠标捕获。

use feathrise_editor_ui::UiInteraction;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use feathrise_editor_core::EditorSettings;

use super::components::*;
use super::helpers::{tooltip_copy, tooltip_position, tooltip_title, unavailable_reason};
use super::resources::EditorTooltipState;
use super::types::TooltipAction;
use crate::authoring::TerrainExternalChangeState;
use crate::components::{
    SceneEditorActionButton, SceneEditorLayoutButton, SceneEditorRoot, SceneEditorSettingsButton,
};
use crate::resources::{
    SceneBrowserState, SceneDeletionTask, SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState,
    SceneEditorState, SceneEditorViewportState,
};
use crate::types::EditorSettingsAction;
use crate::ui::action_availability::EditorActionInputs;

const HOVER_DELAY_SECONDS: f64 = 0.35;

pub(super) fn setup_header_targets(
    mut commands: Commands,
    headers: Query<&Children, Added<TooltipHeader>>,
    buttons: Query<(
        Option<&SceneEditorActionButton>,
        Option<&SceneEditorLayoutButton>,
        Option<&SceneEditorSettingsButton>,
    )>,
) {
    for children in &headers {
        for child in children {
            let Ok((editor, layout, settings)) = buttons.get(*child) else {
                continue;
            };
            let action = editor
                .map(|button| TooltipAction::Editor(button.0))
                .or_else(|| layout.map(|button| TooltipAction::Layout(button.0)))
                .or_else(|| {
                    settings
                        .filter(|button| button.0 == EditorSettingsAction::Open)
                        .map(|_| TooltipAction::Settings)
                });
            if let Some(action) = action {
                commands.entity(*child).insert(EditorTooltipTarget(action));
            }
        }
    }
}

#[derive(SystemParam)]
pub(super) struct TooltipContext<'w> {
    editor: Res<'w, SceneEditorState>,
    settings: Res<'w, EditorSettings>,
    availability: EditorActionInputs<'w>,
    guard: Res<'w, SceneEditorNumericScrubMouseGuard>,
    settings_modal: Option<Res<'w, SceneEditorSettingsState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    folders: Option<Res<'w, SceneBrowserState>>,
    conflict: Option<Res<'w, TerrainExternalChangeState>>,
    viewport: Option<Res<'w, SceneEditorViewportState>>,
    mesh_edit: Option<Res<'w, crate::authoring::mesh_edit::MeshEditState>>,
    launch: Option<Res<'w, feathrise_editor_core::ProjectLaunchSettings>>,
    interruption: Option<Res<'w, feathrise_editor_core::ProjectLaunchInterruption>>,
}

impl TooltipContext<'_> {
    fn suppressed(&self) -> bool {
        !self.editor.open
            || self.editor.close_requested
            || self.editor.new_scene_dialog_open
            || self.editor.scene_transition_in_progress
            || self.editor.pending_scene_transition.is_some()
            || self.editor.scene_switch_confirmation.is_some()
            || self.guard.blocked()
            || self
                .settings_modal
                .as_ref()
                .is_some_and(|state| state.open || state.owns_mouse_this_frame)
            || self
                .deletion
                .as_ref()
                .is_some_and(|state| state.confirmation.is_some())
            || self
                .folders
                .as_ref()
                .is_some_and(|state| state.dialog.is_some())
            || self
                .conflict
                .as_ref()
                .is_some_and(|state| state.modal_open())
    }

    fn availability(&self) -> crate::ui::action_availability::EditorActionAvailability {
        self.availability
            .snapshot(&self.editor, self.viewport.as_deref())
    }
}

pub(super) fn sync_tooltip(
    time: Res<Time<Real>>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    context: TooltipContext,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    targets: Query<(
        Entity,
        &EditorTooltipTarget,
        &UiInteraction,
        &ComputedNode,
        &InheritedVisibility,
    )>,
    mut state: ResMut<EditorTooltipState>,
    mut popups: Query<(&mut Node, &mut Visibility), (With<TooltipPopup>, With<TooltipElement>)>,
    mut texts: Query<(&TooltipText, &mut Text, &mut Node), Without<TooltipPopup>>,
) {
    let Ok((mut popup, mut visibility)) = popups.single_mut() else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if state.scene_id != context.editor.selected_scene_id {
        state.scene_id = context.editor.selected_scene_id.clone();
        state.target = None;
    }
    let suppressed = context.suppressed()
        || windows
            .single()
            .is_ok_and(|window| !window.focused || window.physical_cursor_position().is_none())
        || [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .any(|button| mouse.pressed(button) || mouse.just_released(button));
    // Interaction来自正式命中系统，尊重前方遮罩与工具条裁切；禁用按钮也可提供只读悬停说明。
    let hovered = (!suppressed)
        .then(|| {
            targets.iter().find(|(_, _, interaction, node, visible)| {
                **interaction == UiInteraction::Hovered
                    && visible.get()
                    && node.size().min_element() > 0.0
            })
        })
        .flatten();
    let target = hovered.map(|(entity, ..)| entity);
    if state.target != target {
        state.target = target;
        state.since = now;
        state.reason = None;
        if popup.display != Display::None {
            popup.display = Display::None;
        }
    }
    let Some((_, target, ..)) = hovered.filter(|_| now - state.since >= HOVER_DELAY_SECONDS) else {
        if popup.display != Display::None {
            popup.display = Display::None;
        }
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let first_show = popup.display != Display::Flex;
    let mut copy = tooltip_copy(target.0, context.mesh_edit.as_ref().is_some_and(|state| state.active()));
    if target.0 == TooltipAction::Editor(crate::types::EditorAction::Close)
        && let Some(launch) = context.launch.as_ref()
    {
        copy.title = launch.action_label();
        copy.description = if launch.preview {
            "预览当前工程的内存草稿；点击预览悬浮工具条的“返回”恢复原作者现场，不会隐式保存。Debug/Release决定工程构建方式。"
        } else {
            "在独立窗口运行已保存的工程，编辑器保留未保存的草稿。"
        };
        if let Some(interruption) = context.interruption.as_ref()
            && let Some(label) = interruption.action_label()
        {
            copy.title = label;
            copy.fixed_key = None;
            copy.description = match **interruption {
                feathrise_editor_core::ProjectLaunchInterruption::CanStopCompilation =>
                    "停止本次工程3D编译，保留当前视口模式、未保存草稿和历史。",
                _ => "正在等待本次编译及后台任务退出，未保存草稿和历史保留。",
            };
        }
    }
    let reason = unavailable_reason(target.0, &context.availability());
    let language_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    let locale = locale.as_deref().copied().unwrap_or_default();
    let description = locale.editor_label(copy.description);
    let title = (first_show || language_changed || context.settings.is_changed() || context.mesh_edit.as_ref().is_some_and(|state| state.is_changed())
        || context.launch.as_ref().is_some_and(|launch| launch.is_changed())
        || context.interruption.as_ref().is_some_and(|phase|phase.is_changed()))
        .then(|| tooltip_title(&copy, &context.settings, locale));
    for (kind, mut text, mut node) in &mut texts {
        match kind {
            TooltipText::Title => {
                if let Some(title) = &title
                    && text.as_str() != title
                {
                    **text = title.clone();
                }
            }
            TooltipText::Description => {
                if text.as_str() != description {
                    **text = description.to_owned();
                }
            }
            TooltipText::Reason => {
                let display = if reason.is_some() {
                    Display::Flex
                } else {
                    Display::None
                };
                if node.display != display {
                    node.display = display;
                }
                if first_show || language_changed || state.reason != reason {
                    let label =
                        reason.map_or_else(String::new, |reason| locale.format("tooltip.unavailable",
                            &[("reason", locale.editor_label(reason).into())]));
                    if text.as_str() != label {
                        **text = label;
                    }
                }
            }
        }
    }
    if state.reason != reason {
        state.reason = reason;
    }
    if popup.display != Display::Flex {
        popup.display = Display::Flex;
    }
    visibility.set_if_neq(Visibility::Inherited);
}

/// 提示只平移自身和三个固定文字节点；绝不改按钮Node或工具条/视口的尺寸。
pub(super) fn position_tooltip(
    mut state: ResMut<EditorTooltipState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<
        (
            Entity,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
        ),
        (With<SceneEditorRoot>, Without<TooltipElement>),
    >,
    geometry: Query<
        (
            &ComputedNode,
            &UiGlobalTransform,
            &Node,
            &InheritedVisibility,
        ),
        Without<TooltipElement>,
    >,
    parents: Query<&ChildOf, Without<bevy::ui::OverrideClip>>,
    mut popups: Query<
        (
            &mut Node,
            &ComputedNode,
            &mut UiTransform,
            &mut UiGlobalTransform,
            &mut Visibility,
        ),
        (With<TooltipPopup>, With<TooltipElement>),
    >,
    mut children: Query<
        &mut UiGlobalTransform,
        (
            With<TooltipText>,
            With<TooltipElement>,
            Without<TooltipPopup>,
        ),
    >,
    mut elements: Query<&mut InheritedVisibility, With<TooltipElement>>,
) {
    let Ok((mut node, computed, mut transform, mut global, mut visibility)) = popups.single_mut()
    else {
        return;
    };
    if node.display == Display::None {
        return;
    }
    let root = roots.single().ok();
    let target = state
        .target
        .and_then(|entity| geometry.get(entity).ok().map(|geometry| (entity, geometry)));
    let cursor = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position);
    let valid = root.is_some_and(|(_, computed, _, visible)| {
        visible.get() && computed.size().min_element() > 0.0
    }) && target.is_some_and(|(entity, (node, transform, _, visible))| {
        visible.get()
            && cursor.is_some_and(|cursor| {
                node.contains_point(*transform, cursor)
                    && cursor_unclipped(cursor, entity, &geometry, &parents)
            })
    }) && computed.size().min_element() > 0.0;
    if !valid {
        state.target = None;
        node.display = Display::None;
        *visibility = Visibility::Hidden;
        // 滚动/切页在布局后才使旧按钮不可见；同帧撤下固定四节点，不能留一帧幽灵提示。
        for mut inherited in &mut elements {
            inherited.set_if_neq(InheritedVisibility::HIDDEN);
        }
        return;
    }
    let (_, root_node, root_transform, _) = root.unwrap();
    let (_, (target_node, target_transform, _, _)) = target.unwrap();
    let root_bounds = Rect::from_center_size(root_transform.translation, root_node.size());
    let inverse = root_node.inverse_scale_factor();
    let target_bounds = Rect::from_center_size(target_transform.translation, target_node.size());
    let anchor = Rect::from_corners(
        (target_bounds.min - root_bounds.min) * inverse,
        (target_bounds.max - root_bounds.min) * inverse,
    );
    let position = tooltip_position(
        anchor,
        computed.size() * inverse,
        root_node.size() * inverse,
    );
    let wanted = root_bounds.min + position / inverse + computed.size() * 0.5;
    let delta = wanted - global.translation;
    if delta.length_squared() > 0.0001 {
        let previous = match (transform.translation.x, transform.translation.y) {
            (Val::Px(horizontal), Val::Px(vertical)) => Vec2::new(horizontal, vertical),
            _ => Vec2::ZERO,
        };
        let translation = previous + delta * inverse;
        transform.translation = Val2::px(translation.x, translation.y);
        let mut affine = global.affine();
        affine.translation += delta;
        *global = affine.into();
        for mut child in &mut children {
            let mut affine = child.affine();
            affine.translation += delta;
            *child = affine.into();
        }
    }
    for mut inherited in &mut elements {
        inherited.set_if_neq(InheritedVisibility::VISIBLE);
    }
}

/// 与官方命中相同的祖先裁切判断；这些静态按钮到F12根不足12层，超出则安全收起。
fn cursor_unclipped(
    point: Vec2,
    entity: Entity,
    geometry: &Query<
        (
            &ComputedNode,
            &UiGlobalTransform,
            &Node,
            &InheritedVisibility,
        ),
        Without<TooltipElement>,
    >,
    parents: &Query<&ChildOf, Without<bevy::ui::OverrideClip>>,
) -> bool {
    let mut current = entity;
    for _ in 0..12 {
        let Ok(parent) = parents.get(current) else {
            return true;
        };
        current = parent.parent();
        if let Ok((computed, transform, node, _)) = geometry.get(current)
            && !node.overflow.is_visible()
            && transform.try_inverse().is_none_or(|inverse| {
                !computed
                    .resolve_clip_rect(node.overflow, node.overflow_clip_margin)
                    .contains(inverse.transform_point2(point))
            })
        {
            return false;
        }
    }
    false
}
