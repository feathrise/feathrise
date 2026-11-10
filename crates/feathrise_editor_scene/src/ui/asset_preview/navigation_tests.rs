//! 无窗口/GPU的真实ECS导航回归，覆盖输入隔离、缩放、reset与窗口尺寸变化。

use feathrise_editor_ui::UiInteraction;
use super::*; // 使用真实预览输入和视图系统。
use crate::authoring::WaterNetworkEditState; // 场景相机原有输入门所需的最小状态。
use crate::components::{SceneEditorRightPanelScroll, SceneEditorViewport}; // 构造可观测外层滚动和场景视口。
use crate::resources::SceneEditorViewportState; // 验证预览不会误改实际场景相机状态。
use crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard; // 复用真实统一鼠标门。

/// 只记录测试将直接调整的少数实体，不启动操作系统窗口。
struct Fixture {
    app: App,         // 最小ECS世界与真实系统调度。
    canvas: Entity,   // 可变画布尺寸和命中。
    image: Entity,    // 原比例图片布局与UiTransform。
    window: Entity,   // 滚轮窗口身份与焦点。
    camera: Entity,   // 独立预览相机。
    panel: Entity,    // 外层右栏滚动位置。
    scene_view: Entity, // 模拟拖出后进入中央场景。
}

/// 用真实资源和查询初始化系统，顺便覆盖B0001访问冲突。
fn fixture(image: bool) -> Fixture {
    let mut app = App::new(); // 不添加Winit或Render插件。
    app.init_resource::<SceneEditorState>() // 只读作者会话。
        .init_resource::<SceneEditorSidebarState>() // 资产页显隐。
        .init_resource::<SceneEditorNumericScrubMouseGuard>() // 真实和弦释放门。
        .init_resource::<ActiveNumericInputScrub>() // 共享 UI 控件公共拖动身份。
        .init_resource::<SceneBrowserDragState>() // 归类拖动保持未激活。
        .init_resource::<SceneBrowserState>() // 文件夹模态默认关闭。
        .init_resource::<SceneDeletionTask>() // 删除确认默认关闭。
        .init_resource::<TerrainExternalChangeState>() // 外部冲突默认关闭。
        .init_resource::<WaterNetworkEditState>() // 场景相机水网拖动门。
        .init_resource::<AssetPreviewNavigation>() // 初始100%完整取景。
        .init_resource::<ButtonInput<MouseButton>>() // 测试手动推进按键边沿。
        .add_message::<MouseMotion>() // 原始鼠标移动。
        .add_message::<MouseWheel>() // 按窗口过滤的原始滚轮。
        .add_systems(
            Update,
            (
                control_asset_preview,                         // 先发布预览手势及滚轮所有权。
                update_scene_editor_numeric_scrub_mouse_guard,   // 同帧阻止其他作者鼠标。
                crate::ui::systems::scroll_editor_side_panels, // 真实外层滚动消费者。
                crate::viewport::control_editor_viewport,      // 真实场景相机消费者。
                sync_asset_preview_view,                       // 采用图片或模型视图变换。
                super::super::presentation::sync_asset_preview_controls, // 初始化真实工具条查询。
            )
                .chain(),
        ); // 与正式插件保持相同关键先后关系。
    let window = app
        .world_mut()
        .spawn((
            PrimaryWindow,
            Window {
                focused: true,
                ..default()
            },
        ))
        .id(); // 只有Component，不创建真实窗口。
    let canvas = app
        .world_mut()
        .spawn((
            AssetPreviewCanvas, // 唯一预览画布。
            ComputedNode {
                size: Vec2::splat(300.0),
                inverse_scale_factor: 1.0,
                ..default()
            }, // 明确模拟物理布局。
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            }, // 从画布中心开始。
            UiInteraction::Hovered, // 没有前方模态遮挡。
        ))
        .id(); // 后续测试会切换窗口比例与命中。
    let image_node = app
        .world_mut()
        .spawn((
            AssetPreviewImage, // 唯一图片UI。
            ComputedNode {
                size: Vec2::new(300.0, 200.0),
                ..default()
            }, // 原图在画布中的适配比例。
            UiTransform::default(), // 初始无缩放或平移。
        ))
        .id(); // 图片无需实际解码即可验证视觉矩阵。
    let camera = app
        .world_mut()
        .spawn((
            AssetPreviewCamera,
            Transform::default(),
            Projection::default(),
        ))
        .id(); // 独立可观测预览相机。
    let panel = app
        .world_mut()
        .spawn((
            SceneEditorRightPanelScroll, // 真实右栏滚动标记。
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            }, // 画布属于右侧面板内部。
            ComputedNode {
                size: Vec2::new(300.0, 600.0),
                content_size: Vec2::new(300.0, 1200.0),
                inverse_scale_factor: 1.0,
                ..default()
            }, // 内容足够长，串滚动有可观测结果。
            ScrollPosition(Vec2::new(0.0, 100.0)), // 不从边界开始，确保正负滚动都能测到。
        ))
        .id(); // 右栏保持原始100位置。
    let scene_view = app
        .world_mut()
        .spawn((SceneEditorViewport, RelativeCursorPosition::default()))
        .id(); // 场景初始未悬停。
    app.insert_resource(SceneEditorViewportState {
        _image: Handle::default(),      // 测试无需实际离屏纹理。
        camera: Entity::PLACEHOLDER,    // 场景控制系统只修改资源，不访问该实体。
        center: Vec3::ZERO,             // 可检测平移污染。
        scene_center: Vec3::ZERO,       // 最小场景几何中心。
        scene_half_extents: Vec3::ONE,  // 正交数学所需范围。
        extent: 100.0,                  // 可检测滚轮污染。
        yaw: 0.2,                       // 可检测水平旋转污染。
        pitch: 0.4,                     // 可检测俯仰污染。
        framed_scene_id: None,            // 本测试不涉及自动场景取景。
        render_size: UVec2::splat(600), // 场景自身物理视口尺寸。
    }); // 与资产预览完全独立的相机状态。
    let path = if image { "test.png" } else { "test.glb" }.to_owned(); // 稳定当前文件身份。
    app.world_mut().resource_mut::<SceneEditorState>().open = true; // 模拟已打开F12。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some(path.clone()); // 作者只选择文件，不修改场景。
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets; // 只开放资产页。
    app.insert_resource(AssetPreviewState {
        selected_path: Some(path), // 与作者选择严格匹配。
        content: if image {
            AssetPreviewContent::Image {
                handle: Handle::default(),
                phase: AssetPreviewPhase::Ready,
            } // 合法图片已完成解码。
        } else {
            AssetPreviewContent::Model {
                handle: Handle::default(),
                root: Entity::PLACEHOLDER,
                phase: AssetPreviewPhase::Ready,
            } // 合法Scene0已完成隔离。
        }, // 不需要真实IO资产。
        model_bounds: (!image).then_some(AssetPreviewModelBounds {
            minimum: Vec3::NEG_ONE,
            maximum: Vec3::ONE,
        }), // 模型固定缓存范围。
        camera: Some(camera),      // 只声明专用预览相机。
        ..default()                // 与加载无关的状态保持安全默认。
    }); // 输入系统严格读取当前Ready状态。
    app.update(); // 首帧完成系统初始化和默认相机应用。
    Fixture {
        app,
        canvas,
        image: image_node,
        window,
        camera,
        panel,
        scene_view,
    } // 返回小型测试夹具。
}

/// 每个模拟渲染帧后清除边沿，保留持续按住电平。
fn step(app: &mut App) {
    app.update(); // 推进所有真实消费者。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear(); // 对齐InputPlugin的每帧边沿生命周期。
}

/// 发送一条明确属于主窗口的滚轮事件。
fn wheel(fixture: &mut Fixture, unit: MouseScrollUnit, y: f32) {
    fixture.app.world_mut().write_message(MouseWheel {
        unit,                                         // 测试明确区分整档和像素单位。
        x: 0.0,                                       // 水平滚轮不参与预览倍率。
        y,                                            // 本帧纵向缩放增量。
        window: fixture.window,                       // 只属于测试主窗口。
        phase: bevy::input::touch::TouchPhase::Moved, // 普通鼠标沿用Bevy标准滚动阶段。
    }); // 不伪造全局滚轮状态。
    step(&mut fixture.app); // 同一帧由预览和右栏各自读取。
}

#[test] // 预览内缩放与预览外列表滚动必须互斥。
fn wheel_zooms_preview_without_scrolling_sidebar_or_scene() {
    let mut test = fixture(true); // 真实图片预览消费者。
    wheel(&mut test, MouseScrollUnit::Line, 1.0); // 悬停画布向上滚一档。
    assert!(
        test.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .zoom
            > 1.0
    ); // 图片已放大。
    assert_eq!(
        test.app
            .world()
            .get::<ScrollPosition>(test.panel)
            .unwrap()
            .y,
        100.0
    ); // 外层面板没有跟着动。
    assert_eq!(
        test.app.world().resource::<SceneEditorViewportState>().extent,
        100.0
    ); // 场景距离不变。
    test.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(test.canvas)
        .unwrap()
        .cursor_over = false; // 光标移到列表区域。
    wheel(&mut test, MouseScrollUnit::Line, 1.0); // 此时滚轮归还右栏。
    assert_eq!(
        test.app
            .world()
            .get::<ScrollPosition>(test.panel)
            .unwrap()
            .y,
        73.0
    ); // 列表按既有27px每档滚动。
    assert!(!test.app.world().resource::<SceneEditorState>().dirty); // 整个预览过程不修改作者数据。
}

#[test] // 触控板与鼠标只差单位换算，不允许像素事件产生巨幅缩放。
fn pixel_wheel_matches_line_wheel() {
    let mut line = fixture(false); // 一般鼠标。
    let mut pixel = fixture(false); // 像素滚轮设备。
    wheel(&mut line, MouseScrollUnit::Line, 1.0); // 一档。
    wheel(&mut pixel, MouseScrollUnit::Pixel, 27.0); // 相同逻辑位移。
    assert_eq!(
        line.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .zoom,
        pixel
            .app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .zoom
    ); // 两者手感一致。
}

#[test] // 中央画布不依赖右栏，但仍遵守完整文件身份和全局模态输入门。
fn asset_center_navigation_works_with_hidden_sidebar_and_pinned_source() {
    let mut test = fixture(true);
    let path = test.app.world().resource::<AssetPreviewState>().selected_path.clone().unwrap();
    test.app.init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    test.app.world_mut().resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>().active_tab =
        crate::ui::vegetation_preset_editor::PresetWorkspaceTab::AssetPreview;
    test.app.insert_resource(super::super::AssetPreviewWorkspace {
        expanded: Some(super::super::types::ExpandedAssetPreview {
            id: 1,
            path, return_tab: crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Scene,
            view: default(), target: default(), restore_cursor: 0, audio_position: 0.0,
        }), ..default()
    });
    test.app.init_resource::<crate::resources::SceneEditorLayoutState>();
    test.app.world_mut().resource_mut::<crate::resources::SceneEditorLayoutState>().inspector_visible = false;
    test.app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Scene;
    test.app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some("another.json".into());
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    let view = test.app.world().resource::<AssetPreviewNavigation>().view;
    assert!(view.zoom > 1.0);
    assert!(test.app.world().resource::<AssetPreviewNavigation>().owns_wheel());
    assert_eq!(test.app.world().get::<ScrollPosition>(test.panel).unwrap().y, 100.0);
    test.app.world_mut().resource_mut::<SceneEditorState>().new_scene_dialog_open = true;
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    assert_eq!(test.app.world().resource::<AssetPreviewNavigation>().view, view);
    test.app.world_mut().resource_mut::<SceneEditorState>().new_scene_dialog_open = false;
    test.app.world_mut().resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>().active_tab =
        crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Scene;
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    assert_eq!(test.app.world().resource::<AssetPreviewNavigation>().view, view); // 后台中央标签不接收旧矩形上的滚轮。
}

#[test] // 中央模型右键旋转时拖出画布仍保持捕获，不能移动场景相机。
fn asset_center_model_drag_keeps_scene_camera_unchanged() {
    let mut test = fixture(false);
    let path = test.app.world().resource::<AssetPreviewState>().selected_path.clone().unwrap();
    test.app.init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    test.app.world_mut().resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>().active_tab =
        crate::ui::vegetation_preset_editor::PresetWorkspaceTab::AssetPreview;
    test.app.insert_resource(super::super::AssetPreviewWorkspace {
        expanded: Some(super::super::types::ExpandedAssetPreview {
            id: 1,
            path, return_tab: crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Scene,
            view: default(), target: default(), restore_cursor: 0, audio_position: 0.0,
        }), ..default()
    });
    test.app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Scene;
    let map = test.app.world().resource::<SceneEditorViewportState>().yaw;
    test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
    step(&mut test.app); // 按下帧建立捕获，后续帧才应用鼠标位移，与真实手势相同。
    test.app.world_mut().write_message(MouseMotion { delta: Vec2::new(30.0, 12.0) });
    step(&mut test.app);
    let view = test.app.world().resource::<AssetPreviewNavigation>().view;
    assert_ne!(view.yaw, super::super::view_math::AssetPreviewView::default().yaw);
    assert!(test.app.world().resource::<AssetPreviewNavigation>().dragging());
    test.app.world_mut().get_mut::<RelativeCursorPosition>(test.canvas).unwrap().cursor_over = false;
    test.app.world_mut().write_message(MouseMotion { delta: Vec2::new(15.0, 0.0) });
    step(&mut test.app);
    assert_ne!(test.app.world().resource::<AssetPreviewNavigation>().view.yaw, view.yaw);
    assert_eq!(test.app.world().resource::<SceneEditorViewportState>().yaw, map);
}

#[test] // 捕获拖出后仍只旋转模型，release帧不得让场景接手。
fn model_drag_outside_canvas_keeps_scene_camera_unchanged_until_release() {
    let mut test = fixture(false); // 使用模型右键轨道。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right); // 起始点在预览内。
    step(&mut test.app); // 建立明确鼠标所有权。
    let initial_view = test.app.world().resource::<AssetPreviewNavigation>().view; // 同时记录初始方位与俯仰，验证真实输入方向。
    test.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(test.canvas)
        .unwrap()
        .cursor_over = false; // 拖出预览区。
    test.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(test.scene_view)
        .unwrap()
        .cursor_over = true; // 光标进入场景视口。
    test.app.world_mut().write_message(MouseMotion {
        delta: Vec2::new(40.0, 25.0),
    }); // 保持右键移动。
    step(&mut test.app); // 两个视口消费者都真实运行。
    assert_ne!(
        test.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .yaw,
        initial_view.yaw
    ); // 模型继续旋转。
    assert!(
        test.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .pitch
            > initial_view.pitch
    ); // 鼠标向下拖动时预览俯仰增加，与中间3D视口保持一致。
    assert_eq!(
        test.app.world().resource::<SceneEditorViewportState>().yaw,
        0.2
    ); // 场景不能跟着转。
    assert_eq!(
        test.app.world().resource::<SceneEditorViewportState>().pitch,
        0.4
    ); // 场景俯仰也不变。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right); // 原按键松开。
    step(&mut test.app); // release帧仍发布独占门。
    assert!(
        test.app
            .world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    ); // 不能同帧穿透。
    step(&mut test.app); // 三键静默后的下一帧。
    assert!(
        !test
            .app
            .world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    ); // 正常归还鼠标。
}

#[test] // 先在其他区域按住右键再移入预览不应抢到控制权。
fn dragging_from_outside_and_chords_do_not_start_preview_navigation() {
    let mut mouse = ButtonInput::<MouseButton>::default(); // 没有既有鼠标所有者。
    mouse.press(MouseButton::Right); // 外部按下。
    mouse.clear(); // 到达画布前按下边沿已经过去。
    assert_eq!(begin_preview_drag(false, &mouse), None); // 电平不是新的画布手势。
    mouse.press(MouseButton::Middle); // 右键尚未松开又按中键。
    assert_eq!(begin_preview_drag(false, &mouse), None); // 和弦不能抢成模型平移。
}

#[test] // 图片的视觉矩阵在高DPI下使用正确逻辑像素且不改Node布局。
fn image_pan_and_zoom_respect_dpi_without_changing_layout_size() {
    let mut test = fixture(true); // 初始图片布局300×200。
    test.app
        .world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view
        .zoom = 2.0; // 放大后允许查看边缘。
    test.app
        .world_mut()
        .get_mut::<ComputedNode>(test.canvas)
        .unwrap()
        .inverse_scale_factor = 0.5; // 模拟二倍UI/DPI缩放。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left); // 图片内部左键平移。
    step(&mut test.app); // 冻结手势。
    test.app.world_mut().write_message(MouseMotion {
        delta: Vec2::new(30.0, 0.0),
    }); // 移动30物理像素。
    step(&mut test.app); // 应用视觉矩阵。
    let transform = test.app.world().get::<UiTransform>(test.image).unwrap(); // 检查真正图片显示参数。
    assert_eq!(transform.translation, Val2::px(15.0, 0.0)); // 只转换一次DPI，不能漂移或加倍。
    assert_eq!(transform.scale, Vec2::splat(2.0)); // 两轴等比例。
    assert_eq!(
        test.app
            .world()
            .get::<ComputedNode>(test.image)
            .unwrap()
            .size(),
        Vec2::new(300.0, 200.0)
    ); // 缩放不参与Flex重排。
}

#[test] // 窗口/全屏切换保留观察参数，按钮显式恢复默认值。
fn resize_preserves_view_and_reset_restores_complete_framing() {
    let mut test = fixture(false); // 使用真实相机应用系统。
    let view = AssetPreviewView {
        zoom: 2.0,
        yaw: 2.2,
        pitch: -0.4,
        model_pan: Vec3::new(0.2, 0.3, 0.1),
        ..default()
    }; // 用户已有自定义视角。
    test.app
        .world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view = view; // 设置临时导航，不动作者数据。
    step(&mut test.app); // 先采用窗口模式相机。
    let rotation = test
        .app
        .world()
        .get::<Transform>(test.camera)
        .unwrap()
        .rotation; // 保存当前朝向。
    test.app
        .world_mut()
        .get_mut::<ComputedNode>(test.canvas)
        .unwrap()
        .size = Vec2::new(300.0, 900.0); // 切换为全屏高画布。
    step(&mut test.app); // Changed布局触发一次适配。
    assert_eq!(
        test.app.world().resource::<AssetPreviewNavigation>().view,
        view
    ); // 导航参数未被覆盖。
    assert!(
        test.app
            .world()
            .get::<Transform>(test.camera)
            .unwrap()
            .rotation
            .abs_diff_eq(rotation, 1e-5)
    ); // 观察方向没有跳回默认。
    test.app.world_mut().spawn((
        AssetPreviewAction::Reset,    // 显式点击重置动作。
        AssetPreviewNavigationButton, // 与生产缩放工具使用同一查询marker。
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,         // 本帧真实按下边沿。
    ));
    step(&mut test.app); // 同一帧恢复相机。
    assert_eq!(
        test.app.world().resource::<AssetPreviewNavigation>().view,
        AssetPreviewView::default()
    ); // 倍率、旋转和平移全部复位。
}

#[test]
fn equipment_navigation_only_consumes_wheel_on_visible_scene_sidebar() {
    let mut test = fixture(false);
    test.app
        .world_mut()
        .resource_mut::<AssetPreviewState>()
        .external_preview = Some(crate::game_authoring::ExternalPreviewIdentity {domain:"test.author",tab:1,revision:1,ticket:0});
    test.app
        .world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Scene;
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    let zoom = test
        .app
        .world()
        .resource::<AssetPreviewNavigation>()
        .view
        .zoom;
    assert_ne!(zoom, 1.0);
    test.app
        .world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    assert_eq!(
        test.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .zoom,
        zoom
    );
    test.app
        .world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Scene;
    test.app
        .insert_resource(crate::resources::SceneEditorLayoutState {
            inspector_visible: false,
            ..default()
        });
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    assert_eq!(
        test.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .zoom,
        zoom
    );
    assert!(
        !test
            .app
            .world()
            .resource::<AssetPreviewNavigation>()
            .wheel_captured
    );
}

#[test] // 失焦、加载、隐藏页和模态都不允许旧手势继续旋转或缩放。
fn focus_loss_and_hidden_or_loading_content_disable_input() {
    let mut test = fixture(false); // 完整模型已Ready。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right); // 开始捕获。
    step(&mut test.app); // 确认拖动所有权。
    test.app
        .world_mut()
        .get_mut::<Window>(test.window)
        .unwrap()
        .focused = false; // 操作系统焦点丢失。
    wheel(&mut test, MouseScrollUnit::Line, 1.0); // 失焦事件不能修改预览。
    assert!(
        !test
            .app
            .world()
            .resource::<AssetPreviewNavigation>()
            .dragging()
    ); // 捕获立即取消。
    assert_eq!(
        test.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .zoom,
        1.0
    ); // 未发生幽灵缩放。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right); // 清理原物理键。
    step(&mut test.app); // 消费release。
    test.app
        .world_mut()
        .get_mut::<Window>(test.window)
        .unwrap()
        .focused = true; // 恢复窗口焦点。
    test.app
        .world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Scene; // 当前不展示资产页。
    wheel(&mut test, MouseScrollUnit::Line, 1.0); // 隐藏画布不能接收输入。
    assert_eq!(
        test.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .zoom,
        1.0
    ); // 未修改倍率。
    test.app
        .world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets; // 回到资产页。
    if let AssetPreviewContent::Model { phase, .. } = &mut test
        .app
        .world_mut()
        .resource_mut::<AssetPreviewState>()
        .content
    {
        *phase = AssetPreviewPhase::Loading; // 模拟同路径热更尚未完成。
    } // 加载阶段不能假装有完整可操作模型。
    wheel(&mut test, MouseScrollUnit::Line, 1.0); // 加载期滚轮仍不缩放。
    assert_eq!(
        test.app
            .world()
            .resource::<AssetPreviewNavigation>()
            .view
            .zoom,
        1.0
    ); // 原视角等待完整模型恢复。
}

#[test]
fn scene_object_list_scrolls_before_outer_panel_and_hands_off_at_the_end() {
    let mut test = fixture(true);
    test.app.world_mut().get_mut::<RelativeCursorPosition>(test.canvas).unwrap().cursor_over = false;
    let list = test.app.world_mut().spawn((
        crate::ui::list_layout::SceneObjectListScroll,
        RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::ZERO) },
        ComputedNode { size: Vec2::new(300.0, 213.0), content_size: Vec2::new(300.0, 267.0),
            inverse_scale_factor: 1.0, ..default() },
        ScrollPosition::default(),
    )).id();
    wheel(&mut test, MouseScrollUnit::Pixel, -30.0);
    assert_eq!(test.app.world().get::<ScrollPosition>(list).unwrap().y, 30.0);
    assert_eq!(test.app.world().get::<ScrollPosition>(test.panel).unwrap().y, 100.0);
    wheel(&mut test, MouseScrollUnit::Pixel, -30.0);
    assert_eq!(test.app.world().get::<ScrollPosition>(list).unwrap().y, 54.0);
    assert_eq!(test.app.world().get::<ScrollPosition>(test.panel).unwrap().y, 100.0);
    wheel(&mut test, MouseScrollUnit::Pixel, -30.0);
    assert_eq!(test.app.world().get::<ScrollPosition>(list).unwrap().y, 54.0);
    assert_eq!(test.app.world().get::<ScrollPosition>(test.panel).unwrap().y, 130.0);
}

#[test]
fn native_tool_preview_navigation_follows_its_right_tool_page() {
    use crate::game_authoring::{DomainToolPreviewHost, ExternalAssetPreviewRequest, ExternalPreviewIdentity};
    use crate::ui::sidebars::EditorLeftSidebarTab;
    let mut test = fixture(false);
    let host = test.app.world_mut().spawn(DomainToolPreviewHost("test.native")).id();
    test.app.insert_resource(ExternalAssetPreviewRequest { active:true, host:Some(host), ..default() });
    test.app.world_mut().resource_mut::<AssetPreviewState>().external_preview =
        Some(ExternalPreviewIdentity { domain:"test.native", tab:1, revision:1, ticket:0 });
    test.app.world_mut().resource_mut::<SceneEditorState>().selected_asset = None;
    {
        let mut sidebar = test.app.world_mut().resource_mut::<SceneEditorSidebarState>();
        sidebar.left = EditorLeftSidebarTab::Domain("test.native");
        sidebar.right = EditorRightSidebarTab::Tools;
    }
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    let zoom = test.app.world().resource::<AssetPreviewNavigation>().view.zoom;
    assert!(zoom > 1.0, "右侧工具页显示时，唯一预览应接收导航");
    assert_eq!(test.app.world().get::<ScrollPosition>(test.panel).unwrap().y, 100.0);
    // 切属性或另一插件后，即使旧画布仍报告悬停，也必须立即归还滚轮。
    test.app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Scene;
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    assert_eq!(test.app.world().resource::<AssetPreviewNavigation>().view.zoom, zoom);
    assert_eq!(test.app.world().get::<ScrollPosition>(test.panel).unwrap().y, 73.0);
    {
        let mut sidebar = test.app.world_mut().resource_mut::<SceneEditorSidebarState>();
        sidebar.left = EditorLeftSidebarTab::Domain("test.other");
        sidebar.right = EditorRightSidebarTab::Tools;
    }
    wheel(&mut test, MouseScrollUnit::Line, 1.0);
    assert_eq!(test.app.world().resource::<AssetPreviewNavigation>().view.zoom, zoom);
    assert_eq!(test.app.world().resource::<SceneEditorViewportState>().extent, 100.0);
}
