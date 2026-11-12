//! 无工程退出的真实工作台GPU回归；只驱动原宿主入口和共享确认层，不创建作者或用户工程。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{Host, finish_app_exit, handle, poll_empty_exit, publish, request_app_exit};
use crate::{
    native_project_close::{CloseTarget, NativeProjectClosePlugin, NativeProjectCloseState},
    project_log::gpu_tests::{assert_dialog_chrome, frame, screenshot},
    user_preferences::Preferences,
    workspace_ui::{Page, WorkspaceCommand, WorkspaceModel, WorkspaceProbe, WorkspaceUiPlugin},
};
use bevy::{
    camera::RenderTarget,
    ecs::message::MessageCursor,
    input::{ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput},
    prelude::*,
    render::render_resource::{Extent3d, TextureFormat, TextureUsages},
    text::TextLayoutInfo,
    window::{CursorLeft, CursorMoved, PrimaryWindow, WindowCloseRequested, WindowEvent, WindowRef},
};
use feathrise_editor_scene::workbench::{WorkbenchCentralPageHost, WorkbenchHeader, WorkbenchModel, WorkbenchUiPlugin};
use feathrise_editor_ui::EditorButtonClick;
use std::{collections::BTreeMap, fs::OpenOptions, path::{Path, PathBuf}};

#[derive(Resource, Clone)]
struct PointerFixture {
    window: Entity,
    camera: Entity,
    image: Handle<Image>,
    position: Option<Vec2>,
}

#[derive(Resource, Default)]
struct LifecycleAudit {
    window_requests: usize,
    quit_commands: usize,
    exit_messages: usize,
    successful_exits: usize,
}

/// 离屏目标在First拾取输入前提供同尺寸ECS Window及正式窗口消息，捕获后恢复GPU目标。
fn provide_pointer(world: &mut World) {
    let fixture = world.resource::<PointerFixture>().clone();
    let scale = {
        let mut window = world.get_mut::<Window>(fixture.window).unwrap();
        window.set_physical_cursor_position(fixture.position.map(|value| value.as_dvec2()));
        window.scale_factor()
    };
    // 现代拾取只消费聚合WindowEvent；CursorLeft不清位置，先移到窗口外才能清除旧悬停。
    let moved = CursorMoved { window: fixture.window,
        position: fixture.position.map_or(Vec2::splat(-1.), |value| value / scale), delta: None };
    world.write_message(moved.clone());
    world.write_message(WindowEvent::CursorMoved(moved));
    if fixture.position.is_none() {
        let left = CursorLeft { window: fixture.window };
        world.write_message(left.clone());
        world.write_message(WindowEvent::CursorLeft(left));
    }
    world.entity_mut(fixture.camera).insert(RenderTarget::Window(WindowRef::Entity(fixture.window)));
}

fn restore_target(world: &mut World) {
    let fixture = world.resource::<PointerFixture>().clone();
    world.entity_mut(fixture.camera).insert(RenderTarget::from(fixture.image));
}

fn use_sdk_font(mut fonts: Query<&mut TextFont>) {
    for mut font in &mut fonts {
        if font.font != Handle::<Font>::default().into() { font.font = Handle::<Font>::default().into(); }
    }
}

/// 不伪造HWND或作者进程：仅把真实消息交给与主宿主相同的三个函数。
/// 所有确认选择和AppExit均由生产handle/authorize/finish决定，夹具不写退出许可。
fn drive_empty_host(
    world: &mut World,
    mut closes: Local<MessageCursor<WindowCloseRequested>>,
    mut commands: Local<MessageCursor<WorkspaceCommand>>,
) {
    let window = world.resource::<PointerFixture>().window;
    let close_count = closes.read(world.resource::<Messages<WindowCloseRequested>>())
        .filter(|request| request.window == window).count();
    let actions = commands.read(world.resource::<Messages<WorkspaceCommand>>()).cloned().collect::<Vec<_>>();
    let mut host = world.remove_non_send::<Host>().unwrap();
    assert!(host.current.is_none(), "本用例必须一直保持无工程宿主");
    for _ in 0..close_count { request_app_exit(&mut host, world); }
    for command in actions {
        assert!(matches!(command, WorkspaceCommand::QuitApp),
            "确认层不能向后台泄露新建、打开或保存命令：{command:?}");
        world.resource_mut::<LifecycleAudit>().quit_commands += 1;
        handle(&mut host, world, 0, None, command);
    }
    world.resource_mut::<LifecycleAudit>().window_requests += close_count;
    poll_empty_exit(&mut host, world);
    if host.changed { publish(&mut host, world); }
    finish_app_exit(&mut host, world);
    world.insert_non_send(host);
}

fn observe_exit(
    exits: Res<Messages<AppExit>>,
    mut cursor: Local<MessageCursor<AppExit>>,
    mut audit: ResMut<LifecycleAudit>,
) {
    for exit in cursor.read(&exits) {
        audit.exit_messages += 1;
        audit.successful_exits += usize::from(exit.is_success());
    }
}

fn settle(app: &mut App) { for _ in 0..20 { frame(app); } }

fn bounds(app: &App, entity: Entity) -> Rect {
    Rect::from_center_size(app.world().get::<UiGlobalTransform>(entity).unwrap().translation,
        app.world().get::<ComputedNode>(entity).unwrap().size())
}

fn belongs_to(world: &World, mut entity: Entity, root: Entity) -> bool {
    for _ in 0..64 {
        if entity == root { return true; }
        let Some(parent) = world.get::<ChildOf>(entity) else { return false; };
        entity = parent.parent();
    }
    false
}

fn texts(app: &mut App, value: &str) -> Vec<Entity> {
    let world = app.world_mut();
    world.query::<(Entity, &Text, &ComputedNode, &InheritedVisibility)>().iter(world)
        .filter_map(|(entity, text, node, visible)|
            (text.0 == value && !node.is_empty() && visible.get()).then_some(entity)).collect()
}

fn visible_button(app: &mut App, value: &str, root: Option<Entity>) -> Entity {
    let candidates = texts(app, value);
    let found = candidates.into_iter().filter_map(|entity| {
        let parent = app.world().get::<ChildOf>(entity)?.parent();
        (app.world().get::<InteractionButton>(parent).is_some()
            && root.is_none_or(|root| belongs_to(app.world(), parent, root)))
            .then_some(parent)
    }).collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "应唯一找到真实可见按钮：{value}，{found:?}");
    found[0]
}

fn mouse_frame(app: &mut App, state: ButtonState) {
    let window = app.world().resource::<PointerFixture>().window;
    let input = MouseButtonInput { button: MouseButton::Left, state, window };
    // 对齐真实窗口后端：InputPlugin读取独立消息，现代拾取读取同值聚合消息。
    app.world_mut().write_message(input);
    app.world_mut().write_message(WindowEvent::MouseButtonInput(input));
    frame(app);
}

fn press_button(app: &mut App, button: Entity) {
    let position = bounds(app, button).center();
    app.world_mut().resource_mut::<PointerFixture>().position = Some(position);
    for _ in 0..3 { frame(app); }
    assert_eq!(*app.world().get::<UiInteraction>(button).unwrap(), UiInteraction::Hovered);
    mouse_frame(app, ButtonState::Pressed);
    assert_eq!(*app.world().get::<UiInteraction>(button).unwrap(), UiInteraction::Pressed);
    assert!(!app.world().get::<EditorButtonClick>(button).unwrap().is_clicked());
}

fn release_button(app: &mut App, button: Entity) {
    mouse_frame(app, ButtonState::Released);
    assert_eq!(*app.world().get::<UiInteraction>(button).unwrap(), UiInteraction::Hovered);
    assert!(app.world().get::<EditorButtonClick>(button).unwrap().is_clicked());
    app.world_mut().resource_mut::<PointerFixture>().position = None;
}

fn click_button(app: &mut App, button: Entity) {
    press_button(app, button);
    release_button(app, button);
}

fn keyboard_cancel(app: &mut App, key_code: KeyCode, logical_key: Key) {
    let window = app.world().resource::<PointerFixture>().window;
    app.world_mut().write_message(KeyboardInput { key_code, logical_key: logical_key.clone(),
        state: ButtonState::Pressed, text: None, repeat: false, window });
    frame(app);
    assert!(!app.world().resource::<NativeProjectCloseState>().active());
    app.world_mut().write_message(KeyboardInput { key_code, logical_key,
        state: ButtonState::Released, text: None, repeat: false, window });
    settle(app);
}

fn window_close(app: &mut App) {
    let window = app.world().resource::<PointerFixture>().window;
    app.world_mut().write_message(WindowCloseRequested { window });
    frame(app);
}

fn assert_no_exit(app: &App, page: Page) {
    let host = app.world().non_send::<Host>();
    assert!(!host.exit_app);
    assert!(host.current.is_none() && host.candidate.is_none() && host.retiring.is_none());
    assert!(host.job.is_none() && host.preferences_job.is_none() && host.picker.is_none());
    assert_eq!(app.world().resource::<LifecycleAudit>().exit_messages, 0);
    let probe = app.world().resource::<WorkspaceProbe>();
    assert!(probe.project.is_none());
    assert_eq!(probe.page, page);
    assert!(!app.world().resource::<WorkspaceModel>().busy,
        "无原后台任务时确认与取消不能把工作台留在busy，取消后新建/打开入口仍可操作");
    assert!(app.world().resource::<WorkbenchModel>().asset_root.is_none());
    assert!(app.world().resource::<Assets<bevy::gltf::Gltf>>().is_empty());
    assert!(app.world().resource::<Assets<bevy::gltf::GltfMesh>>().is_empty());
}

/// 原工作台根没有硬编码相机；离屏夹具必须登记官方默认UI相机，不能依赖仅适用于Window的回退。
fn assert_workbench_target(app: &mut App, camera: Entity) -> serde_json::Value {
    let world = app.world_mut();
    let header = world.query_filtered::<Entity, With<WorkbenchHeader>>().single(world).unwrap();
    let center = world.query_filtered::<Entity, With<WorkbenchCentralPageHost>>().single(world).unwrap();
    let mut root = header;
    let mut cursor = header;
    for _ in 0..64 {
        let Some(parent) = app.world().get::<ChildOf>(cursor) else { break; };
        cursor = parent.parent();
        if app.world().get::<Node>(cursor).is_some() { root = cursor; }
    }
    let mut observed = Vec::new();
    for (name, entity) in [("root", root), ("header", header), ("central_page", center)] {
        assert_eq!(app.world().get::<bevy::ui::ComputedUiTargetCamera>(entity).unwrap().get(), Some(camera),
            "原工作台{name}必须由官方UI传播绑定到本次实际离屏相机");
        assert_ne!(app.world().get::<Node>(entity).unwrap().display, Display::None);
        assert!(app.world().get::<InheritedVisibility>(entity).unwrap().get());
        let rect = bounds(app, entity);
        assert!(rect.width() > 0. && rect.height() > 0., "原工作台{name}必须具有真实布局");
        observed.push(serde_json::json!({"node":name,"rect":[rect.min.x,rect.min.y,rect.max.x,rect.max.y]}));
    }
    assert!(app.world().get::<bevy::ui::IsDefaultUiCamera>(camera).is_some());
    assert_eq!(app.world().get::<Camera>(camera).unwrap().physical_target_size(), Some(UVec2::new(1280, 720)));
    serde_json::json!({"official_default_image_camera":true,"nodes":observed})
}

fn dialog_nodes(app: &mut App) -> (Entity, Entity, Entity, Entity) {
    let statuses = texts(app, "确定退出编辑器？");
    assert_eq!(statuses.len(), 1);
    let body = app.world().get::<ChildOf>(statuses[0]).unwrap().parent();
    let panel = app.world().get::<ChildOf>(body).unwrap().parent();
    let world = app.world_mut();
    let nodes = world.query::<(Entity, &ChildOf, &Node)>().iter(world)
        .filter(|(_, parent, _)| parent.parent() == panel)
        .map(|(entity, _, node)| (entity, node.min_height)).collect::<Vec<_>>();
    let header = nodes.iter().find(|(_, height)| *height == feathrise_editor_ui::dialog_header_node().min_height).unwrap().0;
    let footer = nodes.iter().find(|(_, height)| *height == feathrise_editor_ui::dialog_footer_node().min_height).unwrap().0;
    (panel, header, body, footer)
}

fn capture_dialog(app: &mut App, image: &Handle<Image>, output: &Path, width: usize) -> serde_json::Value {
    settle(app);
    assert_no_exit(app, app.world().resource::<WorkspaceProbe>().page);
    let state = app.world().resource::<NativeProjectCloseState>();
    assert!(state.active());
    assert_eq!(state.target(), CloseTarget::Editor);
    let (panel, header, body, footer) = dialog_nodes(app);
    let rect = bounds(app, panel);
    assert!(rect.width() <= 360.1 && rect.width() <= width as f32 * 0.94 + 1.1);
    assert!(rect.height() <= 190., "无工程确认层同样应收紧空白：{rect:?}");
    assert_eq!(app.world().get::<Node>(body).unwrap().padding, feathrise_editor_ui::dialog_body_node().padding);
    let exit = visible_button(app, "退出编辑器", Some(footer));
    let cancel = visible_button(app, "取消 (Esc)", Some(footer));
    let close = visible_button(app, "×", Some(header));
    assert_eq!(app.world().resource::<bevy::input_focus::InputFocus>().get(), Some(cancel));
    let visible = {
        let world = app.world_mut();
        world.query_filtered::<(Entity, &ComputedNode, &InheritedVisibility), With<InteractionButton>>().iter(world)
            .filter_map(|(entity, node, visible)| (!node.is_empty() && visible.get()).then_some(entity)).collect::<Vec<_>>()
    };
    assert_eq!(visible.iter().filter(|entity| belongs_to(app.world(), **entity, footer)).count(), 2,
        "无工程退出只显示退出与取消，隐藏的保存/放弃实体不能计入");
    for (button, primary) in [(exit, true), (cancel, false)] {
        let style = feathrise_editor_ui::dialog_button_style(primary);
        let node = app.world().get::<Node>(button).unwrap();
        assert_eq!(node.height, style.node.height);
        assert_eq!(node.border, style.node.border);
        assert_eq!(node.border_radius, style.node.border_radius);
        assert_eq!(app.world().get::<feathrise_editor_ui::ButtonColors>(button).unwrap().normal, style.colors.normal);
    }
    assert_eq!(app.world().get::<Node>(close).unwrap().width,
        feathrise_editor_ui::dialog_close_button_style().node.width);
    let pixels = screenshot(app, image, output);
    let chrome = assert_dialog_chrome(app, panel, header, footer, width, &pixels);
    let mut words = Vec::new();
    for (value, owner) in [("退出编辑器", header), ("确定退出编辑器？", body),
        ("退出编辑器", exit), ("取消 (Esc)", cancel)] {
        let actual = texts(app, value).into_iter().find(|entity| belongs_to(app.world(), *entity, owner)).unwrap();
        let font_size = if owner == header || owner == body {
            feathrise_editor_ui::metrics::FONT_BODY
        } else { feathrise_editor_ui::metrics::FONT_CONTROL };
        assert_eq!(app.world().get::<TextFont>(actual).unwrap().font_size, font_size.into());
        let layout = app.world().get::<TextLayoutInfo>(actual).unwrap();
        assert!(layout.glyphs.len() >= value.chars().filter(|value| !value.is_whitespace()).count());
        let text_rect = bounds(app, actual);
        let owner_rect = bounds(app, owner);
        assert!(text_rect.min.x >= owner_rect.min.x - 1.1 && text_rect.max.x <= owner_rect.max.x + 1.1);
        assert!(text_rect.min.y >= owner_rect.min.y - 1.1 && text_rect.max.y <= owner_rect.max.y + 1.1);
        let mut ink = 0;
        let height = pixels.len() / (width * 4);
        for y in text_rect.min.y.ceil().max(0.) as usize..(text_rect.max.y.floor().max(0.) as usize).min(height) {
            for x in text_rect.min.x.ceil().max(0.) as usize..(text_rect.max.x.floor().max(0.) as usize).min(width) {
                let at = (y * width + x) * 4;
                if (0..3).all(|channel| pixels[at + channel] > 135) { ink += 1; }
            }
        }
        assert!(ink >= value.chars().count() * 3, "本次GPU帧必须真的绘制中文：{value}");
        words.push(serde_json::json!({"text":value,"glyphs":layout.glyphs.len(),"font_size":font_size,"ink_pixels":ink}));
    }
    serde_json::json!({"chrome":chrome,"rendered_words":words,"visible_footer_buttons":2})
}

fn resize(app: &mut App, image: &Handle<Image>, width: u32) {
    app.world_mut().resource_mut::<Assets<Image>>().get_mut(image).unwrap()
        .resize(Extent3d { width, height: 720, depth_or_array_layers: 1 });
    let window = app.world().resource::<PointerFixture>().window;
    app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(width as f32, 720.);
}

#[test]
#[ignore = "显式无工程GPU验收，FEATHRISE_EMPTY_EXIT_UI_GPU_REPORT必须是本轮新绝对目录"]
fn empty_workspace_exit_requires_a_real_choice_and_preserves_the_original_form() {
    let output = PathBuf::from(std::env::var_os("FEATHRISE_EMPTY_EXIT_UI_GPU_REPORT").expect("指定本轮独占目录"));
    assert!(output.is_absolute());
    std::fs::create_dir(&output).expect("禁止覆盖旧验收目录");
    let owner = serde_json::json!({"fixture":"empty-workspace-exit-ui-v3","pid":std::process::id()});
    serde_json::to_writer_pretty(OpenOptions::new().write(true).create_new(true)
        .open(output.join("owner.json")).unwrap(), &owner).unwrap();
    let preferences = Preferences::default_for_home(&output.join("隔离用户目录")).unwrap();
    let configuration = preferences.file().to_path_buf();
    let model = crate::workspace::model(&preferences, None);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin { file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
        watch_for_changes_override: Some(false), ..default() }, bevy::scene::ScenePlugin));
    app.add_plugins((WindowPlugin { primary_window: None, close_when_requested: false,
        exit_condition: bevy::window::ExitCondition::DontExit, ..default() },
        bevy::render::RenderPlugin::default(), bevy::image::ImagePlugin::default(), bevy::mesh::MeshPlugin,
        bevy::camera::CameraPlugin, bevy::core_pipeline::CorePipelinePlugin, bevy::sprite::SpritePlugin,
        bevy::sprite_render::SpriteRenderPlugin));
    app.add_plugins((bevy::input::InputPlugin, bevy::input_focus::InputFocusPlugin,
        bevy::input_focus::InputDispatchPlugin, bevy::input_focus::tab_navigation::TabNavigationPlugin,
        bevy::picking::input::PointerInputPlugin, bevy::picking::PickingPlugin, bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin, bevy::text::TextPlugin, feathrise_runtime::DefaultFontPlugin,
        bevy::ui::UiPlugin, bevy::ui_render::UiRenderPlugin));
    // 生产DefaultPlugins自带官方字段/滚动控件和GLTF资产注册。原工作台的预设标签表现
    // 无条件读取Gltf/GltfMesh元数据表；本夹具只补空注册，不安装模型加载器、PBR或实际预览。
    app.add_plugins(bevy::ui_widgets::UiWidgetsPlugins)
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<bevy::gltf::GltfMesh>();
    app.add_plugins((crate::branding::EditorBrandingPlugin, feathrise_editor_ui::EditorUiPlugin,
        feathrise_editor_documents::DocumentsEditorPlugin,
        feathrise_editor_weights::WeightPanelPlugin, feathrise_editor_project_browser::ProjectBrowserPlugin, WorkbenchUiPlugin))
        .insert_resource(model).add_plugins((WorkspaceUiPlugin, NativeProjectClosePlugin))
        .init_resource::<LifecycleAudit>()
        .add_systems(Update, use_sdk_font)
        .add_systems(First, provide_pointer.before(bevy::picking::PickingSystems::Input))
        .add_systems(PreUpdate, restore_target.after(feathrise_editor_ui::UiInteractionSystems::Sync).after(feathrise_editor_ui::update_editor_button_clicks))
        .add_systems(Last, drive_empty_host)
        .add_systems(Last, observe_exit.after(drive_empty_host));
    assert!(!app.is_plugin_added::<feathrise_editor_scene::SceneEditorPlugin>());
    assert!(!app.is_plugin_added::<crate::native_play::NativePlayPlugin>());
    let mut texture = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
    texture.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(texture);
    // DefaultUiCamera只会自动回退到主Window相机；Image目标必须显式登记同一个官方默认标记。
    let camera = app.world_mut().spawn((Camera2d, bevy::ui::IsDefaultUiCamera,
        RenderTarget::from(image.clone()))).id();
    let window = app.world_mut().spawn((Window { resolution: (1280, 720).into(), ..default() }, PrimaryWindow)).id();
    app.insert_resource(PointerFixture { window, camera, image: image.clone(), position: None });
    app.insert_non_send(Host {
        target_override: None, preferences, preferences_job: None, job: None, prepared: None, picker: None,
        current: None, candidate: None, retiring: None, want_close: false, close_sent: false, exit_app: false,
        notice: "未打开工程".into(), changed: true, initial: None,
    });
    app.finish(); app.cleanup();
    settle(&mut app);
    assert_no_exit(&app, Page::Start);
    assert!(!app.world().resource::<NativeProjectCloseState>().active());
    let workbench_target = assert_workbench_target(&mut app, camera);
    screenshot(&mut app, &image, &output.join("empty-start.png"));

    window_close(&mut app);
    settle(&mut app);
    for _ in 0..30 { frame(&mut app); assert_no_exit(&app, Page::Start); }
    let wide = capture_dialog(&mut app, &image, &output.join("empty-exit-wide.png"), 1280);
    let (_, _, _, footer) = dialog_nodes(&mut app);
    let cancel = visible_button(&mut app, "取消 (Esc)", Some(footer));
    let original_click = *app.world().get::<EditorButtonClick>(cancel).unwrap();
    window_close(&mut app);
    settle(&mut app);
    assert_eq!(*app.world().get::<EditorButtonClick>(cancel).unwrap(), original_click,
        "重复主窗口X不能重建确认层或作废原按钮身份");
    click_button(&mut app, cancel);
    settle(&mut app);
    assert_no_exit(&app, Page::Start);
    assert!(!app.world().resource::<NativeProjectCloseState>().active());

    // QuitApp仍经过原宿主handle；Esc只取消本次退出，不关闭主工作台。
    app.world_mut().write_message(WorkspaceCommand::QuitApp);
    frame(&mut app);
    settle(&mut app);
    keyboard_cancel(&mut app, KeyCode::Escape, Key::Escape);
    assert_no_exit(&app, Page::Start);

    resize(&mut app, &image, 280);
    window_close(&mut app);
    let narrow = capture_dialog(&mut app, &image, &output.join("empty-exit-narrow.png"), 280);
    let (_, header, _, _) = dialog_nodes(&mut app);
    let close = visible_button(&mut app, "×", Some(header));
    click_button(&mut app, close);
    settle(&mut app);
    assert_no_exit(&app, Page::Start);

    // 背景使用真实新建页和生产回填入口；旧动作脉冲及Enter不能穿透确认层提交表单。
    resize(&mut app, &image, 1280);
    settle(&mut app);
    let new = visible_button(&mut app, "新建工程…", None);
    assert!(app.world().get::<bevy::ui::InteractionDisabled>(new).is_none(),
        "取消退出后原新建入口必须实际恢复可用");
    click_button(&mut app, new);
    settle(&mut app);
    assert_no_exit(&app, Page::New);
    crate::workspace_ui::fill_path(app.world_mut(), crate::workspace_ui::WorkspaceField::NewParent,
        output.join("保持原表单目录").to_string_lossy().into_owned());
    settle(&mut app);
    let fields = app.world().resource::<WorkspaceProbe>().fields.iter()
        .map(|field| (field.field.clone(), field.value.clone())).collect::<BTreeMap<_, _>>();
    assert!(fields.iter().any(|(field, value)| field == "Parent" && value.contains("保持原表单目录")));
    let create = visible_button(&mut app, "创建并打开", None);
    window_close(&mut app);
    settle(&mut app);
    app.world_mut().entity_mut(create).insert(EditorButtonClick::triggered());
    frame(&mut app);
    app.world_mut().entity_mut(create).insert(EditorButtonClick::default());
    assert!(app.world().resource::<NativeProjectCloseState>().active());
    assert_no_exit(&app, Page::New);
    keyboard_cancel(&mut app, KeyCode::Enter, Key::Enter);
    assert_no_exit(&app, Page::New);
    let retained = app.world().resource::<WorkspaceProbe>().fields.iter()
        .map(|field| (field.field.clone(), field.value.clone())).collect::<BTreeMap<_, _>>();
    assert_eq!(retained, fields);
    assert!(!output.join("保持原表单目录").exists());
    screenshot(&mut app, &image, &output.join("cancelled-form.png"));

    // 新一次明确鼠标释放才发布真实AppExit；重复X发生在本次按住期间也不能重置它。
    app.world_mut().write_message(WorkspaceCommand::QuitApp);
    frame(&mut app);
    settle(&mut app);
    let (_, _, _, footer) = dialog_nodes(&mut app);
    let exit = visible_button(&mut app, "退出编辑器", Some(footer));
    press_button(&mut app, exit);
    assert_no_exit(&app, Page::New);
    window_close(&mut app);
    assert_no_exit(&app, Page::New);
    release_button(&mut app, exit);
    let audit = app.world().resource::<LifecycleAudit>();
    assert_eq!(audit.exit_messages, 1, "未选择、取消和重复请求都不能充当退出授权");
    assert_eq!(audit.successful_exits, 1);
    assert!(app.world().non_send::<Host>().exit_app);
    assert!(!configuration.exists(), "配置始终位于自有夹具，退出确认不得触发偏好写入");
    let report = serde_json::json!({"passed":true,"owner":owner,"wide":wide,"narrow":narrow,
        "workbench_camera_binding":workbench_target,
        "window_requests":audit.window_requests,"quit_commands":audit.quit_commands,"app_exit_messages":audit.exit_messages,
        "configuration":configuration,"retained_form":retained,"unselected_wait_frames":30,
        "cancel_button_escape_header_close_and_enter_preserved_host":true,"background_action_blocked":true,
        "repeat_x_preserved_active_mouse_gesture":true,"no_author_or_project_started":true,
        "scope":"原无工程Workbench/Workspace布局与离屏GPU；真实Bevy鼠标/键盘消息和生产宿主函数，未验证OS实体键鼠或真实窗口销毁"});
    serde_json::to_writer_pretty(OpenOptions::new().write(true).create_new(true)
        .open(output.join("evidence.json")).unwrap(), &report).unwrap();
    println!("FEATHRISE_EMPTY_EXIT_UI_GPU_REPORT {}", output.display());
}
