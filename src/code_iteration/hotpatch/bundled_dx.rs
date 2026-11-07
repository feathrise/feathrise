//! 在后台完整释放内嵌 DX 与本地 CRT，按内容摘要复用缓存，拒绝半包与被替换的工具。
use super::tool_cache;
use crate::build_process::BuildControl;
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}};

struct Payload { name: &'static str, bytes: usize, sha256: &'static str, compressed: &'static [u8] }
include!(concat!(env!("OUT_DIR"), "/embedded_dx.rs"));

pub(super) fn resolve(target: &Path, legacy: &Path, control: &BuildControl) -> Result<PathBuf, String> {
    select(std::env::var_os("FEATHRISE_DX_PATH").map(PathBuf::from), target, legacy, control)
}

fn select(explicit: Option<PathBuf>, target: &Path, legacy: &Path, control: &BuildControl) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return path.canonicalize().map_err(|error| format!("显式 DX 路径不可用：{error}"));
    }
    if PAYLOAD.is_empty() {
        return legacy.canonicalize().map_err(|error| format!("此源码构建未内嵌 DX，开发工具不可用：{error}"));
    }
    std::hint::black_box(MARKER);
    materialize(target, IDENTITY, PAYLOAD, control)
}

fn verify(directory: &Path, payload: &[Payload], control: &BuildControl) -> Result<bool, String> {
    control.check()?;
    let valid = (|| {
        tool_cache::ordinary_directory(directory)?;
        for file in payload {
            let path = directory.join(file.name);
            if fs::metadata(&path).map_err(|error| error.to_string())?.len() != file.bytes as u64 {
                return Err("内嵌 DX 缓存长度不符".to_owned());
            }
            tool_cache::checked_binary(&path, file.sha256, control)?;
        }
        Ok::<_, String>(())
    })().is_ok();
    control.check()?;
    Ok(valid)
}

fn materialize(target: &Path, identity: &str, payload: &[Payload], control: &BuildControl) -> Result<PathBuf, String> {
    // 所有 I/O、解压和摘要都由既有后台构建槽执行，64 KiB 一次检查取消，不阻塞作者帧。
    control.check()?;
    tool_cache::ordinary_directory(target)?;
    let tools = target.join("hotpatch-tools");
    tool_cache::create_directory(&tools)?;
    let family = tools.join("embedded-dx-0.7.9");
    tool_cache::create_directory(&family)?;
    let root = family.join(identity);
    tool_cache::create_directory(&root)?;
    let candidates: Vec<_> = std::iter::once(root.join("ready"))
        .chain((1..=8).map(|index| root.join(format!("recovery-{index:02}")))).collect();
    for candidate in &candidates {
        if verify(candidate, payload, control)? { return Ok(candidate.join("dx.exe")); }
    }
    let staging = root.join(format!(".stage-{}", bevy::asset::uuid::Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    for file in payload {
        let mut decoder = flate2::read::DeflateDecoder::new(file.compressed);
        let mut output = fs::OpenOptions::new().write(true).create_new(true).open(staging.join(file.name))
            .map_err(|error| error.to_string())?;
        let mut buffer = [0; 64 * 1024];
        let mut total = 0;
        loop {
            control.check()?;
            let count = decoder.read(&mut buffer).map_err(|error| format!("内嵌 DX 解压失败：{error}"))?;
            if count == 0 { break; }
            total += count;
            if total > file.bytes { return Err("内嵌 DX 解压超出固定大小".into()); }
            output.write_all(&buffer[..count]).map_err(|error| error.to_string())?;
        }
        output.sync_all().map_err(|error| error.to_string())?;
    }
    if !verify(&staging, payload, control)? { return Err("内嵌 DX 完整性检查失败".into()); }
    // 只有全体文件匹配后才能原子发布。并发实例采用同一完整胜出版本，坏缓存留在旧槽。
    for candidate in &candidates {
        control.check()?;
        if tool_cache::publish_new(&staging, candidate).is_ok() || verify(candidate, payload, control)? {
            return Ok(candidate.join("dx.exe"));
        }
    }
    Err("内嵌 DX 缓存的发布槽均不可用".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_tool_works_without_external_path_and_repairs_a_corrupt_cache() {
        if PAYLOAD.is_empty() { return; }
        let directory = std::env::temp_dir().join(format!("feathrise-embedded-dx-{}", bevy::asset::uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let target = directory.canonicalize().unwrap();
        let control = BuildControl::default();
        let path = select(None, &target, Path::new("missing-development-tool"), &control).unwrap();
        let output = std::process::Command::new(&path).arg("--version").output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).split_whitespace().any(|part| part == "0.7.9"));
        assert!(path.with_file_name("vcruntime140.dll").is_file());
        assert_eq!(select(None, &target, Path::new("missing"), &control).unwrap(), path);
        fs::write(path.with_file_name("vcruntime140.dll"), b"corrupt").unwrap();
        let recovered = select(None, &target, Path::new("missing"), &control).unwrap();
        assert_ne!(recovered, path);
        assert_eq!(select(None, &target, Path::new("missing"), &control).unwrap(), recovered);
        let cancelled = BuildControl::default();
        cancelled.cancel();
        assert!(select(None, &target, Path::new("missing"), &cancelled).is_err());
        fs::remove_dir_all(&directory).unwrap();
    }
}
