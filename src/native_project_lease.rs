//! 独立作者实例的工程写租约；原生句柄锁随实例释放，残留文件不代表活实例。

use crate::project::Project;
use std::fs::{File, OpenOptions};
use std::io::Write;
#[cfg(windows)]
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
#[cfg(unix)]
use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};

const LOCK_FILE: &str = "author-instance.lock";
#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x1;
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// 由native_editor::run局部持有至App退出；不能Clone或按PID／文件时间推断所有权。
#[derive(Debug)]
pub(crate) struct NativeProjectLease {
    _owner: LockedAuthorPath,
    _legacy: Option<LockedAuthorPath>,
}

#[derive(Debug)]
struct LockedAuthorPath {
    file: File,
    // Unix 可以移除活文件，目录锁同时阻断对旧锁文件的删除绕过。
    #[cfg(unix)]
    _directory: File,
}

impl NativeProjectLease {
    /// 只在作者实例初始化调用；未取得独占写权限前不得安装作者插件或绑定工程全局路径。
    pub(crate) fn acquire(project: &Project) -> Result<Self, String> {
        if crate::project::open(&project.root)? != *project {
            return Err("工程清单已变化，请重新打开后再进入编辑器".into());
        }
        let directory = project.context.author_lease_directory(true)?;
        let path = directory.join(LOCK_FILE);
        let mut owner = lock_author_path(project, &directory, &path, true)?;

        // 旧版曾把锁放在当前清单解析出的作者目录，默认布局曾使用 UUID 子目录。
        // 已存在的旧锁只持有句柄，避免原位启动旧版时产生第二个写入者。
        let legacy_directory = match project.context.checked_legacy_data_directory()? {
            Some(directory) => directory,
            None => project.context.checked_data_directory()?,
        };
        let legacy = if legacy_directory != directory {
            let directory = legacy_directory;
            let path = directory.join(LOCK_FILE);
            match std::fs::symlink_metadata(&path) {
                Ok(_) => Some(lock_author_path(project, &directory, &path, false)?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(format!("无法检查旧工程写租约：{error}")),
            }
        } else {
            None
        };
        if project.context.author_lease_directory(false)? != directory
            || crate::project::open(&project.root)? != *project
        {
            return Err("取得写租约期间工程路径或清单已变化，请重新打开工程".into());
        }
        let diagnostic = serde_json::to_vec(&serde_json::json!({
            "process_id": std::process::id(),
            "project_id": project.id,
            "project_root": project.root,
        }))
        .map_err(|error| format!("无法生成工程写租约诊断：{error}"))?;
        owner.file
            .set_len(0)
            .map_err(|error| format!("无法更新工程写租约诊断：{error}"))?;
        owner.file
            .write_all(&diagnostic)
            .and_then(|()| owner.file.sync_all())
            .map_err(|error| format!("无法写入工程写租约诊断：{error}"))?;
        Ok(Self { _owner: owner, _legacy: legacy })
    }
}

fn lock_author_path(
    project: &Project,
    #[cfg_attr(windows, allow(unused_variables))] directory: &std::path::Path,
    path: &std::path::Path,
    create: bool,
) -> Result<LockedAuthorPath, String> {
    // 文件是否存在仅决定兼容检查范围。活租约由原生锁或不可共享的句柄判定。

    #[cfg(unix)]
    let directory_owner = {
        let owner = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory).map_err(|error| format!("无法打开工程租约目录：{error}"))?;
        lock_unix(&owner).map_err(|error| lease_error(project, path, error))?;
        owner
    };
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create).truncate(false);
    #[cfg(windows)]
    options.share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let owner = options.open(path)
        .map_err(|error| match error.raw_os_error() {
            Some(32 | 33) => format!(
                "工程“{}”已在其他编辑器实例中打开，请返回原窗口：{}",
                project.name,
                project.root.display()
            ),
            _ => format!("无法取得工程写租约{}：{error}", path.display()),
        })?;
    let metadata = owner
        .metadata()
        .map_err(|error| format!("无法检查工程写租约：{error}"))?;
    #[cfg(unix)]
    lock_unix(&owner).map_err(|error| lease_error(project, path, error))?;
    if !metadata.is_file() || is_reparse(&metadata)
        || path.canonicalize().map_err(|error| format!("无法定位工程写租约：{error}"))? != path
    {
        return Err("工程写租约必须位于经过校验的普通文件中".into());
    }
    Ok(LockedAuthorPath { file: owner, #[cfg(unix)] _directory: directory_owner })
}

#[cfg(windows)]
fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(unix)]
fn is_reparse(_metadata: &std::fs::Metadata) -> bool { false } // O_NOFOLLOW已在打开时拒绝符号链接。

#[cfg(unix)]
fn lock_unix(owner: &File) -> Result<(), std::io::Error> {
    if unsafe { libc::flock(owner.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 { Ok(()) }
    else { Err(std::io::Error::last_os_error()) }
}

#[cfg(unix)]
fn lease_error(project: &Project, path: &std::path::Path, error: std::io::Error) -> String {
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        format!("工程“{}”已在其他编辑器实例中打开，请返回原窗口：{}", project.name, project.root.display())
    } else { format!("无法取得工程写租约{}：{error}", path.display()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        project: Project,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            Self::with_data(label, ".editor")
        }

        fn with_data(label: &str, data: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "new-editor-writer-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("assets")).unwrap();
            std::fs::write(root.join("assets/keep.txt"), "作者数据保持原样").unwrap();
            std::fs::write(
                root.join("Cargo.toml"),
                "[package]\nname=\"writer_fixture\"\nversion=\"0.1.0\"\n",
            )
            .unwrap();
            Self { project: Self::write_manifest(&root, data) }
        }

        fn write_manifest(root: &std::path::Path, data: &str) -> Project {
            let manifest = serde_json::json!({
                "format": 2,
                "sdk_abi": crate::project::LEGACY_SDK_ABI,
                "id": "1d4aefae-890c-45d2-9dcb-11ad2a1a3b40",
                "name": "写租约回归",
                "cargo_manifest": "Cargo.toml",
                "package": "writer_fixture",
                "asset_roots": ["assets"],
                "editor_data": data
            });
            std::fs::write(root.join("editor-project.json"), serde_json::to_vec(&manifest).unwrap())
                .unwrap();
            crate::project::open(root).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // 只回收夹具精确创建的几个文件和空目录，绝不递归删除作者目录。
            let project = &self.project;
            if let Ok(directory) = project.context.author_lease_directory(false) {
                let _ = std::fs::remove_file(directory.join(LOCK_FILE));
                let _ = std::fs::remove_dir(&directory);
                if let Some(parent) = directory.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
            }
            let legacy = match project.context.checked_legacy_data_directory() {
                Ok(Some(directory)) => Some(directory),
                Ok(None) => project.context.checked_data_directory().ok(),
                Err(_) => None,
            };
            if let Some(directory) = legacy {
                let _ = std::fs::remove_file(directory.join(LOCK_FILE));
                let _ = std::fs::remove_dir(&directory);
                let _ = std::fs::remove_dir(project.context.author_data_root());
            }
            for relative in ["assets/keep.txt", "Cargo.toml", "editor-project.json"] {
                let _ = std::fs::remove_file(project.root.join(relative));
            }
            let _ = std::fs::remove_dir(project.root.join("assets"));
            let _ = std::fs::remove_dir(&project.root);
        }
    }

    #[test]
    fn second_author_is_rejected_and_release_allows_reopening() {
        let fixture = Fixture::new("exclusive");
        let first = NativeProjectLease::acquire(&fixture.project).unwrap();
        let path = fixture.project.root.join("target/feathrise").join(LOCK_FILE);
        assert!(
            NativeProjectLease::acquire(&fixture.project)
                .unwrap_err()
                .contains("其他编辑器实例")
        );
        #[cfg(windows)]
        assert!(std::fs::remove_file(&path).is_err()); // Windows活锁不能通过替换路径绕过。
        drop(first);
        assert!(path.is_file()); // 文件残留不代表仍有实例。
        let _reopened = NativeProjectLease::acquire(&fixture.project).unwrap();
        assert!(path.is_file());
        assert_eq!(
            std::fs::read_to_string(fixture.project.root.join("assets/keep.txt")).unwrap(),
            "作者数据保持原样"
        );
    }

    #[test]
    fn default_author_lease_uses_target_and_excludes_a_live_legacy_writer() {
        let fixture = Fixture::with_data("legacy-lease", ".feathrise");
        let project = &fixture.project;
        let owner = NativeProjectLease::acquire(project).unwrap();
        let new = project.root.join("target/feathrise/author-instance.lock");
        assert!(new.is_file());
        assert!(!project.root.join(".feathrise").exists(), "租约不创建配置目录");
        drop(owner);

        let legacy = project.context.checked_legacy_data_directory().unwrap().unwrap();
        std::fs::create_dir_all(&legacy).unwrap();
        let path = legacy.join(LOCK_FILE);
        std::fs::write(&path, b"old process diagnostic").unwrap();
        let old = lock_author_path(project, &legacy, &path, false).unwrap();
        assert!(NativeProjectLease::acquire(project).unwrap_err().contains("其他编辑器实例"));
        drop(old);

        let owner = NativeProjectLease::acquire(project).unwrap();
        assert!(lock_author_path(project, &legacy, &path, false).unwrap_err().contains("其他编辑器实例"));
        drop(owner);
        assert_eq!(std::fs::read(path).unwrap(), b"old process diagnostic");
    }

    #[test]
    fn changing_editor_data_cannot_open_a_second_writer() {
        let fixture = Fixture::with_data("changed-preferences", ".feathrise");
        let root = &fixture.project.root;
        let first = NativeProjectLease::acquire(&fixture.project).unwrap();
        let custom = Fixture::write_manifest(root, "private-author");
        assert_ne!(fixture.project.context.editor_data, custom.context.editor_data);
        assert!(NativeProjectLease::acquire(&custom).unwrap_err().contains("其他编辑器实例"));
        assert!(!root.join("private-author").exists());

        drop(first);
        let next = NativeProjectLease::acquire(&custom).unwrap();
        let default = Fixture::write_manifest(root, ".feathrise");
        assert!(NativeProjectLease::acquire(&default).unwrap_err().contains("其他编辑器实例"));
        drop(next);
        let _reopened = NativeProjectLease::acquire(&default).unwrap();
    }

    #[test]
    fn stale_file_is_reused_and_a_copied_project_has_its_own_lease() {
        let a = Fixture::with_data("stale", "private-author");
        let b = Fixture::new("copy"); // 相同UUID但不同真实工程目录不能互相抢占。
        let legacy = a.project.context.ensure_data_directory().unwrap();
        let legacy_file = legacy.join(LOCK_FILE);
        std::fs::write(&legacy_file, b"old process diagnostic").unwrap();
        let old = lock_author_path(&a.project, &legacy, &legacy_file, false).unwrap();
        assert!(NativeProjectLease::acquire(&a.project).unwrap_err().contains("其他编辑器实例"));
        drop(old);
        let path = a.project.context.author_lease_directory(true).unwrap().join(LOCK_FILE);
        std::fs::write(&path, b"previous owner").unwrap();
        let _first = NativeProjectLease::acquire(&a.project).unwrap();
        let _other = NativeProjectLease::acquire(&b.project).unwrap();
        assert!(!a.project.root.join(".editor").exists());
        assert_ne!(a.project.context.editor_data, b.project.context.editor_data);
        assert_eq!(std::fs::read(legacy_file).unwrap(), b"old process diagnostic");
        let diagnostic: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).unwrap(),
        )
        .unwrap();
        assert_eq!(diagnostic["process_id"], std::process::id());
        assert_eq!(
            std::fs::read_to_string(b.project.root.join("assets/keep.txt")).unwrap(),
            "作者数据保持原样"
        );
    }
}
