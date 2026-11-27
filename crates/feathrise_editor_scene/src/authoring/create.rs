//! 新场景后台创建事务与主线程采用系统。

use std::path::Path; // 私有目录和目录JSON路径拼接。
use std::time::{SystemTime, UNIX_EPOCH}; // 无额外依赖生成稳定ASCII草稿ID。

use super::creation_input::{SceneDimensions, normalize_display_name, scene_directory_name, validated_creation_request};
use super::files::{
    atomic_write, atomic_write_json, decode_scene_catalog, workspace_asset_root,
};
use super::ktx2::encode_surface_maps; // 新场景一次预生成严格匹配的地表索引和权重 Mip。
use super::vegetation_defaults::empty_vegetation_authoring; // 新场景的正式输出和作者画布都从空白开始。
use super::water_defaults::default_water_asset;
use crate::constants::SCENE_FOLDER_ASSET_PATH; // 创建归入虚拟文件夹后显式刷新编辑器专用资产。
use crate::scene_browser::prepare_scene_assignment_mutation; // 新场景归属与catalog入口组成联合发布事务。
use crate::scene_browser::{expand_folder_ancestors, selected_container_folder_id}; // 树选择决定创建归属并在成功后展开完整父链。
use crate::resources::{
    EditorAssetCatalog, SceneBrowserState, SceneCreationResult, SceneCreationTask, SceneEditorState, SceneFolderPersistence,
};
use crate::types::{CreatedScene, SceneBrowserItem, SceneCreationRequest, NewSceneTerrainMode};
use crate::types::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget};
use bevy::prelude::*; // ECS资源与日志。
use bevy::tasks::futures::check_ready; // Update只非阻塞探测任务结果。
use bevy::tasks::IoTaskPool; // 空容器和分页高度源的目录事务均在IO池完成。
use feathrise_scene_content::{
    GENERATED_SCENE_ID_PREFIX, SCENE_BOUNDARIES_FILE_NAME, SCENE_CATALOG_ASSET_PATH, SCENE_MANIFEST_FILE_NAME,
    SCENE_PLACEMENTS_GLB_FILE_NAME, SCENE_PORTALS_FILE_NAME, SCENE_SCHEMA_VERSION, SCENE_SURFACE_FILE_NAME,
    SCENE_SURFACE_SCHEMA_VERSION, SCENE_VEGETATION_FILE_NAME, SCENE_WATER_FILE_NAME, SceneBoundariesAsset,
    SceneCatalogEntry, SceneClipmapCollisionMode, SceneId, SceneManifestAsset,
    ScenePlacementsAsset, ScenePortalsAsset, SceneSurfaceAsset, SceneTerrainDescriptor, SceneVegetationAsset,
    SparseHeightDocument, SparseHeightSpec, sparse_height_manifest_asset_path,
}; // 新建文件严格复用运行时资产契约。 // 后台任务只返回轻量成功身份。

pub(crate) const DEFAULT_HEIGHT_SCALE: f32 = 160.0; // 显示LOD新建回显与最终后台创建必须使用同一高度规格。
pub(crate) const DEFAULT_BASE_HEIGHT: f32 = -80.0; // 平地位于量程中点，可同时向上造山和向下开河。
const DEFAULT_SURFACE_RESOLUTION: [u32; 2] = [512, 512]; // 固定作者纹理预算；世界尺寸只改变每纹素覆盖米数。

/// 新场景先保留可编辑的空白地表文件，作者手动新建层后才引用预设。
pub(crate) fn new_scene_surface_asset(scene_id: &SceneId, folder_relative: &str) -> SceneSurfaceAsset {
    SceneSurfaceAsset {
        schema_version: SCENE_SURFACE_SCHEMA_VERSION, // 继续使用正式地表文件版本。
        scene_id: scene_id.clone(),                     // 作者层所属场景身份。
        weight_source_path: format!("{folder_relative}/surface/weights.rgba8"), // 全零作者权重源。
        weight_texture_path: format!("{folder_relative}/surface/weights.ktx2"), // 成对运行时权重纹理。
        index_source_path: format!("{folder_relative}/surface/indices.rgba32"), // 全零作者索引源。
        index_texture_path: format!("{folder_relative}/surface/indices.ktx2"), // 成对运行时索引纹理。
        resolution: DEFAULT_SURFACE_RESOLUTION, // 两条地形路线共用画布规格。
        layers: Vec::new(),                     // 未显式新建作者层之前保持白模。
    }
}

/// 保留可手动绘制的完整尺寸画布，但新图不产生任何隐含材质贡献。
fn empty_new_scene_surface_sources() -> (Vec<u8>, Vec<u8>) {
    let surface_texels =
        DEFAULT_SURFACE_RESOLUTION[0] as usize * DEFAULT_SURFACE_RESOLUTION[1] as usize; // 作者画布纹素数与地形路线无关。
    (
        vec![0u8; surface_texels * 16], // 四个u32索引都不引用预设。
        vec![0u8; surface_texels * 4],  // 四个u8权重都没有绘制贡献。
    )
}

/// 后台计算阶段交给统一目录事务的路线专属产物。
enum PreparedTerrain {
    /// 真正空图没有地形文件或派生产物。
    Empty,
    /// Clipmap 空白图无需预分配任何整图顶点或高度数组。
    SparseClipmap,
}

/// 构造新场景的植被文件：目录预设只在作者首次使用时延迟写入。
fn default_new_scene_vegetation(
    scene_id: &SceneId,         // JSON所属稳定场景身份。
    _folder_relative: &str, // 保留纯构造器接口，实际路径由首次使用的预设生成。
) -> SceneVegetationAsset {
    SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: SCENE_SCHEMA_VERSION, // 使用当前数据驱动场景格式。
        scene_id: scene_id.clone(),             // 资产身份必须与私有目录一致。
        layers: Vec::new(), // 未使用的第 1、100 或 1000 个目录预设都不占场景引用与磁盘文件。
        authoring: Some(empty_vegetation_authoring()), // 显式空结构阻止旧图迁移逻辑猜测内容。
    }
}

/// 目录名与引用身份分离，同时避开既有目录登记和 SDK 按 ID 保留的地形路径。
pub(super) fn next_scene_id(scenes_root: &Path, entries: &[SceneCatalogEntry], unix_millis: u128) -> Result<SceneId, String> {
    let base = format!("{GENERATED_SCENE_ID_PREFIX}{unix_millis}");
    for suffix in 0..10_000u32 {
        let value = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        if !entries.iter().any(|entry| entry.id.as_str().eq_ignore_ascii_case(&value))
            && std::fs::symlink_metadata(scenes_root.join(&value))
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return SceneId::parse(value);
        }
    }
    Err("同一时间生成的场景ID已超过上限".to_owned())
}

/// 新建和复制共用显示名、作者目录和 SDK 保留目录的冲突规则。
pub(super) fn ensure_unique_scene_name(entries: &[SceneCatalogEntry], display_name: &str, directory_name: &str) -> Result<(), String> {
    let folder_key = format!("scenes/{directory_name}").to_lowercase();
    let name_key = display_name.to_lowercase();
    if entries.iter().any(|entry| {
        entry.display_name.trim().to_lowercase() == name_key
            || entry.folder.trim_end_matches('/').to_lowercase() == folder_key
            || entry.id.as_str().eq_ignore_ascii_case(directory_name)
    }) {
        return Err(format!("场景目录已登记同名场景或目录“{directory_name}”"));
    }
    Ok(())
}

/// 由冻结路线构造唯一地形作者源描述；同一清单不能同时登记两种可写源。
fn new_scene_terrain_descriptor(
    terrain_mode: NewSceneTerrainMode,
    scene_id: &SceneId,
    folder_relative: &str,
    resolution: [u32; 2],
) -> SceneTerrainDescriptor {
    if terrain_mode == NewSceneTerrainMode::Empty { return SceneTerrainDescriptor::empty(); }
    // 创建只冻结作者路线与唯一碰撞列表。
    let sparse = terrain_mode == NewSceneTerrainMode::SparseClipmap; // GLB数据不变。
    SceneTerrainDescriptor {resolved_stitches: Default::default(),
        kind: terrain_mode.terrain_kind(), // 保持用户选择的地形作者类型。
        collision_mode: SceneClipmapCollisionMode::LayeredSharedChunks, // 兼容读标记，序列化不再输出。
        collision_lods: if sparse {
            feathrise_scene_content::default_collision_lods_for_resolution(resolution)
        } else {
            Vec::new()
        }, // 仅初始默认按高度层级适配。
        display_lods: None, // 稀疏新图在完整SparseHeightSpec冻结后写入显式默认列表，GLB始终保持None。
        player_collision_preload_radius_meters: 16.0, // 只读兼容字段不会写入新清单。
        role_collision_preload_radius_meters: None, // 旧分块设置已废弃。
        player_collision_chunk_cells: None, // 新固定块不接受旧格数。
        role_collision_chunk_cells: None, // 旧分块兼容字段不参与新图配置。
        player_collision_span_meters: None, // 距离不再联动块大小。
        role_collision_span_meters: None, // 不写旧分块范围覆盖。
        glb_path: if sparse {
            String::new()
        } else {
            format!("{folder_relative}/terrain/terrain.glb")
        }, // 唯一作者路线。
        height_path: sparse.then(|| sparse_height_manifest_asset_path(scene_id)), // 稀疏高度源路径不改。
        height_texture_path: None,          // 运行时仍使用现有显示派生。
        resolution,                         // 保持作者格点数。
        base_height: DEFAULT_BASE_HEIGHT,   // 高度量程保持不变。
        height_scale: DEFAULT_HEIGHT_SCALE, // 碰撞配置不改变作者精度。
    } // 完整请求稍后覆盖用户列表。
}

/// 新场景目录与作者文件同批发布；纯构造器便于同时验证两条路线的真实注册入口。
fn new_scene_catalog_entry(
    scene_id: &SceneId,
    display_name: &str,
    folder: &str,
    terrain_mode: NewSceneTerrainMode,
) -> SceneCatalogEntry {
    // 不读取磁盘，不分配任何密度画布。
    SceneCatalogEntry {
        // 只登记创建事务已经完整写入的内容。
        id: scene_id.clone(),                    // 稳定场景身份。
        display_name: display_name.to_owned(), // 已通过创建请求名称校验。
        folder: folder.to_owned(),             // 物理目录使用名称，ID 仍用于保存与引用。
        published: true,                       // 完整创建后公开场景。
        placements_glb: true,
        surface_path: (terrain_mode != NewSceneTerrainMode::Empty).then(|| format!("{folder}/{SCENE_SURFACE_FILE_NAME}")), // 空图不创建无地形可承载的画布。
        vegetation_path: Some(format!("{folder}/{SCENE_VEGETATION_FILE_NAME}")),
        water_path: Some(format!("{folder}/{SCENE_WATER_FILE_NAME}")),
        characters_path: None, // 旧格式入口保持为空；外部作者首次使用时自行登记对应文件。
        boundary_path: (terrain_mode != NewSceneTerrainMode::Empty).then(|| format!("{folder}/{SCENE_BOUNDARIES_FILE_NAME}")), // 空图没有隐含安全墙。
    } // 目录是创建事务最后一个可见目标。
} // 不为新图预建任何植被正式层。

/// 在IO线程写入所选地形作者源和公共私有文件，最后才原子发布目录入口。
fn create_scene(
    asset_root: &Path,
    request: SceneCreationRequest,
    prepared_terrain: PreparedTerrain,
) -> Result<CreatedScene, String> {
    let display_name = normalize_display_name(&request.display_name)?;
    let directory_name = scene_directory_name(&display_name)?;
    let dimensions = SceneDimensions::for_mode(
        request.terrain_mode,
        request.width_meters,        // 后台重验冻结的场景X尺寸。
        request.length_meters,       // 后台重验冻结的场景Z尺寸。
        request.terrain_cell_meters, // 后台按冻结路线重验作者格距与容量，不能只信UI。
    )?;
    let _content_author_lock = feathrise_scene_content::ContentAuthorLock::acquire(asset_root)?; // 场景目录发布与已有领域作者事务继续使用同一写锁，避免并发丢登记。
    let scenes_root = asset_root.join("scenes");
    let catalog_path = asset_root.join(SCENE_CATALOG_ASSET_PATH);
    let original_catalog_bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取场景目录{}失败：{error}", catalog_path.display()))?; // 保留原字节用于并发检测。
    let mut catalog = decode_scene_catalog(&catalog_path, &original_catalog_bytes)?; // 与删除事务复用同一目录损坏和版本保护门。
    let unix_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间早于Unix纪元：{error}"))?
        .as_millis();
    let scene_id = next_scene_id(&scenes_root, &catalog.scenes, unix_millis)?;
    let folder_relative = format!("scenes/{directory_name}");
    ensure_unique_scene_name(&catalog.scenes, &display_name, &directory_name)?;

    let folder = asset_root.join(&folder_relative);
    // 独占创建成功后才写入；预先存在的普通目录、文件或链接均不能被接管。
    std::fs::create_dir(&folder).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            format!("场景私有目录已存在：{}", folder.display())
        } else {
            format!("创建场景目录{}失败：{error}", folder.display())
        }
    })?;
    let mut sparse_folder = None;

    let result = (|| {
        let mesh_resolution = dimensions.terrain_mesh_resolution; // 原始路线表示GLB顶点，Clipmap路线表示作者逻辑格点。
        let height_resolution = dimensions.terrain_height_resolution; // 两条路线使用各自经过校验的清单规格。
        match (request.terrain_mode, prepared_terrain) {
            (NewSceneTerrainMode::Empty, PreparedTerrain::Empty) => {},
            (NewSceneTerrainMode::SparseClipmap, PreparedTerrain::SparseClipmap) => {
                // 已发布 SDK 的稀疏高度清单仍使用稳定 ID 路径，不随场景资料目录更名。
                let sdk_folder = scenes_root.join(scene_id.as_str());
                if sdk_folder != folder {
                    std::fs::create_dir(&sdk_folder)
                        .map_err(|error| format!("创建场景高度目录{}失败：{error}", sdk_folder.display()))?;
                    sparse_folder = Some(sdk_folder);
                }
                let sparse_spec = SparseHeightSpec::new(
                    dimensions.world_min,
                    dimensions.world_max,
                    request.terrain_cell_meters,
                    DEFAULT_BASE_HEIGHT,
                    DEFAULT_HEIGHT_SCALE,
                )?; // 只计算标量元数据，不生成最大场景的整图网格。
                let mut sparse_height =
                    SparseHeightDocument::create_empty(asset_root, scene_id.clone(), sparse_spec)?; // 独立编辑器可复用同一稀疏作者数据入口。
                sparse_height.save_dirty()?; // 先发布空白稀疏清单，最后才发布场景目录入口。
            }
            _ => return Err("新建场景地形方案与后台准备产物不一致".to_owned()),
        }

        // 走正式清单的默认规则，让公开固定 SDK 和新增可选字段的本地 SDK 共用创建入口。
        let mut manifest: SceneManifestAsset = serde_json::from_value(serde_json::json!({
            "schema_version": SCENE_SCHEMA_VERSION,
            "id": scene_id,
            "display_name": display_name,
            "notes": request.notes,
            "runtime_scene": feathrise_scene_content::SceneRuntimeKind::DataDriven.as_str(),
            "world_min": dimensions.world_min,
            "world_max": dimensions.world_max,
            "default_spawn": [0.0, 2.0, 0.0],
            "asset_roots": [folder_relative.as_str(), "gltf", "textures"],
        })).map_err(|error| format!("构造新场景清单失败：{error}"))?;
        manifest.environment = feathrise_scene_content::SceneEnvironmentDescriptor::inheriting_default(); // 只给新场景选项目默认，保留旧清单的原解析。
        manifest.terrains = (request.terrain_mode == NewSceneTerrainMode::Empty).then(Default::default);
        manifest.terrain = new_scene_terrain_descriptor(request.terrain_mode, &scene_id, &folder_relative, height_resolution);
        if request.terrain_mode == NewSceneTerrainMode::SparseClipmap {
            if sparse_folder.is_some() { manifest.asset_roots.push(format!("scenes/{scene_id}")); }
            // 新模式才写完整列表。
            let spec = SparseHeightSpec::new(
                dimensions.world_min,
                dimensions.world_max,
                request.terrain_cell_meters,
                DEFAULT_BASE_HEIGHT,
                DEFAULT_HEIGHT_SCALE,
            )?; // 后台再次核验冻结请求。
            feathrise_scene::validate_layered_collision_settings(&spec, &request.collision_lods)?; // 不信任仅前台校验。
            manifest
                .terrain
                .collision_lods
                .clone_from(&request.collision_lods); // 与其他清单字段同批发布。
            let display_lods = request
                .display_lods
                .as_ref()
                .ok_or("SparseClipmap新图缺少显式显示LOD配置")?; // 新图不得意外落回旧缺字段兼容模式。
            feathrise_scene_content::compile_display_lod_plan(&spec, display_lods)?; // 后台再次执行量化、Mesh和最坏页预算预检。
            manifest.terrain.display_lods = Some(display_lods.clone()); // 与清单和高度规格同批原子发布。
        } // 不给旧场景补写新格式。
        atomic_write_json(&folder.join(SCENE_MANIFEST_FILE_NAME), &manifest)?;
        atomic_write_json(
            &folder.join(SCENE_PORTALS_FILE_NAME),
            &ScenePortalsAsset {
                schema_version: SCENE_SCHEMA_VERSION,
                scene_id: scene_id.clone(),
                portals: Vec::new(),
            },
        )?;
        let placement_glb = feathrise_scene_content::write_scene_placements_glb(
            &ScenePlacementsAsset {
                schema_version: SCENE_SCHEMA_VERSION,
                scene_id: scene_id.clone(),
                placements: Vec::new(),
            },
            &format!("{folder_relative}/{SCENE_PLACEMENTS_GLB_FILE_NAME}"),
            None,
        )?;
        atomic_write(&folder.join(SCENE_PLACEMENTS_GLB_FILE_NAME), &placement_glb)?;
        if request.terrain_mode != NewSceneTerrainMode::Empty {
        let surface_folder = folder.join("surface");
        let (surface_indices, surface_weights) = empty_new_scene_surface_sources(); // 新场景不自动涂绘，空层全零权重着色为白模。
        let (index_texture, weight_texture) = encode_surface_maps(
            &surface_indices,
            &surface_weights,
            DEFAULT_SURFACE_RESOLUTION,
            0, // 两种地形路线都不引用全局目录第一项。
        )?;
        atomic_write(&surface_folder.join("indices.rgba32"), &surface_indices)?;
        atomic_write(&surface_folder.join("weights.rgba8"), &surface_weights)?;
        atomic_write(&surface_folder.join("indices.ktx2"), &index_texture)?;
        atomic_write(&surface_folder.join("weights.ktx2"), &weight_texture)?;
        atomic_write_json(
            &folder.join(SCENE_SURFACE_FILE_NAME), // 白模也要登记可继续编辑的地表文件。
            &new_scene_surface_asset(&scene_id, &folder_relative), // 仅显式新建层后才加入预设引用。
        )?;
        }
        let vegetation = default_new_scene_vegetation(&scene_id, &folder_relative); // 零输出文件只写轻量 JSON，不预分配任何预设密度。
        atomic_write_json(
            &folder.join(SCENE_VEGETATION_FILE_NAME),
            &vegetation, // 目录首次热加载直接得到零作者层，不依赖随后修正。
        )?;
        let water = default_water_asset(&scene_id); // 空水域JSON先登记；首张R8/KTX2只在作者显式新建后产生。
        atomic_write_json(&folder.join(SCENE_WATER_FILE_NAME), &water)?;
        if request.terrain_mode != NewSceneTerrainMode::Empty {
        let boundaries = SceneBoundariesAsset::rectangular_default(
            &scene_id,              // 新边界文件必须属于本次唯一场景ID。
            dimensions.world_min, // 四面安全墙使用新场景实际X/Z最小角。
            dimensions.world_max, // 非方形场景分别沿东西宽度与南北长度生成。
            DEFAULT_BASE_HEIGHT,  // 墙底覆盖高度场最低可雕刻位置以下。
            DEFAULT_HEIGHT_SCALE, // 墙顶覆盖完整可雕刻高度和额外跳跃余量。
        )?;
        atomic_write_json(&folder.join(SCENE_BOUNDARIES_FILE_NAME), &boundaries)?; // 边界私有文件完整写入后才发布目录入口。
        }

        let folder_mutation = prepare_scene_assignment_mutation(
            asset_root,
            request.folder_source_bytes.clone(),
            &scene_id,
            request.folder_id.as_deref(),
        )?; // 当前虚拟文件夹在点击确认时冻结，根目录新场景无需改写归类文件。

        // 全部私有文件成功后才联合发布归属和catalog入口，AssetServer永远不会发现半成品场景。
        catalog.scenes.push(new_scene_catalog_entry(
            &scene_id,
            &display_name,
            &folder_relative,
            request.terrain_mode,
        )); // 私有文件全部完成后，两条路线均公开Blade植被入口。
        let latest_catalog_bytes = std::fs::read(&catalog_path)
            .map_err(|error| format!("再次读取场景目录{}失败：{error}", catalog_path.display()))?; // 发布前检测外部并发修改。
        if latest_catalog_bytes != original_catalog_bytes {
            return Err("场景目录在创建检查期间发生变化，请重试".to_owned()); // 禁止覆盖同时发生的删除或外部编辑。
        }
        if let Some(mutation) = folder_mutation.as_ref() {
            mutation.publish_checked()?; // 先发布场景归属，catalog入口随后才对运行时可见。
        }
        if let Err(catalog_error) = atomic_write_json(&catalog_path, &catalog) {
            let rollback = folder_mutation.as_ref().map(|mutation| mutation.rollback()); // catalog失败必须恢复原归类文件。
            return match rollback {
                Some(Err(rollback_error)) => Err(format!(
                    "发布场景目录失败：{catalog_error}；恢复场景归类也失败：{rollback_error}"
                )),
                _ => Err(format!("发布场景目录失败：{catalog_error}")),
            };
        }
        let folder_snapshot = folder_mutation.as_ref().map(|mutation| mutation.snapshot()); // 主世界直接采用联合事务成功值。
        Ok(CreatedScene {
            scene_id,
            display_name,
            folder: folder_relative,
            width_meters: dimensions.width_meters,
            length_meters: dimensions.length_meters,
            terrain_mode: request.terrain_mode,
            terrain_mesh_resolution: mesh_resolution, // 完成回执按路线解释为GLB顶点或作者逻辑格点。
            folder_snapshot,
        })
    })();
    if result.is_err() {
        // 新目录尚未进入全局目录，失败时可安全清除本次唯一目标；既有场景不受影响。
        let _ = std::fs::remove_dir_all(&folder);
        if let Some(folder) = sparse_folder { let _ = std::fs::remove_dir_all(folder); }
    }
    result
}

/// 启动一份已经校验的创建快照；调用方保证同一时间没有其他在途事务。
fn start_scene_creation(
    editor: &mut SceneEditorState,
    task: &mut SceneCreationTask,
    request: SceneCreationRequest,
) {
    let dimensions = SceneDimensions::for_mode(
        request.terrain_mode,
        request.width_meters,        // 启动前再次确认冻结的世界X尺寸。
        request.length_meters,       // 启动前再次确认冻结的世界Z尺寸。
        request.terrain_cell_meters, // 启动前按路线再次确认作者格距与容量。
    )
    .expect("新建请求在进入任务槽前已经完成尺寸校验");
    let width_meters = request.width_meters;
    let length_meters = request.length_meters;
    let terrain_mode = request.terrain_mode;
    let display_name = request.display_name.clone();
    task.feedback_target = Some(EditorStatusTarget::new(&display_name, &display_name)); // 名称在进入后台前冻结，失败回执不读取后来修改的输入。
    task.ticket = task.ticket.wrapping_add(1);
    let ticket = task.ticket;
    let asset_root = workspace_asset_root();
    task.task = Some(IoTaskPool::get().spawn(async move {
        let prepared = match terrain_mode {
            NewSceneTerrainMode::Empty => Ok(PreparedTerrain::Empty),
            NewSceneTerrainMode::SparseClipmap => Ok(PreparedTerrain::SparseClipmap),
        };
        (
            ticket,
            prepared.and_then(|terrain| create_scene(&asset_root, request, terrain)).map(SceneCreationResult::Created),
        )
    })); // 同一任务槽和票据覆盖可选计算、IO及发布阶段，主线程始终只非阻塞轮询。
    let grid_label = match terrain_mode {
        NewSceneTerrainMode::Empty => "无地形格点",
        NewSceneTerrainMode::SparseClipmap => "稀疏作者格点",
    };
    editor.status = format!(
        "正在后台生成场景“{display_name}”：{width_meters}×{length_meters}米，{}，{grid_label}{}×{}",
        terrain_mode.label(),
        dimensions.terrain_mesh_resolution[0],
        dimensions.terrain_mesh_resolution[1],
    ); // 作者在生成期间可核对已经冻结的路线和实际规格。
}

/// 前台拒绝只形成需处理回执，不启动IO、不修改保存等待或创建票据。
fn record_creation_warning(editor: &mut SceneEditorState) {
    let name = editor.new_scene_name.trim();
    let target = EditorStatusTarget::new(name, name);
    editor.record_operation_result(
        EditorStatusOperation::SceneCreate,
        &target,
        EditorStatusKind::Warning,
    );
}

/// 从UI请求一次创建；多文档入口保留源草稿，独立旧入口继续先保存。
pub(crate) fn request_scene_creation(
    editor: &mut SceneEditorState,
    task: &mut SceneCreationTask,
    browser: &SceneBrowserState,
    persistence: &SceneFolderPersistence,
    preserve_document: bool,
) {
    if task.busy() {
        editor.status = "已有场景创建请求正在执行".to_owned();
        record_creation_warning(editor);
        return;
    }
    if persistence.busy() {
        editor.status = "场景归类保存完成前不能新建场景".to_owned();
        record_creation_warning(editor);
        return; // 创建任务必须冻结已落盘的归类字节基线。
    }
    if let Some(reason) = browser.write_blocked.as_deref() {
        editor.status = format!("场景归类尚未就绪，不能新建场景：{reason}");
        record_creation_warning(editor);
        return; // 首载或损坏时不能误把已选归类上下文猜成根。
    }
    let mut request = match validated_creation_request(
        &editor.new_scene_name,
        "",
        "",
        "",
        NewSceneTerrainMode::Empty, // 场景创建只生成容器，网格规格和LOD由添加网格事务创建。
    ) {
        Ok(value) => value,
        Err(error) => {
            editor.status = error; // 弹框内和底栏同时显示精确字段错误。
            record_creation_warning(editor);
            return;
        }
    };
    request.folder_id =
        selected_container_folder_id(&browser.document, browser.selected_item.as_ref()); // 目录选择归入该目录，场景选择沿用其父级，根或空选择归入根。
    request.notes.clone_from(&editor.new_scene_notes); // 保存当前场景期间冻结备注，后续输入不能改写本次创建。
    request.folder_source_bytes = browser.source_bytes.clone(); // 后台发布前使用精确原字节检测并发。
    if editor.dirty && !preserve_document {
        let display_name = request.display_name.clone();
        task.pending_after_save = Some(request); // 冻结输入，保存期间后续键盘事件不能改变目标。
        editor.save_requested = true; // 复用既有原子保存、派生资源和失败处理，不另写旁路。
        editor.status =
            format!("当前场景有未保存修改；正在先保存当前场景，成功后继续创建“{display_name}”");
        return;
    }
    start_scene_creation(editor, task, request);
}

/// 创建和复制共用场景目录采用；后台确认的归属与选中身份始终属于同一个场景。
fn adopt_created_scene_directory(
    editor: &mut SceneEditorState,
    browser: &mut SceneBrowserState,
    asset_server: &AssetServer,
    id: &SceneId,
    folder: Option<&crate::types::SceneFolderFileSnapshot>,
) {
    asset_server.reload(SCENE_CATALOG_ASSET_PATH);
    if let Some(snapshot) = folder {
        browser.document = snapshot.document.clone();
        browser.source_bytes = Some(snapshot.source_bytes.clone());
        browser.initial_load_resolved = true;
        browser.document_revision = browser.document_revision.wrapping_add(1);
        browser.write_blocked = None;
        browser.last_error = None;
        asset_server.reload(SCENE_FOLDER_ASSET_PATH); // 同进程连续发布不依赖文件监听合并时序。
    }
    let folder = browser.document.scene_folders.get(id.as_str()).map(String::as_str);
    if expand_folder_ancestors(&browser.document, folder, &mut browser.expanded_folder_ids) {
        browser.expansion_revision = browser.expansion_revision.wrapping_add(1);
    }
    browser.select_only(Some(SceneBrowserItem::Scene(id.clone())));
    editor.scene_list_selection_id = Some(id.clone());
}

/// 非阻塞采用新场景结果；显式刷新目录，避免连续原子替换被文件监听合并。
pub(crate) fn poll_scene_creation(
    mut task: ResMut<SceneCreationTask>,
    mut editor: ResMut<SceneEditorState>,
    mut assets: ResMut<EditorAssetCatalog>,
    mut browser: ResMut<SceneBrowserState>,
    asset_server: Res<AssetServer>,
) {
    if task.task.is_none()
        && !editor.dirty
        && let Some(request) = task.pending_after_save.take()
    {
        start_scene_creation(&mut editor, &mut task, request); // 保存成功后的下一帧继续原确认请求。
    }
    if let Some(control) = task.copy_control.as_ref()
        && !control.is_cancel_requested()
        && let Some(stage) = control.progress()
        && editor.status != stage
    {
        stage.clone_into(&mut editor.status);
    }
    let Some(in_flight) = task.task.as_mut() else {
        return;
    };
    let Some((ticket, result)) = check_ready(in_flight) else {
        return;
    };
    task.task = None;
    if ticket != task.ticket {
        return;
    }
    let copy = task.copy_control.take();
    match result {
        Ok(SceneCreationResult::Created(created)) => {
            adopt_created_scene_directory(&mut editor, &mut browser, &asset_server,
                &created.scene_id, created.folder_snapshot.as_ref());
            let grid_label = match created.terrain_mode {
                NewSceneTerrainMode::Empty => "无地形格点",
                NewSceneTerrainMode::SparseClipmap => "稀疏作者格点",
            };
            editor.queue_scene_transition(
                created.scene_id.clone(),
                format!(
                    "场景{} [{}]已创建：{}×{}米，{}，{grid_label}{}×{}；正在等待目录热加载并自动进入",
                    created.display_name,
                    created.scene_id,
                    created.width_meters,
                    created.length_meters,
                    created.terrain_mode.label(),
                    created.terrain_mesh_resolution[0],
                    created.terrain_mesh_resolution[1],
                ),
            ); // 创建结果不能只停留在选中态，必须登记等待清单就绪的正式切换场景请求。
            editor.new_scene_dialog_open = false;
            editor.new_scene_name.clear();
            assets.current_folder = created.folder;
            assets.requested_revision = assets.requested_revision.wrapping_add(1);
            if let Some(target) = task.feedback_target.take() {
                editor.record_operation_result(
                    EditorStatusOperation::SceneCreate,
                    &target,
                    EditorStatusKind::Success,
                ); // 完整目录已发布；原自动进入仍由切换场景任务表示，不能冒充场景已就绪。
            }
            info!("{}", editor.status);
        }
        Ok(SceneCreationResult::Copied(copied)) => {
            adopt_created_scene_directory(&mut editor, &mut browser, &asset_server,
                &copied.scene_id, copied.folder_snapshot.as_ref());
            // 复制只登记并选中副本，当前作者文档、视口和草稿继续由原会话持有。
            editor.new_scene_dialog_open = false;
            editor.new_scene_copy_target = None;
            editor.new_scene_name.clear();
            assets.requested_revision = assets.requested_revision.wrapping_add(1);
            editor.status = format!("场景“{}”已复制，可在场景列表打开副本", copied.display_name);
            if let Some(target) = task.feedback_target.take() {
                editor.record_operation_result(EditorStatusOperation::SceneCopy, &target, EditorStatusKind::Success);
            }
        }
        Err(error) => {
            let (operation, kind) = if let Some(control) = copy {
                if control.is_cancel_requested() {
                    editor.new_scene_dialog_open = false;
                    editor.new_scene_copy_target = None;
                    editor.new_scene_name.clear();
                }
                editor.status = format!("复制场景未完成：{error}");
                (EditorStatusOperation::SceneCopy, if control.is_cancel_requested() {
                    EditorStatusKind::Warning
                } else {
                    EditorStatusKind::Error
                })
            } else {
                editor.status = format!("新建场景失败：{error}");
                (EditorStatusOperation::SceneCreate, EditorStatusKind::Error)
            };
            if let Some(target) = task.feedback_target.take() {
                editor.record_operation_result(operation, &target, kind);
            }
            error!("{}", editor.status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_directory_names_preserve_unicode_and_reject_reserved_components() {
        assert_eq!(scene_directory_name("   秋風/河谷: Forêt #01. . ").unwrap(), "秋風河谷 Forêt 01");
        for name in ["..", " /:*? ", ".TRASH.", "con.txt", "NuL .文件", "COM¹.目录", "lpt9"] {
            assert!(scene_directory_name(name).is_err(), "{name}");
        }
        assert_eq!(scene_directory_name("Combate ２号").unwrap(), "Combate ２号");
    }

    #[test]
    fn new_scene_uses_named_directory_and_keeps_registered_id_and_existing_files() {
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
        }
        let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("feathrise-scene-named-{}-{time}", std::process::id()));
        std::fs::create_dir(&root).unwrap(); // 只清理本例独占创建的目录。
        let fixture = Fixture(root);
        let root = &fixture.0;
        std::fs::create_dir(root.join("scenes")).unwrap();
        let catalog_path = root.join(SCENE_CATALOG_ASSET_PATH);
        atomic_write_json(&catalog_path, &serde_json::json!({
            "schema_version": SCENE_SCHEMA_VERSION,
            "scenes": [{"id":"SCENE-legacy","display_name":"旧場景","folder":"scenes/SCENE-legacy","published":true}]
        })).unwrap();
        let original = std::fs::read(&catalog_path).unwrap();
        let existing = root.join("scenes/既有目录");
        std::fs::create_dir(&existing).unwrap();
        std::fs::write(existing.join("保留.txt"), b"untouched").unwrap();
        let create = |name| create_scene(root, validated_creation_request(
            name, "", "", "", NewSceneTerrainMode::Empty,
        ).unwrap(), PreparedTerrain::Empty);
        assert!(create("既有目录").is_err());
        assert!(create("旧場景").is_err());
        assert!(create("SCENE-legacy").is_err());
        assert_eq!(std::fs::read(existing.join("保留.txt")).unwrap(), b"untouched");
        assert_eq!(std::fs::read(&catalog_path).unwrap(), original);

        let scene = create("新/場景 Réserve*.").unwrap();
        assert_eq!(scene.display_name, "新/場景 Réserve*.");
        assert_eq!(scene.folder, "scenes/新場景 Réserve");
        let bytes = std::fs::read(&catalog_path).unwrap();
        assert!(create("新場景 Réserve").is_err(), "清理后同目录也不能覆盖");
        assert_eq!(std::fs::read(&catalog_path).unwrap(), bytes);
        let catalog = decode_scene_catalog(&catalog_path, &bytes).unwrap();
        assert_eq!(catalog.scenes[0].folder, "scenes/SCENE-legacy");
        let entry = &catalog.scenes[1];
        assert_eq!(entry.id, scene.scene_id);
        assert_eq!(entry.folder, scene.folder);
        assert!(!root.join("scenes").join(scene.scene_id.as_str()).exists());
        let manifest: SceneManifestAsset = serde_json::from_slice(
            &std::fs::read(root.join(entry.manifest_path())).unwrap(),
        ).unwrap();
        assert_eq!(manifest.id, scene.scene_id);
        assert_eq!(manifest.environment.source, feathrise_scene_content::SceneEnvironmentSource::InheritDefault);
        assert_eq!(manifest.asset_roots[0], entry.folder);
        let path = entry.placements_path();
        let glb = std::fs::read(root.join(&path)).unwrap();
        assert_eq!(feathrise_scene_content::read_scene_placements(&glb, &path).unwrap().scene_id, scene.scene_id);
    }

    #[test] // 原始GLB与Clipmap共享空白画布，不因地形路线隐式引用全局地表。
    fn both_new_scene_routes_start_with_unpainted_layerless_surface() {
        let scene_id = SceneId::parse("MAP-empty-surface-test").unwrap(); // 两种路线都复用同一场景地表契约。
        for terrain_mode in [
            NewSceneTerrainMode::SparseClipmap,     // 稀疏Clipmap路线。
        ] {
            let request =
                validated_creation_request("空白场景", "256", "256", "2", terrain_mode).unwrap(); // 两种入口都无需预设目录。
            assert_eq!(request.terrain_mode, terrain_mode); // 冻结本次明确选择的地形路线。
            let surface = new_scene_surface_asset(&scene_id, "scenes/MAP-empty-surface-test"); // 复用生产创建事务的地表定义。
            assert!(surface.layers.is_empty()); // 地表只能由作者显式添加。
            let (indices, weights) = empty_new_scene_surface_sources(); // 复用生产写盘前的原始作者数据。
            assert!(indices.iter().all(|value| *value == 0)); // 不包含隐藏预设索引。
            assert!(weights.iter().all(|value| *value == 0)); // 不会覆盖白模或制造已绘制地皮。
            encode_surface_maps(&indices, &weights, surface.resolution, 0).unwrap(); // 全零作者源仍可生成成对Mip纹理。
        }
    }

    #[test] // 锁定创建事务实际使用的入口构造器。
    fn clipmap_and_legacy_creation_register_empty_vegetation_and_water() {
        // 不启动真实场景创建或修改用户目录。
        let scene_id = SceneId::parse("MAP-vegetation-entry").unwrap(); // 固定有效测试身份。
        for mode in [
            NewSceneTerrainMode::SparseClipmap,
        ] {
            // 两路线必须具有同样Blade作者入口。
            let entry = new_scene_catalog_entry(&scene_id, "空草地", "scenes/空草地", mode);
            assert_eq!(
                entry.vegetation_path.as_deref(),
                Some("scenes/空草地/vegetation.scene_vegetation.json")
            ); // 路径严格指向已写入的空JSON。
            assert_eq!(
                entry.water_path.as_deref(),
                Some("scenes/空草地/water.scene_water.json")
            ); // 空水域JSON与目录入口同批发布。
        } // 不需要任何整图高度数组。
    } // 默认Clipmap路线不会再次出现无植被入口。

    #[test]
    fn new_scene_vegetation_starts_without_formal_outputs_or_author_layers() {
        let scene_id = SceneId::parse("MAP-empty-authoring-test").expect("测试SceneId合法"); // 模拟尚未发布的新场景身份。
        let vegetation = default_new_scene_vegetation(&scene_id, "scenes/MAP-empty-authoring-test"); // 调用创建事务实际使用的纯构造器。
        let authoring = vegetation
            .authoring
            .as_ref()
            .expect("新场景显式保存作者结构"); // `None`会被旧图迁移误判。

        assert!(vegetation.layers.is_empty()); // 预设按钮来自全局目录，不要求场景预登记固定槽位。
        assert!(authoring.layers.is_empty()); // 场景目录首次打开不得显示任何预设图层。
        assert_eq!(authoring.groups.len(), 1); // 第一张新层无需作者先手工创建组。
        authoring
            .validate(&vegetation.layers)
            .expect("零正式输出与零作者层的新场景植被文件必须通过正式校验"); // 保存和编译路径接受空列表。
    }

    #[test]
    fn new_scene_water_starts_without_static_or_spline_objects() {
        let scene_id = SceneId::parse("MAP-empty-water-test").expect("测试SceneId合法");
        let water = default_water_asset(&scene_id); // 使用创建事务实际写入JSON的纯构造器。

        assert!(water.layers.is_empty()); // 作者未操作前列表默认为零个静水体。
        assert!(water.rivers.is_empty()); // 新场景不得凭空形成河流样条。
        assert!(water.waterfalls.is_empty()); // 新场景不得凭空形成瀑布连接。
        water
            .validate(&scene_id)
            .expect("零对象水域文件必须可保存和加载");
    }


    #[test]
    fn dirty_scene_confirmation_queues_save_then_creation_snapshot() {
        let mut editor = SceneEditorState {
            dirty: true,
            new_scene_name: "  第二个场景  ".to_owned(),
            new_scene_notes: "入口村落\n待完善河岸".to_owned(), // 保存等待期间也必须冻结多行备注。
            ..default()
        };
        let mut task = SceneCreationTask::default();
        let browser = SceneBrowserState {
            write_blocked: None, // 模拟归类资产已经完成缺失或有效首载。
            ..default()
        };
        request_scene_creation(
            &mut editor,
            &mut task,
            &browser,
            &SceneFolderPersistence::default(),
            false,
        );
        assert!(editor.save_requested); // 一次点击必须主动推进保存，而不是静默拒绝。
        assert!(task.task.is_none()); // 当前草稿尚未保存时不能提前切换场景。
        assert_eq!(
            task.pending_after_save,
            Some(SceneCreationRequest {
                collision_lods: Vec::new(), // GLB确认快照不消费稀疏碰撞列表。
                display_lods: None,         // GLB确认快照不携带SparseClipmap显示配置。
                display_name: "第二个场景".to_owned(),
                notes: "入口村落\n待完善河岸".to_owned(), // 当前未保存场景不会吞掉本次备注输入。
                width_meters: 1.0,
                length_meters: 1.0,
                terrain_mode: NewSceneTerrainMode::Empty, // 新建只冻结空容器与文本资料。
                terrain_cell_meters: 2.0, // 弹框默认作者格距与待保存的确认请求一起冻结。
                folder_id: None,
                folder_source_bytes: None,
            })
        ); // 后台保存期间作者输入变化不得改变原请求。
    }

    #[test]
    fn scene_creation_ignores_stale_geometry_and_never_creates_scene_level_lods() {
        let mut editor = SceneEditorState {
            dirty: true,
            new_scene_name: "空容器场景".into(),
            ..default()
        };
        let mut task = SceneCreationTask::default();
        request_scene_creation(&mut editor, &mut task, &SceneBrowserState {
            write_blocked: None, ..default()
        }, &SceneFolderPersistence::default(), false);
        let request = task.pending_after_save.as_ref().expect("先保存当前场景，再创建空容器");
        assert_eq!(request.terrain_mode, NewSceneTerrainMode::Empty);
        assert!(request.collision_lods.is_empty());
        assert!(request.display_lods.is_none());
        assert_eq!((request.width_meters, request.length_meters), (1.0, 1.0));
    }
}
