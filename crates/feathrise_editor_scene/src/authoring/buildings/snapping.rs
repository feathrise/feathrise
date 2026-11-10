//! F12 建筑模块地面连接点的确定性手工吸附解析。
//!
//! 【功能设计】普通网格点放保持原行为；作者显式开启连接吸附后，当前模块的全部
//! 连接点只在固定两米半径内查询同代建筑 BVH。最近同类型点决定平移和九十度旋转，
//! 没有兼容点时失败关闭，预览幽灵与点击请求必须共同调用本文件的唯一解析入口。

use std::cmp::Ordering; // 相同距离按稳定建筑、目标点和源点 ID 确定唯一结果。

use bevy::prelude::*; // Rect、Quat、Vec2 与 Vec3 提供纯数学运算。
use feathrise_scene_content::BuildingPresetDescriptor; // 所选模块连接点来自已采用目录快照。

use super::resources::BuildingAuthoringState; // 当前同代不可变 BVH。
use super::types::*; // 姿态、吸附结果、失败原因和索引项。
use crate::ui::building_presets::BuildingPlacementControls; // 网格、朝向和显式吸附开关。

/// 光标附近允许连接点接管候选的固定世界距离。
pub(super) const CONNECTION_SNAP_DISTANCE_METERS: f32 = 2.0;

/// 解析本帧唯一建筑平面姿态；函数不修改资源，也不产生作者副作用。
#[cfg(test)]
pub(super) fn resolve_building_pose(
    cursor_world: Vec3,                       // 当前视口真实地形命中。
    preset: &BuildingPresetDescriptor,        // 当前所选规范预设。
    controls: &BuildingPlacementControls,     // 普通网格、手工角度和吸附开关。
    identity: Option<&BuildingIndexIdentity>, // 当前草稿与目录的完整期望身份。
    state: &BuildingAuthoringState,           // 只借用已经原子采用的同代索引。
    exclude_id: Option<&str>,                 // 移动既有实例时可排除自身；新建为 None。
) -> BuildingPlacementPose {
    resolve_building_pose_snapped(cursor_world, preset, controls, identity, state, exclude_id, None)
}

/// 统一吸附生效时使用其米制基准，未启用时保留原建筑网格；连接点始终拥有最终姿态。
pub(super) fn resolve_building_pose_snapped(
    cursor_world: Vec3, preset: &BuildingPresetDescriptor, controls: &BuildingPlacementControls,
    identity: Option<&BuildingIndexIdentity>, state: &BuildingAuthoringState, exclude_id: Option<&str>,
    snap: Option<feathrise_editor_core::EditorTranslationSnap>,
) -> BuildingPlacementPose {
    let base_world_xz = snap.filter(|snap| snap.active()).and_then(|snap| snap.horizontal(cursor_world.to_array()))
        .map_or_else(|| controls.snapped_xz(cursor_world), |point| Vec2::new(point[0], point[2]));
    let base_yaw_degrees = controls.yaw_degrees.rem_euclid(360.0); // 手工朝向始终规范化。
    if !controls.connection_snap {
        return BuildingPlacementPose {
            world_xz: base_world_xz,
            yaw_degrees: base_yaw_degrees,
            connection: BuildingConnectionSnapState::Disabled,
        }; // 普通点放不会因为预设新增可选字段而改变。
    }
    if preset.connections.is_empty() {
        return failed_pose(
            base_world_xz,
            base_yaw_degrees,
            BuildingConnectionSnapFailure::SourceHasNoPoints,
            None,
        );
    }
    let Some(identity) = identity else {
        return failed_pose(
            base_world_xz,
            base_yaw_degrees,
            BuildingConnectionSnapFailure::IndexUnavailable,
            None,
        );
    };
    if state.active_identity.as_ref() != Some(identity) {
        return failed_pose(
            base_world_xz,
            base_yaw_degrees,
            BuildingConnectionSnapFailure::IndexUnavailable,
            None,
        );
    }
    let Some(index) = state.active_index.as_deref() else {
        return failed_pose(
            base_world_xz,
            base_yaw_degrees,
            BuildingConnectionSnapFailure::IndexUnavailable,
            None,
        );
    };
    resolve_against_index(preset, base_world_xz, base_yaw_degrees, index, exclude_id)
}

/// 在一份完整不可变索引中选择最近兼容点并解析精确根姿态。
fn resolve_against_index(
    preset: &BuildingPresetDescriptor,
    base_world_xz: Vec2,
    base_yaw_degrees: f32,
    index: &BuildingSpatialIndex,
    exclude_id: Option<&str>,
) -> BuildingPlacementPose {
    let base_root = [base_world_xz.x, 0.0, base_world_xz.y]; // 查询只解释水平位置。
    let mut query_min = Vec2::splat(f32::INFINITY);
    let mut query_max = Vec2::splat(f32::NEG_INFINITY);
    for source in &preset.connections {
        let (position, _) =
            preset.world_connection_marker(source, base_root, base_yaw_degrees, [1.0; 3]);
        let xz = Vec2::new(position.x, position.z);
        query_min = query_min.min(xz);
        query_max = query_max.max(xz);
    }
    let radius = Vec2::splat(CONNECTION_SNAP_DISTANCE_METERS);
    let query_bounds = Rect::from_corners(query_min - radius, query_max + radius); // 全部源点只执行一次BVH查询。
    let maximum_distance_squared = CONNECTION_SNAP_DISTANCE_METERS.powi(2);
    let mut best: Option<(f32, BuildingConnectionSnap)> = None;
    let mut nearest_target: Option<(f32, Vec3, Vec3, usize, usize)> = None;

    for entry_index in index.query(query_bounds) {
        let Some(entry) = index.entry(entry_index) else {
            continue; // BVH 内部异常不制造伪连接。
        };
        if exclude_id == Some(entry.id.as_str()) {
            continue; // 已有建筑移动不能吸附回自身。
        }
        for (target_index, target) in entry.connections.iter().enumerate() {
            let target_xz = Vec2::new(target.position.x, target.position.z);
            for (source_index, source) in preset.connections.iter().enumerate() {
                let (source_position, _) =
                    preset.world_connection_marker(source, base_root, base_yaw_degrees, [1.0; 3]);
                let source_xz = Vec2::new(source_position.x, source_position.z);
                let distance_squared = source_xz.distance_squared(target_xz);
                if distance_squared > maximum_distance_squared {
                    continue; // 固定半径外的点不参与兼容或错误提示。
                }
                if nearest_target.as_ref().is_none_or(
                    |(best_distance, _, _, best_entry, best_target)| {
                        distance_squared.total_cmp(best_distance) == Ordering::Less
                            || (distance_squared.total_cmp(best_distance) == Ordering::Equal
                                && (entry.id.as_str(), target.id.as_str())
                                    < target_key(index, *best_entry, *best_target))
                    },
                ) {
                    nearest_target = Some((
                        distance_squared,
                        target.position,
                        target.outward,
                        entry_index,
                        target_index,
                    )); // 即使类型不兼容也保留最近点供红色Gizmo指出。
                }
                if source.kind != target.kind {
                    continue; // 首期没有隐式公母端、通配符或类型转换。
                }
                let snap = BuildingConnectionSnap {
                    source_connection_index: source_index,
                    target_entry_index: entry_index,
                    target_connection_index: target_index,
                    target_position: target.position,
                    target_outward: target.outward,
                };
                let replace = best.as_ref().is_none_or(|(best_distance, best_snap)| {
                    match distance_squared.total_cmp(best_distance) {
                        Ordering::Less => true,
                        Ordering::Greater => false,
                        Ordering::Equal => {
                            let best_entry = index
                                .entry(best_snap.target_entry_index)
                                .expect("吸附候选来自同一不可变索引");
                            let best_target =
                                &best_entry.connections[best_snap.target_connection_index];
                            (entry.id.as_str(), target.id.as_str(), source.id.as_str())
                                < (
                                    best_entry.id.as_str(),
                                    best_target.id.as_str(),
                                    preset.connections[best_snap.source_connection_index]
                                        .id
                                        .as_str(),
                                )
                        }
                    }
                });
                if replace {
                    best = Some((distance_squared, snap));
                }
            }
        }
    }

    let Some((_, snap)) = best else {
        let nearby = nearest_target.map(|(_, position, outward, _, _)| (position, outward));
        let reason = if nearby.is_some() {
            BuildingConnectionSnapFailure::IncompatibleKind
        } else {
            BuildingConnectionSnapFailure::NoPointInRange
        };
        return failed_pose(base_world_xz, base_yaw_degrees, reason, nearby);
    };
    let entry = index
        .entry(snap.target_entry_index)
        .expect("吸附候选来自同一不可变索引");
    let target = &entry.connections[snap.target_connection_index];
    let source = &preset.connections[snap.source_connection_index];
    let yaw_degrees =
        (target.outward_yaw_degrees + 180.0 - source.outward_yaw_degrees).rem_euclid(360.0);
    let local = Vec3::new(source.position[0], 0.0, source.position[1]);
    let rotated = Quat::from_rotation_y(yaw_degrees.to_radians()) * local;
    let target_xz = Vec2::new(target.position.x, target.position.z);
    BuildingPlacementPose {
        world_xz: target_xz - Vec2::new(rotated.x, rotated.z),
        yaw_degrees,
        connection: BuildingConnectionSnapState::Snapped(snap),
    }
}

/// 返回索引中一个目标点的稳定二元键。
fn target_key(
    index: &BuildingSpatialIndex,
    entry_index: usize,
    target_index: usize,
) -> (&str, &str) {
    let entry = index
        .entry(entry_index)
        .expect("最近目标来自同一不可变索引");
    (
        entry.id.as_str(),
        entry.connections[target_index].id.as_str(),
    )
}

/// 构造保持普通候选姿态但明确失败的吸附结果。
fn failed_pose(
    world_xz: Vec2,
    yaw_degrees: f32,
    reason: BuildingConnectionSnapFailure,
    nearby_target: Option<(Vec3, Vec3)>,
) -> BuildingPlacementPose {
    BuildingPlacementPose {
        world_xz,
        yaw_degrees,
        connection: BuildingConnectionSnapState::Failed {
            reason,
            nearby_target,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 纯吸附测试不建立Bevy App或后台任务。
    use feathrise_scene_content::{
        BuildingConnectionPointDescriptor, BuildingFootprintRect, BuildingFoundationDescriptor,
        BuildingWorldQuad, ScenePlacementAssetTarget, ScenePlacementHeightMode,
    };
    use std::sync::Arc;

    fn module(id: &str, kind: &str) -> BuildingPresetDescriptor {
        let mut preset = BuildingPresetDescriptor::from_asset(
            id.to_owned(),
            id.to_owned(),
            "gltf/test.glb".to_owned(),
            ScenePlacementAssetTarget::Scene0,
        );
        preset.connections = vec![BuildingConnectionPointDescriptor {
            id: "left".to_owned(),
            kind: kind.to_owned(),
            position: [-2.0, 0.0],
            outward_yaw_degrees: 270.0,
        }];
        preset
    }

    fn indexed_target(id: &str, kind: &str, position: Vec3, yaw: f32) -> IndexedBuilding {
        let footprint = BuildingWorldQuad::from_local_rect(
            BuildingFootprintRect::default(),
            [position.x, 0.0, position.z],
            0.0,
            [1.0; 3],
        );
        let connection = IndexedBuildingConnection {
            id: "target".to_owned(),
            kind: kind.to_owned(),
            position,
            outward: Quat::from_rotation_y(yaw.to_radians()) * Vec3::Z,
            outward_yaw_degrees: yaw,
        };
        let point = Vec2::new(position.x, position.z);
        IndexedBuilding {
            terrain_bound: false,
            id: id.to_owned(),
            preset_label: id.to_owned(),
            footprint,
            entrance_clearance: footprint,
            vegetation_clearance: footprint,
            connections: vec![connection],
            bounds: union_rect(footprint.aabb(), Rect::from_corners(point, point)),
            translation: position.to_array(),
            yaw_degrees: 0.0,
            scale: [1.0; 3],
            height_mode: ScenePlacementHeightMode::FixedHeight,
            foundation: BuildingFoundationDescriptor::default(),
            asset_matches_preset: true,
        }
    }

    #[test]
    fn unified_snap_uses_custom_spacing_without_relaxing_building_orientation() {
        let preset = module("wall", "wall-end");
        let controls = BuildingPlacementControls { yaw_degrees: 90.0, ..default() };
        let snap = feathrise_editor_core::EditorSnapSettings { enabled: true, translation_meters: 0.25, ..default() }.translation_snap(false, [0.0; 3]);
        let pose = resolve_building_pose_snapped(Vec3::new(1.4, 2.0, -2.6), &preset, &controls, None, &BuildingAuthoringState::default(), None, Some(snap));
        assert_eq!(pose.world_xz, Vec2::new(1.5, -2.5));
        assert_eq!(pose.yaw_degrees, 90.0);
        assert_eq!(pose.connection, BuildingConnectionSnapState::Disabled);
    }

    #[test]
    fn disabled_connection_mode_keeps_original_grid_pose_without_an_index() {
        let preset = module("wall", "wall-end");
        let controls = BuildingPlacementControls {
            yaw_degrees: 90.0,
            ..default()
        };
        let pose = resolve_building_pose(
            Vec3::new(1.4, 2.0, -2.6),
            &preset,
            &controls,
            None,
            &BuildingAuthoringState::default(),
            None,
        );
        assert_eq!(pose.world_xz, Vec2::new(1.0, -3.0)); // 默认一米网格保持既有舍入。
        assert_eq!(pose.yaw_degrees, 90.0); // 手工朝向不被连接数据接管。
        assert_eq!(pose.connection, BuildingConnectionSnapState::Disabled);
    }

    #[test]
    fn compatible_point_resolves_exact_translation_and_opposite_quarter_turn() {
        let preset = module("wall", "wall-end");
        let index = BuildingSpatialIndex::build(vec![indexed_target(
            "existing",
            "wall-end",
            Vec3::new(5.0, 0.0, 0.0),
            90.0,
        )])
        .expect("合法目标应建立索引");
        let pose = resolve_against_index(&preset, Vec2::new(7.1, 0.0), 0.0, &index, None);
        assert_eq!(pose.world_xz, Vec2::new(7.0, 0.0)); // 左端-2精确落到世界X=5。
        assert_eq!(pose.yaw_degrees, 0.0); // 源- X与目标+X保持相反朝外方向。
        assert!(matches!(
            pose.connection,
            BuildingConnectionSnapState::Snapped(_)
        ));
    }

    #[test]
    fn equal_distance_uses_stable_target_identity_and_incompatible_near_point_fails() {
        let preset = module("wall", "wall-end");
        let index = BuildingSpatialIndex::build(vec![
            indexed_target("target-b", "wall-end", Vec3::new(5.0, 0.0, 1.0), 90.0),
            indexed_target("target-a", "wall-end", Vec3::new(5.0, 0.0, -1.0), 90.0),
        ])
        .expect("合法目标应建立索引");
        let pose = resolve_against_index(&preset, Vec2::new(7.0, 0.0), 0.0, &index, None);
        let BuildingConnectionSnapState::Snapped(snap) = pose.connection else {
            panic!("等距兼容目标应形成吸附");
        };
        assert_eq!(index.entry(snap.target_entry_index).unwrap().id, "target-a");

        let incompatible = BuildingSpatialIndex::build(vec![indexed_target(
            "door",
            "door-end",
            Vec3::new(5.0, 0.0, 0.0),
            90.0,
        )])
        .expect("不兼容类型本身仍是合法索引数据");
        let pose = resolve_against_index(&preset, Vec2::new(7.0, 0.0), 0.0, &incompatible, None);
        assert!(matches!(
            pose.connection,
            BuildingConnectionSnapState::Failed {
                reason: BuildingConnectionSnapFailure::IncompatibleKind,
                nearby_target: Some(_),
            }
        ));
    }

    #[test]
    fn over_distance_and_stale_index_fail_without_silent_grid_fallback() {
        let preset = module("wall", "wall-end");
        let index = BuildingSpatialIndex::build(vec![indexed_target(
            "far",
            "wall-end",
            Vec3::new(20.0, 0.0, 0.0),
            90.0,
        )])
        .expect("远目标仍是合法索引数据");
        let pose = resolve_against_index(&preset, Vec2::ZERO, 0.0, &index, None);
        assert!(matches!(
            pose.connection,
            BuildingConnectionSnapState::Failed {
                reason: BuildingConnectionSnapFailure::NoPointInRange,
                ..
            }
        ));

        let controls = BuildingPlacementControls {
            connection_snap: true,
            ..default()
        };
        let expected = BuildingIndexIdentity {
            scene_id: feathrise_scene_content::SceneId::parse("MAP-snap-test").unwrap(),
            author_revision: 1,
            terrain_revision: 1,
            structure_revision: 1,
            catalog_revision: 1,
        };
        let state = BuildingAuthoringState {
            active_identity: Some(BuildingIndexIdentity {
                author_revision: 2,
                ..expected.clone()
            }),
            active_index: Some(Arc::new(index)),
            ..default()
        };
        let pose = resolve_building_pose(
            Vec3::ZERO,
            &preset,
            &controls,
            Some(&expected),
            &state,
            None,
        );
        assert!(matches!(
            pose.connection,
            BuildingConnectionSnapState::Failed {
                reason: BuildingConnectionSnapFailure::IndexUnavailable,
                ..
            }
        )); // 目录或作者换代后不能采用旧候选。
    }
}
