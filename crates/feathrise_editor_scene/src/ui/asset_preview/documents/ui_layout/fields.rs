//! 属性面板的原生输入和一次性提交。校验失败时保留已采用的控件树。

use bevy::{input_focus::tab_navigation::TabIndex, prelude::*, text::EditableText, ui::FocusPolicy};
use feathrise_editor_ui::colors;
use feathrise_ui_bsn::{UiWidget, WidgetKind, valid_asset_path};
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
}

pub(super) type Values = HashMap<Field, String>;

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
            .is_some_and(|parent| parent.kind == WidgetKind::Container)
        {
            return Err("父节点需要已有容器的 ID".into());
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
    item.padding = number(value(Field::Padding), false)?;
    item.gap = number(value(Field::Gap), false)?;
    let (x, y) = (value(Field::X).trim(), value(Field::Y).trim());
    item.position = if root_selected || x.is_empty() && y.is_empty() {
        None
    } else {
        Some(Vec2::new(
            if x.is_empty() { 0.0 } else { number(x, true)? },
            if y.is_empty() { 0.0 } else { number(y, true)? },
        ))
    };
    item.background = background(value(Field::Background))?;
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
        return Err("背景色需要十六进制 RGB 或 RGBA，例如 #405060FF".into());
    }
    let mut color = [255; 4];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        let channel = std::str::from_utf8(pair).expect("已校验 ASCII");
        color[index] = u8::from_str_radix(channel, 16).map_err(|_| "背景色含无效字符")?;
    }
    Ok(Some(color))
}
