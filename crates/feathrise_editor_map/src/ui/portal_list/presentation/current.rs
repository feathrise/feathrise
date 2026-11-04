//! 当前已进入地图全部传送点的增量回显与选择反馈。

use bevy::prelude::*; // UI查询、资源变化检测和动态行组件。
use feathrise_map_content::{MapContentRegistry, MapId, MapManifestAsset, MapPortalsAsset}; // 当前行从目标清单派生地图名，稳定ID只参与内部查找。
use crate::ui::theme as colors; // 选中源点使用统一铜金配色。

use crate::authoring::EditorAuthoringSession; // 当前列表只读取未保存作者草稿。
use crate::components::{
    MapEditorCurrentPortalActions, MapEditorCurrentPortalList, MapEditorCurrentPortalListTitle,
    MapEditorCurrentPortalRow, MapEditorCurrentPortalRowText, MapEditorPortalSelectionPanel,
}; // 标题、容器、行与操作区查询显式互斥。
use crate::resources::MapEditorState; // 工具和当前选择属于编辑会话态。
use super::super::navigation::PortalSidebarNavigation; // Select仍显示当前传送页的已有源点。

use super::super::helpers::PortalDisplayContext;
use super::{PortalRowEntityRevision, fit_portal_list, portal_row_background};

/// 当前列表只跟踪与源点回显有关的常数大小身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CurrentPortalSignature {
    open: bool,                         // 再次打开时需要重建可见内容。
    visible: bool,                      // 只在传送页可见时维护列表；Inspect不再收起属性。
    map_id: Option<MapId>,              // 草稿换图即使修订归零也必须刷新。
    portal_revision: u64,               // 新增、删除、撤销后刷新源点行。
    catalog_revision: u64,              // 目标地图改名或目录换代后重新派生“前往{地图名}”。
    selected_portal_id: Option<String>, // 当前源点选择只影响本列表高亮。
}

fn current_signature(
    editor: &MapEditorState,               // 读取工具与当前源点选择身份。
    session: &EditorAuthoringSession,      // 读取当前草稿及传送专用修订。
    registry: Option<&MapContentRegistry>, // 目标地图目录修订参与稳定帧短路。
    visible: bool,
) -> CurrentPortalSignature {
    CurrentPortalSignature {
        open: editor.open, // 壳关闭期间不维护隐藏行字符串。
        visible, // 切回传送页时绕过稳定签名一次。
        map_id: session.draft.as_ref().map(|draft| draft.map_id.clone()), // 只克隆稳定短ID。
        portal_revision: session.portal_revision, // 其他作者域修订不会污染列表。
        catalog_revision: registry.map_or(0, |registry| registry.revision), // 目录未就绪使用零代次。
        selected_portal_id: editor.selected_current_portal_id.clone(),      // 只复制一个短ID。
    }
}

/// 只在传送工具页显示双列表；显隐唯一写者仍属于原传送表现域。
pub(crate) fn sync_portal_selection_panel(
    editor: Res<MapEditorState>,
    navigation: PortalSidebarNavigation,
    mut panels: Query<&mut Node, With<MapEditorPortalSelectionPanel>>,
) {
    let visible = navigation.visible(&editor);
    for mut node in &mut panels {
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        }; // 不增删BSN实体；Added新壳也能在稳定可见态初始化。
        if node.display != display { node.display = display; }
    }
}

/// 草稿或选择变化时原位更新当前地图全部源点行。
#[allow(clippy::type_complexity)] // 显式Without使多个可变Node/Text查询在Bevy中可证明互斥。
pub(crate) fn sync_current_portal_rows(
    editor: Res<MapEditorState>,
    navigation: PortalSidebarNavigation,
    session: Res<EditorAuthoringSession>,
    registry: Option<Res<MapContentRegistry>>, // 当前全部行通过正式目录解析各自目标地图清单。
    manifests: Res<Assets<MapManifestAsset>>,  // 目标地图显示名来自Ready清单而不是MAP-*。
    portal_assets: Res<Assets<MapPortalsAsset>>, // 具体到达点解析其世界“前往…”标签。
    row_entity_revision: Res<PortalRowEntityRevision>, // 跨帧新增行必须在创建同帧完成身份与文字绑定。
    mut last_signature: Local<Option<CurrentPortalSignature>>,
    added_containers: Query<(), Added<MapEditorCurrentPortalList>>,
    mut containers: Query<
        &mut Node,
        (
            With<MapEditorCurrentPortalList>,
            Without<MapEditorCurrentPortalRow>,
            Without<MapEditorCurrentPortalActions>,
        ),
    >,
    mut actions: Query<
        &mut Node,
        (
            With<MapEditorCurrentPortalActions>,
            Without<MapEditorCurrentPortalList>,
            Without<MapEditorCurrentPortalRow>,
        ),
    >,
    mut rows: Query<
        (
            &mut MapEditorCurrentPortalRow,
            &mut Node,
            &mut BackgroundColor,
            &Children,
            Option<&mut feathrise_editor_ui::EditorButtonClick>,
        ),
        (
            Without<MapEditorCurrentPortalList>,
            Without<MapEditorCurrentPortalActions>,
        ),
    >,
    mut row_texts: Query<
        (&mut Text, &mut TextColor),
        (
            With<MapEditorCurrentPortalRowText>,
            Without<MapEditorCurrentPortalListTitle>,
        ),
    >,
    mut titles: Query<
        &mut Text,
        (
            With<MapEditorCurrentPortalListTitle>,
            Without<MapEditorCurrentPortalRowText>,
        ),
    >,
) {
    let signature = current_signature(
        &editor,
        &session,
        registry.as_deref(),
        navigation.visible(&editor),
    ); // 先冻结调用前身份用于稳定帧短路。
    let assets_changed = manifests.is_changed() || portal_assets.is_changed(); // 异步Ready与地图改名热更都必须刷新全部行。
    let row_entities_changed = row_entity_revision.is_changed(); // 只在实际排队增删行的帧绕过稳定签名。
    let container_added = !added_containers.is_empty(); // 新BSN壳必须初始化完整列表和高度。
    if last_signature.as_ref() == Some(&signature)
        && !assets_changed
        && !row_entities_changed
        && !container_added
    {
        return; // 每帧视口状态不会扫描完整传送数组。
    }
    if !signature.visible {
        *last_signature = Some(signature); // 切回传送页时可见身份变化会触发完整刷新。
        return;
    }
    let display_context = PortalDisplayContext::new(&session, registry.as_deref(), &manifests); // 一次结构刷新解析全部可见目标名称。

    let portals = session.draft.as_ref().map(|draft| &draft.portals.portals); // 当前草稿是唯一作者数据源。
    let total = portals.map_or(0, Vec::len); // 草稿加载中明确显示零项等待态。

    for mut title in &mut titles {
        **title = session.draft.as_ref().map_or_else(
            || "当前地图传送点 · 等待作者数据".to_owned(),
            |draft| format!("当前地图 · {} · {total}项", draft.manifest.display_name),
        ); // 标题同时说明列表确实属于当前已进入地图。
    }
    for mut container in &mut containers {
        fit_portal_list(&mut container, total); // 全部真实行进入左栏整体滚动，空列表彻底退出布局。
    }
    for mut action in &mut actions {
        action.display = if total > 0 {
            Display::Flex
        } else {
            Display::None
        }; // 删除操作只在当前地图确有源点时保留，目标地图列表始终只读。
    }
    for (mut row, mut node, mut background, children, click) in &mut rows {
        // 只有完整目录/作者签名刷新才进入此循环；旧按住不能借新目标或新修订获得释放资格。
        if let Some(mut click) = click { click.invalidate(); }
        let Some(portal) = portals.and_then(|items| items.get(row.slot)) else {
            row.portal_id = None; // 结构换代边界先清掉已经越界的旧源点身份。
            node.display = Display::None; // 等待行实体收缩期间不占高度也不接受鼠标。
            continue;
        };
        row.portal_id = Some(portal.id.clone()); // 点击时再按稳定ID二次解析当前草稿。
        node.display = Display::Flex; // 有真实源点才恢复点击区域。
        let selected = editor.selected_current_portal_id.as_deref() == Some(portal.id.as_str()); // 标签重名不影响选择。
        background.0 = portal_row_background(Interaction::None, selected); // Presentation恢复稳定背景。
        let portal_label = display_context.portal_label(portal); // 源点名称固定派生为“前往{目标地图显示名}”。
        let value = format!("◇ {portal_label}"); // 列表优先名称；完整目标身份与安全坐标继续用于原点击详情。
        for child in children.iter() {
            if let Ok((mut text, mut color)) = row_texts.get_mut(child) {
                **text = value.clone(); // 每行只有一个文字子节点，防御性兼容额外子节点。
                color.0 = if selected {
                    colors::TEXT_ACCENT
                } else {
                    colors::TEXT_PRIMARY
                }; // 当前源点金色高亮。
            }
        }
    }
    *last_signature = Some(current_signature(
        &editor,             // 保存本次工具与选择身份。
        &session,            // 保存当前地图及传送修订。
        registry.as_deref(), // 保存目标目录修订，地图改名时可再次刷新。
        navigation.visible(&editor),
    )); // 保存完整列表最终身份。
}

/// 当前源点行的悬停和按下反馈不改变目标落点选择。
pub(crate) fn sync_current_portal_row_interactions(
    editor: Res<MapEditorState>,
    mut rows: Query<
        (
            &Interaction,
            &MapEditorCurrentPortalRow,
            &mut BackgroundColor,
        ),
        Changed<Interaction>,
    >,
) {
    for (interaction, row, mut background) in &mut rows {
        let Some(portal_id) = row.portal_id.as_deref() else {
            background.0 = colors::SLOT_BG; // 空槽永不保留旧悬停色。
            continue;
        };
        let selected = editor.selected_current_portal_id.as_deref() == Some(portal_id); // 稳定ID决定选中态。
        background.0 = portal_row_background(*interaction, selected); // 按下与悬停覆盖稳定背景。
    }
}
