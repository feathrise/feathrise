//! 发行构建可显式携入独立编码器。公开源码构建不查找、不下载本机工具。
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

pub fn embed() {
    const VARIABLE: &str = "FEATHRISE_FFMPEG_BUNDLE";
    println!("cargo::rerun-if-env-changed={VARIABLE}");
    println!("cargo::rerun-if-changed=build/ffmpeg.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let generated = out.join("embedded_ffmpeg.rs");
    let Some(path) = env::var_os(VARIABLE).map(PathBuf::from) else {
        fs::write(
            generated,
            "const PAYLOAD: Option<Payload<'static>> = None;\n",
        )
        .expect("无法生成空的 FFmpeg 内嵌入口");
        return;
    };
    assert!(path.is_absolute(), "{VARIABLE} 必须是编码器的绝对文件路径");
    assert!(path.is_file(), "{VARIABLE} 指定的编码器文件不存在");
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("windows"),
        "内嵌 FFmpeg 用于 Windows"
    );
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ARCH").as_deref(),
        Ok("x86_64"),
        "内嵌 FFmpeg 用于 x86_64"
    );
    println!("cargo::rerun-if-changed={}", path.display());
    let mut input = fs::File::open(path).expect("无法读取内嵌 FFmpeg 文件");
    assert_x64_exe(&mut input);
    let compressed = out.join("ffmpeg.exe.deflate");
    let file = fs::File::create(&compressed).expect("无法创建 FFmpeg 压缩资源");
    let mut encoder = flate2::write::DeflateEncoder::new(file, flate2::Compression::default());
    let mut sha = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).expect("无法读取 FFmpeg 字节");
        if count == 0 {
            break;
        }
        let chunk = &buffer[..count];
        size += count as u64;
        sha.update(chunk);
        encoder.write_all(chunk).expect("无法压缩 FFmpeg 字节");
    }
    encoder.finish().expect("无法完成 FFmpeg 压缩");
    let digest: String = sha.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let marker = format!("FEATHRISE_EMBEDDED_FFMPEG_V1\0{digest}\0FEATHRISE_EMBEDDED_FFMPEG_END");
    fs::write(generated, format!(
        "const PAYLOAD: Option<Payload<'static>> = Some(Payload {{ size: {size}, sha256: {digest:?}, compressed: include_bytes!({compressed:?}), marker: {marker:?} }});\n"
    )).expect("无法生成 FFmpeg 内嵌入口");
}

fn assert_x64_exe(input: &mut fs::File) {
    let mut dos = [0; 64];
    input.read_exact(&mut dos).expect("FFmpeg 缺少完整 DOS 头");
    assert_eq!(&dos[..2], b"MZ", "FFmpeg 不是 Windows 可执行文件");
    let start = u32::from_le_bytes(dos[60..].try_into().unwrap());
    input
        .seek(SeekFrom::Start(u64::from(start)))
        .expect("FFmpeg PE 头偏移无效");
    let mut pe = [0; 24];
    input.read_exact(&mut pe).expect("FFmpeg 缺少完整 PE 头");
    assert_eq!(
        &pe[..6],
        b"PE\0\0\x64\x86",
        "FFmpeg 不是 Windows x86_64 可执行文件"
    );
    let flags = u16::from_le_bytes([pe[22], pe[23]]);
    assert!(
        flags & 0x0002 != 0 && flags & 0x2000 == 0,
        "FFmpeg 必须是独立 EXE"
    );
    input.rewind().expect("无法重新读取 FFmpeg 字节");
}
