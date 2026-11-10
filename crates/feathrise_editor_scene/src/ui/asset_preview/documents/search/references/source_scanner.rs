//! 后台识别Rust完整路径字符串；词法解析与转义复用已锁定库，不把注释或部分文件名算作引用。

use super::super::types::Hit;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn match_rust(
    source: &str,
    target: &str,
    maximum: usize,
    cancel: &AtomicBool,
) -> Result<Vec<Hit>, String> {
    let result = scan(source, target, maximum, cancel);
    // Token/Span均已在scan内部释放；清掉此线程的SourceMap，防止反复查整个项目累积源码。
    // 这里不await，也不把!Send的词法对象移出当前计算闭包。
    proc_macro2::extra::invalidate_current_thread_spans();
    result
}

fn scan(
    source: &str,
    target: &str,
    maximum: usize,
    cancel: &AtomicBool,
) -> Result<Vec<Hit>, String> {
    if source.len() > super::super::super::constants::MAX_FILE_BYTES {
        return Err("Rust源码超过256 KiB解析预算".into());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("引用查询已取消".into());
    }
    let tokens: TokenStream = source
        .parse()
        .map_err(|error| format!("Rust词法解析失败：{error}"))?;
    let mut stack = vec![(tokens.into_iter(), false)];
    let mut hits = Vec::new();
    while let Some((iter, hash)) = stack.last_mut() {
        if cancel.load(Ordering::Relaxed) {
            return Err("引用查询已取消".into());
        }
        let Some(token) = iter.next() else {
            stack.pop();
            continue;
        };
        let attribute = *hash;
        *hash = matches!(&token, TokenTree::Punct(punct) if punct.as_char() == '#' || (attribute && punct.as_char() == '!'));
        match token {
            TokenTree::Group(group) => {
                if attribute
                    && group.delimiter() == Delimiter::Bracket
                    && matches!(group.stream().into_iter().next(), Some(TokenTree::Ident(name)) if name == "doc")
                {
                    continue; // ///、//!及显式doc属性只描述文件，不是源码中的路径使用。
                }
                if stack.len() >= 128 {
                    return Err("Rust词法嵌套超过128层".into());
                }
                stack.push((group.stream().into_iter(), false));
            }
            TokenTree::Literal(literal) if hits.len() < maximum => {
                let range = literal.span().byte_range();
                let token = TokenStream::from(TokenTree::Literal(literal));
                let Ok(value) = syn::parse2::<syn::LitStr>(token) else {
                    continue;
                };
                let value = value.value();
                if value.split('#').next() != Some(target) {
                    continue;
                }
                let Some(raw) = source.get(range.clone()) else {
                    return Err("Rust原文位置无效".into());
                };
                let left: String = source[..range.start]
                    .chars()
                    .rev()
                    .take_while(|ch| *ch != '\n')
                    .take(24)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let right: String = source[range.end..]
                    .chars()
                    .take_while(|ch| *ch != '\n')
                    .take(48)
                    .collect();
                let matched: String = raw.chars().take(128).collect();
                hits.push(Hit {
                    range,
                    line: 0,
                    column: 0,
                    snippet: format!("{left}‹{matched}›{right}").replace('\t', " "),
                });
            }
            _ => (),
        }
    }
    let (mut byte, mut line, mut column) = (0, 1, 1);
    for hit in &mut hits {
        for ch in source[byte..hit.range.start].chars() {
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        byte = hit.range.start;
        hit.line = line;
        hit.column = column;
    }
    Ok(hits)
}
