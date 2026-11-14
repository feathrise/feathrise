//! 按键/按钮、有限捕获、非阻塞CPU轮询与完整作者采用；进行中的手势始终能收到取消。
use feathrise_editor_ui::EditorButtonClick;
use super::{components::*, resources::PlacementBatchState, types::*, worker};
use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand,
        courtyards::{CourtyardAuthoringState, queue_visual_sync},
        placement_pose::is_automatic_volume,
        },
    components::SceneEditorViewport,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    viewport::{
        multi_select::{PlacementSelectionSet, SelectedMember},
        placement_move::{MoveInputContext, PlacementMoveState},
    },
};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
    ui::RelativeCursorPosition,
};
use feathrise_scene_content::BuildingPresetRegistry;
use std::sync::Arc;

#[derive(SystemParam)]
pub(crate) struct BatchEnvironment<'w, 's> {
    pub(super) commands: Commands<'w, 's>,
    pub(super) assets: Res<'w, AssetServer>,
    pub(super) viewport: Option<Res<'w, SceneEditorViewportState>>,
    pub(super) nodes: Query<
        'w,
        's,
        (&'static RelativeCursorPosition, &'static ComputedNode),
        With<SceneEditorViewport>,
    >,
    pub(super) buildings: Option<Res<'w, crate::authoring::buildings::BuildingAuthoringState>>,
    pub(super) registry: Option<Res<'w, BuildingPresetRegistry>>,
    pub(super) clipmap: Option<Res<'w, feathrise_scene::DynamicClipmapGroundSurface>>,
    pub(super) terrain_queries: feathrise_scene::TerrainObjectQueries<'w, 's>,
    pub(super) legacy_buttons: Query<
        'w,
        's,
        (
            &'static crate::ui::placement_transform::SceneEditorPlacementTransformActionButton,
            &'static EditorButtonClick,
        ),
        Changed<EditorButtonClick>,
    >,
    pub(super) close_buttons: Query<
        'w,
        's,
        (
            &'static crate::components::SceneEditorActionButton,
            &'static EditorButtonClick,
            Has<bevy::ui::InteractionDisabled>,
        ),
        Changed<EditorButtonClick>,
    >,
}

/// 冻结多地形集合与实际姿态快照；重建后的同修订结果也不能接收旧射线或旧绑定。
pub(super) fn object_content(session: &EditorAuthoringSession) -> Option<[u64; 7]> {
    let draft = session.draft.as_ref()?;
    draft.manifest.terrains.as_ref()?;
    let ground = draft.terrain_ground.as_ref()
        .filter(|ground| ground.scene_id == draft.scene_id && ground.revision == session.terrain_revision)?;
    Some([ground.revision, ground.generation, Arc::as_ptr(ground) as usize as u64, 0, 0, 0, 0])
}

pub(super) fn content(
    env: &BatchEnvironment,
    session: &EditorAuthoringSession,
) -> Option<[u64; 7]> {
    if session.draft.as_ref().is_some_and(|draft| draft.manifest.terrains.is_some()) {
        return object_content(session); // 空容器不能借用旧全局Clipmap取得采用信用。
    }
    env.clipmap
        .as_ref()?
        .snapshot
        .as_ref()
        .filter(|snapshot| {
            session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.scene_id == snapshot.scene_id)
                && snapshot.authoring_height_revision() == Some(session.terrain_revision)
        })
        .map(|snapshot| snapshot.content_revision_words())
}
/// 固定数量的身份门只返回首个失效原因；取消时可解释真正变化，而不削弱迟到结果的拒收条件。
fn current_failure(
    identity: &BatchIdentity,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    env: &BatchEnvironment,
) -> Option<&'static str> {
    [
        (editor.open && !editor.close_requested, "编辑器已关闭或请求关闭"),
        (!editor.new_scene_dialog_open, "场景弹框已打开"),
        (!editor.scene_transition_in_progress && editor.pending_scene_transition.is_none(), "场景正在切换"),
        (!session.saving, "场景已开始保存"),
        (!session.model_vegetation_busy, "模型植被作者任务正在处理"),
        (editor.selected_scene_id.as_ref() == Some(&identity.scene_id)
            && session.draft.as_ref().is_some_and(|draft| draft.scene_id == identity.scene_id), "作者场景已变化"),
        (session.revision == identity.author, "作者修订已变化"),
        (session.placement_structure_revision == identity.structure, "对象结构修订已变化"),
        (session.terrain_revision == identity.terrain, "地形修订已变化"),
        (session.placement_derived_pose_revision == identity.placement_derived_pose_revision, "派生贴地姿态已变化"),
        (identity.document.matches(&session.placement_deletions), "作者文档身份已变化"),
        (identity.content.is_none_or(|old| content(env, session) == Some(old)), "地形来源快照已变化"),
    ].into_iter().find_map(|(valid, reason)| (!valid).then_some(reason))
}

pub(super) fn start_job(
    state: &mut PlacementBatchState,
    identity: BatchIdentity,
    work: impl std::future::Future<Output = BatchOutput> + Send + 'static,
) {
    debug_assert!(state.job.is_none());
    state.job = Some(BatchJob {
        identity,
        ticket: state.ticket,
        task: AsyncComputeTaskPool::get().spawn(work),
        selection: None,
    });
}
pub(super) fn retire<T: Send + 'static>(
    state: &mut PlacementBatchState,
    identity: BatchIdentity,
    owned: T,
) {
    start_job(state, identity, async move {
        drop(owned);
        BatchOutput::Retired
    });
}

pub(super) fn cancel(state: &mut PlacementBatchState, env: &mut BatchEnvironment) {
    state.ticket = state.ticket.wrapping_add(1);
    let capture = state.capture.take();
    let waiting = state.waiting.take();
    let paste = state.paste.take().map(|mut paste| {
        env.commands
            .entity(paste.parent)
            .try_insert(Visibility::Hidden);
        state.cleaning.push(paste.parent); // 最后清理父级，避免级联销毁整组选中根造成峰值。
        state.cleaning.append(&mut paste.roots);
        state.cleaning.extend(paste.frames.iter().flatten().copied());
        paste
    });
    let identity = capture
        .as_ref()
        .map(|capture| capture.identity.clone())
        .or_else(|| paste.as_ref().map(|paste| paste.identity.clone()));
    if let Some(identity) = identity {
        debug_assert!(state.garbage.is_none());
        state.garbage = Some(CancelledBatch {
            identity,
            _capture: capture,
            _waiting: waiting,
            _paste: paste,
        });
    }
    // 在途任务仍占唯一槽；它持有的旧完成值稍后在同一槽回收，不能启动第二个worker。
}

fn consume_edges(keyboard: &mut ButtonInput<KeyCode>) {
    let pressed = keyboard
        .get_just_pressed()
        .filter(|key| **key != KeyCode::F12)
        .copied()
        .collect::<Vec<_>>();
    let released = keyboard
        .get_just_released()
        .filter(|key| **key != KeyCode::F12)
        .copied()
        .collect::<Vec<_>>();
    for key in pressed {
        keyboard.clear_just_pressed(key);
    }
    for key in released {
        keyboard.clear_just_released(key);
    }
}

pub(crate) fn handle_placement_batch(
    mut state: ResMut<PlacementBatchState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    context: MoveInputContext,
    movement: Res<PlacementMoveState>,
    mut selection: ResMut<PlacementSelectionSet>,
    mut visuals: ResMut<CourtyardAuthoringState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    buttons: Query<(&PlacementBatchButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut env: BatchEnvironment,
) {
    if (state.busy() || movement.multi_transform_active()) && editor.open {
        let close_button = env
            .close_buttons
            .iter()
            .any(|(button, interaction, disabled)| {
                button.0 == crate::types::EditorAction::Close
                    && interaction.is_clicked()
                    && !disabled
            });
        if keyboard.just_pressed(KeyCode::F12) {
            editor.toggle_close_request();
            keyboard.clear_just_pressed(KeyCode::F12); // 本域已登记请求，后续通用入口不能再把它切回取消。
            cancel(&mut state, &mut env);
        } else if close_button {
            editor.request_close();
            cancel(&mut state, &mut env);
        }
    } // 【需求设计】关闭始终可请求；未确认对象先取消，后台槽和鼠标释放由既有安全关闭流程收尾。
    for _ in 0..RECORD_BUDGET {
        let Some(entity) = state.cleaning.pop() else {
            break;
        };
        env.commands.entity(entity).try_despawn();
    }
    let invalid = state.capture.as_ref().and_then(|capture| {
        current_failure(&capture.identity, &editor, &session, &env)
            .or_else(|| (capture.selection_revision != selection.revision).then_some("对象选集已变化"))
            .or_else(|| (capture.selected_id != context.browser.selected_placement_id).then_some("当前对象选择已变化"))
    }).or_else(|| state.paste.as_ref().and_then(|paste| {
        current_failure(&paste.identity, &editor, &session, &env)
            .or_else(|| paste.building_revision.is_some_and(|revision| {
                env.registry
                    .as_ref()
                    .map_or(0, |registry| registry.revision)
                    != revision
            }).then_some("建筑预设已变化"))
    }));
    if invalid.is_some()
        || state.busy()
            && (keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right))
    {
        cancel(&mut state, &mut env);
        editor.status = format!("已取消对象批量操作（{}），场景数据保持原样", invalid.unwrap_or("Esc或鼠标右键取消"));
    }
    if let Some(output) = state
        .job
        .as_mut()
        .and_then(|job| check_ready(&mut job.task))
    {
        let job = state.job.take().unwrap();
        if !matches!(output, BatchOutput::Retired) {
            let rejected = (job.ticket != state.ticket).then_some("任务票据已变化")
                .or_else(|| current_failure(&job.identity, &editor, &session, &env))
                .or_else(|| job.selection.as_ref().is_some_and(|(revision, id)| {
                    *revision != selection.revision || *id != context.browser.selected_placement_id
                }).then_some("对象选集或当前选择已变化"));
            if let Some(reason) = rejected {
                // 旧输出失去信用时同时撤销其未确认粘贴；只回收输出会留下永远等不到Ghosts的0/N空壳。
                cancel(&mut state, &mut env);
                editor.status = format!("批量操作身份已变化（{reason}），旧结果未应用");
                retire(&mut state, job.identity, output);
            } else {
                accept_output(
                    output,
                    &mut state,
                    &mut editor,
                    &mut session,
                    &mut selection,
                    &mut visuals,
                    &mut env,
                );
            }
        }
    }
    if state.job.is_none()
        && let Some(garbage) = state.garbage.take()
    {
        retire(&mut state, garbage.identity.clone(), garbage);
    }
    if let Some(mut capture) = state.capture.take() {
        let end = (capture.records.len() + RECORD_BUDGET).min(capture.members.len());
        let mut error = None;
        while capture.records.len() < end {
            let member = &capture.members[capture.records.len()];
            let record = session
                .draft
                .as_ref()
                .and_then(|draft| draft.placements.placements.get(member.index))
                .filter(|record| record.id == member.id && record.editor.selectable());
            if let Some(record) = record {
                if is_automatic_volume(record) {
                    error = Some("自动三维雕刻层请使用专用作者入口，整批操作未应用");
                    break; // 旧选集也只能整批拒绝，不能把普通删除提交到专用体积历史之外。
                }
                capture.records.push((member.index, record.clone()));
            } else {
                error = Some("选中的完整对象已变化");
                break;
            }
        }
        if let Some(error) = error {
            editor.status = error.into();
            retire(&mut state, capture.identity.clone(), capture);
        } else if capture.records.len() == capture.members.len() {
            // 只在真正交给worker前共享原表，捕获跨帧期间不占用写门；其他写者持源时保持本槽等待。
            if capture.action == PlacementBatchAction::Delete && session.placement_deletions.write_shared() {
                state.capture = Some(capture);
                editor.status = "删除票据正在后台准备，完成后继续整批删除".into();
            } else {
                let deletion_source = (capture.action == PlacementBatchAction::Delete)
                    .then(|| session.placement_deletions.source());
                let selected = (capture.selection_revision, capture.selected_id.clone());
                start_job(&mut state, capture.identity.clone(), async move {
                    worker::capture_with_deletions(capture, deletion_source)
                });
                state.job.as_mut().unwrap().selection = Some(selected);
            }
        } else {
            state.capture = Some(capture);
        }
    }
    super::paste::advance(
        &mut state,
        &mut editor,
        &mut session,
        &mut selection,
        &mut visuals,
        &mouse,
        &keyboard,
        &context,
        &mut env,
    );
    if state.busy() {
        state.owns_mouse = true;
        editor.suppress_world_pick_this_frame = true;
        consume_edges(&mut keyboard);
        return;
    }
    let quiet = !mouse.any_pressed([MouseButton::Left, MouseButton::Middle, MouseButton::Right])
        && ![MouseButton::Left, MouseButton::Middle, MouseButton::Right]
            .into_iter()
            .any(|key| mouse.just_released(key));
    if state.owns_mouse {
        if quiet {
            state.owns_mouse = false;
        } else {
            consume_edges(&mut keyboard);
            return;
        }
    }
    if !context.batch_available(&editor)
        || movement.owns_mouse()
        || selection.owns_mouse()
        || guard.blocked()
        || session.saving
        || session.model_vegetation_busy
        || visuals.operation_pending()
        || visuals.visual_sync_pending()
    {
        return;
    }
    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let action = buttons
        .iter()
        .find(|(_, interaction)| interaction.is_clicked())
        .map(|(button, _)| button.0)
        .or_else(|| {
            env.legacy_buttons.iter().find_map(|(button, interaction)| {
                if !interaction.is_clicked() {
                    return None;
                }
                use crate::ui::placement_transform::PlacementTransformAction;
                match button.0 {
                    // 【需求设计】属性面板的单对象复制同样冻结模型植被并后台重绑新ID；
                    // 不能再落到旧PlacementAdded捷径，否则作者看得见的草会在副本上丢失。
                    PlacementTransformAction::Duplicate => Some(PlacementBatchAction::Duplicate),
                    PlacementTransformAction::Delete if selection.members.len() > 1 => Some(PlacementBatchAction::Delete),
                    _ => None,
                }
            })
        })
        .or_else(|| {
            if ctrl && keyboard.just_pressed(KeyCode::KeyC) {
                Some(PlacementBatchAction::Copy)
            } else if ctrl && keyboard.just_pressed(KeyCode::KeyV) {
                Some(PlacementBatchAction::Paste)
            } else if ctrl && keyboard.just_pressed(KeyCode::KeyD) {
                Some(PlacementBatchAction::Duplicate)
            } else if keyboard.just_pressed(KeyCode::Delete) {
                Some(PlacementBatchAction::Delete)
            } else {
                None
            }
        });
    let Some(action) = action else {
        return;
    };
    consume_edges(&mut keyboard);
    editor.suppress_world_pick_this_frame = true;
    if action == PlacementBatchAction::Paste {
        if let Some(source) = state.clipboard.clone() {
            super::paste::begin(source, &mut state, &mut editor, &session, &mut env);
        } else {
            editor.status = "请先在场景中选择对象并复制（Ctrl+C）".into();
        }
        return;
    }
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    if !selection.members.is_empty() && !selection.ready_for(&session) {
        editor.status = "多选对象索引正在更新，请稍后再操作".into();
        return;
    }
    let members = if !selection.members.is_empty() {
        selection.members.clone()
    } else {
        let member = env
            .viewport
            .as_deref()
            .and_then(|viewport| {
                crate::viewport::placement_move::selected_record(
                    &editor,
                    &session,
                    &context.browser,
                    &context.candidates,
                    viewport,
                )
            })
            .map(|(index, record)| SelectedMember {
                id: record.id.clone(),
                index,
                position: Vec3::from_array(record.translation),
            });
        let Some(member) = member else {
            editor.status = "请先选择完整摆放对象".into();
            return;
        };
        Arc::new(vec![member])
    };
    let Some(bounds) = draft.manifest.world_bounds() else {
        return;
    };
    state.ticket = state.ticket.wrapping_add(1);
    state.capture = Some(Capture {
        model_vegetation: crate::authoring::model_vegetation::root_targets(draft),
        identity: BatchIdentity::from_session(&session, content(&env, &session)).unwrap(),
        action,
        records: Vec::with_capacity(members.len()),
        members,
        bounds,
        source_objects: draft.manifest.terrain_objects_for_edit(),
        length: draft.placements.placements.len(),
        selection_revision: selection.revision,
        selected_id: context.browser.selected_placement_id.clone(),
    });
    state.owns_mouse = true;
    editor.status = "正在准备选中对象的完整快照…".into();
}

fn accept_output(
    output: BatchOutput,
    state: &mut PlacementBatchState,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    selection: &mut PlacementSelectionSet,
    visuals: &mut CourtyardAuthoringState,
    env: &mut BatchEnvironment,
) {
    match output {
        BatchOutput::Captured(Ok(source), duplicate) => {
            if duplicate {
                super::paste::begin(source, state, editor, session, env);
            } else {
                editor.status = format!(
                    "已复制 {} 个对象（{}）；切换场景后 Ctrl+V 粘贴",
                    source.records.len(),
                    source.source_scene.as_str()
                );
                let old = state.clipboard.replace(source);
                if old.is_some() {
                    retire(
                        state,
                        BatchIdentity::from_session(session, content(env, session)).unwrap(),
                        old,
                    );
                }
            }
        }
        BatchOutput::Captured(Err(error), _) | BatchOutput::Ghosts(Err(error)) => {
            editor.status = error;
            cancel(state, env);
        }
        BatchOutput::Deleted(Ok((history, updates, prepared))) => {
            if !prepared.matches(&session.placement_deletions) {
                editor.status = "删除票据已换代，整批删除未应用".into();
                retire(state, BatchIdentity::from_session(session, content(env, session)).unwrap(),
                    (history, updates, prepared));
                return; // 保存确认不推进作者修订，也必须使旧完整票据表失去采用信用。
            }
            let draft = session.draft.as_ref().unwrap();
            if history.original_len != Some(draft.placements.placements.len())
                || history
                    .removed
                    .iter()
                    .any(|(index, record)| draft.placements.placements.get(*index) != Some(record))
            {
                editor.status = "删除对象已经变化，整批未应用".into();
                retire(
                    state,
                    BatchIdentity::from_session(session, content(env, session)).unwrap(),
                    (history, updates, prepared),
                );
                return;
            }
            let removed_water = history
                .removed
                .iter()
                .map(|(_, record)| record.water_surfaces.len())
                .sum::<usize>();
            let Some(next_water) = draft
                .placement_water_surface_count
                .checked_sub(removed_water)
            else {
                editor.status = "网格水计数尚未一致，整批删除未应用".into();
                retire(
                    state,
                    BatchIdentity::from_session(session, content(env, session)).unwrap(),
                    (history, updates, prepared),
                );
                return;
            };
            // worker已经对完整目标表校验容量；matches保证这份容量证明仍属于当前源。
            let draft = session.draft.as_mut().unwrap();
            let mut retired = Vec::with_capacity(history.removed.len());
            for (index, _) in &history.removed {
                retired.push(draft.placements.placements.swap_remove(*index));
            }
            draft.placement_water_surface_count = next_water;
            let map = draft.scene_id.clone();
            crate::authoring::placement_deletion::apply_prepared(
                &mut env.commands, session, &map, prepared,
            ); // 完整表O(1)换入；旧表后台析构，已有根沿十六项视觉预算逐项遮蔽。
            session.push_bounded_water_history(EditorCommand::CourtyardBatch(Box::new(history)));
            session.mark_placement_structure_changed(editor);
            queue_visual_sync(visuals, session, updates);
            selection.clear();
            state.selection_request = Some(String::new());
            editor.status = "已删除选中对象；Ctrl+Z 一次恢复整组及原顺序".into();
            retire(
                state,
                BatchIdentity::from_session(session, content(env, session)).unwrap(),
                retired,
            );
        }
        BatchOutput::Deleted(Err(error)) | BatchOutput::Paste(Err(error)) => {
            editor.status = format!("整批操作未应用：{error}");
            if let Some(paste) = state.paste.as_mut() {
                paste.validating = false;
                paste.clicked = None;
            }
        }
        BatchOutput::Ghosts(Ok(records)) => {
            if let Some(paste) = state.paste.as_mut() {
                paste.records = records;
            }
        }
        BatchOutput::Ray(key, hit, target) => {
            if let Some(paste) = state
                .paste
                .as_mut()
                .filter(|paste| paste.desired == Some(key))
            {
                if let feathrise_scene::ClipmapGroundHit::Hit(point) = hit {
                    paste.point = Some((key, point));
                    paste.patch_target = target;
                } else {
                    paste.point = None;
                    paste.patch_target = None;
                    env.commands
                        .entity(paste.parent)
                        .try_insert(Visibility::Hidden);
                    if hit != feathrise_scene::ClipmapGroundHit::WaitingForHeight {
                        paste.clicked = None;
                    }
                    editor.status = if hit == feathrise_scene::ClipmapGroundHit::WaitingForHeight {
                        "目标高度页正在加载；已确认的落点会等待完整地表，Esc可取消"
                    } else {
                        "没有命中可用地表，请移动鼠标重新选择落点"
                    }
                    .into();
                }
            }
        }
        BatchOutput::Paste(Ok(prepared)) => {
            if let Some(paste) = state.paste.as_mut() {
                paste.prepared = Some(prepared);
            }
        }
        BatchOutput::Waiting(work) => state.waiting = Some(work),
        BatchOutput::Retired => {}
    }
}

/// 独立系统写浏览器，避免读取现有公共输入上下文时同时借用其ResMut而产生Bevy参数冲突。
pub(crate) fn sync_batch_selection(
    mut state: ResMut<PlacementBatchState>,
    mut browser: ResMut<crate::ui::gltf_browser::EditorGltfBrowserState>,
    mut candidates: ResMut<crate::viewport::LayerCandidateState>,
) {
    if let Some(id) = state.selection_request.take() {
        browser.selected_placement_id = (!id.is_empty()).then_some(id);
        browser.selection = None;
        candidates.clear_for_scene_tree();
    }
}

/// 显隐容器不累积空实体，也不把源标签关闭后的剪贴板数据与模型生命周期绑在一起。
pub(crate) fn retire_empty_paste_group(
    remove: On<Remove<Children>>,
    groups: Query<&PastePreviewGroup>,
    mut session: ResMut<EditorAuthoringSession>,
    mut commands: Commands,
) {
    let Ok(group) = groups.get(remove.entity) else {
        return;
    };
    if session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.scene_id == group.scene_id)
        && session.placement_previews.get(&group.key) == Some(&remove.entity)
    {
        session.placement_previews.remove(&group.key);
    }
    commands.entity(remove.entity).try_despawn();
}
