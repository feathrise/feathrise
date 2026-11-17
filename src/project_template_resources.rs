//! 读取打包阶段写入的 Windows 资源，每个资源对应空模板中的一个文件。

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    io, slice,
};

pub(crate) const RESOURCE_TYPE: &str = "FEATHRISE_EMPTY_TEMPLATE";
pub(crate) const FILE_VERSION: u8 = 1;
const REQUIRED: [&str; 5] = [
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    "README.md",
    "src/main.rs",
];
const MAX_NAME: usize = 1024;
const MAX_FILES: usize = 512;
const MAX_BYTES: usize = 512 * 1024 * 1024;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn EnumResourceNamesW(
        module: isize,
        kind: *const u16,
        callback: unsafe extern "system" fn(isize, *const u16, *mut u16, isize) -> i32,
        parameter: isize,
    ) -> i32;
    fn FindResourceW(module: isize, name: *const u16, kind: *const u16) -> isize;
    fn SizeofResource(module: isize, resource: isize) -> u32;
    fn LoadResource(module: isize, resource: isize) -> isize;
    fn LockResource(handle: isize) -> *const c_void;
}

fn relative_path(name: &[u16]) -> io::Result<String> {
    let digit = |unit: u16| {
        char::from_u32(u32::from(unit))
            .and_then(|value| value.to_digit(16))
            .map(|value| value as u8)
            .ok_or_else(|| invalid("工程模板资源名称编码无效"))
    };
    let chunks = name.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return Err(invalid("工程模板资源名称编码无效"));
    }
    let bytes = chunks
        .map(|pair| Ok((digit(pair[0])? << 4) | digit(pair[1])?))
        .collect::<io::Result<Vec<_>>>()?;
    String::from_utf8(bytes).map_err(|_| invalid("工程模板文件名不是有效的 UTF-8"))
}

#[derive(Default)]
struct Scan {
    files: BTreeMap<String, Vec<u8>>,
    names: BTreeSet<String>,
    bytes: usize,
    error: Option<io::Error>,
}

fn invalid(description: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, description)
}

impl Scan {
    // Win32 提供的资源名称只在回调期间有效，文件字节在模块卸载前复制完毕。
    unsafe fn add(&mut self, module: isize, kind: *const u16, name: *const u16) -> io::Result<()> {
        if name as usize <= u16::MAX as usize || self.files.len() >= MAX_FILES {
            return Err(invalid("工程模板资源的名称或数量无效"));
        }
        let length = (0..=MAX_NAME)
            .find(|&index| unsafe { *name.add(index) } == 0)
            .ok_or_else(|| invalid("工程模板文件名过长"))?;
        let path = relative_path(unsafe { slice::from_raw_parts(name, length) })?;
        if !(REQUIRED.contains(&path.as_str()) || path.starts_with("src/"))
            || path.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
            || path.chars().any(char::is_control)
            || path
                .split('/')
                .any(|part| matches!(part, "" | "." | "..") || part.ends_with(['.', ' ']))
            || !self.names.insert(path.to_lowercase())
        {
            return Err(invalid("工程模板资源包含不安全或重复的文件路径"));
        }
        let resource = unsafe { FindResourceW(module, name, kind) };
        if resource == 0 {
            return Err(io::Error::last_os_error());
        }
        let size = unsafe { SizeofResource(module, resource) } as usize;
        if size == 0 || size > MAX_BYTES - self.bytes {
            return Err(invalid("工程模板资源大小无效或超过预算"));
        }
        let handle = unsafe { LoadResource(module, resource) };
        if handle == 0 {
            return Err(io::Error::last_os_error());
        }
        let data = unsafe { LockResource(handle) }.cast::<u8>();
        if data.is_null() {
            return Err(invalid("无法读取工程模板资源"));
        }
        let bytes = unsafe { slice::from_raw_parts(data, size) }
            .strip_prefix(&[FILE_VERSION])
            .ok_or_else(|| invalid("工程模板资源版本无效"))?;
        self.bytes += size;
        self.files.insert(path, bytes.to_vec());
        Ok(())
    }
}

unsafe extern "system" fn collect(
    module: isize,
    kind: *const u16,
    name: *mut u16,
    parameter: isize,
) -> i32 {
    // EnumResourceNamesW 同步调用，每次使用同一个仍然有效的收集器。
    let scan = unsafe { &mut *(parameter as *mut Scan) };
    match unsafe { scan.add(module, kind, name) } {
        Ok(()) => 1,
        Err(error) => {
            scan.error = Some(error);
            0
        }
    }
}

/// module 为 0 时读取当前 EXE，打包器也可使用已作为数据映射的发行文件进行校验。
pub(crate) fn read(module: isize) -> io::Result<Option<BTreeMap<String, Vec<u8>>>> {
    let kind: Vec<u16> = RESOURCE_TYPE.encode_utf16().chain(Some(0)).collect();
    let mut scan = Scan::default();
    let result = unsafe {
        EnumResourceNamesW(
            module,
            kind.as_ptr(),
            collect,
            &mut scan as *mut Scan as isize,
        )
    };
    if result == 0 {
        let error = scan.error.unwrap_or_else(io::Error::last_os_error);
        // 源码构建没有该资源类型，其他读取错误应直接呈现。
        return if matches!(error.raw_os_error(), Some(1812 | 1813)) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if !REQUIRED.iter().all(|name| scan.files.contains_key(*name)) {
        return Err(invalid("发行模板缺少必需的工程文件"));
    }
    Ok(Some(scan.files))
}
