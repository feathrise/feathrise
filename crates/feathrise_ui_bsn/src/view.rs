//! 用同一棵原生 Bevy UI 树渲染工程中的界面和独立编辑器画布。

use crate::{UiLayout, UiWidget, WidgetKind};
use bevy::{
    picking::{events::PointerClick, pointer::PointerButton},
    prelude::*,
    ui_widgets::Button,
};

pub struct LayoutContext<'a> {
    pub font: &'a Handle<Font>,
    pub assets: &'a AssetServer,
    pub text_color: Color,
    pub button_color: Color,
    pub interactive: bool,
    pub preview: bool,
}

#[derive(Message)]
pub struct UiLayoutAction {
    pub name: String,
    pub preview: bool,
}

pub fn spawn_layout(
    parent: &mut ChildSpawnerCommands,
    layout: &UiLayout,
    context: &LayoutContext,
    mut decorate: impl FnMut(&mut EntityCommands, &UiWidget),
    mut binding: impl FnMut(&mut ChildSpawnerCommands, &UiWidget),
) -> Entity {
    spawn_widget(parent, &layout.root, context, &mut decorate, &mut binding)
}

fn spawn_widget(
    parent: &mut ChildSpawnerCommands,
    widget: &UiWidget,
    context: &LayoutContext,
    decorate: &mut impl FnMut(&mut EntityCommands, &UiWidget),
    binding: &mut impl FnMut(&mut ChildSpawnerCommands, &UiWidget),
) -> Entity {
    let mut node = Node {
        width: widget.width,
        height: widget.height,
        flex_shrink: 0.0,
        min_width: Val::Px(0.0),
        flex_direction: widget.direction,
        row_gap: Val::Px(widget.gap),
        column_gap: Val::Px(widget.gap),
        padding: UiRect::all(Val::Px(widget.padding)),
        ..default()
    };
    if let Some(position) = widget.position {
        node.position_type = PositionType::Absolute;
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
    }
    if matches!(widget.kind, WidgetKind::Button | WidgetKind::Binding) {
        node.justify_content = JustifyContent::Center;
    }
    if widget.kind == WidgetKind::Button {
        node.align_items = AlignItems::Center;
    }
    let mut entity = parent.spawn((node, Name::new(widget.id.clone())));
    if let Some([r, g, b, a]) = widget.background {
        entity.insert(BackgroundColor(Color::srgba_u8(r, g, b, a)));
    } else if widget.kind == WidgetKind::Button {
        entity.insert(BackgroundColor(context.button_color));
    }
    if widget.kind == WidgetKind::Button && context.interactive && !widget.action.is_empty() {
        let name = widget.action.clone();
        let preview = context.preview;
        entity.insert(Button::default()).observe(
            move |click: On<PointerClick>, mut actions: MessageWriter<UiLayoutAction>| {
                if click.button == PointerButton::Primary {
                    actions.write(UiLayoutAction {
                        name: name.clone(),
                        preview,
                    });
                }
            },
        );
    }
    decorate(&mut entity, widget);
    entity.with_children(|children| match widget.kind {
        WidgetKind::Container => {
            for child in &widget.children {
                spawn_widget(children, child, context, decorate, binding);
            }
        }
        WidgetKind::Text | WidgetKind::Button => {
            children.spawn((
                Text::new(&widget.content),
                TextFont {
                    font: context.font.clone().into(),
                    font_size: widget.font_size.into(),
                    ..default()
                },
                TextColor(context.text_color),
                Pickable::IGNORE, // 文字交给所属控件处理画布命中或操作。
            ));
        }
        WidgetKind::Image if !widget.content.is_empty() => {
            children.spawn((
                ImageNode::new(context.assets.load::<Image>(widget.content.clone())),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        }
        WidgetKind::Binding => binding(children, widget),
        WidgetKind::Image => {}
    });
    entity.id()
}
