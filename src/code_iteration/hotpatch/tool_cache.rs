//! 工具缓存只在独占暂存目录完整写入后发布；历史坏槽保留，不阻断其他工具链或覆盖活文件。

use super::{hash_file, read_bounded};
use crate::build_process::BuildControl;
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, os::windows::{ffi::OsStrExt, fs::MetadataExt}, path::{Path, PathBuf}};

const BINARY: &str = "lld-link.exe";
const RECEIPT: &str = "cache.json";
const RECOVERY_SLOTS: usize = 8;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt { format: u32, input_sha256: String, binary_sha256: String }

/// 旧固定别名只在字节确实匹配当前LLD时复用；不同工具链各有自己的内容地址。
pub(super) fn raw(tools: &Path, source: &Path, expected: &str, control: &BuildControl) -> Result<PathBuf, String> {
    ordinary_directory(tools)?;
    if let Some(path) = verified_legacy(&tools.join(BINARY), Some(expected), control)? { return Ok(path); }
    prepare(&tools.join("raw-link-v1").join(expected), expected, Some(expected), control, |output| {
        let mut input = File::open(source).map_err(|error| error.to_string())?;
        let mut output = OpenOptions::new().write(true).create_new(true).open(output).map_err(|error| error.to_string())?;
        let mut buffer = [0; 64 * 1024];
        loop {
            control.check()?;
            let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 { return output.sync_all().map_err(|error| error.to_string()); }
            output.write_all(&buffer[..count]).map_err(|error| error.to_string())?;
        }
    })
}

/// 旧代理完整副本继续使用；缺sidecar或半份binary不是已发布缓存，不能由exists()决定采用。
pub(super) fn proxy(tools: &Path, identity: &str, control: &BuildControl,
    build: impl FnOnce(&Path) -> Result<(), String>) -> Result<PathBuf, String> {
    ordinary_directory(tools)?;
    let root = tools.join("function-link-v1").join(identity);
    if let Some(path) = verified_legacy(&root.join(BINARY), None, control)? { return Ok(path); }
    prepare(&root, identity, None, control, build)
}

fn prepare(root: &Path, identity: &str, expected: Option<&str>, control: &BuildControl,
    build: impl FnOnce(&Path) -> Result<(), String>) -> Result<PathBuf, String> {
    control.check()?;
    if !valid_hash(identity) { return Err("工具缓存输入身份不是SHA256".into()); }
    // 逐层核对后再创建子目录，不能沿既有重解析点把新缓存写到target之外。
    let family = root.parent().ok_or("工具缓存缺少功能目录")?;
    ordinary_directory(family.parent().ok_or("工具缓存缺少工具根")?)?;
    create_directory(family)?;
    create_directory(root)?;
    // 固定顺序的有限槽只靠input与实际binary SHA认领；恢复后的路径也必须跨调用稳定，
    // 否则每次新的linker路径都会使Cargo重新判断构建身份。绝不扫描暂存目录或按mtime选版本。
    let candidates: Vec<_> = std::iter::once(root.join("ready"))
        .chain((1..=RECOVERY_SLOTS).map(|slot| root.join(format!("recovery-{slot:02}")))).collect();
    for candidate in &candidates {
        if let Some(path) = verified(candidate, identity, expected, control)? { return Ok(path); }
    }
    let staging = root.join(format!(".stage-{}", bevy::asset::uuid::Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    let binary = staging.join(BINARY);
    // 失败或取消可留下自己的暂存目录，但没有ready路径；以后不枚举或按mtime猜这些残留。
    build(&binary)?;
    control.check()?;
    let digest = hash_file(&binary, control)?;
    if expected.is_some_and(|expected| expected != digest) { return Err("新工具副本与原LLD摘要不一致".into()); }
    let receipt = serde_json::to_vec(&Receipt { format: 1, input_sha256: identity.into(), binary_sha256: digest })
        .map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(staging.join(RECEIPT)).map_err(|error| error.to_string())?;
    file.write_all(&receipt).and_then(|_| file.sync_all()).map_err(|error| error.to_string())?;
    drop(file);
    control.check()?;
    for candidate in &candidates {
        control.check()?;
        if publish_new(&staging, candidate).is_ok() {
            return verified(candidate, identity, expected, control)?.ok_or_else(|| "工具完整发布后校验失败".into());
        }
        // 多个宿主连恢复槽也按同一顺序竞争：只采用完整受检的winner，绝不覆盖旧槽或活目录。
        if let Some(path) = verified(candidate, identity, expected, control)? { return Ok(path); }
    }
    Err("工具缓存的完整发布槽均不可用；历史缓存和本轮暂存目录已保留".into())
}

fn verified(directory: &Path, identity: &str, expected: Option<&str>, control: &BuildControl) -> Result<Option<PathBuf>, String> {
    control.check()?;
    let result = (|| {
        ordinary_directory(directory)?;
        let receipt: Receipt = serde_json::from_slice(&read_bounded(&directory.join(RECEIPT), 1024)?)
            .map_err(|error| error.to_string())?;
        if receipt.format != 1 || receipt.input_sha256 != identity || !valid_hash(&receipt.binary_sha256)
            || expected.is_some_and(|expected| expected != receipt.binary_sha256) { return Err("工具缓存完成记录身份不符".into()); }
        checked_binary(&directory.join(BINARY), &receipt.binary_sha256, control)
    })();
    // 历史缓存不可读/不完整只影响该候选；取消和输入租约失效始终向上传播。
    control.check()?;
    Ok(result.ok())
}

fn verified_legacy(binary: &Path, expected: Option<&str>, control: &BuildControl) -> Result<Option<PathBuf>, String> {
    control.check()?;
    let result = (|| {
        ordinary_directory(binary.parent().ok_or("工具缓存缺少父目录")?)?;
        let digest = match expected {
            Some(expected) => expected.to_owned(),
            None => String::from_utf8(read_bounded(&binary.with_file_name("binary.sha256"), 128)?).map_err(|error| error.to_string())?,
        };
        if !valid_hash(&digest) { return Err("旧工具摘要无效".into()); }
        checked_binary(binary, &digest, control)
    })();
    control.check()?;
    Ok(result.ok())
}

pub(super) fn checked_binary(binary: &Path, digest: &str, control: &BuildControl) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(binary).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.file_attributes() & 0x400 != 0
        || binary.canonicalize().map_err(|error| error.to_string())? != binary || hash_file(binary, control)? != digest {
        return Err("工具缓存binary不是受检的完整副本".into());
    }
    Ok(binary.to_owned())
}

pub(super) fn ordinary_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 || path.canonicalize().map_err(|error| error.to_string())? != path {
        return Err("工具缓存目录不是当前target的实际目录".into());
    }
    Ok(())
}

pub(super) fn create_directory(path: &Path) -> Result<(), String> {
    match fs::create_dir(path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error.to_string()),
    }
    ordinary_directory(path)
}

fn valid_hash(value: &str) -> bool { value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')) }

// MoveFileW不带REPLACE_EXISTING：目录发布是同卷原子rename，旧文件、空目录和活目录都不能替换。
pub(super) fn publish_new(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" { fn MoveFileW(source: *const u16, destination: *const u16) -> i32; }
    let source: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<_> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileW(source.as_ptr(), destination.as_ptr()) } == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::sha;
    use std::{os::windows::fs::OpenOptionsExt, sync::{Arc, Barrier}};

    struct Fixture { directory: PathBuf, tools: PathBuf }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!("feathrise-tool-cache-{}", bevy::asset::uuid::Uuid::new_v4()));
            fs::create_dir_all(directory.join("hotpatch-tools")).unwrap();
            let directory = directory.canonicalize().unwrap();
            Self { tools: directory.join("hotpatch-tools"), directory }
        }
        fn root(&self, identity: &str) -> PathBuf { self.tools.join("function-link-v1").join(identity) }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.directory); } }

    #[test]
    fn different_raw_toolchains_keep_the_old_alias_and_reuse_their_complete_copies() {
        let fixture = Fixture::new(); let control = BuildControl::default();
        let old = fixture.tools.join(BINARY); let first = b"first-rust-toolchain"; let second = b"second-rust-toolchain";
        fs::write(&old, first).unwrap();
        let source = fixture.directory.join("rust-lld.exe"); fs::write(&source, second).unwrap();
        let lease = OpenOptions::new().read(true).share_mode(1).open(&old).unwrap();
        assert_eq!(raw(&fixture.tools, &source, &sha(first), &control).unwrap(), old);
        let current = raw(&fixture.tools, &source, &sha(second), &control).unwrap();
        assert_ne!(current, old);
        assert_eq!(fs::read(&current).unwrap(), second);
        assert_eq!(fs::read(&old).unwrap(), first);
        assert_eq!(raw(&fixture.tools, &source, &sha(second), &control).unwrap(), current);
        drop(lease);
    }

    #[test]
    fn complete_legacy_proxy_is_reused_without_rebuilding_or_rewriting_it() {
        let fixture = Fixture::new(); let identity = sha(b"same-proxy-inputs"); let root = fixture.root(&identity);
        fs::create_dir_all(&root).unwrap(); fs::write(root.join(BINARY), b"existing-proxy").unwrap();
        fs::write(root.join("binary.sha256"), sha(b"existing-proxy")).unwrap();
        let lease = OpenOptions::new().read(true).share_mode(1).open(root.join(BINARY)).unwrap();
        let binary = proxy(&fixture.tools, &identity, &BuildControl::default(), |_| panic!("完整旧副本不应再编译")).unwrap();
        assert_eq!(binary, root.join(BINARY)); assert!(!root.join("ready").exists());
        assert_eq!(fs::read(&binary).unwrap(), b"existing-proxy"); drop(lease);
    }

    #[test]
    fn missing_or_invalid_legacy_sidecar_cannot_poison_the_next_complete_publication() {
        for sidecar in [None, Some("not-a-sha256")] {
            let fixture = Fixture::new(); let identity = sha(b"proxy-inputs"); let root = fixture.root(&identity);
            fs::create_dir_all(&root).unwrap(); fs::write(root.join(BINARY), b"old-partial-binary").unwrap();
            if let Some(text) = sidecar { fs::write(root.join("binary.sha256"), text).unwrap(); }
            let binary = proxy(&fixture.tools, &identity, &BuildControl::default(), |output| {
                assert!(!root.join("ready").exists());
                fs::write(output, b"new-complete-binary").map_err(|error| error.to_string())
            }).unwrap();
            assert_eq!(binary, root.join("ready").join(BINARY));
            assert_eq!(fs::read(&binary).unwrap(), b"new-complete-binary");
            assert_eq!(fs::read(root.join(BINARY)).unwrap(), b"old-partial-binary");
            assert_eq!(fs::read_to_string(root.join("binary.sha256")).ok().as_deref(), sidecar);
        }
    }

    #[test]
    fn cancellation_and_partial_write_never_publish_a_ready_slot_and_a_retry_recovers() {
        for cancelled in [false, true] {
            let fixture = Fixture::new(); let identity = sha(b"cancelled-proxy-inputs"); let root = fixture.root(&identity);
            let control = BuildControl::default();
            let result = proxy(&fixture.tools, &identity, &control, |output| {
                fs::write(output, b"partial-output").unwrap();
                assert!(!root.join("ready").exists());
                if cancelled { control.cancel(); Ok(()) } else { Err("模拟写入失败".into()) }
            });
            assert!(result.is_err()); assert!(!root.join("ready").exists());
            let residue: Vec<_> = fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().path()).collect();
            assert_eq!(residue.len(), 1); assert_eq!(fs::read(residue[0].join(BINARY)).unwrap(), b"partial-output");
            let binary = proxy(&fixture.tools, &identity, &BuildControl::default(), |output|
                fs::write(output, b"retry-complete").map_err(|error| error.to_string())).unwrap();
            assert_eq!(fs::read(binary).unwrap(), b"retry-complete");
            assert_eq!(fs::read(residue[0].join(BINARY)).unwrap(), b"partial-output", "重试不能清理旧暂存目录");
        }
    }

    #[test]
    fn malformed_ready_keeps_one_stable_recovery_and_never_rebuilds_it_on_the_next_call() {
        let fixture = Fixture::new(); let identity = sha(b"proxy-inputs"); let root = fixture.root(&identity);
        let ready = root.join("ready"); fs::create_dir_all(&ready).unwrap(); fs::write(ready.join(BINARY), b"old-incomplete").unwrap();
        let binary = proxy(&fixture.tools, &identity, &BuildControl::default(), |output|
            fs::write(output, b"complete-generation").map_err(|error| error.to_string())).unwrap();
        assert_eq!(binary, root.join("recovery-01").join(BINARY));
        assert_eq!(fs::read(&binary).unwrap(), b"complete-generation");
        assert_eq!(fs::read(ready.join(BINARY)).unwrap(), b"old-incomplete"); assert!(!ready.join(RECEIPT).exists());
        let receipt: Receipt = serde_json::from_slice(&fs::read(binary.with_file_name(RECEIPT)).unwrap()).unwrap();
        assert_eq!(receipt.input_sha256, identity); assert_eq!(receipt.binary_sha256, sha(b"complete-generation"));
        assert_eq!(proxy(&fixture.tools, &identity, &BuildControl::default(), |_| panic!("坏ready不能让已完成恢复副本反复编译")).unwrap(), binary);
    }

    #[test]
    fn concurrent_complete_builds_and_recoveries_adopt_one_verified_stable_winner() {
        for recovering in [false, true] {
            let fixture = Fixture::new(); let identity = sha(b"concurrent-proxy-inputs"); let gate = Arc::new(Barrier::new(2));
            let root = fixture.root(&identity);
            if recovering { fs::create_dir_all(root.join("ready")).unwrap(); fs::write(root.join("ready").join(BINARY), b"old-bad-ready").unwrap(); }
            let mut workers = Vec::new();
            for contents in [b"first-complete".as_slice(), b"second-complete".as_slice()] {
                let tools = fixture.tools.clone(); let identity = identity.clone(); let gate = gate.clone();
                workers.push(std::thread::spawn(move || proxy(&tools, &identity, &BuildControl::default(), |output| {
                    fs::write(output, contents).unwrap(); gate.wait(); Ok(())
                }).unwrap()));
            }
            let first = workers.remove(0).join().unwrap(); let second = workers.remove(0).join().unwrap();
            assert_eq!(first, second); assert_eq!(first, root.join(if recovering { "recovery-01" } else { "ready" }).join(BINARY));
            let bytes = fs::read(&first).unwrap(); assert!(bytes == b"first-complete" || bytes == b"second-complete");
            let receipt: Receipt = serde_json::from_slice(&fs::read(first.with_file_name(RECEIPT)).unwrap()).unwrap();
            assert_eq!(receipt.binary_sha256, sha(&bytes));
            assert_eq!(proxy(&fixture.tools, &identity, &BuildControl::default(), |_| panic!("必须复用winner")).unwrap(), first);
            if recovering { assert_eq!(fs::read(root.join("ready").join(BINARY)).unwrap(), b"old-bad-ready"); }
        }
    }
}
