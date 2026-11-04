//! Windows安装记录只读适配；定长缓冲与枚举上限覆盖自定义目录，不调用Shell或修改注册表。
use super::super::types::ExternalEditor;
use std::path::PathBuf;
use windows_sys::Win32::{Foundation::ERROR_SUCCESS, System::Registry::*};

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) { unsafe { RegCloseKey(self.0); } } // 仅释放本模块成功打开的只读句柄。
}
fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }
fn open(parent: HKEY, path: &str, view: u32) -> Option<Key> {
    let mut key = std::ptr::null_mut();
    // UTF-16键名以NUL结尾；输出句柄只在成功后交给RAII，读取权限不允许写入。
    (unsafe { RegOpenKeyExW(parent, wide(path).as_ptr(), 0, KEY_READ | view, &mut key) } == ERROR_SUCCESS).then(|| Key(key))
}
fn string(key: HKEY, child: &[u16], name: &str) -> Option<String> {
    let mut buffer = [0u16; 4096];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    let status = unsafe { RegGetValueW(key, child.as_ptr(), wide(name).as_ptr(), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
        std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut bytes) }; // Win32限定写入固定8KiB缓冲。
    if status != ERROR_SUCCESS { return None; }
    let count = (bytes as usize / 2).min(buffer.len());
    let end = buffer[..count].iter().position(|unit| *unit == 0).unwrap_or(count);
    String::from_utf16(&buffer[..end]).ok()
}

pub(super) fn registered_installations() -> Vec<(ExternalEditor, PathBuf)> {
    let mut result = Vec::new();
    for (hive, view) in [(HKEY_CURRENT_USER, 0), (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY), (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY)] {
        let Some(key) = open(hive, r"Software\Microsoft\Windows\CurrentVersion\Uninstall", view) else { continue; };
        for index in 0..512 {
            let mut child = [0u16; 512];
            let mut length = (child.len() - 1) as u32;
            let status = unsafe { RegEnumKeyExW(key.0, index, child.as_mut_ptr(), &mut length, std::ptr::null_mut(),
                std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }; // 子键名缓冲与枚举次数均固定。
            if status != ERROR_SUCCESS { break; }
            let Some(name) = string(key.0, &child, "DisplayName") else { continue; };
            let lower = name.to_ascii_lowercase();
            let editor = if lower.contains("rustrover") { ExternalEditor::RustRover }
                else if lower.contains("visual studio code") { ExternalEditor::VsCode }
                else if lower == "zed" || lower.starts_with("zed ") { ExternalEditor::Zed } else { continue; };
            if result.iter().filter(|(kind, _)| *kind == editor).count() >= 16 { continue; } // 三类各最多16条路径，不随安装记录规模扩容。
            if let Some(path) = string(key.0, &child, "InstallLocation").filter(|value| !value.trim().is_empty()) {
                let path = PathBuf::from(path.trim().trim_matches('"'));
                if super::bounded(&path) { result.push((editor, path)); }
            }
            if result.iter().filter(|(kind, _)| *kind == editor).count() >= 16 { continue; }
            if let Some(icon) = string(key.0, &child, "DisplayIcon") {
                let icon = icon.trim();
                let executable = if let Some(quoted) = icon.strip_prefix('"') { quoted.split('"').next().unwrap_or("") }
                    else { icon.rsplit_once(',').filter(|(_, suffix)| suffix.trim().parse::<i32>().is_ok()).map_or(icon, |(path, _)| path) };
                let path = PathBuf::from(executable.trim());
                if let Some(parent) = path.parent().filter(|path| super::bounded(path)) { result.push((editor, parent.to_path_buf())); }
            }
        }
    }
    result
}
