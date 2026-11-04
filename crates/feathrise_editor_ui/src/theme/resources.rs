//! 固定小色板与有界UI所有权缓存；不复制文档、字体、图片或作者数据。
use super::constants::{COLORS, rgba};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Resource)]
pub struct EditorPalette {
    name: &'static str,
    colors: HashMap<[u32; 3], (f32, LinearRgba)>,
}

impl Default for EditorPalette {
    fn default() -> Self {
        Self::neutral_gray()
    }
}

impl EditorPalette {
    pub fn neutral_gray() -> Self {
        Self::build("NeutralGray", false)
    }
    pub fn graphite_blue() -> Self {
        Self::build("GraphiteBlue", true)
    }
    pub fn warm_copper() -> Self {
        Self {
            name: "WarmCopper",
            colors: HashMap::new(),
        }
    }
    pub fn name(&self) -> &'static str {
        self.name
    }

    fn build(name: &'static str, blue: bool) -> Self {
        Self {
            name,
            colors: COLORS
                .iter()
                .map(|&(source, gray, graphite)| {
                    let source = source.to_linear();
                    (
                        key(source),
                        (source.alpha, if blue { graphite } else { gray }.to_linear()),
                    )
                })
                .collect(),
        }
    }

    /// alpha变体仍表示原强调强度；未知颜色（素材、彩色Emoji等）不被改写。
    pub fn resolve(&self, color: Color) -> Color {
        Color::LinearRgba(self.resolve_linear(color.to_linear()))
    }
    pub(super) fn resolve_linear(&self, color: LinearRgba) -> LinearRgba {
        self.colors
            .get(&key(color))
            .map_or(color, |&(alpha, mut target)| {
                target.alpha = (target.alpha * color.alpha / alpha).clamp(0.0, 1.0);
                target
            })
    }

    pub fn syntax_colors(&self) -> [Color; 6] {
        let key = match self.name {
            "NeutralGray" => rgba(0xb8c7d4, 1.0),
            "GraphiteBlue" => rgba(0x9dc6e9, 1.0),
            _ => crate::colors::SYNTAX_KEY,
        };
        [
            key,
            self.resolve(crate::colors::SYNTAX_KEYWORD),
            self.resolve(crate::colors::SYNTAX_STRING),
            self.resolve(crate::colors::SYNTAX_NUMBER),
            self.resolve(crate::colors::SYNTAX_COMMENT),
            self.resolve(crate::colors::SYNTAX_TYPE),
        ]
    }
}

pub(super) fn key(color: LinearRgba) -> [u32; 3] {
    [
        color.red.to_bits(),
        color.green.to_bits(),
        color.blue.to_bits(),
    ]
}

#[derive(Resource, Default)]
pub(super) struct ThemeTopology(pub u64);

#[derive(Resource, Default)]
pub(super) struct ThemeExtraction {
    pub topology: u64,
    pub scopes: HashMap<Entity, bool>,
    pub transparent_buttons: HashSet<Entity>, // 仅保留当前有交互的透明按钮，销毁和移出后即回收。
}
