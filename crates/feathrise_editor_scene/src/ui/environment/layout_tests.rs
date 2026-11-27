//! 原生中文字体下验证环境页缩放、窄栏、方案参数轨道及原输入缓冲；不替代实机视觉验收。

use feathrise_editor_ui::InteractionButton;
use super::*;

#[test]
fn day_and_night_rgb_channels_share_one_row_without_replacing_numeric_roots() {
    let (mut app, _) = page();
    for scope in [CelestialNumericScope::Global, CelestialNumericScope::Scene] {
        app.world_mut().resource_mut::<EnvironmentPageState>().page = match scope {
            CelestialNumericScope::Global => EnvironmentPage::Library,
            CelestialNumericScope::Scene => EnvironmentPage::Scene,
        };
        settle(&mut app);
        let world = app.world_mut();
        for fields in [[CelestialValueField::DayRed, CelestialValueField::DayGreen, CelestialValueField::DayBlue],
            [CelestialValueField::NightRed, CelestialValueField::NightGreen, CelestialValueField::NightBlue]] {
            let mut cells = world.query::<(&CelestialNumericInputSlot, &Children)>().iter(world)
                .filter(|(input, _)| input.0.scope == scope && fields.contains(&input.0.field))
                .map(|(_, children)| {
                    let root = children.iter().find(|child| world.get::<NumericInputRoot>(*child).is_some()).unwrap();
                    (world.get::<ComputedNode>(root).unwrap().size(), world.get::<UiGlobalTransform>(root).unwrap().translation)
                }).collect::<Vec<_>>();
            assert_eq!(cells.len(), 3);
            cells.sort_by(|left, right| left.1.x.total_cmp(&right.1.x));
            for pair in cells.windows(2) {
                assert!((pair[0].1.y - pair[1].1.y).abs() <= 1.0);
                assert!(pair[0].1.x + pair[0].0.x / 2.0 <= pair[1].1.x - pair[1].0.x / 2.0 + 1.0);
            }
        }
    }
    let world = app.world_mut();
    assert_eq!(world.query_filtered::<Entity, With<NumericInputRoot>>().iter(world).count(), 28);
}

#[test]
fn environment_reflow_preserves_numeric_roots_and_active_buffer() {
    let (mut app, host) = page();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .environment
        .skybox_path = Some(format!(
        "skyboxes/{}天空Cube.ktx2",
        "用于检查长中文来源路径的目录/".repeat(10)
    ));
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .load_error = Some("全局环境来源错误：请检查完整中文资源路径。".repeat(12));
    app.world_mut().resource_mut::<EnvironmentPageState>().page = EnvironmentPage::Library;
    settle(&mut app);
    let world = app.world_mut();
    let fields = world.query_filtered::<(Entity, &NumericInputConfig, &NumericInputValue), With<NumericInputRoot>>()
        .iter(world).map(|(entity, config, value)| (entity, *config, *value)).collect::<Vec<_>>();
    assert_eq!(fields.len(), 28);
    let slot = world
        .query::<(Entity, &CelestialNumericInputSlot)>()
        .iter(world)
        .find(|(_, marker)| {
            marker.0.scope == CelestialNumericScope::Global
                && marker.0.field == CelestialValueField::DayIlluminance
        })
        .unwrap()
        .0;
    let active = world.get::<Children>(slot).unwrap().iter().next().unwrap();
    assert!(world.get::<NumericInputRoot>(active).is_some());
    world.resource_mut::<ActiveNumericInput>().entity = Some(active);
    let mut edit = world.get_mut::<NumericInputEditState>(active).unwrap();
    edit.editing = true;
    edit.buffer = "12345".into();
    let before = author(&app);
    for width in [260.0, 320.0, 420.0, 560.0] {
        app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
        for scale in [0.6, 1.0, 1.5] {
            app.world_mut().resource_mut::<UiScale>().0 = scale;
            for dpi in [1.0, 2.0] {
                let world = app.world_mut();
                for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                    camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                }
                settle(&mut app);
                let world = app.world_mut();
                let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
                assert!((host_width - width * scale * dpi).abs() <= 1.1);
                let center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                let left = center - host_width / 2.0;
                let right = center + host_width / 2.0;
                for (entity, config, value) in &fields {
                    assert_eq!(
                        world.get::<NumericInputConfig>(*entity).unwrap().0,
                        config.0
                    );
                    assert_eq!(world.get::<NumericInputValue>(*entity).unwrap(), value);
                    let node = world.get::<Node>(*entity).unwrap();
                    assert_eq!(node.width, percent(100));
                    assert_eq!(node.min_width, px(0));
                    assert_eq!(node.height, px(24));
                    let computed = world.get::<ComputedNode>(*entity).unwrap();
                    let slot = world.get::<ChildOf>(*entity).unwrap().parent();
                    let celestial = world.get::<CelestialNumericInputSlot>(slot);
                    if world.get::<GlobalEnvironmentNumericInputSlot>(slot).is_none()
                        && celestial.is_none_or(|input| input.0.scope == CelestialNumericScope::Scene)
                    {
                        assert_eq!(computed.size(), Vec2::ZERO); // 页签只隐藏场景根，保留同一实体和输入快照。
                        continue;
                    }
                    let rgb = celestial.is_some_and(|input| matches!(input.0.field,
                        CelestialValueField::DayRed | CelestialValueField::DayGreen | CelestialValueField::DayBlue
                        | CelestialValueField::NightRed | CelestialValueField::NightGreen | CelestialValueField::NightBlue));
                    assert!(computed.size().x > if rgb { 40.0 } else { 180.0 } * scale * dpi);
                    assert!(computed.size().x <= host_width + 1.1);
                }
                for (entity, node, layout) in world
                    .query::<(Entity, &ComputedNode, &TextLayout)>()
                    .iter(world)
                {
                    if node.size().x == 0.0
                        || layout.linebreak != bevy::text::LineBreak::AnyCharacter
                    {
                        continue;
                    }
                    let center = world
                        .get::<UiGlobalTransform>(entity)
                        .unwrap()
                        .translation
                        .x;
                    assert!(center - node.size().x / 2.0 >= left - 1.1);
                    assert!(center + node.size().x / 2.0 <= right + 1.1);
                }
                let tracks = world
                    .query::<(Entity, &bevy::ui::RelativeCursorPosition, &Node)>()
                    .iter(world)
                    .filter(|(_, _, node)| node.height == px(SliderVisual::default().track_height))
                    .map(|(entity, _, _)| entity)
                    .collect::<Vec<_>>();
                assert_eq!(tracks.len(), 5);
                for entity in tracks {
                    assert_eq!(world.get::<Node>(entity).unwrap().width, percent(100));
                    let computed = world.get::<ComputedNode>(entity).unwrap();
                    assert!(computed.size().x > 180.0 * scale * dpi);
                    assert!(computed.size().x <= host_width + 1.1);
                    for child in world.get::<Children>(entity).unwrap().iter() {
                        let child_node = world.get::<Node>(child).unwrap();
                        assert!(matches!(child_node.left, Val::Percent(_)));
                        assert!(matches!(child_node.width, Val::Percent(_)));
                    }
                }
                for node in world.query_filtered::<&Node, With<InteractionButton>>().iter(world) {
                    if node.height == px(26) {
                        assert_eq!(node.align_items, AlignItems::Center);
                    }
                }
                assert_eq!(world.resource::<ActiveNumericInput>().entity, Some(active));
                assert_eq!(
                    world.get::<NumericInputEditState>(active).unwrap().buffer,
                    "12345"
                );
                assert_eq!(
                    world
                        .query_filtered::<Entity, With<NumericInputRoot>>()
                        .iter(world)
                        .count(),
                    28
                );
            }
        }
    }
    assert_eq!(author(&app), before);
}

#[test]
fn environment_scope_order_and_collapsing_keep_original_entities_in_low_window() {
    let (mut app, host) = page();
    let world = app.world_mut();
    let mut host_node = world.get_mut::<Node>(host).unwrap();
    host_node.height = px(160);
    host_node.width = px(260);
    let shared_id = world.resource::<GlobalEnvironmentAuthoringState>()
        .draft.library.default_profile_id.clone();
    let mut session = world.resource_mut::<EditorAuthoringSession>();
    let environment = &mut session.draft.as_mut().unwrap().manifest.environment;
    environment.source = feathrise_scene_content::SceneEnvironmentSource::Shared(shared_id);
    settle(&mut app);
    let world = app.world_mut();
    let debug = world.query_filtered::<Entity, With<WorldEnvironmentControlsSlot>>()
        .single(world).unwrap();
    let summary = world.query::<(&EnvironmentSidebarText, &ComputedNode, &TextLayout, &Text)>()
        .iter(world).find(|(marker, _, _, _)| marker.0 == EnvironmentTextField::CurrentValues).unwrap();
    assert_eq!(summary.2.linebreak, bevy::text::LineBreak::AnyCharacter);
    assert!(summary.1.size().x > 0.0 && summary.1.size().x <= 261.0);
    assert!(summary.1.size().y >= feathrise_editor_ui::metrics::FONT_HINT * 6.0);
    assert!(summary.3.as_str().contains("晴天草尖基准")); // 窄栏的继承摘要自然增高，仍由原单滚动宿主访问。
    let y = |world: &mut World, label: &str| {
        let entity = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .find(|(_, text)| text.as_str() == label)
            .unwrap()
            .0;
        world
            .get::<UiGlobalTransform>(entity)
            .unwrap()
            .translation
            .y
    };
    assert!(y(world, "场景环境") < y(world, "环境来源"));
    assert_eq!(world.get::<ComputedNode>(debug).unwrap().size(), Vec2::ZERO);
    assert_eq!(scene_display(&mut app), Display::None); // 引用共享方案时也隐藏场景外观编辑控件。
    app.world_mut().resource_mut::<EnvironmentPageState>().page = EnvironmentPage::Library;
    settle(&mut app);
    let world = app.world_mut();
    assert!(y(world, "全局环境库") < y(world, "全局方案"));
    assert!(y(world, "全局方案") < y(world, "时间、天气与草尖摆幅"));
    assert!(world.get::<ComputedNode>(debug).unwrap().size().y > 0.0);
    let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
    let visible_tracks = world.query::<(&ComputedNode, &bevy::ui::RelativeCursorPosition, &Node)>()
        .iter(world)
        .filter(|(computed, _, node)| node.height == px(SliderVisual::default().track_height)
            && (180.0..=host_width + 1.1).contains(&computed.size().x))
        .count();
    assert_eq!(visible_tracks, 5);

    app.world_mut().resource_mut::<EnvironmentPageState>().page = EnvironmentPage::Library;
    settle(&mut app);
    let world = app.world_mut();
    let original = world
        .query_filtered::<Entity, With<NumericInputRoot>>()
        .iter(world)
        .collect::<Vec<_>>();
    let root = world
        .query_filtered::<&Node, With<SceneEditorRightPanelScroll>>()
        .single(world)
        .unwrap();
    assert_eq!(root.overflow.y, OverflowAxis::Scroll);
    assert_eq!(root.min_height, px(0));
    let before = author(&app);
    for expanded in [false, true] {
        let mut sections = app.world_mut().resource_mut::<CelestialSectionState>();
        sections.global_expanded = expanded;
        sections.scene_expanded = expanded;
        settle(&mut app);
        let world = app.world_mut();
        for (_, node) in world.query::<(&CelestialSectionBody, &Node)>().iter(world) {
            assert_eq!(
                node.display,
                if expanded {
                    Display::Flex
                } else {
                    Display::None
                }
            );
        }
        for entity in &original {
            assert!(world.get::<NumericInputRoot>(*entity).is_some());
        }
        assert_eq!(
            world
                .query_filtered::<Entity, With<NumericInputRoot>>()
                .iter(world)
                .count(),
            28
        );
    }
    assert_eq!(author(&app), before);
}
