//! 新地图后台创建事务与主线程采用系统。

use std::path::Path; // 私有目录和目录JSON路径拼接。
use std::time::{SystemTime, UNIX_EPOCH}; // 无额外依赖生成稳定ASCII草稿ID。

use super::creation_input::{MapDimensions, normalize_display_name, validated_creation_request}; // 旧双层预算已由唯一多层规划器替代。
use super::files::{
    atomic_write, atomic_write_json, decode_map_catalog, workspace_asset_root, workspace_maps_root,
};
use super::ktx2::encode_surface_maps; // 新地图一次预生成严格匹配的地表索引和权重 Mip。
use super::vegetation_defaults::empty_vegetation_authoring; // 新地图的正式输出和作者画布都从空白开始。
use super::water_defaults::default_water_asset;
use crate::constants::MAP_FOLDER_ASSET_PATH; // 创建归入虚拟文件夹后显式刷新编辑器专用资产。
use crate::map_browser::prepare_map_assignment_mutation; // 新地图归属与catalog入口组成联合发布事务。
use crate::map_browser::{expand_folder_ancestors, selected_container_folder_id}; // 树选择决定创建归属并在成功后展开完整父链。
use crate::resources::{
    EditorAssetCatalog, MapBrowserState, MapCreationTask, MapEditorState, MapFolderPersistence,
};
use crate::types::{CreatedMap, MapBrowserItem, MapCreationRequest, NewMapTerrainMode};
use crate::types::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget};
use bevy::prelude::*; // ECS资源与日志。
use bevy::tasks::futures::check_ready; // Update只非阻塞探测任务结果。
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool}; // 完整GLB编码走计算池，两条路线的目录事务都走IO池。
use feathrise_map_content::{
    MAP_BOUNDARIES_FILE_NAME, MAP_CATALOG_ASSET_PATH, MAP_MANIFEST_FILE_NAME,
    MAP_PLACEMENTS_FILE_NAME, MAP_PORTALS_FILE_NAME, MAP_SCHEMA_VERSION, MAP_SURFACE_FILE_NAME,
    MAP_SURFACE_SCHEMA_VERSION, MAP_VEGETATION_FILE_NAME, MAP_WATER_FILE_NAME, MapBoundariesAsset,
    MapCatalogEntry, MapClipmapCollisionMode, MapEnvironmentDescriptor, MapId, MapManifestAsset,
    MapPlacementsAsset, MapPortalsAsset, MapSurfaceAsset, MapTerrainDescriptor, MapVegetationAsset,
    SparseHeightDocument, SparseHeightSpec, sparse_height_manifest_asset_path,
}; // 新建文件严格复用运行时资产契约。 // 后台任务只返回轻量成功身份。
use feathrise_map_terrain::{TerrainSampleSpec, build_flat_terrain_glb}; // 原始路线继续复用正式平面GLB编码器。

pub(crate) const DEFAULT_HEIGHT_SCALE: f32 = 160.0; // 显示LOD新建回显与最终后台创建必须使用同一高度规格。
pub(crate) const DEFAULT_BASE_HEIGHT: f32 = -80.0; // 平地位于量程中点，可同时向上造山和向下开河。
const DEFAULT_SURFACE_RESOLUTION: [u32; 2] = [512, 512]; // 固定作者纹理预算；世界尺寸只改变每纹素覆盖米数。

/// 新地图先保留可编辑的空白地表文件，作者手动新建层后才引用预设。
pub(crate) fn new_map_surface_asset(map_id: &MapId, folder_relative: &str) -> MapSurfaceAsset {
    MapSurfaceAsset {
        schema_version: MAP_SURFACE_SCHEMA_VERSION, // 继续使用正式地表文件版本。
        map_id: map_id.clone(),                     // 作者层所属地图身份。
        weight_source_path: format!("{folder_relative}/surface/weights.rgba8"), // 全零作者权重源。
        weight_texture_path: format!("{folder_relative}/surface/weights.ktx2"), // 成对运行时权重纹理。
        index_source_path: format!("{folder_relative}/surface/indices.rgba32"), // 全零作者索引源。
        index_texture_path: format!("{folder_relative}/surface/indices.ktx2"), // 成对运行时索引纹理。
        resolution: DEFAULT_SURFACE_RESOLUTION, // 两条地形路线共用画布规格。
        layers: Vec::new(),                     // 未显式新建作者层之前保持白模。
    }
}

/// 保留可手动绘制的完整尺寸画布，但新图不产生任何隐含材质贡献。
fn empty_new_map_surface_sources() -> (Vec<u8>, Vec<u8>) {
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
    /// 原始路线在计算池完整编码后才允许开始发布目录。
    LegacyGlb(Vec<u8>),
    /// Clipmap 空白图无需预分配任何整图顶点或高度数组。
    SparseClipmap,
}

/// 构造新地图的植被文件：目录预设只在作者首次使用时延迟写入。
fn default_new_map_vegetation(
    map_id: &MapId,         // JSON所属稳定地图身份。
    _folder_relative: &str, // 保留纯构造器接口，实际路径由首次使用的预设生成。
) -> MapVegetationAsset {
    MapVegetationAsset {
        schema_version: MAP_SCHEMA_VERSION, // 使用当前数据驱动地图格式。
        map_id: map_id.clone(),             // 资产身份必须与私有目录一致。
        layers: Vec::new(), // 未使用的第 1、100 或 1000 个目录预设都不占地图引用与磁盘文件。
        authoring: Some(empty_vegetation_authoring()), // 显式空结构阻止旧图迁移逻辑猜测内容。
    }
}

/// 生成不依赖中文转写、可用于目录与传送引用的稳定ASCII ID。
fn next_map_id(maps_root: &Path, unix_millis: u128) -> Result<MapId, String> {
    let base = format!("MAP-{unix_millis}");
    for suffix in 0..10_000u32 {
        let value = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        if !maps_root.join(&value).exists() {
            return MapId::parse(value);
        }
    }
    Err("同一时间生成的地图ID已超过上限".to_owned())
}

/// 按受检目标格距生成原始路线的零高度完整 GLB。
fn build_new_map_terrain_glb(dimensions: MapDimensions) -> Result<Vec<u8>, String> {
    build_flat_terrain_glb(
        TerrainSampleSpec {
            resolution: dimensions.terrain_mesh_resolution, // 真正写入GLB的X/Z顶点数来自作者格距。
            world_min: dimensions.world_min,                // 网格严格覆盖地图完整X/Z边界。
            world_max: dimensions.world_max,                // 矩形地图各轴独立计算单元数。
            base_height: DEFAULT_BASE_HEIGHT,
            height_scale: DEFAULT_HEIGHT_SCALE,
        },
        0.0, // 新地图全部顶点从世界Y=0平面开始。
    )
}

/// 由冻结路线构造唯一地形作者源描述；同一清单不能同时登记两种可写源。
fn new_map_terrain_descriptor(
    terrain_mode: NewMapTerrainMode,
    map_id: &MapId,
    folder_relative: &str,
    resolution: [u32; 2],
) -> MapTerrainDescriptor {
    if terrain_mode == NewMapTerrainMode::Empty { return MapTerrainDescriptor::empty(); }
    // 创建只冻结作者路线与唯一碰撞列表。
    let sparse = terrain_mode == NewMapTerrainMode::SparseClipmap; // GLB数据不变。
    MapTerrainDescriptor {resolved_stitches: Default::default(),
        kind: terrain_mode.terrain_kind(), // 保持用户选择的地形作者类型。
        collision_mode: MapClipmapCollisionMode::LayeredSharedChunks, // 兼容读标记，序列化不再输出。
        collision_lods: if sparse {
            feathrise_map_content::default_collision_lods_for_resolution(resolution)
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
        height_path: sparse.then(|| sparse_height_manifest_asset_path(map_id)), // 稀疏高度源路径不改。
        height_texture_path: None,          // 运行时仍使用现有显示派生。
        resolution,                         // 保持作者格点数。
        base_height: DEFAULT_BASE_HEIGHT,   // 高度量程保持不变。
        height_scale: DEFAULT_HEIGHT_SCALE, // 碰撞配置不改变作者精度。
    } // 完整请求稍后覆盖用户列表。
}

/// 新地图目录与作者文件同批发布；纯构造器便于同时验证两条路线的真实注册入口。
fn new_map_catalog_entry(
    map_id: &MapId,
    display_name: &str,
    terrain_mode: NewMapTerrainMode,
) -> MapCatalogEntry {
    // 不读取磁盘，不分配任何密度画布。
    MapCatalogEntry {
        // 只登记创建事务已经完整写入的内容。
        id: map_id.clone(),                    // 稳定地图身份。
        display_name: display_name.to_owned(), // 已通过创建请求名称校验。
        folder: format!("maps/{map_id}"),      // 唯一私有目录根。
        published: true,                       // 完整创建后公开地图。
        surface_path: (terrain_mode != NewMapTerrainMode::Empty).then(|| format!("maps/{map_id}/{MAP_SURFACE_FILE_NAME}")), // 空图不创建无地形可承载的画布。
        vegetation_path: Some(format!("maps/{map_id}/{MAP_VEGETATION_FILE_NAME}")), // Clipmap使用分页高度Blade后端，不能再按GLB路线过滤入口。
        water_path: Some(format!("maps/{map_id}/{MAP_WATER_FILE_NAME}")), // 两条动态地形路线都登记已经同批写入的空静水作者文件。
        characters_path: None, // 旧格式入口保持为空；外部作者首次使用时自行登记对应文件。
        boundary_path: (terrain_mode != NewMapTerrainMode::Empty).then(|| format!("maps/{map_id}/{MAP_BOUNDARIES_FILE_NAME}")), // 空图没有隐含安全墙。
    } // 目录是创建事务最后一个可见目标。
} // 不为新图预建任何植被正式层。

/// 在IO线程写入所选地形作者源和公共私有文件，最后才原子发布目录入口。
fn create_map(
    request: MapCreationRequest,
    prepared_terrain: PreparedTerrain,
) -> Result<CreatedMap, String> {
    let display_name = normalize_display_name(&request.display_name)?;
    let dimensions = MapDimensions::for_mode(
        request.terrain_mode,
        request.width_meters,        // 后台重验冻结的地图X尺寸。
        request.length_meters,       // 后台重验冻结的地图Z尺寸。
        request.terrain_cell_meters, // 后台按冻结路线重验作者格距与容量，不能只信UI。
    )?;
    let asset_root = workspace_asset_root();
    let _content_author_lock = feathrise_map_content::ContentAuthorLock::acquire(&asset_root)?; // 地图目录发布与已有领域作者事务继续使用同一写锁，避免并发丢登记。
    let maps_root = workspace_maps_root();
    let catalog_path = asset_root.join(MAP_CATALOG_ASSET_PATH);
    let original_catalog_bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取地图目录{}失败：{error}", catalog_path.display()))?; // 保留原字节用于并发检测。
    let mut catalog = decode_map_catalog(&catalog_path, &original_catalog_bytes)?; // 与删除事务复用同一目录损坏和版本保护门。
    let unix_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间早于Unix纪元：{error}"))?
        .as_millis();
    let map_id = next_map_id(&maps_root, unix_millis)?;
    if catalog.maps.iter().any(|entry| entry.id == map_id) {
        return Err(format!("地图目录已存在ID {map_id}"));
    }

    let folder_relative = format!("maps/{map_id}");
    let folder = asset_root.join(&folder_relative);
    let terrain_folder = folder.join("terrain");
    if folder.exists() {
        return Err(format!("地图私有目录已存在：{}", folder.display()));
    }
    std::fs::create_dir_all(&terrain_folder)
        .map_err(|error| format!("创建地图目录{}失败：{error}", terrain_folder.display()))?;

    let result = (|| {
        let mesh_resolution = dimensions.terrain_mesh_resolution; // 原始路线表示GLB顶点，Clipmap路线表示作者逻辑格点。
        let height_resolution = dimensions.terrain_height_resolution; // 两条路线使用各自经过校验的清单规格。
        match (request.terrain_mode, prepared_terrain) {
            (NewMapTerrainMode::Empty, PreparedTerrain::Empty) => {},
            (NewMapTerrainMode::LegacyHeightfield, PreparedTerrain::LegacyGlb(glb)) => {
                atomic_write(&terrain_folder.join("terrain.glb"), &glb)?; // 完整编码成功后才把旧路线作者源写入私有目录。
            }
            (NewMapTerrainMode::SparseClipmap, PreparedTerrain::SparseClipmap) => {
                let sparse_spec = SparseHeightSpec::new(
                    dimensions.world_min,
                    dimensions.world_max,
                    request.terrain_cell_meters,
                    DEFAULT_BASE_HEIGHT,
                    DEFAULT_HEIGHT_SCALE,
                )?; // 只计算标量元数据，不生成最大地图的整图网格。
                let mut sparse_height =
                    SparseHeightDocument::create_empty(&asset_root, map_id.clone(), sparse_spec)?; // 独立编辑器可复用同一稀疏作者数据入口。
                sparse_height.save_dirty()?; // 先发布空白稀疏清单，最后才发布地图目录入口。
            }
            _ => return Err("新建地图地形方案与后台准备产物不一致".to_owned()),
        }

        let mut manifest = MapManifestAsset {
            vegetation_paint_groups: Default::default(),
            terrains: (request.terrain_mode == NewMapTerrainMode::Empty).then(Default::default),
            sub_maps: Default::default(), // 新图没有子地图引用，添加操作单独进入作者历史。
            schema_version: MAP_SCHEMA_VERSION,
            id: map_id.clone(),
            display_name: display_name.clone(),
            notes: request.notes.clone(), // 与名称一起冻结并写入私有清单，不进入全局目录索引。
            runtime_scene: feathrise_map_content::MapRuntimeKind::DataDriven.as_str().to_owned(), // 新图不依赖原游戏枚举名称。
            world_min: dimensions.world_min,
            world_max: dimensions.world_max,
            default_spawn: [0.0, 2.0, 0.0],
            environment: MapEnvironmentDescriptor::default(), // 新地图先使用中性清屏色，作者在F12明确选择立方体后才增加依赖。
            puddles: None, // 新图及旧图夹具默认没有积水，首次有效落笔才创建。
            terrain: new_map_terrain_descriptor(
                request.terrain_mode,
                &map_id,
                &folder_relative,
                height_resolution,
            ), // 清单永久冻结本次创建选择，现有地图不会随默认项改变。
            asset_roots: vec![
                folder_relative.clone(),
                "gltf".to_owned(),
                "textures".to_owned(),
            ],
        };
        if request.terrain_mode == NewMapTerrainMode::SparseClipmap {
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
            feathrise_map_content::compile_display_lod_plan(&spec, display_lods)?; // 后台再次执行量化、Mesh和最坏页预算预检。
            manifest.terrain.display_lods = Some(display_lods.clone()); // 与清单和高度规格同批原子发布。
        } // 不给旧地图补写新格式。
        atomic_write_json(&folder.join(MAP_MANIFEST_FILE_NAME), &manifest)?;
        atomic_write_json(
            &folder.join(MAP_PORTALS_FILE_NAME),
            &MapPortalsAsset {
                schema_version: MAP_SCHEMA_VERSION,
                map_id: map_id.clone(),
                portals: Vec::new(),
            },
        )?;
        atomic_write_json(
            &folder.join(MAP_PLACEMENTS_FILE_NAME),
            &MapPlacementsAsset {
                schema_version: MAP_SCHEMA_VERSION,
                map_id: map_id.clone(),
                placements: Vec::new(),
            },
        )?;
        if request.terrain_mode != NewMapTerrainMode::Empty {
        let surface_folder = folder.join("surface");
        let (surface_indices, surface_weights) = empty_new_map_surface_sources(); // 新地图不自动涂绘，空层全零权重着色为白模。
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
            &folder.join(MAP_SURFACE_FILE_NAME), // 白模也要登记可继续编辑的地表文件。
            &new_map_surface_asset(&map_id, &folder_relative), // 仅显式新建层后才加入预设引用。
        )?;
        }
        let vegetation = default_new_map_vegetation(&map_id, &folder_relative); // 零输出文件只写轻量 JSON，不预分配任何预设密度。
        atomic_write_json(
            &folder.join(MAP_VEGETATION_FILE_NAME),
            &vegetation, // 目录首次热加载直接得到零作者层，不依赖随后修正。
        )?;
        let water = default_water_asset(&map_id); // 空水域JSON先登记；首张R8/KTX2只在作者显式新建后产生。
        atomic_write_json(&folder.join(MAP_WATER_FILE_NAME), &water)?;
        if request.terrain_mode != NewMapTerrainMode::Empty {
        let boundaries = MapBoundariesAsset::rectangular_default(
            &map_id,              // 新边界文件必须属于本次唯一地图ID。
            dimensions.world_min, // 四面安全墙使用新地图实际X/Z最小角。
            dimensions.world_max, // 非方形地图分别沿东西宽度与南北长度生成。
            DEFAULT_BASE_HEIGHT,  // 墙底覆盖高度场最低可雕刻位置以下。
            DEFAULT_HEIGHT_SCALE, // 墙顶覆盖完整可雕刻高度和额外跳跃余量。
        )?;
        atomic_write_json(&folder.join(MAP_BOUNDARIES_FILE_NAME), &boundaries)?; // 边界私有文件完整写入后才发布目录入口。
        }

        let folder_mutation = prepare_map_assignment_mutation(
            &asset_root,
            request.folder_source_bytes.clone(),
            &map_id,
            request.folder_id.as_deref(),
        )?; // 当前虚拟文件夹在点击确认时冻结，根目录新地图无需改写归类文件。

        // 全部私有文件成功后才联合发布归属和catalog入口，AssetServer永远不会发现半成品地图。
        catalog.maps.push(new_map_catalog_entry(
            &map_id,
            &display_name,
            request.terrain_mode,
        )); // 私有文件全部完成后，两条路线均公开Blade植被入口。
        let latest_catalog_bytes = std::fs::read(&catalog_path)
            .map_err(|error| format!("再次读取地图目录{}失败：{error}", catalog_path.display()))?; // 发布前检测外部并发修改。
        if latest_catalog_bytes != original_catalog_bytes {
            return Err("地图目录在创建检查期间发生变化，请重试".to_owned()); // 禁止覆盖同时发生的删除或外部编辑。
        }
        if let Some(mutation) = folder_mutation.as_ref() {
            mutation.publish_checked()?; // 先发布地图归属，catalog入口随后才对运行时可见。
        }
        if let Err(catalog_error) = atomic_write_json(&catalog_path, &catalog) {
            let rollback = folder_mutation.as_ref().map(|mutation| mutation.rollback()); // catalog失败必须恢复原归类文件。
            return match rollback {
                Some(Err(rollback_error)) => Err(format!(
                    "发布地图目录失败：{catalog_error}；恢复地图归类也失败：{rollback_error}"
                )),
                _ => Err(format!("发布地图目录失败：{catalog_error}")),
            };
        }
        let folder_snapshot = folder_mutation.as_ref().map(|mutation| mutation.snapshot()); // 主世界直接采用联合事务成功值。
        Ok(CreatedMap {
            map_id,
            display_name,
            width_meters: dimensions.width_meters,
            length_meters: dimensions.length_meters,
            terrain_mode: request.terrain_mode,
            terrain_mesh_resolution: mesh_resolution, // 完成回执按路线解释为GLB顶点或作者逻辑格点。
            folder_snapshot,
        })
    })();
    if result.is_err() {
        // 新目录尚未进入全局目录，失败时可安全清除本次唯一目标；既有地图不受影响。
        let _ = std::fs::remove_dir_all(&folder);
    }
    result
}

/// 启动一份已经校验的创建快照；调用方保证同一时间没有其他在途事务。
fn start_map_creation(
    editor: &mut MapEditorState,
    task: &mut MapCreationTask,
    request: MapCreationRequest,
) {
    let dimensions = MapDimensions::for_mode(
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
    task.task = Some(IoTaskPool::get().spawn(async move {
        let prepared = match terrain_mode {
            NewMapTerrainMode::Empty => Ok(PreparedTerrain::Empty),
            NewMapTerrainMode::LegacyHeightfield => AsyncComputeTaskPool::get()
                .spawn(async move { build_new_map_terrain_glb(dimensions) })
                .await
                .map(PreparedTerrain::LegacyGlb), // 大型完整网格只占计算池，不阻塞主线程或IO worker。
            NewMapTerrainMode::SparseClipmap => Ok(PreparedTerrain::SparseClipmap),
        };
        (
            ticket,
            prepared.and_then(|terrain| create_map(request, terrain)),
        )
    })); // 同一任务槽和票据覆盖可选计算、IO及发布阶段，主线程始终只非阻塞轮询。
    let grid_label = match terrain_mode {
        NewMapTerrainMode::Empty => "无地形格点",
        NewMapTerrainMode::LegacyHeightfield => "完整GLB顶点",
        NewMapTerrainMode::SparseClipmap => "稀疏作者格点",
    };
    editor.status = format!(
        "正在后台生成地图“{display_name}”：{width_meters}×{length_meters}米，{}，{grid_label}{}×{}",
        terrain_mode.label(),
        dimensions.terrain_mesh_resolution[0],
        dimensions.terrain_mesh_resolution[1],
    ); // 作者在生成期间可核对已经冻结的路线和实际规格。
}

/// 前台拒绝只形成需处理回执，不启动IO、不修改保存等待或创建票据。
fn record_creation_warning(editor: &mut MapEditorState) {
    let name = editor.new_map_name.trim();
    let target = EditorStatusTarget::new(name, name);
    editor.record_operation_result(
        EditorStatusOperation::MapCreate,
        &target,
        EditorStatusKind::Warning,
    );
}

/// 从UI请求一次创建；多文档入口保留源草稿，独立旧入口继续先保存。
pub(crate) fn request_map_creation(
    editor: &mut MapEditorState,
    task: &mut MapCreationTask,
    browser: &MapBrowserState,
    persistence: &MapFolderPersistence,
    preserve_document: bool,
) {
    if task.task.is_some() || task.pending_after_save.is_some() {
        editor.status = "已有地图创建请求正在执行".to_owned();
        record_creation_warning(editor);
        return;
    }
    if persistence.busy() {
        editor.status = "地图归类保存完成前不能新建地图".to_owned();
        record_creation_warning(editor);
        return; // 创建任务必须冻结已落盘的归类字节基线。
    }
    if let Some(reason) = browser.write_blocked.as_deref() {
        editor.status = format!("地图归类尚未就绪，不能新建地图：{reason}");
        record_creation_warning(editor);
        return; // 首载或损坏时不能误把已选归类上下文猜成根。
    }
    let mut request = match validated_creation_request(
        &editor.new_map_name,
        "",
        "",
        "",
        NewMapTerrainMode::Empty, // 地图创建只生成容器，网格规格和LOD由添加网格事务创建。
    ) {
        Ok(value) => value,
        Err(error) => {
            editor.status = error; // 弹框内和底栏同时显示精确字段错误。
            record_creation_warning(editor);
            return;
        }
    };
    request.folder_id =
        selected_container_folder_id(&browser.document, browser.selected_item.as_ref()); // 目录选择归入该目录，地图选择沿用其父级，根或空选择归入根。
    request.notes.clone_from(&editor.new_map_notes); // 保存当前地图期间冻结备注，后续输入不能改写本次创建。
    request.folder_source_bytes = browser.source_bytes.clone(); // 后台发布前使用精确原字节检测并发。
    if editor.dirty && !preserve_document {
        let display_name = request.display_name.clone();
        task.pending_after_save = Some(request); // 冻结输入，保存期间后续键盘事件不能改变目标。
        editor.save_requested = true; // 复用既有原子保存、派生资源和失败处理，不另写旁路。
        editor.status =
            format!("当前地图有未保存修改；正在先保存当前地图，成功后继续创建“{display_name}”");
        return;
    }
    start_map_creation(editor, task, request);
}

/// 非阻塞采用新地图结果；显式刷新目录，避免连续原子替换被文件监听合并。
pub(crate) fn poll_map_creation(
    mut task: ResMut<MapCreationTask>,
    mut editor: ResMut<MapEditorState>,
    mut assets: ResMut<EditorAssetCatalog>,
    mut browser: ResMut<MapBrowserState>,
    asset_server: Res<AssetServer>,
) {
    if task.task.is_none()
        && !editor.dirty
        && let Some(request) = task.pending_after_save.take()
    {
        start_map_creation(&mut editor, &mut task, request); // 保存成功后的下一帧继续原确认请求。
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
    match result {
        Ok(created) => {
            asset_server.reload(MAP_CATALOG_ASSET_PATH); // 同进程连续创建不只依赖文件监听时序。
            if let Some(snapshot) = created.folder_snapshot.as_ref() {
                browser.document = snapshot.document.clone(); // 联合事务成功值直接成为内存权威。
                browser.source_bytes = Some(snapshot.source_bytes.clone()); // 更新下一次精确并发基线。
                browser.initial_load_resolved = true; // 联合事务成功值已经是明确有效归类资产。
                browser.document_revision = browser.document_revision.wrapping_add(1); // 重建完整虚拟树索引。
                browser.write_blocked = None; // 成功事务保持结构写入开放。
                browser.last_error = None; // 清除旧瞬时诊断。
                asset_server.reload(MAP_FOLDER_ASSET_PATH); // 不依赖文件监听合并时序。
            }
            let created_folder_id = browser
                .document
                .map_folders
                .get(created.map_id.as_str())
                .cloned(); // 映射缺失即根目录；联合事务成功后可直接从采用文档判断可见性。
            let expanded_changed = {
                let browser = &mut *browser; // 把ResMut显式还原为普通结构体借用，允许同时访问互斥字段。
                expand_folder_ancestors(
                    &browser.document,
                    created_folder_id.as_deref(),
                    &mut browser.expanded_folder_ids,
                )
            };
            if expanded_changed {
                browser.expansion_revision = browser.expansion_revision.wrapping_add(1); // 新地图所在父链完整展开后再公开高亮。
            }
            browser.select_only(Some(MapBrowserItem::Map(created.map_id.clone()))); // 创建结果成为独立焦点，不混入旧整理选集。
            editor.map_list_selection_id = Some(created.map_id.clone()); // 进入与删除按钮目标继续使用同一新地图身份。
            let grid_label = match created.terrain_mode {
                NewMapTerrainMode::Empty => "无地形格点",
                NewMapTerrainMode::LegacyHeightfield => "完整GLB顶点",
                NewMapTerrainMode::SparseClipmap => "稀疏作者格点",
            };
            editor.queue_map_transition(
                created.map_id.clone(),
                format!(
                    "地图{} [{}]已创建：{}×{}米，{}，{grid_label}{}×{}；正在等待目录热加载并自动进入",
                    created.display_name,
                    created.map_id,
                    created.width_meters,
                    created.length_meters,
                    created.terrain_mode.label(),
                    created.terrain_mesh_resolution[0],
                    created.terrain_mesh_resolution[1],
                ),
            ); // 创建结果不能只停留在选中态，必须登记等待清单就绪的正式切图请求。
            editor.new_map_dialog_open = false;
            editor.new_map_name.clear();
            assets.current_folder = format!("maps/{}", created.map_id);
            assets.requested_revision = assets.requested_revision.wrapping_add(1);
            if let Some(target) = task.feedback_target.take() {
                editor.record_operation_result(
                    EditorStatusOperation::MapCreate,
                    &target,
                    EditorStatusKind::Success,
                ); // 完整目录已发布；原自动进入仍由切图任务表示，不能冒充场景已就绪。
            }
            info!("{}", editor.status);
        }
        Err(error) => {
            editor.status = format!("新建地图失败：{error}");
            if let Some(target) = task.feedback_target.take() {
                editor.record_operation_result(
                    EditorStatusOperation::MapCreate,
                    &target,
                    EditorStatusKind::Error,
                );
            }
            error!("{}", editor.status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // 原始GLB与Clipmap共享空白画布，不因地形路线隐式引用全局地表。
    fn both_new_map_routes_start_with_unpainted_layerless_surface() {
        let map_id = MapId::parse("MAP-empty-surface-test").unwrap(); // 两种路线都复用同一地图地表契约。
        for terrain_mode in [
            NewMapTerrainMode::LegacyHeightfield, // 原始GLB路线。
            NewMapTerrainMode::SparseClipmap,     // 稀疏Clipmap路线。
        ] {
            let request =
                validated_creation_request("空白地图", "256", "256", "2", terrain_mode).unwrap(); // 两种入口都无需预设目录。
            assert_eq!(request.terrain_mode, terrain_mode); // 冻结本次明确选择的地形路线。
            let surface = new_map_surface_asset(&map_id, "maps/MAP-empty-surface-test"); // 复用生产创建事务的地表定义。
            assert!(surface.layers.is_empty()); // 地表只能由作者显式添加。
            let (indices, weights) = empty_new_map_surface_sources(); // 复用生产写盘前的原始作者数据。
            assert!(indices.iter().all(|value| *value == 0)); // 不包含隐藏预设索引。
            assert!(weights.iter().all(|value| *value == 0)); // 不会覆盖白模或制造已绘制地皮。
            encode_surface_maps(&indices, &weights, surface.resolution, 0).unwrap(); // 全零作者源仍可生成成对Mip纹理。
        }
    }

    #[test] // 锁定创建事务实际使用的入口构造器。
    fn clipmap_and_legacy_creation_register_empty_vegetation_and_water() {
        // 不启动真实地图创建或修改用户目录。
        let map_id = MapId::parse("MAP-vegetation-entry").unwrap(); // 固定有效测试身份。
        for mode in [
            NewMapTerrainMode::SparseClipmap,
            NewMapTerrainMode::LegacyHeightfield,
        ] {
            // 两路线必须具有同样Blade作者入口。
            let entry = new_map_catalog_entry(&map_id, "空草地", mode); // 与生产发布调用相同。
            assert_eq!(
                entry.vegetation_path.as_deref(),
                Some("maps/MAP-vegetation-entry/vegetation.map_vegetation.json")
            ); // 路径严格指向已写入的空JSON。
            assert_eq!(
                entry.water_path.as_deref(),
                Some("maps/MAP-vegetation-entry/water.map_water.json")
            ); // 空水域JSON与目录入口同批发布。
        } // 不需要任何整图高度数组。
    } // 默认Clipmap路线不会再次出现无植被入口。

    #[test]
    fn new_map_vegetation_starts_without_formal_outputs_or_author_layers() {
        let map_id = MapId::parse("MAP-empty-authoring-test").expect("测试MapId合法"); // 模拟尚未发布的新地图身份。
        let vegetation = default_new_map_vegetation(&map_id, "maps/MAP-empty-authoring-test"); // 调用创建事务实际使用的纯构造器。
        let authoring = vegetation
            .authoring
            .as_ref()
            .expect("新地图显式保存作者结构"); // `None`会被旧图迁移误判。

        assert!(vegetation.layers.is_empty()); // 预设按钮来自全局目录，不要求地图预登记固定槽位。
        assert!(authoring.layers.is_empty()); // 地图目录首次打开不得显示任何预设图层。
        assert_eq!(authoring.groups.len(), 1); // 第一张新层无需作者先手工创建组。
        authoring
            .validate(&vegetation.layers)
            .expect("零正式输出与零作者层的新地图植被文件必须通过正式校验"); // 保存和编译路径接受空列表。
    }

    #[test]
    fn new_map_water_starts_without_static_or_spline_objects() {
        let map_id = MapId::parse("MAP-empty-water-test").expect("测试MapId合法");
        let water = default_water_asset(&map_id); // 使用创建事务实际写入JSON的纯构造器。

        assert!(water.layers.is_empty()); // 作者未操作前列表默认为零个静水体。
        assert!(water.rivers.is_empty()); // 新地图不得凭空形成河流样条。
        assert!(water.waterfalls.is_empty()); // 新地图不得凭空形成瀑布连接。
        water
            .validate(&map_id)
            .expect("零对象水域文件必须可保存和加载");
    }

    #[test]
    fn legacy_route_still_encodes_the_selected_full_glb_topology() {
        let dimensions =
            MapDimensions::for_mode(NewMapTerrainMode::LegacyHeightfield, 262.0, 243.0, 2.0)
                .unwrap();
        let glb = build_new_map_terrain_glb(dimensions).unwrap();
        let mesh = feathrise_map_terrain::parse_dynamic_terrain_glb(&glb).unwrap();
        assert_eq!(mesh.positions.len(), 132 * 123); // 旧方案仍真实生成作者选择的完整顶点网格。
        assert_eq!(mesh.indices.len(), 131 * 122 * 6); // 每个单元保持两个三角形，未退化为标签兼容。
    }

    #[test]
    fn dirty_map_confirmation_queues_save_then_creation_snapshot() {
        let mut editor = MapEditorState {
            dirty: true,
            new_map_name: "  第二张地图  ".to_owned(),
            new_map_notes: "入口村落\n待完善河岸".to_owned(), // 保存等待期间也必须冻结多行备注。
            ..default()
        };
        let mut task = MapCreationTask::default();
        let browser = MapBrowserState {
            write_blocked: None, // 模拟归类资产已经完成缺失或有效首载。
            ..default()
        };
        request_map_creation(
            &mut editor,
            &mut task,
            &browser,
            &MapFolderPersistence::default(),
            false,
        );
        assert!(editor.save_requested); // 一次点击必须主动推进保存，而不是静默拒绝。
        assert!(task.task.is_none()); // 当前草稿尚未保存时不能提前切图。
        assert_eq!(
            task.pending_after_save,
            Some(MapCreationRequest {
                collision_lods: Vec::new(), // GLB确认快照不消费稀疏碰撞列表。
                display_lods: None,         // GLB确认快照不携带SparseClipmap显示配置。
                display_name: "第二张地图".to_owned(),
                notes: "入口村落\n待完善河岸".to_owned(), // 当前未保存地图不会吞掉本次备注输入。
                width_meters: 1.0,
                length_meters: 1.0,
                terrain_mode: NewMapTerrainMode::Empty, // 新建只冻结空容器与文本资料。
                terrain_cell_meters: 2.0, // 弹框默认作者格距与待保存的确认请求一起冻结。
                folder_id: None,
                folder_source_bytes: None,
            })
        ); // 后台保存期间作者输入变化不得改变原请求。
    }

    #[test]
    fn map_creation_ignores_stale_geometry_and_never_creates_map_level_lods() {
        let mut editor = MapEditorState {
            dirty: true,
            new_map_name: "空容器地图".into(),
            ..default()
        };
        let mut task = MapCreationTask::default();
        request_map_creation(&mut editor, &mut task, &MapBrowserState {
            write_blocked: None, ..default()
        }, &MapFolderPersistence::default(), false);
        let request = task.pending_after_save.as_ref().expect("先保存当前地图，再创建空容器");
        assert_eq!(request.terrain_mode, NewMapTerrainMode::Empty);
        assert!(request.collision_lods.is_empty());
        assert!(request.display_lods.is_none());
        assert_eq!((request.width_meters, request.length_meters), (1.0, 1.0));
    }
}
