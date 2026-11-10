//! 使用serde_json验证完整结构并记录原文位置；不自行实现JSON语法或用关键词猜引用。

use super::super::{constants::MAX_PATH_BYTES, types::Hit};
use super::{helpers::path_field, types::*};
use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use std::{
    cell::Cell,
    fmt,
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
};

impl Read for Cursor<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("引用查询已取消"));
        }
        let at = self.position.get();
        if buffer.is_empty() || at == self.source.len() {
            return Ok(0);
        }
        // serde的IoRead至多预读一个字节；不提前填充缓冲，游标才能标出原文字符串结尾。
        buffer[0] = self.source[at];
        self.position.set(at + 1);
        Ok(1)
    }
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(mut self, deserializer: D) -> Result<(), D::Error> {
        self.start = self.scan.position.get().saturating_sub(1);
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("有效JSON值")
    }
    fn visit_unit<E: Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: Error>(self, value: &str) -> Result<(), E> {
        if !self.field || value.is_empty()
            || self.scan.target.is_some_and(|target| value.split('#').next() != Some(target)) {
            return Ok(());
        }
        if self.scan.hits.len() >= self.scan.maximum {
            self.scan.limited = true;
            return Ok(()); // 继续验证剩余JSON；不能把语法错误后的半份路径发布出去。
        }
        if self.scan.target.is_none() && value.len() > MAX_PATH_BYTES {
            return Err(E::custom("资产路径超过1024字节检查预算"));
        }
        let end = self.scan.position.get();
        let Some(offset) = self.scan.source[self.start..end].find('"') else {
            return Err(E::custom("无法确认路径字符串的原文位置"));
        };
        let start = self.start + offset;
        if self.scan.source.as_bytes().get(end.saturating_sub(1)) != Some(&b'"') {
            return Err(E::custom("路径字符串位置与JSON解析游标不一致"));
        }
        // JSON Pointer显示完整字段路径；超过行内预算时明确省略，原文定位仍保持精确。
        let mut snippet: String = self.scan.pointer.chars().take(201).collect();
        if self.scan.pointer.chars().count() > 201 {
            snippet.push('…');
        }
        self.scan.hits.push(Hit {
            range: start..end,
            line: 0,
            column: 0,
            snippet,
        });
        if self.scan.target.is_none() { self.scan.paths.push(value.to_owned()); }
        Ok(())
    }
    fn visit_string<E: Error>(self, value: String) -> Result<(), E> {
        self.visit_str(&value)
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        while let Some(key) = map.next_key::<String>()? {
            let length = self.scan.pointer.len();
            self.scan.pointer.push('/');
            for ch in key.chars() {
                match ch {
                    '~' => self.scan.pointer.push_str("~0"),
                    '/' => self.scan.pointer.push_str("~1"),
                    _ => self.scan.pointer.push(ch),
                }
                if self.scan.pointer.len() > MAX_PATH_BYTES {
                    return Err(M::Error::custom("JSON字段路径超过1024字节"));
                }
            }
            map.next_value_seed(ValueSeed {
                scan: self.scan,
                field: path_field(&key),
                start: 0,
            })?;
            self.scan.pointer.truncate(length);
        }
        Ok(())
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<(), S::Error> {
        for index in 0usize.. {
            let length = self.scan.pointer.len();
            self.scan.pointer.push_str(&format!("/{index}"));
            if self.scan.pointer.len() > MAX_PATH_BYTES {
                return Err(S::Error::custom("JSON字段路径超过1024字节"));
            }
            let next = seq.next_element_seed(ValueSeed {
                scan: self.scan,
                field: self.field,
                start: 0,
            })?;
            self.scan.pointer.truncate(length);
            if next.is_none() {
                break;
            }
        }
        Ok(())
    }
}

pub(in super::super) fn match_json(
    source: &str,
    target: &str,
    maximum: usize,
    cancel: &AtomicBool,
) -> Result<Vec<Hit>, String> {
    parse_json(source, Some(target), maximum, cancel).map(|(hits, _, _)| hits)
}

pub(in super::super) fn collect_json(
    source: &str,
    maximum: usize,
    cancel: &AtomicBool,
) -> Result<(Vec<PathReference>, bool), String> {
    let (hits, paths, limited) = parse_json(source, None, maximum, cancel)?;
    Ok((hits.into_iter().zip(paths).map(|(hit, path)| PathReference { hit, path }).collect(), limited))
}

fn parse_json(
    source: &str,
    target: Option<&str>,
    maximum: usize,
    cancel: &AtomicBool,
) -> Result<(Vec<Hit>, Vec<String>, bool), String> {
    if source.len() > super::super::super::constants::MAX_FILE_BYTES {
        return Err("JSON超过256 KiB引用解析预算".into()); // 草稿虽可输入更多字符，引用解析仍遵守文件字节预算。
    }
    let position = Cell::new(0);
    let mut scan = Scan {
        source,
        target,
        position: &position,
        pointer: String::new(),
        maximum,
        hits: Vec::new(),
        paths: Vec::new(),
        limited: false,
    };
    let cursor = Cursor {
        source: source.as_bytes(),
        position: &position,
        cancel,
    };
    let mut deserializer = serde_json::Deserializer::from_reader(cursor);
    ValueSeed {
        scan: &mut scan,
        field: false,
        start: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|error| error.to_string())?;
    // 即便匹配已满，也验证剩余语法；无效JSON整份跳过，不能发布此前的半份结果。
    deserializer.end().map_err(|error| error.to_string())?;
    let (mut byte, mut line, mut column) = (0, 1, 1);
    for hit in &mut scan.hits {
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
    Ok((scan.hits, scan.paths, scan.limited))
}
