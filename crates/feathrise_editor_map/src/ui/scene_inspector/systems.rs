//! 只读同步当前地图的选择身份和原状态文字，不遍历地图实例或猜测运行时成功。

use bevy::prelude::*;
use feathrise_map_content::MapTerrainKind;

use super::components::{SceneInspectorOverview, SceneInspectorProperties, SceneInspectorText};
use super::types::{SceneInspectorField, SceneInspectorObservation};
use crate::authoring::EditorAuthoringSession;
use crate::resources::{MapEditorState, TerrainShadingSession};
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::ui::placement_transform::PlacementTransformInputState;
use crate::viewport::{LayerCandidateState, LayerTarget};
use super::components::{InspectorSection, InspectorBody};

/// 只依据正式选择分派固定属性域，不让其他工具记住的旧选择接管检查器。
pub(super) fn section_for(target: Option<&LayerTarget>) -> Option<InspectorSection> {
    match target? {
        LayerTarget::TerrainObject(_) => Some(InspectorSection::Terrain),
        LayerTarget::Water(_, _) => Some(InspectorSection::Water),
        LayerTarget::Vegetation(_) => Some(InspectorSection::Vegetation),
        LayerTarget::Domain(_) => Some(InspectorSection::Domain),
        LayerTarget::SubMap(_) => Some(InspectorSection::SubMap),
        LayerTarget::Placement(_) => Some(InspectorSection::ObjectManagement),
        _ => None,
    }
}

pub(crate) fn sync_scene_inspector(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    shading: Res<TerrainShadingSession>,
    transform: Res<PlacementTransformInputState>,
    selection: (Option<Res<LayerCandidateState>>, Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>),
    domain: Option<Res<crate::game_authoring::map_interaction::MapInteractionState>>,
    mut previous: Local<Option<SceneInspectorObservation>>,
    added: Query<(), Added<SceneInspectorOverview>>,
    mut texts: Query<
        (&SceneInspectorText, &mut Text, &mut Node),
        (Without<SceneInspectorProperties>, Without<InspectorSection>),
    >,
    mut properties: Query<&mut Node, (With<SceneInspectorProperties>, Without<SceneInspectorText>, Without<InspectorSection>)>,
    mut sections: Query<(&InspectorSection, &mut Node), (Without<SceneInspectorText>, Without<SceneInspectorProperties>)>,
    mut scrolls: Query<&mut ScrollPosition, With<InspectorBody>>,
    mut metadata: Query<&mut Node, (With<super::components::SceneInspectorMetadata>, Without<SceneInspectorText>, Without<SceneInspectorProperties>, Without<InspectorSection>)>,
) {
    if !editor.open {
        return;
    }
    let draft = session.draft.as_ref().filter(|draft| {
        editor.selected_map_id.as_ref() == Some(&draft.map_id) && !editor.map_transition_in_progress
    }); // 旧草稿和新图同名实例不得在检查器中串用。
    let target = selection.0.as_deref().and_then(|state| state.selected.as_ref());
    let multi_count = selection.1.as_deref().filter(|state| state.ready_for(&session))
        .map_or(0, |state| state.members.len());
    let management = matches!(target, None | Some(LayerTarget::Placement(_)))
        && (multi_count > 1 || browser.selected_group.is_some());
    let section = if management { Some(InspectorSection::ObjectManagement) } else { section_for(target) };
    let indexed_record = draft.and_then(|draft| {
        if management || target.is_some_and(|target| !matches!(target, LayerTarget::Placement(_))) { return None; }
        let id = browser.selected_placement_id.as_ref()?;
        let index = browser.placement_record_index(id, session.placement_structure_revision)?;
        draft
            .placements
            .placements
            .get(index)
            .filter(|record| record.id == *id)
            .map(|record| (index, record))
    }); // 复用结构签名的O(1)下标，禁止为一个概览扫描8192条记录。
    let record_index = indexed_record.map(|(index, _)| index);
    let draft_map_id = session.draft.as_ref().map(|draft| &draft.map_id);
    let relocation_armed = transform.relocation_armed();
    let unchanged = previous.as_ref().is_some_and(|old| {
        old.target.as_ref() == target && old.multi_count == multi_count && old.group == browser.selected_group
            && old.map_id.as_ref() == editor.selected_map_id.as_ref()
            && old.draft_map_id.as_ref() == draft_map_id
            && old.revision == session.revision
            && old.structure_revision == session.placement_structure_revision
            && old.saving == session.saving
            && old.tool == editor.tool
            && old.loading == editor.map_transition_in_progress
            && old.placement_id.as_ref() == browser.selected_placement_id.as_ref()
            && old.mesh.as_ref() == browser.selection.as_ref()
            && old.record_index == record_index
            && old.preview_expanded == shading.preview_expanded
            && old.terrain_map_id.as_ref() == shading.map_id.as_ref()
            && old.relocation_armed == relocation_armed
    });
    if unchanged && added.is_empty() {
        return;
    } // 只借用比较一份有界选择，不因浏览进度或光标Changed克隆路径。
    let changed_selection = previous.as_ref().is_none_or(|old| {
        old.target.as_ref() != target || old.map_id != editor.selected_map_id
            || old.placement_id != browser.selected_placement_id || old.group != browser.selected_group
            || old.multi_count != multi_count
    });
    if changed_selection {
        // 新对象从属性标题开始阅读；数值回显、后台反馈与分隔拖动不打断当前滚动位置。
        for mut scroll in &mut scrolls {
            if scroll.0 != Vec2::ZERO { scroll.0 = Vec2::ZERO; }
        }
    }
    *previous = Some(SceneInspectorObservation {
        target: target.cloned(), multi_count, group: browser.selected_group.clone(),
        map_id: editor.selected_map_id.clone(),
        draft_map_id: draft_map_id.cloned(),
        revision: session.revision,
        structure_revision: session.placement_structure_revision,
        saving: session.saving,
        tool: editor.tool,
        loading: editor.map_transition_in_progress,
        placement_id: browser.selected_placement_id.clone(),
        mesh: browser.selection.clone(),
        record_index,
        preview_expanded: shading.preview_expanded,
        terrain_map_id: shading.map_id.clone(),
        relocation_armed,
    }); // 只在真实身份/状态边沿拥有短快照，不复制摆放记录、水绑定或场景树。
    let record = indexed_record.map(|(_, record)| record);
    for (kind, mut node) in &mut sections {
        let visible = draft.is_some() && (section == Some(*kind)
            || (matches!(kind, InspectorSection::ObjectManagement | InspectorSection::ObjectName) && record.is_some()));
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    let mesh = browser
        .selection
        .as_ref()
        .filter(|selection| record.is_some_and(|record| record.id == selection.placement_id));
    for mut node in &mut properties {
        let display = if record.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let (name, details, hint) = if editor.map_transition_in_progress {
        (
            "地图加载中".to_owned(),
            "等待当前地图及作者草稿就绪".to_owned(),
            "加载期间不显示旧地图对象属性；进度与失败原因沿用原加载反馈。".to_owned(),
        )
    } else if let Some(draft) = draft {
        if management {
            (browser.selected_group.as_ref().map_or_else(|| format!("已选择 {multi_count} 个对象"), |group| format!("对象组 · {group}")),
                "批量属性".to_owned(), "分组、显隐和锁定作用于当前选集；单个对象参数已收起。".to_owned())
        } else if target.is_some_and(|target| !matches!(target, LayerTarget::Placement(_))) {
            let (label, id) = match target.unwrap() {
                LayerTarget::TerrainObject(id) => ("网格", id), LayerTarget::SubMap(id) => ("子地图", id),
                LayerTarget::Water(_, id) | LayerTarget::RootWater(_, id) => ("水域", id), LayerTarget::Domain(id) => (domain.as_deref().map_or("扩展内容", |state| state.label()), id),
                LayerTarget::Vegetation(id) => ("植被层", id), LayerTarget::Surface(id) => ("地表层", id),
                LayerTarget::Portal(id) => ("传送点", id), LayerTarget::Boundary(id) => ("边界", id),
                LayerTarget::Placement(id) => ("摆放对象", id),
            };
            (format!("所选{label}"), format!("对象 ID  {id}"),
                "属性修改沿用地图保存与撤销；创建与绘制入口位于顶部工具栏。".to_owned())
        } else if let Some(record) = record {
            let name = mesh.map_or_else(
                || {
                    if record.asset_target.is_builtin() {
                        return record.asset_target.label(); // 内置形状以类型作默认名称，不显示空资产路径。
                    }
                    record
                        .asset_path
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(&record.asset_path)
                        .to_owned()
                },
                |selection| {
                    if selection.mesh_name.trim().is_empty() {
                        "未命名网格".to_owned()
                    } else {
                        selection.mesh_name.clone()
                    }
                },
            );
            let kind = if record.asset_target.is_builtin() {
                "网格"
            } else if record.building_preset_id.is_some() {
                "建筑预设实例"
            } else if !record.asset_target.is_scene0() {
                "静态子资产实例"
            } else {
                "普通摆放实例"
            };
            let mut details = format!(
                "类型  {kind}\n来源  {}\n实例  {}",
                if matches!(record.asset_target, feathrise_map_content::MapPlacementAssetTarget::EditableMesh { .. }) {
                    "编辑网格 · 自定义拓扑"
                } else if matches!(record.asset_target, feathrise_map_content::MapPlacementAssetTarget::EditableCuboid { .. }) {
                    "编辑网格 · 8点 / 12边 / 6面"
                } else if record.asset_target.is_builtin() { "内置立方体" } else { &record.asset_path }, record.id
            );
            if let Some(selection) = mesh {
                details.push_str(&format!(
                    "\n网格路径  {} / {}",
                    selection.node_path.join(" / "),
                    selection.mesh_name
                ));
            }
            let hint = if session.saving {
                "地图保存中，暂不能修改所选对象。"
            } else if mesh.is_some_and(|selection| selection.orphaned_water_binding) {
                "网格水绑定已失效；可在下方“网格水体”中取消绑定，不会删除整个实例。"
            } else if mesh.is_some_and(|selection| !selection.valid) {
                "网格名称不稳定，不能赋水；请在Blender中显式命名对象与网格。"
            } else if transform.relocation_armed() {
                "等待中央视口单击落点；吸附地面沿用真实地形命中规则。"
            } else if record.building_preset_id.is_some() {
                "建筑使用受检落点与90°旋转，不开放任意变换数值。"
            } else if !matches!(
                editor.tool,
                crate::types::EditorTool::Inspect | crate::types::EditorTool::PlaceAsset
            ) {
                "变换、复制与删除仍可使用；定位请先切到选择模式，避免聚焦其他工具的目标。"
            } else {
                "选择模式下定位只调整视图；复制、删除和变换沿用地图撤销与保存。"
            };
            (name, details, hint.to_owned())
        } else if browser.selected_placement_id.is_some() {
            (
                "所选对象暂不可用".to_owned(),
                "场景树正在更新，或实例身份已失效".to_owned(),
                "等待场景树完成后重新选择；不按名称猜测替代对象。".to_owned(),
            )
        } else if shading.preview_expanded && shading.map_id.as_ref() == Some(&draft.map_id)
            && draft.manifest.terrains.is_none() {
            let terrain = &draft.manifest.terrain;
            let source = if terrain.kind == MapTerrainKind::SparseClipmap {
                terrain
                    .height_path
                    .as_deref()
                    .unwrap_or("尚未指定稀疏高度块")
            } else {
                &terrain.glb_path
            };
            (
                "原地图基础网格".to_owned(),
                format!("来源  {source}"),
                "在上方选择网格后，下方显示网格属性；显示预览只影响本次运行。".to_owned(),
            )
        } else {
            (
                format!("地图 · {}", draft.manifest.display_name),
                format!("备注  {}", if draft.manifest.notes.trim().is_empty() { "未填写" } else { &draft.manifest.notes }),
                "在场景树或视口选择对象以编辑属性；地图资料可在左侧地图页修改。".to_owned(),
            )
        }
    } else if editor.selected_map_id.is_some() {
        (
            "等待地图草稿".to_owned(),
            "当前地图的作者草稿尚未就绪".to_owned(),
            "就绪前不显示其他地图的对象来源与属性。".to_owned(),
        )
    } else {
        (
            "未进入地图".to_owned(),
            "从左侧地图页选择并进入地图".to_owned(),
            "空世界仍可编辑全局预设；预设使用对应的独立预览。".to_owned(),
        )
    };
    for mut node in &mut metadata {
        let display = if section == Some(InspectorSection::Terrain) { Display::None } else { Display::Flex };
        if node.display != display { node.display = display; }
    }
    let (name, hint) = if section == Some(InspectorSection::Terrain) {
        (String::new(), String::new()) // 网格自身已给出真实名称、来源与常用属性，避免重复“所选网格”和固定教程。
    } else { (name, hint) };
    for (marker, mut text, mut node) in &mut texts {
        let value = match marker.0 {
            SceneInspectorField::Name => &name,
            SceneInspectorField::Details => &details,
            SceneInspectorField::Hint => &hint,
        };
        if text.as_str() != value {
            **text = value.clone();
        }
        let display = if value.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
}
