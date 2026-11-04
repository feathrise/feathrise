//! 按显式输入嵌入可选品牌资源，写入Windows产品信息并保留MSVC主线程栈保护。

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    validation_extension();
    resource_identity();
    let branding = branding_resources();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let opt_level = std::env::var("OPT_LEVEL").unwrap_or_default();
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
            .compile()
            .expect("无法编译Feathrise程序图标资源");
    }
    if target_os == "windows" && target_env == "msvc" && opt_level == "0" {
        // 原完整BSN在opt0构造时超出默认1MiB；仅增加保留虚拟地址，仍按需提交栈页。
        println!("cargo::rustc-link-arg-bin=feathrise=/STACK:8388608");
    }
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
    println!("cargo::rerun-if-changed=build/runtime_resources.json");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string("build/runtime_resources.json").expect("无法读取作者资源清单"),
    ).expect("作者资源清单格式错误");
    let mut files = manifest["required_files"].as_array().expect("资源清单缺少文件列表")
        .iter().map(|v| v.as_str().expect("资源路径必须为字符串")).collect::<Vec<_>>();
    files.sort_unstable();
    let mut hash = Sha256::new();
    for path in files {
        println!("cargo::rerun-if-changed={path}");
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("作者资源 {path} 不可读取：{e}"));
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
