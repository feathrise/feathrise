//! 文件整理的两阶段引用事务。确认的是当前实际目标和文件版本；目录、冲突命名和部分移动均以真实回执为准。

mod document;
mod relink;
#[cfg(test)]
mod tests;

use super::{Control, filesystem as files};
use feathrise_editor_project_browser_model::file_operations::*;
use feathrise_scene_content::{ContentAuthorLock, authoring_io::file_hash, authoring_transaction::{PreparedWrite, commit_prepared_writes_checked}};
use std::{collections::{BTreeMap, HashSet}, fs, path::{Path, PathBuf}, sync::Arc};

type Moves = BTreeMap<PathBuf, PathBuf>;

pub(super) fn execute(
    request: &FileOperationRequest, update: &ReferenceUpdate, roots: &[PathBuf], control: &Control<'_>,
    receipt: &mut FileOperationReceipt, current: &mut Option<PathBuf>,
) -> Result<(), String> {
    let root = match update { ReferenceUpdate::Inspect(root) => root, ReferenceUpdate::Confirm(old) | ReferenceUpdate::FilesOnly(old) => &old.root };
    if matches!(request.operation, FileOperation::Relink { .. }) {
        return relink::execute(request, root, roots, update, control, receipt);
    }
    let moves = destinations(&request.operation, roots, control)?;
    if matches!(update, ReferenceUpdate::Inspect(_))
        && !moves.keys().any(|source| platform_path(source).starts_with(platform_path(root))) {
        return files::execute_plain(request, roots, control, receipt, current, None);
    }
    let root = files::checked(root, roots, true)?;
    let _author_lock = ContentAuthorLock::acquire(&root)?;
    let preview = inspect(root, &request.operation, moves, control)?;
    match update {
        ReferenceUpdate::Inspect(_) if !preview.documents.is_empty() => {
            receipt.reference_preview = Some(Arc::new(preview));
            return Ok(());
        }
        ReferenceUpdate::Confirm(old) | ReferenceUpdate::FilesOnly(old) if old.moves != preview.moves
            || old.source_stamps != preview.source_stamps || old.documents != preview.documents
            || old.unrepresentable != preview.unrepresentable => {
            return Err("来源、引用文件或移动目标在确认后有变化，请重新发起操作查看最新预览".into());
        }
        _ => {}
    }
    if preview.documents.is_empty() || matches!(update, ReferenceUpdate::FilesOnly(_)) {
        return files::execute_plain(request, roots, control, receipt, current, Some(&preview));
    }
    if preview.unrepresentable { return Err("目标在资产根外或名称含片段分隔符，请重新选择目标或明确选择仅移动文件".into()); }
    let result = files::execute_plain(request, roots, control, receipt, current, Some(&preview));
    // 批处理中止后也必须结清已经真正移走的来源。其余来源仍使用原路径。
    let actual: Moves = receipt.completed.iter().filter(|c| !c.source_retained).filter_map(|c|
        Some((c.source.clone()?, c.destination.clone()?))).collect();
    if !actual.is_empty() {
        match publish(&preview, &actual, &|| Ok(())) {
            Ok(paths) => {
                receipt.repaired_documents = paths.len();
                receipt.repaired_paths = paths.into();
            }
            Err(error) => {
                let mut recovery_errors = Vec::new();
                receipt.completed.retain(|change| {
                    let (Some(source), Some(target)) = (&change.source, &change.destination) else { return true; };
                    if change.source_retained { return true; }
                    let restore = (|| {
                        files::reject_link_chain(target)?;
                        files::reject_link_chain(source.parent().ok_or("源文件缺少目录")?)?;
                        let metadata = fs::symlink_metadata(target).map_err(|e| e.to_string())?;
                        files::verify_source_stamp(target, preview.source_stamps[source], &metadata)?;
                        files::rename_checked(target, source, control.id())
                    })();
                    if let Err(e) = restore { recovery_errors.push(e); true } else { false }
                });
                if recovery_errors.is_empty() { return Err(format!("引用更新失败，已撤回本次移动：{error}")); }
                return Err(format!("引用更新失败：{error}。部分文件未能移回，实际位置见回执：{}", recovery_errors.join("；")));
            }
        }
    }
    result
}


fn destinations(operation: &FileOperation, roots: &[PathBuf], control: &Control<'_>) -> Result<Moves, String> {
    let mut moves = Moves::new();
    match operation {
        FileOperation::Rename { source, stamp, name } => {
            let (source, metadata) = files::checked_entry(source, roots)?;
            files::verify_source_stamp(&source, *stamp, &metadata)?;
            files::validate_name(name)?;
            let target = source.parent().ok_or("不能改名磁盘根")?.join(name);
            if target != source { moves.insert(source, target); }
        }
        FileOperation::Move { sources, destination, conflict } => {
            let destination = files::checked(destination, roots, true)?;
            let sources = files::sources_checked(sources, roots, control, &mut Vec::new(), &mut None)?;
            let mut occupied = HashSet::new();
            for (source, _) in sources {
                let target = destination.join(source.file_name().ok_or("源文件缺少名称")?);
                if target == source { continue; }
                if let Some(target) = files::planned_target(&target, *conflict, &occupied)? {
                    occupied.insert(path_key(&target));
                    moves.insert(source, target);
                }
            }
        }
        _ => return Err("仅重命名和移动使用引用事务".into()),
    }
    Ok(moves)
}

fn relocated(path: &Path, moves: &Moves) -> Option<PathBuf> {
    path.ancestors().find_map(|source| {
        let destination = moves.get(source)?;
        let rest = path.strip_prefix(source).ok()?;
        Some(if rest.as_os_str().is_empty() { destination.clone() } else { destination.join(rest) })
    })
}

fn inspect(root: PathBuf, operation: &FileOperation, moves: Moves, control: &Control<'_>) -> Result<ReferencePreview, String> {
    let relink = matches!(operation, FileOperation::Relink { .. });
    let source_stamps = moves.keys().filter(|_| !relink).map(|path| {
        let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        Ok((path.clone(), FileEntryStamp::from_metadata(Some(&metadata))))
    }).collect::<Result<_, String>>()?;
    let mut documents = Vec::new();
    let mut labels = std::collections::BTreeSet::new();
    let mut unrepresentable = false;
    let sources = document::SourcePaths::new(&root, &moves, relink);
    let mut dirs = Vec::new();
    for name in ["config", "scenes"] {
        let path = root.join(name);
        if path.try_exists().map_err(|e| e.to_string())? { files::reject_link_chain(&path)?; dirs.push(path); }
    }
    while let Some(dir) = dirs.pop() {
        control.check()?;
        files::reject_link_chain(&dir)?;
        for entry in fs::read_dir(&dir).map_err(|e| format!("无法检查 {}：{e}", dir.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            control.check()?;
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if files::reject_link(&path, &metadata).is_err() { continue; }
            if metadata.is_dir() { dirs.push(path); continue; }
            if !document::supported(&path) && !(relink && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))) { continue; }
            let output = document::rewrite(&path, &root, &sources, relink, &|| control.cancelled())?;
            if output.count != 0 {
                labels.extend(output.fragments);
                unrepresentable |= output.unrepresentable;
                documents.push(ReferencingDocument { digest: file_hash(&path)?, path, matches: output.count, sources: output.sources });
            }
        }
    }
    documents.sort_by(|a,b| a.path.cmp(&b.path));
    if relink && let Some(target) = moves.values().next() { relink::check_target(target, &documents, &labels)?; }
    let documents_selection = documents.iter().map(|d| platform_path(&d.path)).collect::<Vec<_>>().into();
    Ok(ReferencePreview { root, operation: operation.clone(), moves, source_stamps, replacement_digest: None, documents, documents_selection, unrepresentable })
}

fn publish(preview: &ReferencePreview, actual: &Moves, check: &dyn Fn() -> Result<(), String>) -> Result<Vec<PathBuf>, String> {
    let exact = matches!(preview.operation, FileOperation::Relink { .. });
    let mut writes = Vec::new();
    let mut baselines = BTreeMap::new();
    for doc in &preview.documents {
        if !doc.sources.iter().any(|(prefix, alias)| actual.contains_key(alias.as_ref().unwrap_or(prefix))) { continue; }
        let path = relocated(&doc.path, actual).unwrap_or_else(|| doc.path.clone());
        files::reject_link_chain(&path)?;
        let baseline = &doc.digest;
        // 父目录也可能在本批移动。按原文档类型解析，以新物理位置做精确摘要校验。
        if file_hash(&path)? != *baseline { return Err(format!("引用文件已被外部修改：{}", path.display())); }
        baselines.insert(path.clone(), baseline);
        // 每次只打开一份 GLB 并流式复制其二进制，避免总内存随全工程模型规模增长。
        let original_kind = &doc.path;
        let root = &preview.root;
        writes.push(PreparedWrite::streaming(path.clone(), move |stage| {
            let updated = document::rewrite_as(&path, original_kind, root, actual, &doc.sources, exact)?;
            if updated.count == 0 || updated.unrepresentable { return Err(format!("文档引用发生变化：{}", path.display())); }
            updated.write(stage)
        }));
    }
    commit_prepared_writes_checked(writes, |_, path| {
        check()?;
        files::reject_link_chain(path)?;
        if file_hash(path)? != *baselines[path] { return Err(format!("引用文件已被外部修改：{}", path.display())); }
        Ok(())
    }).map_err(|failure| failure.message().to_owned())?;
    Ok(baselines.into_keys().collect())
}
