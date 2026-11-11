//! 统一解析当前真正运行的玩法场景身份，供目录、切换场景与删除共用。

use feathrise_scene_content::{ActiveDynamicScene, SceneId}; // 动态场景必须从资源还原真实SceneId。
use feathrise_scene_core::state::ScenePhase; // 公共阶段只证明场景活动，具体身份仍从当前SceneId读取。

/// 返回玩法世界正在使用的稳定SceneId；菜单和加载页没有场景身份。
pub(crate) fn active_gameplay_scene_id(
    scene: ScenePhase,                  // 当前Bevy正式场景状态。
    active_dynamic: &ActiveDynamicScene, // 共用动态场景中的真实场景身份。
) -> Option<SceneId> {
    match scene {
        ScenePhase::Active => active_dynamic.scene_id.clone(), // 相同场景枚举下仍保留每张MAP的稳定身份。
        ScenePhase::Idle | ScenePhase::Loading => None,      // 非玩法场景阶段不猜测身份。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 纯身份测试不启动Bevy App。

    #[test]
    fn dynamic_scene_uses_the_real_scene_id_instead_of_the_shared_scene_enum() {
        let scene_id = SceneId::parse("MAP-test").unwrap(); // 模拟共用动态场景中的某张编辑器场景。
        let active = ActiveDynamicScene {
            scene_id: Some(scene_id.clone()), // 资源保存真正当前SceneId。
        };
        assert_eq!(
            active_gameplay_scene_id(ScenePhase::Active, &active), // 用共用场景和资源联合解析。
            Some(scene_id), // 结果必须是具体MAP而不是一个泛化场景标签。
        );
    }
}
