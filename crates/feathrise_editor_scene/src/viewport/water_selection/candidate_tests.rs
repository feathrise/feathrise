//! 水域候选高亮：预览临时覆盖、跨类别隐藏与移出恢复，不改正式选择及双击序列。
use super::*; // 与生产函数共享身份解析。

#[test]
fn water_candidate_hover_restores_confirmed_layer_without_mutating_clicks() {
    let map = SceneId("water-candidate-hover".to_owned()); // 固定场景身份。
    let mut selection = SelectedWaterViewportState::default(); // 原有水域选择缓存。
    selection.select_from_viewport(&map, WaterAuthoringFeatureKind::StillWater, "lake", 1.0); // 第一次单击。
    selection.select_from_viewport(&map, WaterAuthoringFeatureKind::StillWater, "lake", 1.1); // 登记正式水域定位。
    let focus = selection.focus_request.clone(); // 冻结正式定位身份。
    let mut candidates = LayerCandidateState::default(); // 统一候选状态。
    candidates.selected = Some(LayerTarget::Water(
        WaterAuthoringFeatureKind::StillWater,
        "lake".to_owned(),
    )); // 正式选中湖。
    candidates.hover = Some(LayerTarget::Water(
        WaterAuthoringFeatureKind::River,
        "river".to_owned(),
    )); // 临时查看河流。
    assert_eq!(
        candidate_water_feature(&map, &selection, Some(&candidates)),
        Some((WaterAuthoringFeatureKind::River, "river"))
    ); // 高亮临时水体。
    candidates.hover = Some(LayerTarget::Surface("soil".to_owned())); // 鼠标移入地表候选。
    assert_eq!(
        candidate_water_feature(&map, &selection, Some(&candidates)),
        None
    ); // 旧水域覆盖必须隐藏。
    candidates.hover = None; // 移出列表。
    assert_eq!(
        candidate_water_feature(&map, &selection, Some(&candidates)),
        Some((WaterAuthoringFeatureKind::StillWater, "lake"))
    ); // 恢复正式湖面。
    assert_eq!(
        selection.viewport_feature(&map),
        Some((WaterAuthoringFeatureKind::StillWater, "lake"))
    ); // 正式选择未改变。
    assert_eq!(selection.focus_request, focus); // 预览解析不能改定位票据。
}

#[test]
fn explicit_focus_uses_confirmed_water_and_next_click_restores_hover() {
    let map = SceneId("water-explicit-focus".to_owned());
    let mut selection = SelectedWaterViewportState::default();
    let mut candidates = LayerCandidateState::default();
    candidates.selected = Some(LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, "lake".into()));
    candidates.hover = Some(LayerTarget::Water(WaterAuthoringFeatureKind::River, "river".into()));
    selection.request_focus(&map, WaterAuthoringFeatureKind::StillWater, "lake");
    assert_eq!(candidate_water_feature(&map, &selection, Some(&candidates)), Some((WaterAuthoringFeatureKind::StillWater, "lake"))); // F明确取景正式选择，不能被悬停换成河流。
    selection.register_water_click(&map, WaterAuthoringFeatureKind::StillWater, "lake", 1.0);
    assert_eq!(candidate_water_feature(&map, &selection, Some(&candidates)), Some((WaterAuthoringFeatureKind::River, "river"))); // 下次普通点选恢复悬停预览。
    assert!(!selection.explicit_focus_pending());
}

#[test]
fn unified_empty_selection_does_not_revive_legacy_water_selection() {
    let map = SceneId("water-candidate-empty".to_owned()); // 固定场景身份。
    let mut selection = SelectedWaterViewportState::default(); // 保留旧直选用于验证隔离。
    selection.select_from_viewport(&map, WaterAuthoringFeatureKind::Waterfall, "fall", 1.0); // 旧瀑布身份。
    let candidates = LayerCandidateState::default(); // 已清空统一正式选择。
    assert_eq!(
        candidate_water_feature(&map, &selection, Some(&candidates)),
        None
    ); // 不得回退旧瀑布。
    assert_eq!(
        candidate_water_feature(&map, &selection, None),
        Some((WaterAuthoringFeatureKind::Waterfall, "fall"))
    ); // 没安装候选系统的旧场景仍兼容。
}
