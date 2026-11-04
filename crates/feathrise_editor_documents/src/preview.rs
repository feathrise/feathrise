//! Markdown排版与其他格式的原文共用后台分块预算；界面连续滚动，全文派生只在有界计算池执行。

use feathrise_editor_documents_model::{constants::*, types::*};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::sync::Arc;

#[derive(Default)]
struct Builder {
    pages: Vec<PreviewPage>,
    page: PreviewPage,
    block: Block,
    chars: usize,
    spans: usize,
    clipped: bool,
}

impl Builder {
    fn finish(mut self) -> ParsedPreview {
        self.flush();
        if !self.page.blocks.is_empty() {
            self.pages.push(self.page);
        }
        if self.pages.is_empty() {
            self.pages.push(PreviewPage::default());
        }
        ParsedPreview {
            pages: Arc::new(self.pages),
            clipped: self.clipped,
        }
    }

    fn flush(&mut self) {
        if self.block.spans.is_empty() {
            return;
        }
        if !self.page.blocks.is_empty()
            && (self.page.blocks.len() == PREVIEW_PAGE_BLOCKS
                || self.page.chars + self.block.chars > PREVIEW_PAGE_CHARS)
        {
            self.pages.push(std::mem::take(&mut self.page));
        }
        let kind = self.block.kind;
        self.page.chars += self.block.chars;
        self.page.blocks.push(std::mem::take(&mut self.block));
        self.block.kind = kind;
    }

    fn append(&mut self, text: &str, style: InlineStyle) {
        // 文本和span都按块拆开，极长段落/密集内联语法不能制造无界UI节点。
        let mut start = 0;
        while start < text.len() {
            if self.chars == MAX_PREVIEW_CHARS {
                self.clipped = true;
                return;
            }
            if self.block.chars == BLOCK_CHARS || self.block.spans.len() == BLOCK_SPANS {
                self.flush();
            }
            let room = (BLOCK_CHARS - self.block.chars).min(MAX_PREVIEW_CHARS - self.chars);
            let end = text[start..]
                .char_indices()
                .nth(room)
                .map_or(text.len(), |(offset, _)| start + offset);
            let part = &text[start..end];
            if self.spans == MAX_PREVIEW_SPANS
                && self
                    .block
                    .spans
                    .last()
                    .is_none_or(|last| last.style != style)
            {
                self.clipped = true;
                return;
            }
            let chars = part.chars().count();
            self.block.chars += chars;
            self.chars += chars;
            if let Some(last) = self
                .block
                .spans
                .last_mut()
                .filter(|last| last.style == style)
            {
                last.text.push_str(part);
            } else {
                self.spans += 1;
                self.block.spans.push(Span {
                    text: part.into(),
                    style,
                });
            }
            start = end;
        }
    }
}

pub fn parse(format: DocumentFormat, pages: &[Arc<str>]) -> ParsedPreview {
    if format != DocumentFormat::Markdown {
        // 源码、HTML和配置只显示原文，标点与缩进不经过Markdown解释，也不执行任何内容。
        // 逐个共享源分块推进，只有派生文字复制；内部块边界不产生界面分页。
        let mut out = Builder::default();
        for page in pages {
            out.append(page, InlineStyle::default());
            if out.clipped {
                break;
            }
        }
        return out.finish();
    }
    // 必须完整解析拼接后的文档，跨源分块的代码围栏、引用链接和列表上下文不能丢失。
    let text: String = pages.iter().map(AsRef::as_ref).collect();
    let mut out = Builder::default();
    let mut bold = 0usize;
    let mut italic = 0usize;
    let mut struck = 0usize;
    let mut links: Vec<String> = Vec::new();
    let mut images: Vec<String> = Vec::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut quote = 0usize;
    let mut code = false;
    let mut prefix = String::new();
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(&text, options) {
        if out.clipped {
            break;
        }
        let style = InlineStyle {
            bold: bold > 0,
            italic: italic > 0,
            struck: struck > 0,
            code,
            link: !links.is_empty(),
        };
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                out.flush();
                out.block.kind = BlockKind::Heading(level as u8);
            }
            Event::Start(Tag::Paragraph) => {
                out.flush();
                out.block.kind = if quote > 0 {
                    BlockKind::Quote
                } else {
                    BlockKind::Paragraph
                };
            }
            Event::Start(Tag::CodeBlock(_)) => {
                out.flush();
                out.block.kind = BlockKind::Code;
                code = true;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                out.flush();
                quote += 1;
                out.block.kind = BlockKind::Quote;
            }
            Event::Start(Tag::List(start)) => {
                out.flush();
                lists.push(start);
            }
            Event::Start(Tag::Item) => {
                out.flush();
                prefix = "  ".repeat(lists.len().saturating_sub(1).min(8));
                match lists.last_mut() {
                    Some(Some(number)) => {
                        prefix.push_str(&format!("{number}. "));
                        *number = number.saturating_add(1);
                    }
                    _ => prefix.push_str("• "),
                }
            }
            Event::Start(Tag::Strong) => bold += 1,
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::Start(Tag::Strikethrough) => struck += 1,
            Event::Start(Tag::Link { dest_url, .. }) => links.push(short_destination(&dest_url)),
            Event::Start(Tag::Image { dest_url, .. }) => {
                out.append("[图片：", style);
                images.push(short_destination(&dest_url));
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                out.flush();
                out.block.kind = BlockKind::Paragraph;
            }
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::End(TagEnd::Emphasis) => italic = italic.saturating_sub(1),
            Event::End(TagEnd::Strikethrough) => struck = struck.saturating_sub(1),
            Event::End(TagEnd::Link) => {
                if let Some(url) = links.pop() {
                    out.append(
                        &format!("（{url}）"),
                        InlineStyle {
                            link: true,
                            ..style
                        },
                    );
                }
            }
            Event::End(TagEnd::Image) => {
                if let Some(url) = images.pop() {
                    out.append(&format!(" · {url}]"), style);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                out.flush();
                code = false;
                out.block.kind = BlockKind::Paragraph;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                out.flush();
                quote = quote.saturating_sub(1);
                out.block.kind = BlockKind::Paragraph;
            }
            Event::End(TagEnd::List(_)) => {
                out.flush();
                lists.pop();
            }
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Item
                | TagEnd::Heading(_)
                | TagEnd::TableHead
                | TagEnd::TableRow,
            ) => {
                out.flush();
                prefix.clear();
                out.block.kind = if quote > 0 {
                    BlockKind::Quote
                } else {
                    BlockKind::Paragraph
                };
            }
            Event::End(TagEnd::TableCell) => out.append(" │ ", style),
            Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
                // HTML保持惰性文字；链接和图片不联网、不执行，也不隐式加载外部资产。
                if !prefix.is_empty() {
                    out.append(&std::mem::take(&mut prefix), style);
                }
                out.append(&value, style);
            }
            Event::Code(value) => {
                if !prefix.is_empty() {
                    out.append(&std::mem::take(&mut prefix), style);
                }
                out.append(
                    &value,
                    InlineStyle {
                        code: true,
                        ..style
                    },
                );
            }
            Event::TaskListMarker(done) => {
                if !prefix.is_empty() {
                    out.append(&std::mem::take(&mut prefix), style);
                }
                out.append(if done { "[完成] " } else { "[待办] " }, style);
            }
            Event::SoftBreak | Event::HardBreak => out.append("\n", style),
            Event::Rule => {
                out.flush();
                out.append("────────────────", style);
                out.flush();
            }
            _ => (),
        }
    }
    out.finish()
}

fn short_destination(value: &str) -> String {
    match value.char_indices().nth(160) {
        Some((end, _)) => format!("{}…", &value[..end]),
        None => value.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(preview: &ParsedPreview) -> String {
        preview.pages.iter().flat_map(|page| &page.blocks)
            .flat_map(|block| &block.spans).map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn literal_formats_preserve_chunks_without_interpreting_markup() {
        let pages = [Arc::from("# 中文\r\n**原"), Arc::from("文**\n<script>保留</script>")];
        for format in [DocumentFormat::Text, DocumentFormat::Code, DocumentFormat::Config] {
            let result = parse(format, &pages);
            assert_eq!(text(&result), pages.iter().map(AsRef::as_ref).collect::<String>());
            assert!(!result.clipped);
            assert!(result.pages.iter().flat_map(|page| &page.blocks)
                .flat_map(|block| &block.spans).all(|span| span.style == InlineStyle::default()));
        }
    }

    #[test]
    fn markdown_context_crosses_storage_chunks_and_keeps_html_inert() {
        let pages = [Arc::from("# 标题\n\n```rust\nlet 值"),
            Arc::from(" = 1;\n```\n\n[说明][doc]\n\n<script>原文</script>\n\n[doc]: relative.md")];
        let result = parse(DocumentFormat::Markdown, &pages);
        assert!(!result.clipped);
        assert!(result.pages.iter().flat_map(|page| &page.blocks).any(|block| block.kind == BlockKind::Code));
        assert!(text(&result).contains("relative.md"));
        assert!(text(&result).contains("<script>原文</script>"));
    }

    #[test]
    fn preview_budget_never_truncates_the_shared_source() {
        let source: Arc<str> = Arc::from("文".repeat(MAX_PREVIEW_CHARS + 7));
        let result = parse(DocumentFormat::Code, &[source.clone()]);
        assert!(result.clipped);
        assert_eq!(text(&result).chars().count(), MAX_PREVIEW_CHARS);
        assert_eq!(source.chars().count(), MAX_PREVIEW_CHARS + 7);
        assert!(result.pages.iter().all(|page| page.chars <= PREVIEW_PAGE_CHARS
            && page.blocks.len() <= PREVIEW_PAGE_BLOCKS));
    }
}
