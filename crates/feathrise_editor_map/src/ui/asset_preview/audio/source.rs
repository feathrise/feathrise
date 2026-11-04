//! 资产试听的共享PCM与无阻塞播放游标；跳转只发原子请求，音频线程在完整声道帧边界采用。

use bevy::{
    audio::{ChannelCount, Decodable, SampleRate, Source},
    prelude::{Asset, TypePath},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Debug)]
pub(super) struct PcmData {
    pub samples: Arc<[f32]>,
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
}

impl PcmData {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.get() as usize
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.get() as f64
    }
}

#[derive(Debug)]
pub(super) struct PlaybackCursor {
    // 0表示无请求，其余值是目标帧+1；快速拖动只保留最新位置，不产生任务或消息队列。
    requested_frame: AtomicUsize,
    published_frame: AtomicUsize,
    ended: AtomicBool,
    total_frames: usize,
}

impl PlaybackCursor {
    pub fn new(data: &PcmData, seconds: f64) -> Arc<Self> {
        let frame = target_frame(data, seconds);
        Arc::new(Self {
            requested_frame: AtomicUsize::new(frame + 1),
            published_frame: AtomicUsize::new(frame),
            ended: AtomicBool::new(frame >= data.frames()),
            total_frames: data.frames(),
        })
    }

    pub fn seek(&self, data: &PcmData, seconds: f64) {
        let frame = target_frame(data, seconds);
        self.requested_frame.store(frame + 1, Ordering::Release);
    }

    pub fn position(&self, data: &PcmData) -> f64 {
        let request = self.requested_frame.load(Ordering::Acquire);
        // 暂停的Source不消费采样；此时立即回显最新意图，不能由旧反馈把滑块拉回。
        let frame = if request != 0 {
            request - 1
        } else {
            self.published_frame.load(Ordering::Acquire)
        };
        frame.min(data.frames()) as f64 / data.sample_rate.get() as f64
    }

    pub fn finished(&self) -> bool {
        let request = self.requested_frame.load(Ordering::Acquire);
        if request != 0 {
            request - 1 >= self.total_frames
        } else {
            self.ended.load(Ordering::Acquire)
        }
    }
}

fn target_frame(data: &PcmData, seconds: f64) -> usize {
    if seconds.is_nan() || seconds <= 0.0 {
        return 0;
    }
    (seconds * data.sample_rate.get() as f64)
        .floor()
        .min(data.frames() as f64) as usize
}

#[derive(Asset, TypePath)]
pub(super) struct PreviewAudio {
    pub data: Arc<PcmData>,
    pub cursor: Arc<PlaybackCursor>,
}

impl Decodable for PreviewAudio {
    type Decoder = PcmSource;

    fn decoder(&self) -> Self::Decoder {
        // 每个sink的游标独立，由宿主建立新代次；这里只克隆Arc，绝不重新解码或复制PCM。
        PcmSource {
            data: self.data.clone(),
            cursor: self.cursor.clone(),
            sample: self.cursor.published_frame.load(Ordering::Acquire)
                * self.data.channels.get() as usize,
        }
    }
}

pub(super) struct PcmSource {
    data: Arc<PcmData>,
    cursor: Arc<PlaybackCursor>,
    sample: usize,
}

impl Iterator for PcmSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let channels = self.data.channels.get() as usize;
        if self.sample % channels == 0 {
            let request = self.cursor.requested_frame.load(Ordering::Acquire);
            if request != 0 {
                let frame = (request - 1).min(self.data.frames());
                self.sample = frame * channels;
                // 先发布目标反馈再清意图，UI不会在清请求之后短暂看到旧位置。
                self.cursor.published_frame.store(frame, Ordering::Release);
                self.cursor.ended.store(false, Ordering::Release);
                let _ = self.cursor.requested_frame.compare_exchange(
                    request,
                    0,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            }
        }
        let Some(value) = self.data.samples.get(self.sample).copied() else {
            self.cursor.ended.store(true, Ordering::Release);
            return None;
        };
        self.sample += 1;
        if self.sample % channels == 0 {
            self.cursor
                .published_frame
                .store(self.sample / channels, Ordering::Release);
            self.cursor
                .ended
                .store(self.sample >= self.data.samples.len(), Ordering::Release);
        }
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // 待处理跳转可增加或减少剩余量，不能对音频队列承诺旧位置的ExactSize。
        (0, None)
    }
}

impl Source for PcmSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.sample >= self.data.samples.len()
            && self.cursor.requested_frame.load(Ordering::Acquire) == 0
        {
            Some(0)
        } else {
            Some(self.data.samples.len())
        }
    }

    fn channels(&self) -> ChannelCount {
        self.data.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.data.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.data.duration()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Arc<PcmData> {
        Arc::new(PcmData {
            samples: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0].into(),
            channels: ChannelCount::new(2).unwrap(),
            sample_rate: SampleRate::new(2).unwrap(),
        })
    }

    fn source(data: &Arc<PcmData>, cursor: &Arc<PlaybackCursor>) -> PcmSource {
        PreviewAudio {
            data: data.clone(),
            cursor: cursor.clone(),
        }
        .decoder()
    }

    #[test]
    fn audio_pcm_seek_preserves_stereo_frame_boundaries() {
        let data = data();
        let cursor = PlaybackCursor::new(&data, 0.0);
        let mut source = source(&data, &cursor);
        assert_eq!(source.next(), Some(1.0));
        cursor.seek(&data, 1.0);
        assert_eq!(cursor.position(&data), 1.0);
        assert_eq!(source.next(), Some(2.0));
        assert_eq!(source.next(), Some(5.0));
        assert_eq!(source.next(), Some(6.0));
        assert!(cursor.finished());
        assert_eq!(cursor.position(&data), 1.5);
        assert_eq!(source.next(), None);
    }

    #[test]
    fn audio_pcm_paused_seek_merges_and_reports_latest_intent() {
        let data = data();
        let cursor = PlaybackCursor::new(&data, 0.0);
        let mut source = source(&data, &cursor);
        // 暂停等同于不调用next；连续拖动只占同一个原子请求槽。
        cursor.seek(&data, 0.5);
        cursor.seek(&data, 1.0);
        assert_eq!(cursor.position(&data), 1.0);
        assert!(!cursor.finished());
        assert_eq!(source.next(), Some(5.0));
        assert_eq!(source.next(), Some(6.0));
    }

    #[test]
    fn audio_pcm_eof_rebuild_uses_shared_data_and_independent_cursor() {
        let data = data();
        let first = PlaybackCursor::new(&data, 0.0);
        let mut old_source = source(&data, &first);
        assert_eq!(old_source.by_ref().count(), 6);
        assert!(first.finished());
        let second = PlaybackCursor::new(&data, 0.5);
        let mut new_source = source(&data, &second);
        assert!(Arc::ptr_eq(&old_source.data, &new_source.data));
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(new_source.next(), Some(3.0));
        assert!(first.finished());
        assert!(!second.finished());
    }

    #[test]
    fn audio_pcm_time_and_out_of_range_seek_are_clamped() {
        let data = data();
        assert_eq!(data.frames(), 3);
        assert_eq!(data.duration(), 1.5);
        let cursor = PlaybackCursor::new(&data, f64::NAN);
        assert_eq!(cursor.position(&data), 0.0);
        cursor.seek(&data, f64::INFINITY);
        assert_eq!(cursor.position(&data), 1.5);
        assert!(cursor.finished());
        cursor.seek(&data, -4.0);
        assert_eq!(cursor.position(&data), 0.0);
        assert!(!cursor.finished());
        let source = source(&data, &cursor);
        assert_eq!(source.total_duration(), Some(Duration::from_millis(1500)));
    }
}
