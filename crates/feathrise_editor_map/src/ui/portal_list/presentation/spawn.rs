//! 当前地图与目标地图两套传送点完整列表的行实体维护。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use std::ops::Range; // 行数增长时只追加缺少的连续扁平索引。

use bevy::prelude::*; // Commands、UI组件、AssetServer与变化检测。
use feathrise_map_content::{MapContentRegistry, MapId, MapManifestAsset, MapPortalsAsset}; // 行数身份由当前草稿、目标地图目录和两份目标资产共同决定。
use crate::ui::theme as colors; // 动态行复用统一槽位、铜边和文本颜色。

use crate::authoring::EditorAuthoringSession; // 当前地图与同图目标优先读取未保存草稿。
use crate::components::{
    MapEditorCurrentPortalList, MapEditorCurrentPortalRow, MapEditorCurrentPortalRowText,
    MapEditorTargetPortalList, MapEditorTargetPortalRow, MapEditorTargetPortalRowText,
}; // 两类行使用不同marker，防止输入和Presentation串写。
use crate::resources::{MapBrowserState, MapEditorState}; // 地图树高亮决定唯一目标地图。
use super::super::navigation::PortalSidebarNavigation; // 隐藏页不维护不可见行实体，Select中的传送页保留完整列表。

use super::super::helpers::{TargetPortalDocuments, selected_target_map, target_portal_documents}; // 目标行数必须与Presentation采用同一完整文档判定。
use super::PORTAL_ROW_HEIGHT; // 行实体高度与完整列表容器公式保持一致。

/// 单帧最多增删的传送列表行数；超量列表跨帧补齐，避免作者数据规模直接形成UI尖峰。
const PORTAL_ROW_ENTITY_CHANGES_PER_FRAME: usize = 32;

/// 动态行实体结构修订；只在本帧实际排队增删行时推进，唤醒两套Presentation绑定新行。
#[derive(Resource, Default)]
pub(crate) struct PortalRowEntityRevision(u64);

/// 行实体数量只跟踪会改变两套列表结构的常数大小身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortalRowEntitySignature {
    open: bool,                    // 再次打开时按当时数据重建实体数量。
    visible: bool,                 // 切入传送页时才创建可见列表行。
    current_map_id: Option<MapId>, // 草稿换图即使修订归零也必须重新核对。
    portal_revision: u64,          // 当前或同图目标的传送结构变化时刷新。
    target_map_id: Option<MapId>,  // 地图树高亮地图决定目标列表身份。
    catalog_revision: u64,         // 目录换代可能替换目标地图资产句柄。
}

/// 构造动态行实体专用签名；选择高亮和名称输入不会触发行数核对。
fn portal_row_entity_signature(
    editor: &MapEditorState,
    browser: &MapBrowserState,
    session: &EditorAuthoringSession,
    registry: Option<&MapContentRegistry>,
    visible: bool,
) -> PortalRowEntitySignature {
    PortalRowEntitySignature {
        open: editor.open, // 关闭期间不为不可见列表创建实体。
        visible, // 双列表跟随工具页，不依赖会被Select门改写的editor.tool。
        current_map_id: session.draft.as_ref().map(|draft| draft.map_id.clone()), // 当前作者地图稳定身份。
        portal_revision: session.portal_revision, // 只由传送结构或目标修改推进。
        target_map_id: selected_target_map(browser).cloned(), // 根和虚拟文件夹稳定映射为空。
        catalog_revision: registry.map_or(0, |registry| registry.revision), // 目录未就绪使用零代次。
    }
}

/// 在真实数量变化时增减列表尾部行实体；稳定帧不扫描Children，也不产生Commands。
#[allow(clippy::too_many_arguments)] // 目标数量必须从完整目录、清单和传送图联合解析。
pub(crate) fn spawn_portal_list_rows(
    mut commands: Commands,
    context: (Res<MapEditorState>, PortalSidebarNavigation), // 固定导航与工具合并为一个系统参数，不超过Bevy参数数量上限。
    browser: Res<MapBrowserState>,
    session: Res<EditorAuthoringSession>,
    registry: Option<Res<MapContentRegistry>>,
    manifests: Res<Assets<MapManifestAsset>>,
    portal_assets: Res<Assets<MapPortalsAsset>>,
    asset_server: Res<AssetServer>,
    current_containers: Query<(Entity, Option<&Children>), With<MapEditorCurrentPortalList>>,
    target_containers: Query<(Entity, Option<&Children>), With<MapEditorTargetPortalList>>,
    added_current_containers: Query<(), Added<MapEditorCurrentPortalList>>,
    added_target_containers: Query<(), Added<MapEditorTargetPortalList>>,
    current_rows: Query<&MapEditorCurrentPortalRow>,
    target_rows: Query<&MapEditorTargetPortalRow>,
    mut last_signature: Local<Option<PortalRowEntitySignature>>,
    mut row_entity_revision: ResMut<PortalRowEntityRevision>,
) {
    let (editor, navigation) = context;
    let signature = portal_row_entity_signature(
        &editor,
        &browser,
        &session,
        registry.as_deref(),
        navigation.visible(&editor),
    ); // 先冻结结构身份用于稳定帧短路。
    let assets_changed = manifests.is_changed() || portal_assets.is_changed(); // 异步Ready和热重载可能改变目标行数。
    let container_added =
        !added_current_containers.is_empty() || !added_target_containers.is_empty(); // BSN重建必须补齐新容器。
    if last_signature.as_ref() == Some(&signature) && !assets_changed && !container_added {
        return; // 光标、相机和行悬停不会线性扫描四十三项或更多条目。
    }
    if !signature.visible {
        *last_signature = Some(signature); // 切回传送页时身份变化会执行一次完整核对。
        return;
    }

    let current_count = session
        .draft
        .as_ref()
        .map_or(0, |draft| draft.portals.portals.len()); // 当前列表严格展示草稿中的全部源点。
    let target_count = selected_target_map(&browser).map_or(0, |target_map_id| {
        match target_portal_documents(
            target_map_id,       // 只解析地图树当前高亮的一张真实地图。
            &session,            // 同图目标包含尚未保存的全部源点。
            registry.as_deref(), // 其他地图读取正式目录强句柄。
            &manifests,          // 默认出生点要求清单完整Ready。
            &portal_assets,      // 具体端点要求传送图完整Ready。
        ) {
            TargetPortalDocuments::Ready { portals, .. } => portals.portals.len() + 1, // 首行固定为安全默认出生点。
            TargetPortalDocuments::Loading | TargetPortalDocuments::Missing => 0, // 半套文档不创建可误点的行。
        }
    });
    let font = feathrise_editor_core::editor_font(&asset_server, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"); // 本次结构变化的新增行共享中文字体句柄。

    let mut remaining_changes = PORTAL_ROW_ENTITY_CHANGES_PER_FRAME; // 两套列表共享同一个明确帧预算。
    let initial_remaining_changes = remaining_changes; // 是否实际排队Commands由预算差值确定，不额外扫描实体。
    let mut complete = true; // 所有容器达到目标数量后才允许签名进入稳定短路。
    for (container, children) in &current_containers {
        complete &= reconcile_current_rows(
            &mut commands,
            container,
            children,
            &current_rows,
            current_count,
            &font,
            &mut remaining_changes,
        ); // 只追加或删除尾部差额，既有行实体和Interaction保持稳定。
    }
    for (container, children) in &target_containers {
        complete &= reconcile_target_rows(
            &mut commands,
            container,
            children,
            &target_rows,
            target_count,
            &font,
            &mut remaining_changes,
        ); // 目标地图切换但数量相同时复用现有实体，由后续Presentation改绑身份。
    }
    if remaining_changes != initial_remaining_changes {
        row_entity_revision.0 = row_entity_revision.0.wrapping_add(1); // 下一组Presentation系统同帧看见新结构。
    }
    *last_signature = complete.then_some(signature); // 未补齐时保留非稳定态，下一帧继续消费固定预算。
}

/// 把当前地图行实体调整为从零开始的连续完整索引。
fn reconcile_current_rows(
    commands: &mut Commands,
    container: Entity,
    children: Option<&Children>,
    rows: &Query<&MapEditorCurrentPortalRow>,
    expected: usize,
    font: &Handle<Font>,
    remaining_changes: &mut usize,
) -> bool {
    let mut existing = Vec::new(); // 只在结构身份变化时暂存当前容器的少量行实体。
    if let Some(children) = children {
        for child in children.iter() {
            if let Ok(row) = rows.get(child) {
                existing.push((child, row.slot)); // Children顺序就是最终纵向显示顺序。
            }
        }
    }
    if !rows_form_ordered_prefix(&existing) {
        let change_count = existing.len().min(*remaining_changes); // 异常修复同样不得越过共享帧预算。
        let keep_count = existing.len().saturating_sub(change_count); // 从尾部回收才能逐步恢复连续前缀。
        for (entity, _) in existing.iter().skip(keep_count) {
            commands.entity(*entity).try_despawn(); // 异常重复或断号时按预算清理损坏行及其文字子项。
        }
        *remaining_changes -= change_count; // 本帧其他列表只能使用剩余预算。
        return keep_count == 0 && expected == 0; // 非空目标等下一帧确认旧行清空后再连续重建。
    }
    let existing_count = existing.len(); // 记录Commands采用前的稳定前缀长度。
    if existing_count > expected {
        let change_count = (existing_count - expected).min(*remaining_changes); // 大列表删除跨帧回收。
        let keep_count = existing_count - change_count; // 只删尾部，剩余行仍严格对应零起索引。
        for (entity, _) in existing.into_iter().skip(keep_count) {
            commands.entity(entity).try_despawn(); // 删除作者项时只回收列表尾部多余行。
        }
        *remaining_changes -= change_count; // 与目标列表共享本帧结构变更预算。
        keep_count == expected // 仍有差额时下一帧继续从尾部回收。
    } else if existing_count < expected {
        let change_count = (expected - existing_count).min(*remaining_changes); // 大列表新增跨帧补齐。
        let next_count = existing_count + change_count; // 新行继续保持连续扁平索引。
        spawn_current_row_range(commands, container, existing_count..next_count, font); // 只补本帧有预算的尾部差额。
        *remaining_changes -= change_count; // 目标列表只能消费剩余行数。
        next_count == expected // 仍有差额时不发布稳定签名。
    } else {
        true // 数量与顺序都已满足，无需产生Commands。
    }
}

/// 把目标地图行实体调整为从默认出生点开始的连续完整索引。
fn reconcile_target_rows(
    commands: &mut Commands,
    container: Entity,
    children: Option<&Children>,
    rows: &Query<&MapEditorTargetPortalRow>,
    expected: usize,
    font: &Handle<Font>,
    remaining_changes: &mut usize,
) -> bool {
    let mut existing = Vec::new(); // 只在目标结构或资产状态变化时读取当前Children。
    if let Some(children) = children {
        for child in children.iter() {
            if let Ok(row) = rows.get(child) {
                existing.push((child, row.slot)); // 零槽固定代表默认出生点。
            }
        }
    }
    if !rows_form_ordered_prefix(&existing) {
        let change_count = existing.len().min(*remaining_changes); // 异常目标结构也遵守同一帧预算。
        let keep_count = existing.len().saturating_sub(change_count); // 从尾部开始回收避免扩大错序范围。
        for (entity, _) in existing.iter().skip(keep_count) {
            commands.entity(*entity).try_despawn(); // 损坏结构按预算回收，不让显示顺序与扁平索引永久错位。
        }
        *remaining_changes -= change_count; // 当前地图列表已经消费的预算不会被重复使用。
        return keep_count == 0 && expected == 0; // 非空目标在旧损坏行全部清理后的下一帧重建。
    }
    let existing_count = existing.len(); // 冻结本帧Commands采用前的目标行数。
    if existing_count > expected {
        let change_count = (existing_count - expected).min(*remaining_changes); // 资产缩短跨帧回收。
        let keep_count = existing_count - change_count; // 尾部差额先退出，不破坏默认出生点首行。
        for (entity, _) in existing.into_iter().skip(keep_count) {
            commands.entity(entity).try_despawn(); // 目标资产缩短或离开Ready时回收尾部行。
        }
        *remaining_changes -= change_count; // 更新两套列表共享的剩余预算。
        keep_count == expected // 尚未缩到目标数量时保持非稳定签名。
    } else if existing_count < expected {
        let change_count = (expected - existing_count).min(*remaining_changes); // Ready大列表按固定预算补齐。
        let next_count = existing_count + change_count; // 新增范围保持默认点加具体端点的连续顺序。
        spawn_target_row_range(commands, container, existing_count..next_count, font); // 只创建本帧允许的真实落点行。
        *remaining_changes -= change_count; // 后续容器只能使用剩余预算。
        next_count == expected // 超过预算时下一帧继续追加。
    } else {
        true // 目标数量与顺序已经完整，无需改变实体。
    }
}

/// 验证Children中的行marker严格形成`0..len`前缀，防止异常实体顺序造成错绑。
fn rows_form_ordered_prefix(existing: &[(Entity, usize)]) -> bool {
    existing
        .iter()
        .enumerate()
        .all(|(expected, (_, actual))| expected == *actual) // 正常增删只维护尾部，不需要重建既有行。
}

/// 向当前地图列表尾部追加一段连续行实体。
fn spawn_current_row_range(
    commands: &mut Commands,
    container: Entity,
    slots: Range<usize>,
    font: &Handle<Font>,
) {
    commands.entity(container).with_children(|parent| {
        for slot in slots {
            parent
                .spawn((
                    InteractionButton, // Interaction直接挂在真实源点行，不依赖实体名称分派。
                    MapEditorCurrentPortalRow {
                        slot,            // 扁平索引覆盖完整作者数组。
                        portal_id: None, // Presentation刷新前不得持有虚假源点身份。
                    },
                    portal_row_node(), // 两类行使用同一明确点击尺寸。
                    BackgroundColor(colors::SLOT_BG), // 普通源点使用暗墨背景。
                    BorderColor::all(colors::COPPER_BORDER_WEAK), // 弱铜边表达可点击边界。
                ))
                .with_child((
                    MapEditorCurrentPortalRowText,   // 子文本只由当前列表Presentation更新。
                    Text::new(""),                   // 结构命令采用前保持无文字。
                    portal_row_font(font.clone()),   // 新增行共享同一字体资产。
                    TextColor(colors::TEXT_PRIMARY), // 普通源点使用米白文字。
                    TextLayout::no_wrap(),
                    portal_row_text_node(), // 行内短名始终单行裁切，长中文不得挤到下一控件。
                ));
        }
    });
}

/// 向目标地图列表尾部追加一段连续行实体。
fn spawn_target_row_range(
    commands: &mut Commands,
    container: Entity,
    slots: Range<usize>,
    font: &Handle<Font>,
) {
    commands.entity(container).with_children(|parent| {
        for slot in slots {
            parent
                .spawn((
                    InteractionButton, // 目标落点按下时显式发布完整MapPortalTarget。
                    MapEditorTargetPortalRow {
                        slot,         // 零槽为默认出生点，其后完整对应具体端点。
                        target: None, // 资产回显前不继承旧地图目标。
                    },
                    portal_row_node(), // 目标行与当前行保持相同可点击面积。
                    BackgroundColor(colors::SLOT_BG), // 普通目标使用暗墨背景。
                    BorderColor::all(colors::COPPER_BORDER_WEAK), // 弱铜边保持分组层级。
                ))
                .with_child((
                    MapEditorTargetPortalRowText,    // 子文本只由目标列表Presentation更新。
                    Text::new(""),                   // 首次同步前保持空白。
                    portal_row_font(font.clone()),   // 默认出生点和具体端点共用字体。
                    TextColor(colors::TEXT_PRIMARY), // 未选目标使用米白文字。
                    TextLayout::no_wrap(),
                    portal_row_text_node(),
                ));
        }
    });
}

/// 构造传送行的共享布局；每次调用只创建普通值，不分配资产。
fn portal_row_node() -> Node {
    Node {
        display: Display::None,        // Presentation绑定真实对象后才恢复布局和命中。
        width: percent(100),           // 使用玩法页正文完整宽度。
        height: px(PORTAL_ROW_HEIGHT), // 与容器高度公式共享24px。
        flex_shrink: 0.0,              // 低窗口由外层滚动，不压扁点击区域。
        align_items: AlignItems::Center, // 单行文字垂直居中。
        padding: UiRect::horizontal(px(4)), // 铜边内保留紧凑安全距离。
        border: UiRect::all(px(1)),    // 一像素边界不抢主按钮层级。
        overflow: Overflow::clip_x(),  // 长标签和MapId不撑宽左栏。
        ..default()                    // 其余采用Bevy稳定默认布局。
    }
}

fn portal_row_text_node() -> Node {
    Node { width: percent(100), min_width: px(0), overflow: Overflow::clip_x(), ..default() }
}

/// 构造传送行共享字体组件；句柄克隆保持O(1)。
fn portal_row_font(font: Handle<Font>) -> TextFont {
    TextFont {
        font: font.into(),
        font_size: ui_metrics::FONT_CONTROL.into(), // 与水域对象行保持同一信息密度。
        ..default()            // 字重和抗锯齿沿用项目字体默认值。
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin; // 正式字体句柄路径需要最小AssetServer资源。

    use super::*; // 直接验证生产行生成器不再把完整列表截断成四项。

    /// 测试系统使用产品共享帧预算逐步补齐截图中的四十三条真实行。
    fn reconcile_forty_three_current_rows(
        mut commands: Commands,
        containers: Query<(Entity, Option<&Children>), With<MapEditorCurrentPortalList>>,
        rows: Query<&MapEditorCurrentPortalRow>,
        asset_server: Res<AssetServer>,
    ) {
        let font = feathrise_editor_core::editor_font(&asset_server, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"); // 句柄无需等待字体内容Ready即可挂载UI。
        let mut remaining_changes = PORTAL_ROW_ENTITY_CHANGES_PER_FRAME; // 与产品系统使用同一共享预算。
        for (container, children) in &containers {
            reconcile_current_rows(
                &mut commands,
                container,
                children,
                &rows,
                43,
                &font,
                &mut remaining_changes,
            ); // 不分页，但首帧只创建预算允许的连续前缀。
        }
    }

    /// 返回当前测试World中已经生成的源点行槽位。
    fn current_row_slots(world: &mut World) -> Vec<usize> {
        let mut rows = world.query::<&MapEditorCurrentPortalRow>(); // QueryState只读取行marker。
        let mut slots = rows.iter(world).map(|row| row.slot).collect::<Vec<_>>(); // ECS迭代顺序不作为显示顺序依据。
        slots.sort_unstable(); // 通过扁平索引验证完整连续集合。
        slots
    }

    #[test]
    fn current_portal_rows_generate_all_forty_three_entries_in_order() {
        let mut app = App::new(); // 最小ECS应用足以执行真实Commands父子关系生成。
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Font>() // AssetServer分配正式字体句柄前必须登记字体资产类型。
            .add_systems(Update, reconcile_forty_three_current_rows); // 使用产品中的同一有界增量函数。
        app.world_mut().spawn(MapEditorCurrentPortalList); // 模拟BSN创建当前地图列表容器。

        app.update(); // 首帧Deferred Commands只采用固定预算允许的行与文字子项。
        assert_eq!(
            current_row_slots(app.world_mut()),
            (0..PORTAL_ROW_ENTITY_CHANGES_PER_FRAME).collect::<Vec<_>>(),
        ); // 四十三项不会被误当成固定四行，但也不能单帧无上限创建。
        app.update(); // 第二帧继续从连续前缀尾部补齐剩余十一项。

        assert_eq!(
            current_row_slots(app.world_mut()),
            (0..43).collect::<Vec<_>>(),
        ); // 最终不得漏掉任何真实索引。
    }

    #[test]
    fn row_prefix_validation_rejects_gaps_and_reordering() {
        let first = Entity::from_raw_u32(1).expect("测试实体编号有效"); // 实体身份只用于占位，不访问World。
        let second = Entity::from_raw_u32(2).expect("测试实体编号有效"); // 第二个占位实体保持唯一。
        assert!(rows_form_ordered_prefix(&[(first, 0), (second, 1)])); // 连续Children顺序可安全增量维护尾部。
        assert!(!rows_form_ordered_prefix(&[(first, 1), (second, 0)])); // 错序时必须走完整修复路径。
        assert!(!rows_form_ordered_prefix(&[(first, 0), (second, 2)])); // 断号时不得把显示位置错绑到数据索引。
    }
}
