//! 解析并写出项目支持的 BSN 控件子集。只识别声明式字段，保留已保存的早期布局。

use std::{collections::HashSet, fmt::Write as _};

use super::*;

mod style;
pub(crate) mod rust_document;

impl UiLayout {
    pub fn parse(source: &str) -> Result<Self, String> {
        Self::parse_layout(source, false)
    }

    fn parse_layout(source: &str, native: bool) -> Result<Self, String> {
        let source = source.trim_start_matches('\u{feff}').trim_start();
        let mut reader = Reader {
            source,
            rest: source,
            ids: HashSet::new(),
            comments: Vec::new(),
            native,
        };
        reader.space();
        let unnamed_root = reader.rest.starts_with("Node");
        if native && unnamed_root {
            return Err(reader.error("原生界面根需要显式 #id，避免保存时新增 Name 组件"));
        }
        let mut root = reader.widget(0, unnamed_root)?;
        // 只有旧资产按节点名迁移两路绑定；原生 Rust 的同名节点仍是普通容器。
        if !native
            && unnamed_root
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
        if !reader.rest.is_empty()
            || root.kind != WidgetKind::Container
            || root.position.is_some()
            || root.style.position_type == PositionType::Absolute
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
        "{pad}#{} Node {{ width: {}, height: {}, flex_direction: FlexDirection::{axis}, {gap}: px({:?}f32), padding: {}",
        widget.id,
        length_text(widget.width),
        length_text(widget.height),
        widget.gap,
        style::padding_text(widget)
    );
    if let Some(Vec2 { x, y }) = widget.position {
        let _ = write!(
            output,
            ", position_type: PositionType::Absolute, left: px({x:?}f32), top: px({y:?}f32)"
        );
    }
    style::write_node(output, widget);
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
    style::write_components(output, widget);
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
    native: bool,
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
        if self.native {
            widget.direction = Node::default().flex_direction;
        }
        self.expect("Node")?;
        self.expect("{")?;
        let (mut left, mut top, mut absolute) = (None, None, false);
        let (mut row_gap, mut column_gap) = (None, None);
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
                "row_gap" => row_gap = Some(self.pixels(false)?),
                "column_gap" => column_gap = Some(self.pixels(false)?),
                "padding" => {
                    if self.take("UiRect::all(") {
                        widget.padding = self.pixels(false)?;
                        self.expect(")")?;
                    } else {
                        widget.style.padding = self.rectangle(false)?;
                    }
                }
                "position_type" => {
                    self.expect("PositionType::")?;
                    absolute = match self.word()? {
                        "Absolute" => true,
                        "Relative" => false,
                        _ => return Err(self.error("定位类型需要 Absolute 或 Relative")),
                    };
                }
                "left" => left = Some(self.length_with_sign(true)?),
                "top" => top = Some(self.length_with_sign(true)?),
                _ => self.style_field(&mut widget.style, key)?,
            }
            if !self.take(",") {
                self.expect("}")?;
                break;
            }
        }
        if self.native {
            // 原生 Node 的两轴间距独立，省略轴保持 Bevy 的零值；旧资产继续
            // 兼容单个 gap 同时设置两轴的历史约定，不能把该约定带入 Rust 源码。
            row_gap.get_or_insert(0.0);
            column_gap.get_or_insert(0.0);
        }
        widget.gap = if widget.direction == FlexDirection::Row {
            column_gap.or(row_gap)
        } else {
            row_gap.or(column_gap)
        }
        .unwrap_or(0.0);
        if let Some(gap) = row_gap.filter(|gap| *gap != widget.gap) {
            widget.style.row_gap = Val::Px(gap);
        }
        if let Some(gap) = column_gap.filter(|gap| *gap != widget.gap) {
            widget.style.column_gap = Val::Px(gap);
        }
        match (absolute, left, top) {
            // 保留旧像素坐标的模型形式，编辑器已有拖动与 X/Y 属性继续兼容。
            (true, Some(Val::Px(x)), Some(Val::Px(y)))
                if widget.style.right == Val::Auto && widget.style.bottom == Val::Auto =>
            {
                widget.position = Some(Vec2::new(x, y));
            }
            (true, x, y) => {
                widget.style.position_type = PositionType::Absolute;
                widget.style.left = x.unwrap_or(Val::Auto);
                widget.style.top = y.unwrap_or(Val::Auto);
            }
            (false, None, None)
                if widget.style.right == Val::Auto && widget.style.bottom == Val::Auto => {}
            _ => return Err(self.error("定位偏移需要 PositionType::Absolute")),
        }
        let button_text_on_parent = self.components(&mut widget)?;
        // 控件类型在 Node 后声明；只补齐未显式指定的类型默认，不覆盖作者约束。
        let defaults = if self.native {
            Node::default()
        } else {
            UiWidget::default_style(widget.kind)
        };
        if !fields.contains("min_width") {
            widget.style.min_width = defaults.min_width;
        }
        if !fields.contains("flex_shrink") {
            widget.style.flex_shrink = defaults.flex_shrink;
        }
        if !fields.contains("justify_content") {
            widget.style.justify_content = defaults.justify_content;
        }
        if !fields.contains("align_items") {
            widget.style.align_items = defaults.align_items;
        }
        if self.native {
            // 原生省略项采用 Bevy 默认；不能再次套用旧资产中按钮/绑定的居中补齐。
            if matches!(widget.kind, WidgetKind::Button | WidgetKind::Binding)
                && widget.style.justify_content == JustifyContent::Default
            {
                widget.style.justify_content = JustifyContent::FlexStart;
            }
            if widget.kind == WidgetKind::Button && widget.style.align_items == AlignItems::Default {
                widget.style.align_items = AlignItems::Stretch;
            }
        }
        let has_children = self.take("Children");
        // 分隔处及字段中的行注释随当前控件重排，字符串中的 // 仍由字符串解析器读取。
        widget.comments = std::mem::take(&mut self.comments);
        if has_children {
            if !matches!(widget.kind, WidgetKind::Container | WidgetKind::Button) {
                return Err(self.error("只有容器和按钮可以拥有子控件"));
            }
            self.expect("[")?;
            // 原生按钮的文字是真实子实体；画布把这个无 ID 的标签折回按钮属性。
            if self.native && widget.kind == WidgetKind::Button {
                self.button_label(&mut widget, button_text_on_parent)?;
            }
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
        // 空按钮没有 Text 子实体提供 required 文字组件；原生文件必须显式拥有
        // 导出器保留的三项文字样式，不能在保存时悄悄为实体新增组件。
        if self.native && widget.kind == WidgetKind::Button && widget.content.is_empty()
            && !button_text_on_parent
        {
            return Err(self.error("空按钮需显式 TextFont、TextColor 和 TextLayout；否则请使用源码编辑"));
        }
        Ok(widget)
    }

    fn length(&mut self) -> Result<Val, String> {
        self.length_with_sign(false)
    }

    fn length_with_sign(&mut self, signed: bool) -> Result<Val, String> {
        if self.take("Val::Auto") {
            return Ok(Val::Auto);
        }
        if self.take("percent(") {
            let value = self.number(signed)?;
            self.expect(")")?;
            return Ok(Val::Percent(value));
        }
        self.pixels(signed).map(Val::Px)
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

    fn string_argument(&mut self) -> Result<String, String> {
        self.expect("(")?;
        if self.native {
            let text = self.bare_string()?;
            self.expect(")")?;
            return Ok(text);
        }
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
        loop {
            let end = if self.rest.starts_with("//") {
                self.rest.find('\n').unwrap_or(self.rest.len())
            } else if self.rest.starts_with("/*") {
                // Rust 允许嵌套块注释；保留完整原文，不能在第一个 */ 提前截断。
                let bytes = self.rest.as_bytes();
                let (mut depth, mut index) = (1usize, 2);
                while depth != 0 && index + 1 < bytes.len() {
                    match &bytes[index..index + 2] {
                        b"/*" => { depth += 1; index += 2; }
                        b"*/" => { depth -= 1; index += 2; }
                        _ => index += 1,
                    }
                }
                if depth != 0 { break; }
                index
            } else {
                break;
            };
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

    #[test]
    fn anchored_hud_keeps_styles_bindings_and_button_children_when_edited() {
        let source = r#"#root Node { width: percent(100), height: percent(100) }
        Children [
            #bar Node { width: px(426), height: px(120), position_type: PositionType::Absolute,
                left: percent(50), bottom: px(14), margin: UiRect::left(px(-213)),
                padding: UiRect::axes(px(14), px(10)), border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(7)), align_items: AlignItems::Center,
                justify_content: JustifyContent::Center, flex_wrap: FlexWrap::Wrap,
                flex_grow: 1, flex_shrink: 1, min_width: px(200), max_height: px(200), overflow: Overflow::clip() }
            BorderColor(Color::srgba_u8(190, 135, 62, 122)) BackgroundColor(Color::srgba_u8(11, 9, 8, 148))
            Children [
                #skill Node { width: px(40), height: px(40) } UiBind("skill.0.button") Button Text("")
                Children [
                    #icon Node { width: percent(100), height: percent(100) } UiImage("") UiBind("skill.0.icon")
                    --
                    #key Node { position_type: PositionType::Absolute, right: px(3), bottom: px(1) }
                    Text("1") UiBind("skill.0.key") UiFontSize(8) TextColor(Color::srgba_u8(235, 226, 205, 235))
                    UiTextShadow UiHidden UiTextAlign(Justify::Center)
                ]
            ]
        ]"#;
        let mut layout = UiLayout::parse(source).unwrap();
        let node = widget_node(layout.root.find("bar").unwrap());
        assert_eq!(node.left, Val::Percent(50.0));
        assert_eq!(node.bottom, Val::Px(14.0));
        assert_eq!(node.margin.left, Val::Px(-213.0));
        assert_eq!(node.padding, UiRect::axes(Val::Px(14.0), Val::Px(10.0)));
        assert_eq!(node.border_radius, BorderRadius::all(Val::Px(7.0)));
        assert_eq!(node.flex_shrink, 1.0);
        let key = layout.root.find("key").unwrap();
        assert_eq!(key.binding, "skill.0.key");
        assert_eq!(key.text_color, Some([235, 226, 205, 235]));
        assert!(key.text_shadow);
        assert!(key.hidden);
        assert_eq!(widget_node(key).right, Val::Px(3.0));
        // 普通尺寸调整不能抹掉源码写入的屏幕锚点、四边内距或动态绑定。
        layout.root.find_mut("bar").unwrap().width = Val::Px(440.0);
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }

    #[test]
    fn native_style_errors_are_reported_without_silent_fallback() {
        for source in [
            "#root Node { align_items: AlignItems::Unknown }",
            "#root Node { border: UiRect::all(px(-1)) }",
            "#root Node {} Text(\"a\") UiImage(\"icon.png\")",
            "#root Node {} UiBind(\"a\") UiBind(\"b\")",
            "#root Node { right: px(1) }",
            "#root Node { position_type: PositionType::Absolute, right: px(1) }",
            "#root Node { margin: UiRect { left: px(1), left: px(2) } }",
        ] {
            assert!(UiLayout::parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn wrapped_layout_keeps_independent_gaps() {
        let layout = UiLayout::parse("#root Node { flex_direction: FlexDirection::Row, row_gap: px(0), column_gap: px(3), flex_wrap: FlexWrap::Wrap }").unwrap();
        let node = widget_node(&layout.root);
        assert_eq!(node.row_gap, Val::Px(0.0));
        assert_eq!(node.column_gap, Val::Px(3.0));
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }

    #[test]
    fn external_authored_asset_round_trips_when_requested() {
        let Some(path) = std::env::var_os("FEATHRISE_UI_BSN_TEST_ASSET") else {
            return;
        };
        let source = std::fs::read_to_string(&path).expect("读取指定的本轮实际界面资产");
        let layout = UiLayout::parse(&source).expect("完整作者资产可被公共解析器读取");
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }

    #[test]
    fn text_natural_size_defaults_keep_explicit_constraints() {
        let layout = UiLayout::parse(
            r#"#root Node { width: px(238), flex_direction: FlexDirection::Row }
            Children [
                #name Node {} Text("沈照雪")
                --
                #constrained Node { width: px(54), min_width: px(0), flex_shrink: 0 } Text("0/100")
            ]"#,
        )
        .unwrap();
        let natural = widget_node(layout.root.find("name").unwrap());
        assert_eq!(natural.min_width, Val::Auto);
        assert_eq!(natural.flex_shrink, 1.0);
        let constrained = widget_node(layout.root.find("constrained").unwrap());
        assert_eq!(constrained.width, Val::Px(54.0));
        assert_eq!(constrained.min_width, Val::Px(0.0));
        assert_eq!(constrained.flex_shrink, 0.0);
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }

    #[test]
    fn elliptical_corners_keep_both_axes_and_individual_corners() {
        let layout = UiLayout::parse("#root Node { border_radius: BorderRadius::new(CornerRadius::new(px(8), percent(20)), px(4), CornerRadius::all(px(3)), CornerRadius::circular(px(2))) }").unwrap();
        let radius = layout.root.style.border_radius;
        assert_eq!(radius.top_left.x, Val::Px(8.0));
        assert_eq!(radius.top_left.y, Val::Percent(20.0));
        assert_eq!(radius.bottom_right.y, Val::Px(3.0));
        assert_eq!(radius.bottom_left.y, Val::Auto);
        assert_eq!(UiLayout::parse(&layout.to_bsn()).unwrap(), layout);
    }
}
