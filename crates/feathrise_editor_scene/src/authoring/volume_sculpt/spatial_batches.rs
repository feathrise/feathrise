//! 旧直线与新路径共享局部批次接口，进度不会变成累计驻留量。
use super::types::SpatialBatches;
use feathrise_terrain::volume::{VolumeBrushBatch, VolumeBrushBatches};
impl From<VolumeBrushBatches> for SpatialBatches {
    fn from(value: VolumeBrushBatches) -> Self {
        Self::Line(value)
    }
}
impl SpatialBatches {
    pub(super) fn is_path(&self) -> bool {
        matches!(self, Self::Path(_))
    }
    pub(super) fn next_batch(&mut self) -> Result<Option<VolumeBrushBatch>, String> {
        match self {
            Self::Line(v) => v.next_batch(),
            Self::Path(v) => v.next_batch(),
        }
    }
    pub(super) fn finished(&self) -> bool {
        match self {
            Self::Line(v) => v.finished(),
            Self::Path(v) => v.finished(),
        }
    }
    pub(super) fn completed_stamps(&self) -> u64 {
        match self {
            Self::Line(v) => u64::from(v.completed_stamps()),
            Self::Path(v) => v.completed_stamps(),
        }
    }
    pub(super) fn total_stamps(&self) -> u64 {
        match self {
            Self::Line(v) => u64::from(v.total_stamps()),
            Self::Path(v) => v.total_stamps(),
        }
    }
}
