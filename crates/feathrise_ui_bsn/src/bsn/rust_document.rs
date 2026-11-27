//! 把画布保存为真正的 Bevy Rust 场景；读取受支持的声明式子集，宏外源码保持原字节。

use std::ops::Range;

use syn::visit::Visit;

use super::*;

const MARKER: &str = "// feathrise-ui: editable";

/// 文件路由只检查首行，不在编辑器的每一帧重新遍历或解析整份 Rust 文件。
pub fn is_rust_ui_document(source: &str) -> bool {
    source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .strip_prefix(MARKER)
        .is_some_and(|tail| tail.is_empty() || tail.starts_with('\n') || tail.starts_with("\r\n"))
}

impl UiLayout {
    /// 旧 BSN 资产继续可读；带标记的 Rust 文件使用真正的 Rust AST 定位宏。
    pub fn parse_document(source: &str) -> Result<Self, String> {
        if !is_rust_ui_document(source) {
            return Self::parse(source);
        }
        let range = macro_body(source)?;
        Self::parse_layout(&source[range], true)
    }

    /// 输出只依赖 Bevy 的普通模块，消费工程直接调用 ui_scene 并装配业务组件。
    pub fn to_rust(&self) -> String {
        let mut output = format!("{MARKER}\n");
        output.push_str(
            "// 可直接修改 bsn! 内受支持的布局；画布保存仅替换这个宏的正文。\n\
             use bevy::prelude::*;\n\
             use bevy::{text::FontSourceTemplate, ui::CornerRadius, ui_widgets::Button};\n\n\
             // 绑定和动作是实际 ECS 元数据，消费工程按这些公开组件接入业务。\n\
             #[derive(Component, Clone)]\n\
             pub struct UiBind(pub &'static str);\n\
             #[derive(Component, Clone)]\n\
             pub struct UiAction(pub &'static str);\n\
             #[derive(Component, Clone)]\n\
             pub struct UiBinding(pub &'static str);\n\n\
             // 字体与默认颜色由工程主题传入；显式控件颜色覆盖对应默认值。\n\
             pub fn ui_scene(font: Handle<Font>, text_color: Color, button_color: Color) -> impl Scene {\n\
                 bsn! {\n",
        );
        write_widget(&mut output, &self.root, 2);
        output.push_str("    }\n}\n");
        output
    }

    /// 保存前先确认旧宏仍完全可理解，避免覆盖手写组件、表达式或多个场景。
    pub fn replace_document(&self, source: &str) -> Result<String, String> {
        if !is_rust_ui_document(source) {
            Self::parse(source)?;
            return Ok(self.to_bsn());
        }
        let range = macro_body(source)?;
        Self::parse_layout(&source[range.clone()], true)?;
        let mut body = String::from("\n");
        write_widget(&mut body, &self.root, 2);
        body.push_str("    ");
        if source.contains("\r\n") {
            body = body.replace('\n', "\r\n");
        }
        let mut result = String::with_capacity(source.len() + body.len());
        result.push_str(&source[..range.start]);
        result.push_str(&body);
        result.push_str(&source[range.end..]);
        Ok(result)
    }
}

fn macro_body(source: &str) -> Result<Range<usize>, String> {
    // syn 会移除 BOM 后再计算 span；显式补回偏移，保存仍针对原始字节串切片。
    let rust = source.strip_prefix('\u{feff}').unwrap_or(source);
    let offset = source.len() - rust.len();
    let ast = syn::parse_file(rust).map_err(|error| format!("Rust 源码有误：{error}"))?;
    struct Macros(Vec<Range<usize>>);
    impl<'ast> Visit<'ast> for Macros {
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if mac
                .path
                .segments
                .last()
                .is_some_and(|part| part.ident == "bsn")
            {
                let span = mac.delimiter.span();
                self.0
                    .push(span.open().byte_range().end..span.close().byte_range().start);
            }
            // 不解释其他宏内的 token；注释、字符串和宏定义不能冒充实际场景。
        }
    }
    let mut macros = Macros(Vec::new());
    macros.visit_file(&ast);
    match macros.0.as_slice() {
        [range] if rust.get(range.clone()).is_some() => {
            Ok(range.start + offset..range.end + offset)
        }
        [] => Err("原生界面需要一个可直接定位的 bsn! 宏；其他 Rust 文件请使用源码编辑".into()),
        _ => Err("原生界面有多个 bsn! 宏，画布无法确定目标；请把可编辑场景分成独立文件".into()),
    }
}

fn write_widget(output: &mut String, widget: &UiWidget, depth: usize) {
    let pad = "    ".repeat(depth);
    for comment in &widget.comments {
        let _ = writeln!(output, "{pad}{comment}");
    }
    let node = widget_node(widget);
    let axis = if widget.direction == FlexDirection::Row {
        "Row"
    } else {
        "Column"
    };
    let _ = write!(
        output,
        "{pad}#{} Node {{ width: {}, height: {}, flex_direction: FlexDirection::{axis}, row_gap: {}, padding: {}",
        widget.id,
        length_text(widget.width),
        length_text(widget.height),
        length_text(node.row_gap),
        style::padding_text(widget)
    );
    // 旧控件模型的默认收缩与按钮居中不同于 Bevy 默认值，导出必须明确声明实际值。
    if let Some(Vec2 { x, y }) = widget.position {
        let _ = write!(
            output,
            ", position_type: PositionType::Absolute, left: px({x:?}f32), top: px({y:?}f32)"
        );
    }
    // row_gap 已写在前面，补列间距与其余原生字段；仅借用当前节点，不克隆整棵子树。
    style::write_node_with_defaults(
        output,
        &node,
        &Node::default(),
        FlexDirection::Column,
        widget.position.is_some(),
    );
    output.push_str(" }\n");
    if let Some([r, g, b, a]) = widget.background {
        let _ = writeln!(
            output,
            "{pad}BackgroundColor(Color::srgba_u8({r}, {g}, {b}, {a}))"
        );
    } else if widget.kind == WidgetKind::Button {
        let _ = writeln!(output, "{pad}BackgroundColor(button_color)");
    }
    if let Some([r, g, b, a]) = widget.border_color {
        let _ = writeln!(
            output,
            "{pad}BorderColor::all(Color::srgba_u8({r}, {g}, {b}, {a}))"
        );
    }
    if !widget.binding.is_empty() {
        let _ = writeln!(output, "{pad}~{{UiBind({:?})}}", widget.binding);
    }
    if widget.hidden {
        let _ = writeln!(output, "{pad}Visibility::Hidden");
    }
    match widget.kind {
        WidgetKind::Container => {}
        WidgetKind::Text => write_text(output, widget, depth, true),
        WidgetKind::Image => {
            if widget.content.is_empty() {
                let _ = writeln!(output, "{pad}ImageNode {{ color: Color::NONE }}");
            } else {
                let _ = writeln!(output, "{pad}ImageNode {{ image: {:?} }}", widget.content);
            }
        }
        WidgetKind::Button => {
            let _ = writeln!(output, "{pad}Button");
            if !widget.action.is_empty() {
                let _ = writeln!(output, "{pad}~{{UiAction({:?})}}", widget.action);
            }
            // 空标签不占一个 flex 子项；保留文字属性，之后可继续在画布填写按钮内容。
            if widget.content.is_empty() {
                write_text(output, widget, depth, false);
            }
        }
        WidgetKind::Binding => {
            let _ = writeln!(output, "{pad}~{{UiBinding({:?})}}", widget.content);
        }
    }
    let label = widget.kind == WidgetKind::Button && !widget.content.is_empty();
    if label || !widget.children.is_empty() {
        let _ = writeln!(output, "{pad}Children [");
        if label {
            write_text(output, widget, depth + 1, true);
            let _ = writeln!(output, "{pad}    Pickable::IGNORE");
        }
        for (index, child) in widget.children.iter().enumerate() {
            if label || index != 0 {
                let _ = writeln!(output, "{pad}    --");
            }
            write_widget(output, child, depth + 1);
        }
        let _ = writeln!(output, "{pad}]");
    }
}

fn write_text(output: &mut String, widget: &UiWidget, depth: usize, content: bool) {
    let pad = "    ".repeat(depth);
    if content {
        let _ = writeln!(output, "{pad}Text({:?})", widget.content);
    }
    let _ = writeln!(
        output,
        "{pad}TextFont {{ font: {{FontSourceTemplate::Handle(font.clone().into())}}, font_size: px({:?}f32) }}",
        widget.font_size
    );
    if let Some([r, g, b, a]) = widget.text_color {
        let _ = writeln!(
            output,
            "{pad}TextColor(Color::srgba_u8({r}, {g}, {b}, {a}))"
        );
    } else {
        let _ = writeln!(output, "{pad}TextColor(text_color)");
    }
    let _ = writeln!(
        output,
        "{pad}TextLayout::justify(Justify::{:?})",
        widget.text_justify
    );
    if widget.text_shadow {
        let _ = writeln!(
            output,
            "{pad}TextShadow {{ offset: Vec2::ONE, color: Color::srgba_u8(0, 0, 0, 204) }}"
        );
    }
}

impl Reader<'_> {
    pub(super) fn native_components(&mut self, widget: &mut UiWidget) -> Result<bool, String> {
        let mut seen = HashSet::new();
        let mut themed_background = false;
        loop {
            self.space();
            if self.rest.is_empty()
                || self.rest.starts_with([']', '#'])
                || self.rest.starts_with("--")
                || self.rest.starts_with("Children")
            {
                break;
            }
            let metadata = self.take("~");
            if metadata {
                self.expect("{")?;
            }
            let name = self.word()?;
            if !seen.insert(name) {
                return Err(self.error("原生组件重复"));
            }
            if metadata {
                match name {
                    "UiBind" => widget.binding = self.string_argument()?,
                    "UiAction" => widget.action = self.string_argument()?,
                    "UiBinding" => {
                        widget.kind = WidgetKind::Binding;
                        widget.content = self.string_argument()?;
                    }
                    _ => {
                        return Err(
                            self.error(&format!("画布不支持元数据组件 {name}，请继续使用源码编辑"))
                        );
                    }
                }
                self.expect("}")?;
                continue;
            }
            match name {
                "BackgroundColor" => {
                    widget.background = self.themed_color("button_color")?;
                    themed_background = widget.background.is_none();
                }
                "BorderColor" => {
                    self.expect("::")?;
                    self.expect("all")?;
                    widget.border_color = Some(self.color_argument()?);
                }
                "Button" => {
                    widget.kind = WidgetKind::Button;
                    widget.content.clear();
                }
                "Text" => {
                    widget.kind = WidgetKind::Text;
                    widget.content = self.string_argument()?;
                }
                "ImageNode" => self.image_node(widget)?,
                "Visibility" => {
                    self.expect("::")?;
                    widget.hidden = match self.word()? {
                        "Hidden" => true,
                        "Inherited" => false,
                        _ => return Err(self.error("画布仅支持 Visibility::Hidden 或 Inherited")),
                    };
                }
                _ if self.text_component(widget, name)? => {}
                _ => {
                    return Err(
                        self.error(&format!("画布不支持原生组件 {name}，请继续使用源码编辑"))
                    );
                }
            }
        }
        let kinds = ["Button", "Text", "ImageNode", "UiBinding"]
            .into_iter()
            .filter(|name| seen.contains(name))
            .count();
        if kinds > 1 {
            return Err(self.error("一个可编辑节点只能声明一种控件类型"));
        }
        if widget.kind == WidgetKind::Text && !seen.contains("TextFont") {
            return Err(self.error("可编辑文本需要显式的 TextFont 与像素字号"));
        }
        // 空字符串仍代表实际 ECS 组件存在，模型不能用“没有 ID”吞掉组件后继续保存。
        for (name, id) in [("UiBind", &widget.binding), ("UiAction", &widget.action)] {
            if seen.contains(name) && id.is_empty() {
                return Err(self.error(&format!("{name} 的 ID 不能为空；请使用源码编辑保留该组件")));
            }
        }
        if seen.contains("UiAction") && widget.kind != WidgetKind::Button {
            return Err(self.error("动作元数据只适用于按钮"));
        }
        let text_styles = ["TextFont", "TextColor", "TextLayout", "TextShadow"]
            .into_iter()
            .any(|name| seen.contains(name));
        if text_styles && !matches!(widget.kind, WidgetKind::Text | WidgetKind::Button) {
            return Err(self.error("文字组件只支持文本或按钮；画布不会丢弃其他节点上的手写组件"));
        }
        if widget.kind == WidgetKind::Button
            && text_styles
            && ["TextFont", "TextColor", "TextLayout"]
                .into_iter()
                .any(|name| !seen.contains(name))
        {
            return Err(self.error("按钮本体文字样式需同时声明 TextFont、TextColor 和 TextLayout"));
        }
        if themed_background && widget.kind != WidgetKind::Button {
            return Err(self.error("button_color 默认色只适用于按钮"));
        }
        if widget.kind == WidgetKind::Button && !seen.contains("BackgroundColor") {
            widget.background = Some([0, 0, 0, 0]);
        }
        if widget.kind == WidgetKind::Text && !seen.contains("TextColor") {
            widget.text_color = Some([255, 255, 255, 255]);
        }
        Ok(widget.kind == WidgetKind::Button && text_styles)
    }

    pub(super) fn button_label(
        &mut self,
        widget: &mut UiWidget,
        text_on_parent: bool,
    ) -> Result<(), String> {
        self.space();
        if !self.take("Text") {
            return Ok(());
        }
        if text_on_parent {
            return Err(self.error("文字样式不能同时写在按钮本体和标签子实体上"));
        }
        widget.content = self.string_argument()?;
        if widget.content.is_empty() {
            return Err(self.error("空按钮请省略标签子实体；画布不能在保存时删除实际 Text 实体"));
        }
        let mut seen = HashSet::new();
        loop {
            let name = self.word()?;
            if name == "Pickable" {
                self.expect("::")?;
                self.expect("IGNORE")?;
                break;
            }
            if !seen.insert(name) || !self.text_component(widget, name)? {
                return Err(self.error("按钮标签只支持文本样式与 Pickable::IGNORE"));
            }
        }
        if !seen.contains("TextFont") {
            return Err(self.error("按钮标签需要 TextFont"));
        }
        if !seen.contains("TextColor") {
            widget.text_color = Some([255, 255, 255, 255]);
        }
        self.space();
        if !self.rest.starts_with(']') {
            self.expect("--")?;
        }
        Ok(())
    }

    fn text_component(&mut self, widget: &mut UiWidget, name: &str) -> Result<bool, String> {
        match name {
            "TextFont" => {
                self.expect("{")?;
                let mut seen = HashSet::new();
                while !self.take("}") {
                    let field = self.word()?;
                    if !seen.insert(field) {
                        return Err(self.error("TextFont 字段重复"));
                    }
                    self.expect(":")?;
                    match field {
                        "font" => {
                            for token in [
                                "{",
                                "FontSourceTemplate",
                                "::",
                                "Handle",
                                "(",
                                "font",
                                ".",
                                "clone",
                                "(",
                                ")",
                                ".",
                                "into",
                                "(",
                                ")",
                                ")",
                                "}",
                            ] {
                                self.expect(token)?;
                            }
                        }
                        "font_size" => {
                            widget.font_size = self.pixels(false)?;
                            if widget.font_size == 0.0 {
                                return Err(self.error("字体大小必须大于零"));
                            }
                        }
                        _ => return Err(self.error("画布仅支持 TextFont 的 font 与 font_size")),
                    }
                    if !self.take(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                if !seen.contains("font") || !seen.contains("font_size") {
                    return Err(self.error(
                        "可编辑 TextFont 需要 font 和 font_size；其他字体表达式请使用源码编辑",
                    ));
                }
            }
            "TextColor" => widget.text_color = self.themed_color("text_color")?,
            "TextLayout" => {
                for token in ["::", "justify", "(", "Justify", "::"] {
                    self.expect(token)?;
                }
                widget.text_justify = match self.word()? {
                    "Left" => Justify::Left,
                    "Center" => Justify::Center,
                    "Right" => Justify::Right,
                    "Justified" => Justify::Justified,
                    _ => return Err(self.error("不支持的文本对齐")),
                };
                self.expect(")")?;
            }
            "TextShadow" => {
                // 画布只有阴影开关，不能把其他偏移或颜色默默压成默认阴影。
                self.expect("{")?;
                let mut seen = HashSet::new();
                while !self.take("}") {
                    let field = self.word()?;
                    if !seen.insert(field) {
                        return Err(self.error("TextShadow 字段重复"));
                    }
                    self.expect(":")?;
                    match field {
                        "offset" => {
                            for token in ["Vec2", "::", "ONE"] {
                                self.expect(token)?;
                            }
                        }
                        "color" if self.color_literal()? == [0, 0, 0, 204] => {}
                        _ => return Err(self.error("画布阴影只支持 Vec2::ONE 与黑色 80% 不透明度")),
                    }
                    if !self.take(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                if seen.len() != 2 {
                    return Err(self.error("画布阴影需要 offset 和 color"));
                }
                widget.text_shadow = true;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn image_node(&mut self, widget: &mut UiWidget) -> Result<(), String> {
        self.expect("{")?;
        match self.word()? {
            "image" => {
                self.expect(":")?;
                widget.content = self.bare_string()?;
                if widget.content.is_empty() {
                    return Err(
                        self.error("image 路径不能为空；透明图片占位请用 color: Color::NONE")
                    );
                }
            }
            "color" => {
                for token in [":", "Color", "::", "NONE"] {
                    self.expect(token)?;
                }
                widget.content.clear();
            }
            _ => return Err(self.error("图片需要 image 相对路径，或 color: Color::NONE 透明占位")),
        }
        self.take(",");
        self.expect("}")?;
        if !widget.content.is_empty() && !valid_asset_path(&widget.content) {
            return Err(self.error("图片需要工程内的相对资产路径"));
        }
        widget.kind = WidgetKind::Image;
        Ok(())
    }

    fn themed_color(&mut self, theme: &str) -> Result<Option<[u8; 4]>, String> {
        self.expect("(")?;
        let color = if self.take(theme) {
            None
        } else {
            Some(self.color_literal()?)
        };
        self.expect(")")?;
        Ok(color)
    }

    pub(super) fn bare_string(&mut self) -> Result<String, String> {
        // 复用已有字符串转义规则；人工改成动态表达式时明确拒绝，保留原源码。
        self.space();
        let source = self.rest;
        if let Some(raw) = source.strip_prefix('r') {
            let hashes = raw.bytes().take_while(|&byte| byte == b'#').count();
            let start = hashes + 2;
            if raw.as_bytes().get(hashes) != Some(&b'"') {
                return Err(self.error("画布需要 Rust 字符串字面量"));
            }
            let ending = format!("\"{}", "#".repeat(hashes));
            let end = source[start..]
                .find(&ending)
                .map(|index| start + index + ending.len())
                .ok_or_else(|| self.error("原始字符串未结束"))?;
            let literal = syn::parse_str::<syn::LitStr>(&source[..end])
                .map_err(|error| self.error(&format!("字符串有误：{error}")))?;
            self.rest = &source[end..];
            return Ok(literal.value());
        }
        let mut chars = source.chars();
        if chars.next() != Some('"') {
            return Err(self.error("画布需要字符串字面量"));
        }
        let mut escaped = false;
        for (index, ch) in source[1..].char_indices() {
            if ch == '"' && !escaped {
                let end = index + 2;
                let literal = syn::parse_str::<syn::LitStr>(&source[..end])
                    .map_err(|error| self.error(&format!("字符串有误：{error}")))?;
                self.rest = &source[end..];
                return Ok(literal.value());
            }
            escaped = ch == '\\' && !escaped;
        }
        Err(self.error("字符串未结束"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> UiLayout {
        let mut layout = UiLayout::default();
        let mut button = UiWidget::new("close".into(), WidgetKind::Button);
        button.action = "dialog.close".into();
        button.binding = "dialog.close_button".into();
        button.text_shadow = true;
        button.text_justify = Justify::Center;
        button.text_color = Some([220, 210, 180, 255]);
        button
            .children
            .push(UiWidget::new("icon".into(), WidgetKind::Image));
        layout.root.children.push(button);
        layout
            .root
            .children
            .push(UiWidget::new("title".into(), WidgetKind::Text));
        layout
    }

    #[test]
    fn native_rust_round_trip_preserves_geometry_and_widget_properties() {
        let layout = example();
        let source = layout.to_rust();
        assert!(is_rust_ui_document(&source));
        assert!(!source.contains("UiFontSize"));
        assert!(!source.contains("UiTextShadow"));
        let parsed = UiLayout::parse_document(&source).unwrap();
        assert_eq!(parsed, layout);
        for id in ["root", "close", "icon", "title"] {
            let original = layout.root.find(id).unwrap();
            let actual = parsed.root.find(id).unwrap();
            assert_eq!(widget_node(actual), widget_node(original), "{id}");
            assert_eq!(actual.kind, original.kind);
            assert_eq!(actual.content, original.content);
            assert_eq!(actual.binding, original.binding);
            assert_eq!(actual.action, original.action);
            assert_eq!(actual.text_color, original.text_color);
            assert_eq!(actual.text_shadow, original.text_shadow);
            assert_eq!(actual.text_justify, original.text_justify);
        }
        let saved = parsed.replace_document(&source).unwrap();
        assert_eq!(UiLayout::parse_document(&saved).unwrap(), parsed);
    }

    #[test]
    fn new_widgets_pass_the_editors_exact_model_save_preflight() {
        let source = UiLayout::default().to_rust();
        // 新增动作先有控件默认模型，再通过相同文档写回；不能只核对视觉几何等价。
        for kind in [
            WidgetKind::Binding,
            WidgetKind::Container,
            WidgetKind::Text,
            WidgetKind::Image,
            WidgetKind::Button,
        ] {
            let mut layout = UiLayout::parse_document(&source).unwrap();
            layout
                .root
                .children
                .push(UiWidget::new("widget_1".into(), kind));
            let written = layout.replace_document(&source).unwrap();
            assert_eq!(
                UiLayout::parse_document(&written).unwrap(),
                layout,
                "{kind:?}"
            );
            if kind == WidgetKind::Button {
                layout.root.children[0].content.clear();
                let written = layout.replace_document(&source).unwrap();
                assert_eq!(
                    UiLayout::parse_document(&written).unwrap(),
                    layout,
                    "空文字按钮"
                );
            }
        }
    }

    #[test]
    fn save_keeps_outer_rust_bytes_and_ignores_macro_text_in_strings() {
        let original = example().to_rust().replace(
            "pub fn ui_scene",
            "// bsn! { 注释不是宏 }\nconst NOTE: &str = r###\"bsn! { 字符串不是宏 }\"###;\n\npub fn ui_scene",
        );
        let original = format!(
            "\u{feff}{}\r\n// 宏外中文保留\r\n",
            original.replace('\n', "\r\n")
        );
        let range = macro_body(&original).unwrap();
        let before = &original[..range.start];
        let after = &original[range.end..];
        let mut layout = UiLayout::parse_document(&original).unwrap();
        layout.root.find_mut("title").unwrap().content = "代码与画布共用文本".into();
        let saved = layout.replace_document(&original).unwrap();
        assert!(saved.starts_with(before));
        assert!(saved.ends_with(after));
        assert!(!saved.replace("\r\n", "").contains('\n'));
        assert_eq!(
            UiLayout::parse_document(&saved)
                .unwrap()
                .root
                .find("title")
                .unwrap()
                .content,
            "代码与画布共用文本"
        );
    }

    #[test]
    fn unsupported_handwritten_rust_is_diagnosed_before_save() {
        let layout = example();
        let original = layout.to_rust();
        for source in [
            original.replace("#root Node", "#root UnknownComponent Node"),
            original.replace("font.clone().into()", "custom_font.clone().into()"),
            original.replace("px(14.0f32)", "px(TEXT_SIZE)"),
            original.replace("Vec2::ONE", "Vec2::splat(3.0)"),
            format!("{original}\nfn other() -> impl Scene {{ bsn! {{ Node {{}} }} }}"),
        ] {
            assert!(UiLayout::parse_document(&source).is_err(), "{source}");
            assert!(layout.replace_document(&source).is_err());
        }
    }

    #[test]
    fn unsupported_component_combinations_cannot_be_silently_discarded() {
        let layout = example();
        let original = layout.to_rust();
        for component in [
            "TextFont { font: {FontSourceTemplate::Handle(font.clone().into())}, font_size: px(15) }",
            "TextColor(Color::srgba_u8(1, 2, 3, 255))",
            "TextLayout::justify(Justify::Center)",
            "TextShadow { offset: Vec2::ONE, color: Color::srgba_u8(0, 0, 0, 204) }",
            "BackgroundColor(button_color)",
        ] {
            for kind in [
                "",
                "ImageNode { color: Color::NONE }",
                "~{UiBinding(\"value\")}",
            ] {
                let source = format!(
                    "{MARKER}\nfn scene() -> impl Scene {{ bsn! {{ #root Node {{}} Children [ #item Node {{}} {kind} {component} ] }} }}"
                );
                assert!(
                    UiLayout::parse_document(&source).is_err(),
                    "{component}, {kind}"
                );
                assert!(layout.replace_document(&source).is_err());
            }
        }
        let duplicated = original.replace(
            "Button\n",
            "Button\n TextColor(Color::srgba_u8(1, 2, 3, 255))\n",
        );
        assert!(layout.replace_document(&duplicated).is_err());
    }

    #[test]
    fn legacy_bsn_stays_compatible_and_marker_is_only_checked_on_first_line() {
        let layout = example();
        assert_eq!(UiLayout::parse_document(&layout.to_bsn()).unwrap(), layout);
        assert!(!is_rust_ui_document(
            "// 普通 Rust\n// feathrise-ui: editable\n"
        ));
        assert!(is_rust_ui_document("\u{feff}// feathrise-ui: editable\r\n"));
    }

    #[test]
    fn handwritten_rust_whitespace_raw_strings_and_block_comments_remain_editable() {
        let original = example()
            .to_rust()
            .replace(
                "Text(\"文本\")",
                "/* 外层 /* 内层 */ 注释 */ Text(r##\"代码里的 // 文本\"##)",
            )
            .replace("font.clone().into()", "font . clone ( ) . into ( )")
            .replace("Color::srgba_u8", "Color :: srgba_u8")
            .replace("Vec2::ONE", "Vec2 :: ONE")
            .replace("Pickable::IGNORE", "Pickable :: IGNORE");
        let layout = UiLayout::parse_document(&original).unwrap();
        assert_eq!(
            layout.root.find("title").unwrap().content,
            "代码里的 // 文本"
        );
        let saved = layout.replace_document(&original).unwrap();
        assert!(saved.contains("/* 外层 /* 内层 */ 注释 */"));
        assert_eq!(UiLayout::parse_document(&saved).unwrap(), layout);
    }

    #[test]
    fn native_single_axis_gaps_keep_bevy_defaults_and_legacy_assets_keep_their_spacing() {
        for direction in ["Row", "Column"] {
            for (field, row, column) in [
                ("row_gap: px(10)", Val::Px(10.0), Val::ZERO),
                ("column_gap: px(10)", Val::ZERO, Val::Px(10.0)),
                ("", Val::ZERO, Val::ZERO),
            ] {
                let body =
                    format!("#root Node {{ flex_direction: FlexDirection::{direction}, {field} }}");
                let source = format!("{MARKER}\nfn scene() -> impl Scene {{ bsn! {{ {body} }} }}");
                let layout = UiLayout::parse_document(&source).unwrap();
                let node = widget_node(&layout.root);
                assert_eq!(
                    (node.row_gap, node.column_gap),
                    (row, column),
                    "{direction}, {field}"
                );
                let saved = layout.replace_document(&source).unwrap();
                assert_eq!(UiLayout::parse_document(&saved).unwrap(), layout);

                let legacy = widget_node(&UiLayout::parse(&body).unwrap().root);
                let gap = if field.is_empty() {
                    Val::ZERO
                } else {
                    Val::Px(10.0)
                };
                assert_eq!(
                    (legacy.row_gap, legacy.column_gap),
                    (gap, gap),
                    "旧资产 {direction}, {field}"
                );
            }
        }
        let source = format!(
            "{MARKER}\nfn scene() -> impl Scene {{ bsn! {{ #root Node {{ row_gap: px(10) }} }} }}"
        );
        let node = widget_node(&UiLayout::parse_document(&source).unwrap().root);
        assert_eq!(node.flex_direction, FlexDirection::Row);
        assert_eq!((node.row_gap, node.column_gap), (Val::Px(10.0), Val::ZERO));
    }

    #[test]
    fn native_unnamed_roots_do_not_inherit_legacy_binding_migration() {
        let body = "Node {} Children [ #health Node {} -- #physical_attack Node {} ]";
        let source = format!("{MARKER}\nfn scene() -> impl Scene {{ bsn! {{ {body} }} }}");
        assert!(
            UiLayout::parse_document(&source)
                .unwrap_err()
                .contains("显式 #id")
        );
        assert!(UiLayout::default().replace_document(&source).is_err());
        let named = source.replace("bsn! { Node", "bsn! { #root Node");
        let layout = UiLayout::parse_document(&named).unwrap();
        assert_eq!(layout.root.width, Val::Auto);
        assert!(
            layout
                .root
                .children
                .iter()
                .all(|child| child.kind == WidgetKind::Container && child.content.is_empty())
        );
        assert_eq!(
            UiLayout::parse_document(&layout.replace_document(&named).unwrap()).unwrap(),
            layout
        );

        let legacy = UiLayout::parse(body).unwrap();
        assert_eq!(legacy.root.width, Val::Percent(100.0));
        for child in &legacy.root.children {
            assert_eq!(child.kind, WidgetKind::Binding);
            assert_eq!(child.content, child.id);
        }
    }

    #[test]
    fn empty_metadata_ids_are_rejected_but_binding_placeholders_remain_editable() {
        for kind in ["", "Button"] {
            for component in ["UiBind", "UiAction"] {
                let source = format!(
                    "{MARKER}\nfn scene() -> impl Scene {{ bsn! {{ #root Node {{}} Children [ #item Node {{}} {kind} ~{{{component}(\"\")}} ] }} }}"
                );
                assert!(
                    UiLayout::parse_document(&source)
                        .unwrap_err()
                        .contains("不能为空")
                );
                assert!(UiLayout::default().replace_document(&source).is_err());
            }
        }
        let source = format!(
            "{MARKER}\nfn scene() -> impl Scene {{ bsn! {{ #root Node {{}} Children [ #item Node {{}} ~{{UiBinding(\"\")}} ] }} }}"
        );
        let layout = UiLayout::parse_document(&source).unwrap();
        let saved = layout.replace_document(&source).unwrap();
        assert!(saved.contains("~{UiBinding(\"\")}"));
        assert_eq!(UiLayout::parse_document(&saved).unwrap(), layout);
    }

    #[test]
    fn native_empty_image_paths_cannot_turn_into_transparent_placeholders_on_save() {
        let source = format!(
            "{MARKER}\nfn scene() -> impl Scene {{ bsn! {{ #root Node {{}} Children [ #image Node {{}} ImageNode {{ image: \"\" }} ] }} }}"
        );
        assert!(
            UiLayout::parse_document(&source)
                .unwrap_err()
                .contains("image 路径不能为空")
        );
        assert!(UiLayout::default().replace_document(&source).is_err());
        let placeholder = source.replace("image: \"\"", "color: Color::NONE");
        let layout = UiLayout::parse_document(&placeholder).unwrap();
        let saved = layout.replace_document(&placeholder).unwrap();
        assert!(saved.contains("ImageNode { color: Color::NONE }"));
        assert_eq!(UiLayout::parse_document(&saved).unwrap(), layout);
    }

    #[test]
    fn native_empty_buttons_and_labels_preserve_actual_text_component_presence() {
        let font = "TextFont { font: {FontSourceTemplate::Handle(font.clone().into())}, font_size: px(14) }";
        let color = "TextColor(text_color)";
        let alignment = "TextLayout::justify(Justify::Left)";
        let source_for = |entry: &str| {
            format!(
                "{MARKER}\nfn scene() -> impl Scene {{ bsn! {{ #root Node {{}} Children [ #button Node {{}} {entry} ] }} }}"
            )
        };
        for styles in [
            "",
            font,
            color,
            alignment,
            "TextShadow { offset: Vec2::ONE, color: Color::srgba_u8(0, 0, 0, 204) }",
        ] {
            let source = source_for(&format!("Button {styles}"));
            assert!(UiLayout::parse_document(&source).is_err(), "{styles}");
            assert!(UiLayout::default().replace_document(&source).is_err());
        }
        let empty_label = source_for(&format!(
            "Button Children [ Text(\"\") {font} Pickable::IGNORE ]"
        ));
        assert!(
            UiLayout::parse_document(&empty_label)
                .unwrap_err()
                .contains("空按钮请省略标签子实体")
        );
        assert!(UiLayout::default().replace_document(&empty_label).is_err());
        for entry in [
            format!("Button {font} {color} {alignment}"),
            format!("Button Children [ Text(\"确认\") {font} Pickable::IGNORE ]"),
        ] {
            let source = source_for(&entry);
            let layout = UiLayout::parse_document(&source).unwrap();
            let saved = layout.replace_document(&source).unwrap();
            assert_eq!(UiLayout::parse_document(&saved).unwrap(), layout);
        }
    }
}
