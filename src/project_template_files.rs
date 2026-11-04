//! 正式空工程模板的有界文件复制；取消时仅丢弃本轮未发布的临时工程。
use super::{CopyBudget, MAX_TEMPLATE_BYTES, MAX_TEMPLATE_FILES, reject_reparse};
use crate::build_process::BuildControl;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

pub(super) fn copy_file(
    source_root: &Path,
    target_root: &Path,
    relative: &str,
    budget: &mut CopyBudget,
    control: &BuildControl,
) -> Result<(), String> {
    control.check()?;
    let source = source_root.join(relative);
    let mut ancestor = Some(source.as_path());
    while let Some(path) = ancestor {
        reject_reparse(path)?;
        if path == source_root {
            break;
        }
        ancestor = path.parent();
    }
    let metadata = fs::metadata(&source).map_err(|e| format!("模板文件{relative}不可读：{e}"))?;
    if !metadata.is_file() {
        return Err(format!("模板文件{relative}必须是普通文件"));
    }
    budget.files += 1;
    budget.bytes = budget.bytes.saturating_add(metadata.len());
    if budget.files > MAX_TEMPLATE_FILES || budget.bytes > MAX_TEMPLATE_BYTES {
        return Err("模板文件超出512文件或512MiB模板预算".into());
    }
    let target = target_root.join(relative);
    fs::create_dir_all(target.parent().ok_or("模板文件目标缺少父目录")?)
        .map_err(|e| e.to_string())?;
    let mut input = fs::File::open(&source).map_err(|e| e.to_string())?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| format!("不能覆盖新工程的既有资产{relative}：{e}"))?;
    // 块边界检查取消，模板准备不占用UI线程。
    let mut bytes = [0u8; 128 * 1024];
    loop {
        control.check()?;
        let count = input.read(&mut bytes).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        output
            .write_all(&bytes[..count])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
