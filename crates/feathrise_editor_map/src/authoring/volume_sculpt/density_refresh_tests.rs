//! 二维草窗快刷须等待真实有界索引完成；洞穴继承和画布切换继续使用完整来源。
use super::*;

#[test]
fn density_refresh_waits_for_complete_index_and_rejects_volume_inheritance_or_retarget() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let ordinary: feathrise_map_content::MapPlacementRecord = serde_json::from_value(serde_json::json!({
        "id":"ordinary", "asset_path":"models/ordinary.glb", "translation":[0,0,0],
        "yaw_degrees":0, "scale":[1,1,1]
    }))
    .unwrap();
    draft.placements.placements = (0..=RECORD_SCAN)
        .map(|index| {
            let mut record = ordinary.clone();
            record.id = format!("ordinary-{index}");
            record
        })
        .collect();
    let mut session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    };
    let mut state = VolumeAuthoringState::default();
    assert!(!state.planar_density_refresh_available(&session));
    scan_records(&mut state, &session);
    assert!(
        !state.planar_density_refresh_available(&session),
        "首批没有洞穴不能代表完整地图没有洞穴"
    );
    scan_records(&mut state, &session);
    assert!(state.planar_density_refresh_available(&session));

    let volume = serde_json::from_value(serde_json::json!({
        "id":"cave", "asset_path":"maps/test/cave.glb", "translation":[0,0,0],
        "yaw_degrees":0, "scale":[1,1,1],
        "terrain_patch":{"sculpted_volume":true,"cutout":{"min":[2,2],"max":[4,4]}}
    }))
    .unwrap();
    session
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements
        .push(volume);
    session.placement_structure_revision += 1;
    assert!(
        !state.planar_density_refresh_available(&session),
        "摆放换代立即撤销旧空索引的快刷资格"
    );
    scan_records(&mut state, &session);
    scan_records(&mut state, &session);
    assert!(state.index.ready && !state.index.records.is_empty());
    assert!(
        !state.planar_density_refresh_available(&session),
        "三维表面必须重新采用最新继承密度"
    );

    session.draft.as_mut().unwrap().placements.placements.pop();
    session.placement_structure_revision += 1;
    scan_records(&mut state, &session);
    scan_records(&mut state, &session);
    assert!(state.planar_density_refresh_available(&session));
    state.retarget_terrain_canvas();
    assert!(
        !state.planar_density_refresh_available(&session),
        "同地图切画布不能沿用上一网格索引"
    );
}
