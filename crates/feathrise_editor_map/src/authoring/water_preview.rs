//! 混合水网作者数据的 single-flight 正式 GPU 水面预览。
//!
//! 【功能设计】每个任务冻结当前地图的全部 R8 静水层、三维河流/急流、瀑布和同一份
//! 未保存GLB地形派生高度，身份同时绑定水域与地形修订。任一已完成水体失败时保留上一完整批次；
//! 全部成功后才一次替换预览并隐藏当前GLB来源的正式热加载水面，避免跨代混搭或误隐藏兄弟水。

use bevy::prelude::*;
use bevy::tasks::futures::check_ready;
use feathrise_map_content::WaterPresetRegistry; // 轮询采用门直接复验当前正式目录修订。
use feathrise_scene::{
    ClipmapSplineWaterSurface, ClipmapWaterSurface, DynamicClipmapWaterDraft, DynamicWaterMaterial,
    DynamicWaterStatus, PaintedWaterPurpose, PaintedWaterSurface, TerrainObjectResource,
    WaterGpuUploadQueue, WaterGpuUploadReceipts,
    spawn_staged_water_surface, spawn_staged_water_warmup, stage_water_surface_chunk,
};
use feathrise_scene_core::state::ScenePhase;
use feathrise_scene_core::residency::MapInstanceOwner;

mod build; // 冻结联合修订并在后台生成完整水网候选。
mod cleanup; // 候选、已采用预览资产与正式水面可见性恢复集中维护。
mod suppression; // 只有实际GLB替代来源可以隐藏自己的完整运行实例。

#[cfg(test)]
mod tests; // 不完整样条的合法编辑中间态回归测试独立维护。
#[cfg(test)]
mod suppression_tests; // 独立根水、私有来源、空预览和热更的显隐边界。

pub(crate) use build::start_water_preview_rebuild; // 保持系统注册使用的既有模块入口。
use cleanup::{discard_pending_preview, remove_preview_assets, restore_hidden_runtime};
use suppression::{current_source, hide_replaced_surface, runtime_target};

use super::resources::{EditorAuthoringSession, PendingWaterPreviewBatch, WaterPreviewState};
use crate::components::EditorWaterPreview;
use crate::resources::MapEditorState;

/// 只有地图和正式目录修订仍完全匹配时，才原子采用全部水层完成值。
#[allow(clippy::too_many_arguments)]
pub(crate) fn poll_water_preview_rebuild(
    mut commands: Commands,
    time: Res<Time>,
    scene: Res<State<ScenePhase>>,
    session: Res<EditorAuthoringSession>,
    water_presets: Option<Res<WaterPresetRegistry>>, // 正式目录可能在旧CPU任务运行期间热更。
    mut editor: ResMut<MapEditorState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DynamicWaterMaterial>>,
    mut uploads: ResMut<WaterGpuUploadQueue>,
    receipts: Res<WaterGpuUploadReceipts>,
    runtime_status: TerrainObjectResource<DynamicWaterStatus>,
    runtime_surfaces: Query<
        (Entity, &PaintedWaterSurface, &Visibility, Option<&MapInstanceOwner>),
        (Without<EditorWaterPreview>, Without<ClipmapWaterSurface>, Without<ClipmapSplineWaterSurface>),
    >,
    mut preview: ResMut<WaterPreviewState>,
) {
    let current_source = current_source(&session)
        .filter(|source| editor.selected_map_id.as_ref() == Some(&source.map_id));
    if preview.replacement_source.as_ref().is_some_and(|source| Some(source) != current_source.as_ref()) {
        // 切到根水、Clipmap或另一GLB时立即归还旧来源；Ready映射不能继续携带旧GLB替代物。
        restore_hidden_runtime(&mut commands, &mut preview);
        remove_preview_assets(&mut commands, &mut meshes, &mut materials, &mut preview);
        preview.applied_map_id = None;
    }
    if let Some(source) = preview.replacement_source.clone() {
        let target = runtime_target(&source, &runtime_status);
        if target != preview.replacement_target {
            // 正式整批实例换代时只重扫既有水面查询一次，Added早于批次采用也能正确接管。
            restore_hidden_runtime(&mut commands, &mut preview);
            preview.replacement_target = target;
            if let Some(target) = target {
                for surface in &runtime_surfaces {
                    hide_replaced_surface(&mut commands, &mut preview, &source, target, surface);
                }
            }
        }
    }
    let current_preset_revision = water_presets.as_deref().map_or(0, |value| value.revision); // 独立样品草稿不再触发整图水网重建。
    let pending_is_stale = preview.pending.as_ref().is_some_and(|pending| {
        current_source.as_ref() != Some(&pending.source)
            || editor.selected_map_id.as_ref() != Some(&pending.source.map_id)
            || preview.desired_map_id.as_ref() != Some(&pending.source.map_id)
            || preview.desired_water_revision != pending.water_revision
            || preview.desired_terrain_revision != pending.terrain_revision
            || preview.desired_preset_revision != pending.preset_revision
            || current_preset_revision != pending.preset_revision // 同帧输入或热更立即淘汰旧GPU候选。
            || session.water_revision != pending.water_revision
            || session.terrain_revision != pending.terrain_revision
            || !session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.map_id == pending.source.map_id)
    });
    if pending_is_stale {
        discard_pending_preview(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut uploads,
            &mut preview,
        );
    }

    if preview.pending.is_none() {
        let Some(task) = preview.task.as_mut() else {
            return;
        };
        let Some((source, water_revision, terrain_revision, preset_revision, result)) =
            check_ready(task)
        else {
            return;
        };
        preview.task = None;
        let map_id = source.map_id.clone();
        let current_draft_matches = session
            .draft
            .as_ref()
            .is_some_and(|draft| draft.map_id == map_id);
        if !current_draft_matches
            || current_source.as_ref() != Some(&source)
            || editor.selected_map_id.as_ref() != Some(&map_id)
            || preview.desired_map_id.as_ref() != Some(&map_id)
            || preview.desired_water_revision != water_revision
            || preview.desired_terrain_revision != terrain_revision
            || preview.desired_preset_revision != preset_revision
            || current_preset_revision != preset_revision // 迟到CPU完成值必须匹配此帧最新外观。
            || session.water_revision != water_revision
            || session.terrain_revision != terrain_revision
        {
            feathrise_scene::retire_water_cpu(&mut commands, result);
            return;
        }
        let builds = match result {
            Ok(builds) => builds,
            Err(error) => {
                preview.failed_map_id = Some(map_id);
                preview.failed_water_revision = water_revision;
                preview.failed_terrain_revision = terrain_revision;
                preview.failed_preset_revision = preset_revision;
                editor.status = format!("水网真实预览构建失败：{error}");
                error!("{}", editor.status);
                return;
            }
        };
        preview.pending = Some(PendingWaterPreviewBatch {
            source,
            water_revision,
            terrain_revision,
            preset_revision,
            remaining: builds.into(),
            staged: Vec::new(),
            warmup_entities: Vec::new(),
            triangle_count: 0,
            ticket: None,
        });
    }

    let mut skipped = 0;
    while skipped < 32
        && preview
            .pending
            .as_ref()
            .and_then(|pending| pending.remaining.front())
            .is_some_and(|build| build.mesh.is_empty())
    {
        preview
            .pending
            .as_mut()
            .expect("上方已确认水预览候选存在")
            .remaining
            .pop_front();
        skipped += 1;
    }
    if preview
        .pending
        .as_ref()
        .and_then(|pending| pending.remaining.front())
        .is_some_and(|build| build.mesh.is_empty())
    {
        return;
    } // 空层也只分批跳过，32是每帧节奏。
    let next_bytes = preview
        .pending
        .as_ref()
        .and_then(|pending| pending.remaining.front())
        .map(|build| build.mesh.estimated_gpu_bytes());
    if let Some(next_bytes) = next_bytes {
        match uploads.try_reserve_registration(next_bytes) {
            Ok(false) => return,
            Err(error) => {
                let (map_id, water_revision, terrain_revision, preset_revision) = preview
                    .pending
                    .as_ref()
                    .map(|pending| {
                        (
                            pending.source.map_id.clone(),
                            pending.water_revision,
                            pending.terrain_revision,
                            pending.preset_revision,
                        )
                    })
                    .expect("存在待登记Mesh时必有水预览候选");
                discard_pending_preview(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut uploads,
                    &mut preview,
                );
                preview.failed_map_id = Some(map_id);
                preview.failed_water_revision = water_revision;
                preview.failed_terrain_revision = terrain_revision;
                preview.failed_preset_revision = preset_revision;
                editor.status = format!("水网真实预览GPU分帧预算失败：{error}");
                error!("{}", editor.status);
                return;
            }
            Ok(true) => {
                let build = preview
                    .pending
                    .as_mut()
                    .expect("存在待登记Mesh时必有水预览候选")
                    .remaining
                    .pop_front()
                    .expect("上方已读取队首完成值");
                let pending = preview.pending.as_mut().expect("登记期间候选保持存在");
                pending.triangle_count += stage_water_surface_chunk(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut pending.staged,
                    build,
                    time.elapsed_secs(),
                );
            }
        }
    }

    let pending = preview.pending.as_mut().expect("水预览候选已建立");
    if !pending.remaining.is_empty() {
        editor.status = format!(
            "水网真实预览正在按每帧一张Mesh登记GPU资产：已登记{}张，剩余{}张",
            pending.staged.len(),
            pending.remaining.len()
        );
        return;
    }
    if !pending.staged.is_empty() && pending.ticket.is_none() {
        for staged in &pending.staged {
            if let Err(error) = staged.rebase_material(&mut materials, time.elapsed_secs()) {
                let (map_id, water_revision, terrain_revision, preset_revision) = (
                    pending.source.map_id.clone(),
                    pending.water_revision,
                    pending.terrain_revision,
                    pending.preset_revision,
                );
                discard_pending_preview(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut uploads,
                    &mut preview,
                );
                preview.failed_map_id = Some(map_id);
                preview.failed_water_revision = water_revision;
                preview.failed_terrain_revision = terrain_revision;
                preview.failed_preset_revision = preset_revision;
                editor.status = format!("水网真实预览候选材质重置失败：{error}");
                error!("{}", editor.status);
                return;
            }
        }
        pending.warmup_entities = pending
            .staged
            .iter()
            .map(|staged| {
                let entity = spawn_staged_water_warmup(&mut commands, staged);
                commands.entity(entity).insert(DespawnOnExit(*scene.get()));
                entity
            })
            .collect();
        pending.ticket = Some(uploads.submit(&pending.staged, &pending.warmup_entities));
    }
    if pending
        .ticket
        .is_some_and(|ticket| receipts.has_failed(ticket))
    {
        let (map_id, water_revision, terrain_revision, preset_revision) = (
            pending.source.map_id.clone(),
            pending.water_revision,
            pending.terrain_revision,
            pending.preset_revision,
        );
        discard_pending_preview(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut uploads,
            &mut preview,
        );
        preview.failed_map_id = Some(map_id);
        preview.failed_water_revision = water_revision;
        preview.failed_terrain_revision = terrain_revision;
        preview.failed_preset_revision = preset_revision;
        editor.status = "水网真实预览透明PBR管线编译失败；已保留上一完整预览".to_owned();
        error!("{}", editor.status);
        return;
    }
    if pending
        .ticket
        .is_some_and(|ticket| !receipts.is_ready(ticket))
    {
        editor.status = format!(
            "水网真实预览正在等待{}张候选水面的连续GPU资产准备回执",
            pending.staged.len()
        );
        return;
    }

    let mut pending = preview.pending.take().expect("水预览候选刚通过完整采用门");
    if let Some(ticket) = pending.ticket.take() {
        uploads.retire(ticket);
    }
    for entity in pending.warmup_entities.drain(..) {
        commands.entity(entity).try_despawn();
    }
    let source = pending.source;
    let map_id = source.map_id.clone();
    let water_revision = pending.water_revision;
    let terrain_revision = pending.terrain_revision;
    let preset_revision = pending.preset_revision;
    let triangle_count = pending.triangle_count;
    let mut new_spawns = Vec::with_capacity(pending.staged.len());
    for staged in pending.staged {
        let spawn = spawn_staged_water_surface(
            &mut commands,
            map_id.clone(),
            PaintedWaterPurpose::EditorPreview,
            staged,
        );
        commands
            .entity(spawn.entity)
            .insert((EditorWaterPreview, DespawnOnExit(*scene.get())));
        new_spawns.push(spawn);
    }

    let target = runtime_target(&source, &runtime_status);
    if preview.replacement_source.as_ref() != Some(&source) || preview.replacement_target != target {
        restore_hidden_runtime(&mut commands, &mut preview);
    }
    preview
        .hidden_runtime_entities
        .retain(|(entity, _)| runtime_surfaces.contains(*entity));
    if let Some(target) = target {
        for surface in &runtime_surfaces {
            hide_replaced_surface(&mut commands, &mut preview, &source, target, surface);
        }
    }
    remove_preview_assets(&mut commands, &mut meshes, &mut materials, &mut preview);
    preview.spawns = new_spawns;
    preview.replacement_source = Some(source);
    preview.replacement_target = target;
    preview.applied_map_id = Some(map_id);
    preview.applied_water_revision = water_revision;
    preview.applied_terrain_revision = terrain_revision;
    preview.applied_preset_revision = preset_revision;
    preview.failed_map_id = None;
    preview.failed_water_revision = 0;
    preview.failed_terrain_revision = 0;
    preview.failed_preset_revision = 0; // 成功采用后清除正式目录外观失败身份。
    editor.status = if triangle_count == 0 {
        "当前地图没有有效水面；已实时隐藏旧正式水面".to_owned()
    } else {
        format!("水网真实预览已刷新：{triangle_count}个三角形，静水、河流/急流与瀑布同修订采用")
    };
}

/// 保存热更新生成新正式水面时继续隐藏，直到预览生命周期结束。
pub(crate) fn suppress_new_runtime_water_surfaces(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    editor: Res<MapEditorState>,
    runtime_status: TerrainObjectResource<DynamicWaterStatus>,
    surfaces: Query<
        (Entity, &PaintedWaterSurface, &Visibility, Option<&MapInstanceOwner>),
        (
            Added<PaintedWaterSurface>,
            Without<EditorWaterPreview>,
            Without<ClipmapWaterSurface>,
            Without<ClipmapSplineWaterSurface>,
        ),
    >,
    existing: Query<(), With<PaintedWaterSurface>>,
    mut preview: ResMut<WaterPreviewState>,
) {
    preview
        .hidden_runtime_entities
        .retain(|(entity, _)| existing.contains(*entity));
    let Some(source) = preview.replacement_source.clone().filter(|source|
        editor.selected_map_id.as_ref() == Some(&source.map_id) && Some(source) == current_source(&session).as_ref()) else {
        return;
    };
    let Some(target) = runtime_target(&source, &runtime_status) else { return; };
    for surface in &surfaces {
        hide_replaced_surface(&mut commands, &mut preview, &source, target, surface);
    }
}

/// 离开地图时恢复正式水面并回收整批预览 Mesh 与材质。
pub(crate) fn clear_water_previews(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DynamicWaterMaterial>>,
    mut uploads: ResMut<WaterGpuUploadQueue>,
    mut preview: ResMut<WaterPreviewState>,
    mut clipmap_draft: ResMut<DynamicClipmapWaterDraft>,
) {
    discard_pending_preview(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut uploads,
        &mut preview,
    );
    remove_preview_assets(&mut commands, &mut meshes, &mut materials, &mut preview);
    restore_hidden_runtime(&mut commands, &mut preview);
    preview.retarget(None, 0, 0); // 离图只清身份与可见资产，在途CPU任务仍占槽并由轮询按身份丢弃。
    clipmap_draft.clear(); // 离开玩法地图立即撤销作者快照，禁止下一地图短暂读取旧R8。
    clipmap_draft.clear_root(); // 根水独立于地形画布，离图仍须撤销自己的作者快照。
}
