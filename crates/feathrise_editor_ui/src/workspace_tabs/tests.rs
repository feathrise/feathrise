//! 使用真实Bevy布局和命中系统检查紧凑标签，不启动窗口或GPU。
//!
//! 字体测量结果由固定内在尺寸提供，隔离字库IO；生产仍使用TextLayout的真实字体测量。

use super::*; // 直接调用生产标签构造函数，不复制标题与关闭键布局。
use bevy::app::{HierarchyPropagatePlugin, PropagateSet, TaskPoolPlugin}; // 与Bevy布局测试相同的最小调度。
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport}; // 提供不同窗口尺寸的虚拟渲染目标。
use bevy::ecs::{system::RunSystemOnce, world::CommandQueue}; // 应用真实构造命令并直接执行命中系统。
use bevy::input::touch::Touches; // 命中系统必需资源，不伪造触屏事件。
use bevy::transform::{StaticTransformOptimizations, systems::*}; // UI布局后的真实层级变换。
use bevy::ui::{
    ContentSize, UiStack, ui_focus_system, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
}; // 不加载渲染插件也能计算真实布局、溢出和鼠标命中。
use bevy::window::PrimaryWindow; // 虚拟相机的鼠标来源。

/// 替代领域动作，仅记录点击落在哪一个互斥命中区。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Map,             // 永久地图标签没有关闭按钮。
    Activate(usize), // 点击标题只切换所属预设。
    Close(usize),    // 点击小关闭键不同时激活标题或邻居。
}

/// 最小真实UI世界，仅保留测试需要调整的固定身份。
struct Fixture {
    app: App,       // Bevy布局和输入系统。
    host: Entity,   // 唯一横向滚动及裁切节点。
    window: Entity, // 只承载输入数据，不创建操作系统窗口。
}

/// 构造短标题、长标题及关闭确认组合；宽度参数表示字体系统已经测得的逻辑尺寸。
fn fixture(width: u32, scale: f32, tabs: &[(&str, f32, bool)]) -> Fixture {
    let mut app = App::new(); // 测试不需要游戏资源、地图或GPU。
    crate::button_input::register(&mut app); // 与工作台相同的按下捕获/释放派发，保留原命中与布局夹具。
    app.add_plugins(TaskPoolPlugin::default()); // 初始化布局需要的任务池。
    app.add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
        PostUpdate,
    )); // 传递UI相机身份。
    app.add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
        PostUpdate,
    )); // 传递目标尺寸与缩放。
    app.insert_resource(UiScale(scale)) // 覆盖窗口与全屏使用的不同逻辑缩放。
        .init_resource::<UiSurface>() // 真实Taffy布局缓存。
        .init_resource::<bevy::text::TextPipeline>() // 布局系统所需字体管线。
        .init_resource::<bevy::text::FontCx>() // 固定测量无需加载实际字库。
        .init_resource::<bevy::text::ScaleCx>() // 与Bevy生产布局参数一致。
        .init_resource::<StaticTransformOptimizations>() // 层级变换系统所需资源。
        .init_resource::<ButtonInput<MouseButton>>() // 测试显式发送鼠标按下与松开。
        .init_resource::<Touches>(); // 没有触摸输入。
    app.add_systems(Update, (
        feathrise_ui_widgets::ensure_button_colors_system,
        feathrise_ui_widgets::button_color_system,
    ).chain()); // 与正式编辑器相同的按钮反馈，检查活动标签和关闭键在交互后仍能恢复统一底色。
    app.add_systems(
        PostUpdate,
        (
            ApplyDeferred,               // 先应用UI构造命令。
            propagate_ui_target_cameras, // 确定相机与尺寸来源。
            ui_layout_system,            // 实际解析Auto宽度、Flex及Scroll。
            mark_dirty_trees,            // 为真实层级变换标记变化。
            sync_simple_transforms,      // 同步独立变换。
            propagate_parent_transforms, // 同步层级变换。
        )
            .chain(),
    ); // 与Bevy自身布局回归采用相同顺序。
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiTargetCamera>::default()
            .after(propagate_ui_target_cameras)
            .before(ui_layout_system),
    ); // 相机先于布局传播。
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(ui_layout_system),
    ); // 缩放先于布局传播。
    let window = app
        .world_mut()
        .spawn((PrimaryWindow, Window::default()))
        .id(); // 仅创建ECS数据。
    let target_size = UVec2::new(width, 180); // 只需容纳一条标签栏。
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: target_size, // 每组测试明确当前物理窗口宽度。
                    scale_factor: 1.0,          // 缩放只由UiScale给出，避免重复应用。
                }),
                ..default() // 无需渲染矩阵。
            },
            viewport: Some(Viewport {
                physical_size: target_size, // 命中系统与布局系统使用相同目标。
                ..default()                 // 视口从窗口左上角开始。
            }),
            ..default() // 默认主窗口目标。
        },
    )); // 不依赖RenderPlugin更新相机。
    let mut queue = CommandQueue::default(); // 复用生产函数要求的Commands接口。
    let mut commands = Commands::new(&mut queue, app.world()); // 只读世界，延迟生成UI。
    let mut host = Entity::PLACEHOLDER; // 在子节点构造时记录真实滚动身份。
    let root = commands
        .spawn(Node {
            width: Val::Percent(100.0), // 对齐实际标签条的窗口宽度约束。
            height: Val::Px(34.0),      // 与生产静态壳相同的逻辑高度。
            padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)), // 保留壳层内边距。
            border: UiRect::bottom(Val::Px(1.0)), // 下边框参与可用高度计算。
            align_items: AlignItems::Center, // 地图和滚动区在同一水平线。
            column_gap: Val::Px(3.0),   // 与生产静态壳保持一致。
            overflow: Overflow::clip_x(), // 仅允许动态内容通过子滚动区访问。
            ..default()                 // 不增加额外根缩放。
        })
        .with_children(|bar| {
            bar.spawn((Button, Action::Map, map_tab_node(), FocusPolicy::Block)) // 使用生产地图标签布局。
                .with_child((
                    Text::new("地图视口"),
                    TextLayout::no_wrap(),
                    FocusPolicy::Pass,
                )); // 提供地图文字测量叶子。
            host = bar
                .spawn((
                    ScrollPosition::default(),
                    Node {
                        height: Val::Percent(100.0),     // 与生产滚动挂载点一致。
                        flex_grow: 1.0,                  // 地图之外的宽度由滚动区接管。
                        min_width: Val::Px(0.0),         // 超宽时允许挂载点缩窄。
                        align_items: AlignItems::Center, // 动态标签保持统一垂直中心。
                        overflow: Overflow::scroll_x(),  // 唯一实际横向滚动容器。
                        ..default()                      // 内部内容根自行保留自然宽度。
                    },
                ))
                .with_children(|host| {
                    host.spawn(tab_list_node()).with_children(|list| {
                        // 直接测试生产内容根，不伪造宽度。
                        for (index, &(label, _, confirmation)) in tabs.iter().enumerate() {
                            spawn_preset_tab(
                                list,
                                &Handle::default(),
                                label,
                                Action::Activate(index),
                                Action::Close(index),
                                index == 0,
                                confirmation,
                            ); // 原封不动调用生产标签。
                        }
                    });
                })
                .id(); // 记录可观察滚动状态。
        })
        .id(); // 记录输入堆栈的唯一UI根。
    queue.apply(app.world_mut()); // 将生产构造函数生成的组件真正写入ECS。
    let text_nodes: Vec<_> = app
        .world_mut()
        .query::<(Entity, &Text)>()
        .iter(app.world())
        .map(|(entity, text)| (entity, text.0.clone()))
        .collect(); // 只收集这组有限测试叶子。
    for (entity, text) in text_nodes {
        let measured_width = match text.as_str() {
            "地图视口" => 38.0, // 测试夹具中的地图文字内在宽度。
            "×" => 8.0,         // 普通关闭符号。
            "确认×" => 25.0,    // 确认文案必须能扩宽，不挤出外框。
            _ => tabs.iter().find(|tab| tab.0 == text).unwrap().1, // 由用例提供测量结果，不按字符数猜尺寸。
        };
        app.world_mut()
            .entity_mut(entity)
            .insert(ContentSize::fixed_size(
                Vec2::new(measured_width, 12.0) * scale,
            )); // 模拟字体系统返回的物理内在尺寸。
    }
    let mut stack = Vec::new(); // 以Children顺序构造与渲染一致的前后堆栈。
    collect_stack(app.world(), root, &mut stack); // 只遍历该测试生成的有限树。
    for &entity in &stack {
        app.world_mut()
            .entity_mut(entity)
            .insert(InheritedVisibility::VISIBLE); // 无VisibilityPlugin时明确UI可见。
    }
    app.insert_resource(UiStack {
        partition: vec![0..stack.len()], // 单个相机及UI根。
        uinodes: stack,                  // 子节点在父节点之上，关闭文字在关闭按钮之上。
    });
    app.update(); // 使用真实布局系统计算大小、位置和滚动溢出。
    Fixture { app, host, window } // 返回小型可观测夹具。
}

/// 输入堆栈使用真实Children顺序，不依赖Entity分配顺序。
fn collect_stack(world: &World, entity: Entity, stack: &mut Vec<Entity>) {
    stack.push(entity); // 父节点先进入堆栈。
    if let Some(children) = world.get::<Children>(entity) {
        for child in children.iter() {
            collect_stack(world, child, stack); // 子节点随后位于视觉上层。
        }
    }
}

/// 返回生产按钮中携带指定动作的唯一实体。
fn button(app: &mut App, action: Action) -> Entity {
    app.world_mut()
        .query::<(Entity, &Action)>()
        .iter(app.world())
        .find(|(_, value)| **value == action)
        .unwrap()
        .0 // 缺失或错误分派会直接失败。
}

/// 读取真实布局结果，转换回逻辑像素后比较不同缩放倍率。
fn rect(world: &World, entity: Entity) -> Rect {
    let node = world.get::<ComputedNode>(entity).unwrap(); // 不手工填写ComputedNode。
    let center = world
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .affine()
        .translation; // 与命中系统使用相同变换。
    Rect::from_center_size(
        center * node.inverse_scale_factor,
        node.size() * node.inverse_scale_factor,
    ) // 仅转换一次缩放。
}

#[test] // 窗口及全屏的物理尺寸与UI倍率不能改变文字和关闭键的相对布局。
fn compact_tabs_follow_text_width_at_window_and_fullscreen_scales() {
    for (width, scale) in [(1000, 1.0), (2160, 2.0), (3840, 3.0)] {
        let mut test = fixture(
            width,
            scale,
            &[
                ("普通草", 28.5, false),
                ("桦树夏季长名称预设*", 145.0, false),
            ],
        ); // 保留完整长名及脏标记。
        let map = button(&mut test.app, Action::Map); // 永久标签也应自然收缩。
        let map_width = 38.0 + TITLE_PADDING * 2.0; // 固定文字测量加左右留白，不含额外边框。
        assert!((rect(test.app.world(), map).width() - map_width).abs() <= 1.0);
        let mut widths = Vec::new(); // 记录两个完整标签的真实逻辑宽度。
        for index in 0..2 {
            let title = button(&mut test.app, Action::Activate(index)); // 单独的标题命中区。
            let close = button(&mut test.app, Action::Close(index)); // 同级小关闭命中区。
            let world = test.app.world(); // 以下只读布局结果。
            let frame = world.get::<ChildOf>(title).unwrap().parent(); // 非按钮共用边框。
            assert_eq!(world.get::<ChildOf>(close).unwrap().parent(), frame); // 关闭键不能嵌套在标题Button内。
            assert!(world.get::<Button>(frame).is_none()); // 点击不会再触发外层第三个动作。
            assert_eq!(
                world.get::<Node>(frame).unwrap().border,
                UiRect::default()
            ); // 活动地图、文件和预设均不恢复亮色底线。
            let fill = world.get::<BackgroundColor>(frame).unwrap();
            assert_eq!(world.get::<BackgroundColor>(title).unwrap(), fill);
            assert_eq!(world.get::<BackgroundColor>(close).unwrap(), fill,
                "普通关闭键与所属标签连成同一背景，不显示常驻独立方块");
            let (outer, title, close) =
                (rect(world, frame), rect(world, title), rect(world, close)); // 同一坐标系比较。
            assert!((outer.height() - TAB_HEIGHT).abs() <= 1.0); // 各倍率高度一致。
            assert!((title.center().y - close.center().y).abs() <= 1.0); // 左侧文字与右侧×垂直对齐。
            assert!((close.width() - CLOSE_SIZE).abs() <= 1.0); // 关闭键不再占独立34px宽框。
            assert!(close.min.x >= title.max.x && close.max.x <= outer.max.x); // 关闭键位于同框右侧且不覆盖文字。
            widths.push(outer.width()); // 长短标题只在文字宽度上有差异。
        }
        assert!((widths[1] - widths[0] - (145.0 - 28.5)).abs() <= 1.0); // 真实测量决定宽度，而非固定值或平分剩余空间。
        assert!(widths[0] < 70.0); // 普通草包含关闭键也远小于原168px。
    }
}

#[test] // 二次关闭确认必须可见且不让普通关闭键回到大按钮尺寸。
fn confirmation_expands_only_its_close_target() {
    let mut test = fixture(
        1000,
        1.0,
        &[("普通草", 28.5, false), ("普通草", 28.5, true)],
    ); // 仅确认状态不同。
    let normal = button(&mut test.app, Action::Close(0)); // 紧凑普通×。
    let confirm = button(&mut test.app, Action::Close(1)); // 同一个标签体系的“确认×”。
    assert!(rect(test.app.world(), confirm).width() > rect(test.app.world(), normal).width()); // Auto让文案完整展开。
    for entity in [normal, confirm] {
        assert_eq!(
            test.app.world().get::<Node>(entity).unwrap().border,
            UiRect::default()
        ); // 两个状态均不产生额外大边框。
        assert_eq!(
            test.app.world().get::<FocusPolicy>(entity),
            Some(&FocusPolicy::Block)
        ); // 小按钮仍有独立命中权。
    }
}

#[test] // 过多标签只能由外层横滚，不允许内层裁切造成末尾×永久不可访问。
fn overflowing_tabs_keep_natural_width_and_scroll_last_close_into_view() {
    let tabs = [("桦树夏季长名称预设*", 145.0, false); 6]; // 固定小样本即可触发真实溢出。
    let mut test = fixture(240, 1.0, &tabs); // 窄窗口不能容纳全部标签。
    let last_close = button(&mut test.app, Action::Close(5)); // 最后一个关闭目标。
    let host_rect = rect(test.app.world(), test.host); // 不随滚动移动的外层视口。
    assert!(rect(test.app.world(), last_close).min.x > host_rect.max.x); // 初始确实处于可见范围之外。
    let host_node = test.app.world().get::<ComputedNode>(test.host).unwrap(); // 读取真实布局计算的滚动范围。
    let overflow = host_node.content_size().x - host_node.size().x; // 不能由内层截断为零。
    assert!(overflow > 500.0); // 六个自然宽度标签的总宽确实被外层观察到。
    test.app
        .world_mut()
        .get_mut::<ScrollPosition>(test.host)
        .unwrap()
        .x = overflow; // 滚到最右端。
    test.app.update(); // 使用真实布局系统应用滚动偏移。
    let last_rect = rect(test.app.world(), last_close); // 重新读取真正的鼠标命中位置。
    assert!(last_rect.min.x >= host_rect.min.x && last_rect.max.x <= host_rect.max.x); // 最后一个×完整进入滚动视口。
}

#[test] // 通过真实鼠标命中验证点击×不会同时激活对应标题或相邻标签。
fn close_and_title_receive_separate_mouse_presses() {
    let mut test = fixture(
        1000,
        1.0,
        &[("普通草", 28.5, false), ("普通草", 28.5, false)],
    ); // 包含相邻标签，覆盖边界混淆。
    for action in [Action::Close(0), Action::Activate(0), Action::Close(1)] {
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left); // 松开上一次按键。
        test.app
            .world_mut()
            .run_system_once(ui_focus_system)
            .unwrap(); // 真实系统清除Pressed状态。
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear(); // 消费上一帧边沿。
        let target = button(&mut test.app, action); // 当前目标来自生产层级中的动作组件。
        let position = rect(test.app.world(), target).center(); // 单倍缩放时逻辑与物理坐标相同。
        test.app
            .world_mut()
            .get_mut::<Window>(test.window)
            .unwrap()
            .set_cursor_position(Some(position)); // 点击字形所在中心。
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left); // 发送独立按下边沿。
        test.app
            .world_mut()
            .run_system_once(ui_focus_system)
            .unwrap(); // 不手工写入Interaction伪造命中。
        let pressed: Vec<_> = test
            .app
            .world_mut()
            .query::<(&Action, &Interaction)>()
            .iter(test.app.world())
            .filter_map(|(action, interaction)| {
                (*interaction == Interaction::Pressed).then_some(*action)
            })
            .collect(); // 收集全部真正收到按下的按钮。
        assert_eq!(pressed, vec![action]); // 每次只允许一个精确动作。
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear(); // 对齐InputPlugin帧边沿清理，下一次松开不能重放旧的just_pressed。
    }
}

/// 生产标签经过真实命中，再由共享协议发布释放脉冲；覆盖不同按钮间的错误松手。
#[test]
fn actual_tab_hit_testing_only_activates_on_original_button_release() {
    let mut test = fixture(1000, 1.0, &[("普通草", 28.5, false)]);
    // 持久系统保留Bevy命中的Local状态，快点后的Pressed清理也必须经过真实下一帧。
    test.app.add_systems(PreUpdate, ui_focus_system.in_set(bevy::ui::UiSystems::Focus));
    let title = button(&mut test.app, Action::Activate(0));
    let close = button(&mut test.app, Action::Close(0));
    let frame = test.app.world().get::<ChildOf>(title).unwrap().parent();
    let fill = test.app.world().get::<BackgroundColor>(frame).unwrap().0;
    assert_eq!(fill, colors::SELECT_BG, "活动标签使用独立选中语义，不能借瞬时Pressed色冒充");
    let step = |test: &mut Fixture, entity: Entity, press: bool, release: bool| {
        let position = rect(test.app.world(), entity).center();
        test.app.world_mut().get_mut::<Window>(test.window).unwrap().set_cursor_position(Some(position));
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        if press { test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left); }
        if release { test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left); }
        test.app.update();
    };
    let clicked = |test: &Fixture, entity| test.app.world().get::<crate::EditorButtonClick>(entity).unwrap().is_clicked();
    step(&mut test, close, false, false);
    let hovered = test.app.world().get::<BackgroundColor>(close).unwrap().0;
    assert_ne!(hovered, fill, "关闭键普通态融合，但悬停仍须有反馈");
    step(&mut test, title, true, false);
    assert!(!clicked(&test, title));
    step(&mut test, title, false, false);
    assert!(!clicked(&test, title));
    step(&mut test, title, false, true);
    assert!(clicked(&test, title) && !clicked(&test, close));
    step(&mut test, close, true, false);
    let pressed = test.app.world().get::<BackgroundColor>(close).unwrap().0;
    assert_ne!(pressed, hovered, "关闭键按住反馈区别于悬停，尚未执行关闭");
    assert!(!clicked(&test, close));
    step(&mut test, title, false, true);
    assert!(!clicked(&test, close) && !clicked(&test, title));
    assert_eq!(test.app.world().get::<BackgroundColor>(close).unwrap().0, fill,
        "移出取消后关闭键恢复整标签基色，不能留下独立底块");
    step(&mut test, close, true, true);
    assert!(clicked(&test, close) && !clicked(&test, title));
    step(&mut test, close, false, false);
    assert!(!clicked(&test, close), "快点不能在下一帧重复触发");
    step(&mut test, close, true, false);
    step(&mut test, title, false, false);
    // Bevy拖出会保留Pressed；移回原按钮后合法松开，仍应像传统按钮一样确认。
    assert_eq!(*test.app.world().get::<Interaction>(close).unwrap(), Interaction::Pressed);
    step(&mut test, close, false, true);
    assert!(clicked(&test, close) && !clicked(&test, title));
}
