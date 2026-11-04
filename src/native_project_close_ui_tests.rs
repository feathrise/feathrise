//! 工程关闭与编辑器退出确认层的生产UI离屏验收：真实中文、布局和原取消事务，不操作用户工程。

use super::{components::*, resources::*, systems::authorize_exit, ui, NativeProjectClosePlugin};
use bevy::{
    camera::RenderTarget,
    ecs::system::RunSystemOnce,
    input::{ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput},
    prelude::*,
    render::render_resource::{Extent3d, TextureFormat, TextureUsages},
    text::TextLayoutInfo,
    window::{PrimaryWindow, WindowRef},
};
use crate::project_log::gpu_tests::{assert_dialog_chrome, frame, screenshot};
use feathrise_editor_map::{standalone::SavedPlayFeedback, workbench::WorkbenchHeader};
use std::{fs::OpenOptions, path::PathBuf};

#[derive(Resource, Clone)]
struct CursorFixture {
    window: Entity,
    camera: Entity,
    image: Handle<Image>,
    position: Option<Vec2>,
}

/// 官方UI命中只接受Window相机；在真实InputPlugin与Focus之间提供同尺寸Window来源，
/// 原Focus和共享释放协议只运行一次，随后恢复实际Image目标；不写Interaction或捕获槽。
fn provide_native_cursor(world: &mut World) {
    let fixture = world.resource::<CursorFixture>().clone();
    world.get_mut::<Window>(fixture.window).unwrap().set_physical_cursor_position(fixture.position.map(|position| position.as_dvec2()));
    world.entity_mut(fixture.camera).insert(RenderTarget::Window(WindowRef::Entity(fixture.window)));
}

fn restore_image_target(world: &mut World) {
    let fixture = world.resource::<CursorFixture>().clone();
    world.entity_mut(fixture.camera).insert(RenderTarget::from(fixture.image));
}

/// 正式独立编辑器通过资源绑定选择SDK默认字体；精简夹具直接复用同一内嵌句柄。
fn use_builtin_editor_font(mut fonts: Query<&mut TextFont>) {
    for mut font in &mut fonts {
        if font.font != Handle::<Font>::default().into() { font.font = Handle::<Font>::default().into(); }
    }
}

fn entity<M: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, With<M>>().single(world).unwrap()
}

fn bounds(app: &App, entity: Entity) -> Rect {
    Rect::from_center_size(app.world().get::<UiGlobalTransform>(entity).unwrap().translation,
        app.world().get::<ComputedNode>(entity).unwrap().size())
}

fn actions(app: &mut App) -> Vec<(Entity, CloseAction)> {
    let world = app.world_mut();
    world.query::<(Entity, &CloseAction, &Node, &ComputedNode, &InheritedVisibility)>().iter(world)
        .filter(|(_, _, node, computed, visible)| node.display != Display::None && visible.get() && !computed.is_empty())
        .map(|(entity, action, ..)| (entity, *action)).collect()
}

fn action_entity(app: &mut App, action: CloseAction) -> Entity {
    let world = app.world_mut();
    world.query::<(Entity, &CloseAction)>().iter(world)
        .find_map(|(entity, value)| (*value == action).then_some(entity)).unwrap()
}

fn settle(app: &mut App) { for _ in 0..20 { frame(app); } }

fn action_label_entity(app: &mut App, action: CloseAction) -> Entity {
    let world = app.world_mut();
    world.query::<(Entity, &CloseActionLabel)>().iter(world)
        .find_map(|(entity, label)| (label.0 == action).then_some(entity)).unwrap()
}

/// 以用户可见文案为验收依据，不用生产label helper给自身实现作同源断言。
fn expected_action_label(target: CloseTarget, action: CloseAction) -> &'static str {
    match (target, action) {
        (_, CloseAction::Cancel) => "取消 (Esc)",
        (CloseTarget::Project, CloseAction::Close) => "关闭工程",
        (CloseTarget::Project, CloseAction::Save) => "保存并关闭",
        (CloseTarget::Project, CloseAction::Discard) => "不保存并关闭",
        (CloseTarget::Editor, CloseAction::Close) => "退出编辑器",
        (CloseTarget::Editor, CloseAction::Save) => "保存并退出",
        (CloseTarget::Editor, CloseAction::Discard) => "不保存并退出",
    }
}

fn assert_visible_text(app: &App, entity: Entity, parent: Entity, expected: &str) -> serde_json::Value {
    let text = app.world().get::<Text>(entity).unwrap();
    assert_eq!(text.0, expected, "必须更新真实渲染Text，不能只更新状态或动作枚举");
    let layout = app.world().get::<TextLayoutInfo>(entity).unwrap();
    let non_space = expected.chars().filter(|value| !value.is_whitespace()).count();
    assert!(layout.glyphs.len() >= non_space, "中文标题和按钮必须经过正式字体排版：{expected}");
    assert!(layout.size.x > 0. && layout.size.y > 0. && layout.size.is_finite());
    let rect = bounds(app, entity);
    let parent_rect = bounds(app, parent);
    assert!(rect.min.x >= parent_rect.min.x - 1.1 && rect.max.x <= parent_rect.max.x + 1.1
        && rect.min.y >= parent_rect.min.y - 1.1 && rect.max.y <= parent_rect.max.y + 1.1,
        "真实文案不能超出所属标题栏或按钮：{expected} {rect:?}/{parent_rect:?}");
    assert!(layout.size.x <= rect.width() + 1.1 && layout.size.y <= rect.height() + 1.1,
        "排版后的中文必须完整留在实际文字区域：{expected}");
    serde_json::json!({"text":text.0,"glyphs":layout.glyphs.len(),
        "rect":[rect.min.x,rect.min.y,rect.max.x,rect.max.y],"layout":[layout.size.x,layout.size.y]})
}

fn assert_compact(app: &mut App, expected_width: f32, clean: bool) -> serde_json::Value {
    let state = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(!state.needs_save_decision(), clean);
    let target = state.target();
    let has_documents = !state.documents.is_empty();
    let documents = state.documents.len();
    let status = state.status.clone();
    let expected_status = match (target, clean) {
        (CloseTarget::Project, true) => "确定关闭当前工程？".into(),
        (CloseTarget::Editor, true) => "确定退出编辑器？".into(),
        (CloseTarget::Project, false) => format!("有 {documents} 项未保存修改，是否保存后关闭？"),
        (CloseTarget::Editor, false) => format!("有 {documents} 项未保存修改，是否保存后退出？"),
    };
    assert_eq!(status, expected_status);
    let panel = entity::<ClosePanel>(app);
    let panel_node = app.world().get::<Node>(panel).unwrap();
    let frame = feathrise_editor_ui::dialog_panel_node();
    assert_eq!(panel_node.border, frame.border, "关闭确认层必须复用所有弹窗的细边界");
    assert_eq!(panel_node.border_radius, frame.border_radius);
    assert_eq!(panel_node.padding, frame.padding);
    let panel_rect = bounds(app, panel);
    assert!(panel_rect.width() <= (if clean { 360.1 } else { 440.1 }));
    assert!(panel_rect.width() <= expected_width * 0.94 + 1.);
    if clean { assert!(panel_rect.height() <= 190., "无修改确认层不能保留大片空白：{panel_rect:?}"); }
    let scroll = entity::<CloseScroll>(app);
    assert_eq!(app.world().get::<Node>(scroll).unwrap().display,
        if has_documents { Display::Flex } else { Display::None }, "空文档区必须完全收起，真实脏文档仍可阅读");
    let body = entity::<CloseBody>(app);
    assert_eq!(app.world().get::<Node>(body).unwrap().padding, feathrise_editor_ui::dialog_body_node().padding);
    let header_close = entity::<CloseHeaderClose>(app);
    let icon = app.world().get::<Node>(header_close).unwrap();
    let close_style = feathrise_editor_ui::dialog_close_button_style();
    assert_eq!(icon.width, close_style.node.width);
    assert_eq!(icon.height, close_style.node.height);
    assert_eq!(icon.border, close_style.node.border);
    assert_eq!(icon.border_radius, close_style.node.border_radius);
    let buttons = actions(app);
    let expected = if clean { vec![CloseAction::Close, CloseAction::Cancel] }
        else { vec![CloseAction::Save, CloseAction::Discard, CloseAction::Cancel] };
    assert_eq!(buttons.len(), expected.len(), "干净工程显示两项，真实修改或失败显示三项");
    assert!(expected.iter().all(|action| buttons.iter().any(|(_, actual)| actual == action)));
    for action in [CloseAction::Close, CloseAction::Save, CloseAction::Discard, CloseAction::Cancel] {
        let entity = action_entity(app, action);
        let shown = expected.contains(&action);
        assert_eq!(app.world().get::<Node>(entity).unwrap().display != Display::None, shown);
        assert_eq!(app.world().get::<bevy::ui::InteractionDisabled>(entity).is_none(), shown,
            "被隐藏的动作不能留有可执行的旧点击：{action:?}");
    }
    let cancel = buttons.iter().find(|(_, action)| *action == CloseAction::Cancel).unwrap().0;
    assert_eq!(app.world().resource::<bevy::input_focus::InputFocus>().get(), Some(cancel),
        "两种确认状态均默认聚焦取消，不能把Enter变成直接退出");
    let mut rows = Vec::new();
    let mut common_y: Option<f32> = None;
    let mut centers = Vec::new();
    for (button, action) in buttons {
        let rect = bounds(app, button);
        let style = feathrise_editor_ui::dialog_button_style(matches!(action, CloseAction::Close | CloseAction::Save));
        let node = app.world().get::<Node>(button).unwrap();
        let colors = app.world().get::<feathrise_editor_ui::ButtonColors>(button).unwrap();
        centers.push(rect.center().y);
        assert_eq!(node.height, style.node.height);
        assert_eq!(node.border, style.node.border);
        assert_eq!(node.border_radius, style.node.border_radius);
        assert_eq!(colors.normal, style.colors.normal);
        assert!((rect.height() - 28.).abs() < 1.1);
        assert!((64. - 1.1..=160.).contains(&rect.width()), "按钮按文字自然占宽：{action:?} {rect:?}");
        assert!(rect.min.x >= panel_rect.min.x && rect.max.x <= panel_rect.max.x + 1.
            && rect.min.y >= panel_rect.min.y && rect.max.y <= panel_rect.max.y + 1.,
            "按钮必须完整位于确认层：{action:?} {rect:?}/{panel_rect:?}");
        if expected_width >= 1280. || clean {
            if let Some(y) = common_y { assert!((y - rect.center().y).abs() < 1.1, "可用操作应处于同一紧凑横排"); }
            common_y = Some(rect.center().y);
        }
        let label = action_label_entity(app, action);
        let actual_text = assert_visible_text(app, label, button, expected_action_label(target, action));
        assert_eq!(app.world().get::<TextFont>(label).unwrap().font_size, style.font_size.into());
        rows.push(serde_json::json!({"action":format!("{action:?}"),"rect":[rect.min.x,rect.min.y,rect.max.x,rect.max.y],
            "label":actual_text}));
    }
    let row = entity::<CloseActions>(app);
    let row_node = app.world().get::<Node>(row).unwrap();
    assert_eq!(row_node.justify_content, JustifyContent::FlexEnd);
    assert_eq!(row_node.flex_wrap, FlexWrap::Wrap);
    if expected_width <= 280. && !clean {
        let min = centers.iter().copied().fold(f32::INFINITY, f32::min);
        let max = centers.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(max - min > 20., "极窄窗口应真正换行，而不是压扁或裁切按钮");
    }
    let title = entity::<CloseTitle>(app);
    let header = entity::<CloseHeader>(app);
    let actual_title = assert_visible_text(app, title, header, match target {
        CloseTarget::Project => "关闭当前工程", CloseTarget::Editor => "退出编辑器",
    });
    assert_eq!(app.world().get::<TextFont>(title).unwrap().font_size, feathrise_editor_ui::metrics::FONT_BODY.into());
    let status_text = entity::<CloseStatus>(app);
    let actual_status = assert_visible_text(app, status_text, body, &expected_status);
    serde_json::json!({"target":format!("{target:?}"),"title":actual_title,
        "panel":[panel_rect.min.x,panel_rect.min.y,panel_rect.max.x,panel_rect.max.y],"buttons":rows,
        "status":status,"status_text":actual_status})
}

/// 文字断言还必须对应本次GPU回读中的实际墨迹，避免只保存布局而正文完全未绘制。
fn assert_visible_text_pixels(app: &mut App, width: usize, pixels: &[u8]) -> serde_json::Value {
    let mut texts = vec![entity::<CloseTitle>(app), entity::<CloseStatus>(app)];
    for (_, action) in actions(app) { texts.push(action_label_entity(app, action)); }
    let height = pixels.len() / (width * 4);
    let mut evidence = Vec::new();
    for entity in texts {
        let rect = bounds(app, entity);
        let text = &app.world().get::<Text>(entity).unwrap().0;
        let mut ink = 0;
        for y in rect.min.y.ceil().max(0.) as usize..(rect.max.y.floor().max(0.) as usize).min(height) {
            for x in rect.min.x.ceil().max(0.) as usize..(rect.max.x.floor().max(0.) as usize).min(width) {
                let at = (y * width + x) * 4;
                if (0..3).all(|channel| pixels[at + channel] > 135) { ink += 1; }
            }
        }
        let minimum = text.chars().filter(|value| !value.is_whitespace()).count() * 3;
        assert!(ink >= minimum, "本次截图中真实中文不可为空白：{text}，墨迹{ink}/{minimum}");
        evidence.push(serde_json::json!({"text":text,"ink_pixels":ink}));
    }
    serde_json::json!(evidence)
}

fn resize_fixture(app: &mut App, target: &Handle<Image>, window: Entity, width: u32) {
    app.world_mut().resource_mut::<Assets<Image>>().get_mut(target).unwrap().resize(
        Extent3d { width, height: 720, depth_or_array_layers: 1 });
    app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(width as f32, 720.);
}

fn capture_confirmation(app: &mut App, target: &Handle<Image>, output: &std::path::Path, width: usize, clean: bool) -> serde_json::Value {
    settle(app);
    let layout = assert_compact(app, width as f32, clean);
    let pixels = screenshot(app, target, output);
    let chrome = assert_close_chrome(app, width, &pixels);
    let text_pixels = assert_visible_text_pixels(app, width, &pixels);
    serde_json::json!({"layout":layout,"chrome":chrome,"rendered_text":text_pixels})
}

fn assert_close_chrome(app: &mut App, width: usize, pixels: &[u8]) -> serde_json::Value {
    let panel = entity::<ClosePanel>(app);
    let header = entity::<CloseHeader>(app);
    let footer = entity::<CloseFooter>(app);
    assert_dialog_chrome(app, panel, header, footer, width, pixels)
}

fn cancel_through_button(app: &mut App, cancel: Entity) {
    // 使用共享按钮的动作队列脉冲，只选择按钮；原handle和authorize_exit决定是否取消。
    app.world_mut().entity_mut(cancel).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.world_mut().run_system_once(ui::handle).unwrap();
    app.world_mut().entity_mut(cancel).insert(feathrise_editor_ui::EditorButtonClick::default());
    assert_eq!(app.world().resource::<NativeProjectCloseState>().choice, Some(CloseAction::Cancel));
    assert!(!authorize_exit(app.world_mut(), false));
    settle(app);
    assert!(!app.world().resource::<NativeProjectCloseState>().active());
    let overlay = entity::<CloseOverlay>(app);
    assert_eq!(app.world().get::<Node>(overlay).unwrap().display, Display::None);
}

fn cancel_through_original_button(app: &mut App) {
    let cancel = actions(app).into_iter().find(|(_, action)| *action == CloseAction::Cancel).unwrap().0;
    cancel_through_button(app, cancel);
}

fn cancel_through_escape(app: &mut App, window: Entity) {
    // 真实InputPlugin先消费键盘消息，原handle再选择取消；不直接写choice或关闭阶段。
    app.world_mut().write_message(KeyboardInput { key_code: KeyCode::Escape, logical_key: Key::Escape,
        state: ButtonState::Pressed, text: None, repeat: false, window });
    frame(app);
    assert_eq!(app.world().resource::<NativeProjectCloseState>().choice, Some(CloseAction::Cancel));
    assert!(!authorize_exit(app.world_mut(), false));
    app.world_mut().write_message(KeyboardInput { key_code: KeyCode::Escape, logical_key: Key::Escape,
        state: ButtonState::Released, text: None, repeat: false, window });
    settle(app);
    assert!(!app.world().resource::<NativeProjectCloseState>().active());
    let overlay = entity::<CloseOverlay>(app);
    assert_eq!(app.world().get::<Node>(overlay).unwrap().display, Display::None);
}

fn choose_action_through_original_button(app: &mut App, action: CloseAction) {
    let button = actions(app).into_iter().find(|(_, value)| *value == action).unwrap().0;
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.world_mut().run_system_once(ui::handle).unwrap();
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::default());
    assert_eq!(app.world().resource::<NativeProjectCloseState>().choice, Some(action));
}

fn mouse_frame(app: &mut App, window: Entity, state: ButtonState) {
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state, window });
    frame(app);
}

/// 原生鼠标消息经InputPlugin、官方命中、共享按钮捕获和原handle，提供正向点击对照。
fn choose_action_through_mouse(app: &mut App, window: Entity, action: CloseAction) {
    let button = actions(app).into_iter().find(|(_, value)| *value == action).unwrap().0;
    let center = bounds(app, button).center();
    app.world_mut().resource_mut::<CursorFixture>().position = Some(center);
    settle(app);
    assert_eq!(*app.world().get::<Interaction>(button).unwrap(), Interaction::Hovered);
    mouse_frame(app, window, ButtonState::Pressed);
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    assert_eq!(*app.world().get::<Interaction>(button).unwrap(), Interaction::Pressed);
    assert!(!app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().is_clicked());
    assert_eq!(app.world().resource::<NativeProjectCloseState>().choice, None, "按下不能抢先执行动作");
    mouse_frame(app, window, ButtonState::Released);
    assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    assert_eq!(*app.world().get::<Interaction>(button).unwrap(), Interaction::Hovered);
    assert!(app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().is_clicked());
    assert_eq!(app.world().resource::<NativeProjectCloseState>().choice, Some(action));
    app.world_mut().resource_mut::<CursorFixture>().position = None;
}

fn upgrade_while_project_button_is_held(app: &mut App, window: Entity) -> serde_json::Value {
    let button = actions(app).into_iter().find(|(_, action)| *action == CloseAction::Close).unwrap().0;
    assert_eq!(app.world().resource::<NativeProjectCloseState>().target(), CloseTarget::Project);
    let label = action_label_entity(app, CloseAction::Close);
    assert_eq!(app.world().get::<Text>(label).unwrap().0, "关闭工程");
    let center = bounds(app, button).center();
    app.world_mut().resource_mut::<CursorFixture>().position = Some(center);
    settle(app);
    mouse_frame(app, window, ButtonState::Pressed);
    assert_eq!(*app.world().get::<Interaction>(button).unwrap(), Interaction::Pressed);
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    let old_click = *app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap();
    for _ in 0..2 {
        frame(app);
        assert_eq!(*app.world().get::<Interaction>(button).unwrap(), Interaction::Pressed);
        assert_eq!(app.world().resource::<NativeProjectCloseState>().choice, None);
    }

    // 相同按钮实体仍按住时升级宿主请求，文案换绑必须撤销旧按下身份，而不是只清choice。
    app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Editor);
    assert!(!authorize_exit(app.world_mut(), false));
    frame(app);
    assert_eq!(app.world().get::<Text>(label).unwrap().0, "退出编辑器");
    assert_eq!(action_entity(app, CloseAction::Close), button, "验收必须覆盖同一按钮实体换绑");
    let rebound_click = *app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap();
    assert_ne!(old_click, rebound_click, "真实目标文案换绑必须让原按钮按下身份失效");
    let center = bounds(app, button).center();
    app.world_mut().resource_mut::<CursorFixture>().position = Some(center);
    frame(app);
    assert_eq!(*app.world().get::<Interaction>(button).unwrap(), Interaction::Pressed);
    mouse_frame(app, window, ButtonState::Released);
    assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    assert_eq!(*app.world().get::<Interaction>(button).unwrap(), Interaction::Hovered,
        "松开仍真实命中新目标，不能靠移出或隐藏按钮让负向断言偶然通过");
    assert!(!app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().is_clicked());
    let state = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(state.target(), CloseTarget::Editor);
    assert_eq!(state.stage, CloseStage::Confirm);
    assert_eq!(state.choice, None);
    assert_eq!(state.intent, None);
    assert!(!authorize_exit(app.world_mut(), false), "旧Project按下的松开不能确认退出编辑器");
    app.world_mut().resource_mut::<CursorFixture>().position = None;
    serde_json::json!({"old_text":"关闭工程","new_text":"退出编辑器","same_entity":true,
        "held_frames":3,"release_inside_new_button":true,"old_gesture_rejected":true})
}

#[test]
#[ignore = "显式GPU验收，FEATHRISE_CLOSE_UI_GPU_REPORT须为本轮独占绝对目录"]
fn production_close_dialog_uses_shared_chrome_and_keeps_original_cancel_flow() {
    let output = PathBuf::from(std::env::var_os("FEATHRISE_CLOSE_UI_GPU_REPORT").expect("指定本轮独占截图目录"));
    assert!(output.is_absolute());
    std::fs::create_dir(&output).expect("验收目录必须是本轮新目录，禁止覆盖旧报告");
    let owner = serde_json::json!({"fixture":"target-and-state-aware-close-dialog-ui-v5","pid":std::process::id()});
    serde_json::to_writer_pretty(OpenOptions::new().write(true).create_new(true).open(output.join("owner.json")).unwrap(), &owner).unwrap();

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin {
        file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(), watch_for_changes_override: Some(false), ..default()
    }, bevy::scene::ScenePlugin));
    app.add_plugins((WindowPlugin { primary_window: None,
        exit_condition: bevy::window::ExitCondition::DontExit, ..default() },
        bevy::render::RenderPlugin::default(), bevy::image::ImagePlugin::default(),
        bevy::mesh::MeshPlugin, bevy::camera::CameraPlugin, bevy::core_pipeline::CorePipelinePlugin,
        bevy::sprite::SpritePlugin, bevy::sprite_render::SpriteRenderPlugin));
    app.add_plugins((bevy::input::InputPlugin, bevy::input_focus::InputFocusPlugin,
        bevy::input_focus::InputDispatchPlugin, bevy::picking::input::PointerInputPlugin,
        bevy::picking::PickingPlugin, bevy::picking::InteractionPlugin, bevy::transform::TransformPlugin,
        bevy::text::TextPlugin, feathrise_runtime::DefaultFontPlugin, bevy::ui::UiPlugin,
        bevy::ui_render::UiRenderPlugin, feathrise_editor_ui::EditorUiPlugin));
    app.add_message::<SavedPlayFeedback>()
        .add_plugins(NativeProjectClosePlugin)
        .add_systems(Update, use_builtin_editor_font.after(ui::mount))
        .add_systems(PreUpdate, provide_native_cursor.after(bevy::input::InputSystems).before(bevy::ui::UiSystems::Focus))
        .add_systems(PreUpdate, restore_image_target.after(bevy::ui::UiSystems::Focus)
            .after(feathrise_editor_ui::update_editor_button_clicks));
    // 原七幅工程关闭截图保留显式Project语义，不能被默认Editor目标悄悄改写。
    app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Project);
    let mut texture = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
    texture.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(texture);
    let camera = app.world_mut().spawn((Camera2d, RenderTarget::from(target.clone()))).id();
    let window = app.world_mut().spawn((Window { resolution: (1280, 720).into(), ..default() }, PrimaryWindow)).id();
    app.insert_resource(CursorFixture { window, camera, image: target.clone(), position: None });
    let original = app.world_mut().spawn((WorkbenchHeader, feathrise_editor_ui::EditorThemeRoot,
        bevy::ui::UiTargetCamera(camera), Node { width: percent(100), height: percent(100), ..default() },
        BackgroundColor(feathrise_editor_ui::colors::PANEL_BG))).with_child((
            Text::new("工程作者现场 · 关闭确认层验收"), TextFont { font_size: 16.0.into(), ..default() },
            Node { position_type: PositionType::Absolute, left: px(18), top: px(18), ..default() },
        )).id();
    app.finish(); app.cleanup();
    // 与真实作者工作台相同，先挂载原界面，再经用户请求进入确认状态。
    settle(&mut app);
    assert!(!authorize_exit(app.world_mut(), true));
    settle(&mut app);
    assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Confirm);
    assert!(app.world().resource::<NativeProjectCloseState>().documents.is_empty());
    let clean = assert_compact(&mut app, 1280., true);
    let before = screenshot(&mut app, &target, &output.join("clean-wide.png"));
    let clean_chrome = assert_close_chrome(&mut app, 1280, &before);
    let clean_words = assert_visible_text_pixels(&mut app, 1280, &before);

    let close_button = actions(&mut app).into_iter().find(|(_, action)| *action == CloseAction::Close).unwrap().0;
    let close_rect = bounds(&app, close_button);
    app.world_mut().resource_mut::<CursorFixture>().position = Some(close_rect.center());
    settle(&mut app);
    assert_eq!(*app.world().get::<Interaction>(close_button).unwrap(), Interaction::Hovered, "必须由官方鼠标命中产生悬停");
    let hover = screenshot(&mut app, &target, &output.join("clean-hover.png"));
    let mut brighter = 0;
    for y in (close_rect.min.y + 3.).ceil() as usize..(close_rect.max.y - 3.).floor() as usize {
        for x in (close_rect.min.x + 3.).ceil() as usize..(close_rect.max.x - 3.).floor() as usize {
            let at = (y * 1280 + x) * 4;
            if (0..3).all(|channel| hover[at + channel] as u16 > before[at + channel] as u16 + 8) { brighter += 1; }
        }
    }
    assert!(brighter > 100, "共享按钮主题必须实际绘制悬停提亮：{brighter}");
    app.world_mut().resource_mut::<CursorFixture>().position = None;
    cancel_through_original_button(&mut app);
    assert!(app.world().get_entity(original).is_ok(), "取消后原作者实体仍属于同一World");
    screenshot(&mut app, &target, &output.join("cancelled.png"));

    resize_fixture(&mut app, &target, window, 280);
    assert!(!authorize_exit(app.world_mut(), true));
    settle(&mut app);
    let narrow = assert_compact(&mut app, 280., true);
    let narrow_pixels = screenshot(&mut app, &target, &output.join("clean-narrow.png"));
    let narrow_chrome = assert_close_chrome(&mut app, 280, &narrow_pixels);
    let narrow_words = assert_visible_text_pixels(&mut app, 280, &narrow_pixels);
    let header_close = entity::<CloseHeaderClose>(&mut app);
    cancel_through_button(&mut app, header_close);

    assert!(!authorize_exit(app.world_mut(), true));
    settle(&mut app);
    cancel_through_escape(&mut app, window);
    assert!(app.world().get_entity(original).is_ok());
    screenshot(&mut app, &target, &output.join("escape-cancelled.png"));

    // 点击可见“关闭”必须直接取得干净关闭许可，保存队列仍为Idle，不借Save/Discard路径。
    assert!(!authorize_exit(app.world_mut(), true));
    settle(&mut app);
    choose_action_through_mouse(&mut app, window, CloseAction::Close);
    assert!(authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().intent, Some(CloseAction::Close));
    assert_eq!(feathrise_editor_map::standalone::project_save_progress(app.world()),
        feathrise_editor_map::standalone::ProjectSaveProgress::Idle);
    settle(&mut app);
    screenshot(&mut app, &target, &output.join("clean-close-authorized.png"));
    cancel_through_original_button(&mut app);

    // 在同一个原确认层切换请求目标，核对实际标题/按钮及GPU墨迹，不能只改状态名。
    assert!(!authorize_exit(app.world_mut(), true));
    settle(&mut app);
    let held_upgrade = upgrade_while_project_button_is_held(&mut app, window);
    assert_eq!(app.world().resource::<NativeProjectCloseState>().intent, None);
    resize_fixture(&mut app, &target, window, 1280);
    let editor_clean_wide = capture_confirmation(&mut app, &target, &output.join("editor-clean-wide.png"), 1280, true);
    let header_close = entity::<CloseHeaderClose>(&mut app);
    cancel_through_button(&mut app, header_close);
    assert_eq!(app.world().resource::<NativeProjectCloseState>().target(), CloseTarget::Editor);

    resize_fixture(&mut app, &target, window, 280);
    assert!(!authorize_exit(app.world_mut(), true));
    let editor_clean_narrow = capture_confirmation(&mut app, &target, &output.join("editor-clean-narrow.png"), 280, true);
    choose_action_through_mouse(&mut app, window, CloseAction::Close);
    assert!(authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().target(), CloseTarget::Editor);
    assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Draining);
    assert_eq!(feathrise_editor_map::standalone::project_save_progress(app.world()),
        feathrise_editor_map::standalone::ProjectSaveProgress::Idle);
    cancel_through_original_button(&mut app);

    // 原地表作者生成真实基线差异，由正式关闭快照聚合出未保存列表，不手写展示文档。
    app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Project);
    app.world_mut().insert_resource(crate::native_authoring_validation::dirty_surface_fixture());
    assert!(!authorize_exit(app.world_mut(), true));
    settle(&mut app);
    let documents = app.world().resource::<NativeProjectCloseState>().documents.clone();
    assert!(!documents.is_empty());
    let dirty = assert_compact(&mut app, 280., false);
    let dirty_pixels = screenshot(&mut app, &target, &output.join("dirty-narrow.png"));
    let dirty_chrome = assert_close_chrome(&mut app, 280, &dirty_pixels);
    let dirty_words = assert_visible_text_pixels(&mut app, 280, &dirty_pixels);
    // 模拟刚被隐藏的旧按钮迟到松开；原输入门必须拒绝，不能把它变成放弃修改。
    let hidden_close = action_entity(&mut app, CloseAction::Close);
    app.world_mut().entity_mut(hidden_close).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.world_mut().run_system_once(ui::handle).unwrap();
    app.world_mut().entity_mut(hidden_close).insert(feathrise_editor_ui::EditorButtonClick::default());
    assert_eq!(app.world().resource::<NativeProjectCloseState>().choice, None);
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().intent, None);
    cancel_through_original_button(&mut app);

    // 有修改时仍由原领域生成三按钮分支；升级目标后不能沿用“仅关闭工程”的放弃意图。
    assert!(!authorize_exit(app.world_mut(), true));
    settle(&mut app);
    choose_action_through_original_button(&mut app, CloseAction::Discard);
    app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Editor);
    assert_eq!(app.world().resource::<NativeProjectCloseState>().choice, None);
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().intent, None);
    resize_fixture(&mut app, &target, window, 1280);
    let editor_dirty_wide = capture_confirmation(&mut app, &target, &output.join("editor-dirty-wide.png"), 1280, false);
    cancel_through_escape(&mut app, window);
    resize_fixture(&mut app, &target, window, 280);
    assert!(!authorize_exit(app.world_mut(), true));
    let editor_dirty_narrow = capture_confirmation(&mut app, &target, &output.join("editor-dirty-narrow.png"), 280, false);
    cancel_through_original_button(&mut app);
    let surface = app.world().resource::<feathrise_editor_surface_model::SurfacePresetEditorState>();
    assert!(surface.tabs[0].draft.dirty());
    assert_eq!(surface.tabs[0].draft.descriptor.label, "未保存地表·修改");
    assert!(app.world().get_entity(original).is_ok());
    let report = serde_json::json!({"passed":true,"fixture":owner,"clean":clean,"narrow":narrow,
        "dirty":dirty,"documents":documents,"hover_brighter_pixels":brighter,
        "dialog_chrome":{"wide":clean_chrome,"narrow":narrow_chrome,"dirty":dirty_chrome},
        "rendered_project_text":{"wide":clean_words,"narrow":narrow_words,"dirty":dirty_words},
        "editor":{"clean_wide":editor_clean_wide,"clean_narrow":editor_clean_narrow,
            "dirty_wide":editor_dirty_wide,"dirty_narrow":editor_dirty_narrow},
        "original_world_preserved":true,"dirty_draft_preserved":true,
        "clean_close_authorized_without_save":true,"hidden_close_rejected_after_dirty":true,"escape_and_header_cancelled":true,
        "editor_clean_exit_authorized_without_save":true,"project_choice_rejected_after_target_upgrade":true,
        "held_project_button_upgrade":held_upgrade,"fresh_editor_mouse_click_authorized":true,
        "scope":"离屏GPU与官方输入系统，不代表实体鼠标操作"});
    serde_json::to_writer_pretty(OpenOptions::new().write(true).create_new(true).open(output.join("evidence.json")).unwrap(), &report).unwrap();
    println!("FEATHRISE_CLOSE_UI_GPU_REPORT {}", output.display());
}
