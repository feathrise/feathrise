//! 唯一编辑状态机：元素/框选、G/R/S与拓扑预览，完整身份核对后每笔只提交一次。

use super::{geometry::*, preview::EditWorld, resources::MeshEditState, types::*};
use crate::{
    authoring::EditorAuthoringSession,
    components::SceneEditorViewport,
    resources::{SceneEditorState, SceneEditorViewportState},
    types::EditorTool,
    viewport::{
        modes::{ViewportMode, ViewportModeState},
        placement_move::{MoveCamera, MoveInputContext, selected_record},
    },
};
use bevy::{ecs::system::SystemParam, prelude::*, ui::RelativeCursorPosition};

/// 网格移动自己持有公共鼠标门，必须只复核外部所有者，避免既抢占浮层又把自身预览锁死。
#[derive(SystemParam)]
pub(crate) struct MeshEditExternalOwners<'w> {
    menus: (
        Option<Res<'w, crate::ui::asset_context_menu::AssetContextMenuState>>,
        Option<Res<'w, crate::ui::scene_context_menu::SceneContextMenuState>>,
        Option<Res<'w, crate::ui::scene_sidebar::SceneDirectoryMenuState>>,
        Option<Res<'w, crate::ui::asset_preview::AssetTabMenuState>>,
    ),
    previews: (
        Option<Res<'w, crate::ui::asset_preview::AssetPreviewNavigation>>,
        Option<Res<'w, crate::ui::asset_preview::AudioPreviewState>>,
        Option<Res<'w, crate::ui::asset_preview::DocumentMinimapState>>,
        Option<Res<'w, crate::ui::water_preset_editor::WaterPresetLivePreviewNavigation>>,
        Option<Res<'w, crate::ui::asset_preview::AnimationEditorState>>,
    ),
    authoring: (
        Option<Res<'w, crate::ui::status_bar::EditorStatusBarState>>,
        Option<Res<'w, crate::ui::scene_objects::SceneObjectState>>,
        Option<Res<'w, crate::ui::terrain_objects::TerrainObjectsEditorState>>,
        Option<Res<'w, crate::ui::context_help::EditorContextHelpState>>,
        Option<Res<'w, crate::authoring::basic_shapes::BasicShapeState>>,
        Option<Res<'w, crate::viewport::spatial_brush::SpatialBrushState>>,
        Option<Res<'w, crate::viewport::subscenes::SubsceneTransformState>>,
        Option<Res<'w, crate::viewport::placement_move::PlacementMoveState>>,
        Option<Res<'w, crate::viewport::multi_select::PlacementSelectionSet>>,
    ),
}

impl MeshEditExternalOwners<'_> {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.menus
            .0
            .as_ref()
            .is_some_and(|state| state.owns_mouse())
            || self
                .menus
                .1
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .menus
                .2
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .menus
                .3
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .previews
                .0
                .as_ref()
                .is_some_and(|state| state.dragging())
            || self
                .previews
                .1
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .previews
                .2
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .previews
                .3
                .as_ref()
                .is_some_and(|state| state.dragging())
            || self
                .previews
                .4
                .as_ref()
                .is_some_and(|state| state.owns_mouse()) // 场景页和右侧动画可同时可见；时间轴及释放帧也必须封锁建模快捷键。
            || self
                .authoring
                .0
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self.authoring.1.as_ref().is_some_and(|state| state.busy())
            || self
                .authoring
                .2
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .authoring
                .3
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .authoring
                .4
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .authoring
                .5
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .authoring
                .6
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .authoring
                .7
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .authoring
                .8
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
    }
}

fn target_is_current(
    target: &EditTarget,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    selected: Option<&(usize, feathrise_scene_content::ScenePlacementRecord)>,
) -> bool {
    session.revision == target.author_revision
        && session.placement_structure_revision == target.structure_revision
        && editor.tool == EditorTool::Inspect
        && selected
            .is_some_and(|(index, record)| *index == target.index && *record == target.record)
        && session
            .draft
            .as_ref()
            .is_some_and(|draft| draft.scene_id == target.scene_id)
}

fn exit(state: &mut MeshEditState, world: &mut EditWorld, session: &EditorAuthoringSession) {
    world.restore(state, session);
    state.target = None;
    state.movement = None;
    state.box_selection = None;
    state.hover = None;
    state.handle_hover = None;
    state.selected = 0;
    state.release_capture = true; // 返回对象模式的这次鼠标释放仍属于元素编辑。
}

fn cancel_move(state: &mut MeshEditState, world: &mut EditWorld, session: &EditorAuthoringSession) {
    world.restore(state, session);
    state.movement = None;
    state.release_capture = true;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_input(
    mut inputs: ParamSet<(MoveInputContext, ResMut<MeshEditState>)>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<(&RelativeCursorPosition, &ComputedNode), With<SceneEditorViewport>>,
    mode: Res<ViewportModeState>,
    external: MeshEditExternalOwners,
    batch: Option<Res<crate::authoring::placement_batch::PlacementBatchState>>,
    selection: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    guard: Res<crate::resources::SceneEditorNumericScrubMouseGuard>,
    mut world: EditWorld,
) {
    // 公共对象上下文也只读编辑模式状态；先提取常量大小的身份/门，再取得状态写权，避免读写冲突。
    let (
        context_available,
        pointer_over_ui,
        snapping,
        selected,
        save_pressed,
        save_key,
        history_pressed,
    ) = {
        let context = inputs.p0();
        let selected = viewport
            .as_deref()
            .and_then(|viewport| {
                selected_record(
                    &editor,
                    &session,
                    &context.browser,
                    &context.candidates,
                    viewport,
                )
            })
            .filter(|(_, record)| record.asset_target.editable_geometry().is_some())
            .map(|(index, record)| (index, record.clone()));
        let (save_pressed, save_key, history_pressed) = context.preferences.as_ref().map_or_else(
            || {
                let bindings = feathrise_editor_core::EditorKeyBindings::default();
                (
                    bindings.pressed(feathrise_editor_core::EditorShortcutAction::Save, &keyboard),
                    bindings
                        .get(feathrise_editor_core::EditorShortcutAction::Save)
                        .map(|binding| binding.key),
                    [
                        feathrise_editor_core::EditorShortcutAction::Undo,
                        feathrise_editor_core::EditorShortcutAction::Redo,
                    ]
                    .into_iter()
                    .any(|action| bindings.pressed(action, &keyboard)),
                )
            },
            |settings| {
                (
                    settings
                        .editor_keybindings
                        .pressed(feathrise_editor_core::EditorShortcutAction::Save, &keyboard),
                    settings
                        .editor_keybindings
                        .get(feathrise_editor_core::EditorShortcutAction::Save)
                        .map(|binding| binding.key),
                    [
                        feathrise_editor_core::EditorShortcutAction::Undo,
                        feathrise_editor_core::EditorShortcutAction::Redo,
                    ]
                    .into_iter()
                    .any(|action| settings.editor_keybindings.pressed(action, &keyboard)),
                )
            },
        );
        (
            context.scene_commands_available(&editor) && !external.owns_mouse(),
            context.pointer_over_ui(),
            context.snap_settings(),
            selected,
            save_pressed,
            save_key,
            history_pressed,
        )
    };
    let mut state = inputs.p1();
    let release_was_captured = state.release_capture;
    if !mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
        && !mouse.any_just_released([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
    {
        state.release_capture = false;
    }
    let request = state.request.take();
    state.hover = None;
    state.handle_hover = None;
    // F12 必须先收回未确认的幽灵，再进入现有关闭保护；输入所有权不能让编辑器无法关闭。
    if state.active() && keyboard.just_pressed(KeyCode::F12) {
        exit(&mut state, &mut world, &session);
        if context_available {
            editor.toggle_close_request();
        }
        keyboard.clear_just_pressed(KeyCode::F12);
        return;
    }
    if state.moving()
        && save_pressed
        && context_available
        && !session.saving
        && !editor.save_requested
        && batch.as_ref().is_none_or(|batch| !batch.busy())
        && state
            .target
            .as_ref()
            .is_some_and(|target| target_is_current(target, &editor, &session, selected.as_ref()))
    {
        exit(&mut state, &mut world, &session);
        editor.save_requested = true;
        if let Some(key) = save_key {
            keyboard.clear_just_pressed(key);
        }
        editor.status = "已取消未确认的元素变换，正在保存此前确认的场景修改".into();
        return; // 复用已有保存请求，不把临时幽灵写入磁盘；释放帧的鼠标门不能吞掉已配置的保存键。
    }
    if !state.active() && guard.blocked() {
        return; // 数字捕获在松键帧已清空，公共门仍持有该次释放；只拦新会话，不能用自身预览的门取消自己。
    }
    let node = nodes.single().ok();
    let camera = viewport
        .as_deref()
        .zip(node)
        .and_then(|(viewport, (_, node))| {
            MoveCamera::new(&editor, viewport, node.inverse_scale_factor())
        });
    let document_available = context_available
        && !session.saving
        && !editor.save_requested
        && batch.as_ref().is_none_or(|batch| !batch.busy())
        && selection
            .as_ref()
            .is_none_or(|selection| selection.members.len() <= 1);
    let available = document_available && viewport.is_some() && camera.is_some();
    if state.active()
        && (!available
            || mode.mode != ViewportMode::Select
            || !state.target.as_ref().is_some_and(|target| {
                target_is_current(target, &editor, &session, selected.as_ref())
            }))
    {
        exit(&mut state, &mut world, &session);
        editor.status = "点边面编辑已结束；未确认的元素变换已取消".into();
        return;
    }
    if !available {
        return;
    }
    if state.active() && !state.moving() && (save_pressed || history_pressed) {
        // 空闲时保留边沿给原保存/历史分派；合法的G/R/S/B/E等及数字重绑优先于元素快捷键。
        // 预览期间仍由变换持有输入，撤销/重做不能取消或采用尚未确认的网格。
        return;
    }
    let camera = camera.expect("已检查相机");
    let modified = keyboard.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let keyboard_toggle = keyboard.just_pressed(KeyCode::Tab) && !modified;
    if request == Some(MeshEditAction::Toggle) || keyboard_toggle {
        keyboard.clear_just_pressed(KeyCode::Tab);
        if state.active() {
            exit(&mut state, &mut world, &session);
            editor.status = "对象模式：已保留确认的网格编辑".into();
            return;
        }
        if release_was_captured || mouse.any_pressed([MouseButton::Right, MouseButton::Middle]) {
            return;
        }
        let Some((index, record)) = selected.as_ref() else {
            editor.status = "请先选择一个内置立方体，再进入点边面编辑".into();
            return;
        };
        if record.asset_target.editable_geometry().is_none()
            || record.terrain_patch.is_some()
            || record.building_preset_id.is_some()
        {
            editor.status = "点边面编辑目前支持内置立方体；导入模型和可塑形地形沿用各自工具".into();
            return;
        }
        if let Err(error) = record.asset_target.validate() {
            editor.status = format!("无法进入网格编辑：{error}");
            return;
        }
        let Some(mesh) = world_mesh(record) else {
            editor.status = "无法进入网格编辑：数量、索引或实例变换无效".into();
            return;
        };
        state.target = Some(EditTarget {
            scene_id: session
                .draft
                .as_ref()
                .expect("选择来自当前草稿")
                .scene_id
                .clone(),
            index: *index,
            author_revision: session.revision,
            structure_revision: session.placement_structure_revision,
            record: record.clone(),
        });
        state.selected = mesh.all(state.kind);
        state.release_capture = true;
        editor.tool = EditorTool::Inspect;
        editor.placement_transform_enabled = false;
        world.commands.queue(|world: &mut World| {
            if let Some(mut mode) = world.get_resource_mut::<ViewportModeState>() {
                mode.mode = ViewportMode::Select;
            }
        }); // MoveInputContext 只读同一模式资源，延迟原子切换避免系统参数读写冲突。
        editor.status =
            "编辑模式：1 点 / 2 边 / 3 面；G/R/S 变换，B框选，E挤出，更多建模见工具栏".into();
        return;
    }
    if !state.active() {
        return;
    }
    let Some(mesh) = state
        .target
        .as_ref()
        .and_then(|target| world_mesh(&target.record))
    else {
        return;
    };
    let cursor = node.and_then(|(cursor, _)| cursor.normalized);
    let pointer_over_viewport =
        node.is_some_and(|(cursor, _)| cursor.cursor_over()) && !pointer_over_ui;
    let escape = keyboard.just_pressed(KeyCode::Escape)
        || (state.moving() && mouse.just_pressed(MouseButton::Right))
        || request == Some(MeshEditAction::Cancel);
    if let Some(mut selection) = state.box_selection.take() {
        if escape
            || camera != selection.camera
            || node.is_none_or(|(_, node)| node.size() != selection.viewport_size)
        {
            state.selected = selection.before;
            state.release_capture = true;
            keyboard.clear_just_pressed(KeyCode::Escape);
            editor.status = "已取消框选，保留原选择".into();
            return;
        }
        if pointer_over_viewport && let Some(cursor) = cursor {
            if selection.start.is_none() && mouse.just_pressed(MouseButton::Left) {
                selection.start = Some(cursor);
                selection.add = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
                selection.subtract =
                    keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
            }
            selection.current = Some(cursor);
        }
        let confirm =
            request == Some(MeshEditAction::Confirm) || keyboard.just_pressed(KeyCode::Enter);
        keyboard.clear_just_pressed(KeyCode::Enter);
        if confirm || (selection.start.is_some() && mouse.just_released(MouseButton::Left)) {
            // 按钮/Enter采用最后冻结的框；尚未画框时明确退出并保留选择，不留无效确认入口。
            if (confirm || pointer_over_viewport)
                && let (Some(start), Some(end)) = (selection.start, selection.current)
            {
                let selected = mesh.in_box(state.kind, camera, start, end);
                state.selected = if selection.subtract {
                    selection.before & !selected
                } else if selection.add {
                    selection.before | selected
                } else {
                    selected
                };
                editor.status = format!(
                    "框选完成：已选{}{}",
                    state.selected_count(),
                    state.kind.label()
                );
            } else {
                state.selected = selection.before;
                editor.status = if confirm {
                    "框选已结束，保留原选择"
                } else {
                    "框选在视口外释放，保留原选择"
                }
                .into();
            }
            state.release_capture = true;
        } else if selection.start.is_some() && !mouse.pressed(MouseButton::Left) {
            state.selected = selection.before;
            state.release_capture = true;
        } else {
            state.box_selection = Some(selection);
        }
        keyboard.clear_just_pressed(KeyCode::KeyB);
        return;
    }
    if state.movement.is_some() {
        if escape || keyboard.just_pressed(KeyCode::Tab) {
            cancel_move(&mut state, &mut world, &session);
            keyboard.clear_just_pressed(KeyCode::Escape);
            keyboard.clear_just_pressed(KeyCode::Tab);
            editor.status = "已取消元素变换，恢复原网格".into();
            return;
        }
        let mut movement = state.movement.take().expect("移动状态已检查");
        let current_camera = movement.camera == camera
            && node.is_some_and(|(_, node)| node.size() == movement.viewport_size);
        if !current_camera || mouse.pressed(MouseButton::Middle) {
            world.restore(&mut state, &session);
            state.release_capture = true;
            editor.status = "视口或导航状态变化，已取消元素变换".into();
            return;
        }
        if movement.dragging
            && ((!mouse.pressed(MouseButton::Left) && !mouse.just_released(MouseButton::Left))
                || (mouse.just_released(MouseButton::Left) && !pointer_over_viewport))
        {
            world.restore(&mut state, &session);
            state.release_capture = true;
            editor.status = "手柄在视口外释放，已取消元素变换".into();
            return;
        }
        for (key, constraint) in [
            (KeyCode::KeyX, MoveConstraint::X),
            (KeyCode::KeyY, MoveConstraint::Y),
            (KeyCode::KeyZ, MoveConstraint::Z),
        ] {
            if movement.topology.is_none() && keyboard.just_pressed(key) {
                movement.constraint = if movement.constraint == constraint {
                    MoveConstraint::View
                } else {
                    constraint
                };
            }
            keyboard.clear_just_pressed(key);
        }
        // 工具栏、属性和确认按钮具有指针所有权；悬停它们只冻结最后预览，不能顺带移动模型。
        if pointer_over_viewport && let Some(cursor) = cursor {
            let start_cursor = *movement.start_cursor.get_or_insert(cursor);
            let control = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
            let preview = if movement.topology.is_some() {
                super::operation_preview::topology_preview(&movement, state.kind, cursor, control)
            } else if movement.tool == MeshEditTool::Scale {
                scale_factor(
                    camera,
                    start_cursor,
                    cursor,
                    movement.origin,
                    movement.constraint,
                    movement.snapping,
                    control,
                )
                .and_then(|factor| {
                    scaled_record(
                        &movement.before,
                        movement.selected_vertices,
                        movement.origin,
                        movement.constraint,
                        factor,
                    )
                })
            } else if movement.tool == MeshEditTool::Rotate {
                let rotation = rotation_delta(
                    camera,
                    start_cursor,
                    cursor,
                    movement.origin,
                    movement.constraint,
                );
                let rotation = if !movement.dragging
                    && rotation.is_err()
                    && rotation_delta(
                        camera,
                        start_cursor,
                        start_cursor,
                        movement.origin,
                        movement.constraint,
                    )
                    .is_err()
                {
                    // R 可从选区轴心启动；无效起点不能永久冻结，等待首个有效方向后再计算角度。
                    // 只探测起点自身；已有效的起点不会因当前指针经过轴心而改变，手柄起点也不重绑。
                    let initial = rotation_delta(
                        camera,
                        cursor,
                        cursor,
                        movement.origin,
                        movement.constraint,
                    );
                    movement.start_cursor = initial.as_ref().ok().map(|_| cursor);
                    initial
                } else {
                    rotation
                };
                rotation.and_then(|(axis, mut radians)| {
                    // 元素没有独立欧拉角；围绕冻结选区中心对相对角度吸附，避免改写整个对象姿态。
                    if movement.snapping.active(control) {
                        let step = movement.snapping.rotation_degrees.to_radians();
                        radians = (radians / step).round() * step;
                    }
                    rotated_record(
                        &movement.before,
                        movement.selected_vertices,
                        movement.origin,
                        axis,
                        radians,
                    )
                })
            } else {
                let start =
                    constraint_point(camera, start_cursor, movement.origin, movement.constraint);
                let point = constraint_point(camera, cursor, movement.origin, movement.constraint);
                start
                    .zip(point)
                    .ok_or_else(|| "当前视角与约束轴平行，请取消后调整视角".to_owned())
                    .and_then(|(start, point)| {
                        translated_record(
                            &movement.before,
                            movement.selected_vertices,
                            snapped_delta(
                                movement.origin,
                                point - start,
                                movement.constraint,
                                movement.snapping,
                                control,
                            ),
                        )
                    })
            };
            match preview {
                Ok(record) => match world.show(&mut state, &session, &record) {
                    Ok(()) => {
                        movement.preview = record;
                        movement.valid = true;
                    }
                    Err(error) => {
                        movement.valid = false;
                        editor.status = format!(
                            "{}预览无效：{error}",
                            super::operation_preview::operation_label(&movement)
                        );
                    }
                },
                Err(error) => {
                    movement.valid = false;
                    editor.status = format!(
                        "{}预览无效：{error}",
                        super::operation_preview::operation_label(&movement)
                    );
                }
            }
        }
        let confirm = keyboard.just_pressed(KeyCode::Enter)
            || request == Some(MeshEditAction::Confirm)
            || (pointer_over_viewport
                && if movement.dragging {
                    mouse.just_released(MouseButton::Left)
                } else {
                    mouse.just_pressed(MouseButton::Left)
                });
        keyboard.clear_just_pressed(KeyCode::Enter);
        keyboard.clear_just_pressed(KeyCode::KeyG);
        keyboard.clear_just_pressed(KeyCode::KeyR);
        keyboard.clear_just_pressed(KeyCode::KeyS);
        keyboard.clear_just_pressed(KeyCode::KeyA);
        for key in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3] {
            keyboard.clear_just_pressed(key);
        }
        if confirm {
            if !movement.valid {
                if movement.dragging {
                    world.restore(&mut state, &session);
                    state.release_capture = true;
                    editor.status = "释放时几何无效，已恢复变换前的网格".into();
                    return;
                }
                editor.status = "当前几何无效，不能确认；请继续移动或 Esc 取消".into();
                state.movement = Some(movement);
                return;
            }
            let target = state.target.as_ref().expect("活动目标存在").clone();
            match world.commit(&target, &movement.preview, &mut session, &mut editor) {
                Ok(changed) => {
                    world.restore(&mut state, &session);
                    let target = state.target.as_mut().expect("提交不清理活动目标");
                    target.record = movement.preview.clone();
                    target.author_revision = session.revision;
                    if movement.topology.is_some() {
                        state.kind = MeshElementKind::Face;
                        state.selected = match movement.topology {
                            Some(TopologyOperation::Extrude { face, .. }) => 1u128 << face,
                            Some(TopologyOperation::Inset) => movement.selected_elements,
                            _ => world_mesh(&movement.preview)
                                .map_or(0, |mesh| mesh.all(MeshElementKind::Face)),
                        };
                    }
                    state.release_capture = true;
                    editor.status = if changed {
                        format!(
                            "已{}所选元素；一次撤销恢复完整网格",
                            super::operation_preview::operation_label(&movement)
                        )
                    } else {
                        "元素位置未改变".into()
                    };
                }
                Err(error) => {
                    world.restore(&mut state, &session);
                    state.release_capture = true;
                    editor.status = format!("未采用元素变换：{error}");
                }
            }
        } else {
            if movement.valid {
                editor.status = format!(
                    "正在{} · {}；XYZ 约束，{}，Esc/右键取消",
                    super::operation_preview::operation_label(&movement),
                    movement.constraint.label(),
                    if movement.dragging {
                        "松开/Enter 确认"
                    } else {
                        "左键/Enter 确认"
                    }
                );
            }
            state.movement = Some(movement);
        }
        return;
    }
    if escape {
        state.selected = 0;
        state.release_capture = true;
        keyboard.clear_just_pressed(KeyCode::Escape);
        editor.status = "已清除元素选择；Tab 返回对象模式".into();
        return;
    }
    let control = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if request == Some(MeshEditAction::BoxSelect)
        || (!modified && keyboard.just_pressed(KeyCode::KeyB))
    {
        state.box_selection = Some(BoxSelection {
            start: None,
            current: cursor,
            before: state.selected,
            add: false,
            subtract: false,
            camera,
            viewport_size: node.expect("框选需要视口节点").1.size(),
        });
        state.release_capture = true;
        keyboard.clear_just_pressed(KeyCode::KeyB);
        editor.status = "框选：左键拖动，Shift添加，Ctrl减选，Esc取消".into();
        return;
    }
    let topology = match request {
        Some(MeshEditAction::Extrude) => Some(MeshEditAction::Extrude),
        Some(MeshEditAction::Inset) => Some(MeshEditAction::Inset),
        Some(MeshEditAction::Bevel) => Some(MeshEditAction::Bevel),
        Some(MeshEditAction::LoopCut) => Some(MeshEditAction::LoopCut),
        Some(MeshEditAction::Knife) => Some(MeshEditAction::Knife),
        Some(MeshEditAction::Subdivide) => Some(MeshEditAction::Subdivide),
        _ if !modified && keyboard.just_pressed(KeyCode::KeyE) => Some(MeshEditAction::Extrude),
        _ if !modified && keyboard.just_pressed(KeyCode::KeyI) => Some(MeshEditAction::Inset),
        _ if !modified && keyboard.just_pressed(KeyCode::KeyK) => Some(MeshEditAction::Knife),
        _ if control && keyboard.just_pressed(KeyCode::KeyB) => Some(MeshEditAction::Bevel),
        _ if control && keyboard.just_pressed(KeyCode::KeyR) => Some(MeshEditAction::LoopCut),
        _ => None,
    };
    if let Some(action) = topology {
        for key in [
            KeyCode::KeyE,
            KeyCode::KeyI,
            KeyCode::KeyK,
            KeyCode::KeyB,
            KeyCode::KeyR,
        ] {
            keyboard.clear_just_pressed(key);
        }
        let before = &state.target.as_ref().expect("活动目标存在").record;
        let operation = match action {
            MeshEditAction::Extrude
                if state.kind == MeshElementKind::Face && state.selected_count() == 1 =>
            {
                let face = state.selected.trailing_zeros() as usize;
                match feathrise_scene_content::extrusion_normal(before, face) {
                    Ok(normal) => TopologyOperation::Extrude { face, normal },
                    Err(error) => {
                        editor.status = error;
                        return;
                    }
                }
            }
            MeshEditAction::Extrude => {
                editor.status = "请在面模式选择一个面，再挤出".into();
                return;
            }
            MeshEditAction::Inset => TopologyOperation::Inset,
            MeshEditAction::Bevel => TopologyOperation::Bevel,
            MeshEditAction::LoopCut => TopologyOperation::LoopCut,
            MeshEditAction::Knife => TopologyOperation::Knife,
            MeshEditAction::Subdivide => TopologyOperation::Subdivide,
            _ => unreachable!(),
        };
        let mask = mesh.selected_vertices(state.kind, state.selected);
        let origin = selection_center(&mesh.vertices, mask)
            .unwrap_or_else(|| mesh.vertices.iter().sum::<Vec3>() / mesh.vertices.len() as f32);
        let before = before.clone();
        let mut movement = ElementMove {
            preview: before.clone(),
            before,
            selected_vertices: mask,
            selected_elements: state.selected,
            origin,
            start_cursor: if pointer_over_viewport { cursor } else { None },
            camera,
            viewport_size: node.expect("有视口").1.size(),
            constraint: MoveConstraint::View,
            tool: MeshEditTool::Select,
            topology: Some(operation),
            dragging: false,
            valid: false,
            snapping,
        };
        if matches!(operation, TopologyOperation::Subdivide) {
            // 细分没有鼠标参数，工具栏启动当帧就显示完整候选；确认按钮无需先移进视口。
            let candidate = super::topology_ops::subdivide_record(
                &movement.before,
                state.kind,
                movement.selected_elements,
            );
            match candidate.and_then(|record| {
                world.show(&mut state, &session, &record)?;
                Ok(record)
            }) {
                Ok(record) => {
                    movement.preview = record;
                    movement.valid = true;
                }
                Err(error) => {
                    state.release_capture = true;
                    editor.status = format!("细分预览无效：{error}");
                    return;
                }
            }
        }
        state.movement = Some(movement);
        state.release_capture = true;
        editor.status = format!(
            "{}预览：移动鼠标调整，左键/Enter确认，Esc取消",
            operation.label()
        );
        return;
    }
    if let Some(MeshEditAction::Tool(tool)) = request {
        state.tool = tool;
        state.release_capture = true; // 切工具的完成点击不能顺带拾取其背后的点或手柄。
        editor.status = format!(
            "网格{}工具：作用于所选点/边/面；G 移动，R 旋转，S 缩放",
            tool.label()
        );
        return;
    }
    let requested_kind = match request {
        Some(MeshEditAction::Kind(kind)) => Some(kind),
        _ if !modified && keyboard.just_pressed(KeyCode::Digit1) => Some(MeshElementKind::Vertex),
        _ if !modified && keyboard.just_pressed(KeyCode::Digit2) => Some(MeshElementKind::Edge),
        _ if !modified && keyboard.just_pressed(KeyCode::Digit3) => Some(MeshElementKind::Face),
        _ => None,
    };
    if let Some(kind) = requested_kind {
        state.selected = mesh.convert_selection(state.kind, kind, state.selected);
        state.kind = kind;
        editor.status = format!(
            "编辑模式 · {}选择；Shift 多选，G 移动，R 旋转，S 缩放",
            kind.label()
        );
    }
    for key in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3] {
        keyboard.clear_just_pressed(key);
    }
    if request == Some(MeshEditAction::SelectAll)
        || (!modified && keyboard.just_pressed(KeyCode::KeyA))
    {
        state.selected = mesh.all(state.kind);
        keyboard.clear_just_pressed(KeyCode::KeyA);
    }
    let vertices = &mesh.vertices;
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mask = mesh.selected_vertices(state.kind, state.selected);
    if pointer_over_viewport
        && !shift
        && !mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
        && let Some(origin) = selection_center(&vertices, mask)
    {
        state.handle_hover =
            cursor.and_then(|cursor| pick_transform_handle(state.tool, camera, origin, cursor));
    }
    let drag_constraint = state
        .handle_hover
        .filter(|_| mouse.just_pressed(MouseButton::Left) && !release_was_captured);
    if drag_constraint.is_none()
        && pointer_over_viewport
        && !mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
    {
        state.hover = cursor.and_then(|cursor| mesh.pick(state.kind, camera, cursor));
        if mouse.just_pressed(MouseButton::Left) && !release_was_captured {
            if let Some(index) = state.hover {
                if shift {
                    state.selected ^= 1 << index;
                } else {
                    state.selected = 1 << index;
                }
            } else if !shift {
                state.selected = 0;
            }
        }
    }
    let requested_tool = if drag_constraint.is_some() {
        Some(state.tool)
    } else if request == Some(MeshEditAction::Rotate)
        || (!modified && keyboard.just_pressed(KeyCode::KeyR))
    {
        Some(MeshEditTool::Rotate)
    } else if request == Some(MeshEditAction::Move)
        || (!modified && keyboard.just_pressed(KeyCode::KeyG))
    {
        Some(MeshEditTool::Move)
    } else if request == Some(MeshEditAction::Scale)
        || (!modified && keyboard.just_pressed(KeyCode::KeyS))
    {
        Some(MeshEditTool::Scale)
    } else {
        None
    };
    keyboard.clear_just_pressed(KeyCode::KeyG);
    keyboard.clear_just_pressed(KeyCode::KeyR);
    keyboard.clear_just_pressed(KeyCode::KeyS);
    if let Some(tool) = requested_tool {
        let mask = mesh.selected_vertices(state.kind, state.selected);
        let Some(origin) = selection_center(&vertices, mask) else {
            editor.status = "请先选择需要变换的点、边或面".into();
            return;
        };
        let start_cursor = if pointer_over_viewport { cursor } else { None };
        let before = state.target.as_ref().expect("活动目标存在").record.clone();
        state.movement = Some(ElementMove {
            preview: before.clone(),
            before,
            selected_vertices: mask,
            selected_elements: state.selected,
            origin,
            start_cursor,
            camera,
            viewport_size: node.expect("相机需要视口节点").1.size(),
            constraint: drag_constraint.unwrap_or(MoveConstraint::View),
            tool,
            topology: None,
            dragging: drag_constraint.is_some(),
            valid: true,
            snapping,
        });
        state.release_capture = true;
        state.tool = tool;
        editor.status = format!(
            "正在{}；XYZ 约束，{}，Esc/右键取消",
            tool.label(),
            if drag_constraint.is_some() {
                "松开/Enter 确认"
            } else {
                "左键/Enter 确认"
            }
        );
    }
}
