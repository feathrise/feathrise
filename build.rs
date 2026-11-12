//! 按显式输入嵌入可选品牌资源，写入Windows产品信息并保留MSVC主线程栈保护。

#[path = "build/dx.rs"]
mod dx;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    validation_extension();
    resource_identity();
    embed_licenses();
    dx::embed();
    let branding = branding_resources();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" {
        let mut resources = winresource::WindowsResource::new();
        if let Some(directory) = branding {
            // 只有显式提供品牌文件的构建才写入ID 1；公开源码默认使用系统图标。
            resources.set_icon(directory.join("feathrise.ico").to_str().expect("品牌输出路径必须为UTF-8"));
        }
        resources
            .set("ProductName", "Feathrise")
            .set("FileDescription", "Feathrise（羽立）编辑器")
            .set("InternalName", "feathrise")
            .set("OriginalFilename", "feathrise.exe")
            // 预发布后缀与「关于」同取 Cargo 版本，Windows 文件属性也能识别具体候选。
            .set("FileVersion", env!("CARGO_PKG_VERSION"))
            .set("ProductVersion", env!("CARGO_PKG_VERSION"))
            .compile()
            .expect("无法编译Feathrise程序图标资源");
    }
    if target_os == "windows" && target_env == "msvc" {
        // 主窗口初始化可能超过默认 1 MiB，宿主程序及其库测试统一保留 8 MiB 栈。
        println!("cargo::rustc-link-arg=/STACK:8388608");
    }
}

/// 源码构建内嵌本仓许可；单文件发行可以传入按实际依赖生成的正文。
fn embed_licenses() {
    use std::{env, fs, io::Read, path::PathBuf};
    const VARIABLE: &str = "FEATHRISE_LICENSE_DOCUMENT";
    const LIMIT: u64 = 4 * 1024 * 1024;
    println!("cargo::rerun-if-env-changed={VARIABLE}");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut data = Vec::new();
    if let Some(path) = env::var_os(VARIABLE).map(PathBuf::from) {
        assert!(env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
            && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
            && env::var("PROFILE").as_deref() == Ok("release"),
            "发行许可文档用于 Windows MSVC Release");
        assert!(env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default().split(',').any(|v| v == "crt-static")
            && ["CARGO_FEATURE_BEVY_DYNAMIC_LINKING", "CARGO_FEATURE_RTX_RENDERING", "CARGO_FEATURE_FSR3_FRAME_GENERATION"]
                .iter().all(|name| env::var_os(name).is_none()),
            "单文件发行需要静态 CRT，且不能选择 Bevy 动态链接或依赖外置 DLL 的厂商组件");
        assert!(path.is_absolute() && path.is_file(), "{VARIABLE} 必须是已生成许可文档的绝对路径");
        println!("cargo::rerun-if-changed={}", path.display());
        fs::File::open(path).expect("无法打开发行许可文档").take(LIMIT + 1)
            .read_to_end(&mut data).expect("无法读取发行许可文档");
    } else {
        // 普通 Cargo 构建只需编辑器源码检出，直接复用仓库随附的声明和许可原文。
        data.extend_from_slice("Feathrise 源码许可与署名\n\n本页收录源码仓库的许可原文和第三方说明。\n\n".as_bytes());
        for name in ["THIRD_PARTY_NOTICES.md", "LICENSE-MIT", "LICENSE-APACHE"] {
            println!("cargo::rerun-if-changed={name}");
            data.extend(fs::read(name).unwrap_or_else(|error| panic!("无法读取 {name}：{error}")));
            data.extend_from_slice(b"\n\n");
        }
    }
    assert!(!data.is_empty() && data.len() as u64 <= LIMIT, "许可文档必须为不超过 4 MiB 的非空文本");
    let text = std::str::from_utf8(&data).expect("许可文档必须为 UTF-8");
    assert!(!text.trim_start().starts_with('<'), "许可文档使用纯文本");
    // 构建时按全文容量分到最多50页。每页至少32 KiB，避免完整发行文档出现数百页。
    // 向后最多找1 KiB内的行尾，保留UTF-8边界；4 MiB输入下单页仍小于84 KiB。
    const MAX_PAGES: usize = 50;
    const LINE_LOOKAHEAD: usize = 1024;
    let page_bytes = (32 * 1024).max(text.len().div_ceil(MAX_PAGES));
    let mut offsets = vec![0];
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + page_bytes).min(text.len());
        while !text.is_char_boundary(end) { end += 1; }
        if end < text.len() {
            let line_limit = (end + LINE_LOOKAHEAD).min(text.len());
            if let Some(line_end) = data[end..line_limit].iter().position(|&byte| byte == b'\n') {
                end += line_end + 1;
            }
        }
        offsets.push(end);
        start = end;
    }
    fs::write(out.join("distribution-licenses.txt"), data).expect("无法内嵌发行许可文档");
    fs::write(out.join("distribution-license-pages.rs"), format!("&{offsets:?}"))
        .expect("无法写入发行许可分页索引");
}

/// 正式Logo由仓外私有材料提供；普通源码构建不探测父仓或用户目录，也不下载品牌文件。
fn branding_resources() -> Option<std::path::PathBuf> {
    use std::{env, fs, path::PathBuf};
    const VARIABLE: &str = "FEATHRISE_BRANDING_DIR";
    println!("cargo::rustc-check-cfg=cfg(feathrise_custom_branding)");
    println!("cargo::rerun-if-env-changed={VARIABLE}");
    let directory = PathBuf::from(env::var_os(VARIABLE)?);
    assert!(directory.is_absolute(), "{VARIABLE} 必须为品牌资源目录的绝对路径");
    let directory = directory.canonicalize().expect("FEATHRISE_BRANDING_DIR 指定的目录不存在或无法读取");
    assert!(directory.is_dir(), "{VARIABLE} 必须指向目录");
    let names = ["feathrise-mark.png", "feathrise.ico"];
    // 显式输入必须完整；缺文件时拒绝构建，避免正式发行静默混入无Logo的产物。
    for name in names {
        let source = directory.join(name);
        assert!(source.is_file(), "{VARIABLE} 缺少品牌文件：{}", source.display());
        println!("cargo::rerun-if-changed={}", source.display());
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for name in names {
        fs::copy(directory.join(name), output.join(name)).expect("无法复制编译用品牌资源");
    }
    println!("cargo::rustc-cfg=feathrise_custom_branding");
    Some(output)
}

/// 仅显式验收编译可接入应用自有的 Rust 场景；普通构建不读取、编译或安装这些系统。
fn validation_extension() {
    use std::{env, fs, path::PathBuf};
    const VARIABLE: &str = "FEATHRISE_VALIDATION_EXTENSION";
    println!("cargo::rustc-check-cfg=cfg(feathrise_external_validation)");
    println!("cargo::rerun-if-env-changed={VARIABLE}");
    if env::var_os("CARGO_FEATURE_VALIDATION").is_none() { return; }
    let Some(source) = env::var_os(VARIABLE) else { return; };
    let source = PathBuf::from(source);
    assert!(source.is_absolute(), "{VARIABLE} 必须为外部适配器的绝对路径");
    let source = source.canonicalize().expect("外部验收适配器不存在或无法读取");
    assert!(source.is_file() && source.extension().is_some_and(|s| s == "rs"), "外部验收适配器必须是 Rust 源文件");
    let path = source.to_str().expect("外部验收适配器路径必须为 UTF-8");
    assert!(!path.chars().any(char::is_control), "外部验收适配器路径不能包含控制字符");
    // 目录监听覆盖适配器自己的子模块；不扫描用户工程，也不改变已有编译缓存位置。
    println!("cargo::rerun-if-changed={}", source.parent().unwrap().display());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(output.join("plugin_validation_extension.rs"), format!("#[path = {path:?}] mod extension;\n"))
        .expect("无法生成外部验收适配器入口");
    println!("cargo::rustc-cfg=feathrise_external_validation");
}

/// 组包与缓存共用编译时身份；只覆盖固定作者资源，不含机器配置或用户工程。
fn resource_identity() {
    use sha2::{Digest, Sha256};
    use std::{fs, path::PathBuf};
    // 与资源嵌入器共享来源变化信号；跨工作树复用 target 时，另一份检出的旧 mtime
    // 可能仍被 Cargo 判为 fresh，不能只跟踪相对文件而让身份落后于新嵌入字节。
    println!("cargo::rerun-if-env-changed=BEVY_ASSET_PATH");
    // Cargo 内置 CARGO_MANIFEST_DIR 不参与环境变化跟踪；由项目配置提供独立绑定，
    // 即使显式共用同一 assets 根，换检出后也重新读取该检出的清单与 Cargo.lock。
    const SOURCE_VARIABLE: &str = "FEATHRISE_BUILD_SOURCE_ROOT";
    println!("cargo::rerun-if-env-changed={SOURCE_VARIABLE}");
    let source_root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let configured_root = PathBuf::from(std::env::var_os(SOURCE_VARIABLE).expect(
        "缺少编辑器构建来源绑定；请在编辑器源码根目录执行 Cargo，或显式传入 --config <源码根>/.cargo/config.toml"
    ));
    assert!(configured_root.is_absolute(), "{SOURCE_VARIABLE} 必须为源码根的绝对路径");
    assert_eq!(configured_root.canonicalize().expect("编辑器构建来源根不可读取"),
        source_root.canonicalize().expect("编辑器 Cargo 源码根不可读取"),
        "{SOURCE_VARIABLE} 与当前编辑器源码根不一致；请加载当前源码的 .cargo/config.toml");
    println!("cargo::rerun-if-changed=build/runtime_resources.json");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(source_root.join("build/runtime_resources.json")).expect("无法读取作者资源清单"),
    ).expect("作者资源清单格式错误");
    let mut files = manifest["required_files"].as_array().expect("资源清单缺少文件列表")
        .iter().map(|v| v.as_str().expect("资源路径必须为字符串")).collect::<Vec<_>>();
    files.sort_unstable();
    let mut hash = Sha256::new();
    for path in files {
        println!("cargo::rerun-if-changed={path}");
        let bytes = fs::read(source_root.join(path)).unwrap_or_else(|e| panic!("作者资源 {path} 不可读取：{e}"));
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(Sha256::digest(&bytes));
    }
    let identity: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("resource_identity.rs"), format!(
        "const RESOURCE_ID: &str = {identity:?};\nstatic RESOURCE_IDENTITY: &str = concat!(\"FEATHRISE_EMBEDDED_RESOURCES_V1\\0\", {identity:?}, \"\\0FEATHRISE_EMBEDDED_RESOURCES_END\");\n"
    )).expect("无法生成内嵌资源身份");
}
