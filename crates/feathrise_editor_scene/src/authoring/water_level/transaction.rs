//! 静水水位实时拖动的稳定层单字段所有权回滚门。

use feathrise_scene_content::SceneWaterAsset; // 纯函数只定位水域文档中的稳定层。

use super::sync_water_network_connections; // 回滚成功后按当前连接结构重算派生端点。
use crate::authoring::resources::{EditorAuthoringSession, WaterLevelScrubState}; // ECS包装和冻结before/latest。
use crate::types::WaterLevelInputTarget; // 场景与水层联合稳定身份。

/// 只在同场景、同稳定层的水位仍等于事务latest时恢复before。
fn rollback_owned_water_level(
    water: &mut SceneWaterAsset,      // 当前权威水域文档。
    target: &WaterLevelInputTarget, // 拖动起点冻结的场景与层ID。
    before: f32,                    // 拖动前未量化raw水位。
    latest: f32,                    // 事务最后实际写入水位。
) -> bool {
    if water.scene_id != target.scene_id {
        return false; // 切换场景后绝不按同层ID误写新水域。
    }
    let Some(layer) = water
        .layers
        .iter_mut()
        .find(|layer| layer.id == target.layer_id)
    else {
        return false; // 层被删除或换代后不猜测替代层。
    };
    if layer.water_level != latest {
        return false; // 外部已改同水位字段时失去回滚权。
    }
    layer.water_level = before; // 只从事务latest恢复该层水位raw before。
    true // 上层可换代一次真实水面预览。
}

/// 对当前草稿尝试单字段回滚，成功时只换代水预览修订。
pub(super) fn rollback_water_level_scrub_if_owned(
    session: &mut EditorAuthoringSession, // 当前草稿可能已包含外部分支。
    scrub: &WaterLevelScrubState,         // 持有稳定层与before/latest。
) -> bool {
    let Some(target) = scrub.target.as_ref() else {
        return false; // 未建立事务没有回滚对象。
    };
    let rolled_back = {
        let Some(water) = session
            .draft
            .as_mut()
            .and_then(|draft| draft.water.as_mut())
        else {
            return false; // 无当前水域草稿时无副作用返回。
        };
        let rolled_back = rollback_owned_water_level(water, target, scrub.before, scrub.after); // 只在owned latest仍在时写入。
        if rolled_back {
            sync_water_network_connections(water); // 按当前外部连接结构重算派生端点，不恢复旧快照。
        }
        rolled_back // 结束水域可变借用后再推进预览修订。
    };
    if rolled_back {
        session.water_revision = session.water_revision.wrapping_add(1); // 撤销实时水位需要一次single-flight预览换代。
    }
    rolled_back // 不推进全局作者修订或历史。
}

#[cfg(test)]
mod tests {
    use super::*; // 纯所有权门不启动后台水面构建。

    #[test]
    fn rollback_requires_water_level_to_still_equal_owned_latest() {
        let scene_id = feathrise_scene_content::SceneId::parse("MAP-water-level-rollback")
            .expect("测试场景ID合法"); // 稳定场景身份。
        let mut water = crate::authoring::water_asset_with_layer_for_tests(
            &scene_id,
            "scenes/MAP-water-level-rollback",
        ); // 含显式稳定lake层。
        let target = WaterLevelInputTarget {
            scene_id,
            layer_id: "lake".to_owned(),
            authoring_revision: 0,
        }; // 冻结稳定水层。
        water.layers[0].water_level = 1.235; // 模拟显示量化后的事务latest。
        assert!(rollback_owned_water_level(
            &mut water, &target, 1.234_567, 1.235
        )); // changed=false时必须恢复高精度raw。
        assert_eq!(water.layers[0].water_level, 1.234_567); // 精确before保留。
        water.layers[0].water_level = 2.0; // 外部已改同字段。
        assert!(!rollback_owned_water_level(
            &mut water, &target, 1.234_567, 1.235
        )); // 旧事务失去所有权。
        assert_eq!(water.layers[0].water_level, 2.0); // 外部值不被覆盖。
    }
}
