//! 右侧Asset所选GLB/GLTF的异步加载、分帧primitive检查与草稿采用。

use std::collections::{BTreeSet, HashSet, VecDeque}; // 根节点识别、LOD编号与常数工作队列。

use bevy::asset::{AssetId, LoadState}; // 根加载失败诊断与节点稳定访问身份。
use bevy::gltf::{Gltf, GltfMesh, GltfNode}; // glTF根、节点和多primitive网格事实。
use bevy::prelude::*; // Assets、Handle、Transform与ECS资源。
use feathrise_scene_content::{
    MeshPresetLod, MeshPresetMaterialOverrides, MeshPresetMeshDerivation, MeshPresetPart,
    MeshPresetPartTransform, VegetationPresetBillboard, VegetationPresetRegistry,
    VegetationPresetRuntime,
}; // 受检资产直接生成正式通用Mesh部件与LOD。

use super::defaults::{new_blade_preset, new_mesh_preset}; // 新建默认值仍通过公共目录校验。
use super::interaction::timestamp_millis; // 新建身份包含不复用时间戳。
use super::types::{
    GltfInspectionWork, InspectedAsset, InspectionJob, InspectionPurpose, PendingInspection,
    PresetDraft, PresetDraftMode, VegetationPresetEditorState,
}; // 加载、分帧检查和草稿状态。

const INSPECTION_WORK_UNITS_PER_FRAME: usize = 64; // 每帧最多处理六十四个节点或primitive，不随资产总量增长。
const TRANSFORM_EPSILON: f32 = 0.001; // GLTF浮点噪声范围内仍视为作者已应用变换。

/// 冻结右侧所选GLB并请求根Gltf；这里只登记异步加载。
pub(super) fn begin_asset_inspection(
    state: &mut VegetationPresetEditorState,
    selected_asset: Option<&str>,
    asset_server: &AssetServer,
    purpose: InspectionPurpose,
) -> Result<(), String> {
    if state.busy() {
        return Err("已有预设资产检查、保存或热采用任务正在进行".to_owned());
    }
    let path = selected_asset.ok_or_else(|| "请先在右侧Asset浏览器选择一个GLB/GLTF".to_owned())?;
    let lower = path.to_ascii_lowercase();
    if !lower.ends_with(".glb") && !lower.ends_with(".gltf") {
        return Err("植被预设资产必须是GLB或GLTF场景文件".to_owned());
    }
    let handle = asset_server.load::<Gltf>(path.to_owned());
    state.pending_inspection = Some(PendingInspection {
        handle,
        source_path: path.to_owned(),
        purpose,
    });
    state.status = format!("正在异步加载并检查Asset：{path}");
    Ok(())
}

/// 非阻塞推进根加载和已加载GLTF的有界节点/primitive检查。
pub(super) fn poll_asset_inspection(
    mut state: ResMut<VegetationPresetEditorState>,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    gltf_nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    registry: Option<Res<VegetationPresetRegistry>>,
) {
    if let Some(pending) = state.pending_inspection.clone() {
        match asset_server.load_state(pending.handle.id()) {
            LoadState::Failed(error) => {
                state.pending_inspection = None;
                state.status = format!("加载Asset {}失败：{error}", pending.source_path);
                return;
            }
            _ if !asset_server.is_loaded_with_dependencies(pending.handle.id()) => return,
            _ => {}
        }
        let Some(gltf) = gltfs.get(&pending.handle) else {
            state.pending_inspection = None;
            state.status = format!("Asset {}报告已加载但缺少Gltf根资产", pending.source_path);
            return;
        };
        match initialize_work(&pending, gltf, &gltf_nodes) {
            Ok(work) => {
                state.pending_inspection = None;
                state.inspection_work = Some(work);
            }
            Err(error) => {
                state.pending_inspection = None;
                state.status = error;
                return;
            }
        }
    }
    let Some(work) = state.inspection_work.as_mut() else {
        return;
    };
    let mut failure = None;
    for _ in 0..INSPECTION_WORK_UNITS_PER_FRAME {
        let Some(job) = work.queue.pop_front() else {
            break;
        };
        if let Err(error) = process_job(work, job, &gltf_nodes, &gltf_meshes) {
            failure = Some(error);
            break;
        }
    }
    if let Some(error) = failure {
        state.inspection_work = None;
        state.status = error;
        return;
    }
    if !work.queue.is_empty() {
        state.status = format!(
            "正在分帧检查GLTF节点与primitive：剩余{}项",
            work.queue.len()
        );
        return;
    }
    let work = state
        .inspection_work
        .take()
        .expect("上方已确认GLTF检查工作存在");
    let inspected = InspectedAsset {
        source_path: work.source_path,
        blade_mesh_paths: work.blade_mesh_paths,
        blade_error: work.blade_error,
        mesh_parts: work.mesh_parts,
    };
    if let Err(error) =
        apply_inspected_asset(&mut state, registry.as_deref(), work.purpose, inspected)
    {
        state.status = error;
    }
}

/// 从Gltf节点关系建立根队列；存在节点时不导入未实例化的孤立Mesh数据块。
fn initialize_work(
    pending: &PendingInspection,
    gltf: &Gltf,
    nodes: &Assets<GltfNode>,
) -> Result<GltfInspectionWork, String> {
    let mut queue = VecDeque::new();
    if gltf.nodes.is_empty() {
        for mesh in &gltf.meshes {
            queue.push_back(InspectionJob::Primitive {
                mesh: mesh.clone(),
                transform: Transform::IDENTITY,
                index: 0,
            });
        }
    } else {
        let mut child_ids = HashSet::<AssetId<GltfNode>>::new();
        for handle in &gltf.nodes {
            let node = nodes
                .get(handle)
                .ok_or_else(|| format!("Asset {}缺少已声明GltfNode", pending.source_path))?;
            child_ids.extend(node.children.iter().map(Handle::id));
        }
        let roots = gltf
            .nodes
            .iter()
            .filter(|handle| !child_ids.contains(&handle.id()))
            .cloned()
            .collect::<Vec<_>>();
        if roots.is_empty() {
            return Err(format!(
                "Asset {}的节点层级没有根，可能存在循环",
                pending.source_path
            ));
        }
        for handle in roots {
            queue.push_back(InspectionJob::Node {
                handle,
                parent: Transform::IDENTITY,
            });
        }
    }
    if queue.is_empty() {
        return Err(format!("Asset {}没有节点或Mesh", pending.source_path));
    }
    Ok(GltfInspectionWork {
        source_path: pending.source_path.clone(),
        purpose: pending.purpose,
        queue,
        visited_nodes: HashSet::new(),
        blade_mesh_paths: Vec::new(),
        blade_error: None,
        mesh_parts: Vec::new(),
    })
}

/// 处理一个节点或一个primitive工作单元，并把后继任务放回队列。
fn process_job(
    work: &mut GltfInspectionWork,
    job: InspectionJob,
    nodes: &Assets<GltfNode>,
    meshes: &Assets<GltfMesh>,
) -> Result<(), String> {
    match job {
        InspectionJob::Node { handle, parent } => {
            if !work.visited_nodes.insert(handle.id()) {
                return Ok(()); // 合法glTF是树；重复引用只需处理第一次，损坏循环也不会卡死。
            }
            let node = nodes
                .get(&handle)
                .ok_or_else(|| format!("Asset {}检查时缺少GltfNode", work.source_path))?;
            let transform = parent.mul_transform(node.transform);
            if let Some(mesh) = node.mesh.clone() {
                work.queue.push_back(InspectionJob::Primitive {
                    mesh,
                    transform,
                    index: 0,
                });
            }
            for child in &node.children {
                work.queue.push_back(InspectionJob::Node {
                    handle: child.clone(),
                    parent: transform,
                });
            }
        }
        InspectionJob::Primitive {
            mesh,
            transform,
            index,
        } => {
            let gltf_mesh = meshes
                .get(&mesh)
                .ok_or_else(|| format!("Asset {}检查时缺少GltfMesh", work.source_path))?;
            let Some(primitive) = gltf_mesh.primitives.get(index) else {
                return Ok(()); // 空GltfMesh由最终“无部件”校验给出统一诊断。
            };
            let part_transform = mesh_part_transform(transform)?;
            let mesh_path = primitive
                .mesh
                .path()
                .map(ToString::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "{}#Mesh{}/Primitive{}",
                        work.source_path, primitive.parent_mesh_index, primitive.index
                    )
                });
            let material_path = primitive
                .material
                .as_ref()
                .and_then(Handle::path)
                .map(|path| format!("{path}/std"))
                .unwrap_or_else(|| format!("{}#DefaultMaterial/std", work.source_path));
            let part_id = u32::try_from(work.mesh_parts.len())
                .map_err(|_| "单个GLTF的Mesh部件数量超过u32数据格式范围".to_owned())?;
            work.mesh_parts.push(MeshPresetPart {
                part_id,
                mesh_path: mesh_path.clone(),
                mesh_derivation: MeshPresetMeshDerivation::Direct, // 新导入的作者几何默认原样使用，派生代理由详细编辑器显式开启。
                material_path,
                material_overrides: MeshPresetMaterialOverrides::default(), // 不猜 GLTF 材质需要 Alpha 或双面覆盖。
                transform: part_transform,
                casts_shadow: true,
                billboard: VegetationPresetBillboard::Fixed,
                wind: None,
            });
            if blade_transform_is_identity(transform) {
                work.blade_mesh_paths.push(mesh_path);
            } else if work.blade_error.is_none() {
                work.blade_error = Some(format!(
                    "Asset {}含未应用的节点平移、旋转或缩放；Blade预设不能保存部件变换，请先在Blender应用变换，或改建模型预设",
                    work.source_path
                ));
            }
            if index + 1 < gltf_mesh.primitives.len() {
                work.queue.push_back(InspectionJob::Primitive {
                    mesh,
                    transform,
                    index: index + 1,
                });
            }
        }
    }
    Ok(())
}

/// 把累计节点变换约束到通用Mesh数据格式支持的平移、Y旋转和逐轴缩放。
fn mesh_part_transform(transform: Transform) -> Result<MeshPresetPartTransform, String> {
    if !transform.translation.is_finite()
        || !transform.scale.is_finite()
        || transform.scale.x.abs() <= f32::EPSILON
        || transform.scale.y.abs() <= f32::EPSILON
        || transform.scale.z.abs() <= f32::EPSILON
    {
        return Err("GLTF节点包含非有限值或零缩放，不能建立植被预设".to_owned());
    }
    let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
    if !yaw.is_finite()
        || !pitch.is_finite()
        || !roll.is_finite()
        || pitch.abs() > TRANSFORM_EPSILON
        || roll.abs() > TRANSFORM_EPSILON
    {
        return Err(
            "GLTF节点含X/Z旋转或不可分解变换；请在Blender应用变换后再建立植被预设".to_owned(),
        );
    }
    Ok(MeshPresetPartTransform {
        translation: transform.translation.to_array(),
        yaw_degrees: yaw.to_degrees(),
        scale: transform.scale.to_array(),
    })
}

/// Blade运行时没有部件变换字段，只接受已经应用的零变换primitive。
fn blade_transform_is_identity(transform: Transform) -> bool {
    transform.translation.length() <= TRANSFORM_EPSILON
        && transform.rotation.angle_between(Quat::IDENTITY) <= TRANSFORM_EPSILON
        && (transform.scale - Vec3::ONE).abs().max_element() <= TRANSFORM_EPSILON
}

/// 把完整受检资产应用到新建草稿或当前编辑草稿。
fn apply_inspected_asset(
    state: &mut VegetationPresetEditorState,
    registry: Option<&VegetationPresetRegistry>,
    purpose: InspectionPurpose,
    inspected: InspectedAsset,
) -> Result<(), String> {
    match purpose {
        InspectionPurpose::CreateBlade | InspectionPurpose::CreateMesh => {
            let registry = registry.ok_or_else(|| "植被预设目录尚未加载完成".to_owned())?;
            if let Some(error) = registry.last_error.as_deref() {
                return Err(format!("当前预设目录无效，不能新建：{error}"));
            }
            let descriptor = if purpose == InspectionPurpose::CreateBlade {
                new_blade_preset(&inspected, registry.presets(), timestamp_millis())?
            } else {
                new_mesh_preset(&inspected, registry.presets(), timestamp_millis())?
            };
            let label = descriptor.label.clone();
            state.open_draft(PresetDraft::new(PresetDraftMode::Create, descriptor));
            state.status = format!("已从真实 GLTF 建立“{label}”中央编辑标签");
        }
        InspectionPurpose::ReplaceLod { tab_id, lod } => {
            replace_draft_lod_asset(state, tab_id, lod, inspected)?
        }
        InspectionPurpose::AppendMesh { tab_id } => append_mesh_lod(state, tab_id, inspected)?,
    }
    Ok(())
}

/// 用右侧Asset替换当前数据预设LOD；所有运行时字段继续保持可编辑。
fn replace_draft_lod_asset(
    state: &mut VegetationPresetEditorState,
    tab_id: u64,
    lod_index: usize,
    inspected: InspectedAsset,
) -> Result<(), String> {
    let draft = state
        .draft_mut(tab_id)
        .ok_or_else(|| "目标预设标签已关闭".to_owned())?;
    let status = match &mut draft.descriptor.runtime {
        VegetationPresetRuntime::Blade { lods, .. } => {
            if let Some(error) = inspected.blade_error {
                return Err(error);
            }
            if inspected.blade_mesh_paths.len() > 1 {
                return Err(
                    "GLB有多个网格；请使用当前LOD路径旁的“使用选中”明确选择子网格".to_owned(),
                ); // 整层旧入口也不得隐式猜第一个。
            }
            let selected = inspected
                .blade_mesh_paths
                .first()
                .ok_or_else(|| "所选GLB没有可用Blade primitive".to_owned())?;
            let lod = lods
                .get_mut(lod_index)
                .ok_or_else(|| "目标 Blade LOD 已不存在".to_owned())?;
            lod.mesh_path = selected.clone(); // 当前档只采用已确认唯一的零变换primitive，多候选必须走显式选择。
            // 替换LOD不改内部兼容来源或自由备注；基础页不再把来源伪装成模型配置入口。
            format!("已用所选 Asset 更新 Blade LOD {}", lod.level)
        }
        VegetationPresetRuntime::Mesh { lods, .. } => {
            if inspected.mesh_parts.is_empty() {
                return Err("所选GLB没有可用Mesh primitive".to_owned());
            }
            let lod = lods
                .get_mut(lod_index)
                .ok_or_else(|| "目标 Mesh LOD 已不存在".to_owned())?;
            lod.parts = inspected.mesh_parts;
            // 整层导入只替换当前档全部部件，保留作者来源与本档可见距离。
            draft.selected_part = 0;
            format!("已用所选 Asset 替换 Mesh LOD {} 的全部部件", lod.level)
        }
    };
    state.status = status;
    state.touch_ui();
    Ok(())
}

/// 追加任意数量Mesh LOD，直到数据格式的u8档位身份空间实际用完。
fn append_mesh_lod(
    state: &mut VegetationPresetEditorState,
    tab_id: u64,
    inspected: InspectedAsset,
) -> Result<(), String> {
    if inspected.mesh_parts.is_empty() {
        return Err("所选GLB没有可用Mesh primitive".to_owned());
    }
    let draft = state
        .draft_mut(tab_id)
        .ok_or_else(|| "目标预设标签已关闭".to_owned())?;
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut draft.descriptor.runtime else {
        return Err("只有数据驱动Mesh预设可以追加LOD".to_owned());
    };
    let used = lods.iter().map(|lod| lod.level).collect::<BTreeSet<_>>();
    let level = (0..=u8::MAX)
        .find(|candidate| !used.contains(candidate))
        .ok_or_else(|| "Mesh LOD的u8稳定档位身份已经全部占用".to_owned())?;
    let base = lods.last().map(|lod| lod.end[1]).unwrap_or(0.0).max(0.0);
    lods.push(MeshPresetLod {
        level,
        start: [(base - 20.0).max(0.0), base],
        end: [base + 80.0, base + 100.0],
        parts: inspected.mesh_parts,
    });
    draft.selected_lod = lods.len() - 1;
    draft.selected_part = 0;
    state.status = format!("已追加 Mesh LOD {level}；请在中央表单精调距离与部件参数");
    state.touch_ui();
    Ok(())
}

#[cfg(test)]
mod tests; // 导入与整层替换回归独立成文件，避免继续扩张生产检查模块。
