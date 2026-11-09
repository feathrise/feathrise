//! 三槽共享内存帧环；只有后台worker复制像素，CAS所有权保证读者永远拿到一份完整帧。

use super::types::{CapturedFrame, Frame, Geometry, MAX_PIXELS};
use sha2::{Digest, Sha256};
use std::{
    cell::UnsafeCell,
    fs::{File, OpenOptions},
    path::Path,
    ptr::NonNull,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};

const SLOTS: usize = 3;
const PREFIX: usize = 64;
const SLOT_HEADER: usize = 64;
const SLOT_STRIDE: usize = (SLOT_HEADER + MAX_PIXELS).next_multiple_of(64);
const MAP_BYTES: usize = PREFIX + SLOT_STRIDE * SLOTS;
const MAGIC: &[u8; 8] = b"FTHFRM01";
const FREE: u32 = 0;
const WRITING: u32 = 1;
const READY: u32 = 2;
const READING: u32 = 3;

#[repr(C, align(64))]
struct Header {
    state: AtomicU32,
    sequence: AtomicU64,
    metadata: UnsafeCell<FrameMetadata>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrameMetadata {
    epoch: u64,
    width: u32,
    height: u32,
    scale: f32,
}

pub(super) struct FrameRing {
    mapping: Mapping,
}

// SAFETY：映射只通过每槽CAS所有权访问，元数据/像素不会被两个worker同时读写。
unsafe impl Send for FrameRing {}
unsafe impl Sync for FrameRing {}

impl FrameRing {
    pub fn create(path: &Path, token: &str) -> Result<Self, String> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|error| format!("无法创建作者共享帧：{error}"))?;
        file.set_len(MAP_BYTES as u64)
            .map_err(|error| format!("无法预留作者共享帧：{error}"))?;
        let mapping = Mapping::new(file)?;
        let digest = Sha256::digest(token.as_bytes());
        // 新建文件没有读者，先写协议身份；所有槽的初始零状态是FREE，不触摸整幅像素页。
        unsafe {
            std::ptr::copy_nonoverlapping(MAGIC.as_ptr(), mapping.pointer.as_ptr(), MAGIC.len());
            std::ptr::copy_nonoverlapping(
                digest.as_ptr(),
                mapping.pointer.as_ptr().add(8),
                digest.len(),
            );
            for index in 0..SLOTS {
                std::ptr::write(
                    mapping
                        .pointer
                        .as_ptr()
                        .add(PREFIX + SLOT_STRIDE * index)
                        .cast::<Header>(),
                    Header {
                        state: AtomicU32::new(FREE),
                        sequence: AtomicU64::new(0),
                        metadata: UnsafeCell::new(FrameMetadata {
                            epoch: 0,
                            width: 0,
                            height: 0,
                            scale: 1.,
                        }),
                    },
                );
            }
        }
        Ok(Self { mapping })
    }

    pub fn open(path: &Path, token: &str) -> Result<Self, String> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|error| format!("无法打开作者共享帧：{error}"))?;
        if !file
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
            || file.metadata().map_err(|error| error.to_string())?.len() != MAP_BYTES as u64
        {
            return Err("作者共享帧文件类型或预算不匹配".into());
        }
        let mapping = Mapping::new(file)?;
        let identity = unsafe { std::slice::from_raw_parts(mapping.pointer.as_ptr(), 40) };
        if &identity[..8] != MAGIC || identity[8..40] != Sha256::digest(token.as_bytes())[..] {
            return Err("作者共享帧会话身份不匹配".into());
        }
        Ok(Self { mapping })
    }

    fn header(&self, index: usize) -> &Header {
        unsafe {
            &*self
                .mapping
                .pointer
                .as_ptr()
                .add(PREFIX + SLOT_STRIDE * index)
                .cast::<Header>()
        }
    }

    /// 仅后台写worker调用；没有空槽时覆盖尚未被读者采用的旧完整帧，不排队无限截图。
    pub fn publish(&self, frame: CapturedFrame) -> Result<(), String> {
        self.publish_mapped(frame.geometry, frame.sequence, &frame.padded)
    }

    pub fn publish_mapped(
        &self,
        geometry: Geometry,
        sequence: u64,
        padded: &[u8],
    ) -> Result<(), String> {
        let geometry = geometry.checked()?;
        let stride = geometry.width as usize * 4;
        let padded_stride = stride.next_multiple_of(256);
        if padded.len() != padded_stride * geometry.height as usize {
            return Err("GPU作者帧不是当前尺寸的完整RGBA回读".into());
        }
        let slot = (0..SLOTS)
            .find(|&index| {
                self.header(index)
                    .state
                    .compare_exchange(FREE, WRITING, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
            })
            .or_else(|| {
                (0..SLOTS).find(|&index| {
                    self.header(index)
                        .state
                        .compare_exchange(READY, WRITING, Ordering::Acquire, Ordering::Relaxed)
                        .is_ok()
                })
            });
        let Some(slot) = slot else {
            return Ok(());
        };
        let base = unsafe {
            self.mapping
                .pointer
                .as_ptr()
                .add(PREFIX + SLOT_STRIDE * slot)
        };
        // SAFETY：WRITING是当前worker的独占所有权；完整元数据与全部行写完后才Release发布。
        unsafe {
            let header = base.cast::<Header>();
            *(*header).metadata.get() = FrameMetadata {
                epoch: geometry.epoch,
                width: geometry.width,
                height: geometry.height,
                scale: geometry.scale,
            };
            for (row, bytes) in padded.chunks_exact(padded_stride).enumerate() {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    base.add(SLOT_HEADER + row * stride),
                    stride,
                );
            }
            (*header).sequence.store(sequence, Ordering::Relaxed);
            (*header).state.store(READY, Ordering::Release);
        }
        Ok(())
    }

    /// 仅后台读worker调用；先争取最新槽，再一次性复制，主世界只接收已经完整的Vec。
    pub fn latest(&self, after: u64) -> Result<Option<Frame>, String> {
        let Some(index) = (0..SLOTS)
            .filter(|&index| self.header(index).state.load(Ordering::Acquire) == READY)
            .filter(|&index| self.header(index).sequence.load(Ordering::Relaxed) > after)
            .max_by_key(|&index| self.header(index).sequence.load(Ordering::Relaxed))
        else {
            return Ok(None);
        };
        let header = self.header(index);
        if header
            .state
            .compare_exchange(READY, READING, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok(None);
        }
        let result = (|| {
            let metadata = unsafe { *header.metadata.get() };
            let geometry = Geometry {
                epoch: metadata.epoch,
                width: metadata.width,
                height: metadata.height,
                scale: metadata.scale,
            }
            .checked()?;
            let sequence = header.sequence.load(Ordering::Relaxed);
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    self.mapping
                        .pointer
                        .as_ptr()
                        .add(PREFIX + SLOT_STRIDE * index + SLOT_HEADER),
                    geometry.pixels(),
                )
            };
            Ok(Some(Frame {
                geometry,
                sequence,
                pixels: bytes.to_vec(),
            }))
        })();
        header.state.store(FREE, Ordering::Release);
        result
    }
}

struct Mapping {
    pointer: NonNull<u8>,
    _file: File,
    #[cfg(windows)]
    // 映射对象只负责保活；Drop 先撤销视图，再由字段析构关闭此句柄，不能提前丢弃。
    _object: std::os::windows::io::OwnedHandle,
}

impl Mapping {
    fn new(file: File) -> Result<Self, String> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let raw = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    MAP_BYTES,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    file.as_raw_fd(),
                    0,
                )
            };
            if raw == libc::MAP_FAILED {
                return Err(format!(
                    "无法映射作者共享帧：{}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self {
                pointer: NonNull::new(raw.cast()).ok_or("共享帧映射为空")?,
                _file: file,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
            use windows_sys::Win32::System::Memory::{
                CreateFileMappingW, FILE_MAP_ALL_ACCESS, MapViewOfFile, PAGE_READWRITE,
            };
            let raw = unsafe {
                CreateFileMappingW(
                    file.as_raw_handle(),
                    std::ptr::null(),
                    PAGE_READWRITE,
                    0,
                    MAP_BYTES as u32,
                    std::ptr::null(),
                )
            };
            if raw.is_null() {
                return Err(format!(
                    "无法映射作者共享帧：{}",
                    std::io::Error::last_os_error()
                ));
            }
            let object = unsafe { OwnedHandle::from_raw_handle(raw) };
            let view = unsafe {
                MapViewOfFile(object.as_raw_handle(), FILE_MAP_ALL_ACCESS, 0, 0, MAP_BYTES)
            };
            Ok(Self {
                pointer: NonNull::new(view.Value.cast()).ok_or("共享帧视图为空")?,
                _file: file,
                _object: object,
            })
        }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::munmap(self.pointer.as_ptr().cast(), MAP_BYTES);
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::Memory::UnmapViewOfFile(
                windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.pointer.as_ptr().cast(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_ring_rejects_wrong_session_and_adopts_only_complete_latest_frame() {
        let path = std::env::temp_dir().join(format!(
            "feathrise-frame-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let writer = FrameRing::create(&path, "owned-fixture").unwrap();
        assert!(FrameRing::open(&path, "another-session").is_err());
        let reader = FrameRing::open(&path, "owned-fixture").unwrap();
        let geometry = Geometry {
            epoch: 7,
            width: 3,
            height: 2,
            scale: 1.5,
        };
        for sequence in 1..=5 {
            writer
                .publish(CapturedFrame {
                    geometry,
                    sequence,
                    padded: vec![sequence as u8; 512],
                })
                .unwrap();
        }
        let latest = reader.latest(0).unwrap().unwrap();
        assert_eq!(latest.sequence, 5);
        assert_eq!(latest.geometry, geometry);
        assert_eq!(latest.pixels, vec![5; 24]);
        assert!(reader.latest(5).unwrap().is_none());
        assert!(
            writer
                .publish(CapturedFrame {
                    geometry,
                    sequence: 6,
                    padded: vec![0; 24]
                })
                .is_err()
        );
        drop(reader);
        drop(writer);
        std::fs::remove_file(path).unwrap();
    }
}
