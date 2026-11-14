//! 示例工程的一块正式Clipmap高度地形和空作者画布；不生成独立GLB高度网格。

use feathrise_scene_content::*;
use std::{fs, io::Write, path::Path};

pub const SAMPLE_TERRAIN_FOLDER: &str = "scenes/M1-SAMPLE/terrains/starter-ground";
pub const SAMPLE_SURFACE_RESOLUTION: [u32; 2] = [512, 512];

pub(super) fn create(root: &Path) -> Result<SceneTerrainObject, String> {
    let map = SceneId::parse("M1-SAMPLE")?;
    let folder = SAMPLE_TERRAIN_FOLDER;
    // 示例与创建面板共用分页高度作者源，出生点仍放在箱子前方。
    let id = TerrainId::parse("starter-ground")?;
    let spec = SparseHeightSpec::new([-32.; 2], [32.; 2], 2.0, -80.0, 160.0)?;
    let mut document = SparseHeightDocument::create_for_terrain(root, map.clone(), Some(id.clone()), spec.clone())?;
    document.save_dirty()?;
    let terrain = SceneTerrainDescriptor {
        kind: SceneTerrainKind::SparseClipmap,
        glb_path: String::new(),
        height_path: Some(document.manifest().asset_path()),
        resolution: spec.cells.map(|count| count as u32 + 1),
        display_lods: Some(default_display_lods_for_spec(&spec)?),
        collision_lods: default_collision_lods_for_resolution(spec.cells.map(|count| count as u32 + 1)),
        base_height: spec.base_height,
        height_scale: spec.height_scale,
        ..SceneTerrainDescriptor::empty()
    };

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
            model_surfaces: Vec::new().into(),
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
        id,
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
