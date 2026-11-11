//! 水域对象列表的纯值分页、格式化与稳定ID选择逻辑。

use feathrise_scene_content::{SceneId, SceneWaterAsset}; // 三类水域作者对象共享的正式描述与当前场景身份。

use crate::resources::SceneEditorState; // 纯选择只修改F12会话索引、页码和工具。
use crate::types::{EditorTool, WaterAuthoringFeatureKind}; // 行类别映射到既有三类作者工具。
use crate::viewport::SelectedWaterViewportState; // “查看”工具直接点选也必须高亮同一固定列表行。

use super::WATER_FEATURE_ROW_COUNT; // 选择后自动把固定行池翻到目标所在页。

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct WaterSourceSelectionGuard<'w> {
    idle: crate::authoring::terrain_documents::CanvasIdle<'w>,
    volume: Option<bevy::prelude::ResMut<'w, crate::authoring::volume_sculpt::VolumeAuthoringState>>,
    numeric: Option<bevy::prelude::Res<'w, feathrise_editor_ui::ActiveNumericInput>>,
    scrub: Option<bevy::prelude::Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
}

impl WaterSourceSelectionGuard<'_> {
    pub(crate) fn busy(&self, session: &crate::authoring::EditorAuthoringSession) -> bool {
        self.idle.busy(session) || self.volume.as_ref().is_some_and(|volume| volume.busy())
            || self.numeric.as_ref().is_some_and(|numeric| numeric.entity.is_some())
            || self.scrub.as_ref().is_some_and(|scrub| scrub.entity.is_some())
    }

    pub(crate) fn retarget_terrain_canvas(&mut self) {
        if let Some(volume) = self.volume.as_deref_mut() { volume.retarget_terrain_canvas(); }
    } // 仅成功换槽后丢弃可重建拾取索引；洞穴文件、历史及失败路径继续保留。
}

/// 根水在网格检查器激活期间仍是场景内容；固定行按来源和类内索引解引用。
pub(crate) fn listed_water_feature_at(
    draft: &crate::authoring::EditorSceneDraft,
    mut flat_index: usize,
) -> Option<(Option<&feathrise_scene_content::TerrainId>, &SceneWaterAsset, WaterAuthoringFeatureKind, usize, &str)> {
    for (terrain, water) in crate::authoring::terrain_documents::root_water::listed_sources(draft).into_iter().flatten() {
        let count = water_feature_count(water);
        if flat_index < count {
            let (kind, index, id) = water_feature_at(water, flat_index)?;
            return Some((terrain, water, kind, index, id));
        }
        flat_index = flat_index.saturating_sub(count);
    }
    None
}

pub(super) fn listed_active_water_feature<'a>(
    draft: &'a crate::authoring::EditorSceneDraft,
    editor: &SceneEditorState,
    selection: &SelectedWaterViewportState,
) -> Option<(Option<&'a feathrise_scene_content::TerrainId>, WaterAuthoringFeatureKind, &'a str, usize)> {
    let water = draft.water.as_ref()?;
    let (kind, id, index) = active_water_feature(water, editor, &draft.scene_id, selection)?;
    let terrain = draft.terrain_documents.active.as_ref().map(|active| &active.definition.id);
    let offset = if terrain.is_some() {
        crate::authoring::terrain_documents::root_water::source(draft).water.map_or(0, water_feature_count)
    } else { 0 };
    Some((terrain, kind, id, offset.saturating_add(index)))
}

/// 选择只恢复来源和淘汰迟到网格载入，不新增历史，也不创建任何水文档或遮罩。
pub(crate) fn focus_water_source(
    editor: &mut SceneEditorState,
    session: &mut crate::authoring::EditorAuthoringSession,
    terrain: Option<&feathrise_scene_content::TerrainId>,
    source_busy: bool,
    selection: Option<&mut crate::ui::terrain_objects::TerrainObjectsEditorState>,
    documents: Option<&mut crate::authoring::terrain_documents::TerrainDocumentTask>,
) -> Result<(), String> {
    let draft = session.draft.as_ref().ok_or("当前场景没有可选择的水域作者数据")?;
    crate::authoring::terrain_documents::root_water::validate_listed_focus(draft, terrain)?;
    focus_author_source(editor, session, terrain, source_busy, selection, documents)
}

/// 候选卡片按原来源恢复地表、植被或水域；全部资格门共享水域列表的固定预算与事务保护。
pub(crate) fn focus_author_source(
    editor: &mut SceneEditorState,
    session: &mut crate::authoring::EditorAuthoringSession,
    terrain: Option<&feathrise_scene_content::TerrainId>,
    source_busy: bool,
    selection: Option<&mut crate::ui::terrain_objects::TerrainObjectsEditorState>,
    documents: Option<&mut crate::authoring::terrain_documents::TerrainDocumentTask>,
) -> Result<(), String> {
    if session.saving { return Err("场景保存期间暂不切换水域对象".into()); }
    let draft = session.draft.as_ref().ok_or("当前场景没有可选择的水域作者数据")?;
    crate::authoring::terrain_documents::root_water::validate_cached_focus(draft, terrain)?;
    if terrain.is_some() && selection.as_ref().is_some_and(|selection| selection.documents_loading) {
        return Err("水域所属网格画布正在载入，请完成后重新选择".into()); // 同一活动ID也不能把正在重载的旧作者槽重新授予写权限。
    }
    let switching = draft.terrain_documents.active.as_ref().map(|active| &active.definition.id) != terrain;
    if switching && (source_busy || selection.as_ref().is_some_and(|selection| selection.busy())) {
        return Err("当前画布仍有编辑事务，请完成后再选择其他水域来源".into());
    }
    if switching && session.sparse_terrain_dirty {
        return Err("当前高度作者源尚未保存；保存完成后继续切换水域来源".into());
    }
    if switching {
        crate::authoring::terrain_documents::scope_history(session); // 沿用每帧至多64条的既有预算，先冻结旧源历史。
        if session.draft.as_ref().is_some_and(|draft| draft.terrain_documents.scoped_through < session.undo.len()) {
            return Err("正在整理当前画布撤销记录，请稍后重新选择水域来源".into()); // 历史尚未完整归属前绝不停放或换槽。
        }
    }
    let revision = session.water_revision;
    let draft = session.draft.as_mut().ok_or("当前场景没有可选择的水域作者数据")?;
    crate::authoring::terrain_documents::root_water::remember_revision(draft, revision);
    let changed = crate::authoring::terrain_documents::root_water::focus_cached(draft, terrain)?;
    if let Some(selection) = selection {
        selection.selected = terrain.cloned();
        selection.selected_index = terrain.and_then(|id| draft.manifest.terrains.as_ref()
            .and_then(|objects| objects.iter().position(|object| &object.id == id)));
        selection.documents_loading = false;
    }
    if let Some(documents) = documents { documents.requested = None; }
    draft.terrain_documents.loading = false;
    editor.terrain_canvas_target = terrain.cloned();
    editor.cursor_world = None;
    editor.brush_cursor_world = None;
    if changed {
        session.revision = session.revision.wrapping_add(1);
        session.surface_revision = session.surface_revision.wrapping_add(1);
        session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
        session.vegetation_structure_revision = session.vegetation_structure_revision.wrapping_add(1);
        session.water_revision = session.water_revision.wrapping_add(1);
    } // 来源换代让旧数值输入、画笔和在途网格结果失效，水域像素保持原Arc。
    Ok(())
}

pub(crate) fn activate_listed_water_feature(
    editor: &mut SceneEditorState,
    session: &mut crate::authoring::EditorAuthoringSession,
    terrain: Option<&feathrise_scene_content::TerrainId>,
    source_busy: bool,
    kind: WaterAuthoringFeatureKind,
    id: &str,
    selection: Option<&mut crate::ui::terrain_objects::TerrainObjectsEditorState>,
    documents: Option<&mut crate::authoring::terrain_documents::TerrainDocumentTask>,
) -> Result<(), String> {
    let water = session.draft.as_ref().and_then(|draft| crate::authoring::terrain_documents::root_water::listed_source(draft, terrain))
        .ok_or("水域来源已变化，请重新选择当前列表项")?;
    let exists = match kind {
        WaterAuthoringFeatureKind::StillWater => water.layers.iter().any(|layer| layer.id == id),
        WaterAuthoringFeatureKind::River => water.rivers.iter().any(|river| river.id == id),
        WaterAuthoringFeatureKind::Waterfall => water.waterfalls.iter().any(|waterfall| waterfall.id == id),
    };
    if !exists { return Err("水域对象已变化，请重新选择当前列表项".into()); }
    focus_water_source(editor, session, terrain, source_busy, selection, documents)?;
    let draft = session.draft.as_ref().unwrap();
    activate_water_feature(editor, draft.water.as_ref().unwrap(), kind, id)?;
    if terrain.is_some() {
        let prefix = crate::authoring::terrain_documents::root_water::source(draft).water.map_or(0, water_feature_count);
        let (_, _, local_index) = active_water_feature(draft.water.as_ref().unwrap(), editor, &draft.scene_id, &SelectedWaterViewportState::default()).unwrap();
        editor.water_feature_page = prefix.saturating_add(local_index) / WATER_FEATURE_ROW_COUNT;
    }
    Ok(())
}

/// 返回三类作者对象合计数量；Blender整网格水继续由右栏Outliner独立管理。
pub(super) fn water_feature_count(water: &SceneWaterAsset) -> usize {
    water
        .layers
        .len()
        .saturating_add(water.rivers.len())
        .saturating_add(water.waterfalls.len()) // 格式上限远低于usize，饱和只作防御性处理。
}

/// 至少返回一页，保证空水域状态标题仍能显示稳定的“第1/1页”。
pub(super) fn water_feature_page_count(total: usize) -> usize {
    total.div_ceil(WATER_FEATURE_ROW_COUNT).max(1) // 六的整倍数不额外制造空白页。
}

/// 把“静水→河流→瀑布”的列表扁平位置解析为类别、类内索引和稳定ID。
pub(super) fn water_feature_at(
    water: &SceneWaterAsset,
    flat_index: usize,
) -> Option<(WaterAuthoringFeatureKind, usize, &str)> {
    if let Some(layer) = water.layers.get(flat_index) {
        return Some((WaterAuthoringFeatureKind::StillWater, flat_index, &layer.id)); // 静水位于统一列表首段。
    }
    let river_index = flat_index.checked_sub(water.layers.len())?; // 第二段从静水数量之后开始。
    if let Some(river) = water.rivers.get(river_index) {
        return Some((WaterAuthoringFeatureKind::River, river_index, &river.id)); // 河流保持JSON数组顺序。
    }
    let waterfall_index = river_index.checked_sub(water.rivers.len())?; // 第三段继续扣除完整河流数量。
    water.waterfalls.get(waterfall_index).map(|waterfall| {
        (
            WaterAuthoringFeatureKind::Waterfall,
            waterfall_index,
            waterfall.id.as_str(),
        ) // 瀑布行同样只发布稳定ID，不把扁平下标当身份。
    })
}

/// 返回当前真正活动水域对象的类别、稳定ID和统一列表位置；非水域工具没有活动高亮。
pub(super) fn active_water_feature<'a>(
    water: &'a SceneWaterAsset,
    editor: &SceneEditorState,
    scene_id: &SceneId,
    viewport_selection: &SelectedWaterViewportState,
) -> Option<(WaterAuthoringFeatureKind, &'a str, usize)> {
    match editor.tool {
        EditorTool::Water => water.layers.get(editor.selected_water_layer).map(|layer| {
            (
                WaterAuthoringFeatureKind::StillWater,
                layer.id.as_str(),
                editor.selected_water_layer,
            )
        }),
        EditorTool::RiverSpline => water.rivers.get(editor.selected_water_river).map(|river| {
            (
                WaterAuthoringFeatureKind::River,
                river.id.as_str(),
                water
                    .layers
                    .len()
                    .saturating_add(editor.selected_water_river),
            )
        }),
        EditorTool::Waterfall => water
            .waterfalls
            .get(editor.selected_waterfall)
            .map(|waterfall| {
                (
                    WaterAuthoringFeatureKind::Waterfall,
                    waterfall.id.as_str(),
                    water
                        .layers
                        .len()
                        .saturating_add(water.rivers.len())
                        .saturating_add(editor.selected_waterfall),
                )
            }),
        EditorTool::Inspect => {
            let (kind, feature_id) = viewport_selection.viewport_feature(scene_id)?; // 只接受本图真实视口点选身份。
            match kind {
                WaterAuthoringFeatureKind::StillWater => water
                    .layers
                    .iter()
                    .enumerate()
                    .find(|(_, layer)| layer.id == feature_id)
                    .map(|(index, layer)| (kind, layer.id.as_str(), index)),
                WaterAuthoringFeatureKind::River => water
                    .rivers
                    .iter()
                    .enumerate()
                    .find(|(_, river)| river.id == feature_id)
                    .map(|(index, river)| {
                        (
                            kind,
                            river.id.as_str(),
                            water.layers.len().saturating_add(index),
                        )
                    }),
                WaterAuthoringFeatureKind::Waterfall => water
                    .waterfalls
                    .iter()
                    .enumerate()
                    .find(|(_, waterfall)| waterfall.id == feature_id)
                    .map(|(index, waterfall)| {
                        (
                            kind,
                            waterfall.id.as_str(),
                            water
                                .layers
                                .len()
                                .saturating_add(water.rivers.len())
                                .saturating_add(index),
                        )
                    }),
            }
        }
        _ => None, // 切到地形、植被等工具后保留三套索引，但不伪装成正在编辑水域。
    }
}

/// 生成一行紧凑摘要；类型和一基序号让同名对象仍可肉眼区分。
pub(super) fn water_feature_row_text(
    water: &SceneWaterAsset,
    kind: WaterAuthoringFeatureKind,
    index: usize,
    selected: bool,
) -> Option<String> {
    let marker = if selected { "◆" } else { "◇" }; // 选中态同时使用符号和背景，不只依赖颜色。
    match kind {
        WaterAuthoringFeatureKind::StillWater => water.layers.get(index).map(|layer| {
            format!(
                "{marker} 静水{} · {} · Y{:.3}m",
                index + 1,
                layer.label,
                layer.water_level,
            )
        }),
        WaterAuthoringFeatureKind::River => water.rivers.get(index).map(|river| {
            format!(
                "{marker} 河流{} · {} · {}点{}",
                index + 1,
                river.label,
                river.points.len(),
                if river.points.len() < 2 {
                    " · 未完成"
                } else {
                    ""
                },
            )
        }),
        WaterAuthoringFeatureKind::Waterfall => water.waterfalls.get(index).map(|waterfall| {
            format!(
                "{marker} 瀑布{} · {} · {}点{}",
                index + 1,
                waterfall.label,
                waterfall.points.len(),
                if waterfall.points.len() < 2 {
                    " · 未完成"
                } else {
                    ""
                },
            )
        }),
    }
}

/// 按类别和稳定ID重新定位当前数组并切换工具；纯选择不修改dirty、作者修订或撤销栈。
pub(crate) fn activate_water_feature(
    editor: &mut SceneEditorState,
    water: &SceneWaterAsset,
    kind: WaterAuthoringFeatureKind,
    feature_id: &str,
) -> Result<(), String> {
    let (flat_index, status) = match kind {
        WaterAuthoringFeatureKind::StillWater => {
            let (index, layer) = water
                .layers
                .iter()
                .enumerate()
                .find(|(_, layer)| layer.id == feature_id)
                .ok_or_else(|| format!("静水体{feature_id}已变化；水域列表正在刷新"))?;
            editor.selected_water_layer = index; // 下游水位框和R8笔刷继续读取既有会话索引。
            editor.water_selected_preset_id =
                (!layer.preset_id.is_empty()).then(|| layer.preset_id.clone()); // 直选静水时让全局预设卡同步回显其稳定引用，空旧值则明确保持未选择。
            editor.tool = EditorTool::Water; // 直选静水后立即进入对应画布工具。
            (
                index,
                format!(
                    "已选择静水体：{} · 水位{:.3}m · 双击行定位",
                    layer.label, layer.water_level
                ),
            )
        }
        WaterAuthoringFeatureKind::River => {
            let (index, river) = water
                .rivers
                .iter()
                .enumerate()
                .find(|(_, river)| river.id == feature_id)
                .ok_or_else(|| format!("河流{feature_id}已变化；水域列表正在刷新"))?;
            editor.selected_water_river = index; // 数值事务随后会再次冻结稳定河流ID和作者修订。
            editor.tool = EditorTool::RiverSpline; // 视口控制点和Gizmo切到该河流。
            (
                water.layers.len().saturating_add(index),
                format!(
                    "已选择河流：{} · {}个控制点",
                    river.label,
                    river.points.len()
                ),
            )
        }
        WaterAuthoringFeatureKind::Waterfall => {
            let (index, waterfall) = water
                .waterfalls
                .iter()
                .enumerate()
                .find(|(_, waterfall)| waterfall.id == feature_id)
                .ok_or_else(|| format!("瀑布{feature_id}已变化；水域列表正在刷新"))?;
            editor.selected_waterfall = index; // 瀑布控制点和端点编辑继续使用既有类内索引。
            editor.tool = EditorTool::Waterfall; // 直选后进入瀑布水幕工具。
            (
                water
                    .layers
                    .len()
                    .saturating_add(water.rivers.len())
                    .saturating_add(index),
                format!(
                    "已选择瀑布：{} · {}个控制点",
                    waterfall.label,
                    waterfall.points.len(),
                ),
            )
        }
    };
    editor.selected_water_control_point = None; // 跨对象切换不得把旧路径点下标套到新对象。
    editor.water_feature_page = flat_index / WATER_FEATURE_ROW_COUNT; // 直选、分页和结构动作共享统一可见页定位。
    editor.status = status; // 底栏立即回显最终稳定对象，而不是临时数组下标。
    Ok(())
}

#[cfg(test)]
mod tests {
    use feathrise_scene_content::{SceneId, SceneWaterRiver, SceneWaterfall}; // 构造三类最小合法作者对象。

    use super::*; // 直接验证纯列表映射和稳定ID选择，不启动Bevy App。
    use crate::authoring::water_asset_with_layer_for_tests; // 列表夹具显式加入一层合法静水材质与路径。

    /// 构造静水、河流和瀑布各一项的紧凑列表夹具。
    fn sample_water() -> SceneWaterAsset {
        let scene_id = SceneId::parse("MAP-water-list-test").expect("测试场景ID有效"); // 使用正式SceneId解析门。
        let mut water = water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-list-test"); // 列表测试明确需要一项静水体。
        water.layers[0].id = "still-a".to_owned(); // 明确稳定ID便于断言扁平顺序。
        water.layers[0].label = "试验湖".to_owned(); // 行文本应显示作者名称。
        water.rivers.push(SceneWaterRiver {
            id: "river-a".to_owned(),                    // 河流身份与显示名分离。
            label: "试验河".to_owned(),                  // 允许与其他类别出现相同或相近名称。
            source_layer_id: None,                       // 空连接是合法编辑中状态。
            target_layer_id: None,                       // 正式保存前可继续连接。
            points: Vec::new(),                          // 零点河流应显示未完成。
            base_color_srgba: [0.055, 0.24, 0.29, 0.72], // 使用统一水色测试值。
            perceptual_roughness: 0.22,                  // 保持生产允许范围。
        });
        water.waterfalls.push(SceneWaterfall {
            id: "fall-a".to_owned(),                     // 瀑布稳定身份独立于河流。
            label: "试验瀑布".to_owned(),                // 列表直接显示作者标签。
            source: None,                                // 空端点允许留在编辑草稿。
            target: None,                                // 正式保存仍由既有校验拒绝未连接瀑布。
            points: Vec::new(),                          // 零点瀑布同样标记未完成。
            foam_strength: 1.0,                          // 使用正式默认泡沫强度。
            base_color_srgba: [0.055, 0.24, 0.29, 0.72], // 使用统一水色测试值。
            perceptual_roughness: 0.22,                  // 保持生产允许范围。
        });
        water // 返回仅用于列表纯值测试的作者描述。
    }

    #[test]
    fn flattened_water_features_keep_still_river_waterfall_order() {
        let water = sample_water(); // 三类各一项应形成连续三行。
        assert_eq!(water_feature_count(&water), 3); // 总数不包含右栏Blender整网格水。
        assert_eq!(
            water_feature_at(&water, 0).map(|value| value.0),
            Some(WaterAuthoringFeatureKind::StillWater)
        ); // 第一行是静水。
        assert_eq!(
            water_feature_at(&water, 1).map(|value| value.0),
            Some(WaterAuthoringFeatureKind::River)
        ); // 第二行是河流。
        assert_eq!(
            water_feature_at(&water, 2).map(|value| value.0),
            Some(WaterAuthoringFeatureKind::Waterfall)
        ); // 第三行是瀑布。
        assert!(water_feature_at(&water, 3).is_none()); // 越界位置不冒充最后一项。
    }

    #[test]
    fn page_count_handles_empty_exact_and_partial_pages() {
        assert_eq!(water_feature_page_count(0), 1); // 空状态仍有稳定标题页码。
        assert_eq!(water_feature_page_count(6), 1); // 整六项只占一页。
        assert_eq!(water_feature_page_count(7), 2); // 第七项进入第二页。
    }

    #[test]
    fn stable_id_selection_switches_tools_without_marking_the_scene_dirty() {
        let water = sample_water(); // 当前数组顺序只是查找入口，不是提交身份。
        let mut editor = SceneEditorState::default(); // 默认状态未标脏且没有控制点选择。
        editor.selected_water_control_point = Some(9); // 模拟上一对象遗留的越界点下标。
        activate_water_feature(
            &mut editor,
            &water,
            WaterAuthoringFeatureKind::Waterfall,
            "fall-a",
        )
        .expect("稳定瀑布ID应能直选");
        assert_eq!(editor.tool, EditorTool::Waterfall); // 直选同步切换对应视口工具。
        assert_eq!(editor.selected_waterfall, 0); // 稳定ID解析为当前类内索引。
        assert_eq!(editor.selected_water_control_point, None); // 旧对象点下标必须清空。
        assert!(!editor.dirty); // 纯选择不进入作者修改与保存责任。
    }

    #[test]
    fn inspect_viewport_selection_resolves_the_same_water_list_row() {
        let water = sample_water(); // 三类对象各一项，瀑布位于扁平列表第三行。
        let scene_id = SceneId::parse("MAP-water-list-test").expect("测试场景ID有效");
        let editor = SceneEditorState {
            tool: EditorTool::Inspect, // 视口点选必须保持只读查看工具。
            ..SceneEditorState::default()
        };
        let mut viewport_selection = SelectedWaterViewportState::default();
        let initial_revision = viewport_selection.viewport_selection_revision();

        assert!(!viewport_selection.select_from_viewport(
            &scene_id,
            WaterAuthoringFeatureKind::Waterfall,
            "fall-a",
            1.0,
        )); // 第一次点击只选择，不登记相机定位。
        assert_eq!(
            viewport_selection.viewport_selection_revision(),
            initial_revision.wrapping_add(1),
        ); // 固定行池在稳定身份真实变化时只推进一次修订。
        assert_eq!(
            active_water_feature(&water, &editor, &scene_id, &viewport_selection),
            Some((WaterAuthoringFeatureKind::Waterfall, "fall-a", 2)),
        ); // 查看模式按稳定ID高亮与列表点击完全相同的瀑布整对象行。
        assert!(
            active_water_feature(
                &water,
                &editor,
                &SceneId("MAP-other".to_owned()),
                &viewport_selection,
            )
            .is_none()
        ); // 同ID跨场景不得污染另一个场景的列表高亮。
        assert!(!editor.dirty); // 视口纯选择不修改作者数据、脏标记或撤销栈。
    }

    #[test]
    fn unfinished_network_rows_are_visible_diagnostics() {
        let water = sample_water(); // 零点河流仍应出现在列表供作者继续编辑。
        let text = water_feature_row_text(&water, WaterAuthoringFeatureKind::River, 0, false)
            .expect("河流行应存在");
        assert!(text.contains("未完成")); // 列表不能静默隐藏正式保存会拒绝的中间态。
    }
}
