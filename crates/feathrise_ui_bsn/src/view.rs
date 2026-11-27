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

/// 借用资产中的独立子树装配，不为屏幕分区复制整棵布局。
pub fn spawn_widget_tree(
    parent: &mut ChildSpawnerCommands,
    widget: &UiWidget,
    context: &LayoutContext,
    mut decorate: impl FnMut(&mut EntityCommands, &UiWidget),
    mut binding: impl FnMut(&mut ChildSpawnerCommands, &UiWidget),
) -> Entity {
    spawn_widget(parent, widget, context, &mut decorate, &mut binding)
}

/// 编辑画布与正式工程必须从相同属性构造布局，拖动亦可复用本函数读取实际锚点。
pub fn widget_node(widget: &UiWidget) -> Node {
    let mut node = Node {
        width: widget.width,
        height: widget.height,
        flex_direction: widget.direction,
        row_gap: if widget.style.row_gap == Val::Auto {
            Val::Px(widget.gap)
        } else {
            widget.style.row_gap
        },
        column_gap: if widget.style.column_gap == Val::Auto {
            Val::Px(widget.gap)
        } else {
            widget.style.column_gap
        },
        padding: if widget.padding != 0.0 || widget.style.padding == UiRect::ZERO {
            UiRect::all(Val::Px(widget.padding))
        } else {
            widget.style.padding
        },
        ..widget.style.clone()
    };
    if let Some(position) = widget.position {
        node.position_type = PositionType::Absolute;
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
        node.right = Val::Auto;
        node.bottom = Val::Auto;
    }
    if matches!(widget.kind, WidgetKind::Button | WidgetKind::Binding)
        && node.justify_content == JustifyContent::Default
    {
        node.justify_content = JustifyContent::Center;
    }
    if widget.kind == WidgetKind::Button && node.align_items == AlignItems::Default {
        node.align_items = AlignItems::Center;
    }
    node
}

fn spawn_widget(
    parent: &mut ChildSpawnerCommands,
    widget: &UiWidget,
    context: &LayoutContext,
    decorate: &mut impl FnMut(&mut EntityCommands, &UiWidget),
    binding: &mut impl FnMut(&mut ChildSpawnerCommands, &UiWidget),
) -> Entity {
    let mut entity = parent.spawn((widget_node(widget), Name::new(widget.id.clone())));
    if let Some([r, g, b, a]) = widget.background {
        entity.insert(BackgroundColor(Color::srgba_u8(r, g, b, a)));
    } else if widget.kind == WidgetKind::Button {
        entity.insert(BackgroundColor(context.button_color));
    }
    if let Some([r, g, b, a]) = widget.border_color {
        entity.insert(BorderColor::all(Color::srgba_u8(r, g, b, a)));
    }
    if widget.hidden {
        entity.insert(Visibility::Hidden);
    }
    // 文本和图片就是控件自身，数据更新标记与正式组件在同一实体，不依赖包装子节点。
    match widget.kind {
        WidgetKind::Text => insert_text(&mut entity, widget, context),
        WidgetKind::Image => {
            let image = if widget.content.is_empty() {
                // 空路径是工程动态图片的占位，编辑画布不能把默认白纹理当成实际内容。
                ImageNode {
                    color: Color::NONE,
                    ..default()
                }
            } else {
                ImageNode::new(context.assets.load::<Image>(widget.content.clone()))
            };
            entity.insert(image);
        }
        _ => {}
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
        WidgetKind::Button => {
            if !widget.content.is_empty() {
                let mut label = children.spawn(Pickable::IGNORE);
                insert_text(&mut label, widget, context);
            }
            for child in &widget.children {
                spawn_widget(children, child, context, decorate, binding);
            }
        }
        WidgetKind::Binding => binding(children, widget),
        WidgetKind::Text | WidgetKind::Image => {}
    });
    entity.id()
}

fn insert_text(entity: &mut EntityCommands, widget: &UiWidget, context: &LayoutContext) {
    let color = widget
        .text_color
        .map_or(context.text_color, |[r, g, b, a]| {
            Color::srgba_u8(r, g, b, a)
        });
    entity.insert((
        Text::new(&widget.content),
        TextFont {
            font: context.font.clone().into(),
            font_size: widget.font_size.into(),
            ..default()
        },
        TextColor(color),
        TextLayout::justify(widget.text_justify),
    ));
    if widget.text_shadow {
        entity.insert(TextShadow {
            offset: Vec2::ONE,
            color: Color::srgba(0.0, 0.0, 0.0, 0.80),
        });
    }
}
