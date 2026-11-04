//! 完整实例属性面板的显隐、摘要、数值模型和动作按钮视觉同步。

use crate::ui::theme as colors;
use bevy::prelude::*; // Commands、Node、文本、颜色、查询和资源。
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputConfig, NumericInputDisabled,
    NumericInputScrubConfig, NumericInputSetValue, NumericInputValue, NumericValue,
    round_to_decimals,
};
use feathrise_map_content::{MapPlacementAssetTarget, MapPlacementHeightMode}; // 摘要与两种持久高度模式选中态读取正式数据枚举。
use feathrise_editor_core::EditorSettings; // 三类变换拖动手感的当前持久值。 // 公共输入继续拥有焦点、指针捕获、范围和权威显示同步。

use super::{
    MapEditorPlacementTransformActionButton, MapEditorPlacementTransformPanel, MapEditorPlacementTransformLocate,
    MapEditorPlacementTransformSummary, PlacementRelocationIntent, PlacementTransformAction,
    PlacementTransformField, PlacementTransformInputState, selected_target,
    PlacementPropertiesSection, MapEditorCuboidGeometryRow,
}; // 当前面板marker、字段、动作和吸附模式。
use crate::authoring::EditorAuthoringSession; // 当前完整记录和地图边界来自作者草稿。
use crate::resources::MapEditorState; // F12生命周期、选择地图与保存状态。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 完整实例选择和O(1)结构索引。

/// 选择、撤销、设置或结构换代后原地同步完整资产属性面板。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_placement_transform_panel(
    mut commands: Commands,               // 增删公共NumericInputDisabled标记。
    editor: Res<MapEditorState>,          // F12与当前地图状态决定可编辑性。
    browser: Res<EditorGltfBrowserState>, // 当前完整实例选择。
    session: Res<EditorAuthoringSession>, // 当前记录、修订和地图清单。
    settings: (Res<EditorSettings>, Option<Res<crate::authoring::mesh_edit::MeshEditState>>), // 网格编辑时停用完整对象数值，保留相机导航。
    mut state: ResMut<PlacementTransformInputState>, // 吸附目标失效时收口模式，活动期间禁用数字框。
    active: Res<ActiveNumericInput>,      // 字符缓冲期间不得覆盖显示或规格。
    active_scrub: Res<ActiveNumericInputScrub>, // 左右拖动期间不得覆盖数值和倍率。
    mut panels: Query<
        (&mut Node, Option<&PlacementPropertiesSection>, Has<MapEditorCuboidGeometryRow>),
        (
            Or<(With<MapEditorPlacementTransformPanel>, With<PlacementPropertiesSection>, With<MapEditorCuboidGeometryRow>)>,
            Without<super::PlacementTransformInputSlot>,
            Without<MapEditorPlacementTransformActionButton>,
        ),
    >, // 与槽位Node显式互斥。
    mut slots: Query<
        (&super::PlacementTransformInputSlot, &mut Node),
        (Without<MapEditorPlacementTransformPanel>, Without<PlacementPropertiesSection>, Without<MapEditorCuboidGeometryRow>, Without<MapEditorPlacementTransformActionButton>),
    >,
    mut summaries: Query<&mut Text, With<MapEditorPlacementTransformSummary>>, // 唯一身份与变换摘要。
    mut inputs: Query<(
        Entity,                       // 公共NumericInput业务根。
        &PlacementTransformField,     // 七个字段之一。
        &NumericInputValue,           // 当前规范化显示值。
        Has<NumericInputDisabled>,    // 当前禁用标记。
        &mut NumericInputConfig,      // X/Z随地图清单动态更新范围。
        &mut NumericInputScrubConfig, // 左拖范围和三类持久倍率。
    )>,
    mut set_values: MessageWriter<NumericInputSetValue>, // 外部作者值通过公共桥同步并取消旧缓冲。
    mut buttons: Query<
        (
            &Interaction,
            &MapEditorPlacementTransformActionButton,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (Without<MapEditorPlacementTransformPanel>, Without<PlacementPropertiesSection>, Without<MapEditorCuboidGeometryRow>),
    >, // 面板与按钮Node必须静态互斥，建筑旋转只在适用时显示。
    candidates: Res<crate::viewport::content_selection::LayerCandidateState>,
    locate_buttons: Query<
        (Entity, Has<bevy::ui::InteractionDisabled>),
        With<MapEditorPlacementTransformLocate>,
    >,
) {
    let mesh_editing = settings.1.as_ref().is_some_and(|state| state.active());
    let settings = &settings.0;
    let relocation_valid = state.relocation_target.as_ref().is_none_or(|target| {
        editor.open
            && !session.saving
            && !editor.new_map_dialog_open
            && browser.selected_placement_id.as_deref() == Some(target.placement_id.as_str())
            && target.structure_revision == session.placement_structure_revision
            && target.authoring_revision == session.revision
            && session.draft.as_ref().is_some_and(|draft| {
                draft.map_id == target.map_id
                    && draft
                        .placements
                        .placements
                        .get(target.record_index)
                        .is_some_and(|record| record.id == target.placement_id)
            })
    }); // 关闭、保存、切图、结构变化、另选资产或其他作者动作都不能保留旧落点意图。
    if !relocation_valid {
        state.relocation_target = None; // Presentation边沿及时归还新资产放置和数值输入的鼠标资格。
        state.relocation_intent = PlacementRelocationIntent::Move;
    }
    let target = selected_target(&browser, &session); // O(1)取得当前唯一完整记录身份。
    let record = target.as_ref().and_then(|target| {
        session
            .draft
            .as_ref()
            .filter(|draft| draft.map_id == target.map_id)
            .and_then(|draft| draft.placements.placements.get(target.record_index))
            .filter(|record| record.id == target.placement_id)
    });
    let visible = editor.open && record.is_some(); // 重复ID、在途签名和无选择都完整收起。
    let is_building = record.is_some_and(|record| record.building_preset_id.is_some());
    let is_cuboid = record.is_some_and(|record| record.asset_target.is_builtin());
    let has_size_recipe = record.is_some_and(|record| matches!(record.asset_target, MapPlacementAssetTarget::Cuboid { .. }));
    let locate_enabled = visible
        && !editor.close_requested
        && !editor.new_map_dialog_open
        && !editor.map_transition_in_progress
        && editor.pending_map_transition.is_none()
        && editor.map_switch_confirmation.is_none()
        && target
            .as_ref()
            .is_some_and(|target| editor.selected_map_id.as_ref() == Some(&target.map_id))
        && matches!(
            editor.tool,
            crate::types::EditorTool::Inspect | crate::types::EditorTool::PlaceAsset
        )
        && browser
            .selection
            .as_ref()
            .is_none_or(|selection| selection.valid && !selection.orphaned_water_binding)
        && (editor.tool != crate::types::EditorTool::Inspect
            || !candidates.has_viewport_selection()
            || matches!(candidates.selected.as_ref(), Some(crate::viewport::content_selection::LayerTarget::Placement(id))
                if record.is_some_and(|record| record.id == *id)));
    for (entity, disabled) in &locate_buttons {
        if locate_enabled == disabled {
            if locate_enabled {
                commands
                    .entity(entity)
                    .remove::<bevy::ui::InteractionDisabled>();
            } else {
                commands
                    .entity(entity)
                    .insert(bevy::ui::InteractionDisabled);
            }
        }
    } // 原聚焦按工具选择目标；此处只限制新按钮，避免资产属性里的“定位”聚焦到旧水体或传送点。
    let has_cutout = record.is_some_and(|record| {
        record
            .terrain_patch
            .as_ref()
            .is_some_and(|patch| patch.cutout.is_some())
    });
    for (slot, mut node) in &mut slots {
        let display = if (slot.0.is_cutout() && !has_cutout) || record.is_none_or(|record| !slot.0.supported_by(record)) {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (mut panel, section, cuboid_geometry) in &mut panels {
        let display = if visible && (!cuboid_geometry || has_size_recipe)
            && section.is_none_or(|section| record.is_some_and(|record| section.supported_by(record))) {
            Display::Flex // 选择稳定完整记录后显示全部七字段和动作。
        } else {
            Display::None // 无选择时不占场景页滚动高度。
        };
        if panel.display != display {
            panel.display = display;
        }
    }
    let summary = record.map_or_else(
        || "请选择场景树中的资产实例".to_owned(),
        |record| {
            let height_description = match record.height_mode {
                MapPlacementHeightMode::FollowTerrain => {
                    format!("跟随地形 · 离地 {:.3} m", record.terrain_offset) // 开发者直接核对真正保持的不变量。
                }
                MapPlacementHeightMode::FixedHeight => "固定高度".to_owned(), // 世界Y由位置字段显示，无需重复数值。
            };
            if let MapPlacementAssetTarget::Cuboid { collision, .. } = record.asset_target {
                format!("{height_description} · 盒碰撞{}", if collision { "开启" } else { "关闭" })
            } else if let MapPlacementAssetTarget::EditableCuboid { collision, .. } = record.asset_target {
                format!("{height_description} · 8点 / 12边 / 6面 · 网格碰撞{}", if collision { "开启" } else { "关闭" })
            } else if let MapPlacementAssetTarget::EditableMesh { geometry, collision, .. } = record.asset_target {
                format!("{height_description} · {}点 / {}边 / {}面 · 网格碰撞{}", geometry.vertex_count, geometry.edges().len(), geometry.face_count, if collision { "开启" } else { "关闭" })
            } else if record.building_preset_id.is_some() {
                "建筑高度由五点地基校验；请使用受检落点与旋转。".to_owned()
            } else if record.terrain_patch.is_some() {
                "三维地形 · 源局部位置/旋转/缩放 · 保存后应用洞口".to_owned()
            } else {
                height_description
            } // 身份与来源由概览承担，坐标由原公共字段实时回显。
        },
    );
    for mut text in &mut summaries {
        if text.as_str() != summary {
            **text = summary.clone(); // 选择或真实记录变化时才触发文本重排。
        }
    }

    let bounds = session
        .draft
        .as_ref()
        .and_then(|draft| record.and_then(|record| crate::authoring::placement_record_bounds(draft, record).ok())); // 补丁数值按源局部范围配置，容器不能截断合法源坐标。
    let context_editable = visible
        && !mesh_editing
        && !is_building
        && !session.saving
        && !editor.new_map_dialog_open
        && !state.relocation_armed(); // 建筑只能走五点地基受检动作，禁用七个普通任意数值入口。
    for (entity, field, current, disabled, mut config, mut scrub) in &mut inputs {
        let context_editable = context_editable && (!field.is_cutout() || has_cutout)
            && record.is_some_and(|record| field.supported_by(record));
        if context_editable == disabled {
            if context_editable {
                commands.entity(entity).remove::<NumericInputDisabled>(); // 恢复中键和左右拖动。
            } else {
                commands.entity(entity).insert(NumericInputDisabled); // 无选择、保存或吸附模式全部禁用。
            }
        }
        if active.entity != Some(entity) {
            let spec = field.spec(bounds); // 当前地图X/Z边界可能随切图变化。
            if config.0 != spec {
                config.0 = spec; // 只更新下一次精确输入，当前字符缓冲不换规则。
            }
        }
        if active_scrub.entity != Some(entity) {
            let next = field.scrub_config(&settings, bounds); // 轴范围和三类倍率共同形成下一次拖动配置。
            if *scrub != next {
                *scrub = next; // 当前物理拖动捕获期间不改变映射。
            }
        }
        if active.entity == Some(entity) || active_scrub.entity == Some(entity) {
            continue; // 共享 UI 控件拥有字符或指针时不得覆盖当前显示。
        }
        let value = record.map_or(0.0, |record| {
            round_to_decimals(field.value(record), field.decimals()) // 显示精度不反向量化作者高精度源。
        });
        if current.0.as_f32() != Some(value) {
            set_values.write(NumericInputSetValue {
                entity,                          // 只同步当前字段根。
                value: NumericValue::F32(value), // 选择、撤销和草稿换代后的权威值。
                cancel_edit: true,               // 外部变化必须清空旧字符缓冲。
            });
        }
    }

    for (interaction, button, mut node, mut background, mut border) in &mut buttons {
        let display = if (button.0 == PlacementTransformAction::RotateQuarterTurn && !is_building)
            || (button.0 == PlacementTransformAction::ToggleCuboidCollision && !is_cuboid)
            || (button.0 == PlacementTransformAction::FollowTerrain && record.is_some_and(|record| record.terrain_patch.is_some()))
            || (is_cuboid && matches!(button.0, PlacementTransformAction::ToggleTerrainPatch | PlacementTransformAction::ToggleTerrainCutout)) {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        let danger = button.0 == PlacementTransformAction::Delete; // 删除始终保留危险语义。
        let armed = button.0 == PlacementTransformAction::Relocate && state.relocation_armed(); // “吸附地面”等待视口落点期间持续高亮。
        let mode_selected = !is_building
            && record.is_some_and(|record| {
                matches!(
                    (button.0, record.height_mode),
                    (
                        PlacementTransformAction::FollowTerrain,
                        MapPlacementHeightMode::FollowTerrain
                    ) | (
                        PlacementTransformAction::FixedHeight,
                        MapPlacementHeightMode::FixedHeight
                    )
                ) // 高度模式始终有且只有一个按钮保持选中铜底。
            });
        let terrain_selected = record.is_some_and(|record| match button.0 {
            PlacementTransformAction::ToggleTerrainPatch => record.terrain_patch.is_some(),
            PlacementTransformAction::ToggleTerrainCutout => has_cutout,
            PlacementTransformAction::ToggleCuboidCollision => matches!(record.asset_target, MapPlacementAssetTarget::Cuboid { collision: true, .. } | MapPlacementAssetTarget::EditableCuboid { collision: true, .. } | MapPlacementAssetTarget::EditableMesh { collision: true, .. }),
            _ => false,
        });
        let selected = armed || mode_selected || terrain_selected; // 分类与洞口开关同时显示当前持久设置。
        let color = match (*interaction, danger, selected) {
            (Interaction::Pressed, true, _) => colors::DANGER_BG, // 删除按压保持红色而非普通铜色。
            (Interaction::Hovered, true, _) => colors::DANGER_BG, // 删除悬停强化危险底色。
            (Interaction::None, true, _) => colors::DANGER_BG,    // 删除常态仍可一眼辨认。
            (Interaction::Pressed, false, _) => colors::BUTTON_DEV_SMALL_PRESSED, // 普通动作按压反馈。
            (Interaction::Hovered, false, _) => colors::BUTTON_DEV_SMALL_HOVERED, // 普通动作悬停反馈。
            (Interaction::None, false, true) => colors::SELECT_BG, // 高度模式或等待落点持续显示选中铜底。
            (Interaction::None, false, false) => colors::BUTTON_DEV_SMALL_NORMAL, // 普通静止底色。
        };
        if background.0 != color {
            background.0 = color;
        }
        let border_color = BorderColor::all(if danger {
            colors::DANGER_BORDER // 删除使用红铜危险边。
        } else if selected || *interaction != Interaction::None {
            colors::COPPER_BORDER // 活动、hover和按压强化铜边。
        } else {
            colors::COPPER_BORDER_WEAK // 普通静止动作使用弱边。
        });
        if *border != border_color {
            *border = border_color;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::{IntoSystem, System}; // 初始化真实生产系统即可触发Bevy的B0001查询校验。

    use super::*; // 直接锁定完整资产属性Presentation参数组合。

    #[test] // 面板、摘要、数值根和动作按钮的可变组件访问必须保持互斥。
    fn placement_transform_sync_initializes_without_query_conflicts() {
        let mut world = World::new(); // 查询初始化不需要构造完整编辑器资源或UI实体。
        let mut system = IntoSystem::into_system(sync_placement_transform_panel); // 使用真实生产系统而非简化替身。
        system.initialize(&mut world); // 任一重叠可变组件访问都会在本行以B0001失败。
    }
}
