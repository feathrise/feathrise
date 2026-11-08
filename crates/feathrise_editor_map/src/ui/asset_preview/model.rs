//! GLB/GLTF预览的专层隔离、固定预算扫描与自适应相机取景。

use bevy::camera::{Projection, primitives::Aabb, visibility::RenderLayers}; // 联合模型范围并隔离主画面。
use bevy::prelude::*; // ECS层级、相机变换与测试数学类型。
use bevy::ui::ComputedNode; // 读取窄右栏的真实画布宽高比。
use bevy::world_serialization::WorldInstanceReady; // Scene0全部实体生成后的唯一成功门。
use feathrise_editor_core::constants::render_layers::MAP_EDITOR_ASSET_PREVIEW_LAYER; // 全局唯一预览层。

use super::resources::AssetPreviewNavigation; // 首次完整取景也使用当前独立导航状态。
use super::types::*; // 扫描、模型阶段和专用marker。
use super::view_math::apply_model_preview_frame; // 自动取景与鼠标导航只维护一套相机数学。

const MODEL_DESCENDANTS_PER_FRAME: usize = 128; // 大Scene0隔离与Aabb联合保持固定主线程预算。

/// 共享预览及尚未交付的候选走同一回收点，任何Update/PostUpdate动画装配都先完整落地。
pub(super) fn reclaim_external_preview_roots(world: &mut World) {
    let roots: Vec<_> = world.query_filtered::<Entity, With<AssetPreviewRetiredRoot>>()
        .iter(world).take(4).collect();
    for root in roots { world.despawn(root); }
}

/// 尚未完成布局时使用方形安全值；布局稳定后会按Changed<ComputedNode>再次取景。
pub(super) fn canvas_aspect_ratio(canvas: Option<&ComputedNode>) -> f32 {
    // 导航共用同一布局安全值。
    let Some(size) = canvas.map(ComputedNode::size) else {
        return 1.0;
    };
    if size.x <= 0.0 || size.y <= 0.0 || !size.is_finite() {
        1.0
    } else {
        size.x / size.y
    }
}

/// 将一个局部Aabb八角经过完整世界仿射后加入联合范围。
pub(crate) fn include_transformed_aabb(
    minimum: &mut Vec3,
    maximum: &mut Vec3,
    transform: &GlobalTransform,
    aabb: &Aabb,
) {
    let center = Vec3::from(aabb.center);
    let half = Vec3::from(aabb.half_extents);
    let affine = transform.affine(); // 非均匀缩放和父级旋转都必须参与实际取景。
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                let corner = affine.transform_point3(center + half * Vec3::new(x, y, z));
                *minimum = minimum.min(corner);
                *maximum = maximum.max(corner);
            }
        }
    }
}

/// Scene0首次生成或热更重建后立刻隐藏根，并启动新的固定预算后代扫描。
pub(super) fn observe_asset_preview_world_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands, // 热更时精确回收当前最多一个primitive实体。
    mut roots: Query<&mut Visibility, With<AssetPreviewModelRoot>>,
    mut state: ResMut<AssetPreviewState>,
    mut navigation: ResMut<AssetPreviewNavigation>, // 新Scene代不能继承旧几何的观察中心。
) {
    if state.scene_root() != Some(ready.entity) {
        return; // 玩法地图、摆放实例和已经换掉的旧预览不属于本观察者。
    }
    let Ok(mut visibility) = roots.get_mut(ready.entity) else {
        return;
    };
    *visibility = Visibility::Hidden; // 热更的新后代默认层也不能闪入主画面。
    let Some(model) = state.model.as_ref() else {
        return; // 模型会话已经随换文件释放时忽略迟到通知。
    };
    let was_ready = model.scene_ready; // 同一根第二次Ready代表WorldAsset热更换代。
    let target = model.target; // 首次加载期间作者可能已经选择单体。
    if was_ready {
        let gltf = model.catalog.gltf.clone(); // 根元数据句柄保持同一热更身份。
        let primitive_root = model.primitive_root; // 旧单体绝不能跨Scene换代保留。
        if let Some(root) = primitive_root {
            commands.entity(root).try_despawn(); // 只回收当前会话唯一手工实体。
        }
        let generation = state.allocate_model_generation(); // 所有旧Mesh/材质结果失效。
        let mut replacement = AssetPreviewModelSession::new(generation, gltf); // 重建候选、页码和目标。
        replacement.scene_ready = true; // 当前通知已经证明新Scene0实体完整生成。
        state.model = Some(replacement); // 热更安全回到用户确认的默认Scene0。
        *navigation = AssetPreviewNavigation::default(); // 新几何重新自动居中。
    } else if let Some(model) = state.model.as_mut() {
        model.scene_ready = true; // 首次成功门只发布当前根可扫描身份。
        model.scene_bounds = None; // 防御旧缓存意外残留。
        model.scene_mesh_count = 0; // 完整计数等待新扫描。
        model.local_bounds.clear(); // 局部primitive范围与本Scene代绑定。
    }
    let preview_scene = was_ready || target == AssetPreviewModelTarget::Scene0; // 热更总是回到Scene0；首次单体保持单体目标。
    if !preview_scene {
        return; // 首次Scene0迟到不能覆盖作者已经选择的primitive加载状态。
    }
    if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
        *phase = AssetPreviewPhase::Scanning; // Scene0后代按固定预算隔离并联合边界。
    }
    state.bounds_scan = Some(AssetPreviewBoundsScan::new(ready.entity)); // 当前新代唯一扫描队列。
    state.model_bounds = None; // 热更或首次加载都必须使用本代Scene0范围。
    state.status = "模型Scene0已加载，正在分帧隔离节点并计算自动取景".to_owned(); // 不声称包含其他Scene。
}

/// 上一帧范围和专层已完整采用，传播可见性前公开当前 Scene0 或单体。
pub(super) fn reveal_asset_preview(
    mut state: ResMut<AssetPreviewState>,
    mut model_roots: Query<
        &mut Visibility,
        Or<(With<AssetPreviewModelRoot>, With<AssetPreviewPrimitiveRoot>)>,
    >, // RevealPending只解除当前Scene0或单体之一。
) {
    let reveal_root = match &state.content {
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::RevealPending,
            ..
        } => state.current_model_root(), // 单体不能误解除隐藏Scene0根。
        _ => None,
    };
    if let Some(root) = reveal_root {
        let Ok(mut visibility) = model_roots.get_mut(root) else {
            state.bounds_scan = None;
            state.model_bounds = None;
            if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
                *phase = AssetPreviewPhase::Failed; // 保留文件与子列表，使作者仍可切回Scene0重试。
            }
            state.status = "当前模型预览实体已失效，请切回Scene0或重新选择资产".to_owned();
            return;
        };
        *visibility = Visibility::Inherited; // 上一帧全部后代RenderLayers已经落地，现在才安全显示。
        if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
            *phase = AssetPreviewPhase::Ready;
        }
    }
}

/// Scene0完成后每帧只访问固定数量后代；全部专层命令落地一帧后才解除根隐藏。
pub(super) fn advance_asset_preview_bounds(
    mut commands: Commands,
    mut state: ResMut<AssetPreviewState>,
    navigation: Res<AssetPreviewNavigation>, // 初次扫描与热更完成后保留当前倍率和观察方向。
    children: Query<&Children>,
    mesh_bounds: Query<(&GlobalTransform, &Aabb, &Mesh3d)>, // 联合世界范围时顺带缓存每个primitive的局部范围。
    mut embedded_cameras: Query<&mut Camera, Without<AssetPreviewCamera>>,
    mut preview_cameras: Query<
        (&mut Transform, &mut Projection),
        (With<AssetPreviewCamera>, Without<AssetPreviewModelRoot>),
    >,
    canvases: Query<&ComputedNode, With<AssetPreviewCanvas>>,
) {
    if state.bounds_scan.is_none() {
        return; // 稳定预览帧不扫描层级或Aabb。
    }
    let mut scan = state
        .bounds_scan
        .take()
        .expect("刚确认存在的资产预览扫描状态必须可取出");
    if state.scene_root() != Some(scan.root)
        || state.model.as_ref().map(|model| model.target) != Some(AssetPreviewModelTarget::Scene0)
    {
        return; // 换选择后的迟到扫描直接丢弃。
    }
    for _ in 0..MODEL_DESCENDANTS_PER_FRAME {
        let Some(entity) = scan.pending.pop_front() else {
            break;
        };
        scan.visited_count = scan.visited_count.saturating_add(1);
        commands
            .entity(entity)
            .insert(RenderLayers::layer(MAP_EDITOR_ASSET_PREVIEW_LAYER)); // 覆盖资产自带层，确保只进本相机。
        if let Ok(mut camera) = embedded_cameras.get_mut(entity) {
            camera.is_active = false; // 项目导出默认排除相机；防御性关闭外部GLB残留相机。
        }
        if let Ok((transform, aabb, mesh)) = mesh_bounds.get(entity) {
            include_transformed_aabb(&mut scan.minimum, &mut scan.maximum, transform, aabb);
            scan.mesh_count = scan.mesh_count.saturating_add(1);
            if let Some(model) = state.model.as_mut() {
                model.local_bounds.entry(mesh.id()).or_insert(*aabb); // 单体通常直接复用GLTF加载器的局部Aabb。
            }
        }
        if let Ok(entity_children) = children.get(entity) {
            scan.pending.extend(entity_children.iter()); // 只保存实体ID，绝不复制Mesh顶点或场景组件。
        }
    }

    if !scan.pending.is_empty() {
        state.status = format!(
            "正在准备模型预览 · 已检查{}个节点 · 当前队列{}项",
            scan.visited_count,
            scan.pending.len()
        );
        state.bounds_scan = Some(scan);
        return;
    }
    if !scan.has_bounds() || scan.mesh_count == 0 {
        if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
            *phase = AssetPreviewPhase::Failed;
        }
        state.status = format!(
            "Scene0已检查{}个节点，但没有找到可显示的Mesh",
            scan.visited_count
        );
        return;
    }

    let bounds = AssetPreviewModelBounds {
        minimum: scan.minimum,
        maximum: scan.maximum,
    };
    let Ok((mut transform, mut projection)) = preview_cameras.single_mut() else {
        state.status = "模型已加载，但独立预览相机尚未建立".to_owned();
        state.bounds_scan = Some(scan); // 相机下一帧出现后继续从完成态重试，不重扫任何节点。
        return;
    };
    let frame = apply_model_preview_frame(
        bounds.minimum,
        bounds.maximum,
        canvas_aspect_ratio(canvases.single().ok()),
        navigation.view, // resize和鼠标手势不再被自动取景覆盖。
        &mut transform,
        &mut projection,
    );
    state.model_bounds = Some(bounds);
    if let Some(model) = state.model.as_mut() {
        model.scene_bounds = Some(bounds); // 同代切回Scene0只恢复相机，不重新遍历层级。
        model.scene_mesh_count = scan.mesh_count; // 状态栏恢复真实完整网格数。
    }
    if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
        *phase = AssetPreviewPhase::RevealPending; // 延迟一帧确保所有RenderLayers命令先完整采用。
    }
    state.status = format!(
        "Scene0完整预览 · {}个网格 · 尺寸 {:.2} × {:.2} × {:.2} m", // 不把多Scene文件误称全部场景。
        scan.mesh_count, frame.size.x, frame.size.y, frame.size.z
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructed_root_retires_after_post_update_animation_commands_have_landed() {
        #[derive(Resource)]
        struct Fixture { player: Entity, observed: bool }
        fn delayed_graph(mut commands: Commands, fixture: Res<Fixture>) {
            commands.entity(fixture.player).insert(AnimationGraphHandle(Handle::default()));
        }
        fn observe_graph(world: &mut World) {
            let player = world.resource::<Fixture>().player;
            assert!(world.get::<AnimationGraphHandle>(player).is_some());
            world.resource_mut::<Fixture>().observed = true;
        }
        let mut app = App::new();
        let root = app.world_mut().spawn((AssetPreviewRetiredRoot, Visibility::Hidden)).id();
        let player = app.world_mut().spawn((AnimationPlayer::default(), ChildOf(root))).id();
        app.insert_resource(Fixture { player, observed: false })
            .add_systems(PostUpdate, (delayed_graph, bevy::ecs::schedule::ApplyDeferred, observe_graph).chain())
            .add_systems(Last, reclaim_external_preview_roots);
        app.update();
        assert!(app.world().resource::<Fixture>().observed);
        assert!(app.world().get_entity(root).is_err());
        assert!(app.world().get_entity(player).is_err());
    }

    #[test]
    fn transformed_aabb_preserves_translation_and_non_uniform_scale() {
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        let transform = GlobalTransform::from(
            Transform::from_xyz(5.0, 7.0, 9.0).with_scale(Vec3::new(2.0, 3.0, 4.0)),
        );
        let aabb = Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0));
        include_transformed_aabb(&mut minimum, &mut maximum, &transform, &aabb);
        assert_eq!(minimum, Vec3::new(3.0, 4.0, 5.0));
        assert_eq!(maximum, Vec3::new(7.0, 10.0, 13.0));
    }
}
