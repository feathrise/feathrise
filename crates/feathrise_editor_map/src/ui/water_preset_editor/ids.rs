//! 新建水体预设使用的稳定且不复用目录身份。

use std::time::{SystemTime, UNIX_EPOCH};

use feathrise_map_content::WaterPresetRegistry;

/// 以当前毫秒为可读前缀，并在同毫秒碰撞时追加确定性后缀。
pub(super) fn unique_id(registry: &WaterPresetRegistry) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base = format!("water-{millis}");
    if registry.resolve(&base).is_none() {
        return base;
    }
    (2_u32..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| registry.resolve(candidate).is_none())
        .expect("u32 后缀空间足以形成新水体预设 ID")
}
