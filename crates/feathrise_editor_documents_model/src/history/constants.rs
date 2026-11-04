//! 历史条数、共享快照保守预算与连续输入合并时间窗。

use std::time::Duration;

pub const MAX_ENTRIES: usize = 128;
pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024; // 每份文档最多8 MiB；八份会话最多64 MiB。
pub const TYPING_WINDOW: Duration = Duration::from_millis(750); // 使用真实时钟，F12暂停玩法不冻结分组。
