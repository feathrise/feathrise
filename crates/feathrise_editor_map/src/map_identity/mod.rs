//! 统一解析当前真正运行的玩法地图身份，供目录、切图与删除共用。

use feathrise_map_content::{ActiveDynamicMap, MapId}; // 动态场景必须从资源还原真实MapId。
use feathrise_scene_core::state::ScenePhase; // 公共阶段只证明地图活动，具体身份仍从当前MapId读取。

/// 返回玩法世界正在使用的稳定MapId；菜单和加载页没有地图身份。
pub(crate) fn active_gameplay_map_id(
    scene: ScenePhase,                  // 当前Bevy正式场景状态。
    active_dynamic: &ActiveDynamicMap, // 共用动态场景中的真实地图身份。
) -> Option<MapId> {
    match scene {
        ScenePhase::Active => active_dynamic.map_id.clone(), // 相同场景枚举下仍保留每张MAP的稳定身份。
        ScenePhase::Idle | ScenePhase::Loading => None,      // 非玩法地图阶段不猜测身份。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 纯身份测试不启动Bevy App。

    #[test]
    fn dynamic_scene_uses_the_real_map_id_instead_of_the_shared_scene_enum() {
        let map_id = MapId::parse("MAP-test").unwrap(); // 模拟共用动态场景中的某张编辑器地图。
        let active = ActiveDynamicMap {
            map_id: Some(map_id.clone()), // 资源保存真正当前MapId。
        };
        assert_eq!(
            active_gameplay_map_id(ScenePhase::Active, &active), // 用共用场景和资源联合解析。
            Some(map_id), // 结果必须是具体MAP而不是一个泛化场景标签。
        );
    }
}
