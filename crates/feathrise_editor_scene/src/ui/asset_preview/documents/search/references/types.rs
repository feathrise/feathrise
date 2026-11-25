//! 缺失检查保留匹配字段的原值与搜索结果中的精确位置。

use super::super::types::Hit;

pub(in super::super) struct PathReference {
    pub hit: Hit,
    pub path: String,
    pub authored: bool,
}
