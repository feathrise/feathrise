//! 地表作者层固定行池的增量回显与鼠标反馈，稳定帧不扫描地图层表。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // 原生UI节点、资源与变更检测。
use feathrise_map_content::SurfacePresetRegistry; // 预设热更后显示当前目录名称。
use crate::ui::theme as colors; // 与植被作者层共用铜金选择反馈。

use crate::authoring::{EditorAuthoringSession, SurfaceLayerEditTask}; // 当前作者草稿与后台删除状态。
use crate::resources::MapEditorState; // 复用笔刷层选择而不维护第二份业务状态。

use super::components::*; // 列表容器、行和操作按钮只属于本功能域。
use super::constants::{LAYOUT, ROW_COUNT}; // 六槽固定容量和动态真实高度。
use super::helpers::{layer_display_label, selected_index}; // 名称与选择都以真实地图作者层为准。
use super::types::{SurfaceLayerAction, SurfaceLayerListSnapshot, SurfaceLayerTarget}; // 稳定快照阻止相机移动触发列表重建。

/// 列表容器首次出现时创建六行，后续仅重绑内容。
pub(super) fn spawn_rows(
    mut commands: Commands,                             // UI实体只在主线程创建。
    containers: Query<Entity, Added<SurfaceLayerList>>, // 不逐帧增删列表条目。
    assets: Res<AssetServer>,                           // 共享项目中文字体。
) {
    if containers.is_empty() {
        return;
    } // 稳定帧不重复请求字体。
    let font = feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH); // 与植被作者行保持一致。
    for container in &containers {
        commands.entity(container).with_children(|parent| {
            for slot in 0..ROW_COUNT {
                parent
                    .spawn((
                        // 每个固定槽只创建一套按钮和文本。
                        Button, // 原生Interaction承担点击和悬停。
                        SurfaceLayerRow { slot, target: None }, // 数据就绪前没有可操作身份。
                        Node {
                            // 固定行采用真实20像素高度。
                            display: Display::None, // 未使用槽完全退出布局。
                            width: percent(100),    // 行与列表等宽。
                            height: px(LAYOUT.row_height), // 与容器高度计算同源。
                            flex_shrink: 0.0,       // 不被窄窗口压扁。
                            align_items: AlignItems::Center, // 标签垂直居中。
                            padding: UiRect::horizontal(px(4)), // 文字与铜边保留间距。
                            border: UiRect::all(px(1)), // 使用列表统一细边。
                            overflow: Overflow::clip_x(), // 长材质名不挤压相邻面板。
                            ..default()             // 其余遵循Bevy普通按钮布局。
                        }, // 视觉尺寸固定。
                        BackgroundColor(colors::SLOT_BG), // 未选择时使用暗墨槽位。
                        BorderColor::all(colors::COPPER_BORDER_WEAK), // 与植被行相同的弱铜边。
                    ))
                    .with_child((
                        // 每行文本实体数量恒定。
                        SurfaceLayerRowText, // 专用文字过滤器避免修改其他页。
                        Text::new(""),       // 尚无地图身份时不伪造材质名。
                        TextFont {
                            // 共享中文字体并保留紧凑字号。
                            font: font.clone().into(), // 字体强句柄只做浅克隆。
                            font_size: ui_metrics::FONT_CONTROL.into(),     // 六行列表完整适配260像素左栏。
                            ..default()                // 行高使用现有默认值。
                        }, // 字体规格不随地图缩放变化。
                        TextColor(colors::TEXT_PRIMARY), // 与植被未选中层一致。
                    )); // 子文本结构稳定。
            } // 总共只创建六个槽位。
        }); // 不为地图层总数创建额外实体。
    } // 后续由同步系统按页复用。
} // 无文件IO或地表像素处理。

/// 草稿修订、选择或目录名称变化时才原位刷新当前六行。
pub(super) fn sync_rows(
    editor: Res<MapEditorState>, // 相机和鼠标变化不会进入下面的专用快照。
    session: Res<EditorAuthoringSession>, // 地表专用修订独立于植被等其他修改。
    registry: Option<Res<SurfacePresetRegistry>>, // 目录改名会推进显示修订。
    task: Res<SurfaceLayerEditTask>, // 删除期间界面继续保留旧完整层列表。
    added: Query<(), Added<SurfaceLayerList>>, // UI重新创建时强制初始化一次。
    mut seen: Local<Option<SurfaceLayerListSnapshot>>, // 只缓存常数规模状态。
    mut chrome: Query<
        // 列表外壳与操作按钮互不拥有行标记。
        (
            &mut Node,
            Option<&SurfaceLayerList>,
            Option<&SurfaceLayerActions>,
            Option<&SurfaceLayerButton>,
        ), // 一次查询调整固定容器。
        (
            Without<SurfaceLayerRow>,
            Or<(
                With<SurfaceLayerList>,
                With<SurfaceLayerActions>,
                With<SurfaceLayerButton>,
            )>,
        ), // 与下方可变行Node互斥。
    >, // 不查询无关页面节点。
    mut rows: Query<
        // 固定六行原位换内容。
        (
            &mut SurfaceLayerRow,
            Option<&mut feathrise_editor_ui::EditorButtonClick>,
            &mut Node,
            &mut BackgroundColor,
            &Interaction,
            &Children,
        ), // 保留当前鼠标反馈。
        (
            Without<SurfaceLayerList>,
            Without<SurfaceLayerActions>,
            Without<SurfaceLayerButton>,
        ), // 与容器及按钮Node查询保持互斥。
    >, // 实体数不会随层数增长。
    mut texts: Query<
        // 摘要和行文本共用一个可变文字查询。
        (&mut Text, &mut TextColor, Option<&SurfaceLayerListTitle>), // 标题与行靠marker区分。
        Or<(With<SurfaceLayerRowText>, With<SurfaceLayerListTitle>)>, // 不扫描其他作者页面文字。
    >, // 所有Children查询只针对六个固定行。
) {
    if !editor.open {
        *seen = None;
        return;
    } // 重新打开时必须恢复完整列表状态。
    let draft = session.draft.as_ref(); // 没有草稿时展示零项。
    let surface = draft.and_then(|draft| draft.surface.as_ref()); // 只列当前地图实际引用层。
    let count = surface.map_or(0, |surface| surface.layers.len()); // O(1)取得真实层数。
    let snapshot = SurfaceLayerListSnapshot {
        open: editor.open,                               // 页面打开状态进入显示身份。
        map_id: draft.map(|draft| draft.map_id.clone()), // 换图重新绑定所有行。
        surface_revision: session.surface_revision,      // 删除、撤销和落笔后刷新稳定目标。
        registry_revision: registry.as_ref().map_or(0, |registry| registry.revision), // 目录热更后刷新名称。
        selected: editor.selected_surface_layer, // 同一个索引同时决定当前页。
        preset_id: editor.surface_selected_preset_id.clone(), // 管理选择不同预设时清除错误高亮。
        count,                                   // 防御加载或空表变化。
        pending: task.is_deleting(),             // 后台完成后恢复正常删除提示。
    }; // 不包含逐帧变化的光标和相机字段。
    if seen.as_ref() == Some(&snapshot) && added.is_empty() {
        return;
    } // 稳定帧不写节点或文本。
    let page = editor.selected_surface_layer.min(count.saturating_sub(1)) / ROW_COUNT; // 删除最后一页后安全返回有效页。
    let start = page * ROW_COUNT; // 只访问当前页六层。
    let visible = count.saturating_sub(start).min(ROW_COUNT); // 末页按真实行数收缩。
    let selected =
        surface.and_then(|surface| selected_index(surface, &editor, registry.as_deref())); // 未引用预设不冒充选中层。
    for (mut node, list, actions, button) in &mut chrome {
        if list.is_some() {
            LAYOUT.fit(&mut node, visible);
        } // 复用植被相同动态高度算法。
        if actions.is_some() {
            node.display = if surface.is_some() {
                // 空作者表也必须保留新建入口。
                Display::Flex
            } else {
                Display::None
            };
        } // 只有尚未加载地表定义时隐藏整行。
        if let Some(button) = button {
            node.display = if button.0 == SurfaceLayerAction::Create // 新建始终可见，零层时也能恢复编辑。
                || (button.0 == SurfaceLayerAction::DeleteSelected && count > 0) // 空表不展示无目标删除。
                || (matches!(button.0, SurfaceLayerAction::PreviousPage | SurfaceLayerAction::NextPage) && count > ROW_COUNT)
            {
                // 翻页仅用于真实多页。
                Display::Flex
            } else {
                Display::None
            }; // 单页不显示无用翻页按钮。
        } // 空表只保留新建，有层时仍允许删除最后一层。
    } // 容器更新数量保持常数。
    for (mut text, _, title) in &mut texts {
        if title.is_some() {
            **text = if snapshot.pending {
                "地表作者层 · 正在删除所选层…".to_owned()
            } else if count > ROW_COUNT {
                format!(
                    "地表作者层 · {count}项 · 第{}/{}页",
                    page + 1,
                    count.div_ceil(ROW_COUNT)
                )
            } else {
                format!("地表作者层 · {count}项")
            }; // 只有多页时才显示页码。
        } // 行文本在下面按真实身份生成。
    } // 摘要不扫描任何像素。
    for (mut row, mut click, mut node, mut background, interaction, children) in &mut rows {
        let index = start + row.slot; // 槽位映射到当前地图绝对层下标。
        let Some(layer) = surface.and_then(|surface| surface.layers.get(index)) else {
            if row.target.is_some() && let Some(click) = click.as_deref_mut() { click.invalidate(); }
            row.target = None; // 旧页尾槽不保留可点击身份。
            node.display = Display::None; // 未使用槽不占高度。
            continue; // 不读取旧子文本。
        }; // 以下全部来自当前真实地图层。
        let target = draft.map(|draft| SurfaceLayerTarget {
            map_id: draft.map_id.clone(),       // 防止同名层跨图串选。
            revision: session.surface_revision, // 点击必须对应当前定义与像素修订。
            index,                              // 有效身份下O(1)取得层。
            layer_id: layer.id.clone(),         // 删除和重排后仍能复验稳定层ID。
        }); // 短身份复制与像素规模无关。
        if row.target != target && let Some(click) = click.as_deref_mut() { click.invalidate(); }
        row.target = target;
        node.display = Display::Flex; // 只启用当前页真实层。
        let highlighted = selected == Some(index); // 显示与删除使用完全相同的选择语义。
        background.0 = row_background(*interaction, highlighted); // 正在悬停时不被刷新覆盖反馈。
        let label = layer_display_label(layer, registry.as_deref()); // 同预设的不同作者层仍保留可辨名称。
        for child in children.iter() {
            if let Ok((mut text, mut color, _)) = texts.get_mut(child) {
                **text = format!(
                    "{} {}{}",
                    if highlighted { "◆" } else { "◇" },
                    label,
                    if index == 0 { " · 基础" } else { "" }
                ); // 基础层明确承接删除后的空白回退。
                color.0 = if highlighted {
                    colors::TEXT_ACCENT
                } else {
                    colors::TEXT_PRIMARY
                }; // 铜金高亮准确对应删除目标。
            } // 其他子组件不参与作者文字回显。
        } // 每行只有固定一个文本子节点。
    } // 地图即使拥有上千层也只更新六行。
    *seen = Some(snapshot); // 记录已经完整回显的UI身份。
} // 不改变地表定义、像素或保存状态。

/// 点击和悬停反馈与植被作者层保持一致。
fn row_background(interaction: Interaction, selected: bool) -> Color {
    match interaction {
        Interaction::Pressed => colors::SELECT_BG_PRESSED, // 按下时提供明确反馈。
        Interaction::Hovered => colors::SELECT_BG_HOVER,   // 悬停时提示可选。
        Interaction::None if selected => colors::SELECT_BG, // 松手后保留选中背景。
        Interaction::None => colors::SLOT_BG,              // 普通行恢复默认背景。
    } // 背景规则由行与按钮共享。
} // 不创建额外UI实体。

/// 只处理Interaction变化，不因鼠标停留每帧重写颜色。
pub(super) fn sync_interactions(
    editor: Res<MapEditorState>,                  // 当前高亮索引。
    session: Res<EditorAuthoringSession>,         // 仅用当前层表验证预设选择。
    registry: Option<Res<SurfacePresetRegistry>>, // 规范ID与历史别名保持一致。
    mut controls: Query<
        (&Interaction, Option<&SurfaceLayerRow>, &mut BackgroundColor),
        (
            Changed<Interaction>,
            Or<(With<SurfaceLayerRow>, With<SurfaceLayerButton>)>,
        ),
    >, // 至多六行和三个按钮。
) {
    let selected = session
        .draft
        .as_ref()
        .and_then(|draft| draft.surface.as_ref())
        .and_then(|surface| selected_index(surface, &editor, registry.as_deref())); // 管理选择未引用预设时不恢复旧高亮。
    for (interaction, row, mut background) in &mut controls {
        let highlighted = row
            .and_then(|row| row.target.as_ref())
            .is_some_and(|target| Some(target.index) == selected); // 按钮本身不保留选中态。
        background.0 = row_background(*interaction, highlighted); // 复用统一反馈规则。
    } // Changed过滤使静止鼠标无更新成本。
} // 作者数据完全只读。
