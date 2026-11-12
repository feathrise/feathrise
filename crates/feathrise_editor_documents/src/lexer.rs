//! 容忍未写完源码的线性分词；只输出非默认色的UTF-8范围，不改动或解析执行正文。

use feathrise_editor_documents_model::syntax::{Kind, Language, Token};

fn step(source: &str, i: usize) -> usize {
    source[i..].chars().next().map_or(i, |ch| i + ch.len_utf8())
}
fn ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}
fn ident_end(source: &str, mut i: usize) -> usize {
    while let Some(ch) = source[i..]
        .chars()
        .next()
        .filter(|ch| *ch == '_' || ch.is_alphanumeric())
    {
        i += ch.len_utf8();
    }
    i
}

fn quoted(source: &str, mut i: usize) -> (usize, bool) {
    i += 1;
    while i < source.len() {
        match source.as_bytes()[i] {
            b'"' => return (i + 1, true),
            b'\\' => {
                i += 1;
                if i < source.len() {
                    i = step(source, i);
                }
            }
            _ => i = step(source, i),
        }
    }
    (i, false)
}

/// Rust原始串的引号后井号逐段消费；不构造巨型终止字符串，也不反复回扫。
fn raw_string(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = start;
    if matches!(bytes.get(i), Some(b'b' | b'c')) {
        i += 1;
    }
    if bytes.get(i) != Some(&b'r') {
        return None;
    }
    i += 1;
    let hashes = i;
    while bytes.get(i) == Some(&b'#') {
        i += 1;
    }
    let count = i - hashes;
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let quote = i;
            i += 1;
            let after = i;
            while i - after < count && bytes.get(i) == Some(&b'#') {
                i += 1;
            }
            if i - after == count {
                return Some(quote + 1 + count);
            }
        } else {
            i = step(source, i);
        }
    }
    Some(i)
}

fn character(source: &str, quote: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = quote + 1;
    if i >= bytes.len() || matches!(bytes[i], b'\n' | b'\r' | b'\'') {
        return None;
    }
    if bytes[i] == b'\\' {
        i += 1;
        match bytes.get(i) {
            Some(b'x') => {
                i += 1;
                for _ in 0..2 {
                    if !bytes.get(i)?.is_ascii_hexdigit() {
                        return None;
                    }
                    i += 1;
                }
            }
            Some(b'u') if bytes.get(i + 1) == Some(&b'{') => {
                i += 2;
                while bytes
                    .get(i)
                    .is_some_and(|ch| ch.is_ascii_hexdigit() || *ch == b'_')
                {
                    i += 1;
                }
                if bytes.get(i) != Some(&b'}') {
                    return None;
                }
                i += 1;
            }
            Some(_) => i = step(source, i),
            None => return None,
        }
    } else {
        i = step(source, i);
    }
    (bytes.get(i) == Some(&b'\'')).then_some(i + 1)
}

fn number(source: &str, start: usize, language: Language) -> usize {
    let bytes = source.as_bytes();
    let mut i = start + usize::from(bytes[start] == b'-');
    let prefixed = language != Language::Json
        && bytes.get(i) == Some(&b'0')
        && bytes
            .get(i + 1)
            .is_some_and(|ch| matches!(ch, b'x' | b'X' | b'b' | b'B' | b'o' | b'O'));
    let hex = prefixed && matches!(bytes[i + 1], b'x' | b'X');
    if prefixed {
        i += 2;
    }
    let digit = |ch: &u8| {
        if hex {
            ch.is_ascii_hexdigit() || *ch == b'_'
        } else {
            ch.is_ascii_digit() || (language != Language::Json && *ch == b'_')
        }
    };
    while bytes.get(i).is_some_and(digit) {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.')
        && bytes.get(i + 1) != Some(&b'.')
        && (!prefixed || hex)
        && (bytes.get(i + 1).is_some_and(digit)
            || language != Language::Json
                && bytes
                    .get(i + 1)
                    .is_none_or(|ch| !ch.is_ascii_alphabetic() && *ch != b'_'))
    {
        i += 1;
        while bytes.get(i).is_some_and(digit) {
            i += 1;
        }
    }
    if bytes.get(i).is_some_and(|ch| {
        if hex {
            matches!(ch, b'p' | b'P')
        } else {
            matches!(ch, b'e' | b'E')
        }
    }) {
        let mut exponent = i + 1;
        if bytes
            .get(exponent)
            .is_some_and(|ch| matches!(ch, b'+' | b'-'))
        {
            exponent += 1;
        }
        if bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
            i = exponent + 1;
            while bytes.get(i).is_some_and(|ch| {
                ch.is_ascii_digit() || (language != Language::Json && *ch == b'_')
            }) {
                i += 1;
            }
        }
    }
    if language != Language::Json {
        while bytes
            .get(i)
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == b'_')
        {
            i += 1;
        }
    }
    i
}

fn word(language: Language, text: &str) -> Option<Kind> {
    use Kind::*;
    if matches!(text, "true" | "false") || language == Language::Json && text == "null" {
        return Some(Keyword);
    }
    match language {
        Language::Json => None,
        Language::Rust => {
            if matches!(
                text,
                "as" | "async"
                    | "await"
                    | "break"
                    | "const"
                    | "continue"
                    | "crate"
                    | "dyn"
                    | "else"
                    | "enum"
                    | "extern"
                    | "fn"
                    | "for"
                    | "if"
                    | "impl"
                    | "in"
                    | "let"
                    | "loop"
                    | "match"
                    | "mod"
                    | "move"
                    | "mut"
                    | "pub"
                    | "ref"
                    | "return"
                    | "self"
                    | "static"
                    | "struct"
                    | "super"
                    | "trait"
                    | "type"
                    | "unsafe"
                    | "use"
                    | "where"
                    | "while"
                    | "yield"
                    | "gen"
            ) {
                Some(Keyword)
            } else if matches!(
                text,
                "Self"
                    | "bool"
                    | "char"
                    | "str"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "f16"
                    | "f32"
                    | "f64"
                    | "f128"
            ) {
                Some(Type)
            } else {
                None
            }
        }
        Language::Wgsl => {
            if matches!(
                text,
                "alias"
                    | "break"
                    | "case"
                    | "const"
                    | "const_assert"
                    | "continue"
                    | "continuing"
                    | "default"
                    | "diagnostic"
                    | "discard"
                    | "else"
                    | "enable"
                    | "fn"
                    | "for"
                    | "if"
                    | "let"
                    | "loop"
                    | "override"
                    | "requires"
                    | "return"
                    | "struct"
                    | "switch"
                    | "var"
                    | "while"
            ) {
                Some(Keyword)
            } else if matches!(
                text,
                "bool"
                    | "f16"
                    | "f32"
                    | "i32"
                    | "u32"
                    | "vec2"
                    | "vec3"
                    | "vec4"
                    | "vec2f"
                    | "vec3f"
                    | "vec4f"
                    | "vec2i"
                    | "vec3i"
                    | "vec4i"
                    | "vec2u"
                    | "vec3u"
                    | "vec4u"
                    | "vec2h"
                    | "vec3h"
                    | "vec4h"
                    | "mat2x2"
                    | "mat2x3"
                    | "mat2x4"
                    | "mat3x2"
                    | "mat3x3"
                    | "mat3x4"
                    | "mat4x2"
                    | "mat4x3"
                    | "mat4x4"
                    | "array"
                    | "atomic"
                    | "ptr"
                    | "sampler"
                    | "sampler_comparison"
                    | "texture_1d"
                    | "texture_2d"
                    | "texture_2d_array"
                    | "texture_3d"
                    | "texture_cube"
                    | "texture_cube_array"
                    | "texture_multisampled_2d"
                    | "texture_depth_2d"
                    | "texture_depth_2d_array"
                    | "texture_depth_cube"
                    | "texture_depth_cube_array"
                    | "texture_depth_multisampled_2d"
                    | "texture_storage_1d"
                    | "texture_storage_2d"
                    | "texture_storage_2d_array"
                    | "texture_storage_3d"
            ) {
                Some(Type)
            } else {
                None
            }
        }
    }
}

pub(super) fn scan(language: Language, source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let mut kind = None;
        if language != Language::Json && source[i..].starts_with("//") {
            i = source[i..].find('\n').map_or(bytes.len(), |end| i + end);
            kind = Some(Kind::Comment);
        } else if language != Language::Json && source[i..].starts_with("/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if source[i..].starts_with("/*") {
                    depth += 1;
                    i += 2;
                } else if source[i..].starts_with("*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i = step(source, i);
                }
            }
            kind = Some(Kind::Comment);
        } else if language == Language::Rust
            && let Some(end) = raw_string(source, i)
        {
            i = end;
            kind = Some(Kind::String);
        } else if language == Language::Wgsl
            && bytes[i] == b'@'
            && source[i + 1..].chars().next().is_some_and(ident_start)
        {
            i = ident_end(source, i + 1);
            kind = Some(Kind::Key); // 着色器阶段、绑定和location等属性连同@一起以铜金提示。
        } else if bytes[i] == b'"'
            || language == Language::Rust
                && matches!(bytes[i], b'b' | b'c')
                && bytes.get(i + 1) == Some(&b'"')
        {
            let quote = i + usize::from(bytes[i] != b'"');
            let (end, closed) = quoted(source, quote);
            i = end;
            kind = Some(
                if language == Language::Json && closed && source[i..].trim_start().starts_with(':')
                {
                    Kind::Key
                } else {
                    Kind::String
                },
            );
        } else if language == Language::Rust
            && (bytes[i] == b'\'' || bytes[i] == b'b' && bytes.get(i + 1) == Some(&b'\''))
        {
            let quote = i + usize::from(bytes[i] != b'\'');
            if let Some(end) = character(source, quote) {
                i = end;
                kind = Some(Kind::String);
            } else if quote == i && source[i + 1..].chars().next().is_some_and(ident_start) {
                i = ident_end(source, i + 1);
                kind = Some(Kind::Type);
            } else {
                i += 1;
            }
        } else if language == Language::Rust
            && source[i..].starts_with("r#")
            && source[i + 2..].chars().next().is_some_and(ident_start)
        {
            i = ident_end(source, i + 2); // r#type是原始标识符，不把内部type误标成关键字。
        } else if bytes[i].is_ascii_digit()
            || language == Language::Json
                && bytes[i] == b'-'
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
            || language == Language::Wgsl
                && bytes[i] == b'.'
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
        {
            i = number(source, i, language);
            kind = Some(Kind::Number);
        } else if source[i..].chars().next().is_some_and(ident_start) {
            i = ident_end(source, i);
            kind = word(language, &source[start..i]);
        } else {
            i = step(source, i);
        }
        if let Some(kind) = kind {
            result.push(Token {
                start: start as u32,
                end: i as u32,
                kind,
            });
        }
    }
    result
}
