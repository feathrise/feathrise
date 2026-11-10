//! 在共享不可变作者数据上计算跨类型命中；大场景角色搜索不进入点击帧。

use super::layers::*; // 完整请求与结果身份。
use crate::authoring::author_layers_at_world; // 使用与编译完全一致的贡献规则。
use crate::authoring::EditorAuthoringSession;
use crate::game_authoring::scene_interaction::{SceneInteractionState, SceneInteractionSnapshot}; // 外部作者层选择端口。
use crate::resources::SceneEditorState; // 全局编辑生命周期门。
use crate::viewport::modes::{SelectionFilter, ViewportModeState}; // 唯一模式及筛选。
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
}; // 有界后台与非阻塞轮询。
use feathrise_scene_content::{SceneVegetationAsset}; // 轻量文档快照和Arc角色资产。
use std::{collections::BTreeMap, sync::Arc}; // 像素只增加引用计数。 // 与正式组规则、覆盖合并和池化完全一致。

fn identity(
    session: &EditorAuthoringSession,
    domain: &SceneInteractionState,
    ticket: u64,
    filter: SelectionFilter,
) -> Option<Identity> {
    // 快照仅含常数元数据。
    let draft = session.draft.as_ref()?; // 无草稿不能选择。
    Some(Identity {
        scene_id: draft.scene_id.clone(),
        revision: session.revision,
        placement_derived_pose_revision: session.placement_derived_pose_revision,
        domain_hash: draft.adopted_domain_hash().to_owned(),
        generation: domain.observed_generation,
        ticket,
        filter,
    }) // 同图重新进入仍换代。
}

fn surface_hits(canvas: &super::layer_canvas::Snapshot, world: Option<Vec3>, distance: f32) -> Vec<LayerCandidate> {
    // 单像素固定四槽，无整图扫描。
    let Some(world) = world else {
        return Vec::new();
    }; // 没有真实地面命中就没有地表候选。
    let Some(surface) = canvas.surface.as_ref() else {
        return Vec::new();
    }; // 不造地形伪作者层。
    let Some((indices, weights)) = canvas
        .indices
        .as_ref()
        .zip(canvas.weights.as_ref())
    else {
        return Vec::new();
    }; // 配对源同时有效。
    let min = Vec2::from_array(canvas.min); // 所点网格的源局部范围，不是空场景容器的1米占位范围。
    let max = Vec2::from_array(canvas.max);
    let uv = (Vec2::new(world.x, world.z) - min) / (max - min); // 与高度/笔刷同坐标。
    if !uv.is_finite()
        || uv.cmplt(Vec2::ZERO).any()
        || uv.cmpgt(Vec2::ONE).any()
        || surface.resolution.contains(&0)
    {
        return Vec::new();
    } // 不夹紧场景外落点。
    let x = (uv.x * surface.resolution[0].saturating_sub(1) as f32).round() as usize; // 最近纹素。
    let z = (uv.y * surface.resolution[1].saturating_sub(1) as f32).round() as usize; // 最近行。
    let Some(pixel) = z
        .checked_mul(surface.resolution[0] as usize)
        .and_then(|row| row.checked_add(x))
    else {
        return Vec::new();
    }; // 防御坏尺寸。
    let mut candidates: Vec<LayerCandidate> = Vec::with_capacity(4); // 每像素最多四层贡献。
    for slot in 0..4 {
        // 固定成本。
        let Some(weight) = pixel
            .checked_mul(4)
            .and_then(|p| weights.get(p + slot))
            .copied()
            .filter(|w| *w > 0)
        else {
            continue;
        }; // 零权重不命中。
        let Some(offset) = pixel.checked_mul(16).and_then(|p| p.checked_add(slot * 4)) else {
            continue;
        }; // RGBA32索引偏移。
        let Some(bytes) = indices.get(offset..offset + 4) else {
            continue;
        }; // 严格检查源边界。
        let index = u32::from_le_bytes(bytes.try_into().expect("四字节切片")) as usize; // 与正式索引图同字节序。
        let Some(layer) = surface.layers.get(index) else {
            continue;
        }; // 删除后的坏索引不能选错层。
        let target = LayerTarget::Surface(layer.id.clone()); // 同预设不同作者层保留独立身份。
        if !candidates.iter().any(|c| c.target == target) {
            candidates.push(LayerCandidate {
                author_index: Some(index),
                target,
                label: format!("地表 · {}", layer.label),
                rank: 2,
                distance: 255.0 - weight as f32,
            });
        } // 权重更高的地表优先。
    }
    candidates.sort_by(|a, b| a.distance.total_cmp(&b.distance)); // 同面仍先显示贡献更高的地表。
    for candidate in &mut candidates { candidate.distance = distance; }
    candidates // 排序对外只比较真实射线深度，不把纹素权重当成空间米数。
}

pub(crate) fn poll_layer_candidates(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    domain: Res<SceneInteractionState>,
    modes: Res<ViewportModeState>,
    mesh_edit: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
    mut state: ResMut<LayerCandidateState>,
    mut selection_locked: Local<bool>, // 只在进入变换时淘汰旧点击票据，稳定帧不重建选择。
    mut locked_scene: Local<Option<feathrise_scene_content::SceneId>>, // confirm_terrain没有候选身份时，也不能把primary选中项带到另一场景。
    multi_selection: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    subscenes: super::subscenes::PickInputs,
    mut terrains: Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    documents: Option<Res<crate::authoring::terrain_documents::TerrainDocumentTask>>,
) {
    // 唯一single-flight调度出口。
    let filter = modes.selection_filter(&editor); // 保留选择时使用的类别身份。
    let ready = state.task.as_mut().and_then(check_ready); // 关闭也回收完成任务。
    let ready_identity = ready.as_ref().and_then(|_| state.task_identity.take());
    if ready.is_some() {
        state.task = None;
    } // 只在任务真实完成后释放槽。
    let available = editor.open
        && !editor.close_requested
        && !editor.new_scene_dialog_open
        && session.draft.is_some();
    let editing_terrain = modes.mode == crate::viewport::modes::ViewportMode::Edit
        && matches!(state.selected, Some(LayerTarget::TerrainObject(_)));
    if available && (modes.mode.transforms_placement() || editing_terrain || mesh_edit.as_ref().is_some_and(|state| state.active())) {
        if !*selection_locked {
            state.freeze_selection();
            *locked_scene = session.draft.as_ref().map(|draft| draft.scene_id.clone());
        }
        *selection_locked = true;
        let terrain_removed = editing_terrain && !session.draft.as_ref().is_some_and(|draft| {
            if let Some(objects) = &draft.manifest.terrains {
                state.selected_author_index.and_then(|index| objects.get(index))
                    .is_some_and(|object| matches!(state.selected.as_ref(), Some(LayerTarget::TerrainObject(id)) if id == object.id.as_str()))
            } else {
                state.selected_author_index == Some(0) && !draft.manifest.terrain.is_empty()
                    && matches!(state.selected.as_ref(), Some(LayerTarget::TerrainObject(id)) if id == feathrise_scene_content::TerrainId::legacy().as_str())
            }
        });
        let scene_changed = session.draft.as_ref().is_none_or(|draft| locked_scene.as_ref() != Some(&draft.scene_id));
        if terrain_removed || scene_changed || state.identity.as_ref().is_some_and(|old| {
            session.draft.as_ref().is_none_or(|draft| old.scene_id != draft.scene_id)
                || editor.selected_scene_id.as_ref() != Some(&old.scene_id)
                || old.generation != domain.observed_generation
        }) {
            state.clear(); // 切换场景或角色会话换代仍淘汰旧身份，不能把选择锁延续到另一个场景。
        } else {
            if editing_terrain && let Some(old) = state.identity.as_mut() {
                old.revision = session.revision; // 只更新已确认网格的版本；回到选择模式后也不因连续雕刻丢失属性目标。
            }
            state.invalidate_derived_pose(&session, domain.observed_generation, filter);
        }
        return; // 编辑/变换保留已确认网格；高度修订不能清空属性目标，迟到点击仍不得抢选。
    }
    *selection_locked = false;
    *locked_scene = None;
    let active = available && modes.selection_allowed(&editor); // 编辑模式不能采纳旧点击。
    if multi_selection.as_ref().is_some_and(|selection| selection.box_busy()) {
        state.freeze_selection(); return; // 拖框期间旧单击（含空命中）不得改变现有选集。
    }
    if !active {
        if state.identity.is_some() || state.request.is_some() || state.waiting_canvas.is_some() || state.selected.is_some() {
            state.clear();
        }
        return;
    } // 保留在途句柄但淘汰资格。
    state.invalidate_derived_pose(&session, domain.observed_generation, filter);
    let waiting_identity = state.waiting_canvas.as_ref().and_then(|_| identity(&session, &domain, state.ticket, filter));
    if let Some(wait) = state.waiting_canvas.as_mut() {
        let valid = waiting_identity.as_ref() == Some(&wait.request.identity)
            && wait.request.hit_terrain.as_ref().is_some_and(|(id, index)| terrains.as_ref().is_some_and(|terrain|
                terrain.selected.as_ref() == Some(id) && terrain.selected_index == Some(*index)))
            && (Arc::ptr_eq(&wait.request.hidden_terrains, &editor.terrain_hidden)
                || wait.request.hidden_terrains.is_empty() && editor.terrain_hidden.is_empty());
        let loading = documents.as_ref().is_some_and(|documents|
            wait.request.hit_terrain.as_ref().is_some_and(|(id, index)| documents.loading_for(
                &wait.request.identity.scene_id, id, *index, wait.request.identity.revision)));
        if !valid || (wait.observed_loading && !loading) {
            state.waiting_canvas = None; // 切换场景、换目标、失败或作者换代永久取消，不能在未来载入时复活。
        } else { wait.observed_loading |= loading; }
    }
    if state.identity.as_ref().is_some_and(|old| {
        identity(&session, &domain, old.ticket, filter).as_ref() != Some(old)
    }) {
        let pending = state.request.take().filter(|request| {
            identity(&session, &domain, request.ticket, filter).as_ref()
                == Some(&request.identity)
        }); // 保留切换后刚点下的有效请求。
        state.clear();
        if let Some(mut request) = pending {
            request.ticket = state.ticket;
            request.identity.ticket = state.ticket;
            state.request = Some(request);
        } // 新取消票据仍保证唯一性。
    } // 切换场景、修订、筛选变化清理旧候选。
    if let Some((mut key, mut request, candidates)) = ready {
        if let Some(rebound) = ready_identity {
            let mut comparable = key.clone(); comparable.revision = rebound.revision;
            if comparable == rebound { key = rebound; request.identity = key.clone(); }
        } // 只有正式画布采用的显式修订转接可以继续；旧worker仍必须真实完成才归还槽位。
        // 只发布完整结果。
        if identity(&session, &domain, state.ticket, filter).as_ref() == Some(&key)
            && (Arc::ptr_eq(&request.hidden_terrains, &editor.terrain_hidden)
                || request.hidden_terrains.is_empty() && editor.terrain_hidden.is_empty()) {
            let active = session.draft.as_ref().and_then(|draft| draft.terrain_documents.active.as_ref())
                .map(|active| &active.definition.id);
            if super::layer_canvas::needs_author_canvas(filter)
                && let Some((id, index)) = request.hit_terrain.as_ref()
                && (request.canvas_terrain.as_ref() != Some(id) || active != Some(id)) {
                if active == Some(id) && !session.draft.as_ref().unwrap().terrain_documents.loading {
                    state.request = Some(request); // 旧画布完成期间的新点击，用刚完成的画布重采样同一射线。
                } else if let Some(terrains) = terrains.as_mut() {
                    terrains.selected = Some(id.clone()); terrains.selected_index = Some(*index);
                    state.waiting_canvas = Some(CanvasWait { request, observed_loading: false });
                    editor.status = "正在载入所点网格作者画布，完成后继续选择最上层".into();
                }
                return; // 不提前公布只有网格的半套候选，整次点击只确认最终最上层一次。
            }
            if super::layer_canvas::needs_author_canvas(filter) && request.canvas_terrain.as_ref() != active {
                state.request = Some(request); return;
            }
            // 所有身份与最新点击一致才采用。
            let repeat = !request.additive && state
                .pixel
                .is_some_and(|p| p.distance_squared(request.pixel) <= 36.0)
                && state.candidate_canvas.as_deref() == request.canvas_definition.as_deref()
                && state.candidate_water_canvas.as_deref() == request.water_canvas_definition.as_deref()
                && state
                    .candidates
                    .iter()
                    .map(|c| &c.target)
                    .eq(candidates.iter().map(|c| &c.target)); // 同点稳定候选顺序才能轮选。
            let index = if repeat && !candidates.is_empty() {
                (state.index + 1) % candidates.len()
            } else {
                0
            }; // 新点默认第一项。
            state.pixel = Some(request.pixel); // 下一次同点检测。
            state.identity = Some(key); // 与结果原子采用。
            state.candidate_canvas = request.canvas_definition;
            state.candidate_water_canvas = request.water_canvas_definition;
            state.candidates = candidates; // UI下一次只构建八行。
            state.placement_hits = request.placement_hits; // 采用相同票据的场景树网格身份。
            state.additive = request.additive;
            state.page = index / 8; // 卡片直接显示待确认项所在页，正式行仅在成功后发布。
            state.hover = None; // 清除旧菜单悬停。
            state.hover_author_index = None;
            state.apply_candidate_index = state.candidates.get(index).map(|_| index);
            state.apply = state.candidates.get(index).map(|c| c.target.clone()); // 正式选择交给唯一应用系统。
            if state.apply.is_none() {
                state.clear_objects = true; // 空落点也在完整结果采用后统一清除旧对象覆盖。
            } // 确认目标留给apply原子清理；两条Tasks链之间若先贴地，旧空命中仍可安全取消。
            state.ui_revision = state.ui_revision.wrapping_add(1); // 列表完整变化后再更新显示。
        }
    }
    if state.task.is_some() {
        return;
    } // 不通过drop假装取消CPU任务。
    let Some(mut request) = state.request.take() else {
        return;
    }; // 没有新输入就不做计算。
    let Some(key) = identity(&session, &domain, request.ticket, filter) else {
        return;
    }; // 冻结实际草稿身份。
    if key != request.identity {
        return;
    } // 等待期间切换场景/换代的点击失败关闭。
    if matches!(filter, SelectionFilter::All | SelectionFilter::Placements) && !subscenes.capture(&key.scene_id, &mut request.subscenes) {
        state.request = Some(request); return;
    }
    let draft = session.draft.as_ref().expect("身份已校验草稿"); // 与key来自同一借用。
    request.hidden_terrains = editor.terrain_hidden.clone();
    if matches!(
        filter,
        SelectionFilter::All | SelectionFilter::Gameplay
    ) && !request.gameplay.capture_batch(draft)
    {
        state.request = Some(request); // 每帧有界捕获，身份变化会整批丢弃；等待时不发布半套结果。
        return;
    }
    let canvas = super::layer_canvas::Snapshot::capture(draft);
    request.canvas_terrain = canvas.terrain.clone();
    request.canvas_definition = draft.terrain_documents.active.as_ref().map(|active| active.definition.clone());
    let asset = domain.snapshot_for(draft).cloned(); // 文档和空间索引均为Arc，并与当前场景完整身份一致。
    if draft.domain.is_some() && asset.is_none() {
        state.request = Some(request);
        return; // 新文档尚未发布交互快照时等待，不能把旧域内容绑定到新请求身份。
    }
    state.task_identity = Some(key.clone());
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let mut request = request;
        if let Some(ground) = &request.terrain_ground && let Some((id, point)) = ground.ray_visible(request.ray.origin, *request.ray.direction, &request.hidden_terrains) {
            request.world = Some(point);
            request.hit_terrain = request.terrain_objects.iter().position(|object| object.id == id).map(|index| (id, index));
            if request.preferred.is_none() { request.vegetation_world = Some(Vec2::new(point.x, point.z)); }
            let depth = request.ray.origin.distance(point) + 0.02;
            request.placement_candidates.retain(|candidate| candidate.distance <= depth);
            request.water_candidates.retain(|candidate| candidate.distance <= depth);
        }
        let local = request.world.map(|point| canvas.inverse.transform_point3(point));
        let distance = request.world.map_or(f32::MAX, |point| request.ray.origin.distance(point));
        let vegetation_world = request.vegetation_world.map(|point| {
            let local = canvas.inverse.transform_point3(Vec3::new(point.x, 0.0, point.y)); Vec2::new(local.x, local.z)
        });
        let author_matches = request.terrain_objects.is_empty()
            || request.hit_terrain.as_ref().is_some_and(|(id, _)| canvas.terrain.as_ref() == Some(id));
        let surfaces = if author_matches && matches!(filter, SelectionFilter::All | SelectionFilter::Surface) {
            surface_hits(&canvas, local, distance)
        } else { Vec::new() };
        // 一个worker负责该点击完整候选。
        let candidates = collect_candidates(
            &key, &request, surfaces, author_matches.then_some(canvas.vegetation).flatten(), canvas.densities,
            asset, canvas.min, canvas.max, vegetation_world,
        ); // 跨规模迭代只在后台。
        (key, request, candidates) // 不借用Commands/Assets/World。
    }));
}

fn collect_candidates(
    key: &Identity,
    request: &Request,
    mut candidates: Vec<LayerCandidate>,
    vegetation: Option<SceneVegetationAsset>,
    densities: BTreeMap<String, Arc<Vec<u8>>>,
    domain: Option<Arc<SceneInteractionSnapshot>>,
    min: [f32; 2],
    max: [f32; 2],
    vegetation_world: Option<Vec2>,
) -> Vec<LayerCandidate> {
    if key.filter == SelectionFilter::All && let Some(ground) = &request.terrain_ground
        && let Some((id, point)) = ground.ray_visible(request.ray.origin, *request.ray.direction, &request.hidden_terrains)
        && let Some((index, object)) = request.terrain_objects.iter().enumerate().find(|(_, object)| object.id == id) {
        candidates.push(LayerCandidate { author_index: Some(index), target: LayerTarget::TerrainObject(id.as_str().to_owned()),
            label: format!("网格 · {}", object.display_name), rank: 3, distance: request.ray.origin.distance(point) });
    }
    // 纯数据计算。
    if matches!(key.filter, SelectionFilter::All | SelectionFilter::Gameplay) {
        candidates.extend(
            request
                .gameplay
                .visible_candidates(request.ray, request.world),
        ); // 与所有作者类型共用稳定距离排序和地形遮挡门。
    }
    if matches!(
        key.filter,
        SelectionFilter::All | SelectionFilter::Placements
    ) {
        candidates.extend(request.placement_candidates.iter().cloned()); // 资产与作者层一起排序，不提前消费点击。
    }
    if matches!(key.filter, SelectionFilter::All | SelectionFilter::Water) {
        candidates.extend(request.water_candidates.iter().cloned()); // 水面与其他真实命中共同排序，不提前返回最近水体。
    }
    if matches!(
        key.filter,
        SelectionFilter::All | SelectionFilter::Domain
    ) {
        // 明确筛选才搜索角色。
        for (id, label, distance) in
            domain.as_ref().map_or_else(Vec::new, |snapshot| (snapshot.pick)(request.ray, request.world, request.radius))
        {
            candidates.push(LayerCandidate {
                author_index: None,
                target: LayerTarget::Domain(id),
                label: format!("{} · {label}", domain.as_ref().map_or("", |snapshot| snapshot.label)),
                rank: 0,
                distance,
            });
        } // 每层去重由角色空间域完成。
    }
    if matches!(
        key.filter,
        SelectionFilter::All | SelectionFilter::Vegetation
    ) {
        // 实际贡献的草地是同深度地表的覆盖层；没有贡献、隐藏或被竞争淘汰的作者层不列出。
        if let Some((vegetation, world)) = vegetation.as_ref().zip(vegetation_world) {
            // 植被作者画布只接受真实地面落点。
            if let Some(authoring) = vegetation.authoring.as_ref() {
                // 原正式层不伪装唯一作者来源。
                let contributors: std::collections::HashSet<_> =
                    author_layers_at_world(vegetation, &densities, min, max, world)
                        .into_iter()
                        .collect(); // 后台解析真实贡献后常数查询。
                for (index, layer) in authoring.layers.iter().enumerate() {
                    // 同预设多个作者层分别返回。
                    if !layer.enabled
                        || layer.opacity <= 0.0
                        || !authoring
                            .groups
                            .iter()
                            .any(|g| g.id == layer.group_id && g.enabled)
                    {
                        continue;
                    } // 隐藏组/图层均不参与。
                    if !contributors.contains(&layer.id) {
                        continue;
                    } // 被覆盖的作者层不误入候选。
                    let strong = request.preferred.is_some_and(|id| {
                        vegetation.layers.iter().any(|output| {
                            output.id == layer.output_layer_id
                                && output.runtime_layer_id
                                    == Some(
                                        id & !crate::constants::EDITOR_VEGETATION_PREVIEW_LAYER_BIT,
                                    )
                        })
                    }); // 冠层真实Mesh优先于地表。
                    candidates.push(LayerCandidate {
                        author_index: Some(index),
                        target: LayerTarget::Vegetation(layer.id.clone()),
                        label: format!(
                            "植被 · {}{}",
                            layer.label,
                            if layer.locked { "（锁定）" } else { "" }
                        ),
                        rank: if strong { 0 } else { 1 },
                        distance: if strong { request.preferred_distance.unwrap_or(f32::MAX) }
                            else { request.world.map_or(f32::MAX, |point| request.ray.origin.distance(point)) },
                    }); // 锁定允许查看不解锁。
                }
            }
        }
    }
    if matches!(key.filter, SelectionFilter::All | SelectionFilter::Placements) { candidates.extend(request.subscenes.candidates(request.ray, request.world)); }
    candidates.sort_by(|a, b| {
        a.distance.total_cmp(&b.distance)
            .then_with(|| a.rank.cmp(&b.rank))
    }); // 稳定排序保留同距离作者定义顺序。
    let mut seen = std::collections::BTreeSet::new();
    candidates.retain(|candidate| match &candidate.target { LayerTarget::Subscene(id) => seen.insert(id.clone()), _ => true });
    candidates // 无人为候选数量截断。
}
