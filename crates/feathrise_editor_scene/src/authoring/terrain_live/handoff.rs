//! 保存后运行时 GLB 派生交接的真实时间截止。

use std::time::Duration;

const TERRAIN_FILE_HANDOFF_TIMEOUT: Duration = Duration::from_secs(30);

/// 即使文件事件丢失或派生阶段不收敛，也不能永久锁住F12作者会话。
pub(super) fn terrain_file_handoff_timed_out(started_at: Duration, now: Duration) -> bool {
    now.saturating_sub(started_at) >= TERRAIN_FILE_HANDOFF_TIMEOUT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_timeout_uses_real_elapsed_time() {
        let started_at = Duration::from_secs(12);
        assert!(!terrain_file_handoff_timed_out(
            started_at,
            Duration::from_secs(41)
        ));
        assert!(terrain_file_handoff_timed_out(
            started_at,
            Duration::from_secs(42)
        ));
    }
}
