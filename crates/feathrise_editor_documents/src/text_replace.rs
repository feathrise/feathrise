//! 单文件与跨文件替换复用的保真算法，输入和结果均为原作者内容值。

use feathrise_editor_documents_model::{
    constants::MAX_FILE_BYTES, history::types::Selection, navigation::types::ReplacedContent,
};
use std::{ops::Range, sync::Arc};

fn too_large() -> String {
    "替换后的完整文件超过256 KiB；草稿保持，请缩小范围或减少替换内容".into()
}

/// 两种替换入口复用同一保真算法；调用者只提交已受检、按UTF-8顺序排列的非重叠范围。
pub fn prepare_ranges(
    source: &str,
    pages: &[Arc<str>],
    crlf: bool,
    bom: bool,
    ranges: &[Range<usize>],
    replacement: &str,
    selection: Selection,
    scope: Option<&Range<usize>>,
    single: bool,
) -> Result<Option<ReplacedContent>, String> {
    let mut previous = 0;
    if ranges.iter().any(|range| {
        let invalid = range.start < previous
            || range.start >= range.end
            || source.get(range.clone()).is_none();
        previous = range.end;
        invalid
    }) {
        return Err("替换范围已失效；草稿保持".into());
    }
    if ranges.is_empty()
        || ranges
            .iter()
            .all(|range| &source[range.clone()] == replacement)
    {
        return Ok(None); // 同文替换不增加修订、脏位、历史或原生重排。
    }
    let limit = MAX_FILE_BYTES - usize::from(bom) * 3;
    let removed: usize = ranges.iter().map(|range| range.len()).sum();
    let size = replacement
        .len()
        .checked_mul(ranges.len())
        .and_then(|added| (source.len() - removed).checked_add(added))
        .filter(|size| *size <= limit)
        .ok_or_else(too_large)?;
    let raw: String = pages.iter().map(AsRef::as_ref).collect();
    let raw_replacement = if crlf {
        replacement.replace('\n', "\r\n")
    } else {
        replacement.to_owned()
    };
    let mut output = String::with_capacity(size);
    let mut bytes = String::with_capacity(limit.min(raw.len().saturating_add(size)));
    let positions = [
        selection.anchor,
        selection.focus,
        scope.map_or(0, |range| range.start),
        scope.map_or(0, |range| range.end),
    ];
    let mut mapped = [None; 4];
    let (mut source_end, mut raw_end, mut norm_cursor, mut raw_cursor, mut count) = (0, 0, 0, 0, 0);
    for range in ranges {
        let start = raw_offset(&raw, range.start, &mut norm_cursor, &mut raw_cursor);
        let end = raw_offset(&raw, range.end, &mut norm_cursor, &mut raw_cursor);
        output.push_str(&source[source_end..range.start]);
        if bytes.len() + start - raw_end > limit {
            return Err(too_large());
        }
        bytes.push_str(&raw[raw_end..start]);
        let changed = &source[range.clone()] != replacement;
        let insertion = if changed {
            raw_replacement.as_str()
        } else {
            &raw[start..end]
        };
        if bytes.len() + insertion.len() > limit {
            return Err(too_large());
        }
        bytes.push_str(insertion);
        for (mapped, position) in mapped.iter_mut().zip(positions) {
            if mapped.is_none() && position <= range.end {
                *mapped = Some(if position <= range.start {
                    output.len() - (range.start - position)
                } else if changed {
                    output.len() + replacement.len() // 区间内端点归到插入末端，不能落在中文UTF-8中间。
                } else {
                    output.len() + position - range.start
                });
            }
        }
        output.push_str(replacement);
        count += usize::from(changed);
        source_end = range.end;
        raw_end = end;
    }
    output.push_str(&source[source_end..]);
    if bytes.len() + raw.len() - raw_end > limit {
        return Err(too_large());
    }
    bytes.push_str(&raw[raw_end..]);
    for (mapped, position) in mapped.iter_mut().zip(positions) {
        if mapped.is_none() {
            *mapped = Some(output.len() - (source.len() - position));
        }
    }
    let selection = if single {
        let range = &ranges[0];
        Selection {
            anchor: range.start,
            focus: range.start + replacement.len(),
        }
    } else {
        Selection {
            anchor: mapped[0].unwrap(),
            focus: mapped[1].unwrap(),
        }
    };
    Ok(Some(ReplacedContent {
        source: Arc::from(output),
        pages: vec![Arc::from(bytes)],
        selection,
        scope: scope.map(|_| mapped[2].unwrap()..mapped[3].unwrap()),
        count,
    }))
}

fn raw_offset(raw: &str, target: usize, normalized: &mut usize, cursor: &mut usize) -> usize {
    while *normalized < target {
        *cursor += if raw.as_bytes()[*cursor..].starts_with(b"\r\n") {
            2
        } else {
            1
        };
        *normalized += 1;
    }
    *cursor
}
