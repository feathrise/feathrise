//! 后台冻结工程的正式场景目录与建筑预设来源，禁止同名文件向仓库或其他工程回退。

use feathrise_scene_content::{
    BUILDING_PRESET_CATALOG_ASSET_PATH, BuildingPresetCatalogAsset,
    COURTYARD_PRESET_CATALOG_ASSET_PATH, CourtyardPresetCatalogAsset, SCENE_CATALOG_ASSET_PATH,
    SCENE_SCHEMA_VERSION, SceneCatalogAsset, SceneId, SceneManifestAsset,
    ScenePlacementsAsset, ScenePortalsAsset, ResolvedBuildingPresetCatalog,
    ResolvedSurfacePresetCatalog, ResolvedVegetationPresetCatalog, ResolvedWaterPresetCatalog,
    SURFACE_PRESET_CATALOG_ASSET_PATH, SurfacePresetCatalogAsset,
    VEGETATION_PRESET_CATALOG_ASSET_PATH, VegetationPresetCatalogAsset,
    WATER_PRESET_CATALOG_ASSET_PATH, WaterPresetCatalogAsset,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
pub struct SceneEntry {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub published: bool,
    pub manifest_sha256: String,
    pub placement_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectScenes {
    /// 工程内容根也是AssetServer默认源；不同工程的相同SceneId不能共享缓存。
    pub asset_root: PathBuf,
    pub catalog_sha256: String,
    pub building_presets_sha256: String,
    pub building_preset_names: Vec<String>,
    pub preset_counts: BTreeMap<String, usize>,
    pub entries: Vec<SceneEntry>,
    #[serde(skip)]
    positions: BTreeMap<String, usize>,
}

impl ProjectScenes {
    pub fn load(asset_root: &Path) -> Result<Self, String> {
        Self::load_with_check(asset_root, || Ok(()))
    }

    /// 使用宿主唯一工作槽的取消牌。长文件每64KiB检查一次，取消后不发布半份上下文。
    pub fn load_with_check(
        asset_root: &Path,
        mut check: impl FnMut() -> Result<(), String>,
    ) -> Result<Self, String> {
        check()?;
        let root = asset_root
            .canonicalize()
            .map_err(|e| format!("工程资产根无效：{e}"))?;
        let (mut catalog, catalog_sha256): (SceneCatalogAsset, _) =
            read(&root, SCENE_CATALOG_ASSET_PATH, &mut check)?;
        if catalog.schema_version != SCENE_SCHEMA_VERSION {
            return Err("正式场景目录版本不兼容".into());
        }
        let (buildings, building_presets_sha256): (BuildingPresetCatalogAsset, _) =
            read(&root, BUILDING_PRESET_CATALOG_ASSET_PATH, &mut check)?;
        ResolvedBuildingPresetCatalog::build(&buildings)?;
        let (courtyards, _): (CourtyardPresetCatalogAsset, _) =
            read(&root, COURTYARD_PRESET_CATALOG_ASSET_PATH, &mut check)?;
        courtyards.validate()?;
        let (vegetation, _): (VegetationPresetCatalogAsset, _) =
            read(&root, VEGETATION_PRESET_CATALOG_ASSET_PATH, &mut check)?;
        ResolvedVegetationPresetCatalog::build(&vegetation)?;
        let (surface, _): (SurfacePresetCatalogAsset, _) =
            read(&root, SURFACE_PRESET_CATALOG_ASSET_PATH, &mut check)?;
        ResolvedSurfacePresetCatalog::build(&surface)?;
        let (water, _): (WaterPresetCatalogAsset, _) =
            read(&root, WATER_PRESET_CATALOG_ASSET_PATH, &mut check)?;
        ResolvedWaterPresetCatalog::build(&water)?;
        let preset_counts = BTreeMap::from([
            ("building".into(), buildings.presets.len()),
            ("courtyard".into(), courtyards.presets.len()),
            ("vegetation".into(), vegetation.presets.len()),
            ("surface".into(), surface.presets.len()),
            ("water".into(), water.presets.len()),
        ]);
        let mut positions = BTreeMap::new();
        let mut entries = Vec::new();
        for entry in &mut catalog.scenes {
            check()?;
            SceneId::parse(&entry.id.0)?;
            if positions
                .insert(entry.id.0.clone(), entries.len())
                .is_some()
            {
                return Err(format!("重复场景ID：{}", entry.id));
            }
            let (manifest, manifest_sha256): (SceneManifestAsset, _) =
                read(&root, &entry.manifest_path(), &mut check)?;
            let (placements, _): (ScenePlacementsAsset, _) =
                read(&root, &entry.placements_path(), &mut check)?;
            let (portals, _): (ScenePortalsAsset, _) =
                read(&root, &entry.portals_path(), &mut check)?;
            if manifest.id != entry.id
                || placements.scene_id != entry.id
                || portals.scene_id != entry.id
                || manifest.schema_version != SCENE_SCHEMA_VERSION
                || placements.schema_version != SCENE_SCHEMA_VERSION
                || portals.schema_version != SCENE_SCHEMA_VERSION
                || !manifest.is_data_driven()
            {
                return Err(format!("场景{}的正式文件身份或版本不一致", entry.id));
            }
            placements.validate_courtyards()?;
            for path in [
                entry.surface_path.as_deref(),
                entry.vegetation_path.as_deref(),
                entry.water_path.as_deref(),
                entry.boundary_path.as_deref(),
                entry.characters_path.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                feathrise_scene_content::authoring_io::safe_content_path(&root, path)?;
            }
            entry.display_name = manifest.display_name.clone(); // UI沿用正式清单已校验的当前显示名。
            entries.push(SceneEntry {
                id: entry.id.0.clone(),
                name: manifest.display_name,
                folder: entry.folder.clone(),
                published: entry.published,
                manifest_sha256,
                placement_count: placements.placements.len(),
            });
        }
        Ok(Self {
            asset_root: root,
            catalog_sha256,
            building_presets_sha256,
            building_preset_names: buildings.presets.iter().map(|p| p.label.clone()).collect(),
            preset_counts,
            entries,
            positions,
        })
    }

    pub fn contains(&self, id: &str) -> bool {
        self.positions.contains_key(id)
    }

    pub fn index(&self, id: &str) -> Option<usize> {
        self.positions.get(id).copied()
    }
}

fn read<T: for<'de> Deserialize<'de>>(
    root: &Path,
    relative: &str,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<(T, String), String> {
    let path = feathrise_scene_content::authoring_io::safe_content_path(root, relative)?;
    // 解析与SHA消费同一次文件流，既不额外保留整份JSON字节，也不为正式摆放文件增设8MiB内容上限。
    let file = File::open(path).map_err(|e| format!("无法打开{relative}：{e}"))?;
    let mut stream = HashReader {
        file,
        digest: Sha256::new(),
        check,
    };
    let value = serde_json::from_reader(BufReader::with_capacity(64 * 1024, &mut stream))
        .map_err(|e| format!("{relative}格式或读取无效：{e}"))?;
    Ok((
        value,
        stream
            .digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}

struct HashReader<'a, F> {
    file: File,
    digest: Sha256,
    check: &'a mut F,
}
impl<F: FnMut() -> Result<(), String>> Read for HashReader<'_, F> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        (self.check)().map_err(io::Error::other)?;
        let count = self.file.read(buffer)?;
        self.digest.update(&buffer[..count]);
        Ok(count)
    }
}
