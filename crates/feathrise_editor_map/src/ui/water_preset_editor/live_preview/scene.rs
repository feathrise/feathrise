//! 独立水体样品场景的固定网格、RTT 与相机创建。
//!
//! 【功能设计】世界实体只包含 `10m × 50m` 河面及观察河床；天空、IBL、环境光和
//! 当前太阳/月亮由相邻环境同步模块复用，禁止在样品世界中再造固定方向光。

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers}; // 样品波峰和专用渲染层。
use bevy::camera::{Projection, RenderTarget}; // 独立透视相机与RTT目标。
use bevy::light::NotShadowCaster; // 透明样品水面不向河床投射整片阴影。
use bevy::mesh::MeshBuilder; // 固定细分平面显式构建为Mesh。
use bevy::prelude::*; // ECS、网格、材质和相机组件。
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
}; // 创建最小占位离屏纹理，真实尺寸由ViewportNode维护。
use bevy::ui::{FocusPolicy, widget::ViewportNode}; // 只读画布阻止点击穿透。
use feathrise_editor_core::constants::render_layers::MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER; // 与地图实体及其他预览完全隔离。
use feathrise_scene::DynamicWaterMaterial; // 样品继续使用正式透明PBR水材质。

use super::navigation::{PREVIEW_FOV_RADIANS, WaterPresetLivePreviewNavigation}; // 初始与交互取景共享同一数学状态。
use super::{
    PREVIEW_LENGTH_METERS, PREVIEW_WIDTH_METERS, WaterPresetLivePreviewCamera,
    WaterPresetLivePreviewRenderable, WaterPresetLivePreviewRuntime, WaterPresetLivePreviewSurface,
    WaterPresetLivePreviewViewport,
}; // 场景实体与父模块的稳定资源、marker共享身份。
use crate::components::MapEditorOwnedEntity; // 独立相机和样品实体纳入F12资源审计。

/// 构造固定米制细分水面；顶点色是正式水Shader的波谷与岸带输入契约。
pub(super) fn build_preview_water_mesh() -> Mesh {
    let mut mesh = Plane3d::default()
        .mesh()
        .size(PREVIEW_WIDTH_METERS, PREVIEW_LENGTH_METERS)
        .subdivisions_x(100) // 保持约零点一米的横向细分，避免放大后GPU波浪形成粗折面。
        .subdivisions_z(500) // 保持约零点一米的纵向细分，使五十米长轴仍连续起伏。
        .build();
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        vec![[1.0, 1.0, 1.0, 1.0]; mesh.count_vertices()],
    );
    mesh
}

/// BSN画布首次出现时建立唯一独立RTT、相机、河床和样品水面。
#[allow(clippy::too_many_arguments)]
pub(in crate::ui::water_preset_editor) fn setup_live_preview_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
    mut water_materials: ResMut<Assets<DynamicWaterMaterial>>,
    viewports: Query<Entity, Added<WaterPresetLivePreviewViewport>>,
    mut runtime: ResMut<WaterPresetLivePreviewRuntime>,
) {
    let Ok(viewport) = viewports.single() else {
        return;
    };
    if let Some(camera) = runtime.camera {
        commands
            .entity(viewport)
            .insert((ViewportNode::new(camera), FocusPolicy::Block));
        return;
    }

    let size = Extent3d {
        width: 1,                 // 首帧只建立合法最小RTT，ViewportNode随后维护真实画布尺寸。
        height: 1,                // 首帧只建立合法最小RTT，避免猜测右栏物理像素高度。
        depth_or_array_layers: 1, // 普通二维颜色目标只需要单层。
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("map_editor_water_preset_preview_target"), // RenderDoc中可直接识别样品目标。
            size,                                                  // 与上方最小占位尺寸一致。
            dimension: TextureDimension::D2,                       // ViewportNode输出普通二维画面。
            format: TextureFormat::Bgra8UnormSrgb, // UI最终显示使用标准sRGB颜色目标。
            mip_level_count: 1,                    // RTT不做缩小采样，不生成无用Mip。
            sample_count: 1,                       // 多重采样由主渲染配置决定，不扩张独立目标。
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT, // 同时允许相机写入和UI采样。
            view_formats: &[],                     // 不为固定sRGB目标声明额外视图格式。
        },
        ..default() // Image其余元数据沿用Bevy正式默认值。
    };
    image.resize(size); // 分配与描述匹配的首帧像素存储。
    let image = images.add(image); // 登记唯一强句柄供相机与运行时共同持有。
    let render_layer = RenderLayers::layer(MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER); // 样品世界独占第十八层。

    let material = water_materials.add(DynamicWaterMaterial::default()); // 首份完整草稿采用前保持稳定占位句柄。
    commands.spawn((
        Name::new("水体预设样品河面"), // 调试检查器中的明确实体名。
        WaterPresetLivePreviewRenderable, // 场景页隐藏或关闭时统一停用。
        WaterPresetLivePreviewSurface,    // 首份完整正式材质采用前继续隐藏。
        MapEditorOwnedEntity,             // 纳入F12实体生命周期审计。
        Mesh3d(meshes.add(build_preview_water_mesh())), // 固定十米乘五十米细分河面。
        MeshMaterial3d(material.clone()), // 后续只原位替换同一个正式水材质资产。
        Transform::IDENTITY,              // 样品世界以原点为河面中心。
        Visibility::Hidden,               // 等活动编辑标签与完整材质共同放行。
        render_layer.clone(),             // 世界实体只进入样品相机专层。
        NoFrustumCulling,                 // GPU顶点波峰可能超出静态平面Aabb。
        NotShadowCaster,                  // 透明水面不向观察河床投下整块平面阴影。
    ));

    let bed_material = standard_materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.095, 0.065), // 深褐河床帮助观察水色与透明度。
        metallic: 0.0,                               // 泥沙河床不使用金属响应。
        perceptual_roughness: 0.94,                  // 高粗糙度避免河床镜面抢过水面反射。
        ..default()                                  // 其余PBR参数沿用安全默认值。
    });
    commands.spawn((
        Name::new("水体预设样品河床"), // 调试检查器中的明确实体名。
        WaterPresetLivePreviewRenderable, // 与水面共享显隐生命周期。
        MapEditorOwnedEntity,             // 纳入F12实体生命周期审计。
        Mesh3d(meshes.add(Plane3d::default().mesh().size(13.5, 53.5).build())), // 河床覆盖水面和水平波位移余量。
        MeshMaterial3d(bed_material), // 使用固定观察材质而非地图地表资产。
        Transform::from_xyz(0.0, -0.32, 0.0), // 保留浅水深度以辨识透明度与波谷压暗。
        Visibility::Hidden,           // 活动场景页出现前不参与渲染。
        render_layer.clone(),         // 河床只进入样品相机专层。
        NotShadowCaster,              // 单平面河床无需向自身或水面投射阴影。
    ));

    let initial_navigation = WaterPresetLivePreviewNavigation::default(); // 初始相机与后续手势共享同一可测试状态。
    let camera = commands
        .spawn((
            Name::new("水体预设独立样品相机"), // 不复用或移动中央地图相机。
            WaterPresetLivePreviewCamera,         // 环境同步和导航只作用于这一台相机。
            MapEditorOwnedEntity,                 // 纳入F12实体生命周期审计。
            Camera3d::default(),                  // 使用正式PBR三维渲染路径。
            Camera {
                order: -5,        // 独立RTT不与窗口相机叠加，但保持稳定排序。
                is_active: false, // 进入活动编辑标签且右栏为场景页后才运行。
                clear_color: ClearColorConfig::Custom(Color::srgb(0.055, 0.065, 0.064)), // 环境同步前使用暗色安全底。
                ..default() // 目标、视口和输出模式沿用正式默认值。
            },
            RenderTarget::from(image.clone()), // 画面只写右栏ViewportNode绑定的RTT。
            Projection::Perspective(PerspectiveProjection {
                fov: PREVIEW_FOV_RADIANS, // 与导航数学使用同一个固定透视视野。
                near: 0.1,                // 放大检查近处波纹时仍保留细节。
                far: 400.0,               // 初始完整覆盖五十米河面及其位移波峰。
                ..default()               // 宽高比由导航系统按真实画布同步。
            }),
            initial_navigation.camera_transform(), // 初始完整取景随后可旋转、平移和缩放。
            render_layer,                          // 世界中除河面和河床外的实体均不进入本相机。
        ))
        .id();
    commands
        .entity(viewport)
        .insert((ViewportNode::new(camera), FocusPolicy::Block)); // UI显示专用RTT且阻止点击穿透。
    runtime._image = Some(image); // 强持有RTT避免UI重建间隙卸载。
    runtime.camera = Some(camera); // 下次UI壳重建直接复用唯一相机。
    runtime.material = Some(material); // 后续实时输入只替换同一材质资产。
}
