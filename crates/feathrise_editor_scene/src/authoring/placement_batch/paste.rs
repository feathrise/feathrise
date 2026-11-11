//! 粘贴模型分帧准备、真实地表射线、建筑完整校验与原子发布；未确认时不产生作者副作用。
use super::{
    components::{PasteGhost, PastePreviewGroup},
    resources::PlacementBatchState,
    systems::{BatchEnvironment, content, retire, start_job},
    types::*,
    worker,
};
use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand, PlacementTerrainSampler,
        buildings::courtyard_validator_snapshot, courtyards::CourtyardAuthoringState,
        spawn_placement_preview,
    },
    resources::SceneEditorState,
    viewport::{
        multi_select::PlacementSelectionSet,
        placement_move::{MoveCamera, MoveInputContext},
    },
};
use bevy::prelude::*;
use feathrise_scene_content::{MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_SCENE, MAX_SCENE_PLACEMENTS_PER_SCENE};
use feathrise_scene::{ClipmapGroundHit, ScenePlacementInstance, ScenePlacementWaterEvidence};
use std::sync::Arc;

pub(super) fn begin(
    source: Arc<ClipboardData>,
    state: &mut PlacementBatchState,
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    env: &mut BatchEnvironment,
) {
    let Some(identity) = BatchIdentity::from_session(session, content(env, session)) else {
        return;
    };
    let draft = session.draft.as_ref().unwrap();
    if draft.placements.placements.len() + source.records.len() > MAX_SCENE_PLACEMENTS_PER_SCENE {
        editor.status = "目标场景摆放数量不足，粘贴未开始".into();
        return;
    }
    if draft.placement_water_surface_count
        + source
            .records
            .iter()
            .map(|record| record.water_surfaces.len())
            .sum::<usize>()
        > MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_SCENE
    {
        editor.status = "目标场景的网格水绑定容量不足，粘贴未开始".into();
        return;
    }
    state.ticket = state.ticket.wrapping_add(1);
    let prefix = format!(
        "paste-{:x}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        state.ticket
    );
    let parent = env
        .commands
        .spawn((
            PasteGhost,
            Name::new("对象粘贴预览"),
            Transform::IDENTITY,
            Visibility::Hidden,
            avian3d::prelude::RigidBody::Static,
            avian3d::prelude::RigidBodyDisabled, // 所有基础形状预览共享关闭的物理门，确认整组后一次启用。
        ))
        .id();
    feathrise_scene::set_builtin_placement_batch_enabled(&mut env.commands, parent, false);
    state.paste = Some(PasteSession {
        identity: identity.clone(),
        source: source.clone(),
        records: Arc::default(),
        parent,
        parent_key: format!("{prefix}-container"),
        roots: Vec::with_capacity(source.records.len()),
        frames: Vec::with_capacity(source.records.len()),
        patch_target: None,
        budget: PatchBudget::default(),
        budget_cursor: 0,
        cursor: 0,
        builtin_only: true,
        point: None,
        desired: None,
        started: None,
        clicked: None,
        validating: false,
        prepared: None,
        applying: 0,
        building_revision: None,
        presets: Vec::with_capacity(source.records.len()),
        frozen_presets: None,
    });
    state.owns_mouse = true;
    start_job(state, identity, async move {
        BatchOutput::Ghosts(worker::ghost_records(&source, &prefix))
    });
    editor.status = "正在准备粘贴预览；鼠标选择落点，左键确认，Esc取消".into();
}

pub(super) fn advance(
    state: &mut PlacementBatchState,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    selection: &mut PlacementSelectionSet,
    _visuals: &mut CourtyardAuthoringState,
    mouse: &ButtonInput<MouseButton>,
    keyboard: &ButtonInput<KeyCode>,
    context: &MoveInputContext,
    env: &mut BatchEnvironment,
) {
    let Some(mut paste) = state.paste.take() else {
        return;
    };
    if paste.identity.content.is_none() {
        paste.identity.content = content(env, session);
    }
    let camera_node = env
        .nodes
        .single()
        .ok()
        .zip(env.viewport.as_deref())
        .and_then(|((cursor, node), viewport)| {
            MoveCamera::new(editor, viewport, node.inverse_scale_factor())
                .map(|camera| (cursor, node, camera))
        });
    if paste.clicked.is_some_and(|key| {
        camera_node
            .is_none_or(|(_, node, camera)| camera != key.camera || node.size() != key.ui_size)
            || !key.snap.same_preferences(context.snap_settings().translation_snap(false, paste.source.origin.to_array()))
    }) {
        state.paste = Some(paste);
        super::systems::cancel(state, env);
        editor.status = "粘贴期间视口相机或尺寸已变化，整批取消".into();
        return;
    }
    if paste.prepared.is_some() {
        adopt_preview(&mut paste, state, editor, session, selection, env);
        if paste.prepared.is_some() {
            state.paste = Some(paste);
        }
        return;
    }
    if !paste.source.patch_frames.is_empty() {
        let records = &session.draft.as_ref().unwrap().placements.placements;
        // 这里只累计小型元数据，固定256项/帧；8192项的最坏扫描为32帧，与模型准备并行，不能整图克隆。
        let end = (paste.budget_cursor + 256).min(records.len());
        for record in &records[paste.budget_cursor..end] {
            if let Some(patch) = &record.terrain_patch {
                paste.budget.roots += 1;
                paste.budget.holes += usize::from(patch.cutout.is_some()) + patch.cutouts.len();
            }
        }
        paste.budget_cursor = end;
    }
    let end = (paste.cursor + RECORD_BUDGET).min(paste.records.len());
    for record in &paste.records[paste.cursor..end] {
        paste.builtin_only &= record.asset_target.is_builtin();
        let preset = record.building_preset_id.as_deref().map(|id| {
            env.registry
                .as_deref()
                .and_then(|registry| registry.resolve(id))
                .cloned()
        });
        if record.building_preset_id.is_some()
            && preset.as_ref().is_none_or(|preset| preset.is_none())
        {
            editor.status = "建筑预设已失效，粘贴预览取消".into();
            state.paste = Some(paste);
            super::systems::cancel(state, env);
            return;
        }
        paste.presets.push(preset.flatten()); // 最多16份选中预设/帧，不克隆整个全局建筑目录。
        if record.building_preset_id.is_some() {
            paste.building_revision = Some(env.registry.as_ref().unwrap().revision);
        }
        let mut ghost = record.clone();
        ghost.building_preset_id = None; // 预览不安装建筑碰撞，确认后才恢复完整建筑语义。
        ghost.terrain_patch = None; // 未确认的模型不能提前安装三维地形碰撞或切洞。
        let root =
            spawn_placement_preview(&mut env.commands, &env.assets, &paste.identity.scene_id, &ghost);
        let frame = paste.source.patch_frames.get(paste.roots.len()).copied().flatten().map(|frame| {
            env.commands.spawn((PasteGhost, frame.as_transform(), Visibility::Inherited,
                ChildOf(paste.parent))).id()
        }); // 保留父子两层TRS，倾斜/非均匀源产生的完整世界矩阵不能通过分解近似。
        env.commands
            .entity(root)
            .insert((PasteGhost, ChildOf(frame.unwrap_or(paste.parent))))
            .remove::<(ScenePlacementInstance, ScenePlacementWaterEvidence)>(); // 不计入活动实例或保存证据。
        if ghost.asset_target.is_builtin() {
            env.commands.entity(root).remove::<avian3d::prelude::RigidBody>()
                .insert(ChildOf(paste.parent)); // 移除刚体后重绑父级，确保Avian将盒碰撞重新归属公共禁用刚体。
        }
        paste.roots.push(root);
        paste.frames.push(frame);
    }
    paste.cursor = end;
    if paste.records.is_empty() || paste.cursor < paste.records.len() {
        editor.status = format!(
            "准备粘贴模型 {}/{}…",
            paste.cursor,
            paste.source.records.len()
        );
        state.paste = Some(paste);
        return;
    }
    if paste.frozen_presets.is_none() {
        paste.frozen_presets = Some(Arc::new(std::mem::take(&mut paste.presets)));
    }
    if !paste.source.patch_frames.is_empty()
        && paste.budget_cursor < session.draft.as_ref().unwrap().placements.placements.len() {
        editor.status = "正在核对目标场景的三维地形与洞口容量…".into();
        state.paste = Some(paste);
        return;
    }
    if let Some(mut work) = state.waiting.take() {
        let sampler = session.draft.as_ref().and_then(|draft| {
            PlacementTerrainSampler::from_sources(
                draft,
                env.clipmap.as_deref(),
                session.terrain_revision,
            )
            .ok()
        });
        let retry = sampler
            .as_ref()
            .zip(work.query.as_ref())
            .map(|(sampler, query)| query.can_retry(sampler));
        match retry {
            Some(Ok(true)) => {
                if let Some(validator) = work.validator.as_mut() {
                    validator.sampler = sampler.clone();
                }
                work.sampler = sampler;
                let base_len = session.draft.as_ref().unwrap().placements.placements.len();
                start_job(state, paste.identity.clone(), async move {
                    worker::run_paste(*work, base_len)
                });
            }
            Some(Err(error)) => {
                paste.validating = false;
                paste.clicked = None;
                editor.status = error;
                retire(state, paste.identity.clone(), work);
            }
            _ => state.waiting = Some(work),
        }
    }
    if paste.validating {
        state.paste = Some(paste);
        return;
    }
    let key = paste.clicked.or_else(|| {
        camera_node.and_then(|(cursor, node, camera)| {
            (cursor.cursor_over() && !context.pointer_over_ui())
                .then_some(cursor.normalized)
                .flatten()
                .filter(|point| point.is_finite())
                .map(|normalized| RayKey {
                    camera,
                    normalized,
                    ui_size: node.size(),
                    snap: context.snap_settings().translation_snap(keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]), paste.source.origin.to_array()),
                })
        })
    });
    let Some(key) = key else {
        state.paste = Some(paste);
        return;
    };
    paste.desired = Some(key);
    if mouse.just_pressed(MouseButton::Left) && !context.pointer_over_ui() {
        paste.clicked = Some(key);
        if state.job.is_none() && paste.point.is_none_or(|(accepted, _)| accepted != key) {
            paste.started = None; // 点击曾经Miss的同一位置也重新查询一次；失败回执解冻落点，后续鼠标移动不能被旧点击锁住。
        }
    }
    if let Some((accepted, point)) = paste.point.filter(|(accepted, _)| *accepted == key) {
        let _ = accepted;
        env.commands.entity(paste.parent).insert((
            Transform::from_translation(point - paste.source.origin),
            Visibility::Inherited,
        ));
        if paste.clicked.is_some() && state.job.is_none() {
            match make_work(&mut paste, point, session, env) {
                Ok(work) => {
                    paste.building_revision = Some(work.building_revision);
                    paste.validating = true;
                    let base_len = session.draft.as_ref().unwrap().placements.placements.len();
                    start_job(state, paste.identity.clone(), async move {
                        worker::run_paste(work, base_len)
                    });
                    editor.status = "正在校验整组落点、地基及网格水配置…".into();
                }
                Err(error) => {
                    paste.clicked = None;
                    editor.status = format!("粘贴未开始：{error}");
                }
            }
        }
    }
    if state.job.is_none() && !paste.validating {
        let work_plane = paste.builtin_only && crate::authoring::basic_shapes::work_plane_is_available(session);
        let object_map = session.draft.as_ref().is_some_and(|draft| draft.manifest.terrains.is_some());
        let objects = session.draft.as_ref().and_then(|draft| draft.terrain_ground.as_ref())
            .filter(|ground| ground.scene_id == paste.identity.scene_id && ground.revision == paste.identity.terrain)
            .cloned();
        let pages = if object_map {
            objects.as_ref().map(|ground| (Some(ground.generation), true))
        } else {
            env.clipmap.as_ref().and_then(|surface| surface.snapshot.as_ref())
                .map(|snapshot| (snapshot.authoritative_page_revision(), snapshot.ready))
        }; // 未就绪到完整对象快照的变化会触发重查，稳定等待帧不反复开启任务。
        if paste.started != Some((key, pages)) {
            paste.started = Some((key, pages));
            let ray = key.camera.ray(key.normalized);
            let snapshot = env
                .clipmap
                .as_ref()
                .and_then(|surface| surface.snapshot.as_ref())
                .filter(|snapshot| {
                    snapshot.scene_id == paste.identity.scene_id
                        && snapshot.authoring_height_revision() == Some(paste.identity.terrain)
                })
                .cloned();
            let mesh = session
                .draft
                .as_ref()
                .and_then(|draft| draft.terrain_mesh.clone());
            let index = session
                .draft
                .as_ref()
                .and_then(|draft| draft.terrain_editing_index.clone());
            let sampler = session.draft.as_ref().and_then(|draft| PlacementTerrainSampler::from_draft(draft).ok());
            let hidden = editor.terrain_hidden.clone();
            let legacy_target = session.draft.as_ref().filter(|draft|
                draft.manifest.terrains.is_none()
                    && draft.manifest.terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap)
                .and_then(|draft| draft.manifest.terrain_objects_for_edit().first().cloned())
                .map(|object| PatchTarget { object: Arc::new(object), legacy: true });
            start_job(state, paste.identity.clone(), async move {
                let mut target = legacy_target;
                let hit = if let Some(ray) = ray {
                    if work_plane {
                        crate::authoring::basic_shapes::plane_hit(ray)
                            .and_then(|point| key.snap.horizontal(point.to_array()))
                            .map_or(ClipmapGroundHit::Miss, |point| ClipmapGroundHit::Hit(Vec3::from_array(point)))
                    } else if object_map {
                        objects.as_ref().map_or(ClipmapGroundHit::WaitingForHeight, |ground| {
                            let (hit, object) = object_hit_with_target(ground, &hidden, ray.origin, *ray.direction, key.snap);
                            target = object.map(|object| PatchTarget { object, legacy: false });
                            hit
                        })
                    } else if let Some(snapshot) = snapshot {
                        crate::viewport::snapping::surface::clipmap_hit(&snapshot, snapshot.ray_intersection(ray.origin, *ray.direction), key.snap)
                    } else if let (Some(mesh), Some(index)) = (mesh, index) {
                        index
                            .ray_intersection(
                                &mesh.positions,
                                &mesh.indices,
                                ray.origin.to_array(),
                                (*ray.direction).to_array(),
                            )
                            .map_or(ClipmapGroundHit::Miss, |hit| {
                                crate::viewport::snapping::surface::mesh_hit(&mesh, &index, Vec3::from_array(hit.position), key.snap, sampler.as_ref())
                            })
                    } else {
                        ClipmapGroundHit::Failed
                    }
                } else {
                    ClipmapGroundHit::Failed
                };
                BatchOutput::Ray(key, hit, target)
            }); // 只有纯立方体且明确零地形使用Y=0工作平面；GLB和未命中真实地形继续失败关闭。
        }
    }
    state.paste = Some(paste);
}

/// 从实际命中对象沿连续承托面吸附；世界射线与网格点不直接查询源局部GLB坐标。
#[cfg(test)]
pub(super) fn object_hit(
    ground: &crate::authoring::terrain_ground::TerrainGroundSnapshot,
    hidden: &std::collections::BTreeSet<feathrise_scene_content::TerrainId>,
    origin: Vec3,
    direction: Vec3,
    snap: feathrise_editor_core::EditorTranslationSnap,
) -> ClipmapGroundHit {
    object_hit_with_target(ground, hidden, origin, direction, snap).0
}

fn object_hit_with_target(
    ground: &crate::authoring::terrain_ground::TerrainGroundSnapshot,
    hidden: &std::collections::BTreeSet<feathrise_scene_content::TerrainId>,
    origin: Vec3,
    direction: Vec3,
    snap: feathrise_editor_core::EditorTranslationSnap,
) -> (ClipmapGroundHit, Option<Arc<feathrise_scene_content::SceneTerrainObject>>) {
    let (id, point) = match ground.ray_visible_checked(origin, direction, hidden) {
        Ok(Some(hit)) => hit,
        Ok(None) => return (ClipmapGroundHit::Miss, None),
        Err(status) => return (status, None), // 上层缺页不能误当Miss后锁定下层对象。
    };
    let target = ground.object_definition(&id).cloned();
    if !snap.active() {
        return (ClipmapGroundHit::Hit(point), target);
    }
    let Some(snapped) = snap.horizontal(point.to_array()).map(Vec3::from_array) else {
        return (ClipmapGroundHit::Failed, None);
    };
    if snapped.xz() == point.xz() {
        return (ClipmapGroundHit::Hit(point), target);
    }
    let hit = ground.object_query(&id).map_or(ClipmapGroundHit::Miss, |query| {
        query.sample_support_along_surface(point, snapped.xz())
    }); // 只允许命中的同对象、同姿态和同连续表面，不跳到其他岛或上层。
    (hit, target)
}

fn make_work(
    paste: &mut PasteSession,
    point: Vec3,
    session: &EditorAuthoringSession,
    env: &BatchEnvironment,
) -> Result<PasteWork, String> {
    let draft = session.draft.as_ref().ok_or("目标场景已离开")?;
    let has_building = paste
        .records
        .iter()
        .any(|record| record.building_preset_id.is_some());
    let validator = if has_building {
        Some(courtyard_validator_snapshot(
            session,
            env.registry.as_deref().ok_or("建筑目录尚未就绪")?,
            env.buildings.as_deref().ok_or("建筑空间索引尚未就绪")?,
            env.clipmap.as_deref(),
        )?)
    } else {
        None
    };
    let work_plane = paste.builtin_only && crate::authoring::basic_shapes::work_plane_is_available(session);
    let sampler = (!work_plane).then(|| PlacementTerrainSampler::from_sources(
        draft,
        env.clipmap.as_deref(),
        session.terrain_revision,
    ).ok()).flatten();
    let presets = paste
        .frozen_presets
        .as_ref()
        .ok_or("粘贴预设快照尚未完整准备")?
        .clone();
    let patch = if paste.source.patch_frames.is_empty() { None } else {
        let target = paste.patch_target.clone().ok_or("三维地形需要命中启用的Clipmap")?;
        if !target.legacy && env.terrain_queries.instance(&draft.scene_id, &target.object.id).is_none() {
            return Err("目标Clipmap实例正在换代，请等待完整地形后重试".into());
        }
        Some(PatchPasteContext {
            frames: paste.source.patch_frames.clone(),
            target,
            existing: paste.budget,
        })
    };
    Ok(PasteWork {
        source: paste.records.clone(),
        delta: point - paste.source.origin,
        records: Vec::new(),
        validator,
        presets,
        sampler,
        work_plane,
        patch,
        query: None,
        bounds: draft.manifest.world_bounds().ok_or("目标场景范围无效")?,
        scene_id: draft.scene_id.clone(),
        building_revision: env
            .registry
            .as_ref()
            .map_or(0, |registry| registry.revision),
    })
}

fn adopt_preview(
    paste: &mut PasteSession,
    state: &mut PlacementBatchState,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    selection: &mut PlacementSelectionSet,
    env: &mut BatchEnvironment,
) {
    let prepared = paste.prepared.as_ref().unwrap();
    let end = (paste.applying + RECORD_BUDGET).min(prepared.apply.len());
    // 先隐藏公共父级，每帧只安装有限根的最终姿态；整组发布前没有半组作者记录或玩法碰撞。
    env.commands
        .entity(paste.parent)
        .try_insert((Visibility::Hidden, Transform::IDENTITY));
    for index in paste.applying..end {
        let record = &prepared.apply[index];
        let transform = crate::authoring::placement_record_bounds(session.draft.as_ref().unwrap(), record)
            .and_then(|bounds| record.validated_transform(&paste.identity.scene_id, bounds));
        let transform = match transform {
            Ok(transform) => transform,
            Err(error) => {
                editor.status = format!("目标地形已变化，整批粘贴未应用：{error}");
                state.cleaning.push(paste.parent);
                state.cleaning.append(&mut paste.roots);
                state.cleaning.extend(paste.frames.iter().flatten().copied());
                let prepared = paste.prepared.take().unwrap();
                retire(state, paste.identity.clone(), prepared);
                return;
            }
        };
        env.commands
            .entity(paste.roots[index])
            .remove::<PasteGhost>()
            .insert((
                transform,
                Visibility::Inherited,
                ScenePlacementInstance::new(&paste.identity.scene_id, record),
            ));
        if !record.water_surfaces.is_empty() {
            env.commands
                .entity(paste.roots[index])
                .insert(ScenePlacementWaterEvidence);
        }
    }
    paste.applying = end;
    if end < prepared.apply.len() {
        editor.status = format!("完成粘贴模型 {end}/{}…", prepared.apply.len());
        return;
    }
    let prepared = paste.prepared.take().unwrap();
    let draft = session.draft.as_mut().unwrap();
    let invalid = draft.placements.placements.len() + prepared.apply.len()
        > MAX_SCENE_PLACEMENTS_PER_SCENE
        || draft.placement_water_surface_count + prepared.water_count
            > MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_SCENE;
    if invalid {
        editor.status = "目标场景容量已变化，整批粘贴未应用".into();
        state.cleaning.push(paste.parent);
        state.cleaning.append(&mut paste.roots);
        state.cleaning.extend(paste.frames.iter().flatten().copied());
        retire(state, paste.identity.clone(), prepared);
        return;
    }
    for (root, record) in paste.roots.iter().zip(&prepared.apply).filter(|(_, record)| record.terrain_patch.is_some()) {
        // 所有模型都已在隐藏容器内就绪；此帧才交给真实地形父，保留源局部TRS和保存联合接管身份。
        env.commands.entity(*root).insert(ChildOf(paste.parent));
        if let Err(error) = crate::authoring::attach_patch_preview(
            &mut env.commands, session.draft.as_ref().unwrap(), &env.terrain_queries, *root, record,
        ) {
            editor.status = format!("目标地形尚未就绪，整批粘贴未应用：{error}");
            state.cleaning.push(paste.parent);
            state.cleaning.append(&mut paste.roots);
            state.cleaning.extend(paste.frames.iter().flatten().copied());
            retire(state, paste.identity.clone(), prepared);
            return;
        }
    }
    // 多选保持浏览器单选为空，避免旧单对象数值控件只编辑组内第一项。
    let first_id = if prepared.apply.len() == 1 {
        prepared.apply[0].id.clone()
    } else {
        String::new()
    };
    // 普通资产根已分帧安装完毕；这里只发布作者索引，整组画面由一个父级显隐门原子放行。
    // 当前建筑单体复制在采用分支才安装碰撞，不在未确认的预览阶段参与物理。
    for (root, record) in paste.roots.iter().zip(&prepared.apply) {
        if let Some(collision) = feathrise_scene::building_placement_collision(record) {
            env.commands.entity(*root).insert(collision);
        }
        session.placement_previews.insert(record.id.clone(), *root);
    }
    session
        .placement_previews
        .insert(paste.parent_key.clone(), paste.parent);
    let draft = session.draft.as_mut().unwrap();
    draft.placement_water_surface_count += prepared.water_count;
    draft.placements.placements.extend(prepared.apply);
    session.push_bounded_water_history(EditorCommand::CourtyardBatch(Box::new(prepared.history)));
    session.mark_placement_structure_changed(editor);
    selection.publish(prepared.members, session);
    state.selection_request = Some(first_id);
    env.commands
        .entity(paste.parent)
        .remove::<PasteGhost>()
        .remove::<avian3d::prelude::RigidBodyDisabled>()
        .insert((
            Visibility::Inherited,
            PastePreviewGroup {
                scene_id: paste.identity.scene_id.clone(),
                key: paste.parent_key.clone(),
            },
        ));
    feathrise_scene::set_builtin_placement_batch_enabled(&mut env.commands, paste.parent, true);
    for frame in paste.frames.iter().flatten() {
        env.commands.entity(*frame).try_despawn();
    } // 真父级接管后释放源预览框架；删除最后一个普通成员时原有group观察者继续回收空容器。
    editor.status = "已粘贴完整对象组；Ctrl+Z 一次撤销，源场景对象保持独立".into();
    retire(
        state,
        BatchIdentity::from_session(session, content(env, session)).unwrap(),
        (paste.source.clone(), paste.records.clone()),
    );
}
