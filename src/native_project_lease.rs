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
    _owner: File,
    // Unix允许 unlink 活文件，因此还锁定已校验作者目录的 inode；删掉诊断文件不能绕过租约。
    #[cfg(unix)]
    _directory: File,
}

impl NativeProjectLease {
    /// 只在作者实例初始化调用；未取得独占写权限前不得安装作者插件或绑定工程全局路径。
    pub(crate) fn acquire(project: &Project) -> Result<Self, String> {
        if crate::project::open(&project.root)? != *project {
            return Err("工程清单已变化，请重新打开后再进入编辑器".into());
        }
        let directory = project.context.ensure_data_directory()?;
        let path = directory.join(LOCK_FILE);
        // 不用create_new或锁文件是否存在判断进程状态；旧文件只承载可覆盖诊断。
        // 不共享WRITE和DELETE，另一作者不能同时打开，也不能删除／替换活锁来绕过租约。
        #[cfg(unix)]
        let directory_owner = {
            let owner = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&directory).map_err(|error| format!("无法打开工程租约目录：{error}"))?;
            lock_unix(&owner).map_err(|error| lease_error(project, &path, error))?;
            owner
        };
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        options.share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut owner = options.open(&path)
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
        lock_unix(&owner).map_err(|error| lease_error(project, &path, error))?;
        if !metadata.is_file() || is_reparse(&metadata) {
            return Err("工程写租约必须是作者数据目录中的普通文件".into());
        }
        if project.context.checked_data_directory()? != directory
            || path
                .canonicalize()
                .map_err(|error| format!("无法定位工程写租约：{error}"))?
                != path
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
        owner
            .set_len(0)
            .map_err(|error| format!("无法更新工程写租约诊断：{error}"))?;
        owner
            .write_all(&diagnostic)
            .and_then(|()| owner.sync_all())
            .map_err(|error| format!("无法写入工程写租约诊断：{error}"))?;
        Ok(Self { _owner: owner, #[cfg(unix)] _directory: directory_owner })
    }
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
            std::fs::write(
                root.join("editor-project.json"),
                serde_json::to_vec(&serde_json::json!({
                    "format": 2,
                    "sdk_abi": crate::project::LEGACY_SDK_ABI,
                    "id": "1d4aefae-890c-45d2-9dcb-11ad2a1a3b40",
                    "name": "写租约回归",
                    "cargo_manifest": "Cargo.toml",
                    "package": "writer_fixture",
                    "asset_roots": ["assets"],
                    "editor_data": data
                }))
                .unwrap(),
            )
            .unwrap();
            Self {
                project: crate::project::open(&root).unwrap(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // 只回收夹具精确创建的几个文件和空目录，绝不递归删除作者目录。
            let project = &self.project;
            let _ = std::fs::remove_file(project.context.editor_data.join(LOCK_FILE));
            let _ = std::fs::remove_dir(&project.context.editor_data);
            if let Some(directory) = project.context.editor_data.parent() {
                let _ = std::fs::remove_dir(directory);
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
        let path = fixture.project.context.editor_data.join(LOCK_FILE);
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
    fn stale_file_is_reused_and_a_copied_project_has_its_own_lease() {
        let a = Fixture::with_data("stale", "private-author"); // 尊重清单editor_data，不能写到硬编码.editor。
        let b = Fixture::new("copy"); // 相同UUID但不同真实工程目录不能互相抢占。
        let directory = a.project.context.ensure_data_directory().unwrap();
        std::fs::write(directory.join(LOCK_FILE), b"old process diagnostic").unwrap();
        let _first = NativeProjectLease::acquire(&a.project).unwrap();
        let _other = NativeProjectLease::acquire(&b.project).unwrap();
        assert!(!a.project.root.join(".editor").exists());
        assert_ne!(a.project.context.editor_data, b.project.context.editor_data);
        let diagnostic: serde_json::Value = serde_json::from_slice(
            &std::fs::read(a.project.context.editor_data.join(LOCK_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(diagnostic["process_id"], std::process::id());
        assert_eq!(
            std::fs::read_to_string(b.project.root.join("assets/keep.txt")).unwrap(),
            "作者数据保持原样"
        );
    }
}
