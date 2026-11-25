//! 引用查询沿用公共 JSON 字符串定位器，并保留搜索界面的数量、字节和位置约定。

use super::super::{constants::MAX_PATH_BYTES, types::Hit};
use super::types::PathReference;
use feathrise_editor_assets::json_fields::{asset_path_field, visit_strings};
use std::sync::atomic::{AtomicBool, Ordering};

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
    let mut hits = Vec::new();
    let mut paths = Vec::new();
    let mut limited = false;
    visit_strings(source, Some(MAX_PATH_BYTES), &|| cancel.load(Ordering::Relaxed), |pointer, key, value, range| {
        if !asset_path_field(key) || value.is_empty() || target.is_some_and(|target| value.split('#').next() != Some(target)) {
            return Ok(());
        }
        if hits.len() >= maximum { limited = true; return Ok(()); }
        if target.is_none() && value.len() > MAX_PATH_BYTES { return Err("资产路径超过1024字节检查预算".into()); }
        let mut snippet: String = pointer.chars().take(201).collect();
        if snippet.len() < pointer.len() { snippet.push('…'); }
        hits.push(Hit { range, line: 0, column: 0, snippet });
        if target.is_none() { paths.push(value.to_owned()); }
        Ok(())
    })?;
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
    Ok((hits, paths, limited))
}
