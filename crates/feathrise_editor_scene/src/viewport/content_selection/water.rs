//! 统一水面轻量完成范围命中到水域作者对象的稳定选择发布。

use bevy::camera::primitives::Aabb; // 水面GPU上传后仍保留后台完成的轻量局部范围。
use bevy::shape::Aabb3d; // Bevy正式射线/Aabb函数使用中心和半尺寸描述局部范围。
use bevy::picking::mesh_picking::ray_cast::ray_aabb_intersection_3d; // 不读取已从MainWorld释放的水面顶点。
use bevy::prelude::{Entity, GlobalTransform, Mat4, Query, Ray3d, Vec2, Vec3, Visibility}; // 点击边沿遍历有限水面根并发布最近命中。
use feathrise_scene_content::{SceneId, SceneWaterAsset}; // 当前场景和三类作者数组共同复验世界marker。
use feathrise_scene::{
    ClipmapGroundSnapshot, ClipmapWaterSurface, PaintedWaterSurface, WaterSurfaceKind,
}; // Clipmap共享环带按R8与真实三角水深拾取，GLB继续使用完成范围。

use super::layers::{LayerCandidate, LayerTarget}; // 水体加入同一重叠候选，而非提前消费整个点击。
use crate::resources::{EditorViewportSelectionState, SceneEditorState}; // 查看工具、植被单选与列表索引会话态。
use crate::types::{EditorTool, WaterAuthoringFeatureKind}; // 世界水面类别显式映射到左栏三类作者工具。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 水域对象与显式摆放保持单选语义。
use crate::ui::sidebars::SceneEditorSidebarState; // 点选只揭示水体属性，不改变作者选中的创建模块。
use crate::ui::water_feature_list::activate_water_feature; // 复用列表稳定ID重定位、分页和控制点清理。
use crate::viewport::{SelectedPlacementViewportState, SelectedWaterViewportState}; // 复用两套精确高亮与双击状态机。
use std::{collections::BTreeMap, sync::Arc}; // 静水只采样共享R8源的一个纹素。

/// 静水范围只是粗筛；R8空白处不能因为落在大矩形内就成为重叠候选。
fn still_water_covers(
    layer: &feathrise_scene_content::SceneWaterLayer,
    mask: Option<&Arc<Vec<u8>>>,
    point: Vec3,
    min: [f32; 2],
    max: [f32; 2],
) -> bool {
    let Some(mask) = mask else { return false }; // 没有完整作者源时不猜测整矩形均有水。
    let uv = (Vec2::new(point.x, point.z) - Vec2::from_array(min))
        / (Vec2::from_array(max) - Vec2::from_array(min)); // 与水域作者画布保持同一世界映射。
    if !uv.is_finite()
        || uv.cmplt(Vec2::ZERO).any()
        || uv.cmpgt(Vec2::ONE).any()
        || layer.resolution.contains(&0)
    {
        return false;
    } // 场景外不能夹紧成岸边。
    let x = (uv.x * layer.resolution[0].saturating_sub(1) as f32).round() as usize; // 单纹素最近采样。
    let z = (uv.y * layer.resolution[1].saturating_sub(1) as f32).round() as usize; // 不扫描整张遮罩。
    z.checked_mul(layer.resolution[0] as usize)
        .and_then(|row| row.checked_add(x))
        .and_then(|index| mask.get(index))
        .is_some_and(|value| *value > 0) // 零覆盖与损坏源均不参与选择。
}

/// 一次点击收集当前场景全部可见水体；同一作者对象的多个水面实体只保留最近命中。
#[allow(clippy::too_many_arguments)]
pub(super) fn water_layer_candidates(
    ray: Ray3d,     // 与其他作者层共享相机射线。
    scene_id: &SceneId, // 跨场景残留实体不参与。
    surfaces: &Query<(
        Entity,
        &PaintedWaterSurface,
        Option<&Aabb>,
        &GlobalTransform,
        &Visibility,
        Option<&ClipmapWaterSurface>,
    )>, // 复用已完成的有限水面范围。
    water: &SceneWaterAsset, // 稳定作者名称和索引唯一来自当前草稿。
    masks: &BTreeMap<String, Arc<Vec<u8>>>, // R8只读共享。
    min: [f32; 2],  // 场景画布下界。
    max: [f32; 2],  // 场景画布上界。
    terrain_distance: Option<f32>, // 保持既有地形遮挡规则。
    clipmap_ground: Option<&ClipmapGroundSnapshot>, // Clipmap用同代地表快照核对缺页与最小水深。
    source_frame: impl Fn(Entity) -> Option<Mat4>, // 根水/私有水按owner或已采用GLB预览来源过滤，并补父域预览的源姿态。
) -> Vec<LayerCandidate> {
    let mut result: Vec<LayerCandidate> = Vec::new(); // 候选来自完整已采用水网络，不按作者数量截断。
    for (entity, surface, aabb, transform, visibility, clipmap) in surfaces.iter() {
        // 只在点击边沿访问水面根。
        if surface.scene_id != *scene_id || *visibility == Visibility::Hidden {
            continue;
        } // 只选当前可见代次。
        let Some(source_frame) = source_frame(entity) else { continue; };
        let transform = GlobalTransform::from(source_frame * transform.to_matrix());
        let distance = if let Some(clipmap) = clipmap {
            clipmap_water_surface_distance(ray, clipmap, clipmap_ground)
        } else {
            aabb.and_then(|aabb| water_surface_aabb_distance(ray, aabb, &transform))
        };
        let Some(distance) = distance else {
            continue;
        }; // Clipmap不能把共享整环Aabb当水体；GLB继续不读取RenderWorld-only顶点。
        if terrain_distance.is_some_and(|terrain| distance > terrain + 0.02) {
            continue;
        } // 不隔山选择背面水体。
        let kind = water_feature_kind(surface.kind); // 明确区分静水、河流、瀑布作者数组。
        let author = match kind {
            // 河流和瀑布继续沿用既有后台完成Aabb拾取口径。
            WaterAuthoringFeatureKind::StillWater => water
                .layers
                .iter()
                .enumerate()
                .find(|(_, layer)| layer.id == surface.layer_id)
                .filter(|(_, layer)| {
                    let (min, max) = layer.effective_canvas_bounds(min, max);
                    clipmap.is_some() || still_water_covers(
                            layer,
                            masks.get(&layer.id),
                            transform.affine().inverse().transform_point3(ray.get_point(distance)),
                            min,
                            max,
                        )
                })
                .map(|(index, layer)| (index, "静水体", layer.label.as_str())), // 静水额外排除R8空洞。
            WaterAuthoringFeatureKind::River => water
                .rivers
                .iter()
                .enumerate()
                .find(|(_, river)| river.id == surface.layer_id)
                .map(|(index, river)| (index, "河流", river.label.as_str())), // 不伪造不存在的作者对象。
            WaterAuthoringFeatureKind::Waterfall => water
                .waterfalls
                .iter()
                .enumerate()
                .find(|(_, waterfall)| waterfall.id == surface.layer_id)
                .map(|(index, waterfall)| (index, "瀑布", waterfall.label.as_str())), // 瀑布使用独立作者身份。
        };
        let Some((index, kind_name, label)) = author else {
            continue;
        }; // 草稿删除后不能选中残留水面。
        let target = LayerTarget::Water(kind, surface.layer_id.clone()); // 不按名称合并两个真实作者对象。
        if let Some(existing) = result
            .iter_mut()
            .find(|candidate| candidate.target == target)
        {
            // 正式与预览重复实体只保留一项。
            existing.distance = existing.distance.min(distance); // 选择同一作者的最近可见表面。
            continue; // 不重复显示稳定ID。
        }
        result.push(LayerCandidate {
            author_index: Some(index),               // 供侧栏精确选择。
            target,                                  // 水类型加作者ID。
            label: format!("{kind_name} · {label}"), // 卡片直接说明水体类型与名称。
            rank: 0,                                 // 水面与角色及树冠共同按真实距离排序。
            distance,                                // 前景优先，不排除同射线后方水体。
        });
    }
    result.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.author_index.cmp(&b.author_index))
    }); // 固定候选顺序使同点轮选稳定。
    result // 所有命中完整交给统一异步查询采用。
}

/// 不依赖CPU Mesh顶点的水面点击完成值；距离用于与摆放和普通植被Mesh统一比较。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WaterSurfaceHit {
    pub(super) entity: Entity, // 命中实体继续读取统一水面marker，不复制作者字符串。
    pub(super) point: Vec3,    // 世界命中点供后续候选语义保持与Mesh射线一致。
    pub(super) distance: f32,  // 沿同一编辑器相机射线的世界距离决定最近可见内容。
}

/// 用水面后台完成Aabb计算一次世界射线距离；零厚度静水平面也保持可命中。
fn water_surface_aabb_distance(
    ray: Ray3d,                  // 当前离屏编辑器相机生成的世界射线。
    aabb: &Aabb,                 // 水面GPU上传前已经完成的局部静止几何范围。
    transform: &GlobalTransform, // 正式或作者预览水面当前世界仿射。
) -> Option<f32> {
    let distance = ray_aabb_intersection_3d(
        ray,                                          // 与普通Mesh选择完全相同的相机射线。
        &Aabb3d::new(aabb.center, aabb.half_extents), // 不重新扫描任何水面顶点。
        &transform.affine(), // 在局部Aabb中完成受旋转和缩放的有向范围求交。
    )?;
    (distance.is_finite() && distance >= 0.0).then_some(distance) // 损坏仿射不能制造最近命中。
}

/// 在绝对水位平面取得射线候选点，并用已采用R8、真实地形三角面和高度修订决定是否存在水。
fn clipmap_water_surface_distance(
    ray: Ray3d,
    surface: &ClipmapWaterSurface,
    ground: Option<&ClipmapGroundSnapshot>,
) -> Option<f32> {
    let direction_y = ray.direction.y;
    if direction_y.abs() <= 1.0e-6 {
        return None; // 近平行前/侧视射线不能退化成共享环带整块命中。
    }
    let distance = (surface.water_level - ray.origin.y) / direction_y;
    if !distance.is_finite() || distance < 0.0 {
        return None;
    }
    let point = ray.get_point(distance);
    surface
        .contains_surface(ground?, Vec2::new(point.x, point.z))
        .then_some(distance)
}

/// 在一次左键边沿扫描当前图最多72张水面完成范围，并返回离相机最近的一张。
pub(super) fn nearest_water_surface_hit(
    ray: Ray3d,     // 与本次普通Mesh射线共享同一不可变世界射线。
    scene_id: &SceneId, // 跨场景残留水面不能进入候选或遮挡当前场景内容。
    surfaces: &Query<(
        Entity,                       // 选择发布继续按实体读取稳定水域marker。
        &PaintedWaterSurface,         // 场景、类别和作者稳定ID的唯一来源。
        Option<&Aabb>,                // GLB保留完成范围；Clipmap共享环带不能直接使用它。
        &GlobalTransform,             // 把局部完成范围变换到当前世界位置。
        &Visibility,                  // 作者预览接管后隐藏的旧正式水面必须排除。
        Option<&ClipmapWaterSurface>, // 已采用R8、水位、水深和高度修订组成真实静水足迹。
    )>,
    clipmap_ground: Option<&ClipmapGroundSnapshot>,
) -> Option<WaterSurfaceHit> {
    surfaces
        .iter()
        .filter(|(_, surface, _, _, visibility, _)| {
            surface.scene_id == *scene_id && **visibility != Visibility::Hidden // 只选当前图真实可见代次。
        })
        .filter_map(|(entity, _, aabb, transform, _, clipmap)| {
            let distance = if let Some(clipmap) = clipmap {
                clipmap_water_surface_distance(ray, clipmap, clipmap_ground)?
            } else {
                water_surface_aabb_distance(ray, aabb?, transform)?
            }; // Clipmap做固定复杂度地表采样，GLB保持常数Aabb求交。
            Some(WaterSurfaceHit {
                entity,                         // 保留命中实体供稳定marker复验。
                point: ray.get_point(distance), // 世界点与普通Mesh命中采用同一距离定义。
                distance,                       // 最近命中比较不依赖迭代顺序。
            })
        })
        .min_by(|left, right| left.distance.total_cmp(&right.distance)) // 重叠湖、河和瀑布选择最前表面。
}

/// 把运行时统一水面类别映射为作者列表类别；不按名称或材质猜测水体类型。
pub(super) fn water_feature_kind(kind: WaterSurfaceKind) -> WaterAuthoringFeatureKind {
    match kind {
        WaterSurfaceKind::Still => WaterAuthoringFeatureKind::StillWater,
        WaterSurfaceKind::River => WaterAuthoringFeatureKind::River,
        WaterSurfaceKind::Waterfall => WaterAuthoringFeatureKind::Waterfall,
    }
}

/// 返回作者可读类别与名称；调用前已经按稳定ID激活真实数组项。
fn selected_water_label<'a>(
    water: &'a SceneWaterAsset,
    editor: &SceneEditorState,
    kind: WaterAuthoringFeatureKind,
) -> (&'static str, &'a str) {
    match kind {
        WaterAuthoringFeatureKind::StillWater => (
            "静水体",
            water
                .layers
                .get(editor.selected_water_layer)
                .map_or("未命名静水体", |layer| layer.label.as_str()),
        ),
        WaterAuthoringFeatureKind::River => (
            "河流",
            water
                .rivers
                .get(editor.selected_water_river)
                .map_or("未命名河流", |river| river.label.as_str()),
        ),
        WaterAuthoringFeatureKind::Waterfall => (
            "瀑布",
            water
                .waterfalls
                .get(editor.selected_waterfall)
                .map_or("未命名瀑布", |waterfall| waterfall.label.as_str()),
        ),
    }
}

/// 选择轻量完成范围命中的整张水域作者对象；纯选择不修改作者数据或撤销栈。
#[allow(clippy::too_many_arguments)]
pub(super) fn select_water_surface(
    surface: &PaintedWaterSurface,
    scene_id: &SceneId,
    water: &SceneWaterAsset,
    now_seconds: f64,
    editor: &mut SceneEditorState,
    browser: &mut EditorGltfBrowserState,
    selected_placement: &mut SelectedPlacementViewportState,
    viewport_selection: &mut EditorViewportSelectionState,
    selected_water: &mut SelectedWaterViewportState,
    sidebars: &mut SceneEditorSidebarState,
) -> bool {
    if &surface.scene_id != scene_id {
        return false; // 跨场景残留水面不能改变当前草稿选择。
    }
    let kind = water_feature_kind(surface.kind); // 统一marker类别是唯一映射来源。
    let previous_tool = editor.tool; // 查看点选保持只读工具，Ctrl吸取则切入命中的水域作者工具。
    if let Err(error) = activate_water_feature(editor, water, kind, &surface.layer_id) {
        editor.status = format!("视口命中的水域对象已变化：{error}"); // 热更边界不任选同类数组项。
        return true; // 世界确实命中水面，不能继续退化成其下方植被选择。
    }

    let double_clicked = if previous_tool == EditorTool::Inspect {
        editor.tool = EditorTool::Inspect; // 普通查看点击绝不暗中切入画水或加控制点模式。
        selected_water.select_from_viewport(scene_id, kind, &surface.layer_id, now_seconds)
    } else {
        selected_water.clear_explicit_viewport_selection(); // Ctrl吸取后对应作者工具与类内索引成为权威选择。
        selected_water.register_water_click(scene_id, kind, &surface.layer_id, now_seconds)
    };
    browser.clear_viewport_selection(); // 水域对象与Scene0摆放保持单选语义。
    selected_placement.clear_click_sequence(); // 旧摆放点击不能跨水域选择组成双击。
    viewport_selection.clear_vegetation_selection(); // 水域精确覆盖接管植被整层密度覆盖。
    sidebars.reveal_scene_properties();

    let (kind_name, label) = selected_water_label(water, editor, kind); // 激活后按真实数组索引取得作者名称。
    editor.status = if double_clicked {
        format!("正在定位视口{kind_name}：{label}；等待当前完整水面") // Cursor下一帧按后台Aabb完成定位。
    } else if previous_tool == EditorTool::Inspect {
        format!("已从视口选中{kind_name}：{label}；再次点击同一水面可定位") // 单击只改变选择表现。
    } else {
        format!("已从视口吸取{kind_name}：{label}；本次Ctrl+左键未修改作者数据") // 水域工具明确说明同键未绘制或新增点。
    };
    true // 水面命中完整消费本次点击，不能继续采样其下方植被R8。
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{Dir3, GlobalTransform, Transform}; // 构造零厚度水面与世界平移射线夹具。

    use super::*; // 纯类别映射测试不启动World或RenderApp。

    #[test]
    fn overlapping_water_candidates_keep_all_authors_and_reject_invisible_holes() {
        use bevy::{ecs::system::SystemState, prelude::World}; // 真实生产Query覆盖过滤与去重。
        let map = SceneId::parse("MAP-water-pick").unwrap(); // 当前场景。
        let other = SceneId::parse("MAP-other").unwrap(); // 残留场景。
        let mut water =
            crate::authoring::water_asset_with_layer_for_tests(&map, "scenes/MAP-water-pick"); // 复用完整作者夹具。
        water.layers[0].id = "upper".into(); // 第一张独立水面。
        water.layers[0].resolution = [3, 3]; // 中央纹素可独立制造空洞。
        let mut second = water.layers[0].clone(); // 同名称仍不能合并。
        second.id = "lower".into(); // 第二张独立水面。
        water.layers.push(second); // 同点重叠作者对象。
        let mut masks = BTreeMap::from([
            ("upper".into(), Arc::new(vec![255; 9])),
            ("lower".into(), Arc::new(vec![255; 9])),
        ]); // 两张均覆盖点击。
        let mut world = World::new(); // 无窗口ECS夹具。
        for (id, height, scene_id, visibility) in [
            ("upper", 4.0, map.clone(), Visibility::Visible),
            ("upper", 3.0, map.clone(), Visibility::Visible),
            ("lower", 2.0, map.clone(), Visibility::Visible),
            ("lower", 8.0, map.clone(), Visibility::Hidden),
            ("lower", 9.0, other, Visibility::Visible),
        ] {
            // 重复、隐藏和跨场景实体均不能增加候选。
            world.spawn((
                PaintedWaterSurface {
                    scene_id,
                    layer_id: id.into(),
                    kind: WaterSurfaceKind::Still,
                    purpose: feathrise_scene::PaintedWaterPurpose::EditorPreview,
                },
                Aabb::from_min_max(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 1.0)),
                GlobalTransform::from(Transform::from_xyz(0.0, height, 0.0)),
                visibility,
            )); // 静止水平几何与真实可见状态。
        }
        let mut state: SystemState<
            Query<(
                Entity,
                &PaintedWaterSurface,
                Option<&Aabb>,
                &GlobalTransform,
                &Visibility,
                Option<&ClipmapWaterSurface>,
            )>,
        > = SystemState::new(&mut world); // 直接调用生产收集函数。
        let query = state.get(&world).expect("水面查询已初始化"); // 只读快照。
        let ray = Ray3d::new(Vec3::Y * 10.0, Dir3::NEG_Y); // 垂直穿过两层。
        let hits = water_layer_candidates(
            ray,
            &map,
            &query,
            &water,
            &masks,
            [-1.0; 2],
            [1.0; 2],
            Some(10.0),
            None,
            |_| Some(Mat4::IDENTITY),
        ); // 地形在两张水面下方。
        assert_eq!(hits.len(), 2); // 保留两个独立作者层，重复实体不增加数量。
        assert_eq!(
            hits[0].target,
            LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, "upper".into())
        ); // 最近可见水体默认第一项。
        assert_eq!(hits[0].distance, 6.0); // 隐藏或跨场景实体不能抢占最近距离。
        Arc::make_mut(masks.get_mut("upper").unwrap())[4] = 0; // 上层范围内制造真实R8空洞。
        let hits = water_layer_candidates(
            ray,
            &map,
            &query,
            &water,
            &masks,
            [-1.0; 2],
            [1.0; 2],
            Some(10.0),
            None,
            |_| Some(Mat4::IDENTITY),
        ); // 再次拾取同一位置。
        assert_eq!(hits.len(), 1); // 空洞不按矩形误判。
        assert_eq!(
            hits[0].target,
            LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, "lower".into())
        ); // 只剩实际覆盖层。
        assert!(
            water_layer_candidates(
                ray,
                &map,
                &query,
                &water,
                &masks,
                [-1.0; 2],
                [1.0; 2],
                Some(7.0),
                None,
                |_| Some(Mat4::IDENTITY),
            )
            .is_empty()
        ); // 地形遮挡下层后不隔山选择。
    }

    #[test]
    fn every_runtime_water_surface_kind_scenes_to_one_authoring_feature_kind() {
        assert_eq!(
            water_feature_kind(WaterSurfaceKind::Still),
            WaterAuthoringFeatureKind::StillWater,
        ); // R8水平水面只能激活静水作者对象。
        assert_eq!(
            water_feature_kind(WaterSurfaceKind::River),
            WaterAuthoringFeatureKind::River,
        ); // 普通与急流段共享河流作者对象。
        assert_eq!(
            water_feature_kind(WaterSurfaceKind::Waterfall),
            WaterAuthoringFeatureKind::Waterfall,
        ); // 近竖直水幕只能激活瀑布作者对象。
    }

    #[test]
    fn render_world_only_water_is_pickable_from_its_completed_zero_thickness_aabb() {
        let ray = Ray3d::new(Vec3::new(10.0, 10.0, -4.0), Dir3::NEG_Y); // 从编辑器顶视相机向下点击。
        let aabb = Aabb::from_min_max(
            Vec3::new(-5.0, 0.0, -5.0), // 静水局部范围没有人为增加Y厚度。
            Vec3::new(5.0, 0.0, 5.0),   // 与真实后台完成值保持相同零厚度语义。
        );
        let transform = GlobalTransform::from(Transform::from_xyz(10.0, 3.0, -4.0)); // 水位由实体平移到世界Y=3。

        assert_eq!(
            water_surface_aabb_distance(ray, &aabb, &transform),
            Some(7.0),
        ); // 拾取只读Aabb和仿射，不要求Assets<Mesh>仍保留CPU顶点。
        let outside_ray = Ray3d::new(Vec3::new(20.0, 10.0, -4.0), Dir3::NEG_Y); // X已落在完整水面范围外。
        assert!(water_surface_aabb_distance(outside_ray, &aabb, &transform).is_none()); // 范围外点击不能误命中。
    }
}
