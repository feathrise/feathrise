//! 已摆放GLB网格的整网格水体属性面板、作者修改与即时正式材质预览。
//!
//! 【功能设计】Blender拥有拓扑、对象层级与UV；F12只给完整对象网格赋予水体语义和少量参数，
//! 不提供面级涂抹，也不生成第二套水Shader或河流几何。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN、UI交互、Commands与作者资源。
use bevy::text::FontSourceTemplate; // 属性面板沿用项目中文字体。
use feathrise_map_content::{
    MapId, MapPlacementRecord, MapPlacementWaterKind,
    MapPlacementWaterSurface, MapPlacementsAsset,
}; // 编辑器直接写入正式摆放文档中的整网格水体格式。
#[cfg(test)]
use feathrise_scene::MapPlacementRuntimeStatus; // 子模块回归继续通过原父模块作用域构造公开批次状态。
use feathrise_scene::{
    DynamicWaterMeshOverrideNumericPatch, DynamicWaterMeshOverrides, MapPlacementInstance,
    MapPlacementWaterEvidence,
}; // 未保存值直接重施到当前根，并只读接收正式批次身份。
use crate::ui::theme as colors; // 面板复用F12水墨、铜边与文字颜色。

use super::gltf_browser::{EditorGltfBrowserState, EditorGltfMeshSelection}; // 场景树提供完整失败关闭选择身份。
use super::scene::FONT_PATH; // 与右栏其他分组共享标题和字体口径。
use super::sidebars::MapEditorPlacementSelectionProperties; // 无网格选择时整区收起，格式说明归入分组问号。
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, WaterFileHandoff, }; // 修改进入统一草稿、保存门与有界水历史。
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState}; // 数值拖动鼠标所有权、作者状态与用户可见诊断。

mod numeric_input; // 流速与世界Y偏移复用公共精确输入、左右拖动和独立灵敏度。
mod presentation; // 属性文字与类别按钮回显独立于作者动作，保持主模块低于行数上限。
mod presentation_gate; // 属性文字只观察选择与正式活动门语义，不跟随浏览/候选进度Changed。
mod root_index; // placement根按生命周期建立O(1)作者写入索引，连续拖动不扫描8192个实例。
mod runtime_status; // 正式根Pending、Ready和Failed回执独立于作者动作与BSN构建。
mod selection_cache; // 三个Presentation消费者共享事件驱动的选中水绑定派生值。

use numeric_input::placement_water_numeric_input_slots; // 属性面板只组合两个固定NumericInput槽位。
pub(crate) use numeric_input::{
    PlacementWaterNumericInputState, apply_placement_water_numeric_scrub,
    handle_placement_water_numeric_input, setup_placement_water_numeric_inputs,
    sync_placement_water_numeric_inputs,
}; // lib.rs按Input与Presentation阶段注册专业数值交互。
pub(crate) use presentation::{sync_placement_water_button_styles, sync_placement_water_text}; // lib.rs继续通过placement_water原路径注册两个Presentation系统。
pub(crate) use root_index::{
    PlacementInstanceIndex, observe_placement_water_status_inserted,
    observe_placement_water_status_removed, register_placement_instance_root,
    unregister_placement_instance_root,
}; // 插件启动即覆盖正式、候选和作者预览根的完整生命周期。
pub(crate) use runtime_status::{
    PlacementWaterRuntimeSelectionCache, refresh_placement_water_runtime_selection_cache,
    sync_placement_water_runtime_status,
}; // 生命周期票据只定向聚合当前选择，不用Changed查询扫描8192根。
pub(crate) use selection_cache::{
    PlacementWaterSelectionCache, refresh_placement_water_selection_cache,
}; // 插件注册唯一派生缓存并在属性消费者前刷新。

const DEFAULT_MESH_WATER_FLOW_SPEED: f32 = 2.0; // 首次赋水即能在普通光照下观察沿UV下游移动。

/// 属性面板的一次整网格水体作者动作。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PlacementWaterAction {
    #[default]
    SetHorizontal,
    SetSloped,
    SetWaterfall,
    Clear,
}

/// 属性面板按钮使用独立动作组件，避免继续扩大全局工具枚举。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MapEditorPlacementWaterActionButton(pub(super) PlacementWaterAction);

/// 当前场景树网格、水体类别和参数的紧凑回显。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct MapEditorPlacementWaterText;

/// 构造与F12通用按钮一致的整网格水体属性按钮。
fn property_button(label: &str, action: PlacementWaterAction) -> impl Scene {
    bsn! {
        InteractionButton
        MapEditorPlacementWaterActionButton(action)
        Node {
            height: px(26),
            flex_shrink: 0.0,
            flex_grow: 1.0, // 同一属性行四个类别按钮严格等宽。
            flex_basis: px(0), // 中文标签长度不参与列宽计算。
            min_width: px(0), // 300px右栏内安全容纳四列。
            border: px(1),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(4)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(ui_metrics::FONT_CONTROL),
            }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

/// 右栏场景树下方的完整网格属性区；下方资产浏览器仍只负责选择和摆放源文件。
pub(super) fn placement_water_properties_panel() -> impl Scene {
    bsn! {
        MapEditorPlacementSelectionProperties
        Node {
            display: Display::None, // 无网格选择时完整属性区不参与场景页布局。
            width: percent(100),
            min_width: px(0),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::vertical(px(3)),
        }
        Children [
            (@crate::ui::context_help::section_header("网格水体", "数值框支持左拖实际值、右拖本字段灵敏度、中键精确输入，左右键单击无动作。UV0要求U沿下游按米递增、V横向0..1；局部水面请回Blender拆分为独立对象。资产浏览器只选择源文件和摆放；网格属性从场景树选择已摆放对象。失效绑定可用取消水体清除；名称不稳定时请在Blender显式命名对象与网格后重新导出。")),
            (
                MapEditorPlacementWaterText
                Text("请选择场景树中的Blender网格") @feathrise_editor_ui::localization::label_scene("请选择场景树中的Blender网格")
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_CONTROL),
                }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    padding: UiRect::all(px(4)), // 状态高度按实际身份与验证文字自然增长。
                }
            ),
            (
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(3),
                }
                Children [
                    (@property_button("水平水", PlacementWaterAction::SetHorizontal)),
                    (@property_button("斜坡水", PlacementWaterAction::SetSloped)),
                    (@property_button("瀑布", PlacementWaterAction::SetWaterfall)),
                    (@property_button("取消水体", PlacementWaterAction::Clear)),
                ]
            ),
            (@placement_water_numeric_input_slots()), // 两个字段分别绑定自己的值、拖动事务和持久灵敏度。
        ]
    }
}

pub(super) fn surface_index(
    record: &MapPlacementRecord,
    selection: &EditorGltfMeshSelection,
) -> Option<usize> {
    record.water_surfaces.iter().position(|surface| {
        surface.node_path == selection.node_path && surface.mesh_name == selection.mesh_name
    }) // 最终身份不读取可因Blender重导出而变化的数组索引。
}

/// 对单条摆放记录应用一次无ECS作者动作；返回是否产生真实变化。
fn apply_action_to_record(
    record: &mut MapPlacementRecord,
    selection: &EditorGltfMeshSelection,
    action: PlacementWaterAction,
) -> Result<bool, String> {
    if !record.editor.selectable() { return Err("对象已隐藏或锁定，请先在场景树解除保护".into()); }
    let before = record.water_surfaces.clone(); // 小型覆盖数组用于无变化判断，完整撤销由调用方保存记录快照。
    let existing = surface_index(record, selection);
    if action == PlacementWaterAction::Clear {
        let Some(index) = existing else {
            return Ok(false); // 普通网格再次取消不会污染撤销栈或作者修订。
        };
        record.water_surfaces.remove(index); // 即使当前GLB索引暂不可恢复，也允许按已保存稳定身份解除旧绑定。
        return Ok(true); // 删除既有合法项不可能制造新的格式错误。
    }
    if !selection.valid || selection.node_path.is_empty() || selection.mesh_name.trim().is_empty() {
        return Err("当前GLB节点或网格没有可持久化的显式名称".to_owned()); // 生成索引名不得冒充稳定Blender身份。
    }
    let mesh_index = selection
        .mesh_index
        .ok_or_else(|| "无法从当前GLB primitive恢复网格索引诊断值".to_owned())?;
    match action {
        PlacementWaterAction::SetHorizontal
        | PlacementWaterAction::SetSloped
        | PlacementWaterAction::SetWaterfall => {
            let kind = match action {
                PlacementWaterAction::SetHorizontal => MapPlacementWaterKind::Horizontal,
                PlacementWaterAction::SetSloped => MapPlacementWaterKind::Sloped,
                PlacementWaterAction::SetWaterfall => MapPlacementWaterKind::Waterfall,
                _ => unreachable!("上方分支只包含三类赋水动作"),
            };
            if let Some(index) = existing {
                let surface = &mut record.water_surfaces[index];
                surface.mesh_index = mesh_index; // 当前重导出索引只刷新诊断，不改变最终节点路径身份。
                surface.kind = kind;
            } else {
                record.water_surfaces.push(MapPlacementWaterSurface {
                    mesh_index,
                    node_path: selection.node_path.clone(),
                    mesh_name: selection.mesh_name.clone(),
                    kind,
                    flow_speed: DEFAULT_MESH_WATER_FLOW_SPEED,
                    height_offset: 0.0,
                }); // 首次赋水保留Blender原高度并使用明确可见的默认流速。
            }
        }
        PlacementWaterAction::Clear => unreachable!("清除动作已在稳定身份预检前独立处理"),
    }
    record.water_surfaces.sort_by(|left, right| {
        left.node_path
            .cmp(&right.node_path)
            .then_with(|| left.mesh_name.cmp(&right.mesh_name))
    }); // JSON、差分和测试保持确定顺序，不按易漂移mesh_index排序。
    if let Err(error) = record.validate_water_surfaces() {
        record.water_surfaces = before; // 任一公共校验失败都完整回滚本次临时修改，不能留下无撤销、未标脏的半操作。
        return Err(error);
    }
    Ok(record.water_surfaces != before)
}

fn kind_label(kind: MapPlacementWaterKind) -> &'static str {
    match kind {
        MapPlacementWaterKind::Horizontal => "水平水",
        MapPlacementWaterKind::Sloped => "斜坡水",
        MapPlacementWaterKind::Waterfall => "瀑布",
    }
}

/// 以完整结构签名下标原子应用一次赋水动作，并按缓存差值执行全图公共绑定预算校验。
fn apply_action_to_placements(
    placements: &mut MapPlacementsAsset,
    placement_index: usize,
    current_surface_count: usize,
    selection: &EditorGltfMeshSelection,
    action: PlacementWaterAction,
) -> Result<Option<(MapPlacementRecord, MapPlacementRecord, usize)>, String> {
    let record = placements
        .placements
        .get_mut(placement_index)
        .filter(|record| record.id == selection.placement_id)
        .ok_or_else(|| "选中的摆放实例索引已经不在当前草稿结构中".to_owned())?;
    let before = record.clone();
    if !apply_action_to_record(record, selection, action)? {
        return Ok(None); // 重复类别或无目标的清除不制造空历史。
    }
    let Some(next_surface_count) = current_surface_count
        .checked_sub(before.water_surfaces.len())
        .and_then(|count| count.checked_add(record.water_surfaces.len()))
    else {
        *record = before; // 缓存不变量损坏也不能留下已经修改但没有历史/dirty证据的半操作。
        return Err("网格水体全图绑定计数缓存损坏".to_owned());
    };
    let after = record.clone();
    Ok(Some((before, after, next_surface_count))) // 单项公共校验与O(1)全图预算都通过才发布。
}

/// 把当前草稿覆盖重施到编辑器预览根或已原子采用的正式根。
fn placement_root_is_author_writable(
    instance_map_id: &MapId, // 根创建时冻结的地图身份。
    instance_id: &str,       // 根创建时冻结的完整摆放稳定ID。
    is_active: bool,         // 根共享批次门的Acquire读取结果。
    current_map_id: &MapId,  // 当前作者草稿地图是跨图写入的唯一信用。
    target_id: &str,         // 当前完整摆放记录的稳定ID。
) -> bool {
    instance_map_id == current_map_id && is_active && instance_id == target_id // 物理Pending标记可能分帧残留，只有共享活动门是整批作者身份。
}

/// 精确预览表也必须经过与活动根索引相同的地图、活动门和稳定ID复核。
fn precise_author_preview_root(
    session: &EditorAuthoringSession, // 精确实体映射来自当前F12作者会话。
    current_map_id: &MapId,           // 调用方已经取得当前草稿地图身份。
    placement_roots: &Query<(Entity, &MapPlacementInstance)>, // 查询根的不可伪造运行时身份。
    placement_id: &str,               // 当前要写入的完整摆放稳定ID。
) -> Option<Entity> {
    let entity = session.placement_previews.get(placement_id).copied()?; // 没有精确预览时回退活动正式根索引。
    placement_roots
        .get(entity)
        .ok()
        .filter(|(_, instance)| {
            placement_root_is_author_writable(
                instance.map_id(),
                instance.id(),
                instance.is_active(),
                current_map_id,
                placement_id,
            ) // 精确实体映射不能跳过活动索引使用的三重身份门。
        })
        .map(|(entity, _)| entity) // 只有三重身份全部匹配才允许跳过索引回退。
}

/// 同批替换当前水数组与非空保存证据标记，等待帧Query不会扫描普通placement根。
fn write_placement_water_override(
    commands: &mut Commands,
    entity: Entity,
    surfaces: &[MapPlacementWaterSurface],
) {
    let mut root_commands = commands.entity(entity); // 同一个延迟命令目标保持覆盖和marker原子可见。
    root_commands.insert(DynamicWaterMeshOverrides::new(surfaces.to_vec())); // 当前完整小型作者数组触发唯一运行时预检。
    if surfaces.is_empty() {
        root_commands.remove::<MapPlacementWaterEvidence>(); // 清空后普通根退出保存等待查询。
    } else {
        root_commands.insert(MapPlacementWaterEvidence); // 新增首项绑定后根进入最多256项证据集合。
    }
}

pub(super) fn preview_placement_overrides(
    commands: &mut Commands,
    session: &EditorAuthoringSession,
    root_index: &PlacementInstanceIndex,
    placement_roots: &Query<(Entity, &MapPlacementInstance)>,
    record: &MapPlacementRecord,
) {
    let Some(draft) = session.draft.as_ref() else {
        return; // 没有当前地图身份时，精确预览引用也不能绕过跨图写入保护。
    };
    preview_placement_overrides_for_map(
        commands,
        session,
        root_index,
        placement_roots,
        &draft.map_id,
        record,
    ); // 生产入口只允许当前草稿提供跨图写入信用。
}

/// 以调用方已复核的当前地图身份执行精确预览或活动正式根回退。
fn preview_placement_overrides_for_map(
    commands: &mut Commands,             // 同批插入完整覆盖与非空证据marker。
    session: &EditorAuthoringSession,    // 精确预览表仍只是候选，不自带地图信用。
    root_index: &PlacementInstanceIndex, // 精确候选失效后定向回退当前图同ID活动根。
    placement_roots: &Query<(Entity, &MapPlacementInstance)>, // 所有候选都复核不可伪造的根身份。
    current_map_id: &MapId,              // 只能由当前草稿或定向测试夹具提供。
    record: &MapPlacementRecord,         // 要即时重施的完整单条摆放记录。
) {
    if let Some(entity) =
        precise_author_preview_root(session, current_map_id, placement_roots, &record.id)
    {
        write_placement_water_override(commands, entity, &record.water_surfaces); // 精确预览根同步覆盖与非空证据。
        return; // 只有真实写入精确预览后才阻止回退到活动正式根。
    }
    root_index.for_each_active_root(current_map_id, &record.id, placement_roots, |entity| {
        write_placement_water_override(commands, entity, &record.water_surfaces)
    }); // 稳定ID只检查少量并存代次，不再枚举整张地图全部根。
}

/// 拖动每个吸附档只向目标根插入一次性数值补丁，不复制最多六十四项完整水绑定数组。
pub(super) fn preview_placement_numeric_patch(
    commands: &mut Commands,             // 延迟插入scene侧受控一次性补丁组件。
    session: &EditorAuthoringSession,    // 当前草稿地图和精确预览根提供作者身份。
    root_index: &PlacementInstanceIndex, // 没有精确预览时按同图稳定ID定位小型活动根集合。
    placement_roots: &Query<(Entity, &MapPlacementInstance)>, // 每个候选根仍复核不可伪造活动门。
    placement_id: &str,                  // 拖动开始时冻结并持续复核的完整摆放稳定ID。
    patch: DynamicWaterMeshOverrideNumericPatch, // Copy补丁只含数组下标、字段枚举和一个f32。
) {
    let Some(draft) = session.draft.as_ref() else {
        return; // 没有当前地图身份时即使精确实体仍在也拒绝跨图或过期写入。
    };
    if let Some(entity) =
        precise_author_preview_root(session, &draft.map_id, placement_roots, placement_id)
    {
        commands.entity(entity).insert(patch); // 精确预览根直接消费O(1)补丁且不改变非空证据。
        return; // 已命中当前作者预览时不重复写活动正式根。
    }
    root_index.for_each_active_root(&draft.map_id, placement_id, placement_roots, |entity| {
        commands.entity(entity).insert(patch); // 异常重复活动代次沿用整批覆盖的全部重施语义。
    }); // fallback同样由map_id + active + id三重门保护，不扫描全图根。
}

/// 消费属性按钮，把完整前后摆放记录压入统一撤销栈并即时刷新现有水材质。
pub(crate) fn handle_placement_water_actions(
    mut commands: Commands,
    buttons: Query<(&EditorButtonClick, &MapEditorPlacementWaterActionButton), Changed<EditorButtonClick>>,
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>, // 数值和弦期间只推进按钮Changed tick，不改摆放草稿。
    browser: Res<EditorGltfBrowserState>,
    root_index: Res<PlacementInstanceIndex>, // 稳定ID直接定位少量并存根，按钮动作不扫描全图placement。
    placement_roots: Query<(Entity, &MapPlacementInstance)>,
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    handoff: Res<WaterFileHandoff>, // 保存后正式候选采用前只冻结网格水作者值。
) {
    if numeric_scrub_mouse.blocked() {
        return; // 被数值拖动使用过的类别按钮边沿就地丢弃，不能在解锁后补执行。
    }
    let placement_handoff_in_flight = session
        .draft
        .as_ref()
        .is_some_and(|draft| handoff.placement_handoff_in_flight_for(&draft.map_id)); // Input早于Tasks运行，完成证据最多多冻结一帧，不会同帧污染候选。
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        if session.saving {
            editor.status = "保存事务完成前不能修改网格水体".to_owned();
            continue;
        }
        if placement_handoff_in_flight {
            editor.status = "已保存的正式摆放正在原子交接；完成前仅暂停网格水编辑".to_owned();
            continue; // 其他作者功能不受影响，本次保存的隐藏候选保持冻结快照。
        }
        let Some(selection) = browser.selection.clone() else {
            editor.status = "请先在右上场景树选中一个Blender网格".to_owned();
            continue;
        };
        let Some(placement_index) = browser.placement_record_index(
            &selection.placement_id,
            session.placement_structure_revision,
        ) else {
            editor.status = "场景树结构正在换代，完成后再修改网格水体".to_owned();
            continue; // 旧数组下标绝不跨摆放增删或换源复用。
        };
        let result = (|| {
            let draft = session
                .draft
                .as_mut()
                .ok_or_else(|| "地图作者草稿尚未就绪".to_owned())?;
            let current_surface_count = draft.placement_water_surface_count;
            let result = apply_action_to_placements(
                &mut draft.placements,
                placement_index,
                current_surface_count,
                &selection,
                button.0,
            )?;
            if let Some((_, _, next_surface_count)) = result.as_ref() {
                draft.placement_water_surface_count = *next_surface_count; // 只有成功动作推进缓存。
            }
            Ok::<_, String>(result)
        })();
        let Some((before, after, _)) = (match result {
            Ok(value) => value,
            Err(error) => {
                editor.status = format!("网格水体修改失败：{error}");
                continue;
            }
        }) else {
            editor.status = "当前网格水体属性没有变化".to_owned();
            continue;
        };
        preview_placement_overrides(
            &mut commands,
            &session,
            &root_index,
            &placement_roots,
            &after,
        ); // 同帧发布当前完整覆盖；材质系统随后原子恢复并重施。
        let session = &mut *session; // 显式拆借两个历史字段并复用统一水历史硬门。
        session.push_bounded_water_history(EditorCommand::PlacementChanged {
            record_index: placement_index, // 当前完整结构签名已经复核ID，下次撤销无需扫描8192条记录。
            before,
            after: after.clone(),
        }); // 完整记录保证撤销不遗漏同对象其他网格，并受命令数与内存预算裁剪。
        session.mark_placement_structure_changed(&mut editor); // 类别、绑定增删或诊断索引变化会改变GLB树提示。
        editor.status = if let Some(index) = surface_index(&after, &selection) {
            let surface = &after.water_surfaces[index];
            format!(
                "已提交{}：{}，流速{:.2}，高度偏移{:+.2}m；正在验证Scene0与动态水材质",
                selection.mesh_name,
                kind_label(surface.kind),
                surface.flow_speed,
                surface.height_offset,
            )
        } else {
            format!(
                "已提交取消{}的水体语义；正在验证并恢复Blender原材质",
                selection.mesh_name
            )
        };
    }
}

#[cfg(test)]
mod tests; // 纯作者动作回归拆出，保持属性面板与ECS系统主文件低于模块上限。
