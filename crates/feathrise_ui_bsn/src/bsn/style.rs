//! 通用原生布局和显示注解的读写；与控件树语法分离，避免复制一套 Node 模型。

use super::*;
use bevy::ui::CornerRadius;

macro_rules! read_enum {
    ($reader:expr, $kind:ident, $($variant:ident),+ $(,)?) => {{
        $reader.expect(concat!(stringify!($kind), "::"))?;
        match $reader.word()? {
            $(stringify!($variant) => $kind::$variant,)+
            _ => return Err($reader.error(concat!("不支持的 ", stringify!($kind), " 取值"))),
        }
    }};
}

pub(super) fn padding_text(widget: &UiWidget) -> String {
    if widget.padding != 0.0 || widget.style.padding == UiRect::ZERO {
        format!("UiRect::all(px({:?}f32))", widget.padding)
    } else {
        let rect = widget.style.padding;
        format!(
            "UiRect {{ left: {}, right: {}, top: {}, bottom: {} }}",
            length_text(rect.left),
            length_text(rect.right),
            length_text(rect.top),
            length_text(rect.bottom)
        )
    }
}

pub(super) fn write_node(output: &mut String, widget: &UiWidget) {
    let node = &widget.style;
    let defaults = UiWidget::default_style(widget.kind);
    let cross_gap = if widget.direction == FlexDirection::Row {
        ("row_gap", node.row_gap)
    } else {
        ("column_gap", node.column_gap)
    };
    if cross_gap.1 != Val::Auto {
        let _ = write!(output, ", {}: {}", cross_gap.0, length_text(cross_gap.1));
    }
    if widget.position.is_none() && node.position_type == PositionType::Absolute {
        let _ = write!(output, ", position_type: PositionType::Absolute");
        for (name, value) in [
            ("left", node.left),
            ("right", node.right),
            ("top", node.top),
            ("bottom", node.bottom),
        ] {
            if value != Val::Auto {
                let _ = write!(output, ", {name}: {}", length_text(value));
            }
        }
    }
    for (name, value, default) in [
        ("min_width", node.min_width, defaults.min_width),
        ("max_width", node.max_width, defaults.max_width),
        ("min_height", node.min_height, defaults.min_height),
        ("max_height", node.max_height, defaults.max_height),
    ] {
        if value != default {
            let _ = write!(output, ", {name}: {}", length_text(value));
        }
    }
    for (name, value, default) in [
        ("flex_grow", node.flex_grow, defaults.flex_grow),
        ("flex_shrink", node.flex_shrink, defaults.flex_shrink),
    ] {
        if value != default {
            let _ = write!(output, ", {name}: {value:?}f32");
        }
    }
    for (name, rect, default) in [
        ("margin", node.margin, defaults.margin),
        ("border", node.border, defaults.border),
    ] {
        if rect != default {
            let _ = write!(
                output,
                ", {name}: UiRect {{ left: {}, right: {}, top: {}, bottom: {} }}",
                length_text(rect.left),
                length_text(rect.right),
                length_text(rect.top),
                length_text(rect.bottom)
            );
        }
    }
    if node.border_radius != defaults.border_radius {
        let radius = node.border_radius;
        if [radius.top_right, radius.bottom_left, radius.bottom_right]
            .into_iter()
            .all(|other| other == radius.top_left)
        {
            let _ = write!(
                output,
                ", border_radius: BorderRadius::all({})",
                corner_text(radius.top_left)
            );
        } else {
            let _ = write!(
                output,
                ", border_radius: BorderRadius::new({}, {}, {}, {})",
                corner_text(radius.top_left),
                corner_text(radius.top_right),
                corner_text(radius.bottom_right),
                corner_text(radius.bottom_left)
            );
        }
    }
    if node.justify_content != defaults.justify_content {
        let _ = write!(
            output,
            ", justify_content: JustifyContent::{:?}",
            node.justify_content
        );
    }
    if node.align_items != defaults.align_items {
        let _ = write!(output, ", align_items: AlignItems::{:?}", node.align_items);
    }
    if node.flex_wrap != defaults.flex_wrap {
        let _ = write!(output, ", flex_wrap: FlexWrap::{:?}", node.flex_wrap);
    }
    if node.overflow != defaults.overflow {
        let method = if node.overflow == Overflow::clip() {
            "clip"
        } else if node.overflow == Overflow::clip_x() {
            "clip_x"
        } else {
            "clip_y"
        };
        let _ = write!(output, ", overflow: Overflow::{method}()");
    }
}

fn corner_text(corner: CornerRadius) -> String {
    if corner.y == Val::Auto {
        length_text(corner.x)
    } else {
        format!(
            "CornerRadius::new({}, {})",
            length_text(corner.x),
            length_text(corner.y)
        )
    }
}

pub(super) fn write_components(output: &mut String, widget: &UiWidget) {
    for (name, color) in [
        ("BorderColor", widget.border_color),
        ("TextColor", widget.text_color),
    ] {
        if let Some([r, g, b, a]) = color {
            let _ = write!(output, " {name}(Color::srgba_u8({r}, {g}, {b}, {a}))");
        }
    }
    if !widget.binding.is_empty() {
        let _ = write!(output, " UiBind({:?})", widget.binding);
    }
    if widget.text_shadow {
        let _ = write!(output, " UiTextShadow");
    }
    if widget.hidden {
        let _ = write!(output, " UiHidden");
    }
    if widget.text_justify != Justify::Left {
        let _ = write!(output, " UiTextAlign(Justify::{:?})", widget.text_justify);
    }
}

impl Reader<'_> {
    pub(super) fn style_field(&mut self, node: &mut Node, field: &str) -> Result<(), String> {
        match field {
            "right" => node.right = self.length_with_sign(true)?,
            "bottom" => node.bottom = self.length_with_sign(true)?,
            "min_width" => node.min_width = self.length()?,
            "max_width" => node.max_width = self.length()?,
            "min_height" => node.min_height = self.length()?,
            "max_height" => node.max_height = self.length()?,
            "margin" => node.margin = self.rectangle(true)?,
            "border" => node.border = self.rectangle(false)?,
            "flex_grow" => node.flex_grow = self.number(false)?,
            "flex_shrink" => node.flex_shrink = self.number(false)?,
            "border_radius" => {
                self.expect("BorderRadius::")?;
                let method = self.word()?;
                self.expect("(")?;
                let first = self.corner_radius()?;
                node.border_radius = match method {
                    "all" => BorderRadius::all(first),
                    "new" => {
                        self.expect(",")?;
                        let right = self.corner_radius()?;
                        self.expect(",")?;
                        let bottom = self.corner_radius()?;
                        self.expect(",")?;
                        BorderRadius::new(first, right, bottom, self.corner_radius()?)
                    }
                    _ => return Err(self.error("圆角需要 BorderRadius::all 或 new")),
                };
                self.expect(")")?;
            }
            "justify_content" => {
                node.justify_content = read_enum!(
                    self,
                    JustifyContent,
                    Default,
                    Start,
                    StartSafe,
                    End,
                    EndSafe,
                    FlexStart,
                    FlexStartSafe,
                    FlexEnd,
                    FlexEndSafe,
                    Center,
                    CenterSafe,
                    Stretch,
                    SpaceBetween,
                    SpaceEvenly,
                    SpaceAround
                )
            }
            "align_items" => {
                node.align_items = read_enum!(
                    self,
                    AlignItems,
                    Default,
                    Start,
                    StartSafe,
                    End,
                    EndSafe,
                    FlexStart,
                    FlexStartSafe,
                    FlexEnd,
                    FlexEndSafe,
                    Center,
                    CenterSafe,
                    Baseline,
                    Stretch
                )
            }
            "flex_wrap" => node.flex_wrap = read_enum!(self, FlexWrap, NoWrap, Wrap, WrapReverse),
            "overflow" => {
                self.expect("Overflow::")?;
                node.overflow = match self.word()? {
                    "clip" => Overflow::clip(),
                    "clip_x" => Overflow::clip_x(),
                    "clip_y" => Overflow::clip_y(),
                    "visible" => Overflow::visible(),
                    _ => return Err(self.error("不支持的 overflow")),
                };
                self.expect("()")?;
            }
            _ => return Err(self.error(&format!("不支持的 Node 字段 {field}"))),
        }
        Ok(())
    }

    fn corner_radius(&mut self) -> Result<CornerRadius, String> {
        if !self.take("CornerRadius::") {
            return self.length().map(Into::into);
        }
        let method = self.word()?;
        self.expect("(")?;
        let x = self.length()?;
        let radius = match method {
            "circular" => CornerRadius::circular(x),
            "all" => CornerRadius::all(x),
            "new" => {
                self.expect(",")?;
                CornerRadius::new(x, self.length()?)
            }
            _ => return Err(self.error("圆角需要 CornerRadius::circular、all 或 new")),
        };
        self.expect(")")?;
        Ok(radius)
    }

    pub(super) fn rectangle(&mut self, signed: bool) -> Result<UiRect, String> {
        self.expect("UiRect")?;
        if self.take("::") {
            let method = self.word()?;
            self.expect("(")?;
            let first = self.length_with_sign(signed)?;
            let rect = match method {
                "all" => UiRect::all(first),
                "left" => UiRect::left(first),
                "right" => UiRect::right(first),
                "top" => UiRect::top(first),
                "bottom" => UiRect::bottom(first),
                "horizontal" => UiRect::horizontal(first),
                "vertical" => UiRect::vertical(first),
                "axes" => {
                    self.expect(",")?;
                    UiRect::axes(first, self.length_with_sign(signed)?)
                }
                _ => return Err(self.error("不支持的 UiRect 构造方式")),
            };
            self.expect(")")?;
            return Ok(rect);
        }
        self.expect("{")?;
        let mut rect = UiRect::ZERO;
        let mut fields = HashSet::new();
        while !self.take("}") {
            let name = self.word()?;
            if !fields.insert(name) {
                return Err(self.error("UiRect 字段重复"));
            }
            self.expect(":")?;
            let value = self.length_with_sign(signed)?;
            match name {
                "left" => rect.left = value,
                "right" => rect.right = value,
                "top" => rect.top = value,
                "bottom" => rect.bottom = value,
                _ => return Err(self.error("UiRect 仅支持 left/right/top/bottom")),
            }
            if !self.take(",") {
                self.expect("}")?;
                break;
            }
        }
        Ok(rect)
    }

    pub(super) fn components(&mut self, widget: &mut UiWidget) -> Result<(), String> {
        let mut seen = HashSet::new();
        loop {
            self.space();
            if self.rest.is_empty()
                || self.rest.starts_with(']')
                || self.rest.starts_with('#')
                || self.rest.starts_with("--")
                || self.rest.starts_with("Children")
            {
                break;
            }
            let name = self.word()?;
            if !seen.insert(name) {
                return Err(self.error("控件注解重复"));
            }
            match name {
                "BackgroundColor" => widget.background = Some(self.color_argument()?),
                "BorderColor" => widget.border_color = Some(self.color_argument()?),
                "TextColor" => widget.text_color = Some(self.color_argument()?),
                "UiBind" => widget.binding = self.string_argument()?,
                "UiTextShadow" => widget.text_shadow = true,
                "UiHidden" => widget.hidden = true,
                "UiTextAlign" => {
                    self.expect("(")?;
                    widget.text_justify = read_enum!(self, Justify, Left, Center, Right, Justified);
                    self.expect(")")?;
                }
                "Button" => widget.kind = WidgetKind::Button,
                "Text" => {
                    if widget.kind != WidgetKind::Button {
                        widget.kind = WidgetKind::Text;
                    }
                    widget.content = self.string_argument()?;
                }
                "UiImage" => {
                    widget.kind = WidgetKind::Image;
                    widget.content = self.string_argument()?;
                    if !widget.content.is_empty() && !valid_asset_path(&widget.content) {
                        return Err(self.error("图片需要工程内的相对资产路径"));
                    }
                }
                "UiBinding" | "UiAttribute" => {
                    widget.kind = WidgetKind::Binding;
                    widget.content = self.string_argument()?;
                }
                "UiAction" => widget.action = self.string_argument()?,
                "UiFontSize" => {
                    self.expect("(")?;
                    widget.font_size = self.number(false)?;
                    if widget.font_size == 0.0 {
                        return Err(self.error("字体大小必须大于零"));
                    }
                    self.expect(")")?;
                }
                _ => return Err(self.error(&format!("不支持的控件注解 {name}"))),
            }
        }
        if seen.contains("UiFontSize")
            && !matches!(widget.kind, WidgetKind::Text | WidgetKind::Button)
        {
            return Err(self.error("字体大小只适用于文本和按钮"));
        }
        let kinds = usize::from(seen.contains("Button") || seen.contains("Text"))
            + usize::from(seen.contains("UiImage"))
            + usize::from(seen.contains("UiBinding"))
            + usize::from(seen.contains("UiAttribute"));
        if kinds > 1 {
            return Err(self.error("一个节点只能声明一种控件类型"));
        }
        if !widget.action.is_empty() && widget.kind != WidgetKind::Button {
            return Err(self.error("动作只适用于按钮"));
        }
        Ok(())
    }

    fn color_argument(&mut self) -> Result<[u8; 4], String> {
        self.expect("(")?;
        self.expect("Color::srgba_u8(")?;
        let mut color = [0; 4];
        for (index, channel) in color.iter_mut().enumerate() {
            *channel = self
                .word()?
                .parse::<u8>()
                .map_err(|_| self.error("颜色通道需要 0 到 255"))?;
            if index != 3 {
                self.expect(",")?;
            }
        }
        self.expect("))")?;
        Ok(color)
    }
}
