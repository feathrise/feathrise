//! 纯文本预览固定完整来源身份，构造中和退役正文不能冒充当前草稿。

use super::types::SyntaxKey;
use bevy::prelude::*;

#[derive(Component)]
pub(in super::super) struct SyntaxPreview(pub(super) SyntaxKey);

impl SyntaxPreview {
    pub(in super::super) fn new(doc: &super::super::types::Document) -> Self {
        Self(SyntaxKey::new(doc))
    }
}
