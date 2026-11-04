//! 场景显隐的非ECS身份契约；临时预览只持有票据，不取得显隐资源的内部写权限。

use std::sync::Arc;

/// 每次外部显隐Insert都换身份；即使值仍为Hidden，也会撤销旧临时隐藏的归还信用。
#[derive(Clone, Debug)]
pub(crate) struct PlacementVisibilityIntent(Arc<()>);

impl Default for PlacementVisibilityIntent {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}

impl PlacementVisibilityIntent {
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
