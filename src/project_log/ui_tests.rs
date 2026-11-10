//! 工程日志生产界面的真实中文字体、原生指针选区与虚拟滚动回归；不创建系统窗口或读写系统剪贴板。

use super::{
    LogDecoder, LogSource, ProjectLogPlugin,
    components::{LogAction, LogBody, LogContent, LogDisplay, LogOverlay, LogScroll, LogSearchBar, LogSearchCount, LogSearchInput, LogStatus},
    resources::{LINE_HEIGHT, ProjectLogChannel, ProjectLogState, VIEW_LINES},
    scene, systems,
};
use bevy::{
    ecs::system::RunSystemOnce,
    camera::{ComputedCameraValues, RenderTargetInfo},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    input_focus::{InputDispatchPlugin, InputFocus, InputFocusPlugin, pointer_focus::PointerFocusPlugin},
    picking::{
        backend::HitData,
        events::{PointerDrag, PointerDragEnd, PointerDragStart, Pointer, PointerPress},
        pointer::{Location, PointerButton, PointerId},
    },
    prelude::*,
    text::{EditableText, TextEdit, TextLayoutInfo},
    ui_widgets::{
        ControlOrientation, TextInputPlugin, Scrollbar, ScrollbarDragState,
        ScrollbarPlugin, ScrollbarThumb,
    },
    window::PrimaryWindow,
};
use feathrise_editor_map::workbench::WorkbenchHeader;

#[derive(Resource)]
struct TestFont(Handle<Font>);

/// 安装原生产插件，只用ECS窗口与明确的相机目标尺寸提供布局环境。
fn layout_app(payload: &str) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
            watch_for_changes_override: Some(false),
            ..default()
        },
        bevy::scene::ScenePlugin,
        bevy::input::InputPlugin,
        InputFocusPlugin,
        InputDispatchPlugin,
        PointerFocusPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        feathrise_runtime::DefaultFontPlugin,
        bevy::ui::UiPlugin,
        TextInputPlugin,
        ScrollbarPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<bevy::image::TextureAtlasLayout>()
    .add_message::<bevy::window::Ime>()
    .add_plugins(ProjectLogPlugin {
        project_id: "日志界面回归夹具".into(),
        project_name: "日志界面回归夹具".into(),
    })
    // 正式地图输入也在Update提前应用官方队列，测试保留同一只读过滤与选区读取顺序。
    .add_systems(Update, bevy::text::apply_text_edits)
    .add_systems(
        Update,
        use_test_font.after(scene::mount).before(systems::present),
    );

    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(1280, 720),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
        ))
        .id();
    app.world_mut().spawn((
        WorkbenchHeader,
        Node {
            width: percent(100),
            height: percent(100),
            ..default()
        },
    ));
    let font = Handle::<Font>::default();
    app.insert_resource(TestFont(font));
    app.world_mut().resource_mut::<ProjectLogState>().open = true;

    // 通过生产解码与有界邮箱输入，避免直接往UI或状态写入预期成功结果。
    let channel = app.world().resource::<ProjectLogChannel>().clone();
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    decoder.write(payload.as_bytes());
    decoder.finish();
    settle(&mut app);
    (app, window, camera)
}

fn use_test_font(font: Res<TestFont>, mut texts: Query<&mut TextFont, (Without<LogBody>, Without<LogDisplay>, Without<TextSpan>)>) {
    for mut text in &mut texts {
        if text.font != font.0.clone().into() {
            text.font = font.0.clone().into();
        }
    }
}

fn settle(app: &mut App) {
    // 同时覆盖有界邮箱、软换行索引分批准备、字体登记和布局后的宽度回执。
    for _ in 0..16 {
        app.update();
    }
}

fn toggle_wrap(app: &mut App) {
    click_action(app, LogAction::Wrap);
    settle(app);
}

pub(super) fn click_action(app: &mut App, action: LogAction) {
    let world = app.world_mut();
    let button = world.query::<(Entity, &LogAction)>().iter(world)
        .find(|(_, candidate)| **candidate == action).unwrap().0;
    // 复用共享控件允许的动作队列脉冲，动作仍必须经过真实按钮携带的LogAction路由。
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.world_mut().run_system_once(systems::control).unwrap();
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::default());
}

fn entity<Marker: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Marker>>()
        .single(world)
        .unwrap()
}

fn bounds(app: &App, entity: Entity) -> Rect {
    Rect::from_center_size(
        app.world()
            .get::<UiGlobalTransform>(entity)
            .unwrap()
            .translation,
        app.world().get::<ComputedNode>(entity).unwrap().size(),
    )
}

fn content_bounds(app: &App, entity: Entity) -> Rect {
    let node = app.world().get::<ComputedNode>(entity).unwrap();
    let transform = app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .affine();
    Rect::from_corners(
        transform.transform_point2(node.content_box().min),
        transform.transform_point2(node.content_box().max),
    )
}

fn scroll_range(app: &App, pane: Entity) -> Vec2 {
    let node = app.world().get::<ComputedNode>(pane).unwrap();
    ((node.content_size() - (node.size() - node.scrollbar_size)) * node.inverse_scale_factor())
        .max(Vec2::ZERO)
}

fn scrollbar(app: &mut App, orientation: ControlOrientation) -> (Entity, Entity) {
    let world = app.world_mut();
    let track = world
        .query::<(Entity, &Scrollbar)>()
        .iter(world)
        .find(|(_, bar)| bar.orientation == orientation)
        .unwrap()
        .0;
    let thumb = app
        .world()
        .get::<Children>(track)
        .unwrap()
        .iter()
        .find(|child| app.world().get::<ScrollbarThumb>(*child).is_some())
        .unwrap();
    (track, thumb)
}

fn location(window: Entity, position: Vec2) -> Location {
    Location {
        target: bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window))
            .normalize(Some(window))
            .unwrap(),
        position,
    }
}

fn press(app: &mut App, window: Entity, camera: Entity, target: Entity, position: Vec2) {
    app.world_mut().trigger(PointerPress {
        entity: target,
        pointer: Pointer::new(PointerId::Mouse, location(window, position)),
        button: PointerButton::Primary,
            hit: HitData::new(camera, 0.0, None, None),
            count: 1,
    });
}

fn drag_start(app: &mut App, window: Entity, camera: Entity, target: Entity, position: Vec2) {
    app.world_mut().trigger(PointerDragStart {
        entity: target,
        pointer: Pointer::new(PointerId::Mouse, location(window, position)),
        button: PointerButton::Primary,
            hit: HitData::new(camera, 0.0, None, None),
    });
}

fn drag(app: &mut App, window: Entity, target: Entity, start: Vec2, distance: Vec2) {
    app.world_mut().trigger(PointerDrag {
        entity: target,
        pointer: Pointer::new(PointerId::Mouse, location(window, start + distance)),
        button: PointerButton::Primary,
            distance,
            delta: distance,
    });
}

fn drag_scrollbar(
    app: &mut App,
    window: Entity,
    camera: Entity,
    orientation: ControlOrientation,
    distance: Vec2,
) {
    let (_, thumb) = scrollbar(app, orientation);
    let start = bounds(app, thumb).center();
    press(app, window, camera, thumb, start);
    drag_start(app, window, camera, thumb, start);
    drag(app, window, thumb, start, distance);
    app.update();
    app.world_mut().trigger(PointerDragEnd {
        entity: thumb,
        pointer: Pointer::new(PointerId::Mouse, location(window, start + distance)),
        button: PointerButton::Primary,
            distance,
    });
    settle(app);
}

fn key(app: &mut App, window: Entity, code: KeyCode, logical: Key) {
    // 先让日志获得正文焦点，再走原生键盘消息；工作台在浮窗外也可接收同一按键。
    let body = entity::<LogBody>(app);
    app.world_mut().resource_mut::<InputFocus>().set(body, bevy::input_focus::FocusCause::Pressed);
    // 经原生键盘消息更新ButtonInput，不能在InputPlugin清理前手工设置just_pressed。
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical.clone(),
            state,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    }
}

fn assert_last_row_fits(app: &App, pane: Entity, body: Entity) {
    let viewport = content_bounds(app, pane);
    let body_rect = bounds(app, body);
    let row = Rect::from_corners(
        Vec2::new(viewport.min.x, body_rect.max.y - LINE_HEIGHT),
        Vec2::new(viewport.min.x + 1.0, body_rect.max.y),
    );
    assert!(
        row.min.y >= viewport.min.y - 1.1 && row.max.y <= viewport.max.y + 1.1,
        "End之后最新完整行仍被裁切：viewport={viewport:?}, last_row={row:?}"
    );
}

#[test]
fn native_scrollbars_cover_full_history_without_laying_out_more_than_64_rows() {
    let rows = (0..120)
        .map(|index| format!("日志第{index:03}行：生产缓存完整可见的中文内容。"))
        .collect::<Vec<_>>();
    let (mut app, window, camera) = layout_app(&(rows.join("\n") + "\n"));
    toggle_wrap(&mut app);
    let pane = entity::<LogScroll>(&mut app);
    let body = entity::<LogBody>(&mut app);
    let content = entity::<LogContent>(&mut app);
    assert_eq!(
        app.world().resource::<ProjectLogState>().row_count(),
        rows.len()
    );
    for orientation in [ControlOrientation::Vertical, ControlOrientation::Horizontal] {
        let (track, thumb) = scrollbar(&mut app, orientation);
        assert!(bounds(&app, track).size().min_element() > 0.0);
        assert!(bounds(&app, thumb).size().min_element() > 0.0);
        assert!(
            app.world().get::<Node>(thumb).is_none(),
            "原生滑块不能被Node尺寸覆盖"
        );
        assert_eq!(app.world().get::<Scrollbar>(track).unwrap().target, pane);
    }
    assert!(
        scroll_range(&app, pane).y > 1000.0,
        "滚动范围必须覆盖完整历史，而非只覆盖可见页"
    );
    assert!((bounds(&app, content).height() - rows.len() as f32 * LINE_HEIGHT).abs() < 1.1);

    // 真实原生滑块把窗口拖到最早记录，不能直接设置state.scroll_back伪造翻页结果。
    drag_scrollbar(
        &mut app,
        window,
        camera,
        ControlOrientation::Vertical,
        Vec2::new(0.0, -100_000.0),
    );
    let first_page = app
        .world()
        .get::<EditableText>(body)
        .unwrap()
        .value()
        .to_string();
    assert!(first_page.starts_with(&rows[0]));
    assert!(first_page.lines().count() <= VIEW_LINES);
    assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().y, 0.0);
    assert!(!app.world().resource::<ProjectLogState>().follow);

    key(&mut app, window, KeyCode::End, Key::End);
    settle(&mut app);
    let last_page = app
        .world()
        .get::<EditableText>(body)
        .unwrap()
        .value()
        .to_string();
    assert!(last_page.ends_with(rows.last().unwrap()));
    assert!(last_page.lines().count() <= VIEW_LINES);
    assert!(app.world().resource::<ProjectLogState>().follow);
    assert_last_row_fits(&app, pane, body);
}

#[test]
fn native_pointer_selects_real_chinese_glyphs_and_close_releases_focus_and_drag() {
    let source = "首行可以拖选中文日志内容，验证原生真实字体字形与选区。\n第二行保持完整。";
    let (mut app, window, camera) = layout_app(source);
    let body = entity::<LogBody>(&mut app);
    let body_rect = content_bounds(&app, body);
    assert!(body_rect.width() > 150.0 && body_rect.height() >= LINE_HEIGHT * 2.0 - 1.1);
    assert!(
        !app.world()
            .get::<TextLayoutInfo>(body)
            .unwrap()
            .glyphs
            .is_empty()
    );
    let start = body_rect.min + Vec2::new(1.0, LINE_HEIGHT * 0.5);
    let distance = Vec2::new(150.0, 0.0);
    press(&mut app, window, camera, body, start);
    app.update();
    drag_start(&mut app, window, camera, body, start);
    drag(&mut app, window, body, start, distance);
    settle(&mut app);
    let text = app.world().get::<EditableText>(body).unwrap();
    let selected = text
        .editor
        .selected_text()
        .expect("原生拖动必须生成真实选区")
        .to_owned();
    assert!(!selected.is_empty() && source.contains(&selected));
    assert!(
        selected.chars().any(|character| !character.is_ascii()),
        "不能仅用空格或假字形证明中文选区"
    );
    assert!(
        !app.world()
            .get::<TextLayoutInfo>(body)
            .unwrap()
            .selection_rects
            .is_empty()
    );
    assert_eq!(
        app.world().resource::<ProjectLogState>().copy_text(false),
        selected
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(body));

    // 同一真实控件队列中的编辑命令应被生产只读门阻止；测试只用本地编辑，失败路径也不接触系统剪贴板。
    let before = text.value().to_string();
    let mut text = app.world_mut().get_mut::<EditableText>(body).unwrap();
    text.queue_edit(TextEdit::Insert("不可写入".into()));
    text.queue_edit(TextEdit::Delete);
    drop(text);
    settle(&mut app);
    assert_eq!(
        app.world()
            .get::<EditableText>(body)
            .unwrap()
            .value()
            .to_string(),
        before
    );

    let (_, thumb) = scrollbar(&mut app, ControlOrientation::Vertical);
    let position = bounds(&app, thumb).center();
    drag_start(&mut app, window, camera, thumb, position);
    assert!(
        app.world()
            .get::<ScrollbarDragState>(thumb)
            .unwrap()
            .dragging
    );
    key(&mut app, window, KeyCode::Escape, Key::Escape);
    settle(&mut app);
    assert!(!app.world().resource::<ProjectLogState>().open);
    assert!(!app.world().resource::<ProjectLogState>().owns_keyboard());
    assert_ne!(app.world().resource::<InputFocus>().get(), Some(body));
    assert!(
        !app.world()
            .get::<ScrollbarDragState>(thumb)
            .unwrap()
            .dragging
    );
    let overlay = entity::<LogOverlay>(&mut app);
    assert_eq!(
        app.world().get::<Node>(overlay).unwrap().display,
        Display::None
    );
}

#[test]
fn final_long_line_gains_horizontal_range_without_any_followup_output() {
    // 小于单行1KiB上限，确保测试的是一条真正的超长末行而非分段产生的后续事件。
    let long_line = format!("唯一末行：{}", "横向完整日志".repeat(45));
    assert!(long_line.len() < super::resources::LINE_BYTES);
    let (mut app, window, camera) = layout_app(&long_line);
    toggle_wrap(&mut app);
    let pane = entity::<LogScroll>(&mut app);
    let body = entity::<LogBody>(&mut app);
    assert_eq!(app.world().resource::<ProjectLogState>().row_count(), 1);
    assert_eq!(
        app.world()
            .get::<EditableText>(body)
            .unwrap()
            .value()
            .to_string(),
        long_line
    );
    let layout = app.world().get::<TextLayoutInfo>(body).unwrap();
    assert!(layout.size.x > content_bounds(&app, pane).width() + 200.0);
    assert!(
        scroll_range(&app, pane).x > 200.0,
        "最后一次PostLayout的真实宽度必须反馈给滚动容器"
    );
    assert!(bounds(&app, body).width() >= layout.size.x - 1.1);
    let (_, thumb) = scrollbar(&mut app, ControlOrientation::Horizontal);
    let (track, _) = scrollbar(&mut app, ControlOrientation::Horizontal);
    assert!(bounds(&app, thumb).width() < bounds(&app, track).width());

    drag_scrollbar(
        &mut app,
        window,
        camera,
        ControlOrientation::Horizontal,
        Vec2::new(100_000.0, 0.0),
    );
    assert!(app.world().get::<ScrollPosition>(pane).unwrap().x > 200.0);
    assert_eq!(
        app.world().resource::<ProjectLogState>().row_count(),
        1,
        "没有新消息也必须完成宽度反馈"
    );
    assert_eq!(
        app.world()
            .get::<EditableText>(body)
            .unwrap()
            .value()
            .to_string(),
        long_line
    );
}

#[test]
fn default_soft_wrap_hides_horizontal_bar_and_preserves_native_selection_when_toggled() {
    let source = format!("长路径：{}", r"D:\worktrees\824b\bevy_learn5_3d\rpg\crates\角色动画\src\preview.rs;".repeat(11));
    assert!(source.len() < super::resources::LINE_BYTES);
    let (mut app, window, camera) = layout_app(&format!("\x1b[33m{source}\x1b[0m"));
    let pane = entity::<LogScroll>(&mut app);
    let body = entity::<LogBody>(&mut app);
    let display = entity::<LogDisplay>(&mut app);
    let (horizontal, thumb) = scrollbar(&mut app, ControlOrientation::Horizontal);
    assert!(app.world().resource::<ProjectLogState>().wrap, "首次打开默认软换行");
    assert_eq!(app.world().get::<Node>(horizontal).unwrap().display, Display::None);
    assert!(scroll_range(&app, pane).x <= 1.1, "换行时不能保留隐藏的横向溢出范围");
    assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().x, 0.);
    let layout = app.world().get::<TextLayoutInfo>(body).unwrap();
    assert!(layout.size.y >= LINE_HEIGHT * 3. - 1.1, "同一逻辑长行必须实际折成多行");
    assert!(layout.size.x <= content_bounds(&app, pane).width() + 1.1);
    assert_eq!(app.world().get::<EditableText>(body).unwrap().value().to_string(), source);
    let visible = app.world().get::<TextLayoutInfo>(display).unwrap();
    assert_eq!(layout.glyphs.len(), visible.glyphs.len());
    for (input, output) in layout.glyphs.iter().zip(&visible.glyphs) {
        assert!((input.position - output.position).length() < 0.1, "软换行后的富色文字与原生字形命中必须逐字对齐");
    }
    let footer = entity::<LogStatus>(&mut app);
    let status = &app.world().get::<Text>(footer).unwrap().0;
    assert!(status.starts_with("1 行") && !status.contains("日志界面回归夹具"));
    assert_last_row_fits(&app, pane, body);

    // 实际跨两次软换行拖选，原文和复制内容都不得多出显示层的换行符。
    let start = content_bounds(&app, body).min + Vec2::new(1., LINE_HEIGHT * 0.5);
    press(&mut app, window, camera, body, start);
    app.update();
    drag_start(&mut app, window, camera, body, start);
    drag(&mut app, window, body, start, Vec2::new(230., LINE_HEIGHT * 2.));
    settle(&mut app);
    let selected = app.world().get::<EditableText>(body).unwrap().editor.selected_text().unwrap().to_owned();
    assert!(!selected.is_empty() && source.contains(&selected) && !selected.contains('\n'));
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(false), selected);
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(true), source);

    toggle_wrap(&mut app);
    assert!(!app.world().resource::<ProjectLogState>().wrap);
    assert_eq!(app.world().get::<Node>(horizontal).unwrap().display, Display::Flex);
    assert!(scroll_range(&app, pane).x > 200.);
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(false), selected);
    drag_scrollbar(&mut app, window, camera, ControlOrientation::Horizontal, Vec2::new(100_000., 0.));
    assert!(app.world().get::<ScrollPosition>(pane).unwrap().x > 200.);
    let position = bounds(&app, thumb).center();
    drag_start(&mut app, window, camera, thumb, position);
    assert!(app.world().get::<ScrollbarDragState>(thumb).unwrap().dragging);
    toggle_wrap(&mut app);
    assert!(app.world().resource::<ProjectLogState>().wrap);
    assert_eq!(app.world().get::<Node>(horizontal).unwrap().display, Display::None);
    assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().x, 0.);
    assert!(!app.world().get::<ScrollbarDragState>(thumb).unwrap().dragging, "隐藏横条同时撤销旧拖动捕获");
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(false), selected);
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(true), source);
}

#[test]
fn wrapped_history_uses_visual_rows_for_native_vertical_scroll_and_end() {
    let rows = (0..40).map(|index| format!("记录{index:03}：{}末尾{index:03}", "中文长路径D:\\project\\assets\\characters\\".repeat(12))).collect::<Vec<_>>();
    assert!(rows.iter().all(|row| row.len() < super::resources::LINE_BYTES));
    let (mut app, window, camera) = layout_app(&(rows.join("\n") + "\n"));
    let pane = entity::<LogScroll>(&mut app);
    let body = entity::<LogBody>(&mut app);
    let content = entity::<LogContent>(&mut app);
    let state = app.world().resource::<ProjectLogState>();
    assert!(state.wrap && state.display_row_count() > rows.len() * 2);
    assert!((bounds(&app, content).height() - state.display_row_count() as f32 * LINE_HEIGHT).abs() < 1.1);
    assert!(app.world().get::<TextLayoutInfo>(body).unwrap().size.y <= VIEW_LINES as f32 * LINE_HEIGHT + 1.1);
    assert_last_row_fits(&app, pane, body);
    drag_scrollbar(&mut app, window, camera, ControlOrientation::Vertical, Vec2::new(0., -100_000.));
    assert!(app.world().get::<EditableText>(body).unwrap().value().to_string().starts_with("记录000："));
    assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().y, 0.);
    key(&mut app, window, KeyCode::End, Key::End);
    settle(&mut app);
    assert!(app.world().get::<EditableText>(body).unwrap().value().to_string().ends_with("末尾039"));
    assert!(app.world().resource::<ProjectLogState>().follow);
    assert_last_row_fits(&app, pane, body);
}

#[test]
fn resize_rebuild_keeps_old_page_until_new_utf8_tail_and_visual_index_match() {
    let rows = (0..120).map(|index| format!("历史{index:03}：{}", "D:\\project\\assets\\characters\\作者预览\\".repeat(12))).collect::<Vec<_>>();
    assert!(rows.iter().all(|row| row.len() < super::resources::LINE_BYTES));
    let (mut app, window, camera) = layout_app(&(rows.join("\n") + "\n"));
    let pane = entity::<LogScroll>(&mut app);
    let body = entity::<LogBody>(&mut app);
    let display = entity::<LogDisplay>(&mut app);
    let channel = app.world().resource::<ProjectLogChannel>().clone();
    let sink = channel.begin();
    let mut decoder = LogDecoder::new(Some(sink.clone()), LogSource::Stdout);
    // 旧尾行末端881不是3的倍数；套到之后900字节的纯中文正文会落在UTF-8码点内部。
    let ascii_tail = "x".repeat(881);
    decoder.write(ascii_tail.as_bytes());
    settle(&mut app);
    let previous = app.world().get::<EditableText>(body).unwrap().value().to_string();
    assert!(previous.ends_with(&ascii_tail));
    let old_width = app.world().resource::<ProjectLogState>().wrap_layout.width;
    let old_revision = app.world().resource::<ProjectLogState>().content_revision;
    assert_eq!(app.world().resource::<ProjectLogState>().wrap_layout.revision, Some(old_revision));

    // 更新真实UI布局目标。下一帧旧宽度的索引仍合法，PostLayout才发布新的ComputedNode宽度。
    app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(640., 720.);
    app.world_mut().get_mut::<Camera>(camera).unwrap().computed.target_info.as_mut().unwrap().physical_size.x = 640;
    app.update();
    assert!(content_bounds(&app, pane).width() < old_width - 200., "夹具必须真正改变排版宽度");
    let chinese_tail = "正在构建".repeat(75);
    assert_eq!(chinese_tail.len(), 900);
    decoder.write(format!("\r\x1b[2K{chinese_tail}").as_bytes());
    app.update();
    let state = app.world().resource::<ProjectLogState>();
    assert!(state.content_revision > old_revision);
    assert_ne!(state.wrap_layout.revision, Some(state.content_revision), "120条历史的宽度重排不能在首个64条预算内伪完成");
    assert_eq!(app.world().get::<EditableText>(body).unwrap().value().to_string(), previous,
        "新索引尚未完成时保留旧实际页面，不能用旧UTF-8边界截取新尾行");
    let mut incoming = LogDecoder::new(Some(sink), LogSource::Stderr);
    let mut appended = Vec::new();
    for batch in 0..8 {
        // 重排期间仍按生产采用上限持续新增32条；不能靠停止日志输出才完成新宽度索引。
        let next = (0..super::resources::FRAME_LINES).map(|index|
            format!("持续输出-{batch}-{index:02}")).collect::<Vec<_>>();
        incoming.write((next.join("\n") + "\n").as_bytes());
        appended.extend(next);
        app.update();
    }
    let state = app.world().resource::<ProjectLogState>();
    assert_eq!(state.wrap_layout.revision, Some(state.content_revision));
    assert!(state.wrap_layout.width < old_width - 200. && state.follow);
    assert_eq!(state.copy_text(true), format!("{}\n{}\n{chinese_tail}", rows.join("\n"), appended.join("\n")));
    assert!(app.world().get::<EditableText>(body).unwrap().value().to_string().ends_with(&chinese_tail));
    assert_last_row_fits(&app, pane, body);
    let input = app.world().get::<TextLayoutInfo>(body).unwrap();
    let visible = app.world().get::<TextLayoutInfo>(display).unwrap();
    assert!(input.size.y <= VIEW_LINES as f32 * LINE_HEIGHT + 1.1);
    assert_eq!(input.glyphs.len(), visible.glyphs.len());
    for (left, right) in input.glyphs.iter().zip(&visible.glyphs) {
        assert!((left.position - right.position).length() < 0.1, "重排后的中文尾行与原生命中必须逐字对齐");
    }
}

#[test]
fn task_generation_without_output_withdraws_old_live_tail_and_refreshes_visible_page() {
    let rows = (0..120).map(|index| format!("已完成{index:03}：{}", "D:\\project\\assets\\characters\\历史日志\\".repeat(12))).collect::<Vec<_>>();
    assert!(rows.iter().all(|row| row.len() < super::resources::LINE_BYTES));
    let (mut app, _, _) = layout_app(&(rows.join("\n") + "\n"));
    let pane = entity::<LogScroll>(&mut app);
    let body = entity::<LogBody>(&mut app);
    let channel = app.world().resource::<ProjectLogChannel>().clone();
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    let old_tail = format!("旧任务实时进度：{}", "等待构建".repeat(45));
    decoder.write(old_tail.as_bytes());
    settle(&mut app);
    let state = app.world().resource::<ProjectLogState>();
    assert!(state.copy_text(true).ends_with(&old_tail));
    assert!(app.world().get::<EditableText>(body).unwrap().value().to_string().ends_with(&old_tail));
    assert_eq!(state.wrap_layout.revision, Some(state.content_revision));
    let previous_revision = state.content_revision;
    let previous_sequence = channel.sequence.load(std::sync::atomic::Ordering::Acquire);

    // General开始只更新原生产任务代次，不产生分隔行，也不往邮箱补发正文或空尾行。
    let _replacement = channel.begin();
    assert_eq!(channel.sequence.load(std::sync::atomic::Ordering::Acquire), previous_sequence);
    settle(&mut app);
    let state = app.world().resource::<ProjectLogState>();
    assert!(state.content_revision > previous_revision, "没有新输出也须采用任务换代，撤回旧临时尾行");
    assert_eq!(state.wrap_layout.revision, Some(state.content_revision));
    assert_eq!(state.row_count(), rows.len());
    assert_eq!(state.copy_text(true), rows.join("\n"), "换代只撤回旧实时行，完整已提交历史继续保留");
    let visible = app.world().get::<EditableText>(body).unwrap().value().to_string();
    assert!(!visible.contains(&old_tail));
    assert!(visible.ends_with(rows.last().unwrap()));
    assert_last_row_fits(&app, pane, body);
}

#[test]
fn search_buttons_index_ansi_history_and_live_tail_without_changing_original_text() {
    let rows = (0..90).map(|index| match index {
        1 => "中文 \x1b[33mWARNING\x1b[0m warning，首处".into(),
        79 => format!("{}Warning，末处", "依赖构建路径".repeat(38)),
        _ => format!("普通构建输出 {index}"),
    }).collect::<Vec<String>>();
    let (mut app, window, camera) = layout_app(&(rows.join("\n") + "\n"));
    let field = entity::<LogSearchInput>(&mut app);
    let bar = entity::<LogSearchBar>(&mut app);
    let count = entity::<LogSearchCount>(&mut app);
    let pane = entity::<LogScroll>(&mut app);
    let body = entity::<LogBody>(&mut app);
    let copy = app.world().resource::<ProjectLogState>().copy_text(true);
    assert_eq!(app.world().get::<Node>(bar).unwrap().display, Display::None);
    click_action(&mut app, LogAction::Search);
    settle(&mut app);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    assert_eq!(app.world().get::<Node>(bar).unwrap().display, Display::Flex);

    // 走已聚焦的原生键盘文本，验证过滤之前的真实输入链和忽略 ASCII 大小写。
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyW,
        logical_key: Key::Character("w".into()),
        state: ButtonState::Pressed,
        text: Some("wArNiNg".into()),
        repeat: false,
        window,
    });
    app.update();
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "正在查找…",
        "超过 64 行的全量查找不能在首帧发布部分结果");
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "1/3 处匹配");
    assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().y, 0.);
    assert!(!app.world().resource::<ProjectLogState>().follow);
    assert!(app.world().get::<EditableText>(body).unwrap().value().to_string().contains("首处"));

    let body_point = content_bounds(&app, body).min + Vec2::new(2., LINE_HEIGHT / 2.);
    press(&mut app, window, camera, body, body_point);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(body));
    let field_point = content_bounds(&app, field).center();
    press(&mut app, window, camera, field, field_point);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));

    click_action(&mut app, LogAction::SearchNext);
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "2/3 处匹配");
    click_action(&mut app, LogAction::SearchNext);
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "3/3 处匹配");
    assert!(app.world().get::<ScrollPosition>(pane).unwrap().y > 1_000.);
    assert!(app.world().get::<EditableText>(body).unwrap().value().to_string().contains("末处"));
    click_action(&mut app, LogAction::SearchNext);
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "1/3 处匹配");
    click_action(&mut app, LogAction::SearchPrevious);
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "3/3 处匹配");
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(true), copy);

    let channel = app.world().resource::<ProjectLogChannel>().clone();
    let mut tail = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    tail.write("WARNING 临时进度".as_bytes());
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "3/4 处匹配");
    tail.write("\r\x1b[2K实时进度".as_bytes());
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "3/3 处匹配");
    click_action(&mut app, LogAction::Clear);
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "0/0 处匹配");
    assert!(app.world().resource::<ProjectLogState>().follow);
}

#[test]
fn clicking_back_into_log_search_restores_widget_focus() {
    let (mut app, window, camera) = layout_app("warning: fixture\n");
    let field = entity::<LogSearchInput>(&mut app);
    click_action(&mut app, LogAction::Search);
    settle(&mut app);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));

    // 模拟点击空白工作区。原指针焦点插件将 AcquireFocus 请求冒泡到主窗口并释放焦点。
    press(&mut app, window, camera, window, Vec2::new(400., 200.));
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), None);

    let point = content_bounds(&app, field).center();
    press(&mut app, window, camera, field, point);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyW, logical_key: Key::Character("w".into()),
        state: ButtonState::Pressed, text: Some("w".into()), repeat: false, window,
    });
    app.update();
    assert_eq!(app.world().get::<EditableText>(field).unwrap().value().to_string(), "w");
}

#[test]
fn log_find_keeps_real_text_focus_and_routes_clipboard_shortcuts() {
    let (mut app, window, camera) = layout_app("warning: fixture\n");
    let body = entity::<LogBody>(&mut app);
    let field = entity::<LogSearchInput>(&mut app);
    let bar = entity::<LogSearchBar>(&mut app);
    let foreign = app.world_mut().spawn(feathrise_editor_core::EditorTextInput).id();
    app.world_mut().resource_mut::<InputFocus>().set(foreign, bevy::input_focus::FocusCause::Navigated);
    let key = |app: &mut App, key_code, logical_key, state, text: Option<&str>| {
        app.world_mut().write_message(KeyboardInput {
            key_code, logical_key, state, text: text.map(Into::into), repeat: false, window,
        });
    };
    let control = |app: &mut App, state| key(app, KeyCode::ControlLeft, Key::Control, state, None);
    let find = |app: &mut App, state| key(app, KeyCode::KeyF, Key::Character("f".into()), state, None);
    control(&mut app, ButtonState::Pressed);
    find(&mut app, ButtonState::Pressed);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(foreign));
    assert_eq!(app.world().get::<Node>(bar).unwrap().display, Display::None);
    assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyF));
    find(&mut app, ButtonState::Released);
    app.update();

    // 指针命中已确认目标，浮窗悬停仍是旧帧时，这次真实按下也要获得并保持焦点。
    let point = content_bounds(&app, body).center();
    press(&mut app, window, camera, body, point);
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Pressed, window });
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(body));
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Released, window });
    find(&mut app, ButtonState::Pressed);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    assert_eq!(app.world().get::<Node>(bar).unwrap().display, Display::Flex);
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyF));

    find(&mut app, ButtonState::Released);
    control(&mut app, ButtonState::Released);
    let point = content_bounds(&app, field).center();
    press(&mut app, window, camera, field, point);
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Pressed, window });
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Released, window });
    key(&mut app, KeyCode::KeyW, Key::Character("w".into()), ButtonState::Pressed, Some("w"));
    app.update();
    assert_eq!(app.world().get::<EditableText>(field).unwrap().value().to_string(), "w");

    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    assert!(!app.world().resource::<ProjectLogState>().owns_keyboard());
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    control(&mut app, ButtonState::Pressed);
    key(&mut app, KeyCode::KeyA, Key::Character("a".into()), ButtonState::Pressed, None);
    app.update();
    // 在官方输入分发之后观察复制／粘贴请求，避免专项读写用户的真实剪贴板。
    for (code, letter, expected) in [(KeyCode::KeyC, "c", TextEdit::Copy), (KeyCode::KeyV, "v", TextEdit::Paste)] {
        key(&mut app, code, Key::Character(letter.into()), ButtonState::Pressed, None);
        app.world_mut().run_schedule(PreUpdate);
        let mut input = app.world_mut().get_mut::<EditableText>(field).unwrap();
        assert!(std::mem::take(&mut input.pending_edits).iter().any(|edit|
            std::mem::discriminant(edit) == std::mem::discriminant(&expected)));
        key(&mut app, code, Key::Character(letter.into()), ButtonState::Released, None);
    }
    app.world_mut().get_mut::<EditableText>(field).unwrap().pending_paste =
        Some(bevy::clipboard::ClipboardRead::Ready(Ok("warning".into())));
    control(&mut app, ButtonState::Released);
    settle(&mut app);
    assert_eq!(app.world().get::<EditableText>(field).unwrap().value().to_string(), "warning");
    let count = entity::<LogSearchCount>(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "1/1 处匹配");
    assert!(app.world().resource::<ProjectLogState>().copy_text(true).contains("warning: fixture"));
}

#[test]
fn unwrapped_search_reveals_match_beyond_the_horizontal_viewport() {
    let row = format!("{}唯一目标", "a".repeat(290));
    let (mut app, _, _) = layout_app(&row);
    let pane = entity::<LogScroll>(&mut app);
    let display = entity::<LogDisplay>(&mut app);
    let field = entity::<LogSearchInput>(&mut app);
    let count = entity::<LogSearchCount>(&mut app);
    app.world_mut().resource_mut::<bevy::ui::UiScale>().0 = 0.75;
    toggle_wrap(&mut app);
    click_action(&mut app, LogAction::Search);
    // 注入的是官方文本编辑请求，提交查询、定位和字形颜色都经过原生产系统。
    app.world_mut().get_mut::<EditableText>(field).unwrap()
        .queue_edit(TextEdit::Insert("唯一目标".into()));
    settle(&mut app);
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "1/1 处匹配");
    let left = app.world().get::<ScrollPosition>(pane).unwrap().x
        / app.world().get::<ComputedNode>(pane).unwrap().inverse_scale_factor();
    assert!(left > 300., "命中原本在未换行的可视区右侧");
    let geometry = &app.world().get::<TextLayoutInfo>(display).unwrap().run_geometry;
    let active = geometry.iter().find(|run| run.section_index == 2).expect("可视区域须含当前匹配背景");
    assert!(active.bounds.min.x >= left - 1.);
    assert!(active.bounds.max.x <= left + content_bounds(&app, pane).width() + 1.);
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(true), row);
}
