//! 属性面板的原生输入和一次性提交。校验失败时保留已采用的控件树。

use bevy::{input_focus::tab_navigation::TabIndex, prelude::*, text::EditableText, ui::{CornerRadius, FocusPolicy}};
use feathrise_editor_ui::colors;
use feathrise_ui_bsn::{UiWidget, WidgetKind, valid_asset_path, widget_node};
use std::collections::HashMap;

#[derive(Clone, Copy, Component, Debug, PartialEq, Eq, Hash)]
pub(super) enum Field {
    Id,
    Parent,
    Width,
    Height,
    Gap,
    Padding,
    X,
    Y,
    Background,
    Content,
    FontSize,
    Action,
    Binding,
    Right,
    Bottom,
    Border,
    Radius,
    BorderColor,
    TextColor,
    TextShadow,
    Hidden,
    AlignItems,
    JustifyContent,
    TextJustify,
}

pub(super) type Values = HashMap<Field, String>;

/// 表单与应用比较使用同一个快照；未修改的原生样式保持原表示，不产生额外草稿。
pub(super) fn values(widget: &UiWidget, parent: Option<&str>) -> Values {
    let node = widget_node(widget);
    let mut fields = Values::from([
        (Field::Id, widget.id.clone()), (Field::Parent, parent.unwrap_or_default().into()),
        (Field::Width, length_text(widget.width)), (Field::Height, length_text(widget.height)),
        (Field::Gap, widget.gap.to_string()), (Field::Padding, rect_text(node.padding)),
        (Field::X, anchor_text(node.left)), (Field::Y, anchor_text(node.top)),
        (Field::Right, anchor_text(node.right)), (Field::Bottom, anchor_text(node.bottom)),
        (Field::Background, color_text(widget.background)), (Field::Content, widget.content.clone()),
        (Field::FontSize, widget.font_size.to_string()), (Field::Action, widget.action.clone()),
        (Field::Binding, widget.binding.clone()), (Field::Border, rect_text(node.border)),
        (Field::Radius, radius_text(node.border_radius)),
        (Field::BorderColor, color_text(widget.border_color)), (Field::TextColor, color_text(widget.text_color)),
        (Field::TextShadow, widget.text_shadow.to_string()),
        (Field::Hidden, widget.hidden.to_string()),
        (Field::AlignItems, format!("{:?}", widget.style.align_items)),
        (Field::JustifyContent, format!("{:?}", widget.style.justify_content)),
        (Field::TextJustify, format!("{:?}", widget.text_justify)),
    ]);
    if parent.is_none() { fields.remove(&Field::X); fields.remove(&Field::Y); }
    fields
}

impl Field {
    pub(super) fn input(self, parent: &mut ChildSpawnerCommands, font: &Handle<Font>, text: &str) {
        parent.spawn((
            self,
            crate::ui::text_input::EditorTextInput,
            // 官方指针焦点通过 TabIndex 接纳字段；仅有 EditableText 不能取得键盘焦点。
            TabIndex(0),
            EditableText {
                allow_newlines: false,
                ..EditableText::new(text)
            },
            TextFont {
                font: font.clone().into(),
                font_size: 12.0.into(),
                ..default()
            },
            TextColor(colors::TEXT_PRIMARY),
            TextLayout::no_wrap(),
            FocusPolicy::Block,
            Node {
                width: percent(100),
                height: px(25),
                min_width: px(0),
                flex_shrink: 0.0,
                padding: UiRect::axes(px(5), px(3)),
                overflow: Overflow::clip_x(),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.12, 0.14, 0.17)),
        ));
    }
}

pub(super) fn apply(
    root: &mut UiWidget,
    selected: &mut String,
    fields: &Values,
) -> Result<(), String> {
    let value = |field| fields.get(&field).map(String::as_str).unwrap_or_default();
    let id = value(Field::Id).trim();
    if !valid_id(id) {
        return Err("控件 ID 需要字母或下划线开头，允许字母、数字和下划线".into());
    }
    if id != selected.as_str() && root.find(id).is_some() {
        return Err("控件 ID 已存在".into());
    }
    let parent_id = value(Field::Parent).trim();
    if root.id != *selected {
        let widget = root.find(selected).ok_or("选中的控件已移除")?;
        if widget.find(parent_id).is_some() {
            return Err("不能把控件移入它自己或它的后代".into());
        }
        if !root
            .find(parent_id)
            .is_some_and(|parent| matches!(parent.kind, WidgetKind::Container | WidgetKind::Button))
        {
            return Err("父节点需要已有容器或按钮的 ID".into());
        }
        let current = root.parent_mut(selected).ok_or("找不到父容器")?;
        if current.id != parent_id {
            let i = current
                .children
                .iter()
                .position(|widget| widget.id == *selected)
                .expect("父容器包含当前控件");
            let item = current.children.remove(i);
            root.find_mut(parent_id)
                .expect("目标容器已校验")
                .children
                .push(item);
        }
    }
    let root_selected = root.id == *selected;
    let item = root.find_mut(selected).ok_or("请先选中控件")?;
    item.id = id.to_owned();
    item.width = length(value(Field::Width))?;
    item.height = length(value(Field::Height))?;
    let original_node = widget_node(item);
    let padding = rect(value(Field::Padding))?;
    if padding != original_node.padding {
        // 统一像素和轴向/四边内距都可编辑；不把已有 axes 在未改值的提交中压成 all(0)。
        item.padding = match padding { UiRect { left: Val::Px(left), right: Val::Px(right), top: Val::Px(top), bottom: Val::Px(bottom) }
            if left == right && left == top && left == bottom => left, _ => 0.0 };
        item.style.padding = if item.padding != 0.0 { UiRect::ZERO } else { padding };
    }
    let gap = number(value(Field::Gap), false)?;
    if gap != item.gap {
        item.gap = gap;
        item.style.row_gap = Val::Auto;
        item.style.column_gap = Val::Auto;
    }
    if !root_selected {
        let anchors = [Field::X, Field::Y, Field::Right, Field::Bottom];
        let old = [original_node.left, original_node.top, original_node.right, original_node.bottom];
        let mut next = old;
        for (index, field) in anchors.into_iter().enumerate() {
            if let Some(value) = fields.get(&field) { next[index] = anchor(value)?; }
        }
        if next != old {
            // 未改的空 X/Y 不能清掉 right/bottom 或百分比锚；只有显式编辑才更新定位。
            item.position = match next {
                [Val::Px(x), Val::Px(y), Val::Auto, Val::Auto] => Some(Vec2::new(x, y)),
                [Val::Px(x), Val::Auto, Val::Auto, Val::Auto] => Some(Vec2::new(x, 0.0)),
                [Val::Auto, Val::Px(y), Val::Auto, Val::Auto] => Some(Vec2::new(0.0, y)),
                _ => None,
            };
            let [left, top, right, bottom] = if item.position.is_some() { [Val::Auto; 4] } else { next };
            item.style.position_type = if item.position.is_some() || next == [Val::Auto; 4] {
                PositionType::Relative
            } else { PositionType::Absolute };
            (item.style.left, item.style.top, item.style.right, item.style.bottom) = (left, top, right, bottom);
        }
    }
    item.background = background(value(Field::Background))?;
    if let Some(value) = fields.get(&Field::Binding) { item.binding = value.trim().into(); }
    if let Some(value) = fields.get(&Field::Border) { item.style.border = rect(value)?; }
    if let Some(value) = fields.get(&Field::Radius) {
        item.style.border_radius = radius(value)?;
    }
    if let Some(value) = fields.get(&Field::BorderColor) { item.border_color = background(value)?; }
    if let Some(value) = fields.get(&Field::TextColor) { item.text_color = background(value)?; }
    if let Some(value) = fields.get(&Field::TextShadow) {
        item.text_shadow = match value.trim() { "true" => true, "false" => false, _ => return Err("文字阴影填写 true 或 false".into()) };
    }
    if let Some(value) = fields.get(&Field::Hidden) {
        item.hidden = match value.trim() { "true" => true, "false" => false, _ => return Err("隐藏属性填写 true 或 false".into()) };
    }
    if let Some(value) = fields.get(&Field::AlignItems) {
        item.style.align_items = match enum_value(value) {
            "Default" => AlignItems::Default, "Start" => AlignItems::Start, "End" => AlignItems::End,
            "FlexStart" => AlignItems::FlexStart, "FlexEnd" => AlignItems::FlexEnd,
            "Center" => AlignItems::Center, "Baseline" => AlignItems::Baseline, "Stretch" => AlignItems::Stretch,
            _ => return Err("交叉轴对齐填写 Default、Start、End、Center、Stretch 或 Baseline".into()),
        };
    }
    if let Some(value) = fields.get(&Field::JustifyContent) {
        item.style.justify_content = match enum_value(value) {
            "Default" => JustifyContent::Default, "Start" => JustifyContent::Start, "End" => JustifyContent::End,
            "FlexStart" => JustifyContent::FlexStart, "FlexEnd" => JustifyContent::FlexEnd,
            "Center" => JustifyContent::Center, "SpaceBetween" => JustifyContent::SpaceBetween,
            "SpaceEvenly" => JustifyContent::SpaceEvenly, "SpaceAround" => JustifyContent::SpaceAround,
            "Stretch" => JustifyContent::Stretch, _ => return Err("主轴对齐值无效，例如 Center 或 SpaceBetween".into()),
        };
    }
    if let Some(value) = fields.get(&Field::TextJustify) {
        item.text_justify = match enum_value(value) {
            "Left" => Justify::Left, "Center" => Justify::Center, "Right" => Justify::Right,
            "Justified" => Justify::Justified, _ => return Err("文字对齐填写 Left、Center、Right 或 Justified".into()),
        };
    }
    match item.kind {
        WidgetKind::Text | WidgetKind::Button => {
            item.content = value(Field::Content).to_owned();
            item.font_size = number(value(Field::FontSize), false)?;
            if item.font_size == 0.0 {
                return Err("字号必须大于零".into());
            }
            if item.kind == WidgetKind::Button {
                item.action = value(Field::Action).trim().to_owned();
            }
        }
        WidgetKind::Binding => {
            item.content = value(Field::Content).trim().to_owned();
        }
        WidgetKind::Image => {
            let path = value(Field::Content).trim();
            if !path.is_empty() && !valid_asset_path(path) {
                return Err("图片需要工程内的相对资产路径".into());
            }
            item.content = path.to_owned();
        }
        _ => {}
    }
    *selected = id.to_owned();
    Ok(())
}

fn valid_id(id: &str) -> bool {
    id.as_bytes()
        .first()
        .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn enum_value(value: &str) -> &str { value.trim().rsplit("::").next().unwrap_or_default() }

fn color_text(color: Option<[u8; 4]>) -> String {
    color.map(|[r, g, b, a]| format!("#{r:02X}{g:02X}{b:02X}{a:02X}")).unwrap_or_default()
}

fn anchor_text(value: Val) -> String { if value == Val::Auto { String::new() } else { length_text(value) } }

fn anchor(value: &str) -> Result<Val, String> {
    if value.trim().is_empty() { return Ok(Val::Auto); }
    let value = value.trim();
    if value == "auto" { return Ok(Val::Auto); }
    if let Some(percent) = value.strip_suffix('%') { return number(percent, true).map(Val::Percent); }
    number(value.strip_suffix("px").unwrap_or(value), true).map(Val::Px)
}

fn rect_text(rect: UiRect) -> String {
    if rect.left == rect.right && rect.left == rect.top && rect.left == rect.bottom {
        length_text(rect.left)
    } else { [rect.top, rect.right, rect.bottom, rect.left].map(length_text).join(" ") }
}

fn rect(value: &str) -> Result<UiRect, String> {
    let values = value.split_whitespace().map(length).collect::<Result<Vec<_>, _>>()?;
    Ok(match values.as_slice() {
        [all] => UiRect::all(*all), [vertical, horizontal] => UiRect::axes(*horizontal, *vertical),
        [top, right, bottom, left] => UiRect { top: *top, right: *right, bottom: *bottom, left: *left },
        _ => return Err("边距或圆角填写一个值、上下/左右两值，或上/右/下/左四值".into()),
    })
}

fn radius_text(radius: BorderRadius) -> String {
    let corners = [radius.top_left, radius.top_right, radius.bottom_right, radius.bottom_left];
    let text = |corner: CornerRadius| if corner.y == Val::Auto { length_text(corner.x) }
        else if corner.x == Val::Auto { length_text(corner.y) }
        else { format!("{}/{}", length_text(corner.x), length_text(corner.y)) };
    if corners.iter().all(|corner| *corner == corners[0]) { text(corners[0]) }
    else { corners.map(text).join(" ") }
}

fn radius(value: &str) -> Result<BorderRadius, String> {
    let values = value.split_whitespace().map(|value| {
        if let Some((x, y)) = value.split_once('/') { Ok(CornerRadius { x: length(x)?, y: length(y)? }) }
        else { length(value).map(Into::into) }
    }).collect::<Result<Vec<_>, String>>()?;
    let (top_left, top_right, bottom_right, bottom_left) = match values.as_slice() {
        [all] => (*all, *all, *all, *all), [a, b] => (*a, *b, *a, *b),
        [a, b, c, d] => (*a, *b, *c, *d), _ => return Err("圆角填写一个值、两值或顺时针四值；椭圆使用 x/y".into()),
    };
    Ok(BorderRadius { top_left, top_right, bottom_right, bottom_left })
}

pub(super) fn length_text(value: Val) -> String {
    match value {
        Val::Px(value) => value.to_string(),
        Val::Percent(value) => format!("{value}%"),
        _ => "auto".into(),
    }
}

fn length(value: &str) -> Result<Val, String> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("auto") {
        return Ok(Val::Auto);
    }
    if let Some(percent) = value.strip_suffix('%') {
        return number(percent, false).map(Val::Percent);
    }
    number(value.strip_suffix("px").unwrap_or(value), false).map(Val::Px)
}

fn number(value: &str, signed: bool) -> Result<f32, String> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite() && (signed || *n >= 0.0))
        .ok_or_else(|| "请输入有效的尺寸或坐标数值".into())
}

fn background(value: &str) -> Result<Option<[u8; 4]>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let hex = value.strip_prefix('#').unwrap_or(value);
    if !matches!(hex.len(), 6 | 8) || !hex.is_ascii() {
        return Err("颜色需要十六进制 RGB 或 RGBA，例如 #405060FF".into());
    }
    let mut color = [255; 4];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        let channel = std::str::from_utf8(pair).expect("已校验 ASCII");
        color[index] = u8::from_str_radix(channel, 16).map_err(|_| "背景色含无效字符")?;
    }
    Ok(Some(color))
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_ui_bsn::UiLayout;

    fn hud() -> UiLayout {
        UiLayout::parse(r#"#root Node { width: percent(100), height: percent(100) }
        Children [
            #panel Node { position_type: PositionType::Absolute, left: percent(50), bottom: px(14),
                width: px(414), padding: UiRect::axes(px(8), px(3)), margin: UiRect::left(px(-207)),
                border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(7)),
                justify_content: JustifyContent::Center, align_items: AlignItems::Center }
                BorderColor(Color::srgba_u8(190, 135, 62, 122)) UiBind("hud.panel")
            Children [
                #label Node {} Text("气血") TextColor(Color::srgba_u8(235, 226, 205, 235))
                    UiTextShadow UiHidden UiBind("player.health.text")
                --
                #button Node {} Button Text("返回") UiAction("close")
            ]
        ]"#).unwrap()
    }

    #[test]
    fn unchanged_form_preserves_hud_anchors_axes_padding_and_bindings() {
        let mut layout = hud();
        let original = layout.clone();
        for (id, parent) in [("root", None), ("panel", Some("root")),
            ("label", Some("panel")), ("button", Some("panel"))]
        {
            let fields = values(layout.root.find(id).unwrap(), parent);
            apply(&mut layout.root, &mut id.to_string(), &fields).unwrap();
        }
        assert_eq!(layout, original, "原样应用不能重置锚点、axes 内距、样式、绑定或默认按钮对齐");
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), original);
    }

    #[test]
    fn changing_border_binding_and_padding_roundtrips_without_losing_bottom_anchor() {
        let mut layout = hud();
        let mut fields = values(layout.root.find("panel").unwrap(), Some("root"));
        fields.insert(Field::Binding, "hud.changed".into());
        fields.insert(Field::Padding, "4 9 6 11".into());
        fields.insert(Field::Border, "2".into());
        fields.insert(Field::Radius, "10 12 14 16".into());
        apply(&mut layout.root, &mut "panel".to_string(), &fields).unwrap();
        let panel = layout.root.find("panel").unwrap();
        assert_eq!(panel.binding, "hud.changed");
        assert_eq!(widget_node(panel).bottom, px(14));
        assert_eq!(widget_node(panel).left, percent(50));
        assert_eq!(widget_node(panel).padding, UiRect { top: px(4), right: px(9), bottom: px(6), left: px(11) });
        assert_eq!(widget_node(panel).border_radius.bottom_left, px(16).into());
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }

    #[test]
    fn image_child_of_styled_button_can_resize_without_losing_binding_or_parent() {
        let mut layout = UiLayout::parse(r#"#root Node {} Children [
            #button Node { border_radius: BorderRadius::all(px(5)) } Button Text("") UiBind("skill.0.button")
            Children [ #icon Node { width: px(36), height: px(36) } UiImage("icons/skills/slash.png") UiBind("skill.0.icon") ]
        ]"#).unwrap();
        let mut fields = values(layout.root.find("icon").unwrap(), Some("button"));
        fields.insert(Field::Width, "32".into());
        apply(&mut layout.root, &mut "icon".to_string(), &fields).unwrap();
        assert_eq!(layout.root.find("icon").unwrap().width, px(32));
        assert_eq!(layout.root.find("icon").unwrap().binding, "skill.0.icon");
        assert_eq!(layout.root.parent_mut("icon").unwrap().id, "button");
        assert_eq!(layout.root.find("button").unwrap().style.border_radius, BorderRadius::all(px(5)));
        let mut cycle = values(layout.root.find("button").unwrap(), Some("root"));
        cycle.insert(Field::Parent, "icon".into());
        assert!(apply(&mut layout.root, &mut "button".to_string(), &cycle).is_err());
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }
}
