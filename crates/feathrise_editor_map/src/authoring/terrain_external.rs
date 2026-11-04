//! Blender 保存 `terrain.glb` 时的 F12 自动采用、冲突冻结与一次性覆盖授权。

use std::sync::Arc; // 外部完整拓扑和派生高度以指针换代采用。

use bevy::prelude::*; // 资源、任务与用户状态。
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool, Task, futures::check_ready}; // 文件读取与CPU派生分别离开主线程。
use feathrise_map_content::{MapId, MapTerrainKind, TerrainId}; // 只处理当前对象的动态Heightfield草稿。
use feathrise_map_terrain::{
    TerrainEditingIndex, TerrainMeshInput, TerrainSampleSpec, build_terrain_editing_index,
    derive_terrain, parse_dynamic_terrain_glb,
}; // 与初载、运行时共用同一GLB契约和验证算法。
use feathrise_scene::{DynamicTerrainHeightPhase, DynamicTerrainHeightStatus}; // 当前GLB输入代次唤醒磁盘精确摘要检查。
use sha2::{Digest, Sha256}; // 冲突和覆盖授权都绑定实际文件字节。

use super::files::{safe_asset_path, workspace_asset_root}; // 外部监控仍不得越过assets根。
use super::resources::{
    EditorAuthoringSession, EditorCommand, TerrainLiveApplyState, TerrainPreviewState,
}; // 采用新拓扑时使旧预览、实时碰撞和索引历史一起失效。
use crate::resources::MapEditorState; // 弹框、保存请求和dirty状态。

struct ExternalTerrainBuild {
    digest: [u8; 32],                        // 本完成值对应的精确GLB字节。
    mesh: Arc<TerrainMeshInput>,             // 变换已烘焙的世界顶点与原索引。
    editing_index: Arc<TerrainEditingIndex>, // 与该拓扑绑定的局部查询和邻接。
    heights: Arc<Vec<u16>>,                  // 同一验证任务生成的内存查询高度。
}

type ExternalTerrainTaskResult = (
    u64, // 任务票据；Blender连续保存只采用最新一次。
    MapId,
    u64, // 运行时GLB期望代次。
    Result<ExternalTerrainBuild, String>,
);

/// 当前待作者选择的外部GLB版本。
#[derive(Clone, Debug)]
pub(crate) struct TerrainExternalConflict {
    pub(crate) map_id: MapId,    // 防止同名terrain.glb跨地图选择。
    pub(crate) digest: [u8; 32], // “覆盖”只授权这一份外部内容。
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerrainConflictResolution {
    AdoptBlender,     // 放弃未保存F12顶点Y，采用外部完整拓扑。
    OverwriteWithF12, // 保留当前F12顶点并对检测版本授予一次保存覆盖。
}

/// 外部GLB监控只保留一个IO/CPU任务和一个最新冲突候选。
#[derive(Resource, Default)]
pub struct TerrainExternalChangeState {
    pub(crate) conflict: Option<TerrainExternalConflict>, // Some时显示全屏模态选择。
    pub(crate) resolution: Option<TerrainConflictResolution>, // 按钮只登记选择，Tasks阶段统一应用。
    desired_map_id: Option<MapId>,                        // 当前等待核对的F12地图。
    desired_path: Option<String>, // 同MapId内切换对象也必须取消旧源、旧模态和迟到任务。
    desired_generation: u64,      // 最新运行时GLB输入代次。
    observed_generation: u64,     // 已读盘并处理的代次。
    ticket: u64,                  // 保存冲突或连续GLB热更都会推进。
    force_reload: bool,           // 保存门发现冲突时即使没有新AssetEvent也必须读取外部版本。
    task: Option<Task<ExternalTerrainTaskResult>>, // 全局最多一个外部GLB加载/派生任务。
    candidate: Option<ExternalTerrainBuild>, // 脏草稿冲突期间保留完整外部候选供“采用Blender”。
    adopt_after_reload: Option<[u8; 32]>, // 保存门只有摘要时，把“采用”严格绑定到当时检测的外部版本。
}

impl TerrainExternalChangeState {
    /// 保存SHA门发现外部变化后立即建立模态身份，并请求后台验证该版本。
    pub(crate) fn register_save_conflict(&mut self, map_id: MapId, digest: [u8; 32]) {
        self.conflict = Some(TerrainExternalConflict { map_id, digest });
        self.candidate = None; // 保存门只读了摘要，采用前仍需完整解析和三维地形验证。
        self.resolution = None; // 上一冲突尚未处理的按钮选择不能跨版本复用。
        self.adopt_after_reload = None; // 同理撤销只属于旧摘要的延迟采用授权。
        self.ticket = self.ticket.wrapping_add(1);
        self.force_reload = true;
    }

    pub fn modal_open(&self) -> bool {
        self.conflict.is_some()
    }
}

fn is_dynamic_glb_draft(session: &EditorAuthoringSession) -> bool {
    session.draft.as_ref().is_some_and(|draft| {
        draft.manifest.is_data_driven()
            && draft.terrain_manifest().terrain.kind == MapTerrainKind::Heightfield
            && draft.terrain_mesh.is_some()
    })
}

fn start_external_read(
    draft: &super::resources::EditorMapDraft,
    generation: u64,
    state: &mut TerrainExternalChangeState,
) -> Result<(), String> {
    let root = workspace_asset_root();
    let path = draft.terrain_manifest().terrain.glb_path.clone();
    safe_asset_path(&root, &path)?;
    let map_id = draft.map_id.clone();
    let spec = TerrainSampleSpec {
        resolution: draft.terrain_manifest().terrain.resolution,
        world_min: draft.terrain_manifest().world_min,
        world_max: draft.terrain_manifest().world_max,
        base_height: draft.terrain_manifest().terrain.base_height,
        height_scale: draft.terrain_manifest().terrain.height_scale,
    };
    let ticket = state.ticket;
    state.task = Some(IoTaskPool::get().spawn(async move {
        let result: Result<ExternalTerrainBuild, String> = async {
            let bytes = crate::ui::terrain_objects::read_glb(&path)?;
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            AsyncComputeTaskPool::get()
                .spawn(async move {
                    let mesh = parse_dynamic_terrain_glb(&bytes)?;
                    let editing_index = build_terrain_editing_index(&mesh)?;
                    let derived = derive_terrain(mesh, spec)?;
                    Ok::<_, String>(ExternalTerrainBuild {
                        digest,
                        mesh: Arc::new(TerrainMeshInput {
                            positions: derived.positions,
                            indices: derived.indices,
                        }),
                        editing_index: Arc::new(editing_index),
                        heights: Arc::new(derived.heights),
                    })
                })
                .await
        }
        .await;
        (ticket, map_id, generation, result)
    }));
    state.force_reload = false; // 任务期间的新保存事件会重新置位并推进ticket。
    Ok(())
}

fn command_uses_terrain_topology(command: &EditorCommand, terrain: &TerrainId) -> bool {
    if let EditorCommand::TerrainCanvas { id, command } = command {
        // A的GLB拓扑换代只清A的顶点下标历史；B的相同下标仍属于B的有效源。
        return id == terrain && command_uses_terrain_topology(command, terrain);
    }
    matches!(command, EditorCommand::TerrainChanged(_))
        || matches!(command, EditorCommand::BuildingTerrainChanged(change) if !change.heights.is_empty())
        || matches!(command, EditorCommand::WaterStrokeSnapshot(change) if change.terrain.is_some()) // 外部新拓扑不能保留旧完整河床快照。
        || matches!(
            command,
            EditorCommand::WaterStroke { height_changes, .. } if !height_changes.is_empty()
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExternalTerrainDisposition {
    Unchanged, // 重复资产事件或F12刚保存的同一字节版本。
    Adopt,     // 干净草稿自动接收Blender新拓扑。
    Conflict,  // 脏草稿必须等待作者选择。
}

fn external_terrain_disposition(
    baseline: Option<[u8; 32]>,
    incoming: [u8; 32],
    terrain_dirty: bool,
) -> ExternalTerrainDisposition {
    if baseline == Some(incoming) {
        ExternalTerrainDisposition::Unchanged
    } else if terrain_dirty {
        ExternalTerrainDisposition::Conflict
    } else {
        ExternalTerrainDisposition::Adopt
    }
}

fn adopt_external_build(
    build: ExternalTerrainBuild,
    map_id: &MapId,
    session: &mut EditorAuthoringSession,
    editor: &mut MapEditorState,
    preview: &mut TerrainPreviewState,
    live: &mut TerrainLiveApplyState,
) -> Result<(), String> {
    let draft = session
        .draft
        .as_mut()
        .filter(|draft| &draft.map_id == map_id)
        .ok_or_else(|| "外部地形候选所属地图已经失效".to_owned())?;
    draft.terrain_mesh = Some(build.mesh);
    draft.terrain_editing_index = Some(build.editing_index);
    draft.heights = Some(build.heights);
    draft.terrain_source_sha256 = Some(build.digest);
    draft.terrain_overwrite_authorized_sha256 = None;
    draft.terrain_dirty = false; // “采用Blender”明确放弃本草稿未保存的顶点Y。
    let terrain = draft
        .terrain_documents
        .active
        .as_ref()
        .map_or_else(TerrainId::legacy, |active| active.definition.id.clone());
    session
        .undo
        .retain(|command| !command_uses_terrain_topology(command, &terrain));
    session
        .redo
        .retain(|command| !command_uses_terrain_topology(command, &terrain));
    // 任意删除会改变历史下标；64条/帧重新核对已有包装，防止跳过随后追加的裸画布命令。
    session
        .draft
        .as_mut()
        .unwrap()
        .terrain_documents
        .scoped_through = 0;
    session.revision = session.revision.wrapping_add(1); // 使保存、植被和水域的旧冻结任务全部过期。
    session.terrain_revision = session.terrain_revision.wrapping_add(1); // 触发新拓扑的预览和联合水面派生。
    preview.applied_map_id = None; // 下一稳定帧必须从新GLB拓扑重建Mesh、Image与Trimesh。
    live.discard_candidate(); // 旧F12拓扑不得在运行时新GLB完成后再次覆盖回来。
    let draft = session.draft.as_ref().expect("上方已确认当前草稿存在");
    editor.dirty = session.sparse_terrain_dirty
        || draft.domain_dirty()
        || draft.stored_terrain_changes()
        || draft.surface_dirty
        || draft.puddles_dirty
        || !draft.vegetation_dirty_layers.is_empty()
        || !draft.vegetation_author_dirty_layers.is_empty()
        || !draft.water_dirty_layers.is_empty()
        || draft.environment_dirty
        || draft.boundaries_dirty
        || draft.vegetation_registration_source_hash.is_some() // 首次植被登记同样必须保留未保存责任。
                || draft.water_catalog_registration_required
        || draft.boundary_catalog_registration_required
        || !session.undo.is_empty(); // 只清除地形脏状态，其他作者修改继续等待保存。
    editor.status = format!("已采用Blender保存的地图{map_id}新地形；旧拓扑相关撤销记录已清除");
    Ok(())
}

/// 观察运行时GLB代次、处理冲突按钮并非阻塞采用最新外部完整地形。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_external_terrain_changes(
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    status: feathrise_scene::TerrainObjectResource<DynamicTerrainHeightStatus>,
    mut state: ResMut<TerrainExternalChangeState>,
    mut preview: ResMut<TerrainPreviewState>,
    mut live: ResMut<TerrainLiveApplyState>,
) {
    if !editor.open || !is_dynamic_glb_draft(&session) {
        state.desired_map_id = None;
        state.conflict = None;
        state.candidate = None;
        state.resolution = None;
        state.adopt_after_reload = None;
        return; // 在途任务自然完成后由票据/地图门丢弃，不协作取消重任务。
    }
    let map_id = session
        .draft
        .as_ref()
        .expect("动态草稿门已确认")
        .map_id
        .clone();
    let path = session
        .draft
        .as_ref()
        .unwrap()
        .terrain_manifest()
        .terrain
        .glb_path
        .clone();
    if state.desired_path.as_ref() != Some(&path) {
        state.desired_path = Some(path);
        state.desired_map_id = Some(map_id.clone());
        state.ticket = state.ticket.wrapping_add(1);
        state.conflict = None;
        state.resolution = None;
        state.candidate = None;
        state.adopt_after_reload = None;
        state.force_reload = true;
    }
    if status.map_id.as_ref() == Some(&map_id)
        && status.desired_generation > 0
        && (state.desired_map_id.as_ref() != Some(&map_id)
            || state.desired_generation != status.desired_generation)
    {
        state.desired_map_id = Some(map_id.clone());
        state.desired_generation = status.desired_generation;
        state.ticket = state.ticket.wrapping_add(1);
        state.force_reload = true; // Ready与Failed都读取实际文件，以便显示精确Blender错误。
    }

    if let Some(resolution) = state.resolution.take() {
        let Some(conflict) = state
            .conflict
            .clone()
            .filter(|conflict| conflict.map_id == map_id)
        else {
            editor.status = "地形冲突版本已经失效，请等待最新GLB检查".to_owned();
            return;
        };
        match resolution {
            TerrainConflictResolution::OverwriteWithF12 => {
                if let Some(draft) = session.draft.as_mut() {
                    draft.terrain_overwrite_authorized_sha256 = Some(conflict.digest);
                }
                state.conflict = None;
                state.candidate = None;
                editor.save_requested = true; // 保存门会再次读盘；Blender再保存一次就重新形成冲突。
                editor.status =
                    "已选择使用草稿覆盖 Blender 版本，正在校验并保存".to_owned();
            }
            TerrainConflictResolution::AdoptBlender => {
                if let Some(build) = state
                    .candidate
                    .take()
                    .filter(|build| build.digest == conflict.digest)
                {
                    if let Err(error) = adopt_external_build(
                        build,
                        &map_id,
                        &mut session,
                        &mut editor,
                        &mut preview,
                        &mut live,
                    ) {
                        editor.status = format!("采用Blender地形失败：{error}");
                    } else {
                        state.conflict = None;
                    }
                } else {
                    state.ticket = state.ticket.wrapping_add(1);
                    state.force_reload = true; // 保存门只有摘要时先完整解析，不能直接采用未经验证的GLB。
                    state.adopt_after_reload = Some(conflict.digest); // 后台完成前若字节再次变化，必须重新冲突。
                    editor.status = "正在后台验证Blender地形，完成后自动采用".to_owned();
                }
            }
        }
    }

    if let Some(task) = state.task.as_mut()
        && let Some((ticket, task_map_id, generation, result)) = check_ready(task)
    {
        state.task = None;
        if ticket == state.ticket && task_map_id == map_id {
            state.observed_generation = generation;
            match result {
                Ok(build) => {
                    if let Some(expected_digest) = state.adopt_after_reload.take() {
                        if build.digest != expected_digest {
                            let digest = build.digest;
                            state.conflict = Some(TerrainExternalConflict {
                                map_id: map_id.clone(),
                                digest,
                            });
                            state.candidate = Some(build); // 新版本已完整验证，但必须取得一次新选择。
                            editor.status =
                                "验证期间Blender再次保存了terrain.glb；请针对最新版本重新选择"
                                    .to_owned();
                        } else if let Err(error) = adopt_external_build(
                            build,
                            &map_id,
                            &mut session,
                            &mut editor,
                            &mut preview,
                            &mut live,
                        ) {
                            editor.status = format!("采用Blender地形失败：{error}");
                        } else {
                            state.conflict = None;
                            state.candidate = None;
                        }
                        return; // 显式选择已兑现或因版本变化重新建立冲突，不进入自动分支。
                    }
                    let baseline = session
                        .draft
                        .as_ref()
                        .and_then(|draft| draft.terrain_source_sha256);
                    let terrain_dirty = session
                        .draft
                        .as_ref()
                        .is_some_and(|draft| draft.terrain_dirty);
                    match external_terrain_disposition(baseline, build.digest, terrain_dirty) {
                        ExternalTerrainDisposition::Unchanged => {
                            state.conflict = None; // 初次观察或重复资产事件没有真实字节变化。
                            state.candidate = None;
                        }
                        ExternalTerrainDisposition::Conflict => {
                            let digest = build.digest;
                            state.conflict = Some(TerrainExternalConflict {
                                map_id: map_id.clone(),
                                digest,
                            });
                            state.candidate = Some(build); // 选择采用时无需再次读取同一外部版本。
                            editor.status =
                                "Blender已保存新的terrain.glb；请选择采用 Blender 版本或使用草稿覆盖"
                                    .to_owned();
                        }
                        ExternalTerrainDisposition::Adopt => {
                            if let Err(error) = adopt_external_build(
                                build,
                                &map_id,
                                &mut session,
                                &mut editor,
                                &mut preview,
                                &mut live,
                            ) {
                                editor.status = format!("自动采用Blender地形失败：{error}");
                            } else {
                                state.conflict = None;
                                state.candidate = None;
                            }
                        }
                    }
                }
                Err(error) => {
                    state.adopt_after_reload = None; // 明确失败归还按钮，作者可修复GLB后重新选择。
                    state.candidate = None;
                    editor.status = format!("Blender地形热更新无效，已保留上一完整版本：{error}");
                    if status.phase == DynamicTerrainHeightPhase::Failed {
                        error!("动态地形外部版本验证失败：{}", status.detail);
                    }
                }
            }
        }
    }

    if state.task.is_none()
        && state.force_reload
        && let Some(draft) = session.draft.as_ref()
    {
        if let Err(error) = start_external_read(draft, state.desired_generation, &mut state) {
            state.force_reload = false;
            editor.status = format!("启动Blender地形核对失败：{error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 纯状态测试锁定冲突摘要与延迟选择的版本边界。

    #[test]
    fn a_new_save_conflict_revokes_every_choice_for_the_previous_digest() {
        let map_id = MapId::parse("MAP-terrain-conflict").expect("测试MapId必须有效");
        let mut state = TerrainExternalChangeState {
            resolution: Some(TerrainConflictResolution::OverwriteWithF12),
            adopt_after_reload: Some([1; 32]),
            ticket: 9,
            ..default()
        };

        state.register_save_conflict(map_id.clone(), [2; 32]);

        let conflict = state.conflict.as_ref().expect("新冲突必须可见");
        assert_eq!(conflict.map_id, map_id);
        assert_eq!(conflict.digest, [2; 32]);
        assert_eq!(state.resolution, None); // 旧按钮点击不能授权新版本。
        assert_eq!(state.adopt_after_reload, None); // 旧延迟采用同样必须撤销。
        assert_eq!(state.ticket, 10); // 在途旧读取结果会被票据门丢弃。
        assert!(state.force_reload); // 新摘要仍需完整解析和三维地形验证。
    }

    #[test]
    fn clean_draft_auto_adopts_but_dirty_draft_opens_a_conflict() {
        let baseline = Some([1; 32]);
        assert_eq!(
            external_terrain_disposition(baseline, [2; 32], false),
            ExternalTerrainDisposition::Adopt
        );
        assert_eq!(
            external_terrain_disposition(baseline, [2; 32], true),
            ExternalTerrainDisposition::Conflict
        );
        assert_eq!(
            external_terrain_disposition(baseline, [1; 32], true),
            ExternalTerrainDisposition::Unchanged
        );
    }

    #[test]
    fn glb_topology_reload_invalidates_only_its_own_scoped_history() {
        let a = TerrainId::parse("terrain-a").unwrap();
        let b = TerrainId::parse("terrain-b").unwrap();
        let history = vec![
            EditorCommand::TerrainChanged(Vec::new()),
            EditorCommand::TerrainCanvas {
                id: a.clone(),
                command: Box::new(EditorCommand::TerrainChanged(Vec::new())),
            },
            EditorCommand::TerrainCanvas {
                id: b.clone(),
                command: Box::new(EditorCommand::TerrainChanged(Vec::new())),
            },
        ];
        let kept: Vec<_> = history
            .into_iter()
            .filter(|command| !command_uses_terrain_topology(command, &a))
            .collect();
        assert_eq!(kept.len(), 1);
        assert!(matches!(&kept[0], EditorCommand::TerrainCanvas { id, .. } if id == &b));
    }
}
