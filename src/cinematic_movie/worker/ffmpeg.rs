//! 仅在导出后台准备编码器。单 EXE 首次提取，后续按内容摘要复用同一用户缓存。
use super::BuildControl;
use std::path::PathBuf;

pub(super) fn resolve(
    explicit: Option<PathBuf>,
    control: &BuildControl,
) -> Result<PathBuf, String> {
    control.check()?;
    if let Some(path) = explicit.or_else(|| std::env::var_os("FEATHRISE_FFMPEG").map(PathBuf::from))
    {
        return Ok(path);
    }
    #[cfg(windows)]
    {
        if let Some(path) = embedded::resolve(control)? {
            return Ok(path);
        }
        let developer = PathBuf::from("D:/SDK/ffmpeg/bin/ffmpeg.exe");
        if developer.is_file() {
            return Ok(developer);
        }
    }
    Ok("ffmpeg".into())
}

#[cfg(windows)]
mod embedded {
    use super::{super::Directory, BuildControl};
    use std::{
        fs,
        io::{Read, Write},
        os::windows::fs::MetadataExt,
        path::{Path, PathBuf},
    };

    struct Payload<'a> {
        size: u64,
        sha256: &'a str,
        compressed: &'a [u8],
        marker: &'a str,
    }
    include!(concat!(env!("OUT_DIR"), "/embedded_ffmpeg.rs"));

    pub(super) fn resolve(control: &BuildControl) -> Result<Option<PathBuf>, String> {
        let Some(payload) = PAYLOAD else {
            return Ok(None);
        };
        std::hint::black_box(payload.marker); // 打包器只读 PE 标记校验内嵌原文件摘要。
        let cache = crate::writable_paths::user_cache_dir()?;
        materialize(&cache, &payload, control)
            .map(Some)
            .map_err(|error| format!("无法准备内嵌 FFmpeg：{error}"))
    }

    fn materialize(
        cache: &Path,
        payload: &Payload,
        control: &BuildControl,
    ) -> Result<PathBuf, String> {
        control.check()?;
        let local = cache.parent().ok_or("FFmpeg 缓存缺少用户目录")?;
        // 用户数据目录可通过系统配置移动。规范化后只在真实目录中建立 Feathrise 子树。
        fs::create_dir_all(local).map_err(|error| error.to_string())?;
        let mut root = local.canonicalize().map_err(|error| error.to_string())?;
        let cache_name = cache.file_name().ok_or("FFmpeg 缓存缺少目录名")?;
        for part in [
            cache_name,
            "tools".as_ref(),
            "embedded-ffmpeg-v1".as_ref(),
            payload.sha256.as_ref(),
        ] {
            root.push(part);
            match fs::create_dir(&root) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(error.to_string()),
            }
            if !ordinary_directory(&root) {
                return Err(format!("编码器缓存不是实际目录：{}", root.display()));
            }
        }

        let candidates = || {
            std::iter::once(root.join("ready"))
                .chain((1..=8).map(|index| root.join(format!("recovery-{index:02}"))))
        };
        for candidate in candidates() {
            if let Some(path) = verified(&candidate, payload, control)? {
                return Ok(path);
            }
        }

        // 同卷完整解压、摘要校验后才发布。在途工作只占已有后台导出槽。
        let stage = Directory::create(&root)?;
        let staged = stage.0.join("ffmpeg.exe");
        let mut decoder = flate2::read::DeflateDecoder::new(payload.compressed);
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|error| error.to_string())?;
        let mut buffer = [0; 64 * 1024];
        let mut written = 0u64;
        loop {
            control.check()?;
            let count = decoder
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            written += count as u64;
            if written > payload.size {
                return Err("解压超过内嵌长度".into());
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| error.to_string())?;
        }
        output.sync_all().map_err(|error| error.to_string())?;
        drop(output);
        if verified(&stage.0, payload, control)?.is_none() {
            return Err("内嵌编码器内容与摘要不符".into());
        }
        for candidate in candidates() {
            control.check()?;
            // 与 DX 缓存一样保留旧槽及活进程，多个编辑器同时发布时采用核验过的胜出目录。
            let published =
                feathrise_editor_assets::files::publish_staged_file(&stage.0, &candidate, false);
            if published.is_ok() {
                return Ok(candidate.join("ffmpeg.exe"));
            }
            if let Some(path) = verified(&candidate, payload, control)? {
                return Ok(path);
            }
        }
        Err("FFmpeg 完整缓存的发布槽均不可用".into())
    }

    fn ordinary_directory(path: &Path) -> bool {
        fs::symlink_metadata(path)
            .is_ok_and(|item| item.is_dir() && item.file_attributes() & 0x400 == 0)
            && path.canonicalize().is_ok_and(|real| real == path)
    }

    fn verified(
        directory: &Path,
        payload: &Payload,
        control: &BuildControl,
    ) -> Result<Option<PathBuf>, String> {
        control.check()?;
        let path = directory.join("ffmpeg.exe");
        let valid = ordinary_directory(directory)
            && fs::symlink_metadata(&path).is_ok_and(|item| {
                item.is_file() && item.len() == payload.size && item.file_attributes() & 0x400 == 0
            })
            && crate::build_artifacts::hash_file(&path, control)
                .is_ok_and(|(hash, size)| hash == payload.sha256 && size == payload.size);
        control.check()?;
        Ok(valid.then_some(path))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn bundled_ffmpeg_starts_with_h264_in_an_isolated_cache() {
            let Some(payload) = PAYLOAD else { return };
            let fixture = Directory::create(&std::env::temp_dir()).unwrap();
            let binary = materialize(
                &fixture.0.join("Feathrise"),
                &payload,
                &BuildControl::default(),
            )
            .unwrap();
            let output = std::process::Command::new(&binary)
                .args(["-hide_banner", "-h", "encoder=libx264"])
                .output()
                .unwrap();
            assert!(
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("Encoder libx264")
            );
        }

        #[test]
        fn embedded_ffmpeg_reuses_complete_cache_and_recovers_without_overwriting() {
            let fixture = Directory::create(&std::env::temp_dir()).unwrap();
            let cache = fixture.0.join("Feathrise");
            let data = b"isolated synthetic encoder payload";
            let digest = crate::build_artifacts::digest(data);
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(data).unwrap();
            let compressed = encoder.finish().unwrap();
            let payload = Payload {
                size: data.len() as u64,
                sha256: &digest,
                compressed: &compressed,
                marker: "",
            };
            let control = BuildControl::default();
            let path = materialize(&cache, &payload, &control).unwrap();
            assert_eq!(fs::read(&path).unwrap(), data);
            assert_eq!(materialize(&cache, &payload, &control).unwrap(), path);
            fs::write(&path, vec![0u8; data.len()]).unwrap();
            let recovered = materialize(&cache, &payload, &control).unwrap();
            assert_ne!(recovered, path);
            assert_eq!(fs::read(&recovered).unwrap(), data);
            assert_eq!(materialize(&cache, &payload, &control).unwrap(), recovered);
            let stopped = BuildControl::default();
            stopped.cancel();
            let unused = fixture.0.join("unused");
            assert!(materialize(&unused, &payload, &stopped).is_err());
            assert!(!unused.exists());
        }
    }
}
