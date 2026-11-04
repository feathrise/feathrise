//! 传送图保存发布与作者预览交接；正式新修订Ready前持续保留刚保存的可见预览。

use std::collections::BTreeMap; // 保存批次按稳定传送点ID精确移动对应预览实体。

use bevy::ecs::system::SystemParam; // 保存主系统通过一个参数组读取传送发布三项资源。
use bevy::prelude::*; // Assets、Commands、Entity与Resource等ECS类型。
use feathrise_map_content::{MapContentRegistry, MapId, MapPortalsAsset}; // 正式目录唯一句柄与保存完成快照。
use feathrise_scene::{MapPortalRuntimePhase, MapPortalRuntimeStatus}; // 只有严格新Ready修订才能回收预览。

/// 一次保存后等待正式传送图接替的精确预览批次。
#[derive(Debug)]
struct PendingPortalPreviewHandoff {
    map_id: MapId,                // 迟到的其他地图Ready不能回收当前批次。
    baseline_revision: u64,       // 保存发布前已经存在的正式修订不能冒充新结果。
    expected_portal_count: usize, // 新正式图必须完整包含保存快照中的全部传送点。
    entities: Vec<Entity>,        // 只保存本次会话实际创建的预览实体，不扫描整个World。
}

/// 保存后传送预览的单批交接状态；稳定帧只读取一个结构化运行时状态。
#[derive(Resource, Debug, Default)]
pub(crate) struct PortalPreviewHandoff {
    pending: Option<PendingPortalPreviewHandoff>, // None表示没有等待正式接替的可见预览。
}

/// 保存完成帧使用的传送资产、正式采用证据和预览交接写侧。
#[derive(SystemParam)]
pub(crate) struct PortalSavePublish<'w> {
    pub(super) assets: ResMut<'w, Assets<MapPortalsAsset>>, // 原位替换目录唯一正式传送图资产。
    pub(super) runtime: Res<'w, MapPortalRuntimeStatus>,    // 冻结发布前成功修订作为严格交接基线。
    pub(super) handoff: ResMut<'w, PortalPreviewHandoff>,   // 正式新Ready前继续持有可见作者预览。
}

impl PortalPreviewHandoff {
    /// 在正式资产发布前冻结旧修订，并把当前未保存预览移交给交接资源。
    pub(super) fn begin(
        &mut self,                          // 单一资源持有至多一个传送预览批次。
        map_id: MapId,                      // 本次保存成功的稳定地图身份。
        baseline_revision: u64,             // 发布前正式传送图成功修订。
        expected_portal_count: usize,       // 保存快照中的完整逻辑点数。
        previews: BTreeMap<String, Entity>, // 当前会话按稳定ID登记的精确预览集合。
    ) {
        if previews.is_empty() {
            return; // 没有新预览时无需建立可见交接门，正式资产仍由调用方正常发布。
        }
        let mut entities = self
            .pending
            .take()
            .map(|pending| pending.entities)
            .unwrap_or_default(); // 连续保存时保留上一批尚未被正式图接替的可见实体。
        entities.extend(previews.into_values()); // 新批次实体只移动句柄，不遍历或复制场景层级。
        self.pending = Some(PendingPortalPreviewHandoff {
            map_id,                // 后续只接受这一地图的运行时状态。
            baseline_revision,     // 严格大于此值才能证明保存后的新图已采用。
            expected_portal_count, // 点数不符说明仍是中间或另一份传送图。
            entities,              // 正式Ready前继续显示这些作者预览。
        });
    }

    /// 新正式图完整Ready或玩法已切到另一张Ready地图后，取得需要回收的精确实体批次。
    fn take_cleanup_entities(
        &mut self,                       // 成功时一次性消费pending，禁止重复回收。
        status: &MapPortalRuntimeStatus, // 数据驱动传送运行时公开的原子采用证据。
    ) -> Option<(MapId, Vec<Entity>, bool)> {
        let pending = self.pending.as_ref()?; // 稳定无交接帧保持常数时间返回。
        let adopted = status.phase == MapPortalRuntimePhase::Ready
            && status.map_id.as_ref() == Some(&pending.map_id)
            && status.revision > pending.baseline_revision
            && status.portal_count == pending.expected_portal_count; // 同图、严格新修订和完整点数缺一不可。
        let switched_away = status.phase == MapPortalRuntimePhase::Ready
            && status.map_id.is_some()
            && status.map_id.as_ref() != Some(&pending.map_id); // 切图后旧预览即使已被场景清理也应释放资源身份。
        if !adopted && !switched_away {
            return None; // Waiting或Failed继续保留预览，画面不得先消失。
        }
        let pending = self
            .pending
            .take()
            .expect("刚确认存在的传送预览交接必须可消费");
        Some((pending.map_id, pending.entities, adopted)) // 调用侧在同一命令边界精确回收实体。
    }
}

/// 把已经完整落盘的传送图快照发布到目录唯一强句柄，立即触发正式运行时热更新。
pub(super) fn publish_saved_portals_asset(
    registry: Option<&MapContentRegistry>, // 目录提供本地图唯一正式传送图句柄。
    map_id: &MapId,                        // 只允许更新本次保存成功的地图。
    snapshot: &MapPortalsAsset,            // 与磁盘原子事务完全相同的完整传送图。
    portals: &mut Assets<MapPortalsAsset>, // 主世界资产只能在IO任务完成后由主线程替换。
) -> bool {
    let Some(handle) = registry
        .and_then(|registry| registry.handles(map_id))
        .map(|handles| &handles.portals)
    else {
        return false; // 新地图目录句柄尚未建立时由调用方显式reload正式路径。
    };
    let Some(mut loaded) = portals.get_mut(handle) else {
        return false; // 句柄仍在异步加载时不能创建第二份资产身份。
    };
    *loaded = snapshot.clone(); // AssetMut发布Modified，传送运行时下一次解析会生成完整正式实体。
    true // 已原位发布时禁止再发一次路径reload制造重复传送修订。
}

/// 正式传送图严格采用保存后的新修订后，才回收刚保存的作者预览。
pub(crate) fn cleanup_saved_portal_previews(
    mut commands: Commands,                    // 回收只通过延迟命令修改World。
    mut handoff: ResMut<PortalPreviewHandoff>, // 单批状态成功后一次性清空。
    status: Res<MapPortalRuntimeStatus>,       // 只读正式运行时原子采用证据。
) {
    let Some((map_id, entities, adopted)) = handoff.take_cleanup_entities(&status) else {
        return; // 正式图仍Waiting或Failed时继续显示作者预览，避免保存后消失。
    };
    let entity_count = entities.len(); // 日志只记录固定批次长度，不查询World。
    for entity in entities {
        commands.entity(entity).try_despawn(); // 切图时实体可能已被其他生命周期清理，重复回收保持无告警。
    }
    if adopted {
        info!("地图{map_id}正式传送图已接替并回收{entity_count}个作者预览"); // 明确记录无缝交接成功。
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap; // 构造最小正式目录和精确预览映射。

    use feathrise_map_content::{
        MapCatalogAsset, MapContentHandles, MapManifestAsset, MapPlacementsAsset, MapSurfaceAsset,
        MapVegetationAsset, MapWaterAsset,
    }; // 测试注册表字段保持与生产结构一致。

    use super::*; // 直接验证发布和严格新修订交接门。

    #[test]
    fn saved_portals_replace_the_registered_asset_in_place() {
        let map_id = MapId::parse("MAP-portal-publish").expect("测试地图ID合法"); // 当前保存身份。
        let mut assets = Assets::<MapPortalsAsset>::default(); // 模拟主世界唯一传送资产表。
        let handle = assets.add(MapPortalsAsset {
            schema_version: 0,      // 旧缓存值用于证明实际发生替换。
            map_id: map_id.clone(), // 旧资产仍属于同一地图。
            portals: Vec::new(),    // 测试无需构造跨地图目标。
        });
        let registry = MapContentRegistry {
            catalog: Handle::<MapCatalogAsset>::default(), // 本测试不读取目录资产本体。
            revision: 1,                                   // 目录已经完成一次采用。
            maps: BTreeMap::from([(
                map_id.clone(), // 唯一地图映射到上方同一传送强句柄。
                MapContentHandles {
                    manifest: Handle::<MapManifestAsset>::default(), // 测试不读取清单。
                    portals: handle.clone(), // 被测函数必须原位替换这一句柄。
                    placements: Handle::<MapPlacementsAsset>::default(), // 测试不读取摆放。
                    surface: None::<Handle<MapSurfaceAsset>>, // 测试不包含地表。
                    vegetation: None::<Handle<MapVegetationAsset>>, // 测试不包含植被。
                    water: None::<Handle<MapWaterAsset>>, // 测试不包含水域。
                    characters_path: None,        // 旧测试地图没有角色作者源。
                    boundaries: None,        // 测试不包含边界。
                },
            )]),
            last_error: None, // 目录当前有效。
        };
        let saved = MapPortalsAsset {
            schema_version: 1,      // 保存后的格式版本与旧缓存不同。
            map_id: map_id.clone(), // 快照身份必须与目录目标一致。
            portals: Vec::new(),    // 空图也必须能够正式发布。
        };

        assert!(publish_saved_portals_asset(
            Some(&registry), // 使用目录唯一句柄。
            &map_id,         // 只替换当前保存地图。
            &saved,          // 模拟IO已经完整落盘的快照。
            &mut assets,     // 主世界资产表原位修改。
        ));
        assert_eq!(
            assets
                .get(&handle)
                .expect("正式传送句柄仍存在")
                .schema_version,
            1
        ); // 相同Handle现在必须读取到保存后的新快照。
    }

    #[test]
    fn preview_handoff_waits_for_a_strictly_new_complete_ready_revision() {
        let map_id = MapId::parse("MAP-portal-handoff").expect("测试地图ID合法"); // 交接目标地图。
        let mut handoff = PortalPreviewHandoff::default(); // 初始没有等待批次。
        handoff.begin(
            map_id.clone(),                                                    // 本次保存地图。
            7,                                                                 // 保存前正式修订。
            3, // 保存后应有三个逻辑传送点。
            BTreeMap::from([("portal-new".to_owned(), Entity::from_bits(1))]), // 精确新预览实体。
        );
        let mut status = MapPortalRuntimeStatus {
            phase: MapPortalRuntimePhase::Ready, // 旧正式图本身已经Ready。
            map_id: Some(map_id.clone()),        // 同一地图仍不能绕过修订门。
            revision: 7,                         // 尚未严格越过保存基线。
            portal_count: 3,                     // 即使点数相同也不能冒充新采用。
            detail: String::new(),               // 测试不依赖诊断文本。
        };
        assert!(handoff.take_cleanup_entities(&status).is_none()); // 旧Ready必须继续保留预览。
        status.revision = 8; // 模拟正式运行时消费保存后的Modified事件。
        status.portal_count = 2; // 不完整点数不能回收作者可见兜底。
        assert!(handoff.take_cleanup_entities(&status).is_none()); // 半套正式图不得制造消失窗口。
        status.portal_count = 3; // 同图、严格新修订且完整点数现在全部满足。
        let (_, entities, adopted) = handoff
            .take_cleanup_entities(&status)
            .expect("完整新正式图应接替作者预览");
        assert!(adopted); // 本次清理来自正式采用而不是切图兜底。
        assert_eq!(entities, vec![Entity::from_bits(1)]); // 只回收冻结的精确预览实体。
    }
}
