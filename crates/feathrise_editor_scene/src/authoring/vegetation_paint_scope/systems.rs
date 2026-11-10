//! 跨网格按压捕获、背压、任务轮询和一笔一次历史；旧任务不得借新姿态回写。
use super::super::{
    EditorAuthoringSession, EditorCommand,
    paint::vegetation_paint_exposure,
    terrain_documents::{TerrainCanvas, TerrainDocumentIdentity},
    terrain_ground::TerrainGroundCursor,
};
use super::types::Press;
use super::{model::install_values, resources::*, types::*, worker};
use crate::{resources::SceneEditorState, types::EditorTool};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, futures::check_ready},
};
use feathrise_scene_content::*;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

fn valid(
    state: &VegetationPaintState,
    session: &EditorAuthoringSession,
    editor: &SceneEditorState,
) -> bool {
    let (Some(press), Some(draft)) = (&state.press, &session.draft) else {
        return false;
    };
    draft.scene_id == press.identity.scene_id
        && session.revision == state.expected_revision
        && session.terrain_revision == press.identity.terrain_revision
        && draft
            .manifest
            .terrains
            .as_ref()
            .is_some_and(|v| Arc::ptr_eq(v, &press.objects))
        && Arc::ptr_eq(&draft.manifest.vegetation_paint_groups, &press.groups)
        && Arc::ptr_eq(&editor.terrain_hidden, &press.hidden)
        && editor.terrain_locked == press.locked
        && draft.terrain_ground.as_ref().is_some_and(|g| {
            state.transaction.as_ref().is_none_or(|t| {
                g.generation == t.ground.generation && g.revision == t.ground.revision
            })
        })
}

fn refresh_ground(state: &mut VegetationPaintState, session: &EditorAuthoringSession) {
    if let Some(current) = session
        .draft
        .as_ref()
        .and_then(|d| d.terrain_ground.as_ref())
        && let Some(transaction) = state.transaction.as_mut()
        && current.generation == transaction.ground.generation
        && current.revision == transaction.ground.revision
    {
        transaction.ground = current.clone(); // 只更新同一内容源的驻留页；姿态、雕刻和热更换代仍拒绝。
    }
}

pub(super) fn input(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    cursor: Res<TerrainGroundCursor>,
    settings: Res<VegetationPaintSettings>,
    mut state: ResMut<VegetationPaintState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>,
    ground: Option<Res<crate::viewport::VegetationGroundCursor>>,
) {
    let closing = editor.save_requested || editor.close_requested || editor.undo_requested || editor.redo_requested;
    let finishing = closing || !mouse.pressed(MouseButton::Left);
    let author_context = editor.open
        && editor.tool == EditorTool::Vegetation
        && settings.scope == PaintScope::Cross
        && mode
            .as_ref()
            .is_none_or(|m| m.mode == crate::viewport::modes::ViewportMode::Edit)
        && (finishing || ground
            .as_ref()
            .is_none_or(|g| g.source_authoring_available()))
        && !session.saving
        && (finishing || !keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]));
    let context = author_context && cursor.input_authority().is_some() && !closing;
    if state.press.is_some() {
        let camera_valid = state
            .press
            .as_ref()
            .zip(cursor.identity.as_ref())
            .is_some_and(|(p, c)| {
                p.authority == cursor.input_authority().unwrap_or(u64::MAX)
                    && p.identity.camera == c.camera
                    && p.identity.projection == c.projection
                    && p.identity.size == c.size
                    && p.identity.camera_entity == c.camera_entity
                    && settings.group.as_ref() == Some(&p.group)
            });
        if !author_context || (!finishing && (!context || !camera_valid)) || !valid(&state, &session, &editor) {
            state.ending = true;
            state.cancelled = true;
            state.queue.clear();
            state.last = None;
            state.waiting_exposure = 0.0;
        } else if finishing {
            state.ending = true; // 已受理刷段排空后提交唯一历史；按钮请求保持原样等待。
        }
    }
    if context && mouse.just_pressed(MouseButton::Left) && !state.busy() {
        let captured = (|| -> Option<(Press, Transaction, Vec<Target>)> {
            let draft = session.draft.as_ref()?;
            if draft.terrain_documents.loading {
                return None;
            }
            let groups = draft.manifest.vegetation_paint_groups.clone();
            let group = groups
                .iter()
                .find(|g| Some(&g.id) == settings.group.as_ref())?
                .clone();
            let objects = draft.manifest.terrains.clone()?;
            let ground = draft.terrain_ground.clone()?;
            let identity = cursor.identity.clone()?;
            if identity.revision != session.revision
                || identity.terrain_revision != session.terrain_revision
                || ground.revision != session.terrain_revision
            {
                return None;
            }
            let erase =
                keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
            let mut targets = Vec::new();
            let mut documents = BTreeMap::new();
            for object in group
                .targets
                .iter()
                .filter_map(|id| ground.object_definition(id))
                .filter(|o| {
                    o.enabled
                        && o.terrain.kind != SceneTerrainKind::Voxel
                        && !editor.terrain_hidden.contains(&o.id)
                        && !editor.terrain_locked.contains(&o.id)
                })
            {
                if ground.object_query(&object.id).is_none() {
                    continue;
                }
                let active = draft
                    .terrain_documents
                    .active
                    .as_ref()
                    .is_some_and(|a| a.definition.id == object.id);
                let cached = if active {
                    Some(Arc::new(TerrainCanvas::snapshot(draft)))
                } else {
                    draft.terrain_documents.values.get(&object.id).cloned()
                };
                if let Some(canvas) = cached
                    .as_ref()
                    .filter(|_| !draft.terrain_documents.saved.contains(&object.id))
                {
                    documents.insert(object.id.clone(), canvas.clone());
                } else {
                    targets.push(Target {
                        identity: TerrainDocumentIdentity {
                            definition: object.clone(),
                            manifest: object.source_manifest(&draft.manifest),
                            folder: object.private_folder(&draft.scene_id),
                        },
                        cached,
                        saved: draft.terrain_documents.saved.contains(&object.id),
                    });
                }
            }
            if targets.is_empty() && documents.is_empty() {
                return None;
            }
            let press = Press {
                identity,
                authority: cursor.input_authority()?,
                group: group.id.clone(),
                radius: editor.brush_radius,
                strength: editor.brush_strength,
                erase,
                groups: groups.clone(),
                objects,
                hidden: editor.terrain_hidden.clone(),
                locked: editor.terrain_locked.clone(),
            };
            let transaction = Transaction {
                scene_id: draft.scene_id.clone(),
                group,
                groups_before: groups,
                documents,
                before: BTreeMap::new(),
                changed: BTreeSet::new(),
                ground,
                hidden: editor.terrain_hidden.clone(),
            };
            Some((press, transaction, targets))
        })();
        if let Some((press, transaction, targets)) = captured {
            state.expected_revision = session.revision;
            state.press = Some(press);
            state.ending = false;
            state.cancelled = false;
            state.applied = false;
            state.history_written = false;
            state.waiting_exposure = 0.0;
            state.transaction = Some(transaction.clone());
            if !targets.is_empty() {
                state.work = Some(IoTaskPool::get().spawn(async move {
                    worker::load_and_paint(transaction, targets, VecDeque::new()).await
                }));
            }
            editor.status = "跨网格笔划已冻结目标；等待真实表面与本地作者画布".into();
        } else {
            editor.status =
                "请先新建或选择绘制组，并勾选已就绪、未隐藏锁定的GLB/Clipmap网格".into();
        }
    }
    if state.press.is_some() && !state.ending && mouse.pressed(MouseButton::Left) {
        // 画笔与严格悬停独立消费同一单飞查询；移动中较早完成的真实射线也属于本笔。
        let hit = cursor.paint_hit();
        let point = hit.filter(|hit| {
            let press = state.press.as_ref().unwrap();
            press.groups.iter().find(|group| group.id == press.group)
                .is_some_and(|group| group.targets.contains(&hit.terrain))
        }).map(|hit| hit.point);
        if let Some(point) = point {
            if state.queue.len() + state.in_flight_inputs < 64 {
                let (radius, strength, erase, camera, orthographic) = {
                    let press = state.press.as_ref().unwrap();
                    (press.radius, press.strength, press.erase, press.identity.camera, press.identity.orthographic)
                };
                let starting = state.last.is_none() && !state.applied && state.queue.is_empty();
                let (amount, mut exposure) =
                    vegetation_paint_exposure(strength, time.delta_secs(), starting);
                exposure += std::mem::take(&mut state.waiting_exposure);
                let dab = Dab {
                    from: state.last.unwrap_or(point),
                    to: point,
                    radius,
                    amount,
                    exposure,
                    erase,
                    camera,
                    orthographic,
                };
                state.queue.push_back(dab);
                state.last = Some(point);
            } else {
                state.last = None;
                state.waiting_exposure = 0.0;
                editor.status = "跨网格刷段队列已满，等待后台采用后继续".into();
            }
        } else if cursor.paint_pending() {
            if state.last.is_some() {
                state.waiting_exposure += vegetation_paint_exposure(
                    state.press.as_ref().unwrap().strength, time.delta_secs(), false,
                ).1;
            }
        } else {
            state.last = None;
            state.waiting_exposure = 0.0;
        }
    }
    refresh_ground(&mut state, &session);
    start_batch(&mut state, &session);
    finish(&mut state, &mut session, &mut editor);
    session.vegetation_cross_busy = state.busy();
}

fn start_batch(state: &mut VegetationPaintState, session: &EditorAuthoringSession) {
    if state.cancelled || state.work.is_some() || state.staging.is_some() || state.queue.is_empty()
    {
        return;
    }
    if !state.applied {
        if session.vegetation_compiled_revision != session.vegetation_revision {
            return;
        } // 先等上一笔的完整基线，跨网格撤销不能恢复旧正式图。
        if let Some(draft) = session.draft.as_ref()
            && let Some(active) = draft.terrain_documents.active.as_ref()
            && let Some(transaction) = state.transaction.as_mut()
            && transaction.documents.contains_key(&active.definition.id)
        {
            transaction.documents.insert(
                active.definition.id.clone(),
                Arc::new(TerrainCanvas::snapshot(draft)),
            );
        }
    }
    let Some(transaction) = state.transaction.clone() else {
        return;
    };
    if state
        .waiting_ground
        .as_ref()
        .is_some_and(|g| Arc::ptr_eq(g, &transaction.ground))
    {
        return;
    } // 缺页时等实际快照更新，不每帧重试同一CPU工作。
    state.waiting_ground = None;
    let dabs = std::mem::take(&mut state.queue);
    state.in_flight_inputs = dabs.len();
    state.work =
        Some(AsyncComputeTaskPool::get().spawn(async move { worker::paint(transaction, dabs) }));
}

pub(super) fn poll(
    mut state: ResMut<VegetationPaintState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
) {
    if let Some(result) = state.work.as_mut().and_then(check_ready) {
        state.work = None;
        if state.cancelled || !valid(&state, &session, &editor) {
            state.in_flight_inputs = 0;
            state.cancelled = true;
            state.ending = true;
            state.queue.clear();
        } else {
            match result {
                Ok(mut batch) if !batch.waiting.is_empty() => {
                    state.in_flight_inputs = 0;
                    state.waiting_ground = Some(batch.transaction.ground.clone());
                    batch.waiting.append(&mut state.queue);
                    if batch.waiting.len() > 64 {
                        batch.waiting.truncate(64);
                        state.last = None;
                    }
                    state.queue = batch.waiting;
                    state.transaction = Some(batch.transaction);
                    editor.status = "跨网格刷段正在等待同代真实高度页；已画草地保持显示".into();
                }
                Ok(batch) if batch.prepared.is_empty() => {
                    state.in_flight_inputs = 0;
                    prepend_remaining(&mut state, batch.remaining);
                    state.transaction = Some(batch.transaction)
                }
                Ok(batch) => state.staging = Some(batch),
                Err(error) => {
                    state.in_flight_inputs = 0;
                    editor.status = format!("跨网格绘制未采用：{error}");
                    state.ending = true;
                    state.cancelled = true;
                    state.queue.clear();
                }
            }
        }
    }
    if state.staging.is_some() && !valid(&state, &session, &editor) {
        state.in_flight_inputs = 0;
        state.staging = None;
        state.cancelled = true;
        state.ending = true;
        state.queue.clear();
    }
    refresh_ground(&mut state, &session);
    start_batch(&mut state, &session);
    finish(&mut state, &mut session, &mut editor);
    session.vegetation_cross_busy = state.busy();
}

pub(super) fn adopt(
    batch: Batch,
    state: &mut VegetationPaintState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
) {
    state.in_flight_inputs = 0;
    prepend_remaining(state, batch.remaining);
    let draft = session.draft.as_mut().expect("发布前已校验当前作者身份");
    for prepared in &batch.prepared {
        let id = &prepared.id;
        let canvas = &batch.transaction.documents[id];
        if draft
            .terrain_documents
            .active
            .as_ref()
            .is_none_or(|a| a.definition.id != *id)
            && !draft.terrain_documents.values.contains_key(id)
        {
            draft
                .terrain_documents
                .values
                .insert(id.clone(), canvas.clone());
        }
        install_values(draft, id, &VegetationValues::capture(canvas));
    }
    if draft
        .manifest
        .vegetation_paint_groups
        .iter()
        .find(|g| g.id == batch.transaction.group.id)
        .is_some_and(|g| g.members != batch.transaction.group.members)
    {
        let mut groups = draft.manifest.vegetation_paint_groups.to_vec();
        if let Some(group) = groups
            .iter_mut()
            .find(|g| g.id == batch.transaction.group.id)
        {
            *group = batch.transaction.group.clone();
        }
        draft.manifest.vegetation_paint_groups = groups.into(); // 只在惰性成员真正增加时换代，连续刷草不重建面板。
    }
    draft.terrain_documents.paint_groups_dirty = true;
    session.mark_vegetation_authoring_changed(editor);
    session.vegetation_compiled_revision = session.vegetation_revision;
    session.vegetation_compiled_structure_revision = session.vegetation_structure_revision;
    state.expected_revision = session.revision;
    if let Some(press) = state.press.as_mut() {
        press.groups = session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .vegetation_paint_groups
            .clone();
    }
    state.transaction = Some(batch.transaction);
    state.applied = true;
    record_history(state, session);
    editor.status = "跨网格完整刷印已采用；各网格草地保持本地归属".into();
}

/// 第一份作者结果就进入时间线；取消后的旧CPU任务仍占槽，但不能把历史插到新工具操作之后。
fn record_history(state: &mut VegetationPaintState, session: &mut EditorAuthoringSession) {
    let transaction = state.transaction.as_ref().expect("刚采用的完整事务");
    let change = VegetationPaintChange {
        before_groups: transaction.groups_before.clone(),
        after_groups: session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .vegetation_paint_groups
            .clone(),
        canvases: transaction
            .changed
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    (
                        transaction.before[id].clone(),
                        VegetationValues::capture(&transaction.documents[id]),
                    ),
                )
            })
            .collect(),
    };
    if state.history_written {
        // 未发生外部修订的连续采用只能更新本笔栈顶；经原预算入口替换，不增加撤销次数。
        debug_assert!(matches!(
            session.undo.last(),
            Some(EditorCommand::VegetationPaintChanged(_))
        ));
        session.pop_undo();
        session.push_redone_bounded_water_history(EditorCommand::VegetationPaintChanged(Box::new(
            change,
        )));
    } else {
        session.push_bounded_water_history(EditorCommand::VegetationPaintChanged(Box::new(change)));
        state.history_written = true;
    }
}

fn finish(
    state: &mut VegetationPaintState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
) {
    if !state.ending || state.work.is_some() || state.staging.is_some() || !state.queue.is_empty() {
        return;
    }
    if let Some(transaction) = state.transaction.take()
        && state.applied
        && session
            .draft
            .as_ref()
            .is_some_and(|d| d.scene_id == transaction.scene_id)
    {
        editor.status = if state.cancelled {
            format!(
                "{}；本笔已采用部分可一次撤销，未采用刷段已取消",
                editor.status
            )
        } else {
            "跨网格笔划完成；一次撤销恢复本笔全部网格和新建成员层".into()
        };
    } else if !state.cancelled && state.press.is_some() {
        editor.status = "本笔没有有效像素变化，未新建绘制组成员层".into();
    }
    state.press = None;
    state.last = None;
    state.ending = false;
    state.cancelled = false;
    state.applied = false;
    state.history_written = false;
    state.waiting_ground = None;
    state.in_flight_inputs = 0;
    state.waiting_exposure = 0.0;
}

fn prepend_remaining(state: &mut VegetationPaintState, mut remaining: VecDeque<Dab>) {
    // 输入信用包括在途段；未处理余段和新段共同受64上界约束，不用truncate丢掉已受理笔迹。
    remaining.append(&mut state.queue);
    debug_assert!(remaining.len() <= 64);
    state.queue = remaining;
}
