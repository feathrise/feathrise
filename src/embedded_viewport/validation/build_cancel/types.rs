//! 停止验收的非ECS证据类型；持有精确进程句柄，避免PID复用影响退出判断。

use serde_json::Value;
use std::os::windows::io::OwnedHandle;

pub(super) struct BuildWitness {
    pub handle: OwnedHandle,
    pub proof: Value,
}
