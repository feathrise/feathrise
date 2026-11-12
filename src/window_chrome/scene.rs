//! 原位重排工作台顶栏；布局图标与窄窗更多菜单都复用原按钮，不能复制动作或触发脉冲。
use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, glyphs, resources::*, types::*};
use bevy::{
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_scene::workbench::{WorkbenchHeader, WorkbenchHeaderItem as Item};
use feathrise_editor_ui::{ButtonColors, colors as c};

pub(super) fn button(world: &mut World, action: ChromeButton, glyph: Glyph, name: &str) -> Entity {
    let caption = matches!(action, ChromeButton::Window(_));
    let entity = world
        .spawn((
            InteractionButton,
            action,
            Name::new(name.to_owned()),
            RelativeCursorPosition::default(),
            ButtonColors::new(
                Color::NONE,
                c::BUTTON_DEV_SMALL_HOVERED,
                c::BUTTON_DEV_SMALL_PRESSED,
            ),
            Node {
                width: px(if caption { CAPTION_WIDTH } else { 28. }),
                height: px(if caption { HEIGHT - 1. } else { 26. }),
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(if caption { 0. } else { 3. })),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .id();
    glyphs::spawn(world, entity, glyph);
    let font = feathrise_editor_core::editor_font(
        world.resource::<AssetServer>(),
        feathrise_editor_core::FONT_PATH,
    );
    let label = world
        .spawn((
            feathrise_editor_ui::localization::editor_label(name),
            TextFont {
                font: font.into(),
                font_size: 12.0.into(),
                ..default()
            },
            TextColor(c::TEXT_PRIMARY),
            Node {
                display: Display::None,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    world.entity_mut(entity).add_child(label);
    entity
}
fn popup(world: &mut World, menu: Menu) -> Entity {
    world
        .spawn((
            Popup(menu),
            RelativeCursorPosition::default(),
            UiInteraction::None,
            FocusPolicy::Block,
            GlobalZIndex(4101),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                right: px(134),
                top: px(HEIGHT),
                width: px(210),
                max_width: Val::Vw(94.),
                padding: UiRect::all(px(5)),
                row_gap: px(3),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(c::PANEL_BG_STRONG),
            BorderColor::all(c::COPPER_BORDER_WEAK),
        ))
        .id()
}
pub(super) fn descendants(world: &World, root: Entity, depth: usize) -> Vec<Entity> {
    let mut result = Vec::new();
    let mut pending = vec![(root, 0)];
    while let Some((entity, level)) = pending.pop() {
        result.push(entity);
        if level < depth
            && let Some(children) = world.get::<Children>(entity)
        {
            pending.extend(children.iter().map(|e| (e, level + 1)));
        }
    }
    result
}
pub(super) fn flatten_button(world: &mut World, root: Entity, glyph: Option<Glyph>) {
    let entities = descendants(world, root, 3);
    let Some(button) = entities
        .iter()
        .copied()
        .find(|e| world.get::<InteractionButton>(*e).is_some())
    else {
        return;
    };
    if let Some(mut node) = world.get_mut::<Node>(button) {
        node.border = UiRect::ZERO;
        node.height = px(26);
        if glyph.is_some() {
            node.width = px(28);
            node.padding = UiRect::ZERO;
        }
    }
    world.entity_mut(button).insert(ButtonColors::new(
        Color::NONE,
        c::BUTTON_DEV_SMALL_HOVERED,
        c::BUTTON_DEV_SMALL_PRESSED,
    ));
    if let Some(glyph) = glyph {
        for child in entities {
            if world.get::<Text>(child).is_some()
                && let Some(mut node) = world.get_mut::<Node>(child)
            {
                node.display = Display::None;
            }
        }
        glyphs::spawn(world, button, glyph);
    }
}

/// 移入菜单时显露原文字，返回顶栏时恢复图标；不重建按钮，保留禁用、选中和快捷键语义。
pub(super) fn menu_style(world: &mut World, root: Entity, in_menu: bool) {
    let entities = descendants(world, root, 3);
    for entity in entities {
        let text = world.get::<Text>(entity).is_some();
        let button = world.get::<InteractionButton>(entity).is_some();
        if let Some(mut node) = world.get_mut::<Node>(entity) {
            if text {
                node.display = if in_menu {
                    Display::Flex
                } else {
                    Display::None
                };
            } else if button {
                node.width = if in_menu { percent(100) } else { px(28) };
                node.justify_content = if in_menu {
                    JustifyContent::FlexStart
                } else {
                    JustifyContent::Center
                };
                node.column_gap = px(8);
                node.padding = if in_menu {
                    UiRect::horizontal(px(7))
                } else {
                    UiRect::ZERO
                };
            } else if entity == root {
                node.width = if in_menu { percent(100) } else { Val::Auto };
            }
        }
    }
}

pub(super) fn mount(world: &mut World) {
    if world.resource::<ChromeParts>().0.is_some() {
        return;
    }
    let headers: Vec<_> = world
        .query_filtered::<Entity, (With<WorkbenchHeader>, Without<Mounted>)>()
        .iter(world)
        .collect();
    for header in headers {
        let Some(children) = world.get::<Children>(header) else {
            continue;
        };
        let children: Vec<_> = children.iter().collect();
        let find = |item| {
            children
                .iter()
                .copied()
                .find(|e| world.get::<Item>(*e) == Some(&item))
        };
        let (Some(save), Some(undo), Some(redo), Some(selection), Some(settings), Some(spacer)) = (
            find(Item::Save),
            find(Item::Undo),
            find(Item::Redo),
            find(Item::Selection),
            find(Item::Settings),
            find(Item::Spacer),
        ) else {
            continue;
        };
        let layout_items: Vec<_> = [
            (Item::Navigation, Glyph::Navigation),
            (Item::Inspector, Glyph::Inspector),
            (Item::Focus, Glyph::Focus),
        ]
            .into_iter()
            .filter_map(|(item, glyph)| find(item).map(|entity| (entity, glyph)))
            .collect();
        let layout_controls: Vec<_> = layout_items.iter().map(|(entity, _)| *entity).collect();
        let Some(run) = children.iter().copied().find(|e| {
            world
                .get::<feathrise_editor_core::ProjectLaunchButton>(*e)
                .is_some()
        }) else {
            continue;
        };
        let slot = |kind| {
            children
                .iter()
                .copied()
                .find(|e| world.get::<ChromeSlot>(*e) == Some(&kind))
        };
        let project = slot(ChromeSlot::Project);
        let modes = slot(ChromeSlot::Modes);
        let profile = slot(ChromeSlot::Profile);
        let more = popup(world, Menu::More);
        let more_toggle = button(
            world,
            ChromeButton::Toggle(Menu::More),
            Glyph::More,
            "更多操作",
        );
        let font = feathrise_editor_core::editor_font(
            world.resource::<AssetServer>(),
            feathrise_editor_core::FONT_PATH,
        );
        let editor_fps = world.spawn((
            EditorFpsText,
            Name::new("编辑器帧率"),
            Text::new(""),
            TextLayout::no_wrap(),
            TextFont { font: font.into(), font_size: 12.0.into(), ..default() },
            TextColor(c::TEXT_MUTED),
            // 固定宽度基准让数字变化不移动按钮。空间不足时读数可收缩，保留拖动区和三键。
            Node {
                display: Display::None,
                width: px(64.),
                min_width: px(0.),
                flex_shrink: 1.,
                overflow: Overflow::clip_x(),
                ..default()
            },
            Pickable::IGNORE,
        )).id();
        let backdrop = world
            .spawn((
                Backdrop,
                GlobalZIndex(4100),
                UiInteraction::None,
                FocusPolicy::Block,
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(HEIGHT),
                    width: Val::Vw(100.),
                    height: Val::Vh(100.),
                    ..default()
                },
            ))
            .id();
        let captions = world
            .spawn(Node {
                height: percent(100),
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                margin: UiRect::left(px(5)),
                border: UiRect::left(px(1)),
                ..default()
            })
            .id();
        world
            .entity_mut(captions)
            .insert(BorderColor::all(c::COPPER_BORDER_WEAK));
        for (action, glyph, name) in [
            (WindowAction::Minimize, Glyph::Minimize, "最小化"),
            (WindowAction::Maximize, Glyph::Maximize, "最大化 / 还原"),
            (WindowAction::Close, Glyph::Close, "关闭窗口"),
        ] {
            let control = button(world, ChromeButton::Window(action), glyph, name);
            world.entity_mut(captions).add_child(control);
        }
        // 三个原布局入口直接显示图标，保留原点击、悬停说明及持久化身份。
        for (entity, glyph) in layout_items {
            flatten_button(world, entity, Some(glyph));
            if let Some(mut node) = world.get_mut::<Node>(entity) {
                node.flex_shrink = 0.; // 固定图标入口不可只压缩外层，留下重叠相邻按钮的命中区。
            }
        }
        for (entity, glyph) in [
            (save, Glyph::Save),
            (undo, Glyph::Undo),
            (redo, Glyph::Redo),
            (selection, Glyph::Filter),
            (settings, Glyph::Settings),
        ] {
            flatten_button(world, entity, Some(glyph));
        }
        menu_style(world, selection, true);
        if let Some(project) = project {
            flatten_button(world, project, None);
            if let Some(mut node) = world.get_mut::<Node>(project) {
                node.min_width = px(65);
                node.max_width = px(180);
                node.flex_shrink = 1.;
                node.overflow = Overflow::clip_x();
            }
            let labels = descendants(world, project, 1);
            for label in labels {
                if let Some(mut layout) = world.get_mut::<TextLayout>(label) {
                    layout.linebreak = bevy::text::LineBreak::NoWrap;
                }
            }
        }
        if let Some(mut node) = world.get_mut::<Node>(header) {
            node.height = px(HEIGHT);
            node.padding = UiRect::left(px(6));
            node.column_gap = px(3);
            node.overflow = Overflow::visible();
        }
        if let Some(mut node) = world.get_mut::<Node>(spacer) {
            node.min_width = px(24);
            node.height = percent(100);
        }
        let mut left: Vec<_> = children
            .iter()
            .copied()
            .filter(|e| {
                world.get::<Item>(*e).is_none()
                    && world.get::<ChromeSlot>(*e).is_none()
                    && *e != run
                    && world
                        .get::<Node>(*e)
                        .is_some_and(|n| n.position_type != PositionType::Absolute)
            })
            .collect();
        if let Some(project) = project {
            left.push(project);
        }
        // 作者设置与对象操作跟随左侧工具；空白拖动区隔开右侧的视口与运行入口。
        left.extend([save, undo, redo, selection, settings, more_toggle, editor_fps, spacer]);
        left.extend(modes);
        left.push(run);
        left.extend(profile);
        left.extend(layout_controls.iter().copied());
        left.extend([
            captions,
            more,
            backdrop,
        ]);
        world.entity_mut(header).add_children(&left).insert(Mounted);
        if let Some(mut node) = world.get_mut::<Node>(more_toggle) {
            node.display = Display::None;
        }
        // 配置紧跟启动／预览右侧；只移动原控件，保留原下拉菜单与切换事务。
        world.entity_mut(header).insert_children(0, &left);
        world.resource_mut::<ChromeParts>().0 = Some(HeaderParts {
            header,
            spacer,
            project,
            history: vec![save, undo, redo],
            selection,
            settings,
            modes,
            profile,
            run,
            layout_controls,
            more_toggle,
            editor_fps,
            more,
            backdrop,
            captions,
        });
    }
}
