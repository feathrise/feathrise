use std::collections::{BTreeMap, BTreeSet}; // 测试目录和零写入预检都使用确定性集合。
use std::path::{Path, PathBuf}; // 事务故障测试在系统临时目录构造完全隔离的目标集合。
use std::sync::Arc; // 最小R8画布与保存预检共享所有权。
use std::sync::atomic::{AtomicU64, Ordering}; // 并行测试不会争用同一临时目录。

use bevy::asset::AssetPlugin; // 构造真实Assets存储并验证同一强句柄原地替换。
use feathrise_scene_content::{
    SCENE_SCHEMA_VERSION, SceneCatalogAsset, SceneContentHandles, SceneManifestAsset, ScenePlacementsAsset,
    ScenePortalsAsset, SceneSurfaceAsset, SceneVegetationAuthoring, SceneWaterAsset, SceneWaterRiver,
    SceneWaterSegmentKind, SceneWaterSplinePoint, WorldEnvironmentAsset,
}; // 最小目录仍使用正式公共资产类型，避免伪造另一套句柄结构。

use super::*;
use crate::authoring::files::asset_path_collision_key; // 跨域输出碰撞测试复用正式词法路径身份。

#[path = "rollback_placement_retry_tests.rs"]
mod rollback_placement_retry; // 水文件回滚不能接管或遗失先前失败的正式摆放责任。

static SAVE_TRANSACTION_TEST_COUNTER: AtomicU64 = AtomicU64::new(0); // 唯一测试目录后缀。

pub(super) struct SaveTransactionTestDirectory {
    pub(super) path: PathBuf, // 保存批次测试共用隔离根；Drop只回收自己创建的直接子目录。
}

impl SaveTransactionTestDirectory {
    pub(super) fn new() -> Self {
        let serial = SAVE_TRANSACTION_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bevy-learn5-map-save-transaction-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("创建唯一场景保存事务测试目录");
        Self { path }
    }
}

impl Drop for SaveTransactionTestDirectory {
    fn drop(&mut self) {
        let temporary_root = std::env::temp_dir();
        if self.path.parent() == Some(temporary_root.as_path()) {
            let _ = std::fs::remove_dir_all(&self.path); // 仅回收已验证的唯一临时根直接子目录。
        }
    }
}

fn collect_transaction_artifacts(root: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).expect("遍历事务测试目录") {
        let entry = entry.expect("读取事务测试目录项");
        let path = entry.path();
        if path.is_dir() {
            collect_transaction_artifacts(&path, found);
        } else if entry.file_name().to_string_lossy().contains(".scene-save-") {
            found.push(path); // stage与backup都共享该仅内部可见标记。
        }
    }
}

#[test]
fn global_only_save_uses_the_same_atomic_transaction_path() {
    let temporary = SaveTransactionTestDirectory::new();
    let config_folder = temporary.path.join("config");
    std::fs::create_dir_all(&config_folder).expect("创建全局配置测试目录");
    let global_path = config_folder.join("world_environment.world_environment.json");
    std::fs::write(&global_path, b"old-global").expect("写入旧全局配置");
    let mut snapshot = WorldEnvironmentAsset::default();
    snapshot.celestial_lighting.day_base_illuminance_lux = 12_345.0;

    commit_with_replace_hook(
        vec![PreparedWrite::json(global_path.clone(), &snapshot)],
        |index, target| {
            assert_eq!(index, 1);
            assert_eq!(target, global_path);
            assert_eq!(
                std::fs::read(target).expect("替换前目标仍存在"),
                b"old-global"
            );
            Ok(())
        },
    )
    .unwrap_or_else(|failure| panic!("全局单文件事务应成功：{}", failure.message()));

    let saved: WorldEnvironmentAsset =
        serde_json::from_slice(&std::fs::read(&global_path).expect("读取保存后的全局配置"))
            .expect("保存后的全局配置必须是正式 JSON");
    assert_eq!(saved, snapshot);
    let mut artifacts = Vec::new();
    collect_transaction_artifacts(&temporary.path, &mut artifacts);
    assert!(
        artifacts.is_empty(),
        "单文件事务不应残留内部文件：{artifacts:?}"
    );
}

#[test]
fn global_and_scene_files_commit_as_one_successful_batch() {
    let temporary = SaveTransactionTestDirectory::new();
    let global_path = temporary
        .path
        .join("config/world_environment.world_environment.json");
    let scene_path = temporary
        .path
        .join("scenes/MAP-combined/scene.scene_manifest.json");
    std::fs::create_dir_all(global_path.parent().expect("全局配置父目录")).expect("创建配置目录");
    std::fs::create_dir_all(scene_path.parent().expect("场景清单父目录")).expect("创建场景目录");
    std::fs::write(&global_path, b"old-global").expect("写入旧全局配置");
    std::fs::write(&scene_path, b"old-map").expect("写入旧场景清单");
    let snapshot = WorldEnvironmentAsset::default();

    commit_with_replace_hook(
        vec![
            PreparedWrite::json(global_path.clone(), &snapshot),
            PreparedWrite::new(scene_path.clone(), b"new-map".to_vec()),
        ],
        |_, _| Ok(()),
    )
    .unwrap_or_else(|failure| panic!("全局与场景联合事务应完整成功：{}", failure.message()));

    let saved: WorldEnvironmentAsset =
        serde_json::from_slice(&std::fs::read(&global_path).expect("读取联合保存后的全局配置"))
            .expect("联合保存后的全局配置必须可解析");
    assert_eq!(saved, snapshot);
    assert_eq!(
        std::fs::read(&scene_path).expect("读取联合保存后的场景清单"),
        b"new-map"
    );
}

#[test]
fn combined_failure_rolls_back_both_global_and_scene_targets() {
    let temporary = SaveTransactionTestDirectory::new();
    let global_path = temporary
        .path
        .join("config/world_environment.world_environment.json");
    let scene_path = temporary
        .path
        .join("scenes/MAP-combined/scene.scene_manifest.json");
    let final_path = temporary
        .path
        .join("scenes/MAP-combined/final.scene_portals.json");
    std::fs::create_dir_all(global_path.parent().expect("全局配置父目录")).expect("创建配置目录");
    std::fs::create_dir_all(scene_path.parent().expect("场景清单父目录")).expect("创建场景目录");
    std::fs::write(&global_path, b"old-global").expect("写入旧全局配置");
    std::fs::write(&scene_path, b"old-map").expect("写入旧场景清单");
    std::fs::write(&final_path, b"old-final").expect("写入旧末项文件");

    let failure = commit_with_replace_hook(
        vec![
            PreparedWrite::new(global_path.clone(), b"new-global".to_vec()),
            PreparedWrite::new(scene_path.clone(), b"new-map".to_vec()),
            PreparedWrite::new(final_path.clone(), b"new-final".to_vec()),
        ],
        |index, _| {
            if index == 3 {
                Err("测试注入：联合事务末项替换失败".to_owned())
            } else {
                Ok(())
            }
        },
    )
    .expect_err("末项失败必须回滚全局与场景目标");

    assert!(failure.rollback_complete());
    assert_eq!(
        failure.rollback_targets(),
        [global_path.clone(), scene_path.clone()]
    );
    assert_eq!(
        std::fs::read(&global_path).expect("全局配置已回滚"),
        b"old-global"
    );
    assert_eq!(
        std::fs::read(&scene_path).expect("场景清单已回滚"),
        b"old-map"
    );
    assert_eq!(
        std::fs::read(&final_path).expect("失败末项未变化"),
        b"old-final"
    );
    let mut artifacts = Vec::new();
    collect_transaction_artifacts(&temporary.path, &mut artifacts);
    assert!(
        artifacts.is_empty(),
        "联合回滚不应残留内部文件：{artifacts:?}"
    );
}

#[test]
fn nth_replace_failure_restores_the_entire_old_batch_without_artifacts() {
    let temporary = SaveTransactionTestDirectory::new();
    let scene_folder = temporary.path.join("scenes/MAP-transaction");
    let water_folder = scene_folder.join("water");
    std::fs::create_dir_all(&water_folder).expect("创建测试场景私有目录");
    let manifest_path = scene_folder.join("manifest.scene.json");
    let custom_water_path = scene_folder.join("custom-water.scene_water.json");
    let mask_path = water_folder.join("lake.r8");
    let catalog_path = temporary.path.join("scene_catalog.scene_catalog.json");
    std::fs::write(&manifest_path, b"old-manifest").expect("写入旧清单快照");
    std::fs::write(&mask_path, b"old-mask").expect("写入旧水域R8快照");
    std::fs::write(&catalog_path, b"old-catalog").expect("写入旧目录快照");

    let result = commit_with_replace_hook(
        vec![
            PreparedWrite::new(manifest_path.clone(), b"new-manifest".to_vec()),
            PreparedWrite::new(custom_water_path.clone(), b"new-custom-water".to_vec()),
            PreparedWrite::new(mask_path.clone(), b"new-mask".to_vec()),
            PreparedWrite::new(catalog_path.clone(), b"new-catalog".to_vec()),
        ],
        |index, target| {
            match index {
                1 => assert_eq!(
                    std::fs::read(target).expect("清单正式路径仍存在"),
                    b"old-manifest"
                ),
                2 => assert!(!target.exists()), // 原本不存在的custom water在自身首次发布前保持缺席。
                3 => assert_eq!(
                    std::fs::read(target).expect("R8正式路径仍存在"),
                    b"old-mask"
                ),
                4 => {
                    assert_eq!(
                        std::fs::read(target).expect("目录正式路径仍存在"),
                        b"old-catalog"
                    );
                    return Err("测试注入：第4个replace失败".to_owned()); // 私有文件已发布，最后catalog覆盖前中断。
                }
                _ => unreachable!("测试只包含四个正式目标"),
            }
            Ok(())
        },
    );

    let failure = result.expect_err("第4个replace故障必须让整批事务失败");
    assert_eq!(
        failure.rollback_targets(),
        [
            manifest_path.clone(),
            custom_water_path.clone(),
            mask_path.clone()
        ]
    ); // 主线程只需显式reload确实发布后回滚的前三个正式路径，失败catalog从未变更。
    assert!(failure.rollback_complete()); // 全部旧目标恢复成功才允许主线程建立回滚版本Ready门。
    let save_failure = DraftSaveError::from_transaction(&temporary.path, failure);
    assert!(save_failure.rollback_complete());
    assert_eq!(
        save_failure.reload_asset_paths(),
        [
            "scenes/MAP-transaction/manifest.scene.json",
            "scenes/MAP-transaction/custom-water.scene_water.json",
            "scenes/MAP-transaction/water/lake.r8"
        ]
    ); // IO线程只返回assets相对正式路径，不会把stage/backup内部名泄露给AssetServer。
    assert_eq!(
        std::fs::read(&manifest_path).expect("清单已恢复"),
        b"old-manifest"
    );
    assert_eq!(std::fs::read(&mask_path).expect("R8已恢复"), b"old-mask");
    assert_eq!(
        std::fs::read(&catalog_path).expect("目录保持旧值"),
        b"old-catalog"
    );
    assert!(!custom_water_path.exists()); // 本事务新建且已经发布的实际custom water目标必须回滚删除。
    let mut artifacts = Vec::new();
    collect_transaction_artifacts(&temporary.path, &mut artifacts);
    assert!(
        artifacts.is_empty(),
        "事务内部stage/backup不应残留：{artifacts:?}"
    );
}

#[test]
fn deferred_encoding_failure_keeps_every_formal_target_untouched() {
    let temporary = SaveTransactionTestDirectory::new();
    let first_path = temporary.path.join("first.scene.json");
    let failed_path = temporary.path.join("failed.ktx2");
    std::fs::write(&first_path, b"old-first").expect("写入第一个旧正式目标");
    std::fs::write(&failed_path, b"old-failed").expect("写入第二个旧正式目标");

    let result = commit_with_replace_hook(
        vec![
            PreparedWrite::new(first_path.clone(), b"new-first".to_vec()),
            PreparedWrite::deferred(failed_path.clone(), || {
                Err("测试注入：第二项延迟KTX2编码失败".to_owned())
            }),
        ],
        |_, _| unreachable!("任一编码失败后不允许进入正式replace阶段"),
    );

    let failure = result.expect_err("延迟编码故障必须拒绝整批提交");
    assert!(failure.rollback_targets().is_empty()); // 只产生过内部stage，主线程无需reload正式资产。
    assert!(failure.rollback_complete()); // 正式发布未开始同样属于完整旧磁盘状态。
    assert_eq!(
        std::fs::read(&first_path).expect("首目标保持旧值"),
        b"old-first"
    );
    assert_eq!(
        std::fs::read(&failed_path).expect("失败目标保持旧值"),
        b"old-failed"
    );
    let mut artifacts = Vec::new();
    collect_transaction_artifacts(&temporary.path, &mut artifacts);
    assert!(
        artifacts.is_empty(),
        "编码失败必须清除先前stage：{artifacts:?}"
    );
}

#[test]
fn staged_blend_derivation_reads_new_glb_before_either_target_is_published() {
    let temporary = SaveTransactionTestDirectory::new(); // 每次测试使用唯一系统临时根。
    let glb_path = temporary.path.join("terrain.glb"); // 模拟正式动态地形目标。
    let blend_path = temporary.path.join("terrain.blend"); // 模拟同名Blender镜像目标。
    std::fs::write(&glb_path, b"old-glb").expect("写入旧GLB"); // 正式路径在stage期间必须保持旧值。
    std::fs::write(&blend_path, b"old-blend").expect("写入旧Blend"); // 直接覆盖仍须经过backup与原子替换。

    let result = commit_with_replace_hook(
        vec![
            PreparedWrite::new(glb_path.clone(), b"new-glb".to_vec()), // 第一项先形成完整新GLB stage。
            PreparedWrite::from_staged_file(
                blend_path.clone(), // 第二项正式目标为同名Blend。
                glb_path.clone(),   // 依赖第一项尚未发布的GLB stage。
                |source_stage, output_stage| {
                    let source = std::fs::read(source_stage)
                        .map_err(|error| format!("读取测试GLB stage失败：{error}"))?; // 模拟Blender只读取新GLB。
                    if source != b"new-glb" {
                        return Err("Blend派生没有读取本批新GLB stage".to_owned()); // 旧正式GLB不得成为输入。
                    }
                    std::fs::write(output_stage, b"blend-from-new-glb")
                        .map_err(|error| format!("写入测试Blend stage失败：{error}"))?; // 模拟外部工具直接生成大型文件。
                    Ok(()) // 非空普通文件由事务再次校验并sync。
                },
            ),
        ],
        |_, target| {
            let expected = if target == glb_path {
                b"old-glb".as_slice() // GLB自身replace前仍为旧值。
            } else {
                b"old-blend".as_slice() // Blend自身replace前同样仍有旧正式文件。
            };
            assert_eq!(std::fs::read(target).expect("正式目标始终存在"), expected); // 全部stage完成不等于提前发布。
            Ok(()) // 允许两项依次原子提交。
        },
    );
    if let Err(failure) = result {
        panic!("GLB和依赖Blend应共同提交成功：{}", failure.message()); // TransactionFailure无需为测试暴露内部Debug结构。
    }

    assert_eq!(std::fs::read(&glb_path).expect("读取新GLB"), b"new-glb"); // 正式GLB采用本批值。
    assert_eq!(
        std::fs::read(&blend_path).expect("读取新Blend"),
        b"blend-from-new-glb"
    ); // 正式Blend必须来自同批新GLB stage。
}

#[test]
fn staged_blend_failure_leaves_old_glb_and_blend_untouched() {
    let temporary = SaveTransactionTestDirectory::new(); // 每次测试使用唯一系统临时根。
    let glb_path = temporary.path.join("terrain.glb"); // 模拟正式动态地形目标。
    let blend_path = temporary.path.join("terrain.blend"); // 模拟已有可覆盖Blend目标。
    std::fs::write(&glb_path, b"old-glb").expect("写入旧GLB"); // 失败后必须逐字节保留。
    std::fs::write(&blend_path, b"old-blend").expect("写入旧Blend"); // 失败后必须逐字节保留。

    let failure = commit_with_replace_hook(
        vec![
            PreparedWrite::new(glb_path.clone(), b"new-glb".to_vec()), // 新GLB stage先成功。
            PreparedWrite::from_staged_file(
                blend_path.clone(), // 当前外部派生目标。
                glb_path.clone(),   // 已完成新GLB stage依赖。
                |_, output_stage| {
                    std::fs::write(output_stage, b"partial-blend")
                        .map_err(|error| format!("写入部分Blend失败：{error}"))?; // 模拟Blender崩溃前留下部分文件。
                    Err("测试注入：Blender转换失败".to_owned()) // 事务必须删除部分stage并停止发布。
                },
            ),
        ],
        |_, _| unreachable!("Blend stage失败后不允许开始正式replace"), // 任何正式路径都不能短暂出现新版本。
    )
    .expect_err("Blender转换失败必须拒绝整批提交");

    assert!(failure.rollback_targets().is_empty()); // 正式发布从未开始，无需AssetServer回滚重载。
    assert_eq!(std::fs::read(&glb_path).expect("读取旧GLB"), b"old-glb"); // 新GLB只存在过内部stage。
    assert_eq!(
        std::fs::read(&blend_path).expect("读取旧Blend"),
        b"old-blend"
    ); // 直接覆盖授权不允许失败时留下部分Blend。
    let mut artifacts = Vec::new(); // 收集所有事务内部stage、backup和外部派生文件。
    collect_transaction_artifacts(&temporary.path, &mut artifacts); // 遍历唯一测试根。
    assert!(artifacts.is_empty(), "失败不得残留事务文件：{artifacts:?}"); // 后续保存不会被旧唯一名阻塞。
}

#[test]
fn r16_encoder_is_little_endian_and_complete() {
    assert_eq!(encode_heights(&[0x1234, 0xABCD]), [0x34, 0x12, 0xCD, 0xAB]);
}

#[test]
fn water_handoff_baseline_includes_generations_observed_during_save_io() {
    assert_eq!(water_handoff_generation_baseline(7, 11), 11);
    assert_eq!(water_handoff_generation_baseline(13, 11), 13);
} // 建门后必须看到严格大于11或13的新代次，部分文件热更不能提前放行最终交接。

#[test]
fn custom_catalog_water_path_is_the_actual_save_target() {
    let target = resolve_water_json_output_path(
        Path::new("assets"),
        "scenes/MAP-custom-water",
        "scenes/MAP-custom-water/custom.scene_water.json",
        true,
    )
    .expect("同一私有目录内的旧自定义路径应继续合法")
    .expect("存在水域时应返回写入目标");

    assert_eq!(
        target,
        Path::new("assets/scenes/MAP-custom-water/custom.scene_water.json")
    );
    assert!(
        resolve_water_json_output_path(
            Path::new("assets"),
            "scenes/MAP-custom-water",
            "scenes/MAP-other/custom.scene_water.json",
            true,
        )
        .is_err()
    ); // 自定义文件名兼容不等于允许跨场景写入。
}

#[test]
fn incomplete_water_network_is_rejected_by_zero_write_preflight() {
    let scene_id = SceneId::parse("MAP-water-preflight").expect("测试场景ID合法");
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-preflight");
    water.rivers.push(SceneWaterRiver {
        id: "unfinished-river".to_owned(),
        label: "尚未完成的河流".to_owned(),
        source_layer_id: Some(water.layers[0].id.clone()),
        target_layer_id: None,
        points: Vec::new(), // F12允许暂存空路径，但正式保存必须在任何文件写入前拒绝。
        base_color_srgba: [0.08, 0.22, 0.32, 0.78],
        perceptual_roughness: 0.18,
    });

    let result = prepare_water_files(
        Path::new("assets"),
        &scene_id,
        "scenes/MAP-water-preflight",
        Some(&water),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    );

    assert!(result.is_err()); // 纯预检不接触文件系统，错误不可能留下半份场景文件。
}

#[test]
fn aliased_water_outputs_are_rejected_by_zero_write_preflight() {
    let scene_id = SceneId::parse("MAP-water-path-alias").expect("测试场景ID合法");
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-path-alias");
    water.layers[0].mask_texture_path = water.layers[0].mask_source_path.clone();
    assert!(
        prepare_water_files(
            Path::new("assets"),
            &scene_id,
            "scenes/MAP-water-path-alias",
            Some(&water),
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .is_err()
    ); // 同层source==texture必须在首个文件写入前失败。

    water =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-path-alias");
    water.layers[0].mask_source_path = "scenes/MAP-water-path-alias/water/./lake.r8".to_owned();
    assert!(
        prepare_water_files(
            Path::new("assets"),
            &scene_id,
            "scenes/MAP-water-path-alias",
            Some(&water),
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .is_err()
    ); // `./`词法别名不能绕过正式路径身份。

    water =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-path-alias");
    let mut pond = water.layers[0].clone();
    pond.id = "pond".to_owned();
    pond.mask_texture_path = "scenes/MAP-water-path-alias/water/pond.ktx2".to_owned();
    water.layers.push(pond); // 两个稳定层ID仍不能共享同一raw R8作者文件。
    assert!(
        prepare_water_files(
            Path::new("assets"),
            &scene_id,
            "scenes/MAP-water-path-alias",
            Some(&water),
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .is_err()
    );
}

#[test]
fn non_water_asset_cannot_overwrite_even_a_clean_water_layer() {
    let scene_id = SceneId::parse("MAP-water-cross-domain").expect("测试场景ID合法");
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-cross-domain");
    water.layers[0].resolution = [2, 2];
    let source_path = Path::new("assets").join(&water.layers[0].mask_source_path);
    let reserved = BTreeSet::from([asset_path_collision_key(&source_path)]); // 模拟地形或植被声明了同一目标。

    let result = prepare_water_files(
        Path::new("assets"),
        &scene_id,
        "scenes/MAP-water-cross-domain",
        Some(&water),
        &BTreeSet::new(), // 水层本身未改也必须参与全事务碰撞保护。
        &BTreeMap::from([(water.layers[0].id.clone(), Arc::new(vec![0; 4]))]),
        &reserved,
    );

    assert!(result.is_err()); // 任何其他dirty输出都不能静默覆盖仍由正式水域引用的raw R8。
}

#[test]
fn water_output_cannot_escape_the_catalog_entry_private_folder() {
    let scene_id = SceneId::parse("MAP-water-owner-a").expect("测试场景ID合法");
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-owner-a");
    water.layers[0].mask_source_path = "scenes/MAP-water-owner-b/water/lake.r8".to_owned();
    water.layers[0].mask_texture_path = "scenes/MAP-water-owner-b/water/lake.ktx2".to_owned();

    let result = prepare_water_files(
        Path::new("assets"),
        &scene_id,
        "scenes/MAP-water-owner-a",
        Some(&water),
        &BTreeSet::from([water.layers[0].id.clone()]),
        &BTreeMap::from([(water.layers[0].id.clone(), Arc::new(vec![0; 256 * 256]))]),
        &BTreeSet::new(),
    );

    assert!(result.is_err()); // 手工JSON不能借一次场景A保存覆盖场景B的作者源或派生纹理。
}

#[test]
fn runtime_large_spline_is_accepted_by_zero_write_preflight() {
    let scene_id = SceneId::parse("MAP-water-budget").expect("测试场景ID合法");
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-budget");
    water.rivers.push(SceneWaterRiver {
        id: "oversized-river".to_owned(),
        label: "跨越旧截面预算的合法河流".to_owned(),
        source_layer_id: Some(water.layers[0].id.clone()),
        target_layer_id: None,
        points: vec![
            SceneWaterSplinePoint {
                id: "start".to_owned(),
                position: [0.0, water.layers[0].water_level, 0.0],
                width: 6.0,
                depth: 1.5,
                flow_speed: 2.0,
                outgoing: SceneWaterSegmentKind::River,
            },
            SceneWaterSplinePoint {
                id: "end".to_owned(),
                position: [5_000.0, water.layers[0].water_level - 1.0, 0.0],
                width: 6.0,
                depth: 1.5,
                flow_speed: 2.0,
                outgoing: SceneWaterSegmentKind::River,
            },
        ], // 控制点和schema合法；2m采样超过旧2048截面阈值，扩容后仍须允许完整作者内容。
        base_color_srgba: [0.08, 0.22, 0.32, 0.78],
        perceptual_roughness: 0.18,
    });
    assert!(water.validate(&scene_id).is_ok()); // 大范围样条先通过完整schema校验，再进入零写入预检。

    let result = prepare_water_files(
        Path::new("assets"),
        &scene_id,
        "scenes/MAP-water-budget",
        Some(&water),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    );

    assert!(result.is_ok()); // 水体扩容提交已移除固定截面数量门；合法长河不能在保存前被旧阈值拒绝。
}

#[test]
fn saved_authoring_replaces_the_stale_loaded_asset_before_a_scene_switch() {
    let scene_id = SceneId::parse("MAP-test").expect("测试场景ID合法"); // 模拟刚保存的新建动态场景。
    let mut app = App::new(); // 真实Assets会在AssetMut释放时产生Modified事件。
    app.add_plugins((MinimalPlugins, AssetPlugin::default())) // 提供资产句柄分配器与任务池。
        .init_asset::<SceneVegetationAsset>(); // 只初始化本回归所需的植被资产集合。
    let handle = app
        .world_mut()
        .resource_mut::<Assets<SceneVegetationAsset>>()
        .add(SceneVegetationAsset {
            model_surfaces: Vec::new().into(),
            schema_version: SCENE_SCHEMA_VERSION, // 旧缓存仍是同版本合法文件。
            scene_id: scene_id.clone(),             // 旧缓存属于当前场景。
            layers: Vec::new(),                 // 本测试只比较作者结构发布。
            authoring: None,                    // 模拟保存前仍缓存旧作者结构。
        });
    let registry = SceneContentRegistry {
        catalog: Handle::<SceneCatalogAsset>::default(), // helper只读取场景句柄表。
        revision: 1,                                   // 表示目录已完整应用。
        scenes: BTreeMap::from([(
            scene_id.clone(), // 唯一场景映射到上方同一植被强句柄。
            SceneContentHandles {
                manifest: Handle::<SceneManifestAsset>::default(), // 测试不读取清单。
                portals: Handle::<ScenePortalsAsset>::default(),   // 测试不读取传送数据。
                placements: Handle::<ScenePlacementsAsset>::default(), // 测试不读取摆放数据。
                surface: None::<Handle<SceneSurfaceAsset>>,        // 测试不包含地表。
                vegetation: Some(handle.clone()),                // 保存结果必须替换该唯一正式句柄。
                water: None::<Handle<SceneWaterAsset>>,            // 测试不包含河水。
                characters_path: None,                                // 旧测试场景没有角色作者源。
                boundaries: None,                                // 测试不包含区域边界。
            },
        )]),
        last_error: None, // 目录当前有效。
    };
    let saved = SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: SCENE_SCHEMA_VERSION, // 保存快照仍使用当前格式。
        scene_id: scene_id.clone(),             // 保存结果身份与目录目标一致。
        layers: Vec::new(),                 // 本测试只锁定作者结构缓存回归。
        authoring: Some(SceneVegetationAuthoring::default()), // Some代表刚保存的新作者结构。
    };

    let published = publish_saved_vegetation_asset(
        Some(&registry), // 使用真实目录唯一句柄。
        &scene_id,         // 只替换当前保存场景。
        &saved,          // 模拟IO任务已经完整落盘的快照。
        &mut app.world_mut().resource_mut::<Assets<SceneVegetationAsset>>(), // 主世界资产在切换场景前立即同步。
    );

    assert!(published); // 已加载句柄必须同步成功而不是等待文件监听。
    assert!(
        app.world()
            .resource::<Assets<SceneVegetationAsset>>()
            .get(&handle)
            .expect("正式植被句柄仍存在")
            .authoring
            .is_some()
    ); // 再次进入场景时将读取新作者结构，不会恢复保存前缓存。
}

#[test]
fn saved_water_network_replaces_the_stale_loaded_asset_before_hot_reload() {
    let scene_id = SceneId::parse("MAP-water-test").expect("测试场景ID合法");
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<SceneWaterAsset>();
    let mut stale =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-test");
    stale.layers[0].label = "保存前湖泊".to_owned();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<SceneWaterAsset>>()
        .add(stale);
    let registry = SceneContentRegistry {
        catalog: Handle::<SceneCatalogAsset>::default(),
        revision: 1,
        scenes: BTreeMap::from([(
            scene_id.clone(),
            SceneContentHandles {
                manifest: Handle::<SceneManifestAsset>::default(),
                portals: Handle::<ScenePortalsAsset>::default(),
                placements: Handle::<ScenePlacementsAsset>::default(),
                surface: None,
                vegetation: None,
                water: Some(handle.clone()),
                characters_path: None, // 旧测试场景没有角色作者源。
                boundaries: None,
            },
        )]),
        last_error: None,
    };
    let mut saved =
        crate::authoring::water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-test");
    saved.layers[0].label = "保存后湖泊与水网".to_owned();

    let published = publish_saved_water_asset(
        Some(&registry),
        &scene_id,
        &saved,
        &mut app.world_mut().resource_mut::<Assets<SceneWaterAsset>>(),
    );

    assert!(published);
    assert_eq!(
        app.world()
            .resource::<Assets<SceneWaterAsset>>()
            .get(&handle)
            .expect("正式水域句柄仍存在")
            .layers[0]
            .label,
        "保存后湖泊与水网"
    );
}

/// 隔离IO完成边界，仍由真实poll决定建门、失败恢复和预览清理；不写入工作区场景。
fn completed_placement_save_app(
    gate_failure: bool,
    placements_changed: bool,
    active_saved_scene: bool,
) -> (App, SceneId, Entity, Entity) {
    use crate::authoring::resources::{PlacementPreviewHandoffSnapshot, PlacementSaveHandoffSnapshot, WaterSaveHandoffSnapshot};
    use crate::components::EditorPlacedAsset;

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<SceneManifestAsset>()
        .init_asset::<ScenePlacementsAsset>()
        .init_asset::<ScenePortalsAsset>()
        .init_asset::<SceneSurfaceAsset>()
        .init_asset::<SceneVegetationAsset>()
        .init_asset::<SceneWaterAsset>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<TerrainExternalChangeState>()
        .init_resource::<DynamicWaterStatus>()
        .init_resource::<ScenePlacementRuntimeStatus>()
        .init_resource::<feathrise_scene::ScenePortalRuntimeStatus>()
        .init_resource::<PortalPreviewHandoff>()
        .init_resource::<WaterFileHandoff>()
        .add_message::<DynamicWaterRebuildRequest>()
        .add_message::<ScenePlacementRebuildRequest>();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.scene_id.clone();
    let mut record = crate::viewport::placement_move::interaction_tests::record();
    record.id = "save-gate-preview".into();
    draft.placements.placements = vec![record.clone()];
    if gate_failure {
        draft.water = Some(crate::authoring::water_asset_with_layer_for_tests(&map, &draft.folder));
    }
    let root = app.world_mut().spawn((EditorPlacedAsset, Visibility::Inherited,
        WorldAssetRoot(Handle::default()), feathrise_scene::ScenePlacementInstance::new(&map, &record))).id();
    let child = app.world_mut().spawn((EditorPlacedAsset, Visibility::Inherited, ChildOf(root))).id();
    let preview = PlacementPreviewHandoffSnapshot::capture(app.world(), &map, &record.id, root)
        .expect("保存完成夹具冻结真实作者预览所有权");
    let manifest = app.world_mut().resource_mut::<Assets<SceneManifestAsset>>().add(draft.manifest.clone());
    let placements = app.world_mut().resource_mut::<Assets<ScenePlacementsAsset>>().add(draft.placements.clone());
    let portals = app.world_mut().resource_mut::<Assets<ScenePortalsAsset>>().add(draft.portals.clone());
    let water = draft.water.clone().map(|water| app.world_mut().resource_mut::<Assets<SceneWaterAsset>>().add(water));
    app.insert_resource(SceneContentRegistry {
        catalog: Handle::default(), revision: 1,
        scenes: BTreeMap::from([(map.clone(), SceneContentHandles { manifest, placements, portals,
            surface: None, vegetation: None, water, characters_path: None, boundaries: None })]),
        last_error: None,
    }); // 已加载的唯一正式句柄使回归不依赖文件监听或真实资产文件。
    let revision = 7;
    let water_handoff_snapshot = gate_failure.then(|| WaterSaveHandoffSnapshot {
        scene_id: map.clone(), water_revision: 0, root_water_revision: 0, terrain_revision: 0,
        baseline_generation: 0, manifest_asset_path: String::new(),
        water_asset_path: draft.water_asset_path.clone(), dirty_layers: Vec::new(),
        dirty_height_path: None, catalog_registration_required: false, terrains: Vec::new(),
    }); // 真实begin因缺失清单路径拒绝建门；摆放内容本身仍要求严格新Ready交接。
    let success = DraftSaveSuccess {
        terrain_manifest: None, terrain_saved: Vec::new(), terrain_reload_paths: Vec::new(),
        terrain_digests: Vec::new(), terrain_water_changed: BTreeSet::new(),
        root_water_changed: gate_failure, domain_hash: None, scene_id: map.clone(),
        placements_changed, surface_definition_changed: false, surface_weights_changed: false,
        active_terrain_surface_changes: None, surface_source_identity: None,
        placement_glb_hash: None, global_environment_saved: false,
    };
    app.insert_resource(EditorAuthoringSession { draft: Some(draft), revision, saving: true,
        placement_previews: BTreeMap::from([(record.id.clone(), root)]), ..default() })
        .insert_resource(SceneEditorState { open: true, dirty: true, close_requested: true,
            selected_scene_id: Some(map.clone()), ..default() })
        .insert_resource(ActiveDynamicScene { scene_id: Some(if active_saved_scene { map.clone() }
            else { SceneId::parse("MAP-other-save-gate").unwrap() }) })
        .insert_resource(DraftSaveTask {
            ticket: 1,
            task: Some(bevy::tasks::IoTaskPool::get().spawn(async move { (1, revision, Ok(success)) })),
            water_handoff_snapshot,
            placement_handoff_snapshot: Some(PlacementSaveHandoffSnapshot {
                scene_id: map.clone(), baseline_revision: 0, baseline_attempt_revision: 0,
                retry_failed_runtime: false, editor_previews: vec![preview], deleted: Vec::new(),
            }),
            ..default()
        });
    (app, map, root, child)
}

fn consume_completed_placement_save(app: &mut App) {
    use bevy::ecs::system::RunSystemOnce;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.world().resource::<DraftSaveTask>().task.is_some() {
        app.world_mut().run_system_once(poll_draft_save).unwrap();
        assert!(std::time::Instant::now() < deadline, "保存完成结果未被真实poll消费");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn placement_save_gate_failure_keeps_retry_preview_and_never_queues_cleanup() {
    use bevy::ecs::system::RunSystemOnce;
    for placements_changed in [true, false] {
        // 水域建门失败会使整批保存进入重试态，摆放字节未变也不能误走无门清理。
        let (mut app, map, root, child) = completed_placement_save_app(true, placements_changed, true);
        consume_completed_placement_save(&mut app);
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.dirty && !editor.close_requested);
        assert!(editor.status.contains("清单或水域JSON路径为空"));
        assert!(!app.world().resource::<EditorAuthoringSession>().saving);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 0);
        let handoff = app.world().resource::<WaterFileHandoff>();
        assert_eq!(handoff.placement_retry_required_for(&map), placements_changed);
        assert!(handoff.pending_for(&map, 0, 0));
        assert!(!handoff.placement_preview_cleanup_pending(), "建门失败时不能走合法无门清理");
        assert!(!handoff.blocks_new_save_for(Some(&map)), "同图应保留修复后重新保存的入口");
        for _ in 0..4 {
            app.world_mut().run_system_once(crate::authoring::cleanup_placement_handoff_previews).unwrap();
        }
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
        assert_eq!(app.world().get::<Visibility>(child), Some(&Visibility::Inherited));
        assert!(app.world().get::<WorldAssetRoot>(root).is_some());
        assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(root));
        assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews.get("save-gate-preview"), Some(&root));
    }
}

#[test]
fn placement_save_valid_no_gate_paths_still_cleanup_saved_preview() {
    use bevy::ecs::system::RunSystemOnce;
    for (placements_changed, active_saved_scene) in [(false, true), (true, false)] {
        let (mut app, map, root, child) = completed_placement_save_app(false, placements_changed, active_saved_scene);
        consume_completed_placement_save(&mut app);
        assert!(app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending(),
            "语义未变或非当前玩法场景仍应走合法无门回收");
        for _ in 0..4 {
            app.world_mut().run_system_once(crate::authoring::cleanup_placement_handoff_previews).unwrap();
        }
        assert!(app.world().get_entity(root).is_err() && app.world().get_entity(child).is_err());
        assert!(app.world().resource::<EditorAuthoringSession>().placement_previews.is_empty());
        assert!(!app.world().resource::<WaterFileHandoff>().pending_for(&map, 0, 0));
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 1);
    }
}

// Clipmap植被补登记复用隔离事务夹具，覆盖只读打开、像素保留和并发保护。
fn clipmap_registration_fixture(root: &Path) -> super::super::resources::EditorSceneDraft {
    // 不访问工作区真实场景。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 复用完整作者域夹具。
    draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap; // 本轮目标地形路线。
    draft.manifest.terrain.glb_path.clear(); // Clipmap无整图GLB高度依赖。
    draft.heights = None; // 植被创建与加载不能再要求GLB派生数组。
    let catalog = serde_json::json!({"schema_version": SCENE_SCHEMA_VERSION, "scenes": [{"id": draft.scene_id, "display_name": "旧Clipmap", "folder": draft.folder, "published": true}]}); // 精确模拟旧图遗漏vegetation_path。
    std::fs::create_dir_all(root.join(&draft.folder)).unwrap(); // 仅创建隔离临时场景目录。
    std::fs::write(
        root.join(SCENE_CATALOG_ASSET_PATH),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .unwrap(); // 目录基线供联合保存守卫检查。
    draft // 草稿尚未声明任何自动迁移责任。
} // 每个测试独享根目录与文件版本。

#[test] // 缺失私有文件是唯一允许建立空文档的情况。
fn clipmap_missing_vegetation_opens_without_writes_and_registers_on_save() {
    // 验证读写边界。
    let temporary = SaveTransactionTestDirectory::new(); // RAII清理隔离临时根。
    let mut draft = clipmap_registration_fixture(&temporary.path); // 无GLB与整图高度输入。
    let path = temporary
        .path
        .join(&draft.folder)
        .join(SCENE_VEGETATION_FILE_NAME); // 与生产协议一致。
    crate::authoring::vegetation_load::load_unregistered_clipmap_vegetation(
        &temporary.path,
        &mut draft,
    )
    .unwrap(); // IO加载只创建内存作者组。
    assert!(!path.exists()); // 打开绝不隐式改盘。
    assert!(draft.vegetation.as_ref().unwrap().layers.is_empty()); // 不预分配目录全集。
    assert_eq!(
        draft.vegetation_registration_source_hash.as_deref(),
        Some("missing")
    ); // 首次保存要核对原文件仍然不存在。
    let (_, bytes) = super::write_helpers::prepare_optional_catalog_bytes(&temporary.path, &draft)
        .unwrap()
        .unwrap(); // 使用生产联合事务目录编码。
    let catalog: SceneCatalogAsset = serde_json::from_slice(&bytes).unwrap(); // 验证真实序列化协议。
    assert_eq!(
        catalog.scenes[0].vegetation_path.as_deref(),
        Some("scenes/EDGE-TEST/vegetation.scene_vegetation.json")
    ); // 新入口与正式文件相同。
    let session = EditorAuthoringSession {
        draft: Some(draft),
        ..Default::default()
    }; // 无撤销历史也要保存首次登记。
    assert!(session.has_scene_changes()); // F12关闭不能吞掉迁移责任。
    assert!(session.has_non_sparse_scene_changes()); // 稀疏高度单独保存不能提前结束整批保存。
} // 尚未启动任何真实资产加载器。

#[test] // 有真实内容的旧图必须保留作者结构和源像素。
fn clipmap_unregistered_vegetation_preserves_formal_r8_and_migrates_authoring() {
    // 验证无损一对一兼容。
    let temporary = SaveTransactionTestDirectory::new(); // 隔离全部文件操作。
    let mut draft = clipmap_registration_fixture(&temporary.path); // 当前没有植被注册句柄。
    let layer: feathrise_scene_content::SceneVegetationLayer =
        serde_json::from_value(serde_json::json!({ // 构造最小可编辑Blade正式层。
            "id": "grass", "label": "草地", "kind": "blade", "runtime_profile": "hn01_ordinary_grass", // 保留历史预设别名。
            "asset_path": "gltf/grass.glb", "density_source_path": "scenes/EDGE-TEST/grass.r8", "density_texture_path": "scenes/EDGE-TEST/grass.ktx2", // 正式源不改路径。
            "resolution": [2,2], "spacing_meters": 1.0, "scale_range": [1.0,1.0], "seed": 9 // 四像素即可验证真实数据保留。
        }))
        .unwrap(); // 使用公共格式反序列化。
    let vegetation = SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: SCENE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        layers: vec![layer],
        authoring: None,
    }; // 旧版本尚无作者图。
    let path = temporary
        .path
        .join(&draft.folder)
        .join(SCENE_VEGETATION_FILE_NAME); // 标准私有文件。
    let bytes = serde_json::to_vec(&vegetation).unwrap(); // 冻结原文件以检查打开操作没有重写。
    std::fs::write(&path, &bytes).unwrap(); // 模拟真实旧植被JSON。
    std::fs::write(
        temporary.path.join(&draft.folder).join("grass.r8"),
        [0, 37, 128, 255],
    )
    .unwrap(); // 四个不同密度像素。
    let migrated = crate::authoring::vegetation_load::load_unregistered_clipmap_vegetation(
        &temporary.path,
        &mut draft,
    )
    .unwrap(); // 自动迁移只存在于后台草稿。
    assert!(migrated); // 下游必须用正式像素共享作者基线。
    crate::authoring::vegetation_load::load_vegetation_sources(
        &temporary.path,
        &mut draft,
        migrated,
    )
    .unwrap(); // 生产统一源加载严格检查长度。
    assert_eq!(
        draft.vegetation_densities["grass"].as_slice(),
        &[0, 37, 128, 255]
    ); // 正式源不归零。
    assert_eq!(
        draft.vegetation_author_densities["base-grass"].as_slice(),
        &[0, 37, 128, 255]
    ); // 作者图逐像素无损。
    assert!(draft.vegetation_author_dirty_layers.contains("base-grass")); // 首次保存写出独立作者源。
    assert_eq!(std::fs::read(path).unwrap(), bytes); // 载入不改写旧JSON。
} // 固定小夹具不代表大型场景性能测试。

#[test] // 坏JSON不能被视作缺失文件。
fn clipmap_invalid_private_vegetation_is_rejected_without_overwrite() {
    // 保存责任只在完整校验后建立。
    let temporary = SaveTransactionTestDirectory::new(); // 隔离根目录。
    let mut draft = clipmap_registration_fixture(&temporary.path); // 初始无植被文档。
    let path = temporary
        .path
        .join(&draft.folder)
        .join(SCENE_VEGETATION_FILE_NAME); // 唯一标准文件。
    std::fs::write(&path, b"broken-json").unwrap(); // 模拟损坏内容。
    assert!(
        crate::authoring::vegetation_load::load_unregistered_clipmap_vegetation(
            &temporary.path,
            &mut draft
        )
        .is_err()
    ); // 明确失败，不使用空白托底。
    assert!(draft.vegetation.is_none()); // 错误未部分采用。
    assert!(draft.vegetation_registration_source_hash.is_none()); // 不产生覆盖坏文件的首次保存责任。
    assert_eq!(std::fs::read(path).unwrap(), b"broken-json"); // 原文件供用户修复。
} // 错误将由原加载状态栏呈现。

#[test] // 同图入口改向与源新增均必须拒绝覆盖。
fn clipmap_registration_rejects_external_catalog_and_source_changes() {
    // 覆盖打开后与stage期间两类竞争。
    let temporary = SaveTransactionTestDirectory::new(); // 独占临时根。
    let mut draft = clipmap_registration_fixture(&temporary.path); // 旧目录没有登记。
    crate::authoring::vegetation_load::load_unregistered_clipmap_vegetation(
        &temporary.path,
        &mut draft,
    )
    .unwrap(); // 冻结missing基线。
    let guard =
        super::domain_files::DraftSourceSaveGuard::prepare(&temporary.path, &draft).unwrap(); // stage前取得目录版本。
    let path = temporary
        .path
        .join(&draft.folder)
        .join(SCENE_VEGETATION_FILE_NAME); // 生产源目标。
    std::fs::write(&path, b"external-content").unwrap(); // 外部作者在打开后新建文件。
    assert!(guard.check(&path).is_err()); // 正式替换前拒绝覆盖外部新内容。
    assert!(super::domain_files::DraftSourceSaveGuard::prepare(&temporary.path, &draft).is_err()); // 下次重试也不能跳过源版本冲突。
    let catalog_path = temporary.path.join(SCENE_CATALOG_ASSET_PATH); // 完整目录是最后发布项。
    let mut catalog: SceneCatalogAsset =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap(); // 保留其他目录字段。
    catalog.scenes[0].vegetation_path = Some("scenes/EDGE-TEST/custom.scene_vegetation.json".to_owned()); // 外部工具明确改用其他私有文件。
    std::fs::write(&catalog_path, serde_json::to_vec(&catalog).unwrap()).unwrap(); // 模拟目录并发变化。
    assert!(super::write_helpers::prepare_optional_catalog_bytes(&temporary.path, &draft).is_err()); // 不把新路径悄悄改回标准路径。
    assert!(guard.check(&catalog_path).is_err()); // stage期间任何目录变更也会触发整批回滚。
} // 此处只测试拒绝，通用事务套件另行覆盖回滚恢复。

#[test] // 首次植被目录登记与私有JSON沿用真实跨文件事务，失败完整回滚且可重试。
fn clipmap_registration_transaction_rolls_back_then_reloads_after_retry() {
    // 验证首次登记的失败闭环。
    let temporary = SaveTransactionTestDirectory::new(); // 隔离测试文件。
    let mut draft = clipmap_registration_fixture(&temporary.path); // 旧目录只有场景入口。
    crate::authoring::vegetation_load::load_unregistered_clipmap_vegetation(
        &temporary.path,
        &mut draft,
    )
    .unwrap(); // 建立missing版本基线。
    let vegetation_path = temporary
        .path
        .join(&draft.folder)
        .join(SCENE_VEGETATION_FILE_NAME); // 本批首先发布的私有JSON。
    let catalog_path = temporary.path.join(SCENE_CATALOG_ASSET_PATH); // 本批最后发布的全局目录。
    let original_catalog = std::fs::read(&catalog_path).unwrap(); // 回滚逐字节校验。
    for fail in [true, false] {
        // 首次失败后不丢草稿，第二次执行同一保存责任。
        let guard =
            super::domain_files::DraftSourceSaveGuard::prepare(&temporary.path, &draft).unwrap(); // 生产原字节冲突门。
        let (_, catalog_bytes) =
            super::write_helpers::prepare_optional_catalog_bytes(&temporary.path, &draft)
                .unwrap()
                .unwrap(); // 生产目录合并逻辑。
        let writes = vec![
            PreparedWrite::json(vegetation_path.clone(), draft.vegetation.as_ref().unwrap()),
            PreparedWrite::new(catalog_path.clone(), catalog_bytes),
        ]; // 标准私有文件先于目录入口。
        let result = super::transaction::commit_prepared_writes_checked(writes, |_, path| {
            // 与save_draft使用同一生产事务。
            guard.check(path)?; // 每个正式替换前核对源与目录。
            if fail && path == catalog_path {
                return Err("测试注入目录发布失败".to_owned());
            } // 私有JSON已替换后触发失败。
            Ok(()) // 第二次完整通过。
        }); // 失败时事务自动逆序回滚。
        if fail {
            // 检查首次创建的JSON也被回收。
            assert!(result.unwrap_err().rollback_complete()); // 所有已发布目标恢复。
            assert!(!vegetation_path.exists()); // 不残留半套植被文件。
            assert_eq!(std::fs::read(&catalog_path).unwrap(), original_catalog); // 目录仍是原始字节。
            assert!(draft.vegetation_registration_source_hash.is_some()); // 失败不能结清补登记责任。
        } else {
            result.unwrap_or_else(|failure| panic!("重试失败：{}", failure.message()));
        } // 重试成功。
    } // 磁盘此时只包含同一完整版本。
    let catalog: SceneCatalogAsset =
        serde_json::from_slice(&std::fs::read(catalog_path).unwrap()).unwrap(); // 重开后正式目录可找到新文件。
    let published = temporary
        .path
        .join(catalog.scenes[0].vegetation_path.as_ref().unwrap()); // 使用实际登记入口。
    let vegetation: SceneVegetationAsset =
        serde_json::from_slice(&std::fs::read(published).unwrap()).unwrap(); // 使用正式反序列化协议重载。
    assert_eq!(vegetation.scene_id, draft.scene_id); // 场景身份保持一致。
    assert!(vegetation.authoring.is_some()); // 新建作者层入口可恢复。
} // 真实GPU采用和目标硬件画面另由运行时验证。

#[test]
fn clipmap_missing_water_opens_without_writes_and_registers_on_save() {
    let temporary = SaveTransactionTestDirectory::new();
    let mut draft = clipmap_registration_fixture(&temporary.path);
    let path = temporary
        .path
        .join(&draft.folder)
        .join(feathrise_scene_content::SCENE_WATER_FILE_NAME);
    crate::authoring::water_load::load_unregistered_clipmap_water(&temporary.path, &mut draft)
        .unwrap();
    assert!(!path.exists());
    assert!(draft.water.as_ref().unwrap().layers.is_empty());
    assert_eq!(
        draft.water_registration_source_hash.as_deref(),
        Some("missing")
    );
    let (_, bytes) = super::write_helpers::prepare_optional_catalog_bytes(&temporary.path, &draft)
        .unwrap()
        .unwrap();
    let catalog: SceneCatalogAsset = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        catalog.scenes[0].water_path.as_deref(),
        Some("scenes/EDGE-TEST/water.scene_water.json")
    );
    let session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    };
    assert!(session.has_scene_changes());
    assert!(session.has_non_sparse_scene_changes());
}

#[test]
fn clipmap_unregistered_water_preserves_static_and_spline_content() {
    let temporary = SaveTransactionTestDirectory::new();
    let mut draft = clipmap_registration_fixture(&temporary.path);
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
    water.layers[0].label = "旧湖泊".to_owned();
    let source_water_level = water.layers[0].water_level;
    water.rivers.push(SceneWaterRiver {
        id: "legacy-river".to_owned(),
        label: "只读保留河流".to_owned(),
        source_layer_id: Some(water.layers[0].id.clone()),
        target_layer_id: None,
        points: vec![
            SceneWaterSplinePoint {
                id: "source".to_owned(),
                position: [2.0, source_water_level, 2.0],
                width: 2.0,
                depth: 1.0,
                flow_speed: 1.0,
                outgoing: SceneWaterSegmentKind::River,
            },
            SceneWaterSplinePoint {
                id: "target".to_owned(),
                position: [8.0, source_water_level - 1.0, 8.0],
                width: 2.0,
                depth: 1.0,
                flow_speed: 1.0,
                outgoing: SceneWaterSegmentKind::River,
            },
        ],
        base_color_srgba: [0.08, 0.22, 0.32, 0.78],
        perceptual_roughness: 0.18,
    });
    let path = temporary
        .path
        .join(&draft.folder)
        .join(feathrise_scene_content::SCENE_WATER_FILE_NAME);
    let bytes = serde_json::to_vec(&water).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    crate::authoring::water_load::load_unregistered_clipmap_water(&temporary.path, &mut draft)
        .unwrap();
    assert_eq!(draft.water.as_ref(), Some(&water));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let expected_hash = feathrise_scene_content::authoring_io::bytes_hash(&bytes);
    assert_eq!(
        draft.water_registration_source_hash.as_deref(),
        Some(expected_hash.as_str())
    );
}

#[test]
fn clipmap_invalid_or_foreign_private_water_is_rejected_without_overwrite() {
    let temporary = SaveTransactionTestDirectory::new();
    let path = temporary
        .path
        .join("scenes/EDGE-TEST")
        .join(feathrise_scene_content::SCENE_WATER_FILE_NAME);
    let mut draft = clipmap_registration_fixture(&temporary.path);
    std::fs::write(&path, b"broken-json").unwrap();
    assert!(
        crate::authoring::water_load::load_unregistered_clipmap_water(&temporary.path, &mut draft)
            .is_err()
    );
    assert!(draft.water.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), b"broken-json");

    let foreign_id = SceneId::parse("MAP-foreign-water").unwrap();
    let foreign = crate::authoring::default_water_asset(&foreign_id);
    let foreign_bytes = serde_json::to_vec(&foreign).unwrap();
    std::fs::write(&path, &foreign_bytes).unwrap();
    assert!(
        crate::authoring::water_load::load_unregistered_clipmap_water(&temporary.path, &mut draft)
            .is_err()
    );
    assert!(draft.water.is_none());
    assert!(draft.water_registration_source_hash.is_none());
    assert_eq!(std::fs::read(path).unwrap(), foreign_bytes);
}

#[test]
fn clipmap_water_registration_rejects_external_source_and_catalog_changes() {
    let temporary = SaveTransactionTestDirectory::new();
    let mut draft = clipmap_registration_fixture(&temporary.path);
    crate::authoring::water_load::load_unregistered_clipmap_water(&temporary.path, &mut draft)
        .unwrap();
    let guard =
        super::domain_files::DraftSourceSaveGuard::prepare(&temporary.path, &draft).unwrap();
    let path = temporary
        .path
        .join(&draft.folder)
        .join(feathrise_scene_content::SCENE_WATER_FILE_NAME);
    std::fs::write(&path, b"external-water").unwrap();
    assert!(guard.check(&path).is_err());
    assert!(super::domain_files::DraftSourceSaveGuard::prepare(&temporary.path, &draft).is_err());
    let catalog_path = temporary.path.join(SCENE_CATALOG_ASSET_PATH);
    let mut catalog: SceneCatalogAsset =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    catalog.scenes[0].water_path = Some("scenes/EDGE-TEST/custom.scene_water.json".to_owned());
    std::fs::write(&catalog_path, serde_json::to_vec(&catalog).unwrap()).unwrap();
    assert!(super::write_helpers::prepare_optional_catalog_bytes(&temporary.path, &draft).is_err());
    assert!(guard.check(&catalog_path).is_err());
}

#[test]
fn clipmap_water_registration_transaction_rolls_back_then_reloads_after_retry() {
    let temporary = SaveTransactionTestDirectory::new();
    let mut draft = clipmap_registration_fixture(&temporary.path);
    crate::authoring::water_load::load_unregistered_clipmap_water(&temporary.path, &mut draft)
        .unwrap();
    let water_path = temporary
        .path
        .join(&draft.folder)
        .join(feathrise_scene_content::SCENE_WATER_FILE_NAME);
    let catalog_path = temporary.path.join(SCENE_CATALOG_ASSET_PATH);
    let original_catalog = std::fs::read(&catalog_path).unwrap();
    for fail in [true, false] {
        let guard =
            super::domain_files::DraftSourceSaveGuard::prepare(&temporary.path, &draft).unwrap();
        let (_, catalog_bytes) =
            super::write_helpers::prepare_optional_catalog_bytes(&temporary.path, &draft)
                .unwrap()
                .unwrap();
        let writes = vec![
            PreparedWrite::json(water_path.clone(), draft.water.as_ref().unwrap()),
            PreparedWrite::new(catalog_path.clone(), catalog_bytes),
        ];
        let result = super::transaction::commit_prepared_writes_checked(writes, |_, path| {
            guard.check(path)?;
            if fail && path == catalog_path {
                return Err("测试注入水域目录发布失败".to_owned());
            }
            Ok(())
        });
        if fail {
            assert!(result.unwrap_err().rollback_complete());
            assert!(!water_path.exists());
            assert_eq!(std::fs::read(&catalog_path).unwrap(), original_catalog);
            assert!(draft.water_registration_source_hash.is_some());
        } else {
            result.unwrap_or_else(|failure| panic!("水域登记重试失败：{}", failure.message()));
        }
    }
    let catalog: SceneCatalogAsset =
        serde_json::from_slice(&std::fs::read(catalog_path).unwrap()).unwrap();
    let published = temporary
        .path
        .join(catalog.scenes[0].water_path.as_ref().unwrap());
    let water: SceneWaterAsset = serde_json::from_slice(&std::fs::read(published).unwrap()).unwrap();
    assert_eq!(water.scene_id, draft.scene_id);
    assert!(water.layers.is_empty());
}

#[test]
fn ordinary_catalog_save_preserves_uninterpreted_game_document_and_custom_path() {
    let temporary = SaveTransactionTestDirectory::new();
    let mut draft = clipmap_registration_fixture(&temporary.path);
    let catalog_path = temporary.path.join(SCENE_CATALOG_ASSET_PATH);
    let mut catalog: SceneCatalogAsset = serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    let role_path = format!("{}/custom-game-actors.json", draft.folder);
    catalog.scenes[0].characters_path = Some(role_path.clone());
    std::fs::write(&catalog_path, serde_json::to_vec(&catalog).unwrap()).unwrap();
    let original = br#"{ "private_game_schema": 998, "actors": [{ "skill": "untouched" }] }"#;
    let source_path = temporary.path.join(&role_path);
    std::fs::write(&source_path, original).unwrap();
    draft.domain = None; // 编辑器没有安装原游戏作者适配，不读取或迁移这份未知正文。
    draft.preserved_asset_paths = vec![role_path.clone()].into();
    draft.boundary_catalog_registration_required = true;
    let (target, bytes) = super::write_helpers::prepare_optional_catalog_bytes(&temporary.path, &draft).unwrap().unwrap();
    super::transaction::commit_owned_files(vec![(target, bytes)]).unwrap();
    let saved: SceneCatalogAsset = serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    assert_eq!(saved.scenes[0].characters_path.as_deref(), Some(role_path.as_str()));
    assert_eq!(std::fs::read(&source_path).unwrap(), original);
    draft.water = Some(crate::authoring::water_defaults::default_water_asset(&draft.scene_id));
    draft.water_asset_path = role_path;
    assert!(super::output_paths::collect_reserved_asset_paths(&temporary.path, &draft).is_err());
    assert_eq!(std::fs::read(&source_path).unwrap(), original); // 无法无损写入时拒绝覆盖，不能擦除原游戏内容。
}
