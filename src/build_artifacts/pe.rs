//! 有界读取 Windows PE 导入表，精确定位运行依赖，避免整文件载入或枚举缓存猜版本。

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(super) fn imported_dlls(path: &Path) -> Result<Vec<String>, String> {
    let mut file = File::open(path).map_err(|error| format!("无法读取 PE 依赖：{error}"))?;
    let dos = read_at(&mut file, 0, 64)?;
    if &dos[..2] != b"MZ" {
        return Err(format!("运行产物不是 PE：{}", path.display()));
    }
    let pe_offset = u32_at(&dos, 60) as u64;
    let coff = read_at(&mut file, pe_offset, 24)?;
    if &coff[..4] != b"PE\0\0" {
        return Err("PE 签名无效".into());
    }
    let section_count = u16_at(&coff, 6) as usize;
    let optional_size = u16_at(&coff, 20) as usize;
    if section_count == 0 || section_count > 96 || !(112..=4096).contains(&optional_size) {
        return Err("PE 头超过固定解析范围".into());
    }
    let optional = read_at(&mut file, pe_offset + 24, optional_size)?;
    let directory = match u16_at(&optional, 0) {
        0x20b => 112,
        0x10b => 96,
        _ => return Err("不支持的 PE 可选头".into()),
    };
    if optional.len() < directory + 16 {
        return Err("PE 导入目录缺失".into());
    }
    let import_rva = u32_at(&optional, directory + 8);
    if import_rva == 0 {
        return Ok(Vec::new());
    }
    let sections = read_at(
        &mut file,
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
    let imports = offset(import_rva)?;
    let mut names = Vec::new();
    for index in 0..128 {
        let entry = read_at(&mut file, imports + index * 20, 20)?;
        if entry.iter().all(|byte| *byte == 0) {
            return Ok(names);
        }
        file.seek(SeekFrom::Start(offset(u32_at(&entry, 12))?))
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
    Err("PE 导入目录超过 128 项".into())
}

fn read_at(file: &mut File, offset: u64, length: usize) -> Result<Vec<u8>, String> {
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
