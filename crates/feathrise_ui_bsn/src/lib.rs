//! Feathrise 界面资产，提供独立编辑器和运行时共用的 BSN 控件树。

mod bsn;
mod view;

use bevy::prelude::*;

pub use bsn::valid_asset_path;
pub use view::{LayoutContext, UiLayoutAction, spawn_layout};

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
    pub width: Val,
    pub height: Val,
    pub direction: FlexDirection,
    pub gap: f32,
    pub padding: f32,
    pub position: Option<Vec2>,
    pub background: Option<[u8; 4]>,
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
            width: Val::Auto,
            height,
            direction: FlexDirection::Column,
            gap: 0.0,
            padding: 0.0,
            position: None,
            background: None,
            font_size: 14.0,
            children: Vec::new(),
            comments: Vec::new(),
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
