//! Feathrise 界面资产，提供独立编辑器和运行时共用的 BSN 控件树。

mod bsn;
mod view;

use bevy::prelude::*;

pub use bsn::valid_asset_path;
pub use bsn::rust_document::is_rust_ui_document;
pub use view::{LayoutContext, UiLayoutAction, spawn_layout, spawn_widget_tree, widget_node};

#[derive(Clone, Debug, PartialEq)]
pub struct UiLayout {
    pub root: UiWidget,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiWidget {
    pub id: String,
    pub kind: WidgetKind,
    pub content: String,
    pub action: String,
    /// 通用数据绑定不改变控件类型，画布显示样例内容，工程按此名称装配动态更新。
    pub binding: String,
    pub width: Val,
    pub height: Val,
    pub direction: FlexDirection,
    pub gap: f32,
    pub padding: f32,
    pub position: Option<Vec2>,
    pub background: Option<[u8; 4]>,
    /// 额外布局直接采用原生 Node，避免另建一套对齐、锚点和边距模型。
    /// width、height、方向、间距、内边距及旧像素坐标由上方对应字段统一合成。
    pub style: Node,
    pub border_color: Option<[u8; 4]>,
    pub text_color: Option<[u8; 4]>,
    pub text_shadow: bool,
    pub text_justify: Justify,
    /// 提示和临时通知可从资产声明默认隐藏，运行时仍可按原交互系统显示。
    pub hidden: bool,
    pub font_size: f32,
    pub children: Vec<UiWidget>,
    comments: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetKind {
    Container,
    Text,
    Image,
    Button,
    Binding,
}

impl WidgetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Container => "容器",
            Self::Text => "文本",
            Self::Image => "图片",
            Self::Button => "按钮",
            Self::Binding => "数据绑定",
        }
    }
}

impl UiWidget {
    pub fn new(id: String, kind: WidgetKind) -> Self {
        let (content, height) = match kind {
            WidgetKind::Container => ("", Val::Auto),
            WidgetKind::Text => ("文本", Val::Auto),
            WidgetKind::Image => ("", Val::Px(112.0)),
            WidgetKind::Button => ("按钮", Val::Px(30.0)),
            WidgetKind::Binding => ("", Val::Px(24.0)),
        };
        Self {
            id,
            kind,
            content: content.into(),
            action: String::new(),
            binding: String::new(),
            width: Val::Auto,
            height,
            direction: FlexDirection::Column,
            gap: 0.0,
            padding: 0.0,
            position: None,
            background: None,
            style: Self::default_style(kind),
            border_color: None,
            text_color: None,
            text_shadow: false,
            text_justify: Justify::Left,
            hidden: false,
            font_size: 14.0,
            children: Vec::new(),
            comments: Vec::new(),
        }
    }

    pub(crate) fn default_style(kind: WidgetKind) -> Node {
        Node {
            // Text 在自己实体上参与原生测量：Auto 才会使用内容宽度，零最小宽会被
            // Bevy 的测量器当成有效宽度，令中文逐字换行；容器沿用旧编辑器的收缩策略。
            min_width: if kind == WidgetKind::Text {
                Val::Auto
            } else {
                Val::Px(0.0)
            },
            flex_shrink: if kind == WidgetKind::Text { 1.0 } else { 0.0 },
            // 默认模型直接记录按钮/绑定实际采用的居中值；原生源码写出再读后仍应
            // 与新增控件完全相同，不能仅在渲染时补齐而令编辑器保存检查误判属性丢失。
            justify_content: if matches!(kind, WidgetKind::Button | WidgetKind::Binding) {
                JustifyContent::Center
            } else {
                JustifyContent::Default
            },
            align_items: if kind == WidgetKind::Button {
                AlignItems::Center
            } else {
                AlignItems::Default
            },
            // Auto 表示未单独覆盖某一轴，最终由通用 gap 合成两轴间距。
            row_gap: Val::Auto,
            column_gap: Val::Auto,
            ..default()
        }
    }

    pub fn find(&self, id: &str) -> Option<&Self> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Self> {
        if self.id == id {
            return Some(self);
        }
        self.children
            .iter_mut()
            .find_map(|child| child.find_mut(id))
    }

    pub fn parent_mut(&mut self, id: &str) -> Option<&mut Self> {
        if self.children.iter().any(|child| child.id == id) {
            return Some(self);
        }
        self.children
            .iter_mut()
            .find_map(|child| child.parent_mut(id))
    }
}

impl Default for UiLayout {
    fn default() -> Self {
        let mut root = UiWidget::new("root".into(), WidgetKind::Container);
        root.width = Val::Percent(100.0);
        Self { root }
    }
}
