//! 建筑视口点击请求、后台规则校验和成功后的单条作者事务采用。

use bevy::prelude::*; // ECS 系统参数、鼠标、AssetServer 与数学类型。
use bevy::tasks::AsyncComputeTaskPool; // 五点采样与空间查询进入纯 CPU 池。
use bevy::tasks::futures::check_ready; // 主线程只非阻塞轮询点击任务。
use bevy::ui::RelativeCursorPosition; // 左键必须真实位于中央编辑视口。
use feathrise_scene_content::{
    BuildingPresetRegistry, MAX_SCENE_PLACEMENTS_PER_SCENE,
    ScenePlacementHeightMode, ScenePlacementRecord,
}; // 新记录复用普通摆放协议并附加规范建筑预设 ID。
use feathrise_scene::{DynamicClipmapGroundSurface, ScenePlacementInstance}; // 已有建筑采用后定向更新正式根或作者预览。

use super::index::current_identity; // 点击和采用核对同一联合身份。
use super::leveling::build_leveling; // 显式联合整平只在后台校验任务内深复制并修改GLB。
use super::resources::BuildingAuthoringState; // 单在途校验与当前不可变 BVH。
use super::types::*; // 请求、结果和纯几何索引类型。
use super::{BuildingEditRequest, BuildingEditTarget, BuildingTerrainHit}; // 普通属性面板只提交受检意图与真实命中层。
use crate::authoring::{
    BuildingPlacementChange, BuildingTerrainChange, EditorAuthoringSession, EditorCommand,
    PlacementTerrainSampler, command_uses_bounded_water_history, next_author_id,
    spawn_placement_preview,
}; // 建筑采用复用既有预览、撤销和作者身份协议。
use crate::components::SceneEditorViewport; // 中央离屏视口的唯一 UI 命中根。
use crate::resources::SceneEditorState; // 光标世界点、场景选择、工具和状态栏。
use crate::types::EditorTool; // 只有专用建筑编辑工具允许发起和采用落点。
use crate::ui::building_presets::{BuildingPlacementControls, BuildingPresetEditorState}; // 当前规范选择、朝向与网格。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 已有建筑采用前复核完整实例仍被选中。
use crate::ui::placement_transform::{PlacementTransformInputState, sync_placement_record_world}; // 已武装移动操作独占下一次左键；成功变换复用统一根同步。
use crate::ui::placement_water::PlacementInstanceIndex; // 稳定ID只定向少量活动根。

const FOUNDATION_HEIGHT_EPSILON: f32 = 0.005; // JSON毫米级输入允许五毫米浮点与量化采样误差。
const QUARTER_TURN_EPSILON: f32 = 0.001; // 首期建筑朝向只接受九十度整数档。
const UNIT_SCALE_EPSILON: f32 = 0.001; // 首期占地、门口和碰撞均以单位模型比例为权威。

mod apply; // 完成值只在全部身份仍匹配时原子写入作者记录、预览和历史。
mod requests; // 视口点击与已有实例操作冻结完整身份并提交single-flight后台校验。
mod sparse_apply; // 局部作者页、建筑记录、实时覆盖与历史的完整采用。
mod sparse_leveling; // Clipmap规则格平台在IO/CPU阶段构建，不借用GLB拓扑。
pub(in crate::authoring) use sparse_apply::apply_transaction as apply_sparse_building_transaction;

pub(super) use apply::poll_validation; // 保持建筑插件原有系统注册路径。
pub(super) use requests::{request_existing_edit, request_from_viewport}; // 保持建筑插件原有请求系统路径。

/// 在后台对当前整张草稿执行保存级建筑规则审计，同时保留索引用于作者修复。
pub(super) fn audit_existing_buildings(
    index: &BuildingSpatialIndex,
    sampler: &PlacementTerrainSampler,
    _scene_bounds: Rect,
) -> Option<String> {
    for (entry_index, entry) in index.entries().iter().enumerate() {
        if !entry.asset_matches_preset {
            return Some(format!(
                "建筑{}（{}）的模型来源与当前预设不一致",
                entry.id, entry.preset_label,
            )); // 占地语义不能套在另一份模型上保存。
        }
        if entry.height_mode != ScenePlacementHeightMode::FixedHeight {
            return Some(format!(
                "建筑{}（{}）必须使用五点地基固定高度",
                entry.id, entry.preset_label,
            )); // 普通原点跟随会在地形变化后破坏完整地基。
        }
        if !entry.terrain_bound && entry
            .scale
            .iter()
            .any(|value| (*value - 1.0).abs() > UNIT_SCALE_EPSILON)
        {
            return Some(format!(
                "建筑{}（{}）首期只支持单位缩放，请恢复为1,1,1",
                entry.id, entry.preset_label,
            )); // 缩放编辑将在后续具有独立地基与碰撞重建事务后开放。
        }
        let quarter_turn = (entry.yaw_degrees / 90.0).round() * 90.0;
        if !entry.terrain_bound && (entry.yaw_degrees - quarter_turn).abs() > QUARTER_TURN_EPSILON {
            return Some(format!(
                "建筑{}（{}）旋转必须使用90°档位",
                entry.id, entry.preset_label,
            )); // 精确摆放首期不接受任意角度留下门口与网格误差。
        }
        // 占地由真实地基及邻接对象校验，旧场景矩形不限制建筑位置。
        let mut minimum_ground = f32::INFINITY;
        let mut maximum_ground = f32::NEG_INFINITY;
        let entry_sampler = sampler.clone().for_selected_surface(Vec3::new(
            entry.translation[0],
            entry.translation[1] + entry.foundation.model_base_y * entry.scale[1],
            entry.translation[2],
        ));
        for point in entry.footprint.foundation_samples() {
            let height = match entry_sampler.sample(point.x, point.y) {
                Ok(height) => height,
                Err(error) => {
                    return Some(format!(
                        "建筑{}（{}）地基采样失败：{error}",
                        entry.id, entry.preset_label,
                    ));
                }
            };
            minimum_ground = minimum_ground.min(height);
            maximum_ground = maximum_ground.max(height);
        }
        let ground_delta = maximum_ground - minimum_ground;
        if ground_delta > entry.foundation.max_ground_height_delta {
            return Some(format!(
                "建筑{}（{}）地基五点高差{ground_delta:.3}米，超过允许的{:.3}米",
                entry.id, entry.preset_label, entry.foundation.max_ground_height_delta,
            ));
        }
        let expected_root_y = maximum_ground - entry.foundation.model_base_y * entry.scale[1];
        if (entry.translation[1] - expected_root_y).abs() > FOUNDATION_HEIGHT_EPSILON {
            return Some(format!(
                "建筑{}（{}）根高度{:.3}米与五点地基解析值{expected_root_y:.3}米不一致，请重新吸附地面",
                entry.id, entry.preset_label, entry.translation[1],
            ));
        }
        for candidate_index in index.query(entry.bounds) {
            if candidate_index <= entry_index {
                continue; // 每对建筑只报告一次，并跳过自身。
            }
            let Some(other) = index.entry(candidate_index) else {
                continue; // 损坏内部下标不制造伪诊断。
            };
            let overlap = entry.footprint.aabb().intersect(other.footprint.aabb());
            let other_floor = other.translation[1] + other.foundation.model_base_y * other.scale[1];
            let other_sampler = sampler.clone().for_selected_surface(Vec3::new(
                other.translation[0],
                other_floor,
                other.translation[2],
            ));
            if entry_sampler.separated_by_terrain(other_floor, overlap.center())
                || other_sampler.separated_by_terrain(
                    entry.translation[1] + entry.foundation.model_base_y * entry.scale[1],
                    overlap.center(),
                )
            {
                continue;
            }
            if entry.footprint.overlaps(other.footprint) {
                return Some(format!("建筑{}与{}的实体占地重叠", entry.id, other.id));
            }
            if entry.entrance_clearance.overlaps(other.footprint) {
                return Some(format!("建筑{}的正门空间被建筑{}阻挡", entry.id, other.id));
            }
            if other.entrance_clearance.overlaps(entry.footprint) {
                return Some(format!(
                    "建筑{}的实体占地堵住建筑{}的正门",
                    entry.id, other.id
                ));
            }
        }
    }
    None // 全部建筑都满足当前预设、场景、地基与双向净空规则。
}

/// 在后台执行五点地基、建筑占地和双向门口阻塞校验；场景容器不限制位置。
fn validate_request(
    mut request: BuildingPlacementRequest,
    index: &BuildingSpatialIndex,
    sampler: &PlacementTerrainSampler,
    scene_bounds: Rect,
) -> BuildingValidationResult {
    let root = [request.world_xz.x, 0.0, request.world_xz.y]; // 平面几何不依赖最终世界Y。
    let footprint = request
        .preset
        .world_footprint(root, request.yaw_degrees, request.scale);
    let entrance_clearance =
        request
            .preset
            .world_entrance_clearance(root, request.yaw_degrees, request.scale);
    let geometry = validate_geometry(
        &request,
        footprint,
        entrance_clearance,
        index,
        scene_bounds,
        Some(sampler),
    );
    let mut terrain_leveling = None; // 普通落点和任一失败都不携带可采用地形写侧。
    let outcome = geometry.and_then(|()| {
        if request.level_terrain {
            super::terrain_coordinates::verify_support(request.terrain_frame.as_ref(), sampler, footprint)?;
            let input = request
                .terrain_leveling
                .take()
                .ok_or_else(|| "联合整平请求缺少冻结的真实地形输入".to_owned())?;
            let mut output = build_leveling(input, super::terrain_coordinates::source_quad(request.terrain_frame.as_ref(), footprint))?;
            output.platform_height = super::terrain_coordinates::world_height(request.terrain_frame.as_ref(), output.platform_height);
            let root_y =
                output.platform_height - request.preset.foundation.model_base_y * request.scale[1];
            if !root_y.is_finite() {
                return Err("联合整平后的建筑根高度无效".to_owned());
            }
            let platform_height = output.platform_height;
            terrain_leveling = Some(output); // 只有全部规则成功才把完整候选带回主线程。
            Ok(ValidatedBuildingPlacement {
                terrain_binding: request.terrain_frame.as_ref().map(|frame| feathrise_scene_content::SceneTerrainBinding::capture(frame.id.clone(), frame.transform,
                    feathrise_scene_content::TerrainObjectTransform {translation:[request.world_xz.x,root_y,request.world_xz.y],yaw_degrees:request.yaw_degrees,scale:request.scale, ..Default::default()})).transpose()?,
                translation: Vec3::new(request.world_xz.x, root_y, request.world_xz.y),
                minimum_ground: platform_height, // 平台内最终高度严格统一。
                maximum_ground: platform_height, // 状态摘要不会继续报告旧坡差。
            })
        } else {
            validate_existing_ground(&request, footprint, sampler) // 关闭开关时完全沿用五点坡差规则。
        }
    });
    request.terrain_leveling = None; // 红绿回显只保留轻量身份；几何提前失败也不能长期共享整张旧GLB。
    BuildingValidationResult {
        request,            // 完成值保留全部采用身份。
        footprint,          // 非法时同样可画红色占地。
        entrance_clearance, // 非法时同样可画红色门区。
        outcome,            // 具体合法位置或原因。
        terrain_leveling,   // 成功联合整平才存在，普通请求保持None。
        ground_query: None,
        sparse_leveling: None,
    }
}

/// 先在唯一校验worker收集五个精确点，缺页时把完整冻结请求交还等待槽。
fn validate_with_ground(
    request: BuildingPlacementRequest,
    index: &BuildingSpatialIndex,
    sampler: &PlacementTerrainSampler,
    bounds: Rect,
    query: Option<BuildingGroundQuery>,
) -> BuildingValidationResult {
    let root = [request.world_xz.x, 0., request.world_xz.y];
    let footprint = request
        .preset
        .world_footprint(root, request.yaw_degrees, request.scale);
    let entrance_clearance =
        request
            .preset
            .world_entrance_clearance(root, request.yaw_degrees, request.scale);
    if request.ground_content.is_none() {
        return validate_request(request, index, sampler, bounds); // GLB整平和普通GLB完全沿用原后台路径。
    }
    if sampler.is_volume_layer() || (sampler.is_object_surface() && (!request.level_terrain || request.terrain_leveling.is_some())) {
        return validate_request(request, index, sampler, bounds);
    } // 所选洞内层由同一个后台任务直接完成五点采样。
    if let Err(error) =
        validate_geometry(&request, footprint, entrance_clearance, index, bounds, None)
    {
        return BuildingValidationResult {
            request,
            footprint,
            entrance_clearance,
            outcome: Err(error),
            terrain_leveling: None,
            ground_query: None,
            sparse_leveling: None,
        };
    }
    if request.level_terrain {
        if let Err(error) = super::terrain_coordinates::verify_support(request.terrain_frame.as_ref(), sampler, footprint) {
            return BuildingValidationResult {request, footprint, entrance_clearance, outcome:Err(error), terrain_leveling:None, ground_query:None, sparse_leveling:None};
        }
        return BuildingValidationResult {
            request,
            footprint,
            entrance_clearance,
            outcome: Err("正在等待单栋稀疏整平作者源".into()),
            terrain_leveling: None,
            ground_query: None,
            sparse_leveling: Some(SparseBuildingLevelingState::WaitingForAuthor),
        }; // 几何已通过，采用系统继续提交原单飞槽，不把占位高度授信为成功。
    }
    let mut query =
        query.unwrap_or_else(|| BuildingGroundQuery::new(sampler, footprint.foundation_samples()));
    match query.prepare(sampler) {
        Ok(Some(frozen)) => validate_request(request, index, &frozen, bounds),
        Ok(None) => BuildingValidationResult {
            request,
            footprint,
            entrance_clearance,
            outcome: Err("正在等待五点地基的精确高度页".into()),
            terrain_leveling: None,
            ground_query: Some(query),
            sparse_leveling: None,
        },
        Err(error) => BuildingValidationResult {
            request,
            footprint,
            entrance_clearance,
            outcome: Err(error),
            terrain_leveling: None,
            ground_query: None,
            sparse_leveling: None,
        },
    }
}

/// 校验候选中与地形高度无关的全部纯几何规则。
fn validate_geometry(
    request: &BuildingPlacementRequest,
    footprint: feathrise_scene_content::BuildingWorldQuad,
    entrance_clearance: feathrise_scene_content::BuildingWorldQuad,
    index: &BuildingSpatialIndex,
    _scene_bounds: Rect,
    sampler: Option<&PlacementTerrainSampler>,
) -> Result<(), String> {
    let associated=match &request.operation {
        BuildingPlacementOperation::ChangeExisting{before,..}=>before.terrain_binding.is_some(),
        BuildingPlacementOperation::DuplicateExisting{source,..}=>source.terrain_binding.is_some(),
        _=>false,
    };
    // 作者直接调整仍受原规格约束，随承托对象继承的姿态以完整地基与净空校验为准。
    if !associated && request
        .scale
        .iter()
        .any(|value| (*value - 1.0).abs() > UNIT_SCALE_EPSILON)
    {
        return Err("首期建筑只支持单位缩放1,1,1".to_owned());
    }
    let quarter_turn = (request.yaw_degrees / 90.0).round() * 90.0;
    if !associated && (request.yaw_degrees - quarter_turn).abs() > QUARTER_TURN_EPSILON {
        return Err("建筑旋转必须使用90°档位".to_owned());
    }
    // 保留地基、碰撞和门前净空互斥规则，但不再裁切到旧主地形的矩形。

    let query_bounds = union_rect(footprint.aabb(), entrance_clearance.aabb());
    for candidate in index.query(query_bounds) {
        let Some(existing) = index.entry(candidate) else {
            continue; // BVH 内部异常不会产生伪重叠。
        };
        if request.exclude_id.as_deref() == Some(existing.id.as_str()) {
            continue; // 移动已有建筑时只排除自身，其他规则不变。
        }
        let overlap = footprint.aabb().intersect(existing.footprint.aabb());
        let other_floor =
            existing.translation[1] + existing.foundation.model_base_y * existing.scale[1];
        if let Some(sampler) = sampler {
            let other_sampler = sampler.clone().for_selected_surface(Vec3::new(
                existing.translation[0],
                other_floor,
                existing.translation[2],
            ));
            if sampler.separated_by_terrain(other_floor, overlap.center())
                || sampler
                    .sample(request.world_xz.x, request.world_xz.y)
                    .ok()
                    .is_some_and(|floor| {
                        other_sampler.separated_by_terrain(floor, overlap.center())
                    })
            {
                continue;
            }
        }
        if footprint.overlaps(existing.footprint) {
            return Err(format!("实体占地与现有建筑{}重叠", existing.id));
        }
        if entrance_clearance.overlaps(existing.footprint) {
            return Err(format!("正门空间被现有建筑{}阻挡", existing.id));
        }
        if footprint.overlaps(existing.entrance_clearance) {
            return Err(format!("实体占地会堵住现有建筑{}的正门", existing.id));
        }
    }
    Ok(()) // 地形高度由关闭开关的五点采样或显式后台整平二选一解析。
}

/// 在不修改地形时按正式规则高度场执行五点坡差校验并解析建筑根高度。
fn validate_existing_ground(
    request: &BuildingPlacementRequest,
    footprint: feathrise_scene_content::BuildingWorldQuad,
    sampler: &PlacementTerrainSampler,
) -> Result<ValidatedBuildingPlacement, String> {
    let mut minimum_ground = f32::INFINITY;
    let mut maximum_ground = f32::NEG_INFINITY;
    for point in footprint.foundation_samples() {
        let height = sampler.sample(point.x, point.y)?; // 使用正式三角剖分采样中心与四角。
        minimum_ground = minimum_ground.min(height);
        maximum_ground = maximum_ground.max(height);
    }
    let delta = maximum_ground - minimum_ground;
    if delta > request.preset.foundation.max_ground_height_delta {
        return Err(format!(
            "地基五点高差{delta:.3}米，超过预设允许的{:.3}米",
            request.preset.foundation.max_ground_height_delta,
        ));
    }
    let root_y = maximum_ground - request.preset.foundation.model_base_y * request.scale[1]; // 模型基准随实例Y缩放后贴合最高采样点。
    if !root_y.is_finite() {
        return Err("解析后的建筑根高度无效".to_owned());
    }
    Ok(ValidatedBuildingPlacement {
        terrain_binding: sampler.bind_pose(feathrise_scene_content::TerrainObjectTransform {
            translation:[request.world_xz.x,root_y,request.world_xz.y],yaw_degrees:request.yaw_degrees,scale:request.scale,
            ..Default::default()
        },Some(maximum_ground+0.5))?,
        translation: Vec3::new(request.world_xz.x, root_y, request.world_xz.y), // 建筑保持竖直。
        minimum_ground, // 用于反馈实际坡差。
        maximum_ground, // 明确最终采用的地基高度来源。
    })
}

/// 同步建筑左栏与全局状态栏反馈。
fn report(editor: &mut SceneEditorState, building_ui: &mut BuildingPresetEditorState, message: &str) {
    editor.status = message.to_owned();
    building_ui.report_status(message.to_owned());
}

#[cfg(test)]
#[path = "validation/clipmap_tests.rs"]
mod clipmap_tests;

#[cfg(test)]
mod tests {
    use super::*; // 纯规则夹具验证占地与门区双向阻塞。
    use feathrise_scene_content::{BuildingPresetDescriptor, ScenePlacementAssetTarget};
    use std::sync::Arc; // 失败结果必须释放联合整平输入持有的完整地形共享引用。

    #[test]
    fn preset_geometry_uses_highest_foundation_height_formula() {
        let preset = BuildingPresetDescriptor::from_asset(
            "house".to_owned(),                    // 合法稳定身份。
            "房屋".to_owned(),                     // 中文显示名。
            "gltf/buildings/house.glb".to_owned(), // 合法场景路径。
            ScenePlacementAssetTarget::Scene0,       // 完整场景目标。
        );
        let request = BuildingPlacementRequest {
            terrain_frame: None,
            identity: BuildingIndexIdentity {
                scene_id: feathrise_scene_content::SceneId::parse("MAP-building-test").expect("测试场景ID合法"),
                author_revision: 1,
                terrain_revision: 1,
                structure_revision: 1,
                catalog_revision: 1,
            },
            ticket: 1,
            preset,
            world_xz: Vec2::ZERO,
            yaw_degrees: 0.0,
            scale: [1.0; 3],
            exclude_id: None,
            operation: BuildingPlacementOperation::Create,
            level_terrain: false,
            terrain_leveling: None,
            ground_content: None,
            input_controls: None,
        };
        let footprint = request.preset.world_footprint([0.0; 3], 0.0, [1.0; 3]);
        assert_eq!(footprint.foundation_samples().len(), 5); // 中心加四角是固定采样合同。
        assert_eq!(request.preset.foundation.model_base_y, 0.0); // 默认基准可直接对齐最高地面。
    }

    #[test]
    fn rejected_geometry_does_not_retain_the_full_terrain_input() {
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 复用生产草稿格式取得真实网格和索引。
        let sampler = PlacementTerrainSampler::from_draft(&draft).expect("测试地形应可采样"); // 普通分支参数仍使用合法正式高度场。
        let mesh = draft.terrain_mesh.as_ref().unwrap().clone(); // 保留外部引用以检查结果不再持有整图。
        let editing_index = draft.terrain_editing_index.as_ref().unwrap().clone(); // 拓扑与同一测试网格同代。
        let request = BuildingPlacementRequest {
            terrain_frame: None,
            identity: BuildingIndexIdentity {
                scene_id: draft.scene_id.clone(), // 请求属于测试草稿场景。
                author_revision: 1,           // 纯函数测试不依赖实际会话资源。
                terrain_revision: 1,          // 地形输入和请求使用同一假定修订。
                structure_revision: 1,        // 空建筑索引没有结构变化。
                catalog_revision: 1,          // 预设快照视为首个有效目录版本。
            },
            ticket: 1, // 单请求夹具使用非零票据。
            preset: BuildingPresetDescriptor::from_asset(
                "level-test".to_owned(),         // 合法稳定预设ID。
                "整平失败释放测试".to_owned(),   // 可辨诊断名称。
                "gltf/test.glb".to_owned(),      // 合法项目相对模型路径。
                ScenePlacementAssetTarget::Scene0, // 使用完整场景目标。
            ),
            world_xz: Vec2::new(-100.0, -100.0), // 明确越过场景范围，使几何阶段先失败。
            yaw_degrees: 0.0,                    // 合法九十度档位。
            scale: [1.0; 3],                     // 合法单位缩放。
            exclude_id: None,                    // 新建候选不排除任何实例。
            operation: BuildingPlacementOperation::Create, // 新建路径足以覆盖失败回显。
            level_terrain: true,                 // 显式联合整平才会携带完整地形输入。
            ground_content: None,
            input_controls: None,
            terrain_leveling: Some(TerrainLevelingInput {
                locate_hit_triangle: false,
                mesh: mesh.clone(),            // 校验返回前必须释放这一额外共享引用。
                editing_index,                 // 同代拓扑只在失败请求中持有。
                hit_position: [0.0, 0.0, 0.0], // 合法命中值不会抢先制造其他错误。
                hit_triangle: 0,               // 测试网格首个三角合法。
                resolution: draft.manifest.terrain.resolution, // 复用清单采样规格。
                world_min: draft.manifest.world_min, // 复用清单世界下界。
                world_max: draft.manifest.world_max, // 复用清单世界上界。
                minimum_height: draft.manifest.terrain.base_height, // 合法编码下限。
                maximum_height: draft.manifest.terrain.base_height
                    + draft.manifest.terrain.height_scale, // 合法编码上限。
            }),
        };
        let result = validate_request(
            request,                                // 把完整请求交给生产后台纯函数。
            &BuildingSpatialIndex::default(),       // 空索引只检查场景边界。
            &sampler,                               // 高度场不会在几何失败后被读取。
            draft.manifest.world_bounds().unwrap(), // 使用测试场景正式范围。
        );

        assert!(result.outcome.is_err()); // 越界候选必须失败关闭。
        assert!(result.request.terrain_leveling.is_none()); // 回显结果不得继续持有完整GLB输入。
        assert_eq!(Arc::strong_count(&mesh), 2); // 仅测试变量与草稿各持有一份，不残留失败结果引用。
    }
}
