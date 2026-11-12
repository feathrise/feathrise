//! 已核对的动作重定向结果与运行时采样输入。

use bevy::prelude::*;
use crate::media::AssetPreviewModelBounds;

#[derive(Clone, Copy)]
pub struct JointCorrection {
    pub rest: Transform,
    pub before: Quat,
    pub after: Quat,
}

pub struct RigRetarget {
    pub corrections: Vec<Option<JointCorrection>>,
    pub clips: Vec<bool>,
    pub bounds: AssetPreviewModelBounds,
}

impl RigRetarget {
    pub fn supports(&self, clip: usize) -> bool {
        self.clips.get(clip).copied().unwrap_or(false)
    }

    pub fn sample(&self, source: &[Transform]) -> Vec<(usize, Transform)> {
        self.corrections
            .iter()
            .zip(source)
            .enumerate()
            .filter_map(|(index, (joint, source))| {
                let joint = joint.as_ref()?;
                let transform = Transform {
                    rotation: (joint.before * source.rotation * joint.after).normalize(),
                    ..joint.rest // 外部动作只提供转动，位移和缩放保留目标的静止值。
                };
                Some((index, transform))
            })
            .collect()
    }
}
