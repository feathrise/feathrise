//! 目录扫描槽内的文件监听与子树缓存，主线程只读取原子刷新标记。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, sync_channel},
    },
};

use notify::{
    EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind},
};

use super::{
    filter::{GlbMetadataCache, ResourceTypes},
    scan::{FolderScope, hidden_path},
};

#[derive(Clone, Default)]
pub(super) struct DirectoryWorker {
    state: Arc<Mutex<DirectoryCache>>,
    dirty: Arc<AtomicBool>,
    refresh: Arc<AtomicBool>,
    watching: Arc<AtomicBool>,
}

impl DirectoryWorker {
    pub(super) fn take_changed(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub(super) fn request_refresh(&self) {
        self.refresh.store(true, Ordering::Release);
    }

    pub(super) fn is_watching(&self) -> bool {
        self.watching.load(Ordering::Acquire)
    }

    /// 该锁只由已占用目录扫描槽的 IO 任务使用。监视器安装、析构和缓存回收也留在同一任务。
    pub(super) fn run<T>(&self, operation: impl FnOnce(&mut DirectoryCache) -> T) -> T {
        let mut cache = self.state.lock().expect("目录缓存仅由原扫描槽持有");
        let result = operation(&mut cache);
        self.watching
            .store(cache.watch.is_some(), Ordering::Release);
        result
    }

    pub(super) fn clear(&self) {
        self.run(|cache| *cache = DirectoryCache::default());
        self.dirty.store(false, Ordering::Release);
    }

    pub(super) fn prepare(&self, cache: &mut DirectoryCache, location: ScanLocation, typed: bool) {
        let moved = cache.location.as_ref() != Some(&location);
        if moved {
            // 同工程切目录可复用文件修订，但子树标记仅在实际监听期间可复用。
            if cache
                .location
                .as_ref()
                .is_none_or(|old| old.root != location.root || old.context != location.context)
            {
                cache.glbs = GlbMetadataCache::default();
            } else {
                cache.invalidate_changes();
            }
            cache.watch = None;
            cache.folders.clear();
            cache.location = Some(location);
        }
        if self.refresh.swap(false, Ordering::AcqRel) {
            // 显式刷新也能处理不支持原生监听的文件系统及时间戳未变的外部替换。
            cache.folders.clear();
            cache.glbs = GlbMetadataCache::default();
        }
        if typed && cache.watch.is_none() {
            cache.watch =
                DirectoryWatch::start(cache.location.as_ref().unwrap(), Arc::clone(&self.dirty));
        }
        cache.invalidate_changes();
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct ScanLocation {
    pub(super) context: u64,
    pub(super) root: PathBuf,
    pub(super) folder: PathBuf,
    pub(super) scope: FolderScope,
    pub(super) asset_root: Option<PathBuf>,
}

#[derive(Default)]
pub(super) struct DirectoryCache {
    location: Option<ScanLocation>,
    watch: Option<DirectoryWatch>,
    pub(super) glbs: GlbMetadataCache,
    folders: HashMap<PathBuf, (bool, ResourceTypes)>,
}

impl DirectoryCache {
    pub(super) fn folder_types(&self, path: &Path, with_glb: bool) -> Option<ResourceTypes> {
        self.folders
            .get(path)
            .filter(|(complete, _)| *complete || !with_glb)
            .map(|(_, types)| *types)
    }

    pub(super) fn remember_folder(&mut self, path: PathBuf, with_glb: bool, types: ResourceTypes) {
        // 无监听时下一次扫描重读目录。缓存只影响成本，任何目录均可参与匹配。
        if self.watch.is_some() && self.folders.len() < 32_768 {
            self.folders.insert(path, (with_glb, types));
        }
    }

    fn invalidate_changes(&mut self) {
        let Some(watch) = &self.watch else {
            return;
        };
        if watch.overflow.swap(false, Ordering::AcqRel) {
            self.folders.clear();
            self.glbs = GlbMetadataCache::default();
        }
        // 收件有固定容量和单次读取上限，快速改写不会把后台目录扫描饿死。
        for change in watch.changes.try_iter().take(128) {
            for parent in change.path.ancestors() {
                self.folders.remove(parent);
            }
            if change.subtree {
                // 同名目录删掉再创建时，旧目录后代的命中位和片段不能被误用。
                self.folders
                    .retain(|path, _| !path.starts_with(&change.path));
            }
            self.glbs.invalidate(&change.path, change.subtree);
        }
    }
}

struct DirectoryWatch {
    _watcher: RecommendedWatcher,
    changes: Receiver<ChangedPath>,
    overflow: Arc<AtomicBool>,
}

struct ChangedPath {
    path: PathBuf,
    subtree: bool,
}

impl DirectoryWatch {
    fn start(location: &ScanLocation, dirty: Arc<AtomicBool>) -> Option<Self> {
        let (send, changes) = sync_channel(128);
        let overflow = Arc::new(AtomicBool::new(false));
        let lost = Arc::clone(&overflow);
        let (root, folder, scope) = (
            location.root.clone(),
            location.folder.clone(),
            location.scope,
        );
        let on_change = move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else {
                lost.store(true, Ordering::Release);
                dirty.store(true, Ordering::Release);
                return;
            };
            if event.need_rescan() || event.paths.is_empty() {
                lost.store(true, Ordering::Release);
                dirty.store(true, Ordering::Release);
                return;
            }
            if matches!(event.kind, EventKind::Access(access) if access != AccessKind::Close(AccessMode::Write)) {
                return; // 解析器自身读取 GLB 不应反过来触发无限刷新。
            }
            let subtree = !matches!(event.kind,
                EventKind::Create(CreateKind::File) | EventKind::Remove(RemoveKind::File)
                    | EventKind::Modify(ModifyKind::Data(_)) | EventKind::Access(_));
            for path in event.paths {
                let Some(path) = event_path(&folder, &path) else {
                    // 此 watcher 只属于当前目录。遇到无法映射的路径时使缓存失效，兼容系统路径别名。
                    lost.store(true, Ordering::Release);
                    dirty.store(true, Ordering::Release);
                    continue;
                };
                if hidden_path(&root, &path, scope) {
                    continue;
                }
                if send.try_send(ChangedPath { path, subtree }).is_err() {
                    lost.store(true, Ordering::Release); // 容量满时重新建立当前子树，保留最终状态。
                }
                dirty.store(true, Ordering::Release);
            }
        };
        let mut watcher = RecommendedWatcher::new(on_change, notify::Config::default().with_follow_symlinks(false)).ok()?;
        watcher
            .watch(&location.folder, RecursiveMode::Recursive)
            .ok()?;
        Some(Self {
            _watcher: watcher,
            changes,
            overflow,
        })
    }
}

fn event_path(folder: &Path, path: &Path) -> Option<PathBuf> {
    if let Ok(relative) = path.strip_prefix(folder) {
        return Some(folder.join(relative));
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut parts = path.components();
        for parent in folder.components() {
            let part = parts.next()?;
            let matches = match (parent, part) {
                (Component::Prefix(left), Component::Prefix(right)) => {
                    match (left.kind(), right.kind()) {
                        (
                            Prefix::VerbatimDisk(left) | Prefix::Disk(left),
                            Prefix::VerbatimDisk(right) | Prefix::Disk(right),
                        ) => left.eq_ignore_ascii_case(&right),
                        (
                            Prefix::VerbatimUNC(left_host, left_share)
                            | Prefix::UNC(left_host, left_share),
                            Prefix::VerbatimUNC(right_host, right_share)
                            | Prefix::UNC(right_host, right_share),
                        ) => {
                            left_host.eq_ignore_ascii_case(right_host)
                                && left_share.eq_ignore_ascii_case(right_share)
                        }
                        _ => left == right,
                    }
                }
                _ => parent.as_os_str().eq_ignore_ascii_case(part.as_os_str()),
            };
            if !matches {
                return None;
            }
        }
        return Some(folder.join(parts.as_path()));
    }
    #[cfg(not(windows))]
    None
}
