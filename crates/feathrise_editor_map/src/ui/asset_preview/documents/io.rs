//! 后台受检读取与原子保存；磁盘并发改动、越界和失败均保留原文件及草稿。

use super::{constants::*, types::*};
use crate::asset_catalog::document_format;
use feathrise_map_content::authoring_transaction::{PreparedWrite, commit_prepared_writes_checked};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if document_format(relative).is_none()
        || Path::new(relative)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("只能读取文档根内受支持的文本文件相对路径".into());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("无法打开文档根目录：{error}"))?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("无法打开文档：{error}"))?;
    if !path.starts_with(&root) {
        return Err("文档路径超出所绑定目录".into());
    }
    Ok(path)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("文档读取失败：{error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("读取文档属性失败：{error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err("文档超过256 KiB，请在外部编辑器中打开".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("文档读取失败：{error}"))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err("文档超过256 KiB，请在外部编辑器中打开".into());
    }
    Ok(bytes)
}

/// 后台按Unicode与行边界保存源分块；界面不分页，快照拼接精确还原原文件。
pub(super) fn source_pages(text: &str) -> Vec<Arc<str>> {
    let mut pages = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = text.len();
        let mut last_line = None;
        for (count, (offset, ch)) in text[start..].char_indices().enumerate() {
            if count == SOURCE_PAGE_CHARS {
                end = start + offset;
                break;
            }
            if ch == '\n' {
                last_line = Some(start + offset + 1);
            }
        }
        if end < text.len() {
            if let Some(line) = last_line.filter(|line| *line > start + (end - start) / 2) {
                end = line;
            } else if text[..end].ends_with('\r') && text[end..].starts_with('\n') {
                end -= 1;
            }
        }
        pages.push(Arc::from(&text[start..end]));
        start = end;
    }
    if pages.is_empty() {
        pages.push(Arc::from(""));
    }
    pages
}

pub(super) fn load(root: &Path, relative: &str) -> Result<(DiskIdentity, SourceContents), String> {
    let path = checked_path(root, relative)?;
    let bytes = read_bytes(&path)?;
    let bom = bytes.starts_with(b"\xef\xbb\xbf");
    let text = std::str::from_utf8(if bom { &bytes[3..] } else { &bytes })
        .map_err(|_| "文档不是UTF-8编码，请先在外部编辑器转换编码".to_owned())?;
    if text.contains('\0') {
        return Err("文件包含二进制字符，不能作为文本编辑".into());
    }
    // 混合文件的新换行遵循主要格式，而非碰巧位于第一行的格式；平票时沿用首个换行。
    // 统计留在原受检读取的IO任务内，原始字节和正文LF坐标仍保持各自的来源。
    let (mut crlf_count, mut lf_count, mut first_crlf) = (0, 0, None);
    for (index, byte) in text.bytes().enumerate() {
        if byte != b'\n' { continue; }
        let crlf = index > 0 && text.as_bytes()[index - 1] == b'\r';
        first_crlf.get_or_insert(crlf);
        if crlf { crlf_count += 1; } else { lf_count += 1; }
    }
    let crlf = crlf_count > lf_count || crlf_count == lf_count && first_crlf.unwrap_or(false);
    let disk = DiskIdentity {
        path,
        digest: Sha256::digest(&bytes).into(),
        bom,
        crlf,
    };
    Ok((
        disk,
        SourceContents {
            pages: source_pages(text),
            text: Arc::from(text.replace("\r\n", "\n")),
        },
    ))
}

pub(super) fn load_source(roots: &super::source::SourceRoots, key: &str) -> Result<(DiskIdentity, SourceContents), String> {
    let (root, relative) = roots.resolve(key)?;
    load(root, relative)
}

pub(super) fn save_source(roots: &super::source::SourceRoots, key: &str, disk: &DiskIdentity, pages: &[Arc<str>]) -> Result<DiskIdentity, String> {
    let (root, relative) = roots.resolve(key)?;
    save(root, relative, disk, pages)
}

/// 只替换已确认输入的最小Unicode区间；区间外的混合换行与原字节保持原样。
/// 文件/输入缓冲有固定上限，稳定帧和只移动光标时不会进入这条路径。
pub(super) fn edited_source(
    pages: &[Arc<str>],
    before: &str,
    after: &str,
    crlf: bool,
) -> Arc<str> {
    let mut prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(prefix) || !after.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = before[prefix..]
        .bytes()
        .rev()
        .zip(after[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(before.len() - suffix)
        || !after.is_char_boundary(after.len() - suffix)
    {
        suffix -= 1;
    }
    let raw: String = pages.iter().map(AsRef::as_ref).collect();
    let raw_offset = |normalized: usize| {
        let mut offset = normalized;
        for (carriage, _) in raw.match_indices("\r\n") {
            if carriage >= offset {
                break;
            }
            offset += 1;
        }
        offset
    };
    let start = raw_offset(prefix);
    let end = raw_offset(before.len() - suffix);
    let changed = &after[prefix..after.len() - suffix];
    let changed = if crlf {
        changed.replace('\n', "\r\n")
    } else {
        changed.into()
    };
    let mut output = String::with_capacity(start + changed.len() + raw.len() - end);
    output.push_str(&raw[..start]);
    output.push_str(&changed);
    output.push_str(&raw[end..]);
    Arc::from(output)
}

pub(super) fn save(
    root: &Path,
    relative: &str,
    disk: &DiskIdentity,
    pages: &[Arc<str>],
) -> Result<DiskIdentity, String> {
    let mut bytes = Vec::new();
    if disk.bom {
        bytes.extend_from_slice(b"\xef\xbb\xbf");
    }
    for page in pages {
        if bytes.len() + page.len() > MAX_FILE_BYTES {
            return Err("保存内容超过256 KiB；草稿已保留，请减少内容后再保存".into());
        }
        bytes.extend_from_slice(page.as_bytes());
    }
    // 检查在stage创建之前和每次原子替换重试之前执行；外部文件变更时拒绝覆盖。
    let verify = || -> Result<(), String> {
        let path = checked_path(root, relative)?;
        if path != disk.path || Sha256::digest(read_bytes(&path)?).as_slice() != disk.digest {
            return Err(
                "磁盘文档已被外部修改，保存已停止；草稿保留，可先复制内容再重新载入".into(),
            );
        }
        if path
            .metadata()
            .map_err(|error| error.to_string())?
            .permissions()
            .readonly()
        {
            return Err("文档为只读，无法保存；草稿已保留".into());
        }
        Ok(())
    };
    verify()?;
    let next = DiskIdentity {
        digest: Sha256::digest(&bytes).into(),
        ..disk.clone()
    };
    commit_prepared_writes_checked(
        vec![PreparedWrite::new(disk.path.clone(), bytes)],
        |_, _| verify(),
    )
    .map_err(|error| format!("保存失败：{}", error.message()))?;
    Ok(next)
}
