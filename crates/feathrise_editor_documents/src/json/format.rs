//! 已校验JSON仅重写字符串之外的空白；保留数字、转义、键序和重复键的原始词元。

use feathrise_editor_documents_model::{
    constants::MAX_FILE_BYTES, history::types::Selection, json::types::Formatted,
};
use std::sync::Arc;

/// 输出和原文件同受256 KiB限制；缩进膨胀在后台拒绝，不把截断结果采用为草稿。
pub(super) fn pretty(
    source: &str,
    crlf: bool,
    bom: bool,
    selection: Selection,
) -> Result<Option<Formatted>, String> {
    let limit = MAX_FILE_BYTES - usize::from(bom) * 3;
    let mut out = String::with_capacity(limit);
    let mut mapped = [None, None];
    let positions = [selection.anchor, selection.focus];
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut depth = 0usize;
    let mut previous = 0u8;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        let start = cursor;
        let token = bytes[cursor];
        cursor += 1;
        if token == b'"' {
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'\\' => cursor += 2,
                    b'"' => {
                        cursor += 1;
                        break;
                    }
                    _ => cursor += 1,
                }
            }
        } else if !b"{}[],:".contains(&token) {
            while cursor < bytes.len()
                && !bytes[cursor].is_ascii_whitespace()
                && !b"{}[],:".contains(&bytes[cursor])
            {
                cursor += 1;
            }
        }
        if matches!(token, b'}' | b']') {
            depth -= 1; // 仅在完整语法校验通过后调用，容器层级不会下溢。
            let empty = (previous == b'{' && token == b'}') || (previous == b'[' && token == b']');
            if !empty {
                newline(&mut out, depth, limit)?;
            }
        }
        let output_start = out.len();
        append(&mut out, &source[start..cursor], limit)?;
        // 只映射两个选区端点，词元内UTF-8字节保持；原空白中的端点归到下一个词元。
        for (mapped, position) in mapped.iter_mut().zip(positions) {
            if mapped.is_none() && position <= cursor {
                *mapped = Some(output_start + position.saturating_sub(start).min(cursor - start));
            }
        }
        match token {
            b'{' | b'[' => {
                depth += 1;
                let next = bytes[cursor..]
                    .iter()
                    .find(|byte| !byte.is_ascii_whitespace());
                if !next.is_some_and(|next| matches!(*next, b'}' | b']')) {
                    newline(&mut out, depth, limit)?;
                }
            }
            b',' => newline(&mut out, depth, limit)?,
            b':' => append(&mut out, " ", limit)?,
            _ => (),
        }
        previous = token;
    }
    let end = out.len();
    append(&mut out, "\n", limit)?;
    if source == out {
        return Ok(None);
    } // 已格式化内容不标脏、不新增历史或触发原生重排。
    let raw_size =
        out.len() + usize::from(crlf) * out.bytes().filter(|byte| *byte == b'\n').count();
    if raw_size > limit {
        return Err(too_large());
    }
    let raw = if crlf {
        Arc::from(out.replace('\n', "\r\n"))
    } else {
        Arc::from(out.as_str())
    };
    Ok(Some(Formatted {
        source: Arc::from(out),
        pages: vec![raw],
        selection: Selection {
            anchor: mapped[0].unwrap_or(end),
            focus: mapped[1].unwrap_or(end),
        },
    }))
}

fn too_large() -> String {
    "格式化后的完整文件超过256 KiB；原草稿保持，请减少内容后重试".into()
}

fn append(out: &mut String, value: &str, limit: usize) -> Result<(), String> {
    if out.len() + value.len() > limit {
        return Err(too_large());
    }
    out.push_str(value);
    Ok(())
}

fn newline(out: &mut String, depth: usize, limit: usize) -> Result<(), String> {
    let spaces = depth * 2;
    if out.len() + 1 + spaces > limit {
        return Err(too_large());
    }
    out.push('\n');
    out.extend(std::iter::repeat_n(' ', spaces));
    Ok(())
}
