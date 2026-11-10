//! 文档格式、输入身份与不可变内容派生契约；不包含场景会话或任务调度。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceKey {
    pub path: String,
    pub generation: u64,
}

/// 文本预览只接受显式白名单；二进制数据和GLTF仍走各自的专用入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentFormat {
    Markdown,
    Text,
    Code,
    Config,
}

impl DocumentFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Markdown => "文档",
            Self::Text => "文本",
            Self::Code => "代码",
            Self::Config => "配置",
        }
    }
}

/// 预览只携带排版结构；主线程仍按固定预算构造既有UI。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: bool,
    pub struck: bool,
}

#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub style: InlineStyle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockKind {
    #[default]
    Paragraph,
    Heading(u8),
    Code,
    Quote,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
    pub chars: usize,
}

#[derive(Clone, Debug, Default)]
pub struct PreviewPage {
    pub blocks: Vec<Block>,
    pub chars: usize,
}

pub struct ParsedPreview {
    pub pages: std::sync::Arc<Vec<PreviewPage>>,
    pub clipped: bool,
}
