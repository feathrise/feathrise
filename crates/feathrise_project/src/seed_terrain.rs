//! 示例工程的一块正式高度网格和空作者画布；几何编码与运行时使用同一纯算法库。

use feathrise_scene_content::*;
use feathrise_terrain::{TerrainSampleSpec, build_flat_terrain_glb};
use std::{fs, io::Write, path::Path};

pub const SAMPLE_TERRAIN_FOLDER: &str = "scenes/M1-SAMPLE/terrains/starter-ground";
pub const SAMPLE_SURFACE_RESOLUTION: [u32; 2] = [512, 512];

pub(super) fn create(root: &Path) -> Result<SceneTerrainObject, String> {
    let map = SceneId::parse("M1-SAMPLE")?;
    let folder = SAMPLE_TERRAIN_FOLDER;
    // 与原“添加高度网格”保持相同权威：GLB持久保存，CPU高度/碰撞在正式加载时派生。
    // 64m尺寸与2m格距属于原新建参数域；出生点放在示例箱子前方，避免从碰撞内部开始。
    let spec = TerrainSampleSpec {
        resolution: [33, 33],
        world_min: [-32., -32.],
        world_max: [32., 32.],
        base_height: -80.,
        height_scale: 160.,
    };
    let terrain = SceneTerrainDescriptor {
        kind: SceneTerrainKind::Heightfield,
        glb_path: format!("{folder}/terrain.glb"),
        resolution: spec.resolution,
        base_height: spec.base_height,
        height_scale: spec.height_scale,
        ..SceneTerrainDescriptor::empty()
    };
    write_bytes(root, &terrain.glb_path, &build_flat_terrain_glb(spec, 0.)?)?;

    // 原新网格也是零贡献地表；作者显式添加地表层后才引用项目地表预设。
    // KTX2由宿主模板事务调用现有feathrise_editor_assets编码器生成，不让游戏依赖作者编码库。
    let surface = SceneSurfaceAsset {
        schema_version: SCENE_SURFACE_SCHEMA_VERSION,
        scene_id: map.clone(),
        weight_source_path: format!("{folder}/surface/weights.rgba8"),
        weight_texture_path: format!("{folder}/surface/weights.ktx2"),
        index_source_path: format!("{folder}/surface/indices.rgba32"),
        index_texture_path: format!("{folder}/surface/indices.ktx2"),
        resolution: SAMPLE_SURFACE_RESOLUTION,
        layers: vec![],
    };
    let texels = surface.resolution[0] as usize * surface.resolution[1] as usize;
    write_bytes(root, &surface.index_source_path, &vec![0; texels * 16])?;
    write_bytes(root, &surface.weight_source_path, &vec![0; texels * 4])?;
    super::write(root, &format!("{folder}/{SCENE_SURFACE_FILE_NAME}"), &surface)?;
    super::write(
        root,
        &format!("{folder}/{SCENE_VEGETATION_FILE_NAME}"),
        &SceneVegetationAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scene_id: map.clone(),
            layers: vec![],
            authoring: Some(SceneVegetationAuthoring {
                groups: vec![SceneVegetationAuthorGroup {
                    id: "base".into(),
                    label: "基础植被".into(),
                    mode: SceneVegetationAuthorGroupMode::Additive,
                    output_merge: SceneVegetationOutputMergeMode::Maximum,
                    capacity: 1.,
                    claim_threshold: 0.,
                    selection_seed: 0x4D41_505F_4241_5345,
                    selection_cell_meters: 8.,
                    enabled: true,
                }],
                layers: vec![],
            }),
        },
    )?;
    super::write(
        root,
        &format!("{folder}/{SCENE_WATER_FILE_NAME}"),
        &SceneWaterAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scene_id: map,
            layers: vec![],
            rivers: vec![],
            waterfalls: vec![],
        },
    )?;
    Ok(SceneTerrainObject {
        clipmap_stitches: Default::default(),
        mesh_lod: None,
        id: TerrainId::parse("starter-ground")?,
        display_name: "初始地面".into(),
        enabled: true,
        transform: Default::default(),
        world_min: spec.world_min,
        world_max: spec.world_max,
        terrain,
        voxel: None,
        surface_path: Some(format!("{folder}/{SCENE_SURFACE_FILE_NAME}")),
        vegetation_path: Some(format!("{folder}/{SCENE_VEGETATION_FILE_NAME}")),
        water_path: Some(format!("{folder}/{SCENE_WATER_FILE_NAME}")),
        puddles: None,
    })
}

fn write_bytes(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().ok_or("种子资产缺少目录")?)
        .map_err(|error| error.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())
}
