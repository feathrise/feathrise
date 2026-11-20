//! 文件任务冻结目录行及选区范围；界面全选只共享快照，后台按需访问真实路径。

use std::{
    ffi::{OsStr, OsString},
    fs::Metadata,
    ops::{Deref, Range},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::SystemTime,
};

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileEntryIdentity {
    created: Option<SystemTime>,
    directory: bool,
    #[cfg(unix)]
    file_id: (u64, u64),
    #[cfg(not(unix))]
    fallback: (Option<SystemTime>, u64),
}

impl FileEntryIdentity {
    pub fn from_metadata(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            created: metadata.created().ok(),
            directory: metadata.is_dir(),
            #[cfg(unix)]
            file_id: (metadata.dev(), metadata.ino()),
            // Windows 的稳定 std 元数据不带文件 ID。用现有属性保守拒绝变更，扫描无需另开文件句柄。
            #[cfg(not(unix))]
            fallback: (metadata.modified().ok(), metadata.len()),
        }
    }
}

/// 真实目录快照缺少某行证据时，该行需要刷新。显式路径来源沿用原有后台预检。
#[derive(Clone, Copy, Debug)]
pub enum FileEntryStamp {
    Untracked,
    Missing,
    Captured(FileEntryIdentity),
}

impl FileEntryStamp {
    pub fn from_metadata(metadata: Option<&Metadata>) -> Self {
        metadata.map_or(Self::Missing, |metadata| Self::Captured(FileEntryIdentity::from_metadata(metadata)))
    }

    pub fn matches(self, actual: &Metadata) -> bool {
        match self {
            Self::Untracked => true,
            Self::Missing => false,
            Self::Captured(expected) => expected == FileEntryIdentity::from_metadata(actual),
        }
    }
}

#[derive(Debug, Default)]
pub struct FileDirectory {
    key: PathBuf,
    entries: Vec<FileEntry>,
    stamps: Vec<FileEntryStamp>,
    // 目录扫描时建索引。少量草稿与拖放目标只查相应的文件名或首级祖先。
    names: Vec<(OsString, usize)>,
}

impl FileDirectory {
    pub fn new(path: &Path, entries: Vec<FileEntry>) -> Self {
        Self::build(path, entries, Vec::new())
    }

    pub fn with_stamps(path: &Path, rows: impl IntoIterator<Item = (FileEntry, FileEntryStamp)>) -> Self {
        let (entries, stamps) = rows.into_iter().unzip();
        Self::build(path, entries, stamps)
    }

    fn build(path: &Path, entries: Vec<FileEntry>, stamps: Vec<FileEntryStamp>) -> Self {
        let names = entries.iter().enumerate().filter_map(|(index, entry)| {
            Some((name_key(entry.path.file_name()?), index))
        });
        let mut names: Vec<_> = names.collect();
        names.sort_unstable();
        Self { key: path_key(path), entries, stamps, names }
    }

    fn matching_name(&self, name: &OsStr) -> &[(OsString, usize)] {
        let start = self.names.partition_point(|(key, _)| key.as_os_str() < name);
        let end = self.names[start..].partition_point(|(key, _)| key.as_os_str() == name);
        &self.names[start..start + end]
    }

    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.matching_name(&name_key(path.file_name()?)).iter().find_map(|&(_, index)| {
            (self.entries[index].path == path).then_some(index)
        })
    }
}

impl Deref for FileDirectory {
    type Target = [FileEntry];
    fn deref(&self) -> &[FileEntry] { &self.entries }
}

impl Drop for FileDirectory {
    fn drop(&mut self) {
        // 最后一个租约可能来自剪贴板或输入消息，释放整份目录时移到后台。
        if self.entries.is_empty() { return; }
        if let Some(pool) = bevy::tasks::AsyncComputeTaskPool::try_get() {
            let entries = std::mem::take(&mut self.entries);
            let names = std::mem::take(&mut self.names);
            let stamps = std::mem::take(&mut self.stamps);
            pool.spawn(async move { drop((entries, names, stamps)); }).detach();
        }
    }
}

#[derive(Clone, Debug)]
struct Rows {
    directory: Arc<FileDirectory>,
    ranges: Vec<Range<usize>>,
    count: usize,
}

impl Rows {
    fn append(&mut self, index: usize) {
        self.count += 1;
        if let Some(last) = self.ranges.last_mut().filter(|last| last.end == index) {
            last.end += 1;
        } else {
            self.ranges.push(index..index + 1);
        }
    }

    fn contains(&self, index: usize) -> bool {
        let position = self.ranges.partition_point(|range| range.end <= index);
        self.ranges.get(position).is_some_and(|range| range.contains(&index))
    }

    fn include(&mut self, mut range: Range<usize>) {
        let start = self.ranges.partition_point(|other| other.end < range.start);
        let end = self.ranges.partition_point(|other| other.start <= range.end);
        if start < end {
            range.start = range.start.min(self.ranges[start].start);
            range.end = range.end.max(self.ranges[end - 1].end);
        }
        self.count += range.len();
        self.count -= self.ranges[start..end].iter().map(Range::len).sum::<usize>();
        self.ranges.splice(start..end, [range]);
    }

    fn exclude(&mut self, index: usize) {
        let position = self.ranges.partition_point(|range| range.end <= index);
        let range = self.ranges[position].clone();
        self.ranges.splice(position..=position, [range.start..index, index + 1..range.end]
            .into_iter().filter(|range| !range.is_empty()));
        self.count -= 1;
    }
}

#[derive(Clone, Debug)]
enum Selection {
    Paths(Arc<[PathBuf]>),
    Rows(Arc<Rows>),
}

/// 固定栏可以提交具体路径，浮窗则冻结目录快照和不相交的行区间。
#[derive(Clone, Debug)]
pub struct FileSources(Selection);

#[derive(Clone, Debug)]
enum SelectionIdentity {
    Paths(Weak<[PathBuf]>),
    Rows(Weak<Rows>),
}

#[derive(Clone, Debug)]
pub(crate) struct FileSourceIdentity(SelectionIdentity);

impl FileSourceIdentity {
    pub(crate) fn matches(&self, sources: &FileSources) -> bool {
        match (&self.0, &sources.0) {
            (SelectionIdentity::Paths(a), Selection::Paths(b)) => a.ptr_eq(&Arc::downgrade(b)),
            (SelectionIdentity::Rows(a), Selection::Rows(b)) => a.ptr_eq(&Arc::downgrade(b)),
            _ => false,
        }
    }
}

impl Default for FileSources {
    fn default() -> Self { Self(Selection::Paths(Arc::default())) }
}

impl From<Vec<PathBuf>> for FileSources {
    fn from(mut paths: Vec<PathBuf>) -> Self {
        paths.sort_unstable();
        paths.dedup();
        Self(Selection::Paths(paths.into()))
    }
}

impl FileSources {
    pub fn len(&self) -> usize {
        match &self.0 {
            Selection::Paths(paths) => paths.len(),
            Selection::Rows(rows) => rows.count,
        }
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    pub fn clear(&mut self) { *self = Self::default(); }

    pub fn first(&self) -> Option<&Path> { self.iter().next() }

    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        self.iter_with_stamp().map(|(path, _)| path)
    }

    pub fn iter_with_stamp(&self) -> impl Iterator<Item = (&Path, FileEntryStamp)> {
        let (paths, rows) = match &self.0 {
            Selection::Paths(paths) => (paths.as_ref(), None),
            Selection::Rows(rows) => (&[][..], Some(rows.as_ref())),
        };
        paths.iter().map(|path| (path.as_path(), FileEntryStamp::Untracked)).chain(rows.into_iter().flat_map(|rows| {
            rows.ranges.iter().cloned().flatten().map(move |index| (
                rows.directory[index].path.as_path(),
                rows.directory.stamps.get(index).copied().unwrap_or(FileEntryStamp::Untracked),
            ))
        }))
    }

    pub fn select(&mut self, directory: &Arc<FileDirectory>, range: Range<usize>, extend: bool) {
        let range = range.start.min(directory.len())..range.end.min(directory.len());
        if !extend || !matches!(&self.0, Selection::Rows(rows) if Arc::ptr_eq(&rows.directory, directory)) {
            let count = range.len();
            let ranges = if range.is_empty() { Vec::new() } else { vec![range] };
            *self = Self(Selection::Rows(Arc::new(Rows { directory: directory.clone(), ranges, count })));
        } else if !range.is_empty() && let Selection::Rows(rows) = &mut self.0 {
            Arc::make_mut(rows).include(range);
        }
    }

    pub fn toggle(&mut self, directory: &Arc<FileDirectory>, index: usize) {
        if let Selection::Rows(rows) = &mut self.0
            && Arc::ptr_eq(directory, &rows.directory) && rows.contains(index)
        {
            Arc::make_mut(rows).exclude(index);
        } else {
            self.select(directory, index..index + 1, true);
        }
    }

    /// 扫描后台按新目录保留原选择；剪贴板也可在后台按真实移动回执剔除完成项。
    pub fn matching(directory: Arc<FileDirectory>, mut keep: impl FnMut(&Path) -> bool) -> Self {
        let mut rows = Rows { directory, ranges: Vec::new(), count: 0 };
        for index in 0..rows.directory.len() {
            if keep(&rows.directory[index].path) { rows.append(index); }
        }
        Self(Selection::Rows(Arc::new(rows)))
    }

    pub(crate) fn retaining(&self, mut keep: impl FnMut(&Path) -> bool) -> Self {
        match &self.0 {
            Selection::Paths(paths) => paths.iter().filter(|path| keep(path)).cloned().collect::<Vec<_>>().into(),
            Selection::Rows(rows) => {
                let mut next = Rows { directory: rows.directory.clone(), ranges: Vec::new(), count: 0 };
                for index in rows.ranges.iter().cloned().flatten() {
                    if keep(&rows.directory[index].path) { next.append(index); }
                }
                if next.count == 0 { Self::default() } else { Self(Selection::Rows(Arc::new(next))) }
            }
        }
    }

    pub(crate) fn identity(&self) -> FileSourceIdentity {
        FileSourceIdentity(match &self.0 {
            Selection::Paths(paths) => SelectionIdentity::Paths(Arc::downgrade(paths)),
            Selection::Rows(rows) => SelectionIdentity::Rows(Arc::downgrade(rows)),
        })
    }

    /// 精确行身份用实际路径区分大小写，保护草稿时另用兼容 Windows 显示路径的比较键。
    pub fn contains(&self, path: &Path) -> bool {
        match &self.0 {
            Selection::Paths(paths) => paths.binary_search_by(|entry| entry.as_path().cmp(path)).is_ok(),
            Selection::Rows(rows) => rows.directory.index_of(path).is_some_and(|index| rows.contains(index)),
        }
    }

    pub fn overlaps(&self, path: &Path) -> bool { self.overlaps_key(&path_key(path), false) }

    pub fn overlaps_tree(&self, path: &Path) -> bool { self.overlaps_key(&path_key(path), true) }

    fn overlaps_key(&self, key: &Path, tree: bool) -> bool {
        if self.is_empty() { return false; }
        match &self.0 {
            Selection::Paths(paths) => paths.iter().any(|path| {
                let source = path_key(path);
                key.starts_with(&source) || tree && source.starts_with(key)
            }),
            Selection::Rows(rows) => {
                let parent = &rows.directory.key;
                if tree && parent.starts_with(key) { return true; }
                let Ok(relative) = key.strip_prefix(parent) else { return false; };
                let mut parts = relative.iter();
                let Some(name) = parts.next() else { return false; };
                let descendant = parts.next().is_some();
                rows.directory.matching_name(name).iter().any(|&(_, index)| {
                    (!descendant || rows.directory[index].directory) && rows.contains(index)
                })
            }
        }
    }
}

fn name_key(name: &OsStr) -> OsString {
    #[cfg(windows)]
    if let Some(name) = name.to_str() { return name.to_lowercase().into(); }
    name.to_os_string()
}

/// 目录与回执使用普通平台路径，保留文件名及后缀原样。
pub fn platform_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            let root = match prefix.kind() {
                Prefix::VerbatimDisk(drive) => Some(PathBuf::from(format!("{}:\\", drive as char))),
                Prefix::VerbatimUNC(server, share) => {
                    let mut root = OsString::from("\\\\");
                    root.push(server);
                    root.push("\\");
                    root.push(share);
                    Some(PathBuf::from(root))
                }
                _ => None,
            };
            if let Some(mut root) = root {
                root.extend(components.filter_map(|part| match part {
                    Component::Normal(name) => Some(name),
                    _ => None,
                }));
                return root;
            }
        }
    }
    path.to_path_buf()
}

/// 同一套词法键用于 UI 草稿和刷新范围。磁盘权限及联接继续由后台受检路径决定。
pub fn path_key(path: &Path) -> PathBuf {
    #[cfg(windows)]
    { PathBuf::from(name_key(platform_path(path).as_os_str())) }
    #[cfg(not(windows))]
    path.to_path_buf()
}
