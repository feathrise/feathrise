//! 顶栏输入、窄窗原位收纳与布局完成后的物理命中投影；每帧只访问固定控件集合。
use super::{components::*, glyphs, resources::*, scene, types::*};
use bevy::{prelude::*, ui::RelativeCursorPosition, window::PrimaryWindow};
use feathrise_editor_ui::{ButtonColors, EditorButtonClick, colors as c};

pub(super) fn input(world: &mut World) {
    let actions: Vec<_> = world
        .query::<(&ChromeButton, &EditorButtonClick, &InheritedVisibility)>()
        .iter(world)
        .filter_map(|(action, click, visible)| {
            (visible.get() && click.is_clicked()).then_some(*action)
        })
        .collect();
    let (pressed, released, held) = world
        .get_resource::<ButtonInput<MouseButton>>()
        .map(|m| {
            (
                m.get_just_pressed().next().is_some(),
                m.get_just_released().next().is_some(),
                m.get_pressed().next().is_some(),
            )
        })
        .unwrap_or_default();
    let escape = world
        .get_resource::<ButtonInput<KeyCode>>()
        .is_some_and(|k| k.just_pressed(KeyCode::Escape));
    let modal = world
        .get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|s| s.captures_input())
        || crate::project_export::ui::captures_input(world)
        || world.get_resource::<crate::native_plugins::management::PluginManagerState>()
            .is_some_and(|plugins| plugins.captures_input());
    let focused = world.resource::<ChromeState>().focused;
    let was = world.resource::<ChromeState>().menu;
    let popup_over = world
        .query::<(&Popup, &RelativeCursorPosition, &InheritedVisibility)>()
        .iter(world)
        .any(|(popup, p, v)| Some(popup.0) == was && v.get() && p.cursor_over());
    let original_action = world
        .resource::<ChromeParts>()
        .0
        .as_ref()
        .is_some_and(|parts| {
            scene::descendants(world, parts.more, 6)
                .into_iter()
                .any(|entity| {
                    let clicked = world
                        .get::<EditorButtonClick>(entity)
                        .is_some_and(EditorButtonClick::is_clicked)
                        && world.get::<ChromeButton>(entity).is_none();
                    // 配置下拉仍在原父容器中展开，选择完成前保留“更多”容器。
                    clicked
                        && !parts.profile.is_some_and(|profile| {
                            scene::descendants(world, profile, 4).contains(&entity)
                        })
                })
        });
    {
        let mut state = world.resource_mut::<ChromeState>();
        state.captured = was.is_some() && (held || pressed || released);
        if escape || !focused || modal || (pressed && was.is_some() && !popup_over) {
            state.menu = None;
        }
        for action in &actions {
            if let ChromeButton::Toggle(menu) = action {
                if !modal {
                    state.menu = if was == Some(*menu) {
                        None
                    } else {
                        Some(*menu)
                    };
                    state.captured = true;
                }
            } else {
                state.menu = None;
                state.captured = false;
            }
        }
        if original_action {
            // 只放行已经由原释放点击协议确认的原按钮；业务处理仍在后续原系统中执行。
            state.menu = None;
            state.captured = false;
        }
    }
    if was.is_some() && escape {
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear_just_pressed(KeyCode::Escape);
    }
    #[cfg(target_os = "windows")]
    for action in actions {
        if let ChromeButton::Window(action) = action {
            super::native::request(world, action);
        }
    }
}
fn display(world: &mut World, entity: Entity, show: bool) {
    if let Some(mut node) = world.get_mut::<Node>(entity) {
        let next = if show { Display::Flex } else { Display::None };
        if node.display != next {
            node.display = next;
        }
    }
}
fn move_to(world: &mut World, entity: Entity, parent: Entity) {
    if world
        .get::<ChildOf>(entity)
        .is_none_or(|p| p.parent() != parent)
    {
        world.entity_mut(parent).add_child(entity);
    }
}

pub(super) fn present(world: &mut World) {
    let mut parts = world.remove_resource::<ChromeParts>().unwrap();
    let Some(p) = parts.0.as_mut() else {
        world.insert_resource(parts);
        return;
    };
    let ui_scale = world
        // 使用实际采用的倍率，开始页和自动缩放档也需按真实可用逻辑宽度收纳。
        .get_resource::<UiScale>()
        .map_or(1., |s| s.0)
        .max(0.25);
    let width = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map_or(1270., |w| w.width() / ui_scale);
    let compact = width < 1060.;
    let narrow = width < 760.;
    let (previous_compact, previous_narrow, menu, focused, maximized) = {
        let s = world.resource::<ChromeState>();
        (s.compact, s.narrow, s.menu, s.focused, s.maximized)
    };
    let mut relocate = previous_compact != compact || previous_narrow != narrow;
    // 首次进入也按当前尺寸整理；后续只在档位改变时移挂，不在稳定帧重新设置父关系。
    relocate |= world
        .get::<ChildOf>(p.selection)
        .is_some_and(|parent| parent.parent() == p.header);
    move_to(world, p.selection, p.more);
    if relocate {
        for (index, &item) in p.history.iter().enumerate() {
            let in_menu = narrow || compact && index > 0;
            move_to(world, item, if in_menu { p.more } else { p.header });
            scene::menu_style(world, item, in_menu);
        }
        move_to(world, p.settings, p.header);
        scene::menu_style(world, p.settings, false);
        if let Some(modes) = p.modes {
            move_to(world, modes, if narrow { p.more } else { p.header });
        }
        if let Some(profile) = p.profile {
            // 配置始终紧跟运行入口，窄窗也不拆散预览与其构建配置。
            move_to(world, profile, p.header);
        }
        // 布局图标始终留在顶栏；窄窗减少命中区留白，仍完整保留16px图形和原动作。
        for &control in &p.layout_controls {
            move_to(world, control, p.header);
            for button in scene::descendants(world, control, 1) {
                if world.get::<Button>(button).is_some()
                    && let Some(mut node) = world.get_mut::<Node>(button)
                {
                    node.width = px(if compact { 24. } else { 28. });
                }
            }
        }
        if let Some(mut node) = world.get_mut::<Node>(p.spacer) {
            node.min_width = px(if compact { 12. } else { 24. });
        }
        let mut order: Vec<_> = world
            .get::<Children>(p.header)
            .map(|c| {
                c.iter()
                    .filter(|e| {
                        *e != p.spacer
                            && *e != p.run
                            && *e != p.captions
                            && *e != p.more_toggle
                            && !p.layout_controls.contains(e)
                            && *e != p.settings
                            && !p.history.contains(e)
                            && Some(*e) != p.modes
                            && Some(*e) != p.profile
                    })
                    .collect()
            })
            .unwrap_or_default();
        // 明确重建少量顶层顺序，不依赖原挂载系统或自适应切换时的追加顺序。
        order.retain(|e| *e != p.more && *e != p.backdrop);
        for &item in &p.history {
            if world
                .get::<ChildOf>(item)
                .is_some_and(|parent| parent.parent() == p.header)
                && !order.contains(&item)
            {
                order.push(item);
            }
        }
        order.extend([p.settings, p.more_toggle, p.spacer]);
        for item in [
            p.modes,
            Some(p.run),
            p.profile,
        ]
        .into_iter()
        .flatten()
        {
            if world
                .get::<ChildOf>(item)
                .is_some_and(|parent| parent.parent() == p.header)
            {
                order.push(item);
            }
        }
        order.extend(p.layout_controls.iter().copied());
        order.push(p.captions);
        world.entity_mut(p.header).insert_children(0, &order);
    }
    // 左侧更多菜单始终提供原“筛选与操作”，并收纳窄窗的历史与视口选择。
    display(world, p.more_toggle, true);
    display(world, p.more, menu == Some(Menu::More));
    display(world, p.backdrop, menu.is_some());
    {
        // 沿实际入口锚定左侧菜单；复用最终布局与缩放，不能继续在窗口右端弹出。
        let left = (|| {
            let header = world.get::<ComputedNode>(p.header)?;
            let header_transform = world.get::<UiGlobalTransform>(p.header)?;
            let toggle = world.get::<ComputedNode>(p.more_toggle)?;
            let toggle_transform = world.get::<UiGlobalTransform>(p.more_toggle)?;
            let inverse = header.inverse_scale_factor();
            let origin = header_transform.translation.x - header.size().x / 2.;
            let anchor = (toggle_transform.translation.x - toggle.size().x / 2. - origin) * inverse;
            Some(anchor.clamp(0., (header.size().x * inverse - 210.).max(0.)))
        })();
        if let Some(mut node) = world.get_mut::<Node>(p.more) {
            node.left = px(left.unwrap_or(6.));
            node.right = Val::Auto;
        }
    }
    if let Some(project) = p.project {
        let locale = world.get_resource::<feathrise_editor_ui::localization::EditorLocale>()
            .copied().unwrap_or_default();
        let name = world
            .get_resource::<crate::workspace_ui::WorkspaceModel>()
            .and_then(|m| m.project.as_ref())
            .map_or_else(|| locale.text("label.project.2"), |p| p.name.as_str());
        let value = format!("{name} ▼");
        for child in scene::descendants(world, project, 1) {
            if let Some(mut text) = world.get_mut::<Text>(child) {
                if text.as_str() != value {
                    text.0.clone_from(&value);
                }
            }
        }
        if let Some(mut node) = world.get_mut::<Node>(project) {
            node.max_width = px(if narrow { 110. } else { 180. });
        }
    }
    let icons: Vec<_> = world
        .query::<(Entity, &Icon, &ChildOf)>()
        .iter(world)
        .filter_map(|(e, i, parent)| {
            matches!(
                world.get::<ChromeButton>(parent.parent()),
                Some(ChromeButton::Window(WindowAction::Maximize))
            )
            .then_some((e, i.0))
        })
        .collect();
    for (entity, current) in icons {
        let wanted = if maximized {
            Glyph::Restore
        } else {
            Glyph::Maximize
        };
        if current != wanted {
            glyphs::draw(world, entity, wanted);
        }
    }
    let color = if focused {
        c::TEXT_PRIMARY
    } else {
        c::TEXT_MUTED
    };
    let strokes: Vec<_> = world
        .query_filtered::<Entity, With<IconStroke>>()
        .iter(world)
        .collect();
    for entity in strokes {
        if let Some(mut background) = world.get_mut::<BackgroundColor>(entity)
            && background.0 != color
        {
            background.0 = color;
        }
    }
    let close_buttons: Vec<_> = world
        .query::<(Entity, &ChromeButton)>()
        .iter(world)
        .filter_map(|(e, a)| matches!(a, ChromeButton::Window(WindowAction::Close)).then_some(e))
        .collect();
    for entity in close_buttons {
        world.entity_mut(entity).insert(ButtonColors::new(
            Color::NONE,
            Color::srgb_u8(196, 43, 54),
            Color::srgb_u8(160, 28, 38),
        ));
    }
    {
        let mut state = world.resource_mut::<ChromeState>();
        state.compact = compact;
        state.narrow = narrow;
    }
    world.insert_resource(parts);
}

pub(super) fn measure(world: &mut World) {
    // 预览使用独立原生窗口，作者始终测量原顶栏和原三键，不迁移或复制控件。
    super::preview::measure(world);
}
