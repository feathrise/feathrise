//! 空工程格式骨架与显式验收种子。只写调用方新建的私有目录，不复制或改写仓库 RPG 地图。

use feathrise_scene_content::*;
use serde::Serialize;
use serde_json::json;
use std::{fs, path::Path};

#[cfg(any(test, feature = "validation"))]
#[path = "seed_terrain.rs"]
mod terrain;
#[cfg(any(test, feature = "validation"))]
pub use terrain::{SAMPLE_SURFACE_RESOLUTION, SAMPLE_TERRAIN_FOLDER};

/// 正式空工程只建立格式合法的空目录，不自动创建示例场景、地形、模型或游戏预设。
pub fn create_empty(asset_root: &Path) -> Result<(), String> {
    write(
        asset_root,
        SCENE_CATALOG_ASSET_PATH,
        &SceneCatalogAsset { schema_version: SCENE_SCHEMA_VERSION, scenes: Vec::new() },
    )?;
    write(
        asset_root,
        "scenes/editor.scene_folders.json",
        &json!({"schema_version":1,"folders":[],"scene_folders":{}}),
    )?;
    // 角色、装备、技能和对话属于工程／插件；空工程不携带原游戏的格式骨架。
    write(
        asset_root,
        BUILDING_PRESET_CATALOG_ASSET_PATH,
        &json!({"schema_version":BUILDING_PRESET_SCHEMA_VERSION,"presets":[]}),
    )?;
    write(
        asset_root,
        COURTYARD_PRESET_CATALOG_ASSET_PATH,
        &json!({"schema_version":COURTYARD_PRESET_SCHEMA_VERSION,"presets":[]}),
    )?;
    write(
        asset_root,
        VEGETATION_PRESET_CATALOG_ASSET_PATH,
        &json!({"schema_version":VEGETATION_PRESET_SCHEMA_VERSION,"presets":[]}),
    )?;
    // 派生路径是格式骨架，零预设时不创建或加载纹理数组；首次保存地表由原编码器生成。
    write(
        asset_root,
        SURFACE_PRESET_CATALOG_ASSET_PATH,
        &json!({"schema_version":1,"texture_resolution":[1,1],"base_color_array_path":"textures/surface/base.ktx2","normal_array_path":"textures/surface/normal.ktx2","arm_height_array_path":"textures/surface/arm_height.ktx2","presets":[]}),
    )?;
    write(
        asset_root,
        WATER_PRESET_CATALOG_ASSET_PATH,
        &json!({"schema_version":WATER_PRESET_SCHEMA_VERSION,"presets":[]}),
    )?;
    write(
        asset_root,
        WORLD_ENVIRONMENT_ASSET_PATH,
        &WorldEnvironmentAsset::default(),
    )?;
    Ok(())
}

/// 有真实地形的有限验收夹具；正式新建入口必须使用 `create_empty`。
#[cfg(any(test, feature = "validation"))]
pub fn create(asset_root: &Path, label: &str) -> Result<(), String> {
    let sample_id = "M1-SAMPLE";
    write(
        asset_root,
        SCENE_CATALOG_ASSET_PATH,
        &json!({"schema_version":SCENE_SCHEMA_VERSION,"scenes":[
        {"id":sample_id,"display_name":label,"folder":"scenes/M1-SAMPLE","published":true},
        {"id":"M1-EMPTY","display_name":"空场景","folder":"scenes/M1-EMPTY","published":false}]}),
    )?;
    // 新工程从原编辑器的合法空分类开始；避免首次启动把可选归类文件缺失打印为资产错误。
    write(
        asset_root,
        "scenes/editor.scene_folders.json",
        &json!({"schema_version":1,"folders":[],"scene_folders":{}}),
    )?;
    for (id, name) in [(sample_id, label), ("M1-EMPTY", "空场景")] {
        let folder = format!("scenes/{id}");
        // 示例模板显式包含一块正式地形；空白场景继续是真正空容器，不隐式补碰撞平面。
        let terrains = if id == sample_id {
            vec![terrain::create(asset_root)?]
        } else {
            vec![]
        };
        let spawn = if id == sample_id {
            [0., 2., 6.]
        } else {
            [0., 2., 0.]
        };
        write(
            asset_root,
            &format!("{folder}/{SCENE_MANIFEST_FILE_NAME}"),
            &json!({"schema_version":SCENE_SCHEMA_VERSION,"id":id,
            "display_name":name,"runtime_scene":SceneRuntimeKind::DataDriven.as_str(),"world_min":[-32.,-32.],"world_max":[32.,32.],
            "default_spawn":spawn,"terrains":terrains,"asset_roots":["scene"]}),
        )?;
        write(
            asset_root,
            &format!("{folder}/portals.scene_portals.json"),
            &json!({"schema_version":SCENE_SCHEMA_VERSION,"scene_id":id,"portals":[]}),
        )?;
        // 这是真实placements格式，运行时由feathrise_scene的生产装配器创建WorldAsset和正式实例根。
        // 建筑保存沿用原五点地基规则：平地零高、单位缩放和固定高度，不能继承原型任意缩放。
        let placements = if id == sample_id {
            vec![json!({"id":"sample-model","asset_path":"scene/probe.glb",
            "translation":[0.,0.,0.],"yaw_degrees":0.,"scale":[1.,1.,1.],
            "height_mode":"fixed_height","building_preset_id":"sample-building"})]
        } else {
            vec![]
        };
        write(
            asset_root,
            &format!("{folder}/placements.scene_placements.json"),
            &json!({"schema_version":SCENE_SCHEMA_VERSION,"scene_id":id,"placements":placements}),
        )?;
    }
    let preset = BuildingPresetDescriptor::from_asset(
        "sample-building".into(),
        format!("{label} · 示例建筑"),
        "scene/probe.glb".into(),
        Default::default(),
    );
    write(
        asset_root,
        BUILDING_PRESET_CATALOG_ASSET_PATH,
        &BuildingPresetCatalogAsset {
            schema_version: 1,
            presets: vec![preset],
        },
    )?;
    write(
        asset_root,
        COURTYARD_PRESET_CATALOG_ASSET_PATH,
        &CourtyardPresetCatalogAsset {
            schema_version: COURTYARD_PRESET_SCHEMA_VERSION,
            presets: vec![],
        },
    )?;
    write(
        asset_root,
        VEGETATION_PRESET_CATALOG_ASSET_PATH,
        &VegetationPresetCatalogAsset {
            schema_version: VEGETATION_PRESET_SCHEMA_VERSION,
            presets: vec![],
        },
    )?;
    // 生产地表目录必须有一项回退，数组由宿主复用正式KTX2编码器生成；运行时不依赖作者编码器。
    write(
        asset_root,
        SURFACE_PRESET_CATALOG_ASSET_PATH,
        &SurfacePresetCatalogAsset {
            schema_version: SURFACE_PRESET_SCHEMA_VERSION,
            texture_resolution: [1, 1],
            base_color_array_path: "textures/surface/base.ktx2".into(),
            normal_array_path: "textures/surface/normal.ktx2".into(),
            arm_height_array_path: "textures/surface/arm_height.ktx2".into(),
            presets: vec![SurfacePresetDescriptor {
                id: "neutral".into(),
                aliases: vec![],
                label: format!("{label} · 默认地表"),
                base_color_srgba: [1.; 4],
                base_color_texture_path: None,
                normal_texture_path: None,
                arm_height_texture_path: None,
                tile_meters: 1.,
                normal_strength: 1.,
                normal_y_flip: false,
                height_blend_strength: 0.,
                perceptual_roughness: 0.8,
                metallic: 0.,
                crater_response: 1.,
                projection: Default::default(),
            }],
        },
    )?;
    write(
        asset_root,
        WATER_PRESET_CATALOG_ASSET_PATH,
        &WaterPresetCatalogAsset {
            schema_version: WATER_PRESET_SCHEMA_VERSION,
            presets: vec![WaterPresetDescriptor {
                id: "calm".into(),
                aliases: vec![],
                label: format!("{label} · 默认静水"),
                base_color_srgba: [0.035, 0.18, 0.24, 0.72],
                perceptual_roughness: 0.06,
                reflectance: 0.6,
                trough_darkening: 0.16,
                shore_wave_attenuation: 1.,
                macro_waves: std::array::from_fn(|_| WaterMacroWave {
                    direction: [1., 0.],
                    amplitude_meters: 0.,
                    wavelength_meters: 10.,
                    speed_meters_per_second: 0.,
                    steepness: 0.,
                }),
                micro_normals: std::array::from_fn(|_| WaterMicroNormalLayer {
                    texture_path: None,
                    tile_meters: 1.,
                    velocity_meters_per_second: [0., 0.],
                    strength: 0.,
                }),
            }],
        },
    )?;
    write(
        asset_root,
        WORLD_ENVIRONMENT_ASSET_PATH,
        &WorldEnvironmentAsset::default(),
    )?;
    Ok(())
}

fn write(root: &Path, relative: &str, value: &impl Serialize) -> Result<(), String> {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().ok_or("种子路径缺少目录")?).map_err(|e| e.to_string())?;
    // 新建目录内使用create_new，防调用方误将已存在的工程传入并覆盖内容。
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
