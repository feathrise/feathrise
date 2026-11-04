//! 院落整批作者历史的纯预检与原子摆放记录采用。
//!
//! 【功能设计】先验证整组稳定身份、下标、地图范围和水绑定总数，再一次修改内存数组；
//! 返回每个成员的画面同步意图，由上层在数据采用后定向更新活动根与未保存预览。

use std::collections::HashSet; // 固定最多256个成员的下标和稳定ID必须在写入前唯一。

use bevy::prelude::Rect; // 每个历史快照沿用正式地图世界范围校验。
use feathrise_map_content::{
    MAX_MAP_PLACEMENTS_PER_MAP, MapId, MapPlacementRecord,
}; // 历史方向都服从正式摆放数量、网格水预算和变换硬门。

use crate::authoring::commands::CourtyardBatchChange; // 同一命令保存全部新增和原位修改的成员。

/// 一条完整院落历史命令采用后应作用于当前世界根的目标状态。
pub(super) struct CourtyardWorldChange {
    pub(super) index: usize, // 可见结果须仍占据同一槽，撤销新增须确认该槽不再属于此ID。
    pub(super) record: MapPlacementRecord, // 隐藏或恢复均使用命令冻结的完整成员记录。
    pub(super) visible: bool, // 撤销新增隐藏，重做新增与双向原位修改显示。
}

/// 预检整批后一次应用placements数组与缓存计数；错误时两者均保持原值。
#[cfg(test)]
pub(super) fn apply_courtyard_batch_records(
    records: &mut Vec<MapPlacementRecord>, // 当前草稿唯一摆放数组。
    water_surface_count: &mut usize,       // 与数组同事务维护的全图水绑定缓存。
    change: &CourtyardBatchChange,         // 不会在撤销或重做过程中修改的完整命令。
    undoing: bool,                         // true移除追加尾段并恢复before；false追加并采用after。
    map_id: &MapId,                        // 受检历史快照所属地图。
    world_bounds: Rect,                    // 当前清单已确认的世界XZ范围。
) -> Result<Vec<CourtyardWorldChange>, String> {
    apply_courtyard_batch_records_by_source(records, water_surface_count, change, undoing, map_id, world_bounds, None)
}

/// 显式地形补丁按各自源范围恢复；历史仍保存完整局部记录，不把源坐标误判为容器世界坐标。
pub(super) fn apply_courtyard_batch_records_by_source(
    records: &mut Vec<MapPlacementRecord>,
    water_surface_count: &mut usize,
    change: &CourtyardBatchChange,
    undoing: bool,
    map_id: &MapId,
    world_bounds: Rect,
    objects: Option<&[feathrise_map_content::MapTerrainObject]>,
) -> Result<Vec<CourtyardWorldChange>, String> {
    if !change.removed.is_empty() {
        return apply_removed_records(records, water_surface_count, change, undoing, map_id, world_bounds, objects);
    }
    let base_len = if undoing {
        records
            .len()
            .checked_sub(change.added.len())
            .ok_or_else(|| "院落新增尾段已不存在，拒绝撤销".to_owned())? // 撤销时尾段必须完整存在。
    } else {
        records.len() // 重做前新增成员尚未进入数组。
    };
    if base_len.saturating_add(change.added.len()) > MAX_MAP_PLACEMENTS_PER_MAP {
        return Err("院落整批历史超过单图显式摆放上限".to_owned()); // 历史重做不能绕过正式8192项硬门。
    }
    let mut added_ids = HashSet::with_capacity(change.added.len()); // 一次落印内成员各有独立placement ID。
    for (offset, added) in change.added.iter().enumerate() {
        if added.id.is_empty() || !added_ids.insert(added.id.as_str()) {
            return Err("院落整批历史包含空白或重复的新增实例 ID".to_owned()); // 无法安全区分预览根的命令拒绝应用。
        }
        added.validated_transform(map_id, super::super::placement_record_bounds_by_source(world_bounds, objects, added)?)?;
        added.validate_water_surfaces()?; // 新增记录可携带Scene0网格水，重做前仍需确认格式。
        if undoing && records.get(base_len + offset) != Some(added) {
            return Err(format!("院落新增成员{}的完整快照已改变", added.id)); // 任一尾段字段失配时不弹出任何成员。
        }
    }
    if records[..base_len]
        .iter()
        .any(|record| added_ids.contains(record.id.as_str()))
    {
        return Err("院落新增成员 ID 已被其他摆放占用".to_owned()); // 重做与撤销都不能让同一ID同时指向前缀和尾段。
    }
    let mut changed_indices = HashSet::with_capacity(change.changed.len()); // 防止同一数组槽在单笔内被覆盖两次。
    for (index, before, after) in &change.changed {
        if *index >= base_len || !changed_indices.insert(*index) || before.id != after.id {
            return Err("院落原位修改的下标或稳定 ID 无效".to_owned()); // 完整预检后才允许任何记录变化。
        }
        let expected = if undoing { after } else { before }; // 当前数组必须与命令方向相符。
        let desired = if undoing { before } else { after }; // 所有成员共同采用目标方向。
        if records.get(*index) != Some(expected) {
            return Err(format!("院落成员{}的完整快照或下标已失效", expected.id)); // 单成员后续编辑不能被旧批量历史覆盖。
        }
        desired.validated_transform(map_id, super::super::placement_record_bounds_by_source(world_bounds, objects, desired)?)?;
    }
    let mut next_water_count = *water_surface_count; // 先计算全部差值，不提前修改缓存。
    for added in &change.added {
        next_water_count = if undoing {
            next_water_count.checked_sub(added.water_surfaces.len()) // 撤销落印移除全部新增绑定。
        } else {
            next_water_count.checked_add(added.water_surfaces.len()) // 重做落印恢复全部新增绑定。
        }
        .ok_or_else(|| "院落整批历史的水绑定计数溢出或不足".to_owned())?;
    }
    for (index, before, after) in &change.changed {
        let desired = if undoing { before } else { after }; // 原位修改按完整目标快照维护缓存。
        next_water_count = next_water_count
            .checked_sub(records[*index].water_surfaces.len())
            .and_then(|count| count.checked_add(desired.water_surfaces.len()))
            .ok_or_else(|| "院落成员的水绑定计数溢出或不足".to_owned())?; // 损坏计数不能静默饱和后进入保存。
    }
    let mut world_changes = Vec::with_capacity(change.added.len() + change.changed.len()); // 至多256个定向根同步意图。
    for (index, before, after) in &change.changed {
        let desired = if undoing { before } else { after }; // 与预检完全相同的方向选择。
        records[*index] = desired.clone(); // 前后值已全部通过预检，不会留下半组更新。
        world_changes.push(CourtyardWorldChange {
            index: *index,
            record: desired.clone(),
            visible: true,
        }); // 已有成员只更新同一根。
    }
    if undoing {
        records.truncate(base_len); // 全部新增成员组成经过尾段校验的连续后缀。
    } else {
        records.extend(change.added.iter().cloned()); // 完整重做保持原成员顺序和稳定ID。
    }
    for (offset, added) in change.added.iter().enumerate() {
        world_changes.push(CourtyardWorldChange {
            index: base_len + offset,
            record: added.clone(),
            visible: !undoing,
        }); // 尾段原槽供迟到同步复核。
    }
    *water_surface_count = next_water_count; // 数组与全图水预算同一成功分支发布。
    Ok(world_changes) // ECS更新发生在上层同一命令采用后，不影响数据原子性。
}

/// 全量身份和容量检查先于写入；正向删除与反向恢复均只访问选中的有限槽位。
fn apply_removed_records(
    records: &mut Vec<MapPlacementRecord>, water_count: &mut usize,
    change: &CourtyardBatchChange, undoing: bool, map: &MapId, bounds: Rect,
    objects: Option<&[feathrise_map_content::MapTerrainObject]>,
) -> Result<Vec<CourtyardWorldChange>, String> {
    let original_len = change.original_len.ok_or("批量删除历史缺少原数组长度")?;
    if change.removed.windows(2).any(|pair| pair[0].0 <= pair[1].0)
        || change.removed.iter().any(|(index, _)| *index >= original_len) {
        return Err("批量删除历史的槽位顺序无效".into());
    }
    let expected_len = if undoing { original_len.checked_sub(change.removed.len()).ok_or("删除历史长度无效")? } else { original_len };
    if records.len() != expected_len || !change.added.is_empty() || !change.changed.is_empty() {
        return Err("批量删除的结构身份已变化".into());
    }
    let mut count = *water_count;
    for (index, record) in &change.removed {
        record.validated_transform(map, super::super::placement_record_bounds_by_source(bounds, objects, record)?)?;
        record.validate_water_surfaces()?;
        if !undoing && records.get(*index) != Some(record) {
            return Err(format!("待删除对象{}已经变化", record.id));
        }
        count = if undoing { count.checked_add(record.water_surfaces.len()) } else { count.checked_sub(record.water_surfaces.len()) }
            .ok_or("批量删除的网格水计数无效")?;
    }
    if original_len > MAX_MAP_PLACEMENTS_PER_MAP {
        return Err("恢复整组将超过地图容量".into());
    }
    if undoing {
        let mut length = records.len();
        for (index, _) in change.removed.iter().rev() {
            if *index > length { return Err("批量删除的恢复槽位无效".into()); }
            length += 1;
        }
        for (index, record) in change.removed.iter().rev() {
            if *index == records.len() { records.push(record.clone()); }
            else {
                let displaced = std::mem::replace(&mut records[*index], record.clone());
                records.push(displaced); // 恢复原尾项与原记录顺序，不扫描剩余对象。
            }
        }
    } else {
        for (index, _) in &change.removed { records.swap_remove(*index); }
    }
    *water_count = count;
    Ok(change.removed.iter().map(|(index, record)| CourtyardWorldChange { index: *index, record: record.clone(), visible: undoing }).collect())
}

#[cfg(test)] // 无ECS的纯测试验证整组原子性和双向成员恢复。
mod tests {
    use bevy::prelude::Vec2; // 测试世界范围只使用简单矩形。
    use feathrise_map_content::{MapPlacementCourtyardMembership, MapPlacementHeightMode}; // 真实地图记录与院落身份共同往返。

    use super::*; // 直接测试纯记录事务和世界同步意图。

    fn member(id: &str, member_id: &str, x: f32) -> MapPlacementRecord {
        MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: id.to_owned(), // 每条实例拥有独立的地图稳定ID。
            asset_path: "gltf/house.glb".to_owned(), // 无ECS测试不加载真实文件。
            asset_target: Default::default(), // 普通Scene0路径仍是院落有效成员。
            building_preset_id: None, // 测试组合事务本身，不依赖建筑目录。
            courtyard: Some(MapPlacementCourtyardMembership {
                instance_id: "courtyard-copy-1".to_owned(), // 两成员共享一次落印身份。
                preset_id: "courtyard-test".to_owned(),     // 模板引用不随成员变换更改。
                member_id: member_id.to_owned(),            // 每个成员可按稳定ID定位。
                origin: [0.0, 0.0, 0.0],                    // 整体锚点与成员平移分开保存。
                yaw_degrees: 0.0,                           // 初始落印朝向为世界正向。
            }),
            translation: [x, 0.0, 0.0], // X区分两个成员的独立位置。
            height_mode: MapPlacementHeightMode::FixedHeight, // 快照不依赖地形重采样。
            terrain_offset: 0.0,        // 固定高度无离地偏移。
            yaw_degrees: 0.0,           // 成员本身仍保留独立朝向。
            scale: [1.0, 1.0, 1.0],     // 合法单位缩放。
            water_surfaces: Vec::new(), // 纯组合测试不附带网格水覆盖。
        }
    }

    fn map_and_bounds() -> (MapId, Rect) {
        (
            MapId::parse("MAP-courtyard-history").expect("测试地图身份合法"), // 受检记录归属同一地图。
            Rect::from_corners(Vec2::splat(-20.0), Vec2::splat(20.0)), // 全部测试落点在清单范围内。
        )
    }

    #[test]
    fn terrain_patch_batch_history_uses_source_bounds_for_add_remove_and_restore() {
        let (map, container_bounds) = map_and_bounds();
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = feathrise_map_content::TerrainId::parse("patch-parent").unwrap();
        object.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
        object.world_min = [100.0; 2];
        object.world_max = [200.0; 2];
        let patch: MapPlacementRecord = serde_json::from_value(serde_json::json!({
            "id":"source-local-cave", "asset_path":"gltf/cave.glb", "translation":[150,3,150],
            "yaw_degrees":0.0, "scale":[1.0,1.0,1.0],
            "terrain_patch":{"terrain_id":"patch-parent", "cutout":{"min":[-2,-2],"max":[2,2]}}
        })).unwrap();
        assert!(patch.validated_transform(&map, container_bounds).is_err());
        draft.manifest.terrains = Some(std::sync::Arc::from([object.clone()]));
        assert_eq!(super::super::super::placement_record_bounds(&draft, &patch).unwrap(),
            Rect::from_corners(Vec2::splat(100.0), Vec2::splat(200.0)));
        let added = CourtyardBatchChange::new(vec![patch.clone()], Vec::new()).unwrap();
        let mut records = Vec::new();
        let mut count = 0;
        apply_courtyard_batch_records_by_source(&mut records, &mut count, &added, false,
            &map, container_bounds, Some(std::slice::from_ref(&object))).unwrap();
        assert_eq!(records, vec![patch.clone()]);
        apply_courtyard_batch_records_by_source(&mut records, &mut count, &added, true,
            &map, container_bounds, Some(std::slice::from_ref(&object))).unwrap();
        assert!(records.is_empty());
        records.push(patch.clone());
        let removed = CourtyardBatchChange::new_removed(vec![(0, patch.clone())], 1).unwrap();
        apply_courtyard_batch_records_by_source(&mut records, &mut count, &removed, false,
            &map, container_bounds, Some(std::slice::from_ref(&object))).unwrap();
        assert!(records.is_empty());
        apply_courtyard_batch_records_by_source(&mut records, &mut count, &removed, true,
            &map, container_bounds, Some(std::slice::from_ref(&object))).unwrap();
        assert_eq!(records, vec![patch]); // ID、源局部TRS和完整洞口随同一历史恢复，容器范围不截断。
    }

    #[test]
    fn courtyard_batch_undo_redo_restores_all_members_as_one_operation() {
        let before = member("house", "house", 0.0); // 已有房屋原位置。
        let mut after = before.clone(); // 整组移动后复用同一placement ID。
        after.translation[0] = 4.0; // 单项局部变换进入完整后快照。
        let gate = member("gate", "gate", 8.0); // 同笔新增的院门位于数组末尾。
        let change =
            CourtyardBatchChange::new(vec![gate.clone()], vec![(0, before.clone(), after.clone())])
                .expect("两名成员构成合法整组命令"); // 加入与修改共同属于一次撤销事务。
        let (map_id, bounds) = map_and_bounds(); // 两个方向共享相同地图范围。
        let mut records = vec![after, gate]; // 从命令应用后的草稿状态开始。
        let mut water_count = 0; // 本例没有网格水绑定。
        let undone = apply_courtyard_batch_records(
            &mut records,
            &mut water_count,
            &change,
            true,
            &map_id,
            bounds,
        )
        .expect("整笔撤销应成功"); // 一次恢复已有房屋并移除院门。
        assert_eq!(records.len(), 1); // 不留下半个新增尾段。
        assert_eq!(records[0].translation, before.translation); // 已有房屋回到原位置。
        assert_eq!(undone.len(), 2); // 两条世界根都获得同步意图。
        assert!(!undone[1].visible); // 新增院门在撤销方向必须隐藏。
        let redone = apply_courtyard_batch_records(
            &mut records,
            &mut water_count,
            &change,
            false,
            &map_id,
            bounds,
        )
        .expect("整笔重做应成功"); // 原成员顺序与变换一并恢复。
        assert_eq!(records.len(), 2); // 完整院落重新落印。
        assert_eq!(records[0].translation[0], 4.0); // 整组已有成员使用后快照。
        assert_eq!(records[1].id, "gate"); // 新增成员保持稳定身份和原顺序。
        assert!(redone.iter().all(|update| update.visible)); // 重做两个成员均可见。
    }

    #[test]
    fn courtyard_batch_rejects_stale_index_before_changing_any_member() {
        let before = member("house", "house", 0.0); // 原位置快照。
        let mut after = before.clone(); // 修改后世界位置不同。
        after.translation[0] = 4.0; // 若错误写入可直接从数组观察。
        let gate = member("gate", "gate", 8.0); // 已追加尾段等待撤销。
        let change =
            CourtyardBatchChange::new(vec![gate.clone()], vec![(1, before, after.clone())])
                .expect("命令大小合法，槽位由采用前统一预检"); // 下标1指向新增尾段，不能作为原位修改。
        let (map_id, bounds) = map_and_bounds(); // 坏下标与有效范围互不干扰。
        let mut records = vec![after, gate]; // 当前草稿保持一栋已改房屋与一个院门。
        let mut water_count = 0; // 错误不得改变计数。
        assert!(
            apply_courtyard_batch_records(
                &mut records,
                &mut water_count,
                &change,
                true,
                &map_id,
                bounds
            )
            .is_err()
        ); // 任一成员失配使整笔失败。
        assert_eq!(records.len(), 2); // 新增尾段仍完整存在。
        assert_eq!(records[0].translation[0], 4.0); // 已有房屋没有部分恢复。
        assert_eq!(water_count, 0); // 缓存与数组继续一致。
    }

    #[test]
    fn batch_delete_undo_restores_exact_order_for_every_subset_including_tail() {
        let (map, bounds) = map_and_bounds();
        let original = (0..8).map(|i| member(&format!("record-{i}"), &format!("member-{i}"), i as f32)).collect::<Vec<_>>();
        for mask in 1u16..256 {
            let removed = original.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(i, record)| (i, record.clone())).collect();
            let history = CourtyardBatchChange::new_removed(removed, original.len()).unwrap();
            let mut records = original.clone(); let mut count = 0;
            let deleted = apply_courtyard_batch_records(&mut records, &mut count, &history, false, &map, bounds).unwrap();
            assert_eq!(records.len(), original.len() - mask.count_ones() as usize);
            assert!(deleted.iter().all(|update| !update.visible));
            let after = records.clone();
            let restored = apply_courtyard_batch_records(&mut records, &mut count, &history, true, &map, bounds).unwrap();
            assert_eq!(records, original, "撤销必须恢复完整原顺序，mask={mask}");
            assert!(restored.iter().all(|update| update.visible));
            apply_courtyard_batch_records(&mut records, &mut count, &history, false, &map, bounds).unwrap();
            assert_eq!(records, after, "重做必须重复同一整批结果");
        }
    }

    #[test]
    fn batch_delete_rejects_changed_record_and_water_underflow_before_mutating() {
        let (map, bounds) = map_and_bounds();
        let original = vec![member("a", "a", 1.0), member("b", "b", 2.0), member("c", "c", 3.0)];
        let history = CourtyardBatchChange::new_removed(vec![(0, original[0].clone()), (2, original[2].clone())], 3).unwrap();
        let mut records = original.clone(); records[0].yaw_degrees = 90.0;
        let expected = records.clone(); let mut count = 0;
        assert!(apply_courtyard_batch_records(&mut records, &mut count, &history, false, &map, bounds).is_err());
        assert_eq!(records, expected);
        assert_eq!(count, 0);
        records = original.clone(); records.pop();
        let expected = records.clone();
        assert!(apply_courtyard_batch_records(&mut records, &mut count, &history, false, &map, bounds).is_err());
        assert_eq!(records, expected);
    }
}
