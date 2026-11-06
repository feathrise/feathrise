//! 有界读取 Windows PE 导入表，精确定位运行依赖，避免整文件载入或枚举缓存猜版本。

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(super) fn imported_dlls(path: &Path, include_delay: bool) -> Result<Vec<String>, String> {
    let mut file = File::open(path).map_err(|error| format!("无法读取 PE 依赖：{error}"))?;
    read_imports(&mut file, include_delay)
        .map_err(|error| format!("无法解析 {} 的 PE 依赖：{error}", path.display()))
}

fn read_imports(file: &mut (impl Read + Seek), include_delay: bool) -> Result<Vec<String>, String> {
    let dos = read_at(file, 0, 64)?;
    if &dos[..2] != b"MZ" {
        return Err("运行产物不是 PE".into());
    }
    let pe_offset = u32_at(&dos, 60) as u64;
    let coff = read_at(file, pe_offset, 24)?;
    if &coff[..4] != b"PE\0\0" {
        return Err("PE 签名无效".into());
    }
    let section_count = u16_at(&coff, 6) as usize;
    let optional_size = u16_at(&coff, 20) as usize;
    if section_count == 0 || section_count > 96 || !(112..=4096).contains(&optional_size) {
        return Err("PE 头超过固定解析范围".into());
    }
    let optional = read_at(file, pe_offset + 24, optional_size)?;
    let directory = match u16_at(&optional, 0) {
        0x20b => 112,
        0x10b => 96,
        _ => return Err("不支持的 PE 可选头".into()),
    };
    if optional.len() < directory + 16 {
        return Err("PE 导入目录缺失".into());
    }
    let import_rva = u32_at(&optional, directory + 8);
    let mut delay_rva = 0;
    let mut delay_entries = 0;
    // 延迟导入可在启用某项功能时才加载，导出时也必须在固定预算内核对。
    if include_delay && u32_at(&optional, directory - 4) > 13 {
        let start = directory + 13 * 8;
        let entry = optional.get(start..start + 8).ok_or("PE 延迟导入目录超出可选头")?;
        delay_rva = u32_at(entry, 0);
        let size = u32_at(entry, 4);
        if (delay_rva == 0 && size != 0) || (delay_rva != 0 && size < 32) {
            return Err("PE 延迟导入目录不完整".into());
        }
        delay_entries = (size / 32).min(128) as u64;
    }
    if import_rva == 0 && delay_rva == 0 {
        return Ok(Vec::new());
    }
    let sections = read_at(
        file,
        pe_offset + 24 + optional_size as u64,
        section_count * 40,
    )?;
    let offset = |rva: u32| -> Result<u64, String> {
        for section in sections.chunks_exact(40) {
            let start = u32_at(section, 12);
            if rva >= start && rva - start < u32_at(section, 16) {
                return Ok(u32_at(section, 20) as u64 + (rva - start) as u64);
            }
        }
        Err("PE RVA 不在文件节范围内".into())
    };
    let mut names = Vec::new();
    let mut read_table = |rva, entries, delay| -> Result<(), String> {
        let table = offset(rva)?;
        let (stride, name_field) = if delay { (32, 4) } else { (20, 12) };
        for index in 0..entries {
            let entry = read_at(file, table + index * stride as u64, stride)?;
            if entry.iter().all(|byte| *byte == 0) {
                return Ok(());
            }
            if delay && u32_at(&entry, 0) != 1 {
                return Err("PE 延迟导入要求 RVA 格式".into());
            }
            file.seek(SeekFrom::Start(offset(u32_at(&entry, name_field))?))
                .map_err(|error| error.to_string())?;
            let mut bytes = Vec::with_capacity(64);
            for _ in 0..256 {
                let mut byte = [0u8; 1];
                file.read_exact(&mut byte)
                    .map_err(|error| error.to_string())?;
                if byte[0] == 0 {
                    break;
                }
                bytes.push(byte[0]);
            }
            let name = String::from_utf8(bytes).map_err(|_| "PE 导入名不是 ASCII/UTF8")?;
            if name.is_empty()
                || name.len() == 256
                || name.contains(['/', '\\', ':'])
                || !name.to_ascii_lowercase().ends_with(".dll")
            {
                return Err("PE 导入名无效或超过 256 字节".into());
            }
            if !names.contains(&name) {
                names.push(name);
            }
        }
        Err("PE 导入表在固定读取范围内没有结束项".into())
    };
    if import_rva != 0 {
        read_table(import_rva, 128, false)?;
    }
    if delay_rva != 0 {
        read_table(delay_rva, delay_entries, true)?;
    }
    Ok(names)
}

#[cfg(windows)]
pub(super) fn is_vc_runtime(lower: &str) -> bool {
    // ucrtbase.dll 与 msvcrt.dll 由支持的 Windows 提供，版本化 VC DLL 和调试 UCRT 除外。
    lower == "ucrtbased.dll" || ["vcruntime", "msvcp", "msvcr", "concrt", "vccorlib", "vcomp", "vcamp", "mfc", "mfcm"]
        .iter()
        .any(|&prefix| lower.strip_prefix(prefix).is_some_and(|suffix| suffix.starts_with(|c: char| c.is_ascii_digit())))
}

fn read_at(file: &mut (impl Read + Seek), offset: u64, length: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; length];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut bytes))
        .map_err(|error| format!("PE 结构读取失败：{error}"))?;
    Ok(bytes)
}
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn checks_delay_imports_even_without_an_ordinary_import_table() {
        // 只有一个含延迟导入描述符的 PE32+ 节，模拟可选功能稍后才加载 VC DLL。
        let mut bytes = [0u8; 0x300];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        for (offset, value) in [(0x86, 1u16), (0x94, 0xf0), (0x98, 0x20b)] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (0x3c, 0x80u32), (0x98 + 108, 16), (0x98 + 112 + 13 * 8, 0x1000),
            (0x98 + 112 + 13 * 8 + 4, 64), (0x188 + 12, 0x1000), (0x188 + 16, 0x100),
            (0x188 + 20, 0x200), (0x200, 1), (0x204, 0x1050),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let name = b"vcruntime140.dll";
        bytes[0x250..0x250 + name.len()].copy_from_slice(name);
        assert!(read_imports(&mut Cursor::new(&bytes), false).unwrap().is_empty());
        assert_eq!(read_imports(&mut Cursor::new(&bytes), true).unwrap(), ["vcruntime140.dll"]);
        bytes[0x200] = 0;
        assert!(read_imports(&mut Cursor::new(&bytes), true).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn distinguishes_installed_vc_dlls_from_windows_components() {
        for name in ["vcruntime140.dll", "msvcp140_atomic_wait.dll", "msvcr120.dll", "ucrtbased.dll"] {
            assert!(is_vc_runtime(name), "{name}");
        }
        for name in ["msvcrt.dll", "ucrtbase.dll", "msvcp_win.dll", "api-ms-win-crt-runtime-l1-1-0.dll"] {
            assert!(!is_vc_runtime(name), "{name}");
        }
    }
}
