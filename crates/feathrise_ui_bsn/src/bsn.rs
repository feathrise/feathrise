//! 解析并写出项目支持的 BSN 控件子集。只识别声明式字段，保留已保存的早期布局。

use std::{collections::HashSet, fmt::Write as _};

use super::*;

impl UiLayout {
    pub fn parse(source: &str) -> Result<Self, String> {
        let source = source.trim_start_matches('\u{feff}').trim_start();
        let mut reader = Reader {
            source,
            rest: source,
            ids: HashSet::new(),
            comments: Vec::new(),
        };
        reader.space();
        let unnamed_root = reader.rest.starts_with("Node");
        let mut root = reader.widget(0, unnamed_root)?;
        // 早期两路绑定以节点名表达，转成普通绑定后另存仍保留原数据语义。
        if unnamed_root
            && root.children.len() == 2
            && root.children.iter().all(|child| {
                child.kind == WidgetKind::Container
                    && child.children.is_empty()
                    && matches!(child.id.as_str(), "health" | "physical_attack")
            })
        {
            if root.width == Val::Auto {
                root.width = Val::Percent(100.0);
            }
            for child in &mut root.children {
                child.kind = WidgetKind::Binding;
                child.content.clone_from(&child.id);
            }
        }
        reader.space();
        if !reader.rest.is_empty() || root.kind != WidgetKind::Container || root.position.is_some()
        {
            return Err(reader.error("需要一个相对布局的根容器"));
        }
        root.comments.append(&mut reader.comments);
        Ok(Self { root })
    }

    pub fn to_bsn(&self) -> String {
        let mut source = String::new();
        write_widget(&mut source, &self.root, 0);
        source
    }
}

fn write_widget(output: &mut String, widget: &UiWidget, depth: usize) {
    let pad = "    ".repeat(depth);
    for comment in &widget.comments {
        let _ = writeln!(output, "{pad}{comment}");
    }
    let axis = if widget.direction == FlexDirection::Row {
        "Row"
    } else {
        "Column"
    };
    let gap = if widget.direction == FlexDirection::Row {
        "column_gap"
    } else {
        "row_gap"
    };
    let _ = write!(
        output,
        "{pad}#{} Node {{ width: {}, height: {}, flex_direction: FlexDirection::{axis}, {gap}: px({:?}f32), padding: UiRect::all(px({:?}f32))",
        widget.id,
        length_text(widget.width),
        length_text(widget.height),
        widget.gap,
        widget.padding
    );
    if let Some(Vec2 { x, y }) = widget.position {
        let _ = write!(
            output,
            ", position_type: PositionType::Absolute, left: px({x:?}f32), top: px({y:?}f32)"
        );
    }
    let _ = write!(output, " }}");
    if let Some([r, g, b, a]) = widget.background {
        let _ = write!(
            output,
            " BackgroundColor(Color::srgba_u8({r}, {g}, {b}, {a}))"
        );
    }
    match widget.kind {
        WidgetKind::Container => {}
        WidgetKind::Text => {
            let _ = write!(output, " Text({:?})", widget.content);
        }
        WidgetKind::Image => {
            let _ = write!(output, " UiImage({:?})", widget.content);
        }
        WidgetKind::Button => {
            let _ = write!(output, " Button Text({:?})", widget.content);
            if !widget.action.is_empty() {
                let _ = write!(output, " UiAction({:?})", widget.action);
            }
        }
        WidgetKind::Binding => {
            let _ = write!(output, " UiBinding({:?})", widget.content);
        }
    }
    if matches!(widget.kind, WidgetKind::Text | WidgetKind::Button) {
        let _ = write!(output, " UiFontSize({:?}f32)", widget.font_size);
    }
    output.push('\n');
    if !widget.children.is_empty() {
        let _ = writeln!(output, "{pad}Children [");
        for (index, child) in widget.children.iter().enumerate() {
            if index != 0 {
                let _ = writeln!(output, "{pad}    --");
            }
            write_widget(output, child, depth + 1);
        }
        let _ = writeln!(output, "{pad}]");
    }
}

fn length_text(length: Val) -> String {
    match length {
        Val::Px(value) => format!("px({value:?}f32)"),
        Val::Percent(value) => format!("percent({value:?}f32)"),
        _ => "Val::Auto".into(),
    }
}

struct Reader<'a> {
    source: &'a str,
    rest: &'a str,
    ids: HashSet<&'a str>,
    comments: Vec<String>,
}

impl<'a> Reader<'a> {
    fn widget(&mut self, depth: usize, unnamed_root: bool) -> Result<UiWidget, String> {
        // 文件可来自外部。限定递归深度，防止极端嵌套导致解析或渲染栈溢出。
        if depth > 96 {
            return Err(self.error("界面嵌套过深"));
        }
        let id = if unnamed_root {
            "root"
        } else {
            self.expect("#")?;
            self.word()?
        };
        if !id.as_bytes()[0].is_ascii_alphabetic() && !id.starts_with('_') {
            return Err(self.error("控件 ID 必须以字母或下划线开头"));
        }
        if !self.ids.insert(id) {
            return Err(self.error("控件 ID 重复"));
        }
        let mut widget = UiWidget::new(id.into(), WidgetKind::Container);
        self.expect("Node")?;
        self.expect("{")?;
        let (mut left, mut top, mut absolute) = (None, None, false);
        let mut fields = HashSet::new();
        while !self.take("}") {
            let key = self.word()?;
            if !fields.insert(key) {
                return Err(self.error("Node 字段重复"));
            }
            self.expect(":")?;
            match key {
                "width" => widget.width = self.length()?,
                "height" => widget.height = self.length()?,
                "flex_direction" => {
                    self.expect("FlexDirection")?;
                    self.expect("::")?;
                    widget.direction = match self.word()? {
                        "Row" => FlexDirection::Row,
                        "Column" => FlexDirection::Column,
                        _ => return Err(self.error("布局方向需要 Row 或 Column")),
                    };
                }
                "row_gap" | "column_gap" => {
                    if fields.contains("row_gap") && fields.contains("column_gap") {
                        return Err(self.error("同一容器只设置一种间距"));
                    }
                    widget.gap = self.pixels(false)?;
                }
                "padding" => {
                    self.expect("UiRect::all(")?;
                    widget.padding = self.pixels(false)?;
                    self.expect(")")?;
                }
                "position_type" => {
                    self.expect("PositionType::Absolute")?;
                    absolute = true;
                }
                "left" => left = Some(self.pixels(true)?),
                "top" => top = Some(self.pixels(true)?),
                _ => return Err(self.error(&format!("不支持的 Node 字段 {key}"))),
            }
            if !self.take(",") {
                self.expect("}")?;
                break;
            }
        }
        match (absolute, left, top) {
            (true, Some(x), Some(y)) => widget.position = Some(Vec2::new(x, y)),
            (false, None, None) => {}
            _ => return Err(self.error("绝对定位需要成对的 left 和 top")),
        }
        if self.take("BackgroundColor(") {
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
            widget.background = Some(color);
        }
        if self.take("Button") {
            widget.kind = WidgetKind::Button;
            widget.content = self.component_string("Text")?;
            if self.take("UiAction") {
                widget.action = self.string_argument()?;
            }
        } else if self.take("Text") {
            widget.kind = WidgetKind::Text;
            widget.content = self.string_argument()?;
        } else if self.take("UiImage") {
            widget.kind = WidgetKind::Image;
            widget.content = self.string_argument()?;
            if !widget.content.is_empty() && !valid_asset_path(&widget.content) {
                return Err(self.error("图片需要工程内的相对资产路径"));
            }
        } else if self.take("UiBinding") || self.take("UiAttribute") {
            widget.kind = WidgetKind::Binding;
            widget.content = self.string_argument()?;
        }
        if self.take("UiFontSize(") {
            if !matches!(widget.kind, WidgetKind::Text | WidgetKind::Button) {
                return Err(self.error("字体大小只适用于文本和按钮"));
            }
            widget.font_size = self.number(false)?;
            if widget.font_size == 0.0 {
                return Err(self.error("字体大小必须大于零"));
            }
            self.expect(")")?;
        }
        let has_children = self.take("Children");
        // 分隔处及字段中的行注释随当前控件重排，字符串中的 // 仍由字符串解析器读取。
        widget.comments = std::mem::take(&mut self.comments);
        if has_children {
            if widget.kind != WidgetKind::Container {
                return Err(self.error("只有容器可以拥有子控件"));
            }
            self.expect("[")?;
            if !self.take("]") {
                loop {
                    widget.children.push(self.widget(depth + 1, false)?);
                    if self.take("]") {
                        break;
                    }
                    self.expect("--")?;
                }
            }
        }
        Ok(widget)
    }

    fn length(&mut self) -> Result<Val, String> {
        if self.take("Val::Auto") {
            return Ok(Val::Auto);
        }
        if self.take("percent(") {
            let value = self.number(false)?;
            self.expect(")")?;
            return Ok(Val::Percent(value));
        }
        self.pixels(false).map(Val::Px)
    }

    fn pixels(&mut self, signed: bool) -> Result<f32, String> {
        self.expect("px")?;
        self.expect("(")?;
        let value = self.number(signed)?;
        self.expect(")")?;
        Ok(value)
    }

    fn number(&mut self, signed: bool) -> Result<f32, String> {
        self.space();
        let end = self
            .rest
            .find(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '.' | '-' | '+'))
            .unwrap_or(self.rest.len());
        let (token, rest) = self.rest.split_at(end);
        let value = token
            .strip_suffix("f32")
            .unwrap_or(token)
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite() && (signed || *value >= 0.0))
            .ok_or_else(|| self.error("需要有限数值"))?;
        self.rest = rest;
        Ok(value)
    }

    fn component_string(&mut self, component: &str) -> Result<String, String> {
        self.expect(component)?;
        self.string_argument()
    }

    fn string_argument(&mut self) -> Result<String, String> {
        self.expect("(")?;
        self.expect("\"")?;
        let mut text = String::new();
        let mut chars = self.rest.chars();
        while let Some(ch) = chars.next() {
            if ch == '"' {
                self.rest = chars.as_str();
                self.expect(")")?;
                return Ok(text);
            }
            text.push(if ch == '\\' {
                match chars.next() {
                    Some('"') => '"',
                    Some('\\') => '\\',
                    Some('n') => '\n',
                    Some('r') => '\r',
                    Some('t') => '\t',
                    Some('0') => '\0',
                    Some('u') if chars.next() == Some('{') => {
                        let (hex, tail) = chars
                            .as_str()
                            .split_once('}')
                            .ok_or_else(|| self.error("Unicode 转义未结束"))?;
                        chars = tail.chars();
                        u32::from_str_radix(hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| self.error("无效的 Unicode 转义"))?
                    }
                    _ => return Err(self.error("不支持的文本转义")),
                }
            } else {
                ch
            });
        }
        Err(self.error("文本引号未结束"))
    }

    fn space(&mut self) {
        self.rest = self.rest.trim_start();
        while self.rest.starts_with("//") {
            let end = self.rest.find('\n').unwrap_or(self.rest.len());
            self.comments
                .push(self.rest[..end].trim_end_matches('\r').to_owned());
            self.rest = self.rest[end..].trim_start();
        }
    }

    fn word(&mut self) -> Result<&'a str, String> {
        self.space();
        let end = self
            .rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(self.rest.len());
        if end == 0 {
            return Err(self.error("需要名称"));
        }
        let (word, rest) = self.rest.split_at(end);
        self.rest = rest;
        Ok(word)
    }

    fn take(&mut self, token: &str) -> bool {
        self.space();
        let Some(rest) = self.rest.strip_prefix(token) else {
            return false;
        };
        self.rest = rest;
        true
    }

    fn expect(&mut self, token: &str) -> Result<(), String> {
        self.take(token)
            .then_some(())
            .ok_or_else(|| self.error(&format!("需要 {token}")))
    }

    fn error(&self, reason: &str) -> String {
        let offset = self.source.len() - self.rest.len();
        let line = self.source.as_bytes()[..offset]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count()
            + 1;
        format!("界面 BSN 第 {line} 行有误，{reason}")
    }
}

pub fn valid_asset_path(value: &str) -> bool {
    !value.contains(['\\', ':', '#', '\0'])
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_layout_round_trips_text_and_styles() {
        let mut layout = UiLayout::default();
        let mut group = UiWidget::new("group".into(), WidgetKind::Container);
        group.position = Some(Vec2::new(-14.0, 20.0));
        group.background = Some([51, 72, 20, 255]);
        let mut button = UiWidget::new("close".into(), WidgetKind::Button);
        button.content = "关闭 \"角色\"\n下一行".into();
        button.action = "rpg.close_character".into();
        group.children.push(button);
        layout.root.children.push(group);
        let bsn = layout.to_bsn();
        assert_eq!(UiLayout::parse(&bsn).unwrap(), layout);
        assert!(UiLayout::parse(&bsn.replace("#close", "#group")).is_err());
    }

    #[test]
    fn visual_edits_keep_comments_with_their_widgets() {
        let original = r#"// 标题说明
        #root Node { width: percent(100) }
        Children [
            // 跟随文字
            #title Node { height: px(24) } Text("地址 // 仍是文本")
            --
            #next Node { height: px(24) }
        ]
        // 文件末尾说明
        "#;
        let mut layout = UiLayout::parse(original).unwrap();
        layout.root.children.swap(0, 1);
        let saved = layout.to_bsn();
        assert!(saved.contains("// 标题说明"));
        assert!(saved.contains("// 文件末尾说明"));
        assert!(saved.contains("    // 跟随文字\n    #title"));
        assert!(saved.contains(r#"Text("地址 // 仍是文本")"#));
        assert_eq!(UiLayout::parse(&saved).unwrap(), layout);
    }

    #[test]
    fn legacy_character_file_keeps_actual_geometry() {
        let old = r#"Node { flex_direction: FlexDirection :: Column, row_gap: px (13.0f32) }
            Children [
                #physical_attack Node { height: px(31.0f32) }
                --
                #health Node { height: px(24.0f32) }
            ]"#;
        let layout = UiLayout::parse(old).unwrap();
        assert_eq!(layout.root.children[0].id, "physical_attack");
        assert_eq!(layout.root.children[0].kind, WidgetKind::Binding);
        assert_eq!(layout.root.children[0].height, Val::Px(31.0));
        assert_eq!(layout.root.gap, 13.0);
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }
}
