//! 文件整理只在本轮独占临时目录验证；核对实际字节、边界拒绝和失败保留源。

use super::*;
use std::{fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

pub(super) struct Fixture { root: PathBuf, token: String }

impl Fixture {
    pub(super) fn new() -> Self {
        let token = format!("file-operation-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
        let root = std::env::temp_dir().join(&token);
        fs::create_dir(&root).unwrap();
        fs::write(root.join(".owner"), token.as_bytes()).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("destination")).unwrap();
        Self { root, token }
    }
    pub(super) fn request(&self, operation: FileOperation) -> FileOperationRequest {
        FileOperationRequest { context: 1, allowed_roots: vec![self.root.clone()], operation }
    }
    pub(super) fn path(&self, relative: &str) -> PathBuf { self.root.join(relative) }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // 递归清理前再次验证绝对路径和本轮归属，只删除本测试create_dir成功创建的目录。
        let Ok(root) = self.root.canonicalize() else { return; };
        let Ok(temp) = std::env::temp_dir().canonicalize() else { return; };
        if root.starts_with(&temp) && root != temp && fs::read(root.join(".owner")).ok().as_deref() == Some(self.token.as_bytes()) {
            let _ = fs::remove_dir_all(root);
        }
    }
}

fn execute(request: FileOperationRequest, cancelled: bool) -> (FileOperationReceipt, Result<(), String>) {
    let shared = Shared::default();
    let job = Job { id: 1, request, cancel: Arc::new(AtomicBool::new(cancelled)) };
    let mut receipt = FileOperationReceipt { id: 1, context: 1, kind: job.request.operation.kind(), completed: vec![], skipped: vec![], error: None, cancelled: false };
    let result = filesystem::execute(&job.request, &Control { shared: &shared, job: &job }, &mut receipt);
    (receipt, result)
}

#[test]
fn copy_move_and_conflicts_preserve_actual_file_bytes() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.path("source/nested")).unwrap();
    fs::write(fixture.path("source/nested/中文.txt"), b"actual disk content").unwrap();
    let operation = |conflict| FileOperation::Copy { sources: vec![fixture.path("source/nested")], destination: fixture.path("destination"), conflict };
    let (receipt, result) = execute(fixture.request(operation(ConflictPolicy::Fail)), false);
    result.unwrap();
    assert_eq!(receipt.completed.len(), 1);
    assert_eq!(fs::read(fixture.path("destination/nested/中文.txt")).unwrap(), b"actual disk content");
    assert!(fixture.path("source/nested/中文.txt").exists());
    assert!(execute(fixture.request(operation(ConflictPolicy::Fail)), false).1.is_err());
    let (receipt, result) = execute(fixture.request(operation(ConflictPolicy::Skip)), false);
    result.unwrap();
    assert_eq!(receipt.skipped.len(), 1);
    execute(fixture.request(operation(ConflictPolicy::KeepBoth)), false).1.unwrap();
    assert_eq!(fs::read(fixture.path("destination/nested (2)/中文.txt")).unwrap(), b"actual disk content");
    execute(fixture.request(FileOperation::Move { sources: vec![fixture.path("source/nested")], destination: fixture.path("destination"), conflict: ConflictPolicy::KeepBoth }), false).1.unwrap();
    assert!(!fixture.path("source/nested").exists());
    assert_eq!(fs::read(fixture.path("destination/nested (3)/中文.txt")).unwrap(), b"actual disk content");
}

#[test]
fn create_rename_and_parent_child_selection_use_real_paths() {
    let fixture = Fixture::new();
    execute(fixture.request(FileOperation::CreateFolder { parent: fixture.path("source"), name: "文件夹".into() }), false).1.unwrap();
    fs::write(fixture.path("source/文件夹/a.txt"), b"one copy").unwrap();
    execute(fixture.request(FileOperation::Rename { source: fixture.path("source/文件夹"), name: "renamed".into() }), false).1.unwrap();
    let (receipt, result) = execute(fixture.request(FileOperation::Copy { sources: vec![fixture.path("source/renamed"), fixture.path("source/renamed/a.txt")], destination: fixture.path("destination"), conflict: ConflictPolicy::Fail }), false);
    result.unwrap();
    assert_eq!(receipt.completed.len(), 1, "父目录已包含的文件不能再复制一份");
    assert!(fixture.path("destination/renamed/a.txt").exists());
    assert!(!fixture.path("destination/a.txt").exists());
    assert!(execute(fixture.request(FileOperation::CreateFolder { parent: fixture.path("source"), name: "../escape".into() }), false).1.is_err());
    assert!(execute(fixture.request(FileOperation::Rename { source: fixture.path("source/renamed/a.txt"), name: "CON.txt".into() }), false).1.is_err());
}

#[test]
fn create_files_write_empty_utf8_and_a_parser_validated_glb_scene() {
    let fixture = Fixture::new();
    for (kind, name) in [(NewFileKind::Text, "新建文本文档.txt"), (NewFileKind::Markdown, "新建Markdown.md"), (NewFileKind::Glb, "新建模型.glb")] {
        let (receipt, result) = execute(fixture.request(FileOperation::CreateFile { parent: fixture.path("source"), name: name.into(), kind }), false);
        result.unwrap();
        assert_eq!(receipt.kind, FileOperationKind::CreateFile);
        assert_eq!(receipt.completed.len(), 1);
        assert!(receipt.completed[0].source.is_none());
        assert!(!receipt.completed[0].source_retained);
        assert_eq!(receipt.completed[0].destination.as_ref(), Some(&fixture.path("source").canonicalize().unwrap().join(name)));
        let bytes = fs::read(fixture.path(&format!("source/{name}"))).unwrap();
        if kind == NewFileKind::Glb {
            // 解析实际文件并启用gltf的完整结构校验，不能只对magic或自行拼出的JSON断言。
            let glb = gltf::Gltf::from_slice(&bytes).expect("新建GLB必须可以作为独立文档打开");
            assert!(glb.blob.is_none(), "空场景不需要BIN块");
            assert_eq!(glb.scenes().count(), 1);
            assert_eq!(glb.default_scene().unwrap().index(), 0);
            assert_eq!(glb.default_scene().unwrap().nodes().count(), 0);
            assert_eq!(glb.nodes().count(), 0);
            assert_eq!(glb.meshes().count(), 0);
        } else {
            assert!(bytes.is_empty());
            assert_eq!(std::str::from_utf8(&bytes).unwrap(), "");
        }
    }
    assert_eq!(fs::read_dir(fixture.path("source")).unwrap().count(), 3, "成功后只留下正式文件，不留暂存目录");
}

#[test]
fn create_files_reject_conflicts_and_invalid_names_without_changing_existing_bytes() {
    let fixture = Fixture::new();
    for (kind, name) in [(NewFileKind::Text, "existing.txt"), (NewFileKind::Markdown, "existing.md"), (NewFileKind::Glb, "existing.glb")] {
        let path = fixture.path(&format!("source/{name}"));
        fs::write(&path, b"unrelated author bytes").unwrap();
        let (receipt, result) = execute(fixture.request(FileOperation::CreateFile { parent: fixture.path("source"), name: name.into(), kind }), false);
        assert!(result.is_err(), "新建不允许覆盖同名文件");
        assert!(receipt.completed.is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"unrelated author bytes");
    }
    fs::write(fixture.path("untouched.txt"), b"outside the chosen parent").unwrap();
    for name in ["../untouched.txt", "CON.txt", "name?.glb", "", "..", "trailing."] {
        let (receipt, result) = execute(fixture.request(FileOperation::CreateFile { parent: fixture.path("source"), name: name.into(), kind: NewFileKind::Text }), false);
        assert!(result.is_err(), "非法名称必须在写入前拒绝：{name}");
        assert!(receipt.completed.is_empty());
    }
    assert_eq!(fs::read(fixture.path("untouched.txt")).unwrap(), b"outside the chosen parent");
    assert_eq!(fs::read_dir(fixture.path("source")).unwrap().count(), 3, "失败不留下半文件或暂存目录");
}

#[test]
fn cancelled_or_unauthorized_file_creation_does_not_write_any_file() {
    let fixture = Fixture::new();
    for kind in [NewFileKind::Text, NewFileKind::Markdown, NewFileKind::Glb] {
        let operation = || FileOperation::CreateFile { parent: fixture.path("source"), name: "must-not-exist".into(), kind };
        let (receipt, result) = execute(fixture.request(operation()), true);
        assert!(result.is_err());
        assert!(receipt.completed.is_empty());
        let mut outside = fixture.request(operation());
        outside.allowed_roots = vec![fixture.path("destination")];
        let (receipt, result) = execute(outside, false);
        assert!(result.unwrap_err().contains("超出已打开目录"));
        assert!(receipt.completed.is_empty());
    }
    assert_eq!(fs::read_dir(fixture.path("source")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(fixture.path("destination")).unwrap().count(), 0);
}

#[test]
fn self_descendant_root_outside_and_cancelled_requests_never_remove_source() {
    let fixture = Fixture::new();
    fs::write(fixture.path("source/file.txt"), b"keep me").unwrap();
    let descendant = FileOperation::Move { sources: vec![fixture.path("source")], destination: fixture.path("source"), conflict: ConflictPolicy::Fail };
    assert!(execute(fixture.request(descendant), false).1.is_err());
    assert!(execute(fixture.request(FileOperation::Delete { sources: vec![fixture.root.clone()] }), false).1.is_err());
    let mut outside = fixture.request(FileOperation::Rename { source: fixture.path("source/file.txt"), name: "new.txt".into() });
    outside.allowed_roots = vec![fixture.path("destination")];
    assert!(execute(outside, false).1.is_err());
    let cancelled = fixture.request(FileOperation::Move { sources: vec![fixture.path("source/file.txt")], destination: fixture.path("destination"), conflict: ConflictPolicy::Fail });
    assert!(execute(cancelled, true).1.is_err());
    assert_eq!(fs::read(fixture.path("source/file.txt")).unwrap(), b"keep me");
    assert!(!fixture.path("destination/file.txt").exists());
}

#[test]
fn old_staging_directory_is_never_removed_on_name_collision() {
    let fixture = Fixture::new();
    fs::write(fixture.path("source/file.txt"), b"copy").unwrap();
    let old_stage = fixture.path(&format!("destination/.feathrise-copy-{}-1-0-0", std::process::id()));
    fs::create_dir(&old_stage).unwrap();
    fs::write(old_stage.join("unrelated.txt"), b"preserve").unwrap();
    execute(fixture.request(FileOperation::Copy { sources: vec![fixture.path("source/file.txt")], destination: fixture.path("destination"), conflict: ConflictPolicy::Fail }), false).1.unwrap();
    assert_eq!(fs::read(old_stage.join("unrelated.txt")).unwrap(), b"preserve");
}

#[test]
fn service_queue_has_backpressure_and_context_cancellation() {
    let fixture = Fixture::new();
    let backend = Backend::default();
    {
        let mut queue = backend.shared.queue.lock().unwrap();
        queue.running = true;
        for id in 0..MAX_QUEUE {
            queue.pending.push_back(Job { id: id as u64, request: fixture.request(FileOperation::CreateFolder { parent: fixture.path("source"), name: format!("folder{id}") }), cancel: Arc::new(AtomicBool::new(false)) });
        }
    }
    assert!(backend.is_busy());
    assert!(backend.enqueue(fixture.request(FileOperation::CreateFolder { parent: fixture.path("source"), name: "overflow".into() })).is_err());
    backend.cancel_context(1);
    assert!(backend.snapshot().unwrap().queued.iter().all(|job| job.cancel_requested));
    assert!(backend.enqueue(fixture.request(FileOperation::CreateFolder { parent: fixture.path("source"), name: "after-close".into() })).is_err());
    assert!(!Path::new(&fixture.path("source/overflow")).exists());
}

#[test]
fn unavailable_historical_root_does_not_block_valid_roots_or_grant_its_parent() {
    let fixture = Fixture::new();
    let missing = fixture.path("destination/no-longer-present");
    let mut request = fixture.request(FileOperation::CreateFolder { parent: fixture.path("source"), name: "valid-after-bad-address".into() });
    request.allowed_roots = vec![missing.clone(), fixture.path("source")];
    execute(request, false).1.unwrap();
    assert!(fixture.path("source/valid-after-bad-address").is_dir());

    fs::write(fixture.path("source/file.txt"), b"source stays inside valid root").unwrap();
    let mut request = fixture.request(FileOperation::Copy { sources: vec![fixture.path("source/file.txt")],
        destination: fixture.path("destination"), conflict: ConflictPolicy::Fail });
    request.allowed_roots = vec![missing, fixture.path("source")];
    let (receipt, result) = execute(request, false);
    assert!(result.unwrap_err().contains("超出已打开目录"), "失效子根不能授权其父目录");
    assert!(receipt.completed.is_empty());
    assert_eq!(fs::read(fixture.path("source/file.txt")).unwrap(), b"source stays inside valid root");
    assert!(!fixture.path("destination/file.txt").exists());

    fs::write(fixture.path("destination/outside.txt"), b"outside source must stay").unwrap();
    let mut request = fixture.request(FileOperation::Move { sources: vec![fixture.path("destination/outside.txt")],
        destination: fixture.path("source"), conflict: ConflictPolicy::Fail });
    request.allowed_roots = vec![fixture.path("destination/no-longer-present"), fixture.path("source")];
    assert!(execute(request, false).1.unwrap_err().contains("超出已打开目录"), "有效目标也不能授权越界源");
    assert_eq!(fs::read(fixture.path("destination/outside.txt")).unwrap(), b"outside source must stay");
    assert!(!fixture.path("source/outside.txt").exists());
}

#[test]
fn all_unavailable_or_file_roots_fail_before_any_mutation() {
    let fixture = Fixture::new();
    fs::write(fixture.path("destination/replaced-directory"), b"a file is not a directory root").unwrap();
    let mut request = fixture.request(FileOperation::CreateFolder { parent: fixture.path("source"), name: "must-not-create".into() });
    request.allowed_roots = vec![fixture.path("missing"), fixture.path("destination/replaced-directory")];
    let (receipt, result) = execute(request, false);
    assert!(result.unwrap_err().contains("均已失效或无法访问"));
    assert!(receipt.completed.is_empty());
    assert!(!fixture.path("source/must-not-create").exists());
    assert_eq!(fs::read(fixture.path("destination/replaced-directory")).unwrap(), b"a file is not a directory root");
}

#[cfg(windows)]
#[test]
fn case_only_rename_keeps_content_and_never_replaces_a_different_file() {
    let fixture = Fixture::new();
    fs::write(fixture.path("source/Case.TXT"), b"preserve case bytes").unwrap();
    execute(fixture.request(FileOperation::Rename { source: fixture.path("source/Case.TXT"), name: "case.txt".into() }), false).1.unwrap();
    let names = fs::read_dir(fixture.path("source")).unwrap().map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>();
    assert!(names.iter().any(|name| name == "case.txt"));
    assert_eq!(fs::read(fixture.path("source/case.txt")).unwrap(), b"preserve case bytes");
    fs::write(fixture.path("source/another.txt"), b"another author").unwrap();
    assert!(execute(fixture.request(FileOperation::Rename { source: fixture.path("source/case.txt"), name: "another.txt".into() }), false).1.is_err());
    assert_eq!(fs::read(fixture.path("source/another.txt")).unwrap(), b"another author");
    assert_eq!(fs::read(fixture.path("source/case.txt")).unwrap(), b"preserve case bytes");
}

#[cfg(windows)]
#[test]
fn directory_junction_at_leaf_or_inside_selected_tree_is_rejected_without_touching_target() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    fs::write(fixture.path("destination/author.txt"), b"junction target stays").unwrap();
    let junction = fixture.path("source/junction");
    // mklink/J仅创建本轮临时目录内的联接；它不需要开发者模式，覆盖Windows真实reparse目录行为。
    let result = std::process::Command::new("cmd").args(["/D", "/C"])
        .raw_arg(format!("mklink /J \"{}\" \"{}\"", junction.display(), fixture.path("destination").display()))
        .creation_flags(0x0800_0000).output().unwrap();
    assert!(result.status.success(), "无法创建本轮联接夹具（{}）：{} {}", result.status, String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    for selected in [junction, fixture.path("source")] {
        let (_, result) = execute(fixture.request(FileOperation::Copy { sources: vec![selected], destination: fixture.path("destination"), conflict: ConflictPolicy::Fail }), false);
        assert!(result.is_err(), "叶子和中间目录联接均不得作为普通目录遍历");
    }
    assert_eq!(fs::read(fixture.path("destination/author.txt")).unwrap(), b"junction target stays");
    assert!(!fixture.path("destination/source").exists());
}

#[cfg(windows)]
#[test]
#[ignore = "需要Windows开发者模式或符号链接创建权限；普通目录联接回归不需要该权限"]
fn file_symbolic_link_is_rejected_without_copying_its_target() {
    let fixture = Fixture::new();
    fs::write(fixture.path("destination/author.txt"), b"symbolic target stays").unwrap();
    std::os::windows::fs::symlink_file(fixture.path("destination/author.txt"), fixture.path("source/linked.txt"))
        .expect("本机须启用开发者模式或符号链接权限以创建真实文件链接夹具");
    let (_, result) = execute(fixture.request(FileOperation::Copy { sources: vec![fixture.path("source/linked.txt")], destination: fixture.path("destination"), conflict: ConflictPolicy::Fail }), false);
    assert!(result.is_err());
    assert!(!fixture.path("destination/linked.txt").exists());
    assert_eq!(fs::read(fixture.path("destination/author.txt")).unwrap(), b"symbolic target stays");
}
