//! 建筑新建、移动、旋转与复制请求的联合身份冻结和后台校验提交。
//!
//! 【功能设计】请求只冻结轻量预设、地形与不可变 BVH；同一时间最多保留一个在途任务。

use super::*; // 复用父模块的统一几何校验、状态反馈与建筑作者协议。

/// 左键冻结当前候选并启动唯一后台校验任务。
#[allow(clippy::too_many_arguments)] // 每个身份来源都必须显式进入点击门。
pub(in crate::authoring::buildings) fn request_from_viewport(
    mouse: Res<ButtonInput<MouseButton>>,
    viewport: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    registry: Option<Res<BuildingPresetRegistry>>,
    controls: Res<BuildingPlacementControls>,
    preferences: Option<Res<feathrise_editor_core::EditorSettings>>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    transform_state: Res<PlacementTransformInputState>,
    clipmap: Option<Res<DynamicClipmapGroundSurface>>,
    mut ground_cursor: Option<ResMut<crate::viewport::VegetationGroundCursor>>,
    mut state: ResMut<BuildingAuthoringState>,
    mut building_ui: ResMut<BuildingPresetEditorState>,
) {
    let is_clipmap = session.draft.as_ref().is_some_and(|draft| {
        draft.uses_legacy_clipmap_cursor()
    });
    if is_clipmap
        && (editor.save_requested
            || editor.close_requested
            || editor.new_map_dialog_open
            || editor.suppress_world_pick_this_frame
            || session.saving)
    {
        if let Some(ground) = ground_cursor.as_deref_mut() {
            ground.cancel_building_click();
        }
        return; // 保存或模态抢占只撤销尚未受理的射线意图，已受理地基任务仍独立收口。
    }
    if (!is_clipmap && !mouse.just_pressed(MouseButton::Left) && editor.ground_click_world.is_none())
        || !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || editor.suppress_world_pick_this_frame
        || editor.tool != EditorTool::Building
        || session.saving
        || transform_state.relocation_armed()
        || !viewport
            .single()
            .is_ok_and(RelativeCursorPosition::cursor_over)
    {
        return; // 非建筑工具、保存期、移动模式或视口外点击都不属于本域。
    }
    let click_snap = if is_clipmap {
        ground_cursor.as_ref().and_then(|ground| ground.frozen_click_snap())
    } else {
        preferences.as_ref().map(|settings| controls.snap_policy(settings.editor_snapping, keyboard.as_ref().is_some_and(|keys| keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])), [0.0; 3]))
    };
    let cursor_world = if is_clipmap {
        let target = super::super::ground::cursor_target(
            &editor,
            &controls,
            registry.as_deref().map_or(0, |registry| registry.revision),
        );
        let completion = clipmap
            .as_deref()
            .and_then(|surface| surface.snapshot.as_ref())
            .zip(ground_cursor)
            .zip(target)
            .and_then(|((snapshot, mut ground), target)| {
                ground.take_author_click_result(
                    snapshot,
                    &snapshot.map_id,
                    target,
                    session.revision,
                )
            });
        match completion {
            Some(feathrise_scene::ClipmapGroundHit::Hit(point)) => Some(point),
            other => {
                let message = match other {
                    Some(feathrise_scene::ClipmapGroundHit::WaitingForHeight) => {
                        Some("建筑落点的Clipmap高度页尚未就绪，请稍后重新单击")
                    }
                    Some(feathrise_scene::ClipmapGroundHit::Miss) => {
                        Some("本次建筑点击没有命中地图内真实地表，请重新选择落点")
                    }
                    Some(feathrise_scene::ClipmapGroundHit::Failed) => {
                        Some("建筑地表查询失败，请重新单击")
                    }
                    None if mouse.just_pressed(MouseButton::Left) => {
                        Some("正在后台查询Clipmap真实地表；命中后校验建筑落点")
                    }
                    _ => None,
                };
                if let Some(message) = message {
                    report(&mut editor, &mut building_ui, message);
                } // 区分查询在途与明确失败；状态栏只由当前建筑入口回报。
                None
            }
        }
    } else {
        editor.ground_click_world.take().or(editor.cursor_world)
    };
    let Some(cursor_world) = cursor_world else {
        if is_clipmap {
            return;
        } // 点击在共享射线槽等待；不得借保留光标或零平面补放。
        report(&mut editor, &mut building_ui, "建筑落点没有命中当前地形");
        return;
    };
    let Some(registry) = registry.as_deref() else {
        report(&mut editor, &mut building_ui, "建筑预设目录尚未加载");
        return;
    };
    let Some(identity) = current_identity(&session, registry) else {
        report(&mut editor, &mut building_ui, "建筑空间身份尚未就绪");
        return;
    };
    let Some(draft) = session.draft.as_ref() else {
        report(&mut editor, &mut building_ui, "地图作者数据尚未就绪");
        return;
    };
    if editor.selected_map_id.as_ref() != Some(&draft.map_id) {
        report(
            &mut editor,
            &mut building_ui,
            "正在切换地图作者数据，请稍候",
        );
        return;
    }
    if draft.placements.placements.len() >= MAX_MAP_PLACEMENTS_PER_MAP {
        report(
            &mut editor,
            &mut building_ui,
            &format!("当前地图已达到{MAX_MAP_PLACEMENTS_PER_MAP}个显式摆放上限"),
        );
        return;
    }
    if state.validation_task.is_some() || state.validation_waiting.is_some() {
        report(
            &mut editor,
            &mut building_ui,
            "上一处建筑落点仍在后台校验，请等待完成",
        );
        return; // 单在途任务提供明确背压，不排队无限点击。
    }
    if state.active_identity.as_ref() != Some(&identity) {
        let message = state
            .index_error
            .as_ref()
            .filter(|(failed, _)| failed == &identity)
            .map_or_else(
                || "建筑空间索引正在分帧准备，请稍候再落点".to_owned(),
                |(_, error)| format!("建筑空间索引不可用：{error}"),
            );
        report(&mut editor, &mut building_ui, &message);
        return;
    }
    let Some(index) = state.active_index.as_ref().cloned() else {
        report(
            &mut editor,
            &mut building_ui,
            "建筑空间索引尚未形成完整快照",
        );
        return;
    };
    let Some(selected_id) = editor.building_selected_preset_id.as_deref() else {
        report(&mut editor, &mut building_ui, "请先选择一个建筑预设");
        return;
    };
    let Some(preset) = registry.resolve(selected_id).cloned() else {
        report(&mut editor, &mut building_ui, "所选建筑预设已不存在");
        return;
    };
    editor.building_selected_preset_id = Some(preset.id.clone()); // 别名在新操作边界迁移为规范 ID。
    let pose = super::super::snapping::resolve_building_pose_snapped(
        cursor_world,
        &preset,
        &controls,
        Some(&identity),
        &state,
        None,
        click_snap,
    ); // 与模型幽灵、占地和连接点Gizmo共同使用唯一姿态解析。
    let snap_feedback = match pose.connection {
        BuildingConnectionSnapState::Failed { reason, .. } => {
            report(&mut editor, &mut building_ui, reason.message());
            return; // 显式吸附模式没有候选时不静默退回普通网格点放。
        }
        BuildingConnectionSnapState::Snapped(snap) => {
            let Some(target_entry) = index.entry(snap.target_entry_index) else {
                report(&mut editor, &mut building_ui, "模块吸附目标已经失效");
                return;
            };
            let Some(target) = target_entry.connections.get(snap.target_connection_index) else {
                report(&mut editor, &mut building_ui, "模块吸附目标连接点已经失效");
                return;
            };
            let Some(source) = preset.connections.get(snap.source_connection_index) else {
                report(&mut editor, &mut building_ui, "模块源连接点已经失效");
                return;
            };
            Some(format!(
                "连接 {}:{} → {}:{}",
                source.kind, source.id, target_entry.id, target.id,
            ))
        }
        BuildingConnectionSnapState::Disabled => None,
    };
    let sampler = match PlacementTerrainSampler::from_sources(
        draft,
        clipmap.as_deref(),
        session.terrain_revision,
    ) {
        Ok(sampler) => sampler.for_selected_surface(cursor_world),
        Err(error) => {
            report(
                &mut editor,
                &mut building_ui,
                &format!("建筑地基无法采样：{error}"),
            );
            return; // 首期精确建筑不在未知旧GLTF基准面上猜高度。
        }
    };
    let map_bounds = Rect::from_corners(
        Vec2::from_array(draft.manifest.world_min), // 清单世界 X/Z 最小角。
        Vec2::from_array(draft.manifest.world_max), // 清单世界 X/Z 最大角。
    );
    let terrain_hit = editor.cursor_triangle.map(|triangle| BuildingTerrainHit {
        position: cursor_world, // 世界点与三角来自同一帧视口射线。
        triangle,               // 真实拓扑身份用于隔离洞穴层。
    });
    let level_terrain = controls.level_terrain && !sampler.is_volume_layer();
    let terrain_frame = if level_terrain {
        match super::super::terrain_coordinates::freeze(&session, &editor) {
            Ok(frame) => frame,
            Err(error) => { report(&mut editor, &mut building_ui, &error); return; }
        }
    } else { None };
    let terrain_leveling = if level_terrain && draft.terrain_manifest().terrain.kind != feathrise_map_content::MapTerrainKind::SparseClipmap {
        match super::super::leveling::input_from_selected_surface(draft, terrain_hit, Some(cursor_world)) {
            Ok(input) => Some(input), // Arc浅冻结后随请求移入CPU池。
            Err(error) => {
                report(
                    &mut editor,
                    &mut building_ui,
                    &format!("无法联合整平：{error}"),
                );
                return;
            }
        }
    } else {
        None // 默认关闭时后台任务不取得GLB网格Arc。
    };
    state.next_ticket = state.next_ticket.wrapping_add(1).max(1);
    let request = BuildingPlacementRequest {
        terrain_frame,
        identity,                                      // 冻结地图、作者、结构和目录修订。
        ticket: state.next_ticket,                     // 同身份重复点击也拒绝旧完成值。
        preset,                                        // 小型完整语义快照移入任务。
        world_xz: pose.world_xz, // 普通网格或连接吸附后的根位置与预览完全一致。
        yaw_degrees: pose.yaw_degrees, // 连接模式可自动采用仍受限于90°的对齐角。
        scale: [1.0; 3],         // 新建筑首期固定单位缩放，地基与门口语义不会被拉伸。
        exclude_id: None,        // 新建不排除任何现有建筑。
        operation: BuildingPlacementOperation::Create, // 成功后新增一条规范建筑记录。
        level_terrain, // 三维洞层保持原保护；独立普通地形按实际作者对象整平。
        terrain_leveling, // 真实命中层和当前GLB只在显式开启时存在。
        ground_content: sampler.clipmap_content_revision(),
        input_controls: Some(super::super::ground::controls_identity(&controls)),
    };
    let task_request = request.clone(); // 状态反馈仍可读取短身份，任务拥有独立小快照。
    state.last_validation = None; // 旧红色结果不能继续冒充本次点击。
    state.validation_target = None; // 新建建筑不持有已有实例的临时移动预览。
    state.validation_task = Some(AsyncComputeTaskPool::get().spawn(async move {
        validate_with_ground(task_request, index.as_ref(), &sampler, map_bounds, None).into()
    }));
    let message = snap_feedback.map_or_else(
        || {
            format!(
                "正在后台校验“{}”落点：占地、门口、边界与五点坡差…",
                request.preset.label,
            )
        },
        |snap| {
            format!(
                "已{snap}；正在校验“{}”的占地、边界与五点坡差…",
                request.preset.label,
            )
        },
    );
    report(&mut editor, &mut building_ui, &message);
}

/// 消费普通属性面板发来的已有建筑操作，并冻结同一BVH身份启动后台校验。
#[allow(clippy::too_many_arguments)]
pub(in crate::authoring::buildings) fn request_existing_edit(
    mut requests: MessageReader<BuildingEditRequest>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    registry: Option<Res<BuildingPresetRegistry>>,
    controls: Res<BuildingPlacementControls>,
    clipmap: Option<Res<DynamicClipmapGroundSurface>>,
    mut state: ResMut<BuildingAuthoringState>,
    mut building_ui: ResMut<BuildingPresetEditorState>,
) {
    let mut latest = None;
    for request in requests.read() {
        latest = Some(request.clone()); // 同帧多个边沿只保留最后一个明确用户意图。
    }
    let Some(request) = latest else {
        return; // 稳定帧没有跨域操作。
    };
    if session.saving {
        report(
            &mut editor,
            &mut building_ui,
            "地图正在保存，建筑操作已取消",
        );
        return;
    }
    if state.validation_task.is_some() || state.validation_waiting.is_some() {
        report(
            &mut editor,
            &mut building_ui,
            "上一项建筑操作仍在后台校验，请等待完成",
        );
        return; // 单在途槽拒绝堆积按钮或视口请求。
    }
    let Some(registry) = registry.as_deref() else {
        report(&mut editor, &mut building_ui, "建筑预设目录尚未加载");
        return;
    };
    let Some(identity) = current_identity(&session, registry) else {
        report(&mut editor, &mut building_ui, "建筑空间身份尚未就绪");
        return;
    };
    let (target, requested_xz, duplicate, terrain_hit) = match &request {
        BuildingEditRequest::Relocate {
            target,
            world_xz,
            terrain_hit,
        } => (target, Some(*world_xz), false, *terrain_hit),
        BuildingEditRequest::RotateQuarterTurn { target } => (target, None, false, None),
        BuildingEditRequest::RotateTo { target, .. } => (target, None, false, None),
        BuildingEditRequest::DuplicateAt {
            target,
            world_xz,
            terrain_hit,
        } => (target, Some(*world_xz), true, *terrain_hit),
    };
    if !edit_target_is_current(target, &session, &browser) {
        report(
            &mut editor,
            &mut building_ui,
            "建筑操作目标、选择或作者修订已经变化，请重新操作",
        );
        return;
    }
    if state.active_identity.as_ref() != Some(&identity) {
        let message = state
            .index_error
            .as_ref()
            .filter(|(failed, _)| failed == &identity)
            .map_or_else(
                || "建筑空间索引正在准备，请稍候再操作".to_owned(),
                |(_, error)| format!("建筑空间索引不可用：{error}"),
            );
        report(&mut editor, &mut building_ui, &message);
        return;
    }
    let Some(index) = state.active_index.as_ref().cloned() else {
        report(
            &mut editor,
            &mut building_ui,
            "建筑空间索引尚未形成完整快照",
        );
        return;
    };
    let Some(draft) = session.draft.as_ref() else {
        report(&mut editor, &mut building_ui, "地图作者草稿尚未就绪");
        return;
    };
    if duplicate && draft.placements.placements.len() >= MAX_MAP_PLACEMENTS_PER_MAP {
        report(
            &mut editor,
            &mut building_ui,
            &format!("当前地图已达到{MAX_MAP_PLACEMENTS_PER_MAP}个显式摆放上限"),
        );
        return;
    }
    let Some(source) = draft
        .placements
        .placements
        .get(target.record_index)
        .filter(|record| record.id == target.placement_id && record.editor.selectable())
        .cloned()
    else {
        report(&mut editor, &mut building_ui, "建筑记录已经失效");
        return;
    };
    let Some(preset_id) = source.building_preset_id.as_deref() else {
        report(&mut editor, &mut building_ui, "所选实例不是建筑预设实例");
        return;
    };
    let Some(preset) = registry.resolve(preset_id).cloned() else {
        report(&mut editor, &mut building_ui, "所选建筑引用的预设已不存在");
        return;
    };
    if source.asset_path != preset.asset_path || source.asset_target != preset.asset_target {
        report(
            &mut editor,
            &mut building_ui,
            "所选建筑模型来源与预设不一致，不能继续变换",
        );
        return;
    }
    let sampler = match PlacementTerrainSampler::from_sources(
        draft,
        clipmap.as_deref(),
        session.terrain_revision,
    ) {
        Ok(sampler) => sampler,
        Err(error) => {
            report(
                &mut editor,
                &mut building_ui,
                &format!("建筑地基无法采样：{error}"),
            );
            return;
        }
    };
    let Some(map_bounds) = draft.manifest.world_bounds() else {
        report(&mut editor, &mut building_ui, "当前地图世界范围无效");
        return;
    };
    let world_xz = requested_xz.unwrap_or(Vec2::new(source.translation[0], source.translation[2]));
    let selected = requested_xz
        .and_then(|_| {
            editor
                .brush_cursor_world
                .or(editor.cursor_world)
                .filter(|p| Vec2::new(p.x, p.z) == world_xz)
        })
        .unwrap_or(Vec3::new(
            world_xz.x,
            source.translation[1] + preset.foundation.model_base_y * source.scale[1],
            world_xz.y,
        ));
    let sampler = sampler.for_selected_surface(selected);
    let level_terrain =
        controls.level_terrain && requested_xz.is_some() && !sampler.is_volume_layer(); // 洞内固定承托不改洞顶。
    let terrain_frame = if level_terrain {
        match super::super::terrain_coordinates::freeze(&session, &editor) {
            Ok(frame) => frame,
            Err(error) => { report(&mut editor, &mut building_ui, &error); return; }
        }
    } else { None };
    let terrain_leveling = if level_terrain && draft.terrain_manifest().terrain.kind != feathrise_map_content::MapTerrainKind::SparseClipmap {
        match super::super::leveling::input_from_selected_surface(draft, terrain_hit, Some(selected)) {
            Ok(input) => Some(input),
            Err(error) => {
                report(
                    &mut editor,
                    &mut building_ui,
                    &format!("无法联合整平：{error}"),
                );
                return;
            }
        }
    } else {
        None
    };
    let yaw_degrees = match request {
        BuildingEditRequest::RotateQuarterTurn { .. } => {
            (source.yaw_degrees + 90.0).rem_euclid(360.0)
        }
        BuildingEditRequest::RotateTo { yaw_degrees, .. } => {
            if !yaw_degrees.is_finite()
                || (yaw_degrees - (yaw_degrees / 90.0).round() * 90.0).abs() > 0.001
            {
                report(&mut editor, &mut building_ui, "建筑旋转只允许90°整档角度");
                return;
            }
            yaw_degrees.rem_euclid(360.0)
        }
        _ => source.yaw_degrees.rem_euclid(360.0),
    };
    let edit_target = match &request {
        BuildingEditRequest::Relocate { target, .. }
        | BuildingEditRequest::RotateTo { target, .. } => {
            Some((target.clone(), world_xz, yaw_degrees))
        }
        _ => None,
    }; // 受理时才冻结确切手柄姿态；旧旋转按钮、复制和早期拒绝不能接管候选模型。
    let operation = match request {
        BuildingEditRequest::Relocate { .. } => BuildingPlacementOperation::ChangeExisting {
            record_index: target.record_index,
            before: source.clone(),
            kind: BuildingChangeKind::Relocate,
        },
        BuildingEditRequest::RotateQuarterTurn { .. } => {
            BuildingPlacementOperation::ChangeExisting {
                record_index: target.record_index,
                before: source.clone(),
                kind: BuildingChangeKind::RotateQuarterTurn,
            }
        }
        BuildingEditRequest::RotateTo { .. } => BuildingPlacementOperation::ChangeExisting {
            record_index: target.record_index,
            before: source.clone(),
            kind: BuildingChangeKind::RotateSnapped,
        },
        BuildingEditRequest::DuplicateAt { .. } => BuildingPlacementOperation::DuplicateExisting {
            record_index: target.record_index,
            source: source.clone(),
        },
    };
    state.next_ticket = state.next_ticket.wrapping_add(1).max(1);
    let request = BuildingPlacementRequest {
        terrain_frame,
        identity,
        ticket: state.next_ticket,
        preset,
        world_xz,
        yaw_degrees,
        scale: source.scale,
        exclude_id: (!duplicate).then(|| source.id.clone()),
        operation,
        level_terrain,
        terrain_leveling,
        ground_content: sampler.clipmap_content_revision(),
        input_controls: Some(super::super::ground::controls_identity(&controls)),
    };
    let task_request = request.clone();
    state.last_validation = None;
    state.validation_target = edit_target;
    state.validation_task = Some(AsyncComputeTaskPool::get().spawn(async move {
        validate_with_ground(task_request, index.as_ref(), &sampler, map_bounds, None).into()
    }));
    report(
        &mut editor,
        &mut building_ui,
        &format!(
            "正在后台校验建筑“{}”的占地、门口与五点地基…",
            request.preset.label
        ),
    );
}

/// UI冻结目标仍必须精确对应当前选择、地图、结构和作者修订。
fn edit_target_is_current(
    target: &BuildingEditTarget,
    session: &EditorAuthoringSession,
    browser: &EditorGltfBrowserState,
) -> bool {
    target.structure_revision == session.placement_structure_revision
        && target.author_revision == session.revision
        && browser.selected_placement_id.as_deref() == Some(target.placement_id.as_str())
        && session.draft.as_ref().is_some_and(|draft| {
            draft.map_id == target.map_id
                && draft
                    .placements
                    .placements
                    .get(target.record_index)
                    .is_some_and(|record| record.id == target.placement_id)
        })
}
