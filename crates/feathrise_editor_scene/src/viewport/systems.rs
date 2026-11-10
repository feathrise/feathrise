//! 场景编辑器离屏视口实现。

use bevy::camera::visibility::RenderLayers; // 离屏相机额外接收专用笔刷Gizmo层。
use bevy::camera::{Hdr, Projection, RenderTarget}; // 两种投影共享稳定HDR颜色路径和同一个RTT目标。
use bevy::input::mouse::{MouseMotion, MouseWheel}; // CAD式轨道、平移和缩放。
use bevy::shape::InfinitePlane3d; // 把视口光标射线投到地形基准平面。
use bevy::prelude::*; // ECS、图像、相机与数学类型。
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
}; // 创建固定预算离屏纹理。
use bevy::ui::widget::ViewportNode; // 使用Bevy正式视口节点按中央区域实际尺寸维护RTT，避免黑边和画面拉伸。
use bevy::ui::{ComputedNode, FocusPolicy, RelativeCursorPosition}; // 中央视口阻止点击穿透并只在悬停时接管导航。
use feathrise_atmosphere::WeatherPreviewCamera; // F12明确加入正式天气视图，不让普通辅助相机意外继承云雨雾。
use feathrise_scene_core::view::DeferredRasterAuxiliaryCamera; // 共享Deferred材质在离屏视口保持可见。
use feathrise_scene_core::constants::render_layers::{
    AIR_WALL_GAMEPLAY_LAYER, AIR_WALL_OUTLINE_GAMEPLAY_LAYER, CELESTIAL_AUTHORING_LAYER,
    PORTAL_EFFECT_LAYER, WEATHER_EFFECT_LAYER,
}; // 普通作者相机接收天体远景、正式传送点与天气层；应用对象由适配器显式追加。
use feathrise_editor_core::EditorApplicationViewLayers;
use feathrise_scene_content::{SceneContentRegistry, SceneManifestAsset}; // 根据场景范围一键取景；动态地形光标直接检测实际预览网格。
use feathrise_scene::DynamicClipmapAuthoringView; // 新场景 Clipmap 随当前离屏作者视图的轨道焦点移动。

use super::brush_cursor::{SCENE_EDITOR_BRUSH_RENDER_LAYER, uses_brush};
use super::camera_math::{
    MAX_EXTENT_METERS, MIN_EXTENT_METERS, PERSPECTIVE_FOV_RADIANS, focus_plane_height,
    navigation_axes, orbit_angles_from_view, orthographic_projection,
    orthographic_transform, perspective_transform,
}; // 视口系统只编排ECS；三维包围、裁剪和姿态由纯数学模块统一保证。
use super::view_memory::remember_editor_viewport;
use crate::authoring::{
    EditorAuthoringSession, SparseTerrainAuthoringState, WaterNetworkEditState,
    brush_overlaps_world_bounds,
};
use crate::components::{SceneEditorCamera, SceneEditorOwnedEntity, SceneEditorViewport};
use crate::resources::{
    SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewMemory, SceneEditorViewportState,
    SceneEditorWorldSession,
}; // 视口导航服从统一数值拖动鼠标所有权，空世界不创建场景相机。
use crate::types::EditorView;
use crate::ui::asset_preview::AssetPreviewState; // 普通摆放查询身份冻结当前Scene0或primitive选择。
use crate::ui::placement_transform::PlacementTransformInputState; // 已有普通摆放吸附优先于查看工具的平面回退。

/// 方向按钮只精确对准角度，观察中心、缩放与独立投影开关保持不变。
pub(crate) fn set_editor_view(
    editor: &mut SceneEditorState,
    viewport: Option<&mut SceneEditorViewportState>,
    view: EditorView,
) {
    editor.view = view;
    if let Some(viewport) = viewport {
        (viewport.yaw, viewport.pitch) = orbit_angles_from_view(view);
    } // 不修改作者修订或脏位，也不因快捷方向重建相机、RTT或场景。
}

/// 投影只由独立开关决定；快捷方向和右键旋转共用同一组轨道角。
pub(crate) fn editor_camera_pose(
    orthographic: bool,
    viewport: &SceneEditorViewportState,
) -> (Projection, Transform) {
    if orthographic {
        (
            Projection::Orthographic(orthographic_projection(
                focus_plane_height(viewport.extent),
                viewport.yaw,
                viewport.pitch,
                viewport.scene_half_extents,
            )),
            orthographic_transform(
                viewport.center,
                viewport.scene_center,
                viewport.scene_half_extents,
                viewport.yaw,
                viewport.pitch,
            ),
        )
    } else {
        (
            Projection::Perspective(PerspectiveProjection {
                fov: PERSPECTIVE_FOV_RADIANS,
                near: 0.2,
                far: (viewport.extent * 12.0).max(10_000.0),
                ..default()
            }),
            perspective_transform(
                viewport.center,
                viewport.extent,
                viewport.yaw,
                viewport.pitch,
            ),
        )
    }
}

/// 根据右键纵向拖动连续绕过顶视和底视；角度取模只防止长时间拖动累积精度损失。
fn orbit_pitch_after_drag(current: f32, motion_y: f32) -> f32 {
    (current + motion_y * 0.006 + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI // 姿态使用独立俯仰旋转，不再依赖世界Y作为唯一上方向，因此无需极点夹角限制。
}

/// 构造独立作者相机的公共集合；应用可追加自己的模型层，不能连带开启F1调试轮廓。
fn editor_viewport_render_layers(application: Option<&EditorApplicationViewLayers>) -> RenderLayers {
    let layers = RenderLayers::from_layers(&[
        0,                             // 地形、植被、水面和普通场景内容。
        SCENE_EDITOR_BRUSH_RENDER_LAYER, // 笔刷与边界作者辅助图形。
        PORTAL_EFFECT_LAYER,           // 当前场景正式传送范围与竖直光束，仍排除战斗技能层。
        WEATHER_EFFECT_LAYER,          // 正式云幕与Hanabi雨丝，不复制假预览效果。
        CELESTIAL_AUTHORING_LAYER,     // 动态太阳/月亮只进入F12离屏视图，不串入背后的玩法相机。
    ]);
    let layers = if let Some(application) = application { layers.union(&application.camera) } else { layers };
    layers.without(AIR_WALL_GAMEPLAY_LAYER)
    .without(AIR_WALL_OUTLINE_GAMEPLAY_LAYER) // F12只使用自己的作者轮廓，两种F1显示均不串入。
}

/// 正式场景就绪后才创建独立RTT；驻留交接保留原相机与纹理，未装配驻留的入口仍按原流程回收。
pub(crate) fn setup_editor_viewport(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    viewports: Query<(Entity, Option<&ViewportNode>), With<SceneEditorViewport>>,
    viewport: Option<Res<SceneEditorViewportState>>,
    world: SceneEditorWorldSession,
    editor: Res<SceneEditorState>,
    mut view_memory: ResMut<SceneEditorViewMemory>,
    residency: Option<Res<feathrise_scene_core::residency::SceneResidencyActivity>>,
    standalone: Option<Res<crate::standalone::StandaloneEditorMode>>,
    application_layers: Option<Res<EditorApplicationViewLayers>>,
    mut camera_layers: Query<&mut RenderLayers, With<SceneEditorCamera>>,
) {
    if !world.ready() {
        if residency
            .as_ref()
            .is_some_and(|state| state.enabled && state.active.is_some())
        {
            for (entity, node) in &viewports {
                if node.is_some() {
                    commands.entity(entity).remove::<ViewportNode>();
                }
            }
            return; // 加载及分帧恢复期间保留RTT和相机，资源所有权由驻留场景槽负责。
        }
        if let Some(viewport) = viewport {
            remember_editor_viewport(&mut view_memory, &editor, &viewport); // 加载页先回收旧RTT，必须在回收前冻结源场景视角与投影。
            commands.entity(viewport.camera).despawn();
            for (viewport_entity, _) in &viewports {
                commands.entity(viewport_entity).remove::<ViewportNode>();
            }
            images.remove(viewport._image.id());
            commands.remove_resource::<SceneEditorViewportState>();
        }
        return;
    }
    if let Some(viewport) = viewport {
        // 已有RTT也读取当前显式输入，适配撤销后不会保留上一次应用的可见层。
        if let Ok(mut layers) = camera_layers.get_mut(viewport.camera) {
            let next = editor_viewport_render_layers(application_layers.as_deref());
            if *layers != next {
                *layers = next;
            }
        }
        for (entity, node) in &viewports {
            if node.is_none_or(|node| node.camera != Some(viewport.camera)) {
                commands
                    .entity(entity)
                    .insert((ViewportNode::new(viewport.camera), FocusPolicy::Block));
            }
        }
        return; // BSN节点比世界更早存在，不能再依赖 Added；稳定帧也不得重复创建 RTT。
    }
    let Ok((viewport_entity, _)) = viewports.single() else {
        return;
    };
    let size = Extent3d {
        width: 1,  // 首帧只创建最小占位纹理，PostLayout按中央节点真实物理宽度调整。
        height: 1, // 首帧只创建最小占位纹理，PostLayout按中央节点真实物理高度调整。
        depth_or_array_layers: 1,
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("scene_editor_viewport_target"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    let image_handle = images.add(image);
    let center = Vec3::ZERO;
    let extent = 1_200.0;
    let (yaw, pitch) = orbit_angles_from_view(EditorView::Perspective);
    let camera = commands
        .spawn((
            Name::new("场景编辑器独立离屏相机"),
            SceneEditorCamera,
            DynamicClipmapAuthoringView {
                focus_xz: Vec2::new(center.x, center.z),
                focus_y: center.y,
            },
            SceneEditorOwnedEntity,
            DeferredRasterAuxiliaryCamera,
            WeatherPreviewCamera, // 天气系统只在F12打开时选择该离屏视口作为唯一视觉锚点。
            editor_viewport_render_layers(application_layers.as_deref()), // 应用按自己的真实模型层显式参与作者视图。
            Camera3d::default(),
            Hdr, // 四视图固定HDR，动态大气显隐只改变天空资格，不再引发HDR/LDR目标格式与色调映射换代。
            Camera {
                order: -3,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.16, 0.18, 0.17)),
                ..default()
            },
            RenderTarget::from(image_handle.clone()),
            Projection::Perspective(PerspectiveProjection {
                fov: PERSPECTIVE_FOV_RADIANS,
                near: 0.2,
                far: 80_000.0,
                ..default()
            }),
            perspective_transform(center, extent, yaw, pitch),
        ))
        .id();
    if standalone.is_some() {
        // 独立作者 World 没有游戏玩家，由自己的视图显式申请附近碰撞/流送；不会获得压草或水纹输入。
        // 一体游戏继续由真实玩家持有唯一锚点，不能把作者相机同时加进 single 查询。
        commands.entity(camera).insert(feathrise_scene::SceneRuntimeObserver);
    }
    commands.entity(viewport_entity).insert((
        ViewportNode::new(camera), // Bevy只在节点尺寸变化时同步RTT，画面与区域宽高比始终一致。
        FocusPolicy::Block,        // BSN不强行表达有缺陷的枚举模板，原生补齐交互组件。
    ));
    commands.insert_resource(SceneEditorViewportState {
        _image: image_handle,
        camera,
        center,
        scene_center: center, // 尚未选择场景时使用与默认轨道中心相同的安全深度锚点。
        scene_half_extents: Vec3::splat(extent * 0.5), // 首份清单采用前使用有界三轴默认包围盒。
        extent,
        yaw,
        pitch,
        framed_scene_id: None,
        render_size: UVec2::ONE, // 布局完成前使用合法最小值，下一帧同步为中央节点真实物理尺寸。
    });
}

/// 把Bevy视口节点已经计算出的物理尺寸同步给自定义导航与光标射线。
pub(crate) fn sync_editor_viewport_render_size(
    viewport: Option<ResMut<SceneEditorViewportState>>,
    viewport_nodes: Query<&ComputedNode, (With<SceneEditorViewport>, Changed<ComputedNode>)>,
) {
    let Some(mut viewport) = viewport else {
        return;
    };
    let Ok(node) = viewport_nodes.single() else {
        return;
    };
    if node.is_empty() { return; } // F12收起仅隐藏节点，保留上次有效导航尺寸；重新布局后再采用真实尺寸。
    let render_size = node.size().as_uvec2().max(UVec2::ONE); // 与ViewportNode内部采用相同换算，保持点击像素和相机目标严格一致。
    if viewport.render_size != render_size {
        viewport.render_size = render_size; // 只在窗口或三栏布局真正变化时通知后续导航系统。
    }
}

/// 新场景首次按真实范围取景；曾访问场景恢复会话视角，深度仍采用最新清单。
pub(crate) fn frame_selected_scene(
    mut editor: ResMut<SceneEditorState>,
    registry: Option<Res<SceneContentRegistry>>,
    manifests: Res<Assets<SceneManifestAsset>>,
    viewport: Option<ResMut<SceneEditorViewportState>>,
    mut view_memory: ResMut<SceneEditorViewMemory>,
    session: Option<Res<EditorAuthoringSession>>,
) {
    let Some(mut viewport) = viewport else {
        return;
    };
    let Some(scene_id) = editor.selected_scene_id.as_ref() else {
        return;
    };
    if viewport.framed_scene_id.as_ref() == Some(scene_id) {
        return;
    }
    if session.as_ref().is_some_and(|s| s.draft.as_ref().is_none_or(|d| &d.scene_id != scene_id)) {
        return; // 等作者草稿采用后再登记首次内容取景，避免加载空窗吞掉请求。
    }
    let Some(manifest) = registry
        .as_deref()
        .and_then(|registry| registry.handles(scene_id))
        .and_then(|handles| manifests.get(&handles.manifest))
    else {
        return;
    };
    if manifest.id != *scene_id {
        return; // 句柄换代期间不得把另一个场景的包围盒与缓存冒充目标取景。
    }
    let scene_id = scene_id.clone(); // 只有真正采用新目标时复制一个稳定身份，稳定帧不分配。
    // 空容器使用小型原点视角；内容就绪后的首次取景走同一有界内容入口。
    let scene_center = Vec3::ZERO;
    let scene_half_extents = Vec3::new(16.0, 0.0, 16.0);
    remember_editor_viewport(&mut view_memory, &editor, &viewport); // 兼容无需销毁RTT的切换场景，也必须先记源图再恢复目标。
    viewport.center = scene_center; // 新场景从真实三维量程中心取景，不能继续瞄准R16最低可表示高度。
    viewport.scene_center = scene_center; // 用户随后平移center时仍保留不变的正交观察轴深度锚点。
    viewport.scene_half_extents = scene_half_extents; // 正交任意旋转时沿实际方向投影深度，不能使用旧场景的裁剪包围。
    viewport.extent = 48.0;
    if let Some(saved) = view_memory.scenes.get(&scene_id) {
        editor.view = saved.view;
        editor.orthographic = saved.orthographic;
        viewport.center = saved.center;
        viewport.extent = saved.extent;
        viewport.yaw = saved.yaw;
        viewport.pitch = saved.pitch;
        editor.viewport_framing_requested = Some(crate::types::EditorViewportFraming::RefreshDepth);
    } else {
        editor.view = EditorView::Perspective;
        editor.orthographic = false;
        (viewport.yaw, viewport.pitch) = orbit_angles_from_view(EditorView::Perspective);
        editor.viewport_framing_requested = Some(crate::types::EditorViewportFraming::Scene);
    } // 只恢复观察元数据；相机实体、纹理尺寸、场景深度与草稿始终属于当前正式会话。
    viewport.framed_scene_id = Some(scene_id);
}

/// 仅在鼠标位于中央视口时执行中键平移、右键轨道旋转和滚轮缩放。
pub(crate) fn control_editor_viewport(
    mut motions: MessageReader<MouseMotion>,
    mut wheels: MessageReader<MouseWheel>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut editor: ResMut<SceneEditorState>,
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 拖动和弦及最终release帧都独占导航鼠标。
    spatial: Option<Res<super::spatial_brush::SpatialBrushState>>,
    water_edit: Res<WaterNetworkEditState>, // 左键拖点期间冻结滚轮、轨道和中键平移，避免光标世界点跳变。
    viewport: Option<ResMut<SceneEditorViewportState>>,
    viewport_nodes: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    domain: Option<Res<crate::game_authoring::scene_interaction::SceneInteractionState>>,
    terrain_objects: Option<Res<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    controls: Option<Res<super::modes::ViewportModeState>>,
) {
    let motion = motions
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let wheel = wheels.read().map(|event| event.y).sum::<f32>();
    let Some(mut viewport) = viewport else {
        return;
    };
    let hovered = viewport_nodes
        .single()
        .is_ok_and(|cursor| cursor.cursor_over());
    if !editor.open
        || editor.new_scene_dialog_open
        || editor.scene_switch_confirmation.is_some()
        || editor.scene_transition_in_progress
        || editor.pending_scene_transition.is_some()
        || !hovered
        || controls.as_ref().is_some_and(|controls| controls.pointer_over_controls)
        || numeric_scrub_mouse.blocked()
        || spatial.as_ref().is_some_and(|s| s.owns_mouse())
        || water_edit.dragging()
        || domain.as_ref().is_some_and(|state| state.owns_mouse())
        || terrain_objects.as_ref().is_some_and(|state| state.transforming)
    {
        return;
    } // 弹框覆盖或数字框拖出侧栏进入视口时不得旋转、平移或缩放相机。
    if wheel != 0.0 {
        let max_extent =
            MAX_EXTENT_METERS
                .max(viewport.extent)
                .max(super::camera_math::framing_extent(
                    viewport.scene_half_extents,
                    viewport.yaw,
                    viewport.pitch,
                    viewport.render_size,
                    editor.orthographic,
                )); // 只在滚轮边沿做固定八角计算，显式全图取景后的第一次缩放仍连续。
        viewport.extent =
            (viewport.extent * (-wheel * 0.11).exp()).clamp(MIN_EXTENT_METERS, max_extent);
    }
    if mouse.pressed(MouseButton::Right) && motion != Vec2::ZERO {
        if editor.view != EditorView::Perspective {
            editor.view = EditorView::Perspective; // 实际拖动后标记自由方向；保留当前角度、焦点、缩放和投影。
        }
        viewport.yaw = (viewport.yaw - motion.x * 0.006).rem_euclid(std::f32::consts::TAU);
        viewport.pitch = orbit_pitch_after_drag(viewport.pitch, motion.y); // 四个入口共享同一旋转手感，顶视也可向两个方向越过正上方。
    }
    if mouse.pressed(MouseButton::Middle) && motion != Vec2::ZERO {
        let meters_per_pixel = focus_plane_height(viewport.extent) / viewport.render_size.y as f32;
        let (right, up) = navigation_axes(viewport.yaw, viewport.pitch);
        viewport.center += (-right * motion.x + up * motion.y) * meters_per_pixel;
    }
}

/// 应用相机显隐、投影、姿态，并分别发布严格场景命中与允许跨边缘的笔刷命中。
pub(crate) fn sync_editor_viewport_camera(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut ground: ResMut<super::vegetation_ground::VegetationGroundCursor>, // 独立查询槽。
    mut objects_ground: Option<ResMut<crate::authoring::terrain_ground::TerrainGroundCursor>>,
    inputs: (
        Res<ButtonInput<MouseButton>>,
        Res<ButtonInput<KeyCode>>,
        Option<Res<feathrise_editor_core::EditorSettings>>,
        crate::authoring::mesh_lod::MeshLodInputGate,
    ), // 按压、擦除与吸附偏好共同冻结点击身份，收拢参数以保留合法Bevy系统签名。
    sparse_terrain: Res<SparseTerrainAuthoringState>, // 新图只查询已驻留局部块，不在光标帧触发IO。
    asset_preview: Res<AssetPreviewState>, // 新摆放异步落点绑定当前文件内目标，不能跨子资产采用。
    transform_state: Res<PlacementTransformInputState>, // 已有普通摆放的吸附点击使用稳定记录身份。
    domain: Option<Res<crate::game_authoring::scene_interaction::SceneInteractionState>>, // 角色子工具与作者层参与冻结身份。
    mode: Option<Res<super::modes::ViewportModeState>>, // 浮层按下不能武装松键后才完成的点击。
    boundary: Option<Res<crate::authoring::BoundaryEditState>>, // 边界点击与拖点共享查询槽但分别冻结用途。
    building_inputs: (
        Option<Res<crate::ui::building_presets::BuildingPlacementControls>>,
        Option<Res<crate::ui::courtyard::CourtyardUiState>>,
        Option<Res<feathrise_scene_content::BuildingPresetRegistry>>,
        Option<Res<feathrise_scene_content::CourtyardPresetRegistry>>,
    ), // 建筑参数、院落工具和目录修订共同参与点击身份。
    viewport: Option<Res<SceneEditorViewportState>>,
    cursor: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    mut camera_query: Query<
        (
            &mut Camera,
            &mut Projection,
            &mut Transform,
            &mut DynamicClipmapAuthoringView,
        ),
        With<SceneEditorCamera>,
    >,
) {
    let (mouse, keyboard, snap_settings, lod) = inputs;
    ground.captured = None; // 每帧必须由当前相机重新证明输入。
    editor.ground_click_world = None;
    editor.ground_click_is_lower_layer = false;
    if let Some(ground) = objects_ground.as_mut() { ground.ray = None; ground.identity = None; ground.terrain = None; }
    let epoch = ground.observe_input(
        mouse.pressed(MouseButton::Left),
        mouse.just_pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left),
    ); // 移动不换按压代次。
    let Some(viewport) = viewport else {
        editor.cursor_world = None; // 视口尚未建立时不能沿用上一帧普通命中。
        editor.cursor_triangle = None; // 建筑整平同样不能沿用上一帧真实表层身份。
        editor.brush_cursor_world = None; // 同时归还系统鼠标，避免不可见Gizmo仍声明所有权。
        editor.brush_cursor_triangle = None; // 洞穴分层命中身份必须与光标位置同步清除。
        return;
    };
    let Ok((mut camera, mut projection, mut transform, mut clipmap_view)) =
        camera_query.get_mut(viewport.camera)
    else {
        editor.cursor_world = None; // 相机实体失效时清除全部作者落点。
        editor.cursor_triangle = None; // 相机失效后旧三角不得继续用于建筑整平。
        editor.brush_cursor_world = None; // 下一系统据此恢复系统鼠标。
        editor.brush_cursor_triangle = None; // 不允许下一笔沿用上一相机的源三角。
        return;
    };
    camera.is_active = editor.open;
    clipmap_view.focus_xz = Vec2::new(viewport.center.x, viewport.center.z); // 透视机位远离地面时仍把最细环放在编辑焦点。
    clipmap_view.focus_y = viewport.center.y;
    let (next_projection, next_transform) = editor_camera_pose(editor.orthographic, &viewport);
    *projection = next_projection;
    *transform = next_transform;
    if editor.tool == crate::types::EditorTool::Water && editor.water_level_from_cursor_requested.is_none() {
        editor.cursor_world = None; editor.cursor_triangle = None;
        editor.brush_cursor_world = None; editor.brush_cursor_triangle = None;
        editor.brush_terrain_id = None;
        return; // 静水随后由水位平面发布；普通绘制不等待网格LOD或启动地形射线任务。
    }
    if !lod.ready() {
        editor.cursor_world = None;
        editor.cursor_triangle = None;
        editor.brush_cursor_world = None;
        editor.brush_cursor_triangle = None;
        editor.brush_terrain_id = None;
        return; // 相机仍可导航；分批恢复期间不产生任何旧LOD三角身份或延迟点击。
    }
    let ray = cursor
        .single()
        .ok()
        .filter(|cursor| !editor.new_scene_dialog_open && cursor.cursor_over())
        .and_then(|cursor| cursor.normalized)
        .and_then(|normalized| {
            let viewport_position = Vec2::new(
                (normalized.x + 0.5) * viewport.render_size.x as f32,
                (normalized.y + 0.5) * viewport.render_size.y as f32,
            );
            camera
                .viewport_to_world(&GlobalTransform::from(*transform), viewport_position)
                .ok()
        });
    let supports_brush = (uses_brush(editor.tool) || editor.domain_brush_active)
        && (matches!(editor.view, EditorView::Perspective | EditorView::Top)
            || editor.tool.is_volume_sculpt()); // 体积球刷消费真实三维射线，可从侧面画洞壁；二维画布仍限制观察方向。
    let relocation = session.draft.as_ref().and_then(|draft| {
        let (scene_id, record_index, placement_id, structure_revision, authoring_revision) =
            transform_state.relocation_ground_identity()?;
        (scene_id == &draft.scene_id
            && structure_revision == session.placement_structure_revision
            && authoring_revision == session.revision
            && draft
                .placements
                .placements
                .get(record_index)
                .is_some_and(|record| record.id == placement_id))
        .then(|| {
            let building = draft.placements.placements[record_index]
                .building_preset_id
                .is_some();
            let controls = building_inputs.0.as_deref().map_or(
                [0; 4],
                crate::authoring::buildings::clipmap_controls_identity,
            );
            transform_state.relocation_ground_target(
                building,
                controls,
                building_inputs
                    .2
                    .as_deref()
                    .map_or(0, |registry| registry.revision),
            )
        })
        .flatten()
    }); // 建筑仅复用真实点击XZ种子，最终Y仍由五点地基决定。
    let catalogs = [
        building_inputs
            .2
            .as_deref()
            .map_or(0, |registry| registry.revision),
        building_inputs
            .3
            .as_deref()
            .map_or(0, |registry| registry.revision),
    ];
    let courtyard_target = building_inputs
        .1
        .as_deref()
        .and_then(|ui| crate::authoring::courtyards::clipmap_cursor_target(ui, catalogs));
    let precise_clipmap_tool = matches!(
        editor.tool,
        crate::types::EditorTool::Raise
            | crate::types::EditorTool::VolumeDig
            | crate::types::EditorTool::VolumeFill
            | crate::types::EditorTool::VolumeSmooth
            | crate::types::EditorTool::VolumeFlatten
            | crate::types::EditorTool::Lower
            | crate::types::EditorTool::Smooth
            | crate::types::EditorTool::Flatten
            | crate::types::EditorTool::Surface
            | crate::types::EditorTool::Puddles
            | crate::types::EditorTool::Vegetation
            | crate::types::EditorTool::Water
            | crate::types::EditorTool::RiverSpline
            | crate::types::EditorTool::Waterfall
            | crate::types::EditorTool::PlaceAsset
            | crate::types::EditorTool::Portal
            | crate::types::EditorTool::Boundary
            | crate::types::EditorTool::Building
    ) || relocation.is_some()
        || courtyard_target.is_some()
        || (matches!(editor.tool, crate::types::EditorTool::Domain(_))
            && domain
                .as_ref()
                .is_some_and(|state| state.precise_cursor));
    if precise_clipmap_tool
        && session.draft.as_ref().is_some_and(|draft| {
            draft.manifest.terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap
        })
    {
        // 已接入的Clipmap作者工具共享真实可见面XZ和基础作者Y，缺页不回退平面。
        editor.cursor_world = None;
        editor.cursor_triangle = None; // 本帧尚无精确地面证据。
        editor.brush_cursor_world = None;
        editor.brush_cursor_triangle = None; // 等待或未命中禁止落笔。
        if editor.open
            && !mode.as_ref().is_some_and(|mode| mode.pointer_over_controls)
            && let Some(ray) = ray
            && let Some(draft) = session.draft.as_ref()
        {
            let target = if relocation.is_some() {
                relocation // 已武装吸附优先于普通工具，建筑和普通资产各自冻结用途。
            } else if courtyard_target.is_some() {
                courtyard_target
            } else {
                match editor.tool {
                    crate::types::EditorTool::VolumeDig
                    | crate::types::EditorTool::VolumeFill
                    | crate::types::EditorTool::VolumeSmooth
                    | crate::types::EditorTool::VolumeFlatten => Some(
                        super::vegetation_ground::GroundCursorTarget::VolumeSculpt(editor.tool),
                    ), // 三维雕刻沿真实洞壁命中，不能回投隐藏的高度面。
                    crate::types::EditorTool::Building => {
                        building_inputs.0.as_deref().and_then(|controls| {
                            crate::authoring::buildings::clipmap_cursor_target(
                                &editor,
                                controls,
                                catalogs[0],
                            )
                        })
                    }
                    crate::types::EditorTool::Raise
                    | crate::types::EditorTool::Lower
                    | crate::types::EditorTool::Smooth
                    | crate::types::EditorTool::Flatten => Some(
                        super::vegetation_ground::GroundCursorTarget::TerrainSculpt(editor.tool),
                    ), // 切换雕刻工具也使上一工具的迟到射线失效。
                    crate::types::EditorTool::Surface => draft
                        .surface
                        .as_ref()
                        .and_then(|surface| surface.layers.get(editor.selected_surface_layer))
                        .map(
                            |layer| super::vegetation_ground::GroundCursorTarget::Surface {
                                layer_id: layer.id.clone(),
                                preset_id: editor.surface_selected_preset_id.clone(),
                            },
                        ), // 只查询已显式选择的真实作者层，零层不能借笔刷隐式建层。
                    crate::types::EditorTool::Puddles => {
                        Some(super::vegetation_ground::GroundCursorTarget::Puddles)
                    }
                    crate::types::EditorTool::Domain(_) => domain.as_ref().and_then(|state| state.cursor_target.clone()),
                    crate::types::EditorTool::Portal => editor
                        .portal_target
                        .clone()
                        .map(super::vegetation_ground::GroundCursorTarget::Portal),
                    crate::types::EditorTool::Boundary => {
                        crate::authoring::boundary::clipmap_cursor_target(
                            &session,
                            boundary.as_deref(),
                            keyboard.pressed(KeyCode::ControlLeft)
                                || keyboard.pressed(KeyCode::ControlRight),
                        )
                    }
                    crate::types::EditorTool::Vegetation => draft
                        .vegetation
                        .as_ref()
                        .and_then(|v| v.authoring.as_ref())
                        .and_then(|a| a.layers.get(editor.selected_vegetation_layer))
                        .map(|layer| {
                            super::vegetation_ground::GroundCursorTarget::Vegetation(
                                layer.id.clone(),
                            )
                        }),
                    crate::types::EditorTool::Water => {
                        Some(super::vegetation_ground::GroundCursorTarget::StillWater(
                            draft
                                .water
                                .as_ref()
                                .and_then(|water| water.layers.get(editor.selected_water_layer))
                                .map(|layer| layer.id.clone()),
                        ))
                    }
                    crate::types::EditorTool::RiverSpline => draft
                        .water
                        .as_ref()
                        .and_then(|water| water.rivers.get(editor.selected_water_river))
                        .map(|river| {
                            super::vegetation_ground::GroundCursorTarget::River(river.id.clone())
                        }),
                    crate::types::EditorTool::Waterfall => draft
                        .water
                        .as_ref()
                        .and_then(|water| water.waterfalls.get(editor.selected_waterfall))
                        .map(|waterfall| {
                            super::vegetation_ground::GroundCursorTarget::Waterfall(
                                waterfall.id.clone(),
                            )
                        }),
                    crate::types::EditorTool::PlaceAsset => {
                        editor.selected_asset.as_ref().and_then(|asset_path| {
                            asset_preview.placement_target_for(asset_path).ok().map(
                                |asset_target| {
                                    super::vegetation_ground::GroundCursorTarget::PlaceAsset {
                                        asset_path: asset_path.clone(),
                                        asset_target,
                                    }
                                },
                            )
                        })
                    } // 文件或子资产尚未形成合法持久目标时不启动无归属查询。
                    _ => None,
                }
            }; // 功能域和稳定层/实例共同冻结，迟到结果不能跨工具采用。
            let point_query = target
                .as_ref()
                .is_some_and(super::vegetation_ground::GroundCursorTarget::is_author_click);
            let placement_snap = target.as_ref().and_then(|target| {
                use super::vegetation_ground::GroundCursorTarget;
                let origin = match target {
                    GroundCursorTarget::PlaceAsset { .. } | GroundCursorTarget::Building { .. } => [0.0; 3],
                    GroundCursorTarget::RelocatePlacement(_) | GroundCursorTarget::RelocateBuilding { .. } => {
                        let (_, index, _, _, _) = transform_state.relocation_ground_identity()?;
                        draft.placements.placements.get(index)?.translation
                    }
                    _ => return None,
                };
                let settings = snap_settings.as_ref().map_or_else(feathrise_editor_core::EditorSnapSettings::default, |settings| settings.editor_snapping);
                let control = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
                Some(if matches!(target, GroundCursorTarget::Building { .. } | GroundCursorTarget::RelocateBuilding { .. }) {
                    building_inputs.0.as_ref().map_or_else(|| settings.translation_snap(control, origin), |controls| controls.snap_policy(settings, control, origin))
                } else { settings.translation_snap(control, origin) })
            });
            let boundary_drag = matches!(
                target.as_ref(),
                Some(super::vegetation_ground::GroundCursorTarget::BoundaryDrag { .. }
                    | super::vegetation_ground::GroundCursorTarget::Domain(crate::game_authoring::scene_interaction::DomainGroundTarget { continuous: true, .. }))
            );
            if !supports_brush && !point_query && !boundary_drag {
                return; // 无有效作者目标时保持全部落点为空，不回到水平面。
            }
            let author_click = mouse.just_pressed(MouseButton::Left)
                && point_query
                && !editor.suppress_world_pick_this_frame
                && !editor.close_requested
                && !editor.save_requested
                && !session.saving
                && (placement_snap.is_some() || matches!(
                    target.as_ref(),
                    Some(
                        super::vegetation_ground::GroundCursorTarget::BoundaryPoint {
                            insert: true,
                            ..
                        }
                    )
                ) || (!keyboard.pressed(KeyCode::ControlLeft)
                    && !keyboard.pressed(KeyCode::ControlRight)));
            let authoring_revision = (point_query
                || matches!(
                    target.as_ref(),
                    Some(
                        super::vegetation_ground::GroundCursorTarget::Domain(crate::game_authoring::scene_interaction::DomainGroundTarget { continuous: true, .. })
                            | super::vegetation_ground::GroundCursorTarget::BoundaryDrag { .. }
                    )
                ))
            .then_some(session.revision);
            ground.captured = Some((
                super::vegetation_ground::GroundCursorContext {
                    // 场景修订由随后采用阶段补入。
                    scene_id: draft.scene_id.clone(),
                    target,
                    placement_snap,
                    terrain_revision: session.terrain_revision,
                    surface_revision: [0; 3],
                    missing_pages: [UVec4::ZERO; 16], // 场景和作者目标身份。
                    camera_transform: transform.to_matrix().to_cols_array().map(f32::to_bits),
                    view: editor.view, // 当前方向及独立投影共同冻结真实镜头。
                    orthographic: editor.orthographic,
                    extent: viewport.extent.to_bits(),
                    render_size: viewport.render_size,
                    input_epoch: epoch, // 缩放与按压身份。
                    held: mouse.pressed(MouseButton::Left),
                    modifiers: u8::from(
                        keyboard.pressed(KeyCode::ShiftLeft)
                            || keyboard.pressed(KeyCode::ShiftRight),
                    ) | (u8::from(
                        keyboard.pressed(KeyCode::ControlLeft)
                            || keyboard.pressed(KeyCode::ControlRight),
                    ) << 1), // 不跨擦除或选择模式。
                    authoring_revision,
                },
                ray,
                author_click,
            )); // 后台使用冻结射线，不能把旧点标成新射线。
        } // 无输入时后续resolve清除旧状态。
        return; // 精确Clipmap工具绝不落入水平面替代路径。
    } // GLB及其余工具继续原有拾取。
    let (cursor_world, cursor_triangle, brush_cursor_world, brush_cursor_triangle) = ray
        .map(|ray| {
            let Some(draft) = session.draft.as_ref() else {
                let hit = ray
                    .intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))
                    .map(|distance| ray.get_point(distance));
                return (hit, None, supports_brush.then_some(hit).flatten(), None); // 尚无草稿时保持既有Y=0编辑平面行为。
            };
            if draft.manifest.terrains.is_some() {
                if let Some(ground) = objects_ground.as_mut() {
                    let request = crate::authoring::terrain_ground::CursorRay { origin: ray.origin, direction: *ray.direction, epoch };
                    let relocation = transform_state.relocation_ground_identity()
                        .filter(|(map, index, id, structure, author)| *map == &draft.scene_id
                            && *structure == session.placement_structure_revision && *author == session.revision
                            && draft.placements.placements.get(*index).is_some_and(|record| record.id == *id))
                        .map(|(_, index, id, structure, author)| (index, id.to_owned(), structure, author));
                    let (translation_snap, click_snap) = if editor.tool == crate::types::EditorTool::PlaceAsset || transform_state.relocation_armed() {
                        let origin = relocation.as_ref()
                            .and_then(|(index, _, _, _)| draft.placements.placements.get(*index))
                            .map_or([0.0; 3], |record| record.translation);
                        let settings = snap_settings.as_ref().map_or_else(feathrise_editor_core::EditorSnapSettings::default, |settings| settings.editor_snapping);
                        let control = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
                        (Some(settings.translation_snap(false, origin)), Some(settings.translation_snap(control, origin)))
                    } else { (None, None) }; // 身份只含偏好；实际Ctrl语义随原始点击冻结，松键后仍采用一次。
                    let identity = crate::authoring::terrain_ground::CursorIdentity { scene_id: draft.scene_id.clone(), revision: session.revision,
                        terrain_revision: session.terrain_revision,
                        ground_snapshot: draft.terrain_ground.as_ref().filter(|snapshot| snapshot.scene_id == draft.scene_id
                            && snapshot.revision == session.terrain_revision).map(crate::authoring::terrain_ground::CursorGroundSnapshot::capture),
                        tool: editor.tool, asset: editor.selected_asset.clone(), portal: editor.portal_target.clone(),
                        asset_target: editor.selected_asset.as_ref().and_then(|path| asset_preview.placement_target_for(path).ok()),
                        camera: transform.to_matrix(), size: viewport.render_size,
                        camera_entity: viewport.camera, projection: camera.clip_from_view(),
                        view: editor.view, orthographic: editor.orthographic, extent: viewport.extent.to_bits(),
                        visibility: std::sync::Arc::as_ptr(&editor.terrain_hidden) as usize, translation_snap, relocation };
                    if mouse.just_pressed(MouseButton::Left) && (transform_state.relocation_armed() || matches!(editor.tool, crate::types::EditorTool::PlaceAsset
                        | crate::types::EditorTool::Portal | crate::types::EditorTool::Domain(_) | crate::types::EditorTool::Building | crate::types::EditorTool::Boundary
                        | crate::types::EditorTool::RiverSpline | crate::types::EditorTool::Waterfall)) {
                        // 水网逐点同样消费冻结的真实地面单击，不能拿上一帧悬停补点。
                        ground.capture_click(identity.clone(), request, click_snap);
                    }
                    ground.ray = Some(request); ground.identity = Some(identity);
                }
                return (None, None, None, None); // 后台真实表面查询完成前等待，不创建隐式平面。
            }
            if let (Some(mesh), Some(index)) = (
                draft.terrain_mesh.as_deref(),
                draft.terrain_editing_index.as_deref(),
            ) {
                let hit = index.ray_intersection(
                    &mesh.positions,
                    &mesh.indices,
                    ray.origin.to_array(),
                    (*ray.direction).to_array(),
                ); // 保留最近真实三角身份，洞穴同投影多层不能只靠X/Z区分。
                let point = hit.map(|hit| Vec3::from_array(hit.position)); // 普通工具继续只读取世界命中点。
                let cursor_triangle = hit.map(|hit| hit.triangle); // 普通建筑点击也冻结实际命中层，不再依赖笔刷工具门。
                let brush = supports_brush.then_some(point).flatten(); // 只有支持笔刷的视角发布雕刻中心。
                let triangle = brush.and(hit.map(|hit| hit.triangle)); // 三角身份与有效笔刷位置共同存在。
                return (point, cursor_triangle, brush, triangle); // 光标和笔刷均以实际预览三角面为准。
            }
            // 稀疏空白块世界高度固定为Y=0；旧GLTF仍退到清单基准高。
            let sparse =
                draft.manifest.terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap;
            let plane_height = if sparse {
                0.0
            } else {
                draft.manifest.terrain.base_height
            };
            let fallback_plane_hit = ray
                .intersect_plane(Vec3::Y * plane_height, InfinitePlane3d::new(Vec3::Y))
                .map(|distance| ray.get_point(distance))
                .map(|mut point| {
                    if sparse
                        && let Some(height) = sparse_terrain
                            .sample_loaded_world_height(f64::from(point.x), f64::from(point.z))
                    {
                        point.y = height; // 已驻留块让压平光标贴近真实作者高度。
                    }
                    point
                });
            let plane_hit = sparse
                .then(|| sparse_terrain.intersect_loaded_height_ray(ray.origin, *ray.direction))
                .flatten()
                .or(fallback_plane_hit); // 未驻留真实块时才保留确定性零平面托底。
            let strict = plane_hit.filter(|point| {
                point.x >= draft.manifest.world_min[0]
                    && point.x <= draft.manifest.world_max[0]
                    && point.z >= draft.manifest.world_min[1]
                    && point.z <= draft.manifest.world_max[1]
            });
            let brush = supports_brush
                .then_some(plane_hit)
                .flatten()
                .filter(|point| {
                    brush_overlaps_world_bounds(
                        *point,
                        editor.brush_radius,
                        draft.manifest.world_min,
                        draft.manifest.world_max,
                    )
                });
            (strict, None, brush, None)
        })
        .unwrap_or((None, None, None, None));
    editor.cursor_world = cursor_world; // 摆放、传送、边界与取水位继续读取严格命中。
    editor.cursor_triangle = cursor_triangle; // 建筑联合整平只接受与当前世界点同帧发布的真实三角。
    editor.brush_cursor_world = brush_cursor_world; // 圆形笔刷单独读取可裁剪到场景边缘的中心。
    editor.brush_cursor_triangle = brush_cursor_triangle; // 地形笔刷据此只修改命中层的局部拓扑连通分量。
}

#[cfg(test)]
#[path = "systems_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "navigation_tests.rs"]
mod navigation_tests;

#[cfg(test)]
#[path = "view_memory_tests.rs"]
mod view_memory_tests;
