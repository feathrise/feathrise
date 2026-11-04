//! 资产音频的后台完整解码与预算校验；只返回完整PCM，不触碰ECS、设备或播放状态。

use super::source::PcmData;
use bevy::audio::{ChannelCount, SampleRate};
use std::{
    io::{Cursor, ErrorKind},
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use symphonia::core::{
    audio::{SampleBuffer, SignalSpec},
    codecs::DecoderOptions,
    errors::Error,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

const MIB: usize = 1024 * 1024;

#[derive(Clone, Copy)]
struct Limits {
    encoded_bytes: usize,
    pcm_bytes: usize,
    packet_bytes: usize,
}

const LIMITS: Limits = Limits {
    encoded_bytes: 64 * MIB,
    pcm_bytes: 256 * MIB,
    packet_bytes: 16 * MIB,
};

pub(super) fn decode(
    bytes: Vec<u8>,
    extension: &str,
    cancel: &AtomicBool,
) -> Result<Arc<PcmData>, String> {
    // 解码器对异常作者文件仍可能有内部断言；后台异常变成真实失败，不让编辑器主线程panic。
    catch_unwind(AssertUnwindSafe(|| {
        decode_with_limits(bytes, extension, cancel, LIMITS)
    }))
    .unwrap_or_else(|_| Err("音频解码器无法处理此文件，请检查文件是否损坏".into()))
}

fn decode_with_limits(
    bytes: Vec<u8>,
    extension: &str,
    cancel: &AtomicBool,
    limits: Limits,
) -> Result<Arc<PcmData>, String> {
    check_cancel(cancel)?;
    if bytes.is_empty() {
        return Err("音频文件为空".into());
    }
    if bytes.len() > limits.encoded_bytes {
        return Err("音频文件超过试听读取预算（64 MiB）".into());
    }
    let exact_length = bytes.starts_with(b"fLaC") || validate_wav_bounds(&bytes, cancel)?;
    check_cancel(cancel)?;
    let mut hint = Hint::new();
    hint.with_extension(extension.trim_start_matches('.'));
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let options = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let mut format = symphonia::default::get_probe()
        .format(&hint, stream, &options, &MetadataOptions::default())
        .map_err(|error| format!("无法识别音频文件：{error}"))?
        .format;
    check_cancel(cancel)?;
    let registry = symphonia::default::get_codecs();
    let track = format
        .default_track()
        .filter(|track| registry.get_codec(track.codec_params.codec).is_some())
        .or_else(|| {
            format
                .tracks()
                .iter()
                .find(|track| registry.get_codec(track.codec_params.codec).is_some())
        })
        .ok_or_else(|| "文件中没有可试听的音频轨道".to_string())?;
    let track_id = track.id;
    let parameters = track.codec_params.clone();
    if parameters
        .sample_rate
        .is_some_and(|rate| rate == 0 || rate > 192_000)
        || parameters
            .channels
            .is_some_and(|channels| channels.count() == 0 || channels.count() > 8)
    {
        return Err("试听支持最多8声道、最高192000 Hz的音频".into());
    }
    let mut decoder = registry
        .make(&parameters, &DecoderOptions { verify: true })
        .map_err(|error| format!("无法建立音频解码器：{error}"))?;
    let mut samples = Vec::<f32>::new();
    let mut specification: Option<SignalSpec> = None;
    let mut converted: Option<SampleBuffer<f32>> = None;
    let mut converted_capacity = 0usize;

    loop {
        check_cancel(cancel)?;
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Symphonia 0.5通过UnexpectedEof表示流结束；WAV物理长度与已声明精确帧数另行验收。
            Err(Error::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(format!("音频读取失败：{error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        if packet.data.len() > limits.packet_bytes {
            return Err("音频单包超过试听临时预算（16 MiB）".into());
        }
        check_cancel(cancel)?;
        let decoded = decoder
            .decode(&packet)
            .map_err(|error| format!("音频数据损坏或解码失败：{error}"))?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        if channels == 0 || channels > 8 || spec.rate == 0 || spec.rate > 192_000 {
            return Err("试听支持最多8声道、最高192000 Hz的音频".into());
        }
        if specification.is_some_and(|previous| previous != spec) {
            return Err("音频中途改变声道或采样率，不能完整试听".into());
        }
        specification = Some(spec);
        let packet_capacity =
            checked_sample_count(decoded.capacity(), channels, limits.packet_bytes)?;
        let packet_samples = checked_sample_count(decoded.frames(), channels, limits.packet_bytes)?;
        let required = samples
            .len()
            .checked_add(packet_samples)
            .ok_or_else(|| "音频PCM长度溢出".to_string())?;
        ensure_pcm_capacity(&mut samples, required, limits.pcm_bytes)?;
        check_cancel(cancel)?;
        if converted_capacity < packet_capacity {
            // 容量校验必须先于SampleBuffer分配；替换时先归还旧缓冲，不同时保留两个转换包。
            drop(converted.take());
            converted_capacity = packet_capacity;
            converted = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        if packet_samples == 0 {
            continue;
        }
        let buffer = converted
            .as_mut()
            .ok_or_else(|| "音频包未提供可转换的采样缓冲".to_string())?;
        buffer.copy_interleaved_ref(decoded);
        check_cancel(cancel)?;
        if buffer.samples().len() != packet_samples || buffer.samples().len() % channels != 0 {
            return Err("音频数据没有完整声道帧".into());
        }
        if buffer.samples().iter().any(|sample| !sample.is_finite()) {
            return Err("音频包含无效采样值".into());
        }
        samples.extend_from_slice(buffer.samples());
        check_cancel(cancel)?;
    }
    check_cancel(cancel)?;
    let spec = specification.ok_or_else(|| "音频没有可解码的采样数据".to_string())?;
    if samples.is_empty() || samples.len() % spec.channels.count() != 0 {
        return Err("音频没有完整的声道帧".into());
    }
    // 原始PCM/FLAC长度是精确信息；MP3无Xing时长度可能只是估计，不能据估值拒绝有效VBR。
    if exact_length
        && parameters
            .n_frames
            .filter(|frames| *frames != 0)
            .is_some_and(|frames| frames != (samples.len() / spec.channels.count()) as u64)
    {
        return Err("音频被截断，已解码帧数与文件声明不一致".into());
    }
    if decoder.finalize().verify_ok == Some(false) {
        return Err("音频完整性校验失败".into());
    }
    check_cancel(cancel)?;
    let channels = ChannelCount::new(spec.channels.count() as u16)
        .ok_or_else(|| "音频声道数量无效".to_string())?;
    let sample_rate = SampleRate::new(spec.rate).ok_or_else(|| "音频采样率无效".to_string())?;
    // Vec与Arc转换最坏各持256 MiB，复制只发生在后台；主线程最终采用一份完整共享结果。
    let result = Arc::new(PcmData {
        samples: samples.into(),
        channels,
        sample_rate,
    });
    check_cancel(cancel)?;
    Ok(result)
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("音频试听加载已取消".into())
    } else {
        Ok(())
    }
}

fn checked_sample_count(frames: usize, channels: usize, budget: usize) -> Result<usize, String> {
    let samples = frames
        .checked_mul(channels)
        .filter(|samples| *samples <= budget / size_of::<f32>())
        .ok_or_else(|| "音频单包超过试听临时预算（16 MiB）".to_string())?;
    Ok(samples)
}

fn ensure_pcm_capacity(
    samples: &mut Vec<f32>,
    required: usize,
    budget: usize,
) -> Result<(), String> {
    let maximum = budget / size_of::<f32>();
    if required > maximum {
        return Err("解码后的音频超过试听PCM预算（256 MiB）".into());
    }
    if samples.capacity() < required {
        // 允许有界倍增减少长音乐的重复分配，但reserve_exact绝不把容量扩到结果预算之外。
        let capacity = samples
            .capacity()
            .saturating_mul(2)
            .max(required)
            .min(maximum);
        samples
            .try_reserve_exact(capacity - samples.len())
            .map_err(|_| "无法为音频试听分配PCM内存".to_string())?;
    }
    Ok(())
}

/// WAV流结束与被截断同用UnexpectedEof；先验证RIFF物理界限和data帧整除，再交成熟解码器解析。
fn validate_wav_bounds(bytes: &[u8], cancel: &AtomicBool) -> Result<bool, String> {
    if !bytes.starts_with(b"RIFF") || bytes.get(8..12) != Some(b"WAVE".as_slice()) {
        return Ok(false);
    }
    let declared = u32::from_le_bytes(
        bytes[4..8]
            .try_into()
            .map_err(|_| "WAV文件头被截断".to_string())?,
    ) as usize;
    let end = declared
        .checked_add(8)
        .filter(|end| *end >= 12 && *end <= bytes.len())
        .ok_or_else(|| "WAV文件被截断，实际长度小于RIFF声明".to_string())?;
    let mut offset = 12usize;
    let mut block_align = None;
    let mut data_size = None;
    while offset < end {
        check_cancel(cancel)?;
        let header_end = offset
            .checked_add(8)
            .filter(|header_end| *header_end <= end)
            .ok_or_else(|| "WAV分块头被截断".to_string())?;
        let length = u32::from_le_bytes(bytes[offset + 4..header_end].try_into().unwrap()) as usize;
        let chunk_end = header_end
            .checked_add(length)
            .filter(|chunk_end| *chunk_end <= end)
            .ok_or_else(|| "WAV分块数据被截断".to_string())?;
        match &bytes[offset..offset + 4] {
            b"fmt " if length >= 16 => {
                let align =
                    u16::from_le_bytes(bytes[header_end + 12..header_end + 14].try_into().unwrap())
                        as usize;
                if align == 0 {
                    return Err("WAV声道帧长度无效".into());
                }
                block_align = Some(align);
            }
            b"data" => data_size = Some(length),
            _ => (),
        }
        offset = chunk_end
            .checked_add(length % 2)
            .filter(|offset| *offset <= end)
            .ok_or_else(|| "WAV分块填充被截断".to_string())?;
    }
    if let (Some(align), Some(length)) = (block_align, data_size)
        && length % align != 0
    {
        return Err("WAV数据没有完整声道帧".into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, values: &[i16]) -> Vec<u8> {
        let data_bytes = (values.len() * 2) as u32;
        let mut result = Vec::new();
        result.extend_from_slice(b"RIFF");
        result.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        result.extend_from_slice(b"WAVEfmt ");
        result.extend_from_slice(&16u32.to_le_bytes());
        result.extend_from_slice(&1u16.to_le_bytes());
        result.extend_from_slice(&channels.to_le_bytes());
        result.extend_from_slice(&rate.to_le_bytes());
        result.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        result.extend_from_slice(&(channels * 2).to_le_bytes());
        result.extend_from_slice(&16u16.to_le_bytes());
        result.extend_from_slice(b"data");
        result.extend_from_slice(&data_bytes.to_le_bytes());
        for value in values {
            result.extend_from_slice(&value.to_le_bytes());
        }
        result
    }

    #[test]
    fn audio_decode_stereo_wav_preserves_samples_and_exact_duration() {
        let bytes = wav(2, 8, &[-32768, 32767, 0, 16384, -16384, 8192, 2048, -2048]);
        let data = decode(bytes, "wav", &AtomicBool::new(false)).unwrap();
        assert_eq!(data.channels.get(), 2);
        assert_eq!(data.sample_rate.get(), 8);
        assert_eq!(data.frames(), 4);
        assert_eq!(data.duration(), 0.5);
        for (actual, expected) in data.samples.iter().zip([
            -1.0,
            32767.0 / 32768.0,
            0.0,
            0.5,
            -0.5,
            0.25,
            0.0625,
            -0.0625,
        ]) {
            assert!((actual - expected).abs() < 0.000_001);
        }
    }

    #[test]
    fn audio_decode_bad_empty_truncated_and_incomplete_frames_fail() {
        let cancel = AtomicBool::new(false);
        assert!(
            decode(Vec::new(), "wav", &cancel)
                .unwrap_err()
                .contains("为空")
        );
        assert!(decode(b"broken audio".to_vec(), "ogg", &cancel).is_err());
        let mut truncated = wav(2, 8, &[1, 2, 3, 4]);
        truncated.pop();
        assert!(
            decode(truncated, "wav", &cancel)
                .unwrap_err()
                .contains("截断")
        );
        let incomplete = wav(2, 8, &[1, 2, 3]);
        assert!(
            decode(incomplete, "wav", &cancel)
                .unwrap_err()
                .contains("声道帧")
        );
    }

    #[test]
    fn audio_decode_cancellation_rejects_before_any_result() {
        let cancel = AtomicBool::new(true);
        assert!(
            decode(wav(1, 8, &[1, 2]), "wav", &cancel)
                .unwrap_err()
                .contains("取消")
        );
        let mut samples = Vec::new();
        ensure_pcm_capacity(&mut samples, 8, 32).unwrap();
        assert!(check_cancel(&cancel).is_err());
    }

    #[test]
    fn audio_decode_encoded_packet_and_pcm_budgets_are_independent() {
        let cancel = AtomicBool::new(false);
        let bytes = wav(2, 8, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let encoded = Limits {
            encoded_bytes: bytes.len() - 1,
            ..LIMITS
        };
        assert!(
            decode_with_limits(bytes.clone(), "wav", &cancel, encoded)
                .unwrap_err()
                .contains("读取预算")
        );
        let packet = Limits {
            packet_bytes: 1,
            ..LIMITS
        };
        assert!(
            decode_with_limits(bytes.clone(), "wav", &cancel, packet)
                .unwrap_err()
                .contains("单包")
        );
        let pcm = Limits {
            pcm_bytes: 28,
            ..LIMITS
        };
        assert!(
            decode_with_limits(bytes.clone(), "wav", &cancel, pcm)
                .unwrap_err()
                .contains("PCM预算")
        );
        let exact = Limits {
            pcm_bytes: 32,
            ..LIMITS
        };
        assert_eq!(
            decode_with_limits(bytes, "wav", &cancel, exact)
                .unwrap()
                .samples
                .len(),
            8
        );
    }

    #[test]
    fn audio_decode_capacity_never_grows_past_pcm_budget() {
        let mut samples = Vec::new();
        for required in [3, 5, 9, 16] {
            ensure_pcm_capacity(&mut samples, required, 64).unwrap();
            assert!(samples.capacity() <= 16);
            samples.resize(required, 0.0);
        }
        assert!(ensure_pcm_capacity(&mut samples, 17, 64).is_err());
        assert_eq!(samples.len(), 16);
        assert!(checked_sample_count(usize::MAX, 8, 16 * MIB).is_err());
        assert_eq!(checked_sample_count(2, 2, 16).unwrap(), 4);
        assert!(checked_sample_count(3, 2, 16).is_err());
    }

    #[test]
    fn audio_decode_excessive_rate_and_channels_fail_without_partial_pcm() {
        let cancel = AtomicBool::new(false);
        assert!(decode(wav(1, 192_001, &[1, 2]), "wav", &cancel).is_err());
        assert!(decode(wav(9, 8, &[0; 18]), "wav", &cancel).is_err());
        assert!(decode(wav(1, 8, &[]), "wav", &cancel).is_err());
    }

    #[test]
    #[ignore = "需要.local/validation/audio-codecs中固定版本的官方codec验证夹具"]
    fn audio_real_codec_fixtures_decode_to_complete_pcm() {
        use sha2::{Digest, Sha256};
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/validation/audio-codecs");
        let fixtures = [
            (
                "beep3.ogg",
                "ogg",
                21_949,
                "06f580a3c71c946a20cefff75f2dd6b386a33a3c676e554f474aa5130c30728e",
            ),
            (
                "RL.mp3",
                "mp3",
                439_110,
                "e3a70cbef9b07c0324f6131edd6424329daf6301282ed5723139161fd2471105",
            ),
            (
                "audacity16bit_level5.flac",
                "flac",
                144_786,
                "e2b9c657a2c7dd46dd70d49f83c111fc593a7542efc5307ab528d00fe087f801",
            ),
        ];
        for (name, extension, length, digest) in fixtures {
            let path = directory.join(name);
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("缺少真实音频夹具 {}：{error}", path.display()));
            assert_eq!(bytes.len(), length, "{name}不是固定版本夹具");
            let actual_digest = Sha256::digest(&bytes)
                .as_slice()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(actual_digest, digest, "{name}摘要不符");
            let data = decode(bytes, extension, &AtomicBool::new(false))
                .unwrap_or_else(|error| panic!("真实{extension}夹具{name}解码失败：{error}"));
            assert!((1..=8).contains(&data.channels.get()));
            assert!((1_000..=192_000).contains(&data.sample_rate.get()));
            assert!(data.frames() > 0);
            assert_eq!(data.samples.len() % data.channels.get() as usize, 0);
            assert!(data.samples.iter().all(|sample| sample.is_finite()));
            assert!(data.duration().is_finite() && (0.001..120.0).contains(&data.duration()));
            println!(
                "真实音频夹具{name}：{}声道，{}Hz，{}帧，{:.6}秒",
                data.channels.get(),
                data.sample_rate.get(),
                data.frames(),
                data.duration()
            );
        }
    }
}
