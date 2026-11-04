//! 保存完成帧建立正式水域交接所需的小型身份辅助。

/// 文件写入期间的部分热更不得越过 IO 启动时已经冻结的正式代次。
pub(super) fn water_handoff_generation_baseline(io_start: u64, at_gate_creation: u64) -> u64 {
    io_start.max(at_gate_creation)
}
