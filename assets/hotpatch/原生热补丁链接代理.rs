//! 受控 Windows/COFF 链接代理：修复 dx 0.7.9 的 __chkstk 桩和实际基线 DLL 导入。
//! 用法：由原P0验证脚本或正式函数热更新后台编译；普通rustc链接完整透传。
//! 依赖：当前 Rust/MSVC 工具链及标准库，真实 LLD 路径由 runner 临时传入。
//! 导入清单由 runner 根据已加载 DLL 生成并锁定；不修改官方 dx 或原始 stub.o。
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

fn u16_at(b: &[u8], p: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        b.get(p..p + 2).ok_or("COFF u16 越界")?.try_into().unwrap(),
    ))
}
fn u32_at(b: &[u8], p: usize) -> Result<usize, String> {
    Ok(u32::from_le_bytes(b.get(p..p + 4).ok_or("COFF u32 越界")?.try_into().unwrap()) as usize)
}

/// 只接受实测的 AMD64 普通 COFF；返回新副本及修复字节偏移。
fn fix_stack_probe(b: &[u8]) -> Result<Option<(Vec<u8>, usize)>, String> {
    if u16_at(b, 0)? != 0x8664 || u16_at(b, 16)? != 0 {
        return Err("仅支持 dx AMD64 普通 COFF stub".into());
    }
    let sections = u16_at(b, 2)? as usize;
    let symbols = u32_at(b, 8)?;
    let count = u32_at(b, 12)?;
    let strings = symbols
        .checked_add(count.checked_mul(18).ok_or("符号数量溢出")?)
        .ok_or("符号偏移溢出")?;
    let string_size = u32_at(b, strings)?;
    let names = b
        .get(strings..strings.checked_add(string_size).ok_or("字符串表溢出")?)
        .ok_or("字符串表越界")?;
    let mut entries = Vec::new();
    let mut index = 0;
    while index < count {
        let p = symbols + index * 18;
        let raw = b.get(p..p + 18).ok_or("符号表越界")?;
        let name = if raw[..4] == [0; 4] {
            let start = u32_at(raw, 4)?;
            let tail = names.get(start..).ok_or("符号名称越界")?;
            &tail[..tail
                .iter()
                .position(|&v| v == 0)
                .ok_or("符号名称缺少终止符")?]
        } else {
            &raw[..raw[..8].iter().position(|&v| v == 0).unwrap_or(8)]
        };
        entries.push((name, u32_at(raw, 8)?, u16_at(raw, 12)? as usize));
        index += 1 + raw[17] as usize;
    }
    let targets: Vec<_> = entries.iter().filter(|e| e.0 == b"__chkstk").collect();
    if targets.is_empty() {
        return Ok(None);
    }
    if targets.len() != 1 {
        return Err("__chkstk 符号不唯一".into());
    }
    let (_, value, section) = *targets[0];
    if section == 0 || section > sections {
        return Err("__chkstk 缺少有效代码节".into());
    }
    let header = 20 + (section - 1) * 40;
    if b.get(header..header + 8) != Some(b".text\0\0\0") || u16_at(b, header + 32)? != 0 {
        return Err("代码节名称或重定位布局与已验证 dx stub 不符".into());
    }
    let size = u32_at(b, header + 16)?;
    let start = u32_at(b, header + 20)?;
    if value + 16 > size
        || entries
            .iter()
            .any(|e| e.2 == section && e.1 > value && e.1 < value + 16)
    {
        return Err("__chkstk 后没有完整 16 字节独立槽".into());
    }
    let offset = start + value;
    let code = b.get(offset..offset + 16).ok_or("代码槽越界")?;
    if code[..2] != [0x48, 0xb8] || code[10..12] != [0xff, 0xe0] || code[12..] != [0; 4] {
        return Err("__chkstk 机器码不是 dx 0.7.9 的 mov rax/ jmp rax 桩".into());
    }
    let mut fixed = b.to_vec();
    // __chkstk 用 RAX 接收栈大小，不能把地址写进 RAX；RIP 相对间接跳转不改任何寄存器。
    fixed[offset..offset + 6].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
    fixed[offset + 6..offset + 14].copy_from_slice(&code[2..10]);
    fixed[offset + 14..offset + 16].fill(0x90);
    Ok(Some((fixed, offset)))
}

fn fix_object(path: &Path, root: &Path) -> Result<Option<PathBuf>, String> {
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !path.starts_with(root) {
        return Err("拒绝修改实验 target 之外的 stub".into());
    }
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    if let Some((fixed, offset)) = fix_stack_probe(&bytes)? {
        let replacement = path.with_file_name("stub.p0-chkstk.o");
        fs::write(&replacement, fixed).map_err(|e| e.to_string())?;
        eprintln!(
            "P0 __chkstk 修复：offset={offset}, original={path:?}, replacement={replacement:?}"
        );
        return Ok(Some(replacement));
    }
    Ok(None)
}

fn response_stubs(text: &str) -> Vec<&str> {
    // dx 0.7.9 在 Windows 将每个参数完整加双引号写入 UTF-8 响应文件。
    // 仅替换独立路径 token，原文中的空格、中文、其他参数与引号保持不变。
    text.split('"')
        .skip(1)
        .step_by(2)
        .filter(|token| {
            Path::new(token)
                .file_name()
                .is_some_and(|name| name == "stub.o")
        })
        .collect()
}

/// 只接受 runner 在本次 results 中登记的导入库，不枚举目录、不选择其他 hash 的 DLL。
fn read_imports(manifest: &Path, root: &Path, sysroot_lib: &Path) -> Result<Vec<PathBuf>, String> {
    let manifest = fs::canonicalize(manifest).map_err(|e| e.to_string())?;
    let results = fs::canonicalize(root.parent().ok_or("target 缺少父目录")?.join("results"))
        .map_err(|e| e.to_string())?;
    if !manifest.starts_with(results) {
        return Err("导入清单不在实验 results 中".into());
    }
    read_imports_owned(&manifest, root, sysroot_lib)
}

/// 正式上下文和P0分别核对清单归属后复用相同COFF边界，不枚举缓存猜测DLL版本。
fn read_imports_owned(manifest: &Path, root: &Path, sysroot_lib: &Path) -> Result<Vec<PathBuf>, String> {
    let sysroot_lib = fs::canonicalize(sysroot_lib).map_err(|e| e.to_string())?;
    let mut imports = Vec::new();
    for line in read_text(manifest, 64 * 1024)?.lines() {
        let path = fs::canonicalize(line).map_err(|e| e.to_string())?;
        if !(path.starts_with(root) || path.parent() == Some(sysroot_lib.as_path()))
            || !path.to_string_lossy().ends_with(".dll.lib")
        {
            return Err("导入库路径越界或不是 .dll.lib".into());
        }
        let mut magic = [0; 8];
        fs::File::open(&path).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| e.to_string())?;
        if &magic != b"!<arch>\n" || imports.contains(&path) {
            return Err("导入库格式错误或重复".into());
        }
        imports.push(path);
        if imports.len() > 128 { return Err("动态导入清单超过128项".into()); }
    }
    if imports.is_empty() { return Err("导入清单为空".into()); }
    Ok(imports)
}

fn fix_response(path: &Path, root: &Path, imports: &[PathBuf]) -> Result<Option<PathBuf>, String> {
    fix_response_for_crate(path, root, imports, "bevy_hotpatch_probe")
}

fn fix_response_for_crate(path: &Path, root: &Path, imports: &[PathBuf], bin: &str) -> Result<Option<PathBuf>, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    // 普通 rustc 的其他响应文件完全透传；dx 本身的 Windows 参数文件使用 UTF-8。
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(None);
    };
    let targets = response_stubs(text);
    if targets.is_empty() {
        return Ok(None);
    }
    if targets.len() != 1 {
        return Err("响应文件包含多个 stub.o".into());
    }
    let tokens: Vec<_> = text.split('"').skip(1).step_by(2).collect();
    let outputs: Vec<_> = tokens.iter().filter_map(|v| v.strip_prefix("/OUT:")).collect();
    if tokens.iter().filter(|&&v| v == "/DLL").count() != 1 || outputs.len() != 1 {
        return Err("含 stub.o 的响应文件不是已验证 dx DLL 补丁".into());
    }
    let output = Path::new(outputs[0]);
    // dx 的补丁文件沿用 Cargo target 原名；只有 rustc crate 名才把连字符转成下划线。
    let prefix = format!("lib{bin}-patch-");
    let valid_name = output.file_name().and_then(|v| v.to_str())
        .and_then(|v| v.strip_prefix(prefix.as_str()))
        .and_then(|v| v.strip_suffix(".dll"))
        .is_some_and(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()));
    let stub_directory = fs::canonicalize(Path::new(targets[0]).parent().ok_or("stub 缺少父目录")?)
        .map_err(|e| e.to_string())?;
    if !valid_name || !stub_directory.starts_with(root)
        || fs::canonicalize(output.parent().ok_or("补丁缺少父目录")?).map_err(|e| e.to_string())? != stub_directory {
        return Err("补丁输出名称或目录与本实验不符".into());
    }
    if let Some(replacement) = fix_object(Path::new(targets[0]), root)? {
        let mut updated = text.replace(
            &format!("\"{}\"", targets[0]),
            &format!("\"{}\"", replacement.display()),
        );
        // 只为已确认包含 stub.o 的 dx 薄补丁追加导入；普通 rustc 链接不受影响。
        for import in imports {
            updated.push_str(&format!(" \"{}\"", import.display()));
        }
        // dx 的会话缓存可位于 target 外；只读原响应文件，副本随已验证的 stub 写到 target 内。
        let output = replacement.with_file_name("windows-command.p0-chkstk.rsp");
        fs::write(&output, updated).map_err(|e| e.to_string())?;
        return Ok(Some(output));
    }
    Ok(None)
}

struct FunctionLinkContext { bin: String, imports: PathBuf, sysroot_lib: PathBuf }

/// 正式代理只接受由fat构建登记并由调用方锁定的独占会话清单；没有此变量时保留P0口径。
fn function_context(path: &Path, root: &Path) -> Result<FunctionLinkContext, String> {
    let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
    let sessions = root.join("hotpatch-tools/function-sessions");
    let parent = path.parent().ok_or("链接上下文缺少会话目录")?;
    if path.file_name().is_none_or(|name| name != "link-context.txt")
        || parent.parent() != Some(sessions.as_path()) {
        return Err("正式链接上下文不属于当前target的独占会话".into());
    }
    let text = read_text(&path, 64 * 1024)?;
    let mut lines = text.lines();
    if lines.next() != Some("FEATHRISE_FUNCTION_LINK_V1") { return Err("未知正式链接上下文版本".into()); }
    let mut fields = BTreeMap::new();
    for line in lines {
        let (key, value) = line.split_once('\t').ok_or("链接上下文字段无效")?;
        if value.is_empty() || value.chars().any(char::is_control) || fields.insert(key, value).is_some() {
            return Err("链接上下文字段为空、重复或包含控制字符".into());
        }
    }
    const KEYS: &[&str] = &["project", "root", "manifest", "package", "bin", "crate", "target", "sysroot", "imports"];
    if fields.len() != KEYS.len() || KEYS.iter().any(|key| !fields.contains_key(*key)) {
        return Err("链接上下文字段不完整或未知".into());
    }
    if fs::canonicalize(fields["target"]).map_err(|error| error.to_string())? != root {
        return Err("链接上下文target与实际构建不同".into());
    }
    let project_root = fs::canonicalize(fields["root"]).map_err(|error| error.to_string())?;
    let manifest = fs::canonicalize(fields["manifest"]).map_err(|error| error.to_string())?;
    if !project_root.is_dir() || !manifest.starts_with(&project_root)
        || manifest.file_name().is_none_or(|name| name != "Cargo.toml") {
        return Err("链接上下文工程根或清单不一致".into());
    }
    let bin = fields["bin"];
    if bin.is_empty() || bin.len() > 128 || !bin.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        || fields["crate"] != bin.replace('-', "_") {
        return Err("链接上下文bin与crate不一致".into());
    }
    let imports = PathBuf::from(fields["imports"]);
    // fat初始链接尚未产生清单；此处只验证应有路径，真正薄补丁时再读取并核对。
    if imports.parent() != Some(parent) || imports.file_name().is_none_or(|name| name != "imports.txt") {
        return Err("正式动态导入清单不属于fat会话".into());
    }
    Ok(FunctionLinkContext { bin: bin.to_owned(), imports,
        sysroot_lib: fs::canonicalize(fields["sysroot"]).map_err(|error| error.to_string())? })
}

fn read_text(path: &Path, maximum: usize) -> Result<String, String> {
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|error| error.to_string())?.take(maximum as u64 + 1)
        .read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > maximum { return Err("链接代理输入超过固定预算".into()); }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn run() -> Result<i32, String> {
    let real = env::var_os("HOTPATCH_PROBE_REAL_LINKER").ok_or("缺少真实链接器")?;
    let root = fs::canonicalize(env::var_os("CARGO_TARGET_DIR").ok_or("缺少实验 target")?)
        .map_err(|e| e.to_string())?;
    let context = env::var_os("FEATHRISE_HOTPATCH_LINK_CONTEXT")
        .map(|path| function_context(Path::new(&path), &root)).transpose()?;
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    for arg in &mut args {
        if let Some(response) = arg.to_str().and_then(|text| text.strip_prefix('@')) {
            let response = Path::new(response.trim_matches('"'));
            // 初始完整链接时清单尚未生成；仅在 dx 的 UTF-8 薄补丁响应中读取。
            let is_patch = fs::read_to_string(response).ok().is_some_and(|v| !response_stubs(&v).is_empty());
            let imports = if is_patch && let Some(context) = context.as_ref() {
                read_imports_owned(&context.imports, &root, &context.sysroot_lib)?
            } else if is_patch && let Some(manifest) = env::var_os("HOTPATCH_PROBE_IMPORT_MANIFEST") {
                read_imports(Path::new(&manifest), &root, Path::new(&env::var_os("HOTPATCH_PROBE_SYSROOT_LIB").ok_or("缺少 sysroot lib")?))?
            } else { Vec::new() };
            let replacement = if let Some(context) = context.as_ref() {
                fix_response_for_crate(response, &root, &imports, &context.bin)?
            } else { fix_response(response, &root, &imports)? };
            if let Some(replacement) = replacement {
                *arg = format!("@{}", replacement.display()).into();
            }
            continue;
        }
        let path = Path::new(arg);
        if path.file_name().is_none_or(|v| v != "stub.o") {
            continue;
        }
        if context.is_some() {
            return Err("正式dx薄补丁必须提供可核对DLL输出身份的响应文件".into());
        }
        if let Some(replacement) = fix_object(path, &root)? {
            *arg = replacement.into_os_string();
        }
    }
    Ok(Command::new(real)
        .args(args)
        .status()
        .map_err(|e| e.to_string())?
        .code()
        .unwrap_or(1))
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("P0 链接代理拒绝继续：{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_only_quoted_standalone_stub_in_dx_response() {
        let text = "\"D:/中文 目录/stub.o\" \"D:/other/ordinary.o\" \"/OUT:D:/name stub.o\"";
        assert_eq!(response_stubs(text), vec!["D:/中文 目录/stub.o"]);
        assert!(response_stubs("\"D:/stub.other.o\" /OUT:stub.o").is_empty());
    }
    #[test]
    fn function_context_binds_target_session_bin_and_exact_import_manifest() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = env::temp_dir().join(format!("function-linker-test-{}-{unique}", std::process::id()));
        let target = directory.join("target");
        let session = target.join("hotpatch-tools/function-sessions/session-test");
        let project = directory.join("中文 工程");
        let sysroot = directory.join("sysroot");
        for path in [&session, &project, &sysroot] { fs::create_dir_all(path).unwrap(); }
        let target = fs::canonicalize(target).unwrap();
        let session = fs::canonicalize(session).unwrap();
        let project = fs::canonicalize(project).unwrap();
        let sysroot = fs::canonicalize(sysroot).unwrap();
        let manifest = project.join("Cargo.toml");
        fs::write(&manifest, "[package]\nname=\"different-package\"\n").unwrap();
        let imports = session.join("imports.txt");
        let path = session.join("link-context.txt");
        let text = format!("FEATHRISE_FUNCTION_LINK_V1\nproject\towned-fixture\nroot\t{}\nmanifest\t{}\npackage\tdifferent-package\nbin\tactual-bin\ncrate\tactual_bin\ntarget\t{}\nsysroot\t{}\nimports\t{}\n",
            project.display(), manifest.display(), target.display(), sysroot.display(), imports.display());
        fs::write(&path, &text).unwrap();
        let context = function_context(&path, &target).unwrap();
        assert_eq!(context.bin, "actual-bin");
        assert_eq!(context.imports, imports);
        assert_eq!(context.sysroot_lib, sysroot);
        let stub = session.join("stub.o");
        fs::write(&stub, fixture()).unwrap();
        let response = session.join("windows-command.rsp");
        let original = format!("\"{}\" \"/DLL\" \"/OUT:{}\"", stub.display(), session.join("libactual-bin-patch-123.dll").display());
        fs::write(&response, &original).unwrap();
        let replacement = fix_response_for_crate(&response, &target, &[], &context.bin).unwrap().unwrap();
        assert_eq!(fs::read_to_string(&response).unwrap(), original);
        assert!(fix_response(&response, &target, &[]).is_err());
        fs::write(&response, original.replace("libactual-bin-patch-", "libactual_bin-patch-")).unwrap();
        assert!(fix_response_for_crate(&response, &target, &[], &context.bin).is_err());
        fs::write(&response, &original).unwrap();
        for invalid in [text.replace("crate\tactual_bin", "crate\twrong_bin"),
            text.replace("imports.txt", "other.txt"), format!("{text}bin\tactual-bin\n"), text.replace("FUNCTION_LINK_V1", "FUNCTION_LINK_V9")] {
            fs::write(&path, invalid).unwrap();
            assert!(function_context(&path, &target).is_err());
        }
        let outside = directory.join("link-context.txt");
        fs::write(&outside, &text).unwrap();
        assert!(function_context(&outside, &target).is_err());
        for path in [path, outside, manifest, stub, response, replacement, session.join("stub.p0-chkstk.o")] { fs::remove_file(path).unwrap(); }
        let sessions = session.parent().unwrap().to_owned();
        let tools = sessions.parent().unwrap().to_owned();
        for path in [session, sessions, tools, target, project, sysroot, directory] { fs::remove_dir(path).unwrap(); }
    }
    #[test]
    fn response_file_outside_target_is_read_only_and_replacement_stays_inside() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            env::temp_dir().join(format!("p0-linker-test-{}-{unique}", std::process::id()));
        let target = directory.join("中文 target");
        fs::create_dir_all(&target).unwrap();
        let target = fs::canonicalize(target).unwrap();
        let object = target.join("stub.o");
        fs::write(&object, fixture()).unwrap();
        let results = directory.join("results");
        fs::create_dir(&results).unwrap();
        let sysroot = directory.join("sysroot");
        fs::create_dir(&sysroot).unwrap();
        let import = target.join("bevy_dylib-test.dll.lib");
        fs::write(&import, b"!<arch>\n").unwrap();
        let std_import = sysroot.join("std-test.dll.lib");
        fs::write(&std_import, b"!<arch>\n").unwrap();
        let manifest = results.join("imports.txt");
        fs::write(&manifest, format!("{}\n{}\n", import.display(), std_import.display())).unwrap();
        let imports = read_imports(&manifest, &target, &sysroot).unwrap();
        assert_eq!(imports.len(), 2);
        let response = directory.join("windows_command.txt");
        let original = format!("\"{}\" \"/DLL\" \"/OUT:{}\"", object.display(), target.join("libbevy_hotpatch_probe-patch-123.dll").display());
        fs::write(&response, &original).unwrap();
        let updated = fix_response(&response, &target, &[]).unwrap().unwrap();
        assert!(updated.starts_with(&target));
        assert_eq!(fs::read_to_string(&response).unwrap(), original);
        assert_eq!(fs::read(&object).unwrap(), fixture());
        assert!(
            fs::read_to_string(&updated)
                .unwrap()
                .contains("stub.p0-chkstk.o")
        );
        let updated = fix_response(&response, &target, &imports).unwrap().unwrap();
        let patched = fs::read_to_string(&updated).unwrap();
        assert!(patched.ends_with(&format!(" \"{}\" \"{}\"", imports[0].display(), imports[1].display())));
        assert_eq!(fs::read_to_string(&response).unwrap(), original);
        fs::write(&manifest, "").unwrap();
        assert!(read_imports(&manifest, &target, &sysroot).is_err());
        fs::write(&manifest, format!("{}\n{}", import.display(), import.display())).unwrap();
        assert!(read_imports(&manifest, &target, &sysroot).is_err());
        fs::write(&manifest, directory.join("foreign.dll.lib").to_string_lossy().as_bytes()).unwrap();
        let foreign = directory.join("foreign.dll.lib");
        fs::write(&foreign, b"!<arch>\n").unwrap();
        assert!(read_imports(&manifest, &target, &sysroot).is_err());
        fs::write(&import, b"not-COFF").unwrap();
        fs::write(&manifest, import.to_string_lossy().as_bytes()).unwrap();
        assert!(read_imports(&manifest, &target, &sysroot).is_err());
        assert!(read_imports(&response, &target, &sysroot).is_err());
        // 没有 stub.o 的普通响应文件不添加导入，也不生成修改副本。
        let ordinary = results.join("ordinary.rsp");
        fs::write(&ordinary, "\"ordinary.o\"").unwrap();
        assert!(fix_response(&ordinary, &target, &imports).unwrap().is_none());
        fs::write(&ordinary, original.replace("/DLL", "/EXE")).unwrap();
        assert!(fix_response(&ordinary, &target, &imports).is_err());
        fs::write(&ordinary, original.replace("patch-123.dll", "unknown.dll")).unwrap();
        assert!(fix_response(&ordinary, &target, &imports).is_err());
        let outside = directory.join("stub.o");
        fs::write(&outside, fixture()).unwrap();
        assert!(fix_object(&outside, &target).is_err());
        // 仅删除本测试明确创建的文件，再删除空目录；不用递归清理临时根。
        for path in [
            updated,
            object,
            target.join("stub.p0-chkstk.o"),
            response,
            outside,
            manifest,
            import,
            std_import,
            foreign,
            ordinary,
        ] {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(target).unwrap();
        fs::remove_dir(results).unwrap();
        fs::remove_dir(sysroot).unwrap();
        fs::remove_dir(directory).unwrap();
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 98];
        b[..2].copy_from_slice(&0x8664u16.to_le_bytes());
        b[2..4].copy_from_slice(&1u16.to_le_bytes());
        b[8..12].copy_from_slice(&76u32.to_le_bytes());
        b[12..16].copy_from_slice(&1u32.to_le_bytes());
        b[20..28].copy_from_slice(b".text\0\0\0");
        b[36..40].copy_from_slice(&16u32.to_le_bytes());
        b[40..44].copy_from_slice(&60u32.to_le_bytes());
        b[60..62].copy_from_slice(&[0x48, 0xb8]);
        b[62..70].copy_from_slice(&0x123456789abcdef0u64.to_le_bytes());
        b[70..72].copy_from_slice(&[0xff, 0xe0]);
        b[76..84].copy_from_slice(b"__chkstk");
        b[88..90].copy_from_slice(&1u16.to_le_bytes());
        b[94..98].copy_from_slice(&4u32.to_le_bytes());
        b
    }
    #[test]
    fn changes_only_stub_and_preserves_absolute_target() {
        let b = fixture();
        let (fixed, offset) = fix_stack_probe(&b).unwrap().unwrap();
        assert_eq!(offset, 60);
        assert_eq!(&fixed[..60], &b[..60]);
        assert_eq!(&fixed[76..], &b[76..]);
        assert_eq!(&fixed[60..66], &[0xff, 0x25, 0, 0, 0, 0]);
        assert_eq!(&fixed[66..74], &b[62..70]);
    }
    #[test]
    fn rejects_unknown_or_occupied_code_slot() {
        for byte in [60, 70, 72] {
            let mut b = fixture();
            b[byte] ^= 1;
            assert!(fix_stack_probe(&b).is_err());
        }
        let mut b = fixture();
        b[76..84].copy_from_slice(b"ordinary");
        assert!(fix_stack_probe(&b).unwrap().is_none());
    }
    #[test]
    fn rejects_every_truncated_fixture_without_panicking() {
        let b = fixture();
        for end in 0..b.len() {
            assert!(fix_stack_probe(&b[..end]).is_err());
        }
    }
}
