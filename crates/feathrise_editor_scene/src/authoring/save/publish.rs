//! 保存完成后把小型作者JSON快照发布到正式唯一Asset句柄。

use bevy::prelude::Assets; // 主线程通过Assets写入产生正式Modified事件。
use feathrise_scene_content::{
    SceneContentRegistry, SceneId, SceneManifestAsset, ScenePlacementsAsset, SceneSurfaceAsset,
    SceneVegetationAsset, SceneWaterAsset,
}; // 目录句柄与清单、地表、摆放、植被、水网五类小型JSON完成值。

/// 把已经完整落盘的场景清单发布到目录现有强句柄，让环境正式值与清脏在同一帧接管。
pub(super) fn publish_saved_manifest_asset(
    registry: Option<&SceneContentRegistry>, // 目录提供当前场景唯一正式清单句柄。
    scene_id: &SceneId,                        // 只允许更新本次保存成功的场景。
    snapshot: &SceneManifestAsset,           // 与磁盘原子写入完全相同的完整清单。
    manifests: &mut Assets<SceneManifestAsset>, // 主世界资产只在 IO 事务完整成功后替换。
) -> bool {
    let Some(handle) = registry
        .and_then(|registry| registry.handles(scene_id))
        .map(|handles| &handles.manifest)
    else {
        return false; // 目录尚未建立当前场景句柄时由调用方显式请求文件重载。
    };
    let Some(mut loaded) = manifests.get_mut(handle) else {
        return false; // 唯一句柄仍在异步加载时不能创建第二份清单资产身份。
    };
    *loaded = snapshot.clone(); // AssetMut 同帧发布 Modified，运行时立即读取保存后的环境覆盖。
    true // 已原位发布时无需等待文件监听或再次主动重载。
}

/// 保存完成后对正式摆放资产采取的唯一发布动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SavedPlacementsPublishAction {
    None,    // 磁盘快照与已加载正式资产内容相同，不制造Modified或重复候选。
    Replace, // 唯一正式句柄已加载且内容不同，直接原位替换。
    Reload,  // 目录或资产仍未就绪，只能从刚落盘路径显式重载。
    Rebuild, // 上次正式采用失败但内容未变，请求运行时沿同一正式链路重试。
}

/// 比较摆放作者内容而不是JSON排版，兼容旧文件省略默认空`water_surfaces`字段。
pub(super) fn placement_documents_equal(
    left: &ScenePlacementsAsset,
    right: &ScenePlacementsAsset,
) -> bool {
    left.schema_version == right.schema_version
        && left.scene_id == right.scene_id
        && left.placements.len() == right.placements.len()
        && left
            .placements
            .iter()
            .zip(&right.placements)
            .all(|(left, right)| {
                left.id == right.id
                    && left.asset_path == right.asset_path
                    && left.asset_target == right.asset_target
                    && left.translation == right.translation
                    && left.yaw_degrees == right.yaw_degrees
                    && left.scale == right.scale
                    && left.water_surfaces == right.water_surfaces
            }) // 顺序属于当前作者文档的确定性内容，不能把重排误判成相同快照。
}

/// 只读决定是否需要发布；`None`路径不会取得`AssetMut`，因此不会产生伪`Modified`。
pub(super) fn plan_saved_placements_publish(
    registry: Option<&SceneContentRegistry>,
    scene_id: &SceneId,
    snapshot: &ScenePlacementsAsset,
    placements: &Assets<ScenePlacementsAsset>,
    retry_failed_runtime: bool,
) -> SavedPlacementsPublishAction {
    let Some(handle) = registry
        .and_then(|registry| registry.handles(scene_id))
        .map(|handles| &handles.placements)
    else {
        return SavedPlacementsPublishAction::Reload; // 目录未建立时只能让AssetServer重新取得正式文件身份。
    };
    let Some(loaded) = placements.get(handle) else {
        return SavedPlacementsPublishAction::Reload; // 已有句柄仍在加载时不能原位替换未知内容。
    };
    if !placement_documents_equal(loaded, snapshot) {
        SavedPlacementsPublishAction::Replace // 真实作者内容变化才允许取得AssetMut并触发Modified。
    } else if retry_failed_runtime {
        SavedPlacementsPublishAction::Rebuild // 原样重试通过显式请求，不靠无意义写回资产制造事件。
    } else {
        SavedPlacementsPublishAction::None // 内容无变化时同时跳过内存发布与文件reload。
    }
}

/// 把已经完整落盘的显式摆放快照发布到目录现有强句柄，使网格赋水与实例变换走正式隐藏候选重建。
pub(super) fn publish_saved_placements_asset(
    registry: Option<&SceneContentRegistry>, // 目录提供当前场景唯一正式摆放句柄。
    scene_id: &SceneId,                        // 只允许更新本次保存成功的场景。
    snapshot: &ScenePlacementsAsset,         // 与磁盘原子写入完全相同的实例和整网格水体覆盖。
    placements: &mut Assets<ScenePlacementsAsset>, // 主世界资产只在IO任务完成后由主线程替换。
) -> bool {
    let Some(handle) = registry
        .and_then(|registry| registry.handles(scene_id))
        .map(|handles| &handles.placements)
    else {
        return false; // 目录尚未建立当前场景句柄时由调用方显式请求文件重载。
    };
    let Some(mut loaded) = placements.get_mut(handle) else {
        return false; // 句柄仍在异步加载时交给AssetServer路径重载兜底。
    };
    *loaded = snapshot.clone(); // AssetMut发布Modified，运行时据此建立带网格水体语义的新隐藏候选根。
    true // 调用方无需再等待文件监听偶然观察到原子替换。
}

/// 把真实变化且已完整落盘的地表定义发布到目录唯一句柄，避免另建资产身份或等待文件监听。
pub(super) fn publish_saved_surface_asset(
    registry: Option<&SceneContentRegistry>, // 目录提供当前场景唯一正式地表句柄。
    scene_id: &SceneId,                        // 只允许更新本次保存成功的场景。
    snapshot: &SceneSurfaceAsset,            // 与磁盘原子写入完全相同的地表层定义。
    surfaces: &mut Assets<SceneSurfaceAsset>, // 主世界资产只能在后台事务完成后替换。
) -> bool {
    let Some(handle) = registry
        .and_then(|registry| registry.handles(scene_id))
        .and_then(|handles| handles.surface.as_ref())
    else {
        return false; // 目录尚未登记地表句柄时由调用方显式重载权威JSON路径。
    };
    let Some(mut loaded) = surfaces.get_mut(handle) else {
        return false; // 唯一句柄仍在异步加载时不能创建第二份资产身份。
    };
    *loaded = snapshot.clone(); // AssetMut产生唯一Modified，动态地表系统随后按新修订预热。
    true // 已原位发布时禁止再发一次路径reload制造重复修订。
}

/// 把已经完整落盘的植被JSON快照发布到目录现有强句柄，切换场景重载不得继续读取保存前缓存。
pub(super) fn publish_saved_vegetation_asset(
    registry: Option<&SceneContentRegistry>, // 目录提供当前场景正式植被句柄，不能新建第二份资产身份。
    scene_id: &SceneId,                        // 只允许更新本次保存成功的场景。
    snapshot: &SceneVegetationAsset,         // 保存任务冻结并完整写盘的同一份作者结构与正式输出描述。
    vegetations: &mut Assets<SceneVegetationAsset>, // 主世界资产只能在任务完成后由主线程修改。
) -> bool {
    let Some(handle) = registry
        .and_then(|registry| registry.handles(scene_id))
        .and_then(|handles| handles.vegetation.as_ref())
    else {
        return false; // 目录尚未登记句柄时由调用方显式请求文件重载。
    };
    let Some(mut loaded) = vegetations.get_mut(handle) else {
        return false; // 句柄仍在异步加载时同样交给AssetServer重载兜底。
    };
    *loaded = snapshot.clone(); // AssetMut在本帧发布Modified，运行时和下一次草稿加载立即观察新图层结构。
    true // 调用方无需再制造第二次主动重载事件。
}

/// 把已经完整落盘的水网JSON快照发布到目录现有强句柄，避免保存后重新进入时读回旧样条缓存。
pub(super) fn publish_saved_water_asset(
    registry: Option<&SceneContentRegistry>, // 目录提供当前场景唯一正式水域句柄。
    scene_id: &SceneId,                        // 只允许更新本次保存成功的场景。
    snapshot: &SceneWaterAsset,              // 与磁盘原子写入完全相同的静水、河流和瀑布定义。
    waters: &mut Assets<SceneWaterAsset>,    // 主世界资产只在IO任务完成后由主线程替换。
) -> bool {
    let Some(handle) = registry
        .and_then(|registry| registry.handles(scene_id))
        .and_then(|handles| handles.water.as_ref())
    else {
        return false; // 旧场景首次登记water_path时由调用方显式重载目录与文件。
    };
    let Some(mut loaded) = waters.get_mut(handle) else {
        return false; // 句柄仍在异步加载时交给AssetServer路径重载兜底。
    };
    *loaded = snapshot.clone(); // AssetMut发布Modified，运行时与后续草稿加载取得同一完整水网修订。
    true // 调用方无需再制造第二次主动JSON重载事件。
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap; // 构造只含一个场景的确定性目录注册表。

    use bevy::prelude::Handle; // 非被测私有资产使用空类型化句柄即可。
    use feathrise_scene_content::{
        SCENE_SCHEMA_VERSION, SceneCatalogAsset, SceneContentHandles, SceneEnvironmentDescriptor,
        SceneManifestAsset, ScenePlacementAssetTarget, ScenePlacementHeightMode, ScenePlacementRecord,
        ScenePortalsAsset, SceneTerrainDescriptor, SceneTerrainKind,
    }; // 注册表必须保持与生产结构完全相同的类型。

    use super::*; // 直接验证摆放快照通过唯一句柄产生可见替换。

    /// 构造只含本测试所需字段的完整动态场景清单。
    fn test_manifest(scene_id: &SceneId, display_name: &str) -> SceneManifestAsset {
        SceneManifestAsset {
            vegetation_paint_groups: Default::default(),
            terrains: None,
            subscenes: Default::default(),
            schema_version: SCENE_SCHEMA_VERSION,    // 模拟当前正式清单格式。
            id: scene_id.clone(),                    // 清单身份必须匹配目录键。
            display_name: display_name.to_owned(), // 字段变化用于证明原位替换真实发生。
            notes: String::new(),                  // 测试清单无需备注。
            runtime_scene: "data_driven".to_owned(), // 新天体配置只服务统一动态运行时。
            world_min: [0.0, 0.0],                 // 合法最小测试范围。
            world_max: [10.0, 10.0],               // 合法最大测试范围。
            default_spawn: [5.0, 0.0, 5.0],        // 出生点不参与发布逻辑。
            environment: SceneEnvironmentDescriptor::default(), // 从无场景覆盖的安全环境开始。
            puddles: None, // 新图及旧图夹具默认没有积水，首次有效落笔才创建。
            terrain: SceneTerrainDescriptor {
                resolved_stitches:Default::default(),
                collision_lods: Vec::new(),         // GLB测试不消费稀疏碰撞配置。
                display_lods: None,                 // GLB测试不保存SparseClipmap显示LOD。
                kind: SceneTerrainKind::Heightfield,  // 动态场景使用可编辑高度场。
                collision_mode: Default::default(), // GLB路线不读取稀疏碰撞方案。
                player_collision_preload_radius_meters:
                    feathrise_scene_content::DEFAULT_PLAYER_COLLISION_PRELOAD_RADIUS_METERS, // GLB路线占位。
                role_collision_preload_radius_meters: None, // GLB路线没有粗级分块预载。
                player_collision_chunk_cells: None,
                role_collision_chunk_cells: None,
                player_collision_span_meters: None, // GLB不使用分块联动设置。
                role_collision_span_meters: None,   // GLB不使用分块联动设置。
                glb_path: "scenes/test/terrain.glb".to_owned(), // 测试不实际加载该路径。
                height_path: None,                  // 发布测试不读取 R16。
                height_texture_path: None,          // 发布测试不读取 GPU 高度纹理。
                resolution: [11, 11],               // 保持合法高度场描述。
                base_height: 0.0,                   // 不影响环境清单发布。
                height_scale: 100.0,                // 不影响环境清单发布。
            },
            asset_roots: Vec::new(), // 发布测试不浏览作者资产。
        }
    }

    #[test]
    fn saved_manifest_replaces_the_registered_asset_in_place() {
        let scene_id = SceneId::parse("MAP-manifest-publish").expect("测试场景ID合法"); // 当前保存身份。
        let mut assets = Assets::<SceneManifestAsset>::default(); // 模拟主世界唯一清单资产表。
        let handle = assets.add(test_manifest(&scene_id, "旧清单")); // 正式句柄先持有保存前内容。
        let registry = SceneContentRegistry {
            catalog: Handle::<SceneCatalogAsset>::default(), // 本纯测试不读取目录文件。
            revision: 1,
            scenes: BTreeMap::from([(
                scene_id.clone(),
                SceneContentHandles {
                    manifest: handle.clone(), // 被测函数必须沿用这一唯一正式身份。
                    portals: Handle::<ScenePortalsAsset>::default(),
                    placements: Handle::<ScenePlacementsAsset>::default(),
                    surface: None,
                    vegetation: None,
                    water: None,
                    characters_path: None, // 旧测试场景没有角色作者源。
                    boundaries: None,
                },
            )]),
            last_error: None,
        };
        let mut snapshot = test_manifest(&scene_id, "新清单"); // 模拟已经完整落盘的作者快照。
        snapshot
            .environment
            .celestial_overrides
            .day_base_illuminance_lux = Some(12_345.0); // 保存后的天体覆盖必须同帧进入正式缓存。

        assert!(publish_saved_manifest_asset(
            Some(&registry),
            &scene_id,
            &snapshot,
            &mut assets,
        )); // 已加载正式句柄无需等待 AssetServer reload。
        let published = assets.get(&handle).expect("唯一清单句柄仍存在");
        assert_eq!(published.display_name, "新清单"); // 普通清单字段已经整体替换。
        assert_eq!(
            published
                .environment
                .celestial_overrides
                .day_base_illuminance_lux,
            Some(12_345.0),
        ); // 关闭 F12 后正式运行时立即读取刚保存的天体参数。
    }

    #[test]
    fn saved_placements_replace_the_registered_asset_in_place() {
        let scene_id = SceneId::parse("MAP-placement-publish").expect("测试场景ID合法"); // 当前保存身份。
        let mut assets = Assets::<ScenePlacementsAsset>::default(); // 模拟主世界唯一资产表。
        let handle = assets.add(ScenePlacementsAsset {
            schema_version: 0, // 旧缓存值用于证明本次不是仅返回成功而未替换内容。
            scene_id: scene_id.clone(),
            placements: Vec::new(),
        });
        let registry = SceneContentRegistry {
            catalog: Handle::<SceneCatalogAsset>::default(), // 本纯测试不读取目录资产。
            revision: 1,
            scenes: BTreeMap::from([(
                scene_id.clone(),
                SceneContentHandles {
                    manifest: Handle::<SceneManifestAsset>::default(),
                    portals: Handle::<ScenePortalsAsset>::default(),
                    placements: handle.clone(), // 被测函数必须沿用这一唯一正式身份。
                    surface: None,
                    vegetation: None,
                    water: None,
                    characters_path: None, // 旧测试场景没有角色作者源。
                    boundaries: None,
                },
            )]),
            last_error: None,
        };
        let snapshot = ScenePlacementsAsset {
            schema_version: SCENE_SCHEMA_VERSION, // 模拟已经完整落盘的新作者快照。
            scene_id: scene_id.clone(),
            placements: Vec::new(),
        };

        assert!(publish_saved_placements_asset(
            Some(&registry),
            &scene_id,
            &snapshot,
            &mut assets,
        )); // 现有句柄已加载时不需要AssetServer reload兜底。
        assert_eq!(
            assets
                .get(&handle)
                .expect("唯一摆放句柄仍存在")
                .schema_version,
            SCENE_SCHEMA_VERSION,
        ); // 相同Handle现在读取到保存成功的完整新值。
    }

    #[test]
    fn unchanged_placements_never_request_asset_mutation_but_failed_runtime_can_rebuild() {
        let scene_id = SceneId::parse("MAP-placement-noop").expect("测试场景ID合法");
        let snapshot = ScenePlacementsAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scene_id: scene_id.clone(),
            placements: Vec::new(),
        };
        let mut assets = Assets::<ScenePlacementsAsset>::default();
        let handle = assets.add(snapshot.clone());
        let registry = SceneContentRegistry {
            catalog: Handle::<SceneCatalogAsset>::default(),
            revision: 1,
            scenes: BTreeMap::from([(
                scene_id.clone(),
                SceneContentHandles {
                    manifest: Handle::<SceneManifestAsset>::default(),
                    portals: Handle::<ScenePortalsAsset>::default(),
                    placements: handle,
                    surface: None,
                    vegetation: None,
                    water: None,
                    characters_path: None, // 旧测试场景没有角色作者源。
                    boundaries: None,
                },
            )]),
            last_error: None,
        };

        assert_eq!(
            plan_saved_placements_publish(Some(&registry), &scene_id, &snapshot, &assets, false),
            SavedPlacementsPublishAction::None,
        ); // 普通无变化保存不取得AssetMut，也不制造一次正式候选修订。
        assert_eq!(
            plan_saved_placements_publish(Some(&registry), &scene_id, &snapshot, &assets, true),
            SavedPlacementsPublishAction::Rebuild,
        ); // 失败重试沿显式请求恢复，不违反未变化资产禁止Modified的契约。
    }

    #[test]
    fn placement_target_change_is_real_saved_content_change() {
        let scene_id = SceneId::parse("MAP-placement-target-diff").expect("测试场景ID合法");
        let scene0 = ScenePlacementsAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scene_id: scene_id.clone(),
            placements: vec![ScenePlacementRecord {
                mesh_lod: None,
                terrain_binding: None,
                editor: Default::default(),
                terrain_patch: None,
                id: "plant-1".to_owned(),
                asset_path: "gltf/plant.glb".to_owned(),
                asset_target: ScenePlacementAssetTarget::Scene0,
                building_preset_id: None, // 植物发布差分夹具保持普通资产身份。
                courtyard: None,          // 独立摆放记录不属于任何院落副本。
                translation: [1.0, 2.0, 3.0],
                height_mode: ScenePlacementHeightMode::FixedHeight,
                terrain_offset: 0.0,
                yaw_degrees: 0.0,
                scale: [1.0, 1.0, 1.0],
                water_surfaces: Vec::new(),
            }],
        };
        let mut primitive = scene0.clone();
        primitive.placements[0].asset_target = ScenePlacementAssetTarget::Primitive {
            mesh: 6,
            primitive: 0,
            material: Some(3),
        };

        assert!(!placement_documents_equal(&scene0, &primitive)); // 同路径同变换也必须发布新的可视目标。
    }
}
