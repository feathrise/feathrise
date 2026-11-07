//! 将受检的 Windows DX 及其应用本地运行库压缩嵌入，源码构建可显式提供相同目录。
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::PathBuf};

pub fn embed() {
    const VARIABLE: &str = "FEATHRISE_DX_BUNDLE";
    println!("cargo::rerun-if-env-changed={VARIABLE}");
    println!("cargo::rerun-if-changed=build/dx.rs");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut entries = Vec::new();
    let mut identity = Sha256::new();
    if let Some(directory) = env::var_os(VARIABLE).map(PathBuf::from) {
        assert!(directory.is_absolute(), "{VARIABLE} 必须是绝对目录");
        assert_eq!(env::var("CARGO_CFG_TARGET_OS").as_deref(), Ok("windows"), "内嵌 DX 用于 Windows");
        assert_eq!(env::var("CARGO_CFG_TARGET_ARCH").as_deref(), Ok("x86_64"), "内嵌 DX 用于 x86_64");
        for name in ["dx.exe", "vcruntime140.dll"] {
            let source = directory.join(name);
            println!("cargo::rerun-if-changed={}", source.display());
            let bytes = fs::read(&source).unwrap_or_else(|error| panic!("无法读取 DX 输入 {name}：{error}"));
            assert!(!bytes.is_empty() && bytes.len() <= 96 * 1024 * 1024, "DX 输入大小无效");
            let digest: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
            if name == "dx.exe" {
                // 固定到已验证的官方 0.7.9 Windows x86_64 发布字节，不接受同版本的未知工具。
                assert_eq!(digest, "7395c0bbbe17e6aa86215634cf511387fd1ff8afba58661d0ac81fab28eb9fed", "DX 不是已核对的官方 0.7.9");
            }
            identity.update(name.as_bytes());
            identity.update(digest.as_bytes());
            let compressed = output.join(format!("{name}.deflate"));
            let mut encoder = flate2::write::DeflateEncoder::new(fs::File::create(&compressed).unwrap(), flate2::Compression::default());
            encoder.write_all(&bytes).expect("无法压缩 DX 输入");
            encoder.finish().expect("无法完成 DX 压缩");
            entries.push(format!("Payload {{ name: {name:?}, bytes: {}, sha256: {digest:?}, compressed: include_bytes!({:?}) }}", bytes.len(), compressed));
        }
    }
    let digest: String = identity.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    fs::write(output.join("embedded_dx.rs"), format!(
        "const IDENTITY: &str = {digest:?};\nconst PAYLOAD: &[Payload] = &[{}];\nconst MARKER: &str = \"FEATHRISE_EMBEDDED_DX_V1\\0{digest}\\0FEATHRISE_EMBEDDED_DX_END\";\n", entries.join(",\n")
    )).expect("无法生成 DX 内嵌入口");
}
