//! 网格作者结构只在完整后台结果有效时采用；列表和参数稳定帧仅访问固定行数。
use feathrise_editor_ui::EditorButtonClick;
use super::{components::*, resources::*, types::*};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand},
    resources::SceneEditorState,
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use feathrise_scene_content::{
    SceneTerrainCollection, SceneTerrainDescriptor, SceneTerrainKind, SceneTerrainObject,
};

pub(super) fn enabled(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    state: &TerrainObjectsEditorState,
) -> bool {
    editor.open
        && !session.vegetation_cross_busy
        && !session.saving
        && !session.water_preset_delete_pending
        && !editor.save_requested
        && !editor.close_requested
        && !editor.new_scene_dialog_open
        && !editor.scene_transition_in_progress
        && editor.scene_switch_confirmation.is_none()
        && editor.pending_scene_transition.is_none()
        && session.draft.as_ref().is_some_and(|draft| {
            draft.manifest.is_data_driven()
                && state.scene_id.as_ref().is_none_or(|map| map == &draft.scene_id)
        })
        && !state.busy()
}
pub(crate) fn selected<'a>(
    session: &'a EditorAuthoringSession,
    state: &'a TerrainObjectsEditorState,
) -> Option<&'a SceneTerrainObject> {
    let draft = session.draft.as_ref()?;
    if state.scene_id.as_ref().is_some_and(|map| map != &draft.scene_id) {
        return None;
    }
    let object = if let Some(objects) = &draft.manifest.terrains {
        objects.get(state.selected_index?)?
    } else {
        state
            .legacy
            .as_deref()
            .filter(|_| state.selected_index == Some(0))?
    };
    (Some(&object.id) == state.selected.as_ref()).then_some(object)
}

pub(crate) fn start(
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    state: &mut TerrainObjectsEditorState,
    mutation: Mutation,
) {
    if !enabled(editor, session, state) {
        return;
    }
    // 创建按钮和外部调用共用预检；不让明确无效的规格占用后台槽，也不静默放粗源格距。
    let creation_kind = match &mutation {
        Mutation::Add(kind) => Some(*kind),
        Mutation::ImportGlb(_) => Some(SceneTerrainKind::Gltf),
        _ => None,
    };
    if let Some(kind) = creation_kind {
        if let Err(error) = super::creation::validate_input(kind, &state.creation) {
            editor.status = format!("当前初始规格不能创建该网格：{error}");
            return;
        }
    }
    if matches!(&mutation,Mutation::StitchNearest(..)) && (session.sparse_terrain_dirty || session.draft.as_ref().is_some_and(|d|d.terrain_dirty)) {
        editor.status="请先保存高度编辑，再从完整作者版本建立拼接".into();return;
    }
    let target = match &mutation {
        Mutation::StitchNearest(id, _) | Mutation::Unstitch(id) => Some(id.clone()),
        Mutation::Enabled(id, _)
        | Mutation::Rename(id, _)
        | Mutation::LodPolicy(id, _)
        | Mutation::Brush(id, _)
        | Mutation::Field(id, _)
        | Mutation::Transform(id, _)
        | Mutation::HeightDomain(id, ..)
        | Mutation::Remove(id)
        | Mutation::Value(id, ..)
        | Mutation::AddLod(id, _)
        | Mutation::RemoveLod(id, ..) => Some(id.clone()),
        Mutation::Add(_) | Mutation::ImportGlb(_) => None,
    };
    if target.as_ref().is_some_and(|id| state.locked.contains(id)) {
        editor.status = "所选网格已锁定；解锁后可以修改".into();
        return;
    }
    if matches!(&mutation,Mutation::Unstitch(_)) && selected(session,state).is_some_and(|o|o.clipmap_stitches.iter().any(|s|state.locked.contains(&s.peer))) {
        editor.status="拼接另一侧已锁定，请先解锁后解除关系".into();return;
    }
    let affects_terrain = !matches!(&mutation, Mutation::Rename(_, _));
    let draft = session.draft.as_ref().unwrap();
    if let Mutation::HeightDomain(id, _, mesh) = &mutation
        && selected(session, state).filter(|object| object.id == *id)
            .and_then(|object| crate::authoring::terrain_documents::height_domain_mesh(draft, object))
            .is_none_or(|current| !std::sync::Arc::ptr_eq(current, mesh))
    {
        editor.status = "作者高度域对应的当前网格已换代，请等待所选画布完整载入".into();
        return;
    }
    if target.as_ref() == Some(&feathrise_scene_content::TerrainId::legacy())
        && matches!(
            &mutation,
            Mutation::Brush(_, _) | Mutation::Value(_, 7, _, _)
        )
        && (draft.terrain_dirty || session.sparse_terrain_dirty)
    {
        editor.status = "兼容主网格有未保存的源编辑，请先保存后再修改源数据或格距".into();
        return; // 读盘刷印和格距重采样不能覆盖WIP；姿态、名称和LOD只改元数据，继续允许。
    }
    let owner = draft.manifest.clone();
    let objects = owner.terrain_objects_for_edit();
    let before = SceneTerrainCollection::capture(&owner);
    let map = draft.scene_id.clone();
    let creation = creation_kind.map_or(state.creation, |kind| super::creation::normalized(kind, state.creation));
    let revision = session.revision;
    let stroke = match &mutation {
        Mutation::Brush(_, input) => Some(input.stroke),
        Mutation::Field(_, input) => Some(input.stroke),
        _ => None,
    };
    let lod_navigation = match &mutation {
        Mutation::AddLod(_, display) => Some(LodNavigation::Added { display: *display }),
        Mutation::RemoveLod(_, display, index) => Some(LodNavigation::Removed { display: *display, index: *index }),
        _ => None,
    };
    session.next_serial = session.next_serial.wrapping_add(1);
    let serial = session.next_serial;
    state.pending = Some(Pending {
        preview_stitch:matches!(&mutation,Mutation::StitchNearest(..)),
        stitching:matches!(&mutation,Mutation::StitchNearest(..)|Mutation::Unstitch(_)),
        stroke,
        target,
        affects_terrain,
        lod_navigation,
        scene_id: map,
        revision,
        before,
        task: IoTaskPool::get().spawn(super::jobs::prepare(
            owner, objects, mutation, creation, serial,
        )),
    });
    editor.status = "正在后台准备网格；完整校验后采用".into();
}

pub(super) fn input(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<TerrainObjectsEditorState>,
    buttons: Query<(&EditorButtonClick, &ActionButton, Option<&RowAction>, Option<&LodRowBinding>, Option<&LodActionBinding>), Changed<EditorButtonClick>>,
    rows: Query<(&EditorButtonClick, &Row), Changed<EditorButtonClick>>,
    mut candidates: ResMut<crate::viewport::LayerCandidateState>,
    mut visibility: ResMut<feathrise_scene::TerrainObjectEditorVisibility>,
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>,
    mut sidebar: Option<ResMut<crate::ui::sidebars::SceneEditorSidebarState>>,
    gate: PanelWorkspaceGate,
) {
    // 旧会话可能记着0.010m；先进入公共创建范围，再处理本帧按钮或同步显示，不等创建失败。
    if editor.open && state.creation[7].is_finite() && state.creation[7] < super::creation::MIN_SPACING {
        state.creation[7] = super::creation::MIN_SPACING;
    }
    // 普通按钮和纯选择行统一由共享层确认释放；数值拖动仍由公共数值控件持有。
    if state.stitch_preview.is_some() && editor.open && gate.buttons_allowed() {
        for (interaction,button,_,_,_) in &buttons {if !interaction.is_clicked() {continue;}
            match button.0 {
                Action::ConfirmStitch=>{state.stitch_apply=true;return;}
                Action::CancelStitch=>{state.stitch_cancel=true;return;}
                _=>{},
            }
        }
    }
    if !enabled(&editor, &session, &state) || !gate.creation_buttons_allowed() {
        return;
    }
    // 创建直接作用于当前场景，不改中央标签；隐藏视口和对象属性仍要求场景工作区。
    let scene_workspace = gate.visible();
    let legacy = state.legacy.clone();
    let row_action_pressed = buttons.iter().any(|(interaction, _, row, _, _)| interaction.is_clicked() && row.is_some());
    for (interaction, row) in &rows {
        if !scene_workspace || row_action_pressed || !interaction.is_clicked()
            || sidebar.as_ref().is_some_and(|sidebar| !sidebar.scene_tree_visible())
            || mode
                .as_ref()
                .is_some_and(|mode| !mode.selection_allowed(&editor))
        {
            continue;
        }
        let index = state.list_start + row.0;
        let objects = session
            .draft
            .as_ref()
            .and_then(|draft| draft.manifest.terrains.as_deref())
            .unwrap_or_else(|| legacy.as_deref().map_or(&[], std::slice::from_ref));
        if let Some(object) = objects.get(index) {
            state.selected = Some(object.id.clone());
            state.selected_index = Some(index);
            state.reset_lod_selection();
            candidates.confirm_terrain(&object.id, index);
            if let Some(sidebar) = sidebar.as_mut() {
                sidebar.reveal_scene_properties();
            }
            editor.status = format!("已选择网格：{}", object.display_name);
        }
    }
    for (interaction, button, row_action, lod_binding, lod_action) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        let creation_action = button.0.creation_action();
        if !creation_action && !scene_workspace { continue; }
        if sidebar.as_ref().is_some_and(|sidebar| {
            if creation_action { !sidebar.tool_page_visible(crate::ui::sidebars::EditorLeftSidebarTab::Meshes) }
            else if button.0.scene_list_action() { !sidebar.scene_tree_visible() }
            else { sidebar.right != crate::ui::sidebars::EditorRightSidebarTab::Scene }
        }) { continue; }
        if let Some(row) = row_action {
            // 用上次呈现的身份验证，再选中该行；不能借当前选择或新的list_start把点击改派给另一网格。
            let Some(target) = row.target.as_ref() else { continue; };
            let Some(draft) = session.draft.as_ref().filter(|draft| draft.scene_id == target.scene_id && session.revision == target.revision) else { continue; };
            let objects = draft.manifest.terrains.as_deref()
                .unwrap_or_else(|| legacy.as_deref().map_or(&[], std::slice::from_ref));
            if objects.get(target.index).is_none_or(|object| object.id != target.terrain) { continue; }
            // 行内管理沿用原属性按钮权限，雕刻模式下也能显隐和解锁；它不是视口选择手势。
            if state.selected.as_ref() != Some(&target.terrain) { state.reset_lod_selection(); }
            state.selected = Some(target.terrain.clone());
            state.selected_index = Some(target.index);
            candidates.confirm_terrain(&target.terrain, target.index);
        }
        if !creation_action && !button.0.scene_list_action() && !matches!(candidates.selected.as_ref(),
            Some(crate::viewport::LayerTarget::TerrainObject(id)) if state.selected.as_ref().is_some_and(|selected| selected.as_str() == id)) { continue; }
        let object = selected(&session, &state);
        if !super::presentation::action_available(button.0, object, &state) { continue; }
        if button.0 == Action::CancelPlacement && state.point_placement.is_none()
            && !crate::authoring::basic_shapes::is_cuboid_selection(&editor) { continue; }
        let lod_target = if let Action::AddLod(display) | Action::RemoveLod(display) = button.0 {
            let Some(target) = lod_action.and_then(|binding| binding.0.as_ref()) else { continue; };
            if !session.draft.as_ref().is_some_and(|draft| draft.scene_id == target.scene_id)
                || target.revision != session.revision || state.selected.as_ref() != Some(&target.terrain)
                || target.display != display || target.index != state.lod(display).selected
            { continue; } // 作者采用、撤销或选层发生在旧点击之前时，不能临时借新状态冻结另一笔增删。
            Some(target)
        } else { None };
        let mutation = match button.0 {
            Action::StitchNearest => state.selected.clone().map(|id|{
                let mut blocked=editor.terrain_locked.clone();blocked.extend(editor.terrain_hidden.iter().cloned());Mutation::StitchNearest(id,blocked)
            }),
            Action::ConfirmStitch | Action::CancelStitch => None,
            Action::Unstitch => state.selected.clone().map(Mutation::Unstitch),
            Action::ToggleCreation => {
                state.creation_open = !state.creation_open;
                None
            }
            Action::BasicProperties | Action::TypeProperties | Action::SourceProperties | Action::StitchProperties => {
                let section = match button.0 {
                    Action::TypeProperties => MeshPropertySection::Type,
                    Action::SourceProperties => MeshPropertySection::Source,
                    Action::StitchProperties => MeshPropertySection::Stitch,
                    _ => MeshPropertySection::Basic,
                };
                let open = !state.section_open(section);
                state.set_section_open(section, open);
                None
            }
            Action::DisplayPolicy | Action::CollisionPolicy => selected(&session, &state).map(|object| {
                use feathrise_scene_content::MeshLodMode;
                let mut policy = object.mesh_lod.unwrap_or_default();
                if button.0 == Action::DisplayPolicy {
                    policy.display = match policy.display {
                        MeshLodMode::Inherit => MeshLodMode::Disabled,
                        MeshLodMode::Disabled => MeshLodMode::Override(Default::default()),
                        MeshLodMode::Override(_) => MeshLodMode::Inherit,
                    };
                } else {
                    policy.collision = match policy.collision {
                        MeshLodMode::Inherit => MeshLodMode::Disabled,
                        MeshLodMode::Disabled => MeshLodMode::Override(Default::default()),
                        MeshLodMode::Override(_) => MeshLodMode::Inherit,
                    };
                }
                Mutation::LodPolicy(object.id.clone(), (policy != Default::default()).then_some(policy))
            }),
            Action::Enabled => selected(&session, &state)
                .map(|object| Mutation::Enabled(object.id.clone(), !object.enabled)),
            Action::Visible => {
                if let Some(object) = selected(&session, &state) {
                    let id = object.id.clone();
                    if state.hidden.remove(&id) {
                        std::sync::Arc::make_mut(&mut editor.terrain_hidden).remove(&id);
                        visibility.hidden.remove(&id);
                        editor.status = "已恢复网格在编辑器中的显示".into();
                    } else {
                        state.hidden.insert(id.clone());
                        std::sync::Arc::make_mut(&mut editor.terrain_hidden).insert(id.clone());
                        visibility.hidden.insert(id);
                        editor.status = "已隐藏编辑器网格；场景参与状态和碰撞保持有效".into();
                    }
                }
                None
            }
            Action::Lock => {
                if let Some(id) = state.selected.clone() {
                    if state.locked.remove(&id) {
                        editor.terrain_locked.remove(&id);
                        editor.status = "已解锁网格".into();
                    } else {
                        state.locked.insert(id.clone());
                        editor.terrain_locked.insert(id);
                        editor.status = "网格已锁定，可以选择查看".into();
                    }
                }
                None
            }
            Action::Glb | Action::ImportGlb | Action::Clipmap | Action::Voxel | Action::Cuboid => {
                state.creation_source = match button.0 {
                    Action::Glb => CreationSource::Heightfield,
                    Action::ImportGlb => CreationSource::ImportedGlb,
                    Action::Clipmap => CreationSource::Clipmap,
                    Action::Voxel => CreationSource::Voxel,
                    _ => CreationSource::Cuboid,
                };
                None
            }
            Action::PlacementViewport | Action::PlacementCoordinates => {
                state.placement_mode = if button.0 == Action::PlacementViewport { CreationPlacementMode::Viewport } else { CreationPlacementMode::Coordinates };
                None
            }
            Action::StartCreation => { state.start_requested = true; None }
            Action::CancelPlacement => { state.cancel_requested = true; None }
            Action::CuboidCollision => { state.cuboid_collision = !state.cuboid_collision; None }
            Action::Remove => state.selected.clone().map(Mutation::Remove),
            Action::DisplayLods | Action::CollisionLods => {
                let display = button.0 == Action::DisplayLods;
                let section = TerrainObjectsEditorState::lod_section(display);
                let open = !state.section_open(section);
                state.set_section_open(section, open); // 箭头只控制本组完整表格，不重置另一组选层或迁移输入树。
                if open { state.display_lods = display; }
                None
            }
            Action::SelectLod(local, display) => {
                if let Some(target) = lod_binding.and_then(|binding| binding.0.as_ref())
                    && session.draft.as_ref().is_some_and(|draft| draft.scene_id == target.scene_id)
                    && session.revision == target.revision && state.selected.as_ref() == Some(&target.terrain)
                    && display == target.display && target.index < super::presentation::lod_count(selected(&session, &state), display)
                    && target.index == state.lod(display).start.saturating_add(local)
                {
                    state.lod_mut(display).selected = target.index;
                    state.display_lods = display;
                }
                None
            }
            Action::AddLod(display) => lod_target.map(|target| Mutation::AddLod(target.terrain.clone(), display)),
            Action::RemoveLod(display) => lod_target.map(|target| Mutation::RemoveLod(target.terrain.clone(), display, target.index)),
            Action::Rename => None, // 名称由独立绑定器核对场景、对象和输入法状态后提交。
        };
        if let Some(mutation) = mutation {
            start(&mut session, &mut editor, &mut state, mutation);
            break;
        }
    }
}

pub(crate) fn poll(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<TerrainObjectsEditorState>,
    mut candidates: ResMut<crate::viewport::LayerCandidateState>,
    mut preview: ResMut<feathrise_scene::TerrainObjectTransformPreview>,
    mut sidebar: Option<ResMut<crate::ui::sidebars::SceneEditorSidebarState>>,
) {
    if state.stitch_release.as_mut().is_some_and(|task|check_ready(task).is_some()){state.stitch_release=None;}
    if state.stitch_preview.as_ref().is_some_and(|held|state.stitch_cancel || !editor.open || editor.save_requested || editor.close_requested || session.saving
        || editor.undo_requested || editor.redo_requested || editor.scene_transition_in_progress || editor.new_scene_dialog_open
        || session.revision!=held.pending.revision || session.draft.as_ref().is_none_or(|d|d.scene_id!=held.pending.scene_id)
        || state.selected!=held.pending.target) {
        super::stitches::cancel(&mut state,&mut preview);editor.undo_requested=false;editor.redo_requested=false;
        editor.status="已取消拼接预览，作者数据未修改".into();return;
    }
    let (pending,result)=if state.stitch_apply {
        state.stitch_apply=false;
        let Some(mut held)=state.stitch_preview.take() else{return;};held.pending.preview_stitch=false;
        (held.pending,Ok(held.prepared))
    }else{
        let Some(result)=state.pending.as_mut().and_then(|pending|check_ready(&mut pending.task)) else{return;};
        (state.pending.take().unwrap(),result)
    };
    if session.revision != pending.revision
        || session
            .draft
            .as_ref()
            .is_none_or(|draft| draft.scene_id != pending.scene_id)
        || session.saving
        || pending
            .target
            .as_ref()
            .is_some_and(|id| state.locked.contains(id))
    {
        preview.pose = None;
        editor.status = "网格准备期间场景已换代，过期结果未采用".into();
        if pending.stitching {super::stitches::release(&mut state,(pending,result));}
        return;
    }
    match result {
        Ok(prepared) => {
            let peer_blocked=pending.stitching && prepared.selected_index.and_then(|i|prepared.objects.get(i))
                .is_some_and(|o|o.clipmap_stitches.iter().any(|s|state.locked.contains(&s.peer)||state.hidden.contains(&s.peer)));
            let unlink_blocked=pending.stitching && !pending.preview_stitch && pending.before.objects.as_deref()
                .and_then(|objects|objects.iter().find(|o|Some(&o.id)==pending.target.as_ref()))
                .is_some_and(|o|o.clipmap_stitches.iter().any(|s|state.locked.contains(&s.peer)));
            if peer_blocked || unlink_blocked {
                state.stitch_preview=Some(super::types::StitchPreview{pending,prepared});super::stitches::cancel(&mut state,&mut preview);
                editor.status="参考网格已隐藏或锁定，拼接结果未采用".into();return;
            }
            if !prepared.changed {
                preview.pose = None;
                editor.status = "网格参数未发生变化".into();
                if pending.stitching {super::stitches::release(&mut state,(pending,prepared));}
                return;
            }
            if pending.preview_stitch {
                if !editor.open||editor.save_requested||editor.close_requested||editor.undo_requested||editor.redo_requested {
                    state.stitch_preview=Some(super::types::StitchPreview{pending,prepared});
                    super::stitches::cancel(&mut state,&mut preview);editor.undo_requested=false;editor.redo_requested=false;return;
                }
                let Some(index)=prepared.selected_index else{return;};
                let object=&prepared.objects[index];
                preview.pose=Some((pending.scene_id.clone(),object.id.clone(),object.transform));
                state.stitch_preview=Some(super::types::StitchPreview{pending,prepared});
                editor.status="拼接预览已就绪；检查边界后确认拼接或取消".into();return;
            }
            // 首次迁移先固定旧主对象的作者身份，旧清单仍在时才能保留未保存几何和画布。
            crate::authoring::terrain_documents::ensure_primary_canvas(
                session.draft.as_mut().unwrap(),
            );
            let after = SceneTerrainCollection {
                legacy: SceneTerrainDescriptor::empty(),
                objects: Some(prepared.objects),
            };
            after.apply(&mut session.draft.as_mut().unwrap().manifest);
            if pending.affects_terrain {
                session.draft.as_mut().unwrap().terrain_ground = None;
            }
            let previous = pending.stroke.and_then(|stroke| {
                state
                    .brush_history
                    .filter(|(ticket, index)| *ticket == stroke && *index + 1 == session.undo.len())
            });
            if pending.stitching {
                session.push_bounded_water_history(EditorCommand::ClipmapStitchesChanged {before:pending.before,after,heap_bytes:prepared.stitch_bytes});
            } else if let Some((_, index)) = previous
                && let Some(EditorCommand::TerrainObjectsChanged { after: current, .. }) =
                    session.undo.get_mut(index)
            {
                *current = after;
            } else {
                session.undo.push(EditorCommand::TerrainObjectsChanged {
                    before: pending.before,
                    after,
                });
                state.brush_history = pending
                    .stroke
                    .map(|stroke| (stroke, session.undo.len() - 1));
            }
            session.redo.clear();
            session.mark_changed(&mut editor);
            if pending.affects_terrain {
                session.terrain_revision = session.terrain_revision.wrapping_add(1);
            }
            state.selected = prepared.selected;
            state.selected_index = prepared.selected_index;
            if pending.target.is_none() {
                // 添加完成即回到新网格基本属性，避免创建规格与已有网格表单同时挤满左栏。
                state.creation_open = false;
                state.set_section_open(MeshPropertySection::Basic, true);
                state.reset_lod_selection();
                if let Some(sidebar) = sidebar.as_deref_mut() { sidebar.reveal_scene_properties(); }
            }
            if let (Some(id), Some(index)) = (&state.selected, state.selected_index) {
                if pending.stroke.is_some() {
                    candidates.rebind_terrain_canvas(id, index); // 刷印采用只是源换代，不是重复选择或抢回属性栏。
                } else {
                    candidates.confirm_terrain(id, index);
                }
            }
            editor.status = if let Some(navigation) = pending.lod_navigation {
                let (display, removed) = match navigation {
                    LodNavigation::Added { display } => (display, None),
                    LodNavigation::Removed { display, index } => (display, Some(index)),
                };
                state.display_lods = display;
                let count = super::presentation::lod_count(selected(&session, &state), display);
                state.lod_mut(display).selected = removed.unwrap_or_else(|| count.saturating_sub(1)).min(count.saturating_sub(1));
                let usage = if display { "显示" } else { "碰撞" };
                if let Some(index) = removed {
                    format!("已删除{usage}LOD{index}；保存场景后持久化")
                } else {
                    format!("已添加{usage}LOD{}；保存场景后持久化", state.lod(display).selected)
                }
            } else { "网格已采用；保存场景后持久化".into() };
        }
        Err(error) => {
            preview.pose = None;
            editor.status = format!("网格操作失败：{error}");
            if pending.stitching {super::stitches::release(&mut state,pending);}
        }
    }
}

pub(super) use super::presentation::sync;
