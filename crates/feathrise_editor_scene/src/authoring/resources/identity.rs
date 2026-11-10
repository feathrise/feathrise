//! 作者新对象ID的常数时间分配器；旧文件身份和引用不迁移，载入时也不扫描对象表。

use bevy::asset::uuid::Uuid;

pub(crate) struct AuthorIdSource {
    namespace: Uuid,
    serial: u64,
}

impl Default for AuthorIdSource {
    fn default() -> Self {
        Self {
            namespace: Uuid::new_v4(),
            serial: 0,
        }
    }
}

impl AuthorIdSource {
    pub(crate) fn next(&mut self, prefix: &str) -> String {
        // 同一编辑器内反复打开场景会重置显示序号；此计数不随场景和撤销栈重置。
        // 计数耗尽时切换完整随机命名空间，不能用wrapping_add重发旧身份。
        if self.serial == u64::MAX {
            *self = Self::default();
        }
        self.serial += 1;
        format!("{prefix}-{}-{}", self.namespace.simple(), self.serial)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_identity_counter_exhaustion_rotates_namespace() {
        let original = Uuid::from_u128(17);
        let mut source = AuthorIdSource {
            namespace: original,
            serial: u64::MAX - 1,
        };
        assert_eq!(
            source.next("placement"),
            format!("placement-{}-{}", original.simple(), u64::MAX)
        );
        let next = source.next("placement");
        assert_ne!(source.namespace, original);
        assert_eq!(source.serial, 1);
        assert_eq!(next, format!("placement-{}-1", source.namespace.simple()));
    }
}
