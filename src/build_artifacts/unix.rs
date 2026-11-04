//! ELF/Mach-O运行依赖闭包：按实际导入名和本次明确路径解析，固定预算按需读取，不扫描缓存。
use super::{BYTE_LIMIT, BuildControl, CargoDll, FILE_LIMIT, RuntimeFile};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
const READ_LIMIT: u64 = 8 * 1024 * 1024;
const TABLE_LIMIT: usize = 1024 * 1024;
const NAME_LIMIT: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Linux,
    Mac,
}
#[derive(Debug)]
struct Image {
    needed: Vec<String>,
    rpaths: Vec<String>,
    inherit_rpaths: bool,
    interpreter: Option<String>,
}
struct Pending {
    path: PathBuf,
    inherited: Vec<PathBuf>,
}
struct Reader<'a> {
    file: File,
    len: u64,
    read: u64,
    control: &'a BuildControl,
}
impl<'a> Reader<'a> {
    fn new(path: &Path, control: &'a BuildControl) -> Result<Self, String> {
        control.check()?;
        let file =
            File::open(path).map_err(|e| format!("运行依赖不可读：{}：{e}", path.display()))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > BYTE_LIMIT {
            return Err("运行依赖不是预算内的普通文件".into());
        }
        Ok(Self {
            file,
            len: metadata.len(),
            read: 0,
            control,
        })
    }
    fn at(&mut self, offset: u64, size: usize) -> Result<Vec<u8>, String> {
        self.control.check()?;
        self.read = self
            .read
            .checked_add(size as u64)
            .ok_or("运行依赖读取预算溢出")?;
        if size > TABLE_LIMIT
            || self.read > READ_LIMIT
            || offset
                .checked_add(size as u64)
                .is_none_or(|end| end > self.len)
        {
            return Err("运行依赖结构超出文件或固定读取预算".into());
        }
        let mut bytes = vec![0; size];
        self.file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.file.read_exact(&mut bytes))
            .map_err(|e| format!("运行依赖结构读取失败：{e}"))?;
        Ok(bytes)
    }
    fn string(&mut self, offset: u64, available: u64) -> Result<String, String> {
        let size = available
            .min(self.len.saturating_sub(offset))
            .min(NAME_LIMIT as u64) as usize;
        c_string(&self.at(offset, size)?)
    }
}
fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}
fn c_string(bytes: &[u8]) -> Result<String, String> {
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or("运行依赖名缺少终止或超过4096字节")?;
    if end >= NAME_LIMIT {
        return Err("运行依赖路径超过4096字节".into());
    }
    let value = std::str::from_utf8(&bytes[..end]).map_err(|_| "运行依赖路径不是UTF8")?;
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("运行依赖路径为空或含控制字符".into());
    }
    Ok(value.into())
}
fn architecture(elf: bool) -> u32 {
    match (elf, std::env::consts::ARCH) {
        (true, "x86_64") => object::elf::EM_X86_64.0 as u32,
        (true, "aarch64") => object::elf::EM_AARCH64.0 as u32,
        (false, "x86_64") => object::macho::CPU_TYPE_X86_64.0,
        (false, "aarch64") => object::macho::CPU_TYPE_ARM64.0,
        _ => 0,
    }
}
fn image(path: &Path, platform: Platform, control: &BuildControl) -> Result<Image, String> {
    let mut reader = Reader::new(path, control)?;
    match platform {
        Platform::Linux => elf_image(&mut reader),
        Platform::Mac => macho_image(&mut reader),
    }
}
/// 仅读program header和PT_DYNAMIC，避免完整读取巨大的调试符号；无section header的ELF也可解析。
fn elf_image(reader: &mut Reader<'_>) -> Result<Image, String> {
    let header = reader.at(0, 64)?;
    if &header[..4] != b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || header[6] != 1
        || ![object::elf::ET_EXEC.0, object::elf::ET_DYN.0].contains(&u16_at(&header, 16))
        || u16_at(&header, 18) as u32 != architecture(true)
    {
        return Err("运行版本不是当前CPU的64位小端ELF".into());
    }
    let offset = u64_at(&header, 32);
    let width = u16_at(&header, 54) as usize;
    let count = u16_at(&header, 56) as usize;
    if count > 256 || (count > 0 && !(56..=128).contains(&width)) {
        return Err("ELF程序头超出固定范围".into());
    }
    let programs = reader.at(offset, width * count)?;
    let mut loads = Vec::new();
    let mut dynamic = None;
    let mut interpreter = None;
    for program in programs.chunks_exact(width.max(1)) {
        let kind = u32_at(program, 0);
        let file = u64_at(program, 8);
        let address = u64_at(program, 16);
        let size = u64_at(program, 32);
        if file.checked_add(size).is_none_or(|end| end > reader.len) {
            return Err("ELF程序段越出文件".into());
        }
        if kind == object::elf::PT_LOAD.0 {
            loads.push((file, address, size));
        } else if kind == object::elf::PT_DYNAMIC.0 {
            if dynamic.replace((file, size)).is_some() {
                return Err("ELF具有重复动态表".into());
            }
        } else if kind == object::elf::PT_INTERP.0 {
            if size > NAME_LIMIT as u64 {
                return Err("ELF解释器路径超限".into());
            }
            interpreter = Some(reader.string(file, size)?);
        }
    }
    let Some((offset, size)) = dynamic else {
        return Ok(Image {
            needed: Vec::new(),
            rpaths: Vec::new(),
            inherit_rpaths: false,
            interpreter,
        });
    };
    if size % 16 != 0 || size > 64 * 1024 {
        return Err("ELF动态表超出固定范围".into());
    }
    let bytes = reader.at(offset, size as usize)?;
    let mut strtab = None;
    let mut strsize = None;
    let mut needed = Vec::new();
    let mut rpath = None;
    let mut runpath = None;
    let mut terminated = false;
    for item in bytes.chunks_exact(16) {
        let tag = object::elf::DynamicTag(u64_at(item, 0) as i64);
        let value = u64_at(item, 8);
        if tag == object::elf::DT_NULL {
            terminated = true;
            break;
        }
        match tag {
            object::elf::DT_NEEDED => {
                if needed.len() == FILE_LIMIT {
                    return Err("ELF直接导入超过256项".into());
                }
                needed.push(value);
            }
            object::elf::DT_STRTAB => {
                if strtab.replace(value).is_some() {
                    return Err("ELF字符串表地址重复".into());
                }
            }
            object::elf::DT_STRSZ => {
                if strsize.replace(value).is_some() {
                    return Err("ELF字符串表长度重复".into());
                }
            }
            object::elf::DT_RPATH => rpath = Some(value),
            object::elf::DT_RUNPATH => runpath = Some(value),
            _ => {}
        }
    }
    if !terminated {
        return Err("ELF动态表缺少终止".into());
    }
    if needed.is_empty() && rpath.is_none() && runpath.is_none() {
        return Ok(Image {
            needed: Vec::new(),
            rpaths: Vec::new(),
            inherit_rpaths: false,
            interpreter,
        });
    }
    let address = strtab.ok_or("ELF动态字符串表缺失")?;
    let size = strsize.ok_or("ELF动态字符串表长度缺失")?;
    let start = loads
        .iter()
        .find_map(|(file, base, length)| {
            address
                .checked_sub(*base)
                .filter(|delta| delta.checked_add(size).is_some_and(|end| end <= *length))
                .map(|delta| file + delta)
        })
        .ok_or("ELF动态字符串表不在文件加载段内")?;
    let mut string = |offset: u64| -> Result<String, String> {
        if offset >= size {
            return Err("ELF导入字符串偏移越界".into());
        }
        reader.string(start + offset, size - offset)
    };
    let needed = needed
        .into_iter()
        .map(&mut string)
        .collect::<Result<Vec<_>, _>>()?;
    let paths = runpath.or(rpath).map(&mut string).transpose()?;
    let rpaths = paths
        .map(|value| value.split(':').map(str::to_string).collect())
        .unwrap_or_default();
    Ok(Image {
        needed,
        rpaths,
        inherit_rpaths: runpath.is_none(),
        interpreter,
    })
}
/// Mach-O只读取有界load commands；弱引用、再导出和upward库同样纳入闭包。
fn macho_image(reader: &mut Reader<'_>) -> Result<Image, String> {
    let header = reader.at(0, 32)?;
    if u32_at(&header, 0) != object::macho::MH_MAGIC_64
        || u32_at(&header, 4) != architecture(false)
        || ![object::macho::MH_EXECUTE.0, object::macho::MH_DYLIB.0].contains(&u32_at(&header, 12))
    {
        return Err("运行版本不是当前CPU的64位小端Mach-O".into());
    }
    let count = u32_at(&header, 16) as usize;
    let size = u32_at(&header, 20) as usize;
    if count > 1024 || size > TABLE_LIMIT {
        return Err("Mach-O加载命令超出固定范围".into());
    }
    let commands = reader.at(32, size)?;
    let mut at = 0usize;
    let mut needed = Vec::new();
    let mut rpaths = Vec::new();
    let mut interpreter = None;
    for _ in 0..count {
        if at.checked_add(8).is_none_or(|end| end > commands.len()) {
            return Err("Mach-O加载命令截断".into());
        }
        let command = object::macho::LoadCommandType(u32_at(&commands, at));
        let length = u32_at(&commands, at + 4) as usize;
        if length < 8
            || length % 4 != 0
            || at
                .checked_add(length)
                .is_none_or(|end| end > commands.len())
        {
            return Err("Mach-O加载命令长度无效".into());
        }
        let bytes = &commands[at..at + length];
        if [
            object::macho::LC_LOAD_DYLIB,
            object::macho::LC_LOAD_WEAK_DYLIB,
            object::macho::LC_REEXPORT_DYLIB,
            object::macho::LC_LOAD_UPWARD_DYLIB,
            object::macho::LC_LAZY_LOAD_DYLIB,
        ]
        .contains(&command)
        {
            if length < 24 || needed.len() == FILE_LIMIT {
                return Err("Mach-O库命令无效或超过256项".into());
            }
            let name = u32_at(bytes, 8) as usize;
            if name < 24 || name >= length {
                return Err("Mach-O库名偏移无效".into());
            }
            needed.push(c_string(&bytes[name..])?);
        } else if command == object::macho::LC_RPATH || command == object::macho::LC_LOAD_DYLINKER {
            if length < 12 {
                return Err("Mach-O路径命令截断".into());
            }
            let name = u32_at(bytes, 8) as usize;
            if name < 12 || name >= length {
                return Err("Mach-O路径偏移无效".into());
            }
            let path = c_string(&bytes[name..])?;
            if command == object::macho::LC_RPATH {
                if rpaths.len() == 64 {
                    return Err("Mach-O运行路径超过64项".into());
                }
                rpaths.push(path);
            } else {
                interpreter = Some(path);
            }
        }
        at += length;
    }
    if at != commands.len() {
        return Err("Mach-O命令总长度不一致".into());
    }
    Ok(Image {
        needed,
        rpaths,
        inherit_rpaths: true,
        interpreter,
    })
}

pub(super) fn runtime_files(
    executable: &Path,
    target: &Path,
    dlls: &[CargoDll],
    search_paths: &[PathBuf],
    control: &BuildControl,
) -> Result<Vec<RuntimeFile>, String> {
    let platform = match std::env::consts::OS {
        "linux" => Platform::Linux,
        "macos" => Platform::Mac,
        _ => return Err("Unix运行依赖收集只适用于Linux/macOS".into()),
    };
    collect(executable, target, dlls, search_paths, control, platform)
}
fn collect(
    executable: &Path,
    target: &Path,
    dlls: &[CargoDll],
    search_paths: &[PathBuf],
    control: &BuildControl,
    platform: Platform,
) -> Result<Vec<RuntimeFile>, String> {
    control.check()?;
    if dlls.len() > FILE_LIMIT || search_paths.len() > FILE_LIMIT {
        return Err("运行依赖输入超过256项".into());
    }
    let target = target.canonicalize().map_err(|e| e.to_string())?;
    let executable = executable.canonicalize().map_err(|e| e.to_string())?;
    if !executable.starts_with(&target) {
        return Err("运行入口逃出本次构建缓存".into());
    }
    let executable_directory = executable.parent().ok_or("运行入口缺少父目录")?;
    let mut candidates = BTreeMap::new();
    for record in dlls {
        control.check()?;
        let source = record
            .path
            .canonicalize()
            .map_err(|e| format!("Cargo运行库不存在：{e}"))?;
        if !source.starts_with(&target) || !source.is_file() {
            return Err("Cargo运行库逃出本次构建缓存".into());
        }
        let name = filename(&source)?.to_string();
        if let Some(previous) = candidates.insert(name, source.clone())
            && previous != source
        {
            return Err("本次Cargo存在同名不同来源运行库".into());
        }
    }
    let mut pending = vec![Pending {
        path: executable.clone(),
        inherited: Vec::new(),
    }];
    let mut visited = BTreeSet::new();
    let mut files = BTreeMap::<String, RuntimeFile>::new();
    while let Some(item) = pending.pop() {
        control.check()?;
        if !visited.insert(item.path.clone()) {
            continue;
        }
        if visited.len() > FILE_LIMIT {
            return Err("运行依赖闭包超过256项".into());
        }
        let info = image(&item.path, platform, control)?;
        if let Some(interpreter) = info.interpreter {
            validate_interpreter(&interpreter, platform)?;
        }
        let mut own_paths = Vec::new();
        for path in info.rpaths {
            own_paths.push(expand(&path, &item.path, executable_directory)?);
        }
        if own_paths.len() + item.inherited.len() > 128 {
            return Err("继承运行路径超过128项".into());
        }
        let mut direct = own_paths.clone();
        direct.extend(item.inherited.iter().cloned());
        direct.extend(search_paths.iter().cloned());
        let mut inherited = item.inherited;
        if info.inherit_rpaths {
            inherited.extend(own_paths);
        }
        for name in info.needed {
            control.check()?;
            let leaf = filename(Path::new(&name))?.to_string();
            if name.contains(".framework/") && !system_name(&name, platform) {
                return Err(format!(
                    "非系统framework需显式可重定位交付，当前不能冻结该依赖：{name}"
                ));
            }
            let source = resolve(
                &name,
                &leaf,
                &item.path,
                executable_directory,
                &target,
                dlls,
                &candidates,
                &direct,
                platform,
                control,
            )?;
            let Some(source) = source else {
                continue;
            };
            if let Some(previous) = files.get(&leaf) {
                if previous.source != source {
                    return Err(format!("运行库导入存在同名冲突：{name}"));
                }
                continue;
            }
            if files.len() == FILE_LIMIT {
                return Err("运行依赖超过256文件".into());
            }
            files.insert(
                leaf.clone(),
                RuntimeFile {
                    source: source.clone(),
                    relative: PathBuf::from(leaf),
                },
            );
            pending.push(Pending {
                path: source,
                inherited: inherited.clone(),
            });
        }
    }
    Ok(files.into_values().collect())
}
fn resolve(
    name: &str,
    leaf: &str,
    importer: &Path,
    exe_dir: &Path,
    target: &Path,
    dlls: &[CargoDll],
    candidates: &BTreeMap<String, PathBuf>,
    paths: &[PathBuf],
    platform: Platform,
    control: &BuildControl,
) -> Result<Option<PathBuf>, String> {
    if system_name(name, platform) {
        if platform == Platform::Mac || Path::new(name).is_file() {
            return Ok(None);
        }
        return Err(format!("系统运行库不存在：{name}"));
    }
    if let Some(source) = candidates.get(leaf) {
        return Ok(Some(source.clone()));
    }
    let specific = if let Some(relative) = name.strip_prefix("@rpath/") {
        Some((relative, false))
    } else if name.starts_with('@') || name.starts_with('$') || Path::new(name).is_absolute() {
        Some((name, true))
    } else {
        None
    };
    if let Some((specific, true)) = specific {
        let source = expand(specific, importer, exe_dir)?;
        if source.is_file() {
            return source.canonicalize().map(Some).map_err(|e| e.to_string());
        }
        return Err(format!("明确运行库路径不存在：{name}"));
    }
    if name.contains('/') && specific.is_none() {
        return Err(format!(
            "运行库使用未锚定的相对路径，拒绝猜测工作目录：{name}"
        ));
    }
    let relative = specific.map(|v| v.0).unwrap_or(name);
    if Path::new(relative)
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("运行库相对路径越界".into());
    }
    for directory in paths {
        control.check()?;
        if !directory.is_absolute() {
            return Err("运行库搜索根必须是本次明确的绝对目录".into());
        }
        let file = directory.join(relative);
        if file.is_file() {
            let file = file.canonicalize().map_err(|e| e.to_string())?;
            verify_bevy_alias(&file, leaf, target, dlls, control)?;
            return Ok(Some(file));
        }
    }
    for root in system_roots(platform) {
        if root.join(relative).is_file() {
            return Ok(None);
        }
    }
    Err(format!("本次构建缺少实际导入的运行库：{name}"))
}
fn verify_bevy_alias(
    source: &Path,
    name: &str,
    target: &Path,
    dlls: &[CargoDll],
    control: &BuildControl,
) -> Result<(), String> {
    let tail = name
        .strip_prefix("libbevy_dylib-")
        .and_then(|v| v.strip_suffix(".so").or_else(|| v.strip_suffix(".dylib")));
    let Some(fingerprint) = tail else {
        return Ok(());
    };
    if fingerprint.len() != 16 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Bevy动态库hash导入名无效".into());
    }
    let mut matching = dlls.iter().filter(|record| {
        record.dylib
            && record.target_name == "bevy_dylib"
            && record
                .package_id
                .rsplit_once('#')
                .is_some_and(|(_, value)| value.starts_with("bevy_dylib@"))
    });
    let record = matching
        .next()
        .ok_or("本次Cargo没有Bevy动态库精确来源回执")?;
    if matching.next().is_some()
        || !source.starts_with(target)
        || super::hash_file(source, control)?.0 != super::hash_file(&record.path, control)?.0
    {
        return Err("Bevy精确导入与本次Cargo副本身份不同".into());
    }
    Ok(())
}
fn expand(value: &str, importer: &Path, exe_dir: &Path) -> Result<PathBuf, String> {
    let loader = importer.parent().ok_or("运行库缺少父目录")?;
    for (prefix, base) in [("@loader_path", loader), ("@executable_path", exe_dir), ("$ORIGIN", loader), (concat!("$", "{ORIGIN}"), loader)] {
        if let Some(tail) = value.strip_prefix(prefix) {
            if !tail.is_empty() && !tail.starts_with('/') { break; }
            if tail.contains(['$', '@', '\\']) { return Err(format!("运行库路径含未支持的加载器变量：{value}")); }
            // 逐个加入Unix组件；字符串拼接会把Windows验证夹具的verbatim路径中斜杠保留成文件名。
            let mut path = base.to_path_buf();
            for component in tail.trim_start_matches('/').split('/') {
                match component {
                    "" | "." => {},
                    ".." => { if !path.pop() { return Err("运行库相对路径越出绝对根".into()); } },
                    component => { if component.contains(':') { return Err("运行库相对组件无效".into()); } path.push(component); },
                }
            }
            return Ok(path);
        }
    }
    if value.contains('$') || value.contains('@') || value.is_empty() {
        return Err(format!("运行库路径含未支持的加载器变量：{value}"));
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("运行库搜索路径没有绝对锚点：{value}"));
    }
    Ok(path)
}
fn system_name(name: &str, platform: Platform) -> bool {
    // 静态解析也在Windows夹具中执行；安装名按Unix语法判断，不套宿主盘符规则。
    if !name.starts_with('/') {
        return false;
    }
    let under = |root: &str| {
        name == root
            || name
                .strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    match platform {
        Platform::Mac => under("/usr/lib") || under("/System/Library"),
        Platform::Linux => system_roots(platform)
            .iter()
            .any(|root| under(&root.to_string_lossy())),
    }
}
fn system_roots(platform: Platform) -> Vec<PathBuf> {
    if platform == Platform::Mac {
        return vec!["/usr/lib".into(), "/System/Library/Frameworks".into()];
    }
    let triple = match std::env::consts::ARCH {
        "aarch64" => "aarch64-linux-gnu",
        _ => "x86_64-linux-gnu",
    };
    vec![
        PathBuf::from(format!("/lib/{triple}")),
        PathBuf::from(format!("/usr/lib/{triple}")),
        "/lib64".into(),
        "/usr/lib64".into(),
        "/lib".into(),
        "/usr/lib".into(),
    ]
}
fn validate_interpreter(path: &str, platform: Platform) -> Result<(), String> {
    let path = Path::new(path);
    let valid = match platform {
        Platform::Mac => path == Path::new("/usr/lib/dyld"),
        Platform::Linux => {
            path.is_absolute()
                && path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .is_some_and(|v| v.starts_with("ld-linux") || v.starts_with("ld-musl"))
                && path.is_file()
        }
    };
    if !valid {
        return Err("运行版本的系统动态解释器缺失或未识别".into());
    }
    Ok(())
}
fn filename(path: &Path) -> Result<&str, String> {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("运行库文件名不是有效UTF8")?;
    if name.is_empty()
        || name.len() > 255
        || name.chars().any(char::is_control)
        || name.contains(['\\', ':'])
    {
        return Err("运行库文件名无效".into());
    }
    Ok(name)
}

#[cfg(test)]
#[path = "unix_tests.rs"]
mod tests;
