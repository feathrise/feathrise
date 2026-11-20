//! 文件整理在后台冻结同一份目录清单，正序复核、逆序删除。内存只保留根、固定缓冲和单条路径。

use std::{
    borrow::Cow,
    ffi::OsString,
    fs::{File, Metadata, OpenOptions},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

const BUFFER_BYTES: usize = 16 * 1024;
const RECORD_OVERHEAD: u64 = 4 + 1 + 8 + 16 + 8 + 16 + 4;
const NO_MTIME: i128 = i128::MIN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FileKey {
    pub(super) volume: u64,
    pub(super) id: [u8; 16],
}

pub(super) struct Entry {
    pub(super) relative: PathBuf,
    pub(super) directory: bool,
    pub(super) file_key: Option<FileKey>,
    bytes: u64,
    modified_ns: i128,
}

impl Entry {
    pub(super) fn capture(relative: PathBuf, metadata: &Metadata) -> Self {
        Self {
            relative,
            directory: metadata.is_dir(),
            file_key: metadata_file_key(metadata),
            bytes: metadata.len(),
            modified_ns: modified_ns(metadata),
        }
    }

    pub(super) fn matches(&self, metadata: &Metadata) -> bool {
        let same_type = if self.directory { metadata.is_dir() } else { metadata.is_file() };
        same_type
            && self.bytes == metadata.len()
            && self.modified_ns == modified_ns(metadata)
            && metadata_file_key(metadata).is_none_or(|key| self.file_key == Some(key))
    }
}

#[cfg(unix)]
fn metadata_file_key(metadata: &Metadata) -> Option<FileKey> {
    use std::os::unix::fs::MetadataExt;
    let mut id = [0; 16];
    id[..8].copy_from_slice(&metadata.ino().to_le_bytes());
    Some(FileKey { volume: metadata.dev(), id })
}

#[cfg(not(unix))]
fn metadata_file_key(_metadata: &Metadata) -> Option<FileKey> {
    // Windows 的卷序列号与文件 ID 由调用方通过打开的真实句柄取得。
    None
}

fn modified_ns(metadata: &Metadata) -> i128 {
    metadata.modified().map_or(NO_MTIME, |time| {
        time.duration_since(UNIX_EPOCH).map_or_else(
            |error| -(error.duration().as_nanos() as i128),
            |duration| duration.as_nanos() as i128,
        )
    })
}

pub(super) struct ManifestWriter {
    path: PathBuf,
    first: Option<Entry>,
    output: Option<BufWriter<File>>,
    length: u64,
    max_path: u32,
}

impl ManifestWriter {
    pub(super) fn new(path: PathBuf) -> Self {
        Self { path, first: None, output: None, length: 0, max_path: 0 }
    }

    pub(super) fn push(&mut self, entry: Entry) -> Result<(), String> {
        self.write(entry).map_err(|error| format!("不能写入文件清单：{error}"))
    }

    fn write(&mut self, entry: Entry) -> io::Result<()> {
        check_relative(&entry.relative)?;
        // 单文件和空目录无需打开清单文件，也无需将根重复编码。
        if self.first.is_none() {
            self.first = Some(entry);
            return Ok(());
        }
        let path = encode_path(&entry.relative);
        let size = u32::try_from(path.len()).map_err(|_| invalid_data())?;
        if size == 0 {
            return Err(invalid_data());
        }
        let length = self.length.checked_add(RECORD_OVERHEAD + u64::from(size)).ok_or_else(invalid_data)?;
        if self.output.is_none() {
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(windows)]
            std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, 0);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            self.output = Some(BufWriter::with_capacity(BUFFER_BYTES, options.open(&self.path)?));
        }
        let output = self.output.as_mut().expect("清单写入器已经创建");
        let key = entry.file_key.unwrap_or(FileKey { volume: 0, id: [0; 16] });
        let flags = u8::from(entry.directory) | (u8::from(entry.file_key.is_some()) << 1);
        // 路径的字节数写在记录首尾，反序读取可直接定位上一条记录。
        output.write_all(&size.to_le_bytes())?;
        output.write_all(&[flags])?;
        output.write_all(&entry.bytes.to_le_bytes())?;
        output.write_all(&entry.modified_ns.to_le_bytes())?;
        output.write_all(&key.volume.to_le_bytes())?;
        output.write_all(&key.id)?;
        output.write_all(&path)?;
        output.write_all(&size.to_le_bytes())?;
        self.length = length;
        self.max_path = self.max_path.max(size);
        Ok(())
    }

    pub(super) fn finish(self) -> Result<Manifest, String> {
        let first = self.first.ok_or("文件清单没有根条目")?;
        let file = self.output.map(BufWriter::into_inner).transpose()
            .map_err(|error| format!("不能完成文件清单写入：{error}"))?;
        Ok(Manifest { first, file, length: self.length, max_path: self.max_path })
    }
}

pub(super) struct Manifest {
    first: Entry,
    file: Option<File>,
    length: u64,
    max_path: u32,
}

impl Manifest {
    pub(super) fn visit(&mut self, reverse: bool, mut visit: impl FnMut(&Entry) -> Result<(), String>) -> Result<(), String> {
        let Some(file) = &mut self.file else { return visit(&self.first); };
        if file.metadata().map_err(read_error)?.len() != self.length {
            return Err(read_error(invalid_data()));
        }
        let mut reader = Reader { file, length: self.length, start: 0, available: 0, buffer: [0; BUFFER_BYTES] };
        let (mut position, limit) = if reverse { (self.length, 0) } else { (0, self.length) };
        if !reverse {
            visit(&self.first)?;
        }
        while position != limit {
            let start = if reverse {
                let mut trailer = position.checked_sub(4).ok_or_else(|| read_error(invalid_data()))?;
                let size = u32::from_le_bytes(reader.read(&mut trailer).map_err(read_error)?);
                position.checked_sub(RECORD_OVERHEAD + u64::from(size)).ok_or_else(|| read_error(invalid_data()))?
            } else {
                position
            };
            let mut end = start;
            let entry = reader.entry(&mut end, self.max_path).map_err(read_error)?;
            if reverse && end != position {
                return Err(read_error(invalid_data()));
            }
            visit(&entry)?;
            position = if reverse { start } else { end };
        }
        if reverse {
            visit(&self.first)?;
        }
        Ok(())
    }
}

struct Reader<'a> {
    file: &'a mut File,
    length: u64,
    start: u64,
    available: usize,
    buffer: [u8; BUFFER_BYTES],
}

impl Reader<'_> {
    fn entry(&mut self, position: &mut u64, max_path: u32) -> io::Result<Entry> {
        let size = u32::from_le_bytes(self.read(position)?);
        let [flags] = self.read(position)?;
        if size == 0 || size > max_path || flags & !3 != 0
            || u64::from(size) + RECORD_OVERHEAD - 5 > self.length - *position
        {
            return Err(invalid_data());
        }
        let bytes = u64::from_le_bytes(self.read(position)?);
        let modified_ns = i128::from_le_bytes(self.read(position)?);
        let volume = u64::from_le_bytes(self.read(position)?);
        let id = self.read(position)?;
        let mut path = vec![0; size as usize];
        self.read_into(position, &mut path)?;
        let relative = decode_path(path)?;
        check_relative(&relative)?;
        if u32::from_le_bytes(self.read(position)?) != size {
            return Err(invalid_data());
        }
        Ok(Entry {
            relative, directory: flags & 1 != 0,
            file_key: (flags & 2 != 0).then_some(FileKey { volume, id }),
            bytes, modified_ns,
        })
    }

    fn read<const N: usize>(&mut self, position: &mut u64) -> io::Result<[u8; N]> {
        let mut bytes = [0; N];
        self.read_into(position, &mut bytes)?;
        Ok(bytes)
    }

    fn read_into(&mut self, position: &mut u64, mut output: &mut [u8]) -> io::Result<()> {
        if output.len() as u64 > self.length.saturating_sub(*position) {
            return Err(invalid_data());
        }
        while !output.is_empty() {
            if *position < self.start || *position >= self.start + self.available as u64 {
                // 固定大小的对齐块缓冲兼顾顺读和反读，避免为每条记录各寻道一次。
                self.start = *position / BUFFER_BYTES as u64 * BUFFER_BYTES as u64;
                self.available = (self.length - self.start).min(BUFFER_BYTES as u64) as usize;
                self.file.seek(SeekFrom::Start(self.start))?;
                self.file.read_exact(&mut self.buffer[..self.available])?;
            }
            let offset = (*position - self.start) as usize;
            let count = output.len().min(self.available - offset);
            output[..count].copy_from_slice(&self.buffer[offset..offset + count]);
            output = &mut output[count..];
            *position += count as u64;
        }
        Ok(())
    }
}

fn check_relative(path: &Path) -> io::Result<()> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.contains(&0) || (cfg!(windows) && bytes.contains(&b':'))
        || !path.components().all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(invalid_data());
    }
    Ok(())
}

#[cfg(windows)]
fn encode_path(path: &Path) -> Cow<'_, [u8]> {
    use std::os::windows::ffi::OsStrExt;
    Cow::Owned(path.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect())
}

#[cfg(unix)]
fn encode_path(path: &Path) -> Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(not(any(unix, windows)))]
fn encode_path(path: &Path) -> Cow<'_, [u8]> {
    Cow::Borrowed(path.as_os_str().as_encoded_bytes())
}

#[cfg(windows)]
fn decode_path(path: Vec<u8>) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let (units, trailing) = path.as_chunks::<2>();
    if !trailing.is_empty() {
        return Err(invalid_data());
    }
    let wide: Vec<_> = units.iter().copied().map(u16::from_le_bytes).collect();
    Ok(OsString::from_wide(&wide).into())
}

#[cfg(unix)]
fn decode_path(path: Vec<u8>) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(OsString::from_vec(path).into())
}

#[cfg(not(any(unix, windows)))]
fn decode_path(path: Vec<u8>) -> io::Result<PathBuf> {
    Ok(OsString::from(String::from_utf8(path).map_err(|_| invalid_data())?).into())
}

fn invalid_data() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "条目结构损坏或路径超出清单根")
}

fn read_error(error: io::Error) -> String {
    format!("不能读取文件清单：{error}")
}
