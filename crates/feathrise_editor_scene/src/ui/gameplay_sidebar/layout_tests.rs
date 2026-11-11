//! 真实中文字体、低高度固定导航和公共空气墙字段布局回归，不等同于原生窗口视觉验收。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::*;
use bevy::text::LineHeight;

#[test]
fn wall_objects_use_centered_small_buttons_and_fixed_single_line_name_field() {
    use crate::ui::theme as colors;
    let (mut app, _) = page(false);
    press(&mut app, "空气墙");
    let before = author(&app);
    let labels = [
        "上一页",
        "下一页",
        "新建内部墙",
        "新建外框",
        "复制所选",
        "删除所选",
        "切换碰撞",
        "切换辅助线",
        "应用名称",
    ];
    let world = app.world_mut();
    let buttons = labels.map(|label| {
        world.query_filtered::<(Entity, &Children), (With<UiInteraction>, Without<SceneEditorActionButton>)>()
            .iter(world).find_map(|(entity, children)| {
                children.iter().any(|child| world.get::<Text>(child).is_some_and(|text| text.as_str() == label)).then_some(entity)
            }).unwrap_or_else(|| panic!("缺少墙体操作：{label}"))
    });
    for entity in buttons {
        let node = world.get::<Node>(entity).unwrap();
        assert_eq!(node.height, px(26));
        assert_eq!(node.border_radius, BorderRadius::all(px(3)));
        assert_eq!(node.align_items, AlignItems::Center);
        assert_eq!(node.justify_content, JustifyContent::Center);
        assert_eq!(node.flex_shrink, 0.0);
        assert_eq!(
            world.get::<BackgroundColor>(entity).unwrap().0,
            colors::BUTTON_DEV_SMALL_NORMAL
        );
        assert_eq!(
            *world.get::<BorderColor>(entity).unwrap(),
            BorderColor::all(colors::COPPER_BORDER_WEAK)
        );
        assert!(world.get::<InteractionButton>(entity).is_some()); // 普通墙体动作必须接入共享释放点击，布局仍由原面板负责。
    }
    let name = world
        .query_filtered::<Entity, (
            With<bevy::text::EditableText>,
            With<crate::ui::text_input::EditorTextInput>,
            Without<SceneEditorPortalNameInputText>,
        )>()
        .single(world)
        .unwrap();
    assert!(world.get::<InteractionButton>(name).is_none()); // 名称框继续使用官方文本焦点，不成为普通动作按钮。
    click(&mut app, name);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(name));
    assert_eq!(
        *app.world().get::<BorderColor>(name).unwrap(),
        BorderColor::all(colors::INPUT_ACTIVE_BORDER)
    );
    let long_name = "很长的中文空气墙名称".repeat(6);
    for value in ["", "普通空气墙", long_name.as_str()] {
        app.world_mut()
            .entity_mut(name)
            .insert(crate::ui::text_input::single_line_input(value, 64));
        for (scale, dpi) in [(0.6, 1.0), (1.0, 1.0), (1.5, 1.0), (1.25, 2.0)] {
            app.world_mut().resource_mut::<UiScale>().0 = scale;
            let world = app.world_mut();
            for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
            }
            settle(&mut app);
            let world = app.world_mut();
            let node = world.get::<Node>(name).unwrap();
            assert_eq!(node.height, px(26));
            assert_eq!(node.padding.top, px(5));
            assert_eq!(node.padding.bottom, px(5));
            assert_eq!(
                *world.get::<LineHeight>(name).unwrap(),
                LineHeight::Px(14.0)
            );
            assert_eq!(
                world.get::<TextFont>(name).unwrap().font_size,
                bevy::text::FontSize::Px(11.0)
            );
            assert!(
                (world.get::<ComputedNode>(name).unwrap().size().y - 26.0 * scale * dpi).abs()
                    <= 1.1
            );
            for entity in buttons {
                let computed = world.get::<ComputedNode>(entity).unwrap();
                if computed.size().x == 0.0 {
                    continue;
                }
                let center = world.get::<UiGlobalTransform>(entity).unwrap().translation;
                for child in world.get::<Children>(entity).unwrap().iter() {
                    let text_center = world.get::<UiGlobalTransform>(child).unwrap().translation;
                    assert!((text_center.x - center.x).abs() <= 1.1);
                    assert!((text_center.y - center.y).abs() <= 1.1);
                }
            }
        }
    }
    assert_eq!(author(&app), before); // 聚焦和排版不隐式提交名称或修改墙体。
    app.world_mut().resource_mut::<InputFocus>().clear();
    app.update();
    assert_eq!(
        *app.world().get::<BorderColor>(name).unwrap(),
        BorderColor::all(colors::INPUT_BORDER)
    );
}

#[test]
fn all_tool_pages_fit_real_cjk_font_widths_scales_and_dpi_without_moving_tabs() {
    let (mut app, host) = page(false);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("gltf/很长的中文目录/用于验证窄栏换行的完整资产文件名称.glb".into());
    app.world_mut().resource_mut::<SceneEditorState>().status =
        "作者操作失败的完整反馈与中文路径需要在窄栏中保持可读。".repeat(12);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().portals.portals[0].label = "用于验证单行裁切的完整中文传送点名称".repeat(4);
        session.portal_revision = session.portal_revision.wrapping_add(1);
    }
    let world = app.world_mut();
    for mut text in world
        .query_filtered::<&mut Text, With<feathrise_editor_ui::NumericInputDisplayText>>()
        .iter_mut(world)
    {
        text.0 = "-10000.000".into(); // 仅布局夹具，不修改作者值或公共输入精度。
    }
    for width in [200.0, 240.0, 320.0, 420.0] {
        for scale in [0.6, 1.0, 1.5] {
            for dpi in [1.0, 2.0] {
                app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                let world = app.world_mut();
                for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                    camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                }
                for target in [
                    GameplaySidebarPage::Asset,
                    GameplaySidebarPage::Portal,
                    GameplaySidebarPage::Boundary,
                ] {
                    press(&mut app, target.label());
                    let world = app.world_mut();
                    let host_node = world.get::<ComputedNode>(host).unwrap();
                    let host_center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                    let left = host_center - host_node.size().x * 0.5;
                    let right = host_center + host_node.size().x * 0.5;
                    for (node, transform) in world
                        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<UiInteraction>>()
                        .iter(world)
                    {
                        if node.size().x == 0.0 {
                            continue;
                        }
                        assert!(
                            transform.translation.x - node.size().x * 0.5 >= left - 1.1,
                            "控件左溢出{target:?}/{width}/{scale}/{dpi}"
                        );
                        assert!(
                            transform.translation.x + node.size().x * 0.5 <= right + 1.1,
                            "控件右溢出{target:?}/{width}/{scale}/{dpi}"
                        );
                    }
                    for (node, layout) in world
                        .query_filtered::<(&ComputedNode, &bevy::text::TextLayoutInfo), Or<(
                            With<GameplaySidebarFeedbackText>,
                            With<SceneEditorPortalTargetText>,
                            With<SceneEditorBoundaryText>,
                        )>>()
                        .iter(world)
                    {
                        if node.size().x == 0.0 {
                            continue;
                        }
                        assert!(
                            layout.size.x <= node.size().x + 1.1,
                            "反馈裁字{target:?}/{width}/{scale}/{dpi}"
                        );
                        assert!(
                            layout.size.y <= node.size().y + 1.1,
                            "反馈高度不足{target:?}/{width}/{scale}/{dpi}"
                        );
                    }
                    for (node, transform, children) in world.query_filtered::<(&ComputedNode, &UiGlobalTransform, &Children), With<NumericInputRoot>>().iter(world) {
                        if node.size().x == 0.0 { continue; }
                        for child in children.iter() {
                            if world.get::<Text>(child).is_none() { continue; }
                            let child_node = world.get::<ComputedNode>(child).unwrap();
                            let child_center = world.get::<UiGlobalTransform>(child).unwrap().translation.x;
                            assert!(child_center - child_node.size().x * 0.5 >= transform.translation.x - node.size().x * 0.5 - 1.1, "字段左溢出{width}/{scale}/{dpi}");
                            assert!(child_center + child_node.size().x * 0.5 <= transform.translation.x + node.size().x * 0.5 + 1.1, "字段右溢出{width}/{scale}/{dpi}");
                        }
                    }
                    for (node, children) in world.query_filtered::<(&ComputedNode, &Children), Or<(
                        With<crate::components::SceneEditorCurrentPortalRow>, With<crate::components::SceneEditorTargetPortalRow>,
                    )>>().iter(world) {
                        if node.size().x == 0.0 { continue; }
                        assert!((node.size().y - 24.0 * scale * dpi).abs() <= 1.1,
                            "传送行高度偏离24像素：{width}/{scale}/{dpi}");
                        for child in children.iter() {
                            let Some(layout) = world.get::<bevy::text::TextLayoutInfo>(child) else { continue; };
                            assert!(layout.size.y <= node.size().y + 1.1,
                                "传送长名换行挤压下一项：{width}/{scale}/{dpi}");
                            assert_eq!(world.get::<TextLayout>(child).unwrap().linebreak, LineBreak::NoWrap);
                        }
                    }
                    let scroll = world
                        .query_filtered::<Entity, With<GameplaySidebarScroll>>()
                        .single(world)
                        .unwrap();
                    let before = world
                        .query::<(&GameplaySidebarTab, &UiGlobalTransform)>()
                        .iter(world)
                        .map(|(_, transform)| transform.translation.y)
                        .collect::<Vec<_>>();
                    world.get_mut::<ScrollPosition>(scroll).unwrap().y = 100.0;
                    app.update();
                    let world = app.world_mut();
                    assert_eq!(
                        world
                            .query::<(&GameplaySidebarTab, &UiGlobalTransform)>()
                            .iter(world)
                            .map(|(_, transform)| transform.translation.y)
                            .collect::<Vec<_>>(),
                        before
                    );
                }
            }
        }
    }
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<NumericInputRoot>>()
            .iter(world)
            .count(),
        3
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorPortalNameInputText>>()
            .iter(world)
            .count(),
        1
    );
}

#[test]
fn complete_left_sidebar_keeps_gameplay_navigation_fixed_in_low_windows() {
    let (mut app, host) = page(true);
    app.world_mut().get_mut::<Node>(host).unwrap().width = px(200);
    press(&mut app, "传送点");
    app.world_mut().resource_mut::<SceneEditorState>().status =
        "传送点目标资产加载失败，请检查完整场景与到达点引用。".repeat(16);
    settle(&mut app);
    let world = app.world_mut();
    let scroll = world
        .query_filtered::<Entity, With<GameplaySidebarScroll>>()
        .single(world)
        .unwrap();
    let node = world.get::<ComputedNode>(scroll).unwrap();
    assert!(node.size().y > 0.0 && node.content_size().y > node.size().y);
    let before = world
        .query::<(&GameplaySidebarTab, &UiGlobalTransform)>()
        .iter(world)
        .map(|(_, transform)| transform.translation.y)
        .collect::<Vec<_>>();
    world.get_mut::<ScrollPosition>(scroll).unwrap().y = 180.0;
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<(&GameplaySidebarTab, &UiGlobalTransform)>()
            .iter(world)
            .map(|(_, transform)| transform.translation.y)
            .collect::<Vec<_>>(),
        before
    );
    let root = world
        .query_filtered::<Entity, With<GameplaySidebarRoot>>()
        .single(world)
        .unwrap();
    let outer = world.get::<ChildOf>(root).unwrap().parent();
    assert!(world.get::<SceneEditorRightPanelScroll>(outer).is_none()); // 实际玩法外壳不再生成第二个滚动宿主。
}

#[test]
fn feedback_observes_real_waits_and_stable_frames_do_not_rewrite_nodes() {
    #[derive(Resource, Default)]
    struct Writes(usize);
    fn probe(
        mut writes: ResMut<Writes>,
        texts: Query<(), (With<GameplaySidebarFeedbackText>, Changed<Text>)>,
        panels: Query<(), (With<GameplaySidebarPanel>, Changed<Node>)>,
        tabs: Query<
            (),
            (
                With<GameplaySidebarTab>,
                Or<(Changed<BackgroundColor>, Changed<BorderColor>)>,
            ),
        >,
    ) {
        writes.0 = texts.iter().count() + panels.iter().count() + tabs.iter().count();
    }
    let (mut app, _) = page(false);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    app.update();
    assert!(feedback(&mut app, GameplaySidebarFeedbackKind::Availability).contains("正在保存"));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = false;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = None;
    app.update();
    assert!(feedback(&mut app, GameplaySidebarFeedbackKind::Availability).contains("等待当前场景"));
    app.init_resource::<Writes>();
    app.add_systems(Update, probe.after(SceneEditorSystemSet::Presentation));
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Writes>().0, 0);
}
