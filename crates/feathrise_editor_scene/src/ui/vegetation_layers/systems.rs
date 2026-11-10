//! 植被作者图层固定行池与按钮ECS系统。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // UI实体、交互查询、文本和资源。
use feathrise_scene_content::VegetationPresetRegistry; // 首次新建预设层时延迟创建场景正式输出。
use feathrise_scene::DynamicMeshProfileRequests; // 选中Mesh作者层时请求既有正式四级原型。
use crate::ui::theme as colors; // 固定行与编辑器BSN保持同一水墨铜边配色。

use crate::authoring::{EditorAuthoringSession, VegetationBrushStroke}; // 当前作者描述、正式输出与活动笔划门。
use crate::components::{
    SceneEditorActionButton, SceneEditorVegetationLayerList, SceneEditorVegetationLayerListTitle,
    SceneEditorVegetationLayerRow, SceneEditorVegetationLayerRowText,
}; // 静态容器只生成一次固定行池。
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 当前鼠标所有权、选择、页码、工具和状态。
use crate::types::{EditorAction, EditorTool}; // 区分列表分页与结构动作。
use crate::ui::actions::activate_vegetation_layer; // 选择后仍进入正式LOD/风场profile加载路径。
use crate::ui::list_layout::FixedRowPoolLayout; // 与场景、场景、资产和水域共享动态固定行池算法。

use super::allocation::{
    VegetationBlankLayerAllocation, request_vegetation_blank_layer_allocation,
}; // 新建层先在唯一后台槽完成整张空白R8。
use super::operations::{apply_vegetation_authoring_action, is_vegetation_authoring_action}; // 其余结构按钮共享一次受检原子采用。

pub(crate) const VEGETATION_LAYER_ROW_COUNT: usize = 6; // 固定池最多显示六行，更多层随选择自动翻页。
const VEGETATION_LAYER_ROW_HEIGHT: f32 = 20.0; // 单行高度与场景编辑器正式作者列表密度一致。
const VEGETATION_LAYER_ROW_GAP: f32 = 2.0; // 相邻可见行沿用两像素紧凑间距。
const VEGETATION_LAYER_VERTICAL_PADDING: f32 = 2.0; // 非空列表上下各保留两像素安全边距。
const VEGETATION_LAYER_LAYOUT: FixedRowPoolLayout = FixedRowPoolLayout {
    capacity: VEGETATION_LAYER_ROW_COUNT, // 与固定实体池和分页步长保持同一上限。
    row_height: VEGETATION_LAYER_ROW_HEIGHT, // 使用实际可点击行高。
    row_gap: VEGETATION_LAYER_ROW_GAP,    // 相邻可见行之间保留明确间距。
    padding_y: VEGETATION_LAYER_VERTICAL_PADDING, // 非空列表上下保留安全边距。
};

/// 返回前/后页首层目标；空列表没有目标，首页与尾页安全停留在边界首层。
fn vegetation_page_first_target(
    total: usize,        // 当前真实作者层总数。
    current_page: usize, // 当前会话页码可能因撤销或热更短暂越界。
    previous: bool,      // true向前，false向后。
) -> Option<(usize, usize)> {
    if total == 0 {
        return None; // 空作者列表只保留输出选择和新建入口。
    }
    let last_page = total.saturating_sub(1) / VEGETATION_LAYER_ROW_COUNT;
    let current_page = current_page.min(last_page);
    let target_page = if previous {
        current_page.saturating_sub(1)
    } else {
        current_page.saturating_add(1).min(last_page)
    };
    Some((
        target_page,
        target_page.saturating_mul(VEGETATION_LAYER_ROW_COUNT), // 每次分页明确直选目标页第一层。
    ))
}

/// 根据当前页实际行数计算列表高度；超过六行时固定在单页最大高度。
#[cfg(test)]
fn vegetation_layer_list_height(visible_rows: usize) -> f32 {
    VEGETATION_LAYER_LAYOUT.height(visible_rows) // 共享算法覆盖零项、末页、满页和越界钳制。
}

/// 把植被作者列表容器收缩到当前页可见行数，固定行实体池保持不变。
fn fit_vegetation_layer_list_container(node: &mut Node, visible_rows: usize) {
    VEGETATION_LAYER_LAYOUT.fit(node, visible_rows); // 固定六槽不增删，容器只按真实行数改变布局。
}

/// 在BSN作者图层容器下只创建一次固定行池；稳定帧不增删UI实体。
pub(crate) fn spawn_vegetation_layer_rows(
    mut commands: Commands,
    containers: Query<Entity, Added<SceneEditorVegetationLayerList>>,
    asset_server: Res<AssetServer>,
) {
    let font = feathrise_editor_core::editor_font(&asset_server, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf");
    for container in &containers {
        commands.entity(container).with_children(|parent| {
            for slot in 0..VEGETATION_LAYER_ROW_COUNT {
                parent
                    .spawn((
                        InteractionButton,
                        SceneEditorVegetationLayerRow {
                            slot,
                            layer_index: None,
                        },
                        Node {
                            display: Display::None,
                            width: percent(100),
                            height: px(VEGETATION_LAYER_ROW_HEIGHT), // 行高与动态容器高度公式使用同一常量。
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(4)),
                            border: UiRect::all(px(1)),
                            overflow: Overflow::clip_x(),
                            ..default()
                        },
                        BackgroundColor(colors::SLOT_BG),
                        BorderColor::all(colors::COPPER_BORDER_WEAK),
                    ))
                    .with_child((
                        SceneEditorVegetationLayerRowText,
                        Text::new(""),
                        TextLayout::no_wrap(), // 固定20px行高只容纳单行，长名称由行容器裁剪，避免换行溢出。
                        bevy::ui::FocusPolicy::Pass, // 字形不截断鼠标命中，整行统一处理悬停与点击。
                        TextFont {
                            font: font.clone().into(),
                            font_size: ui_metrics::FONT_CONTROL.into(),
                            ..default()
                        },
                        TextColor(colors::TEXT_PRIMARY),
                    ));
            }
        });
    }
}

/// 作者描述、选择或页码变化时原地刷新六个可点击图层行。
pub(crate) fn sync_vegetation_layer_rows(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    added_containers: Query<(), Added<SceneEditorVegetationLayerList>>, // 面板重建时即使作者修订稳定也必须初始化高度。
    mut containers: Query<
        &mut Node,
        (
            With<SceneEditorVegetationLayerList>, // 只调整BSN声明的列表外层容器。
            Without<SceneEditorVegetationLayerRow>, // 与下方可变行Node查询保持ECS借用互斥。
        ),
    >,
    mut presented_scope: Local<Option<(Option<feathrise_scene_content::SceneId>, u64)>>,
    mut rows: Query<
        (&mut SceneEditorVegetationLayerRow, &mut Node, &Children, Option<&mut EditorButtonClick>),
        Without<SceneEditorVegetationLayerList>,
    >, // 行实体不会被误当作列表外层重复调整高度。
    mut texts: Query<
        (
            &mut Text,
            &mut TextColor,
            Option<&SceneEditorVegetationLayerRowText>,
            Option<&SceneEditorVegetationLayerListTitle>,
        ),
        Or<(
            With<SceneEditorVegetationLayerRowText>,
            With<SceneEditorVegetationLayerListTitle>,
        )>,
    >,
) {
    let container_added = !added_containers.is_empty(); // 新BSN容器必须绕过稳定资源的早退一次。
    if !editor.open || (!(editor.is_changed() || session.is_changed()) && !container_added
        && !locale.as_ref().is_some_and(|locale| locale.is_changed()))
    {
        return;
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let vegetation = session
        .draft
        .as_ref()
        .and_then(|draft| draft.vegetation.as_ref());
    let authoring = vegetation.and_then(|value| value.authoring.as_ref());
    let layer_count = authoring.map_or(0, |value| value.layers.len());
    let desired_selected = if layer_count == 0 {
        0
    } else {
        editor.selected_vegetation_layer.min(layer_count - 1)
    };
    let desired_page = desired_selected / VEGETATION_LAYER_ROW_COUNT;
    if editor.selected_vegetation_layer != desired_selected {
        editor.selected_vegetation_layer = desired_selected; // 只在撤销删除使原索引越界时写资源。
    }
    if editor.vegetation_layer_page != desired_page {
        editor.vegetation_layer_page = desired_page; // 快捷选择自动翻页，但稳定帧不自我制造Changed。
    }
    let start = editor
        .vegetation_layer_page
        .saturating_mul(VEGETATION_LAYER_ROW_COUNT);
    let visible_rows = layer_count
        .saturating_sub(start)
        .min(VEGETATION_LAYER_ROW_COUNT); // 末页只保留真实作者层数量，零层得到零高度。
    let page_count = layer_count
        .saturating_sub(1)
        .div_euclid(VEGETATION_LAYER_ROW_COUNT)
        + 1; // 空作者列表仍显示稳定第1/1页摘要。
    for (mut text, _, _, title) in &mut texts {
        if title.is_some() {
            let value = if layer_count == 0 {
                // 空态给出下一步，不显示没有意义的第1/1页。
                "作者图层 · 暂无\n先选预设，再点“新建空白层”。".to_owned() // 明确显式建层工作流。
            } else if page_count == 1 {
                // 单页只显示真实数量，节省标题宽度。
                format!("作者图层 · {layer_count}层") // 与空态使用同一分区名称。
            } else {
                // 多页保留定位信息，不改变分页直选行为。
                format!(
                    "作者图层 · {layer_count}层 · {}/{page_count}页",
                    editor.vegetation_layer_page + 1
                ) // 当前页与总页数紧凑排列。
            }; // 文案仅在已有变更检测路径更新。
            let value = locale.editor_notice(&value);
            if text.as_str() != value.as_ref() {
                **text = value.into_owned();
            }
        }
    }
    for mut container in &mut containers {
        fit_vegetation_layer_list_container(&mut container, visible_rows); // 仅作者结构、页码或面板重建时触发布局更新。
    }
    let scope = (editor.selected_scene_id.clone(), session.revision);
    let scope_changed = presented_scope.as_ref() != Some(&scope);
    *presented_scope = Some(scope);
    for (mut row, mut node, children, mut click) in &mut rows {
        // 内容刷新不再写背景，避免覆盖持续悬停状态。
        let index = start + row.slot;
        let Some(layer) = authoring.and_then(|value| value.layers.get(index)) else {
            if row.layer_index.is_some() && let Some(click) = click.as_deref_mut() { click.invalidate(); }
            row.layer_index = None;
            node.display = Display::None;
            continue;
        };
        if (scope_changed || row.layer_index != Some(index))
            && let Some(click) = click.as_deref_mut() { click.invalidate(); }
        row.layer_index = Some(index);
        node.display = Display::Flex;
        let selected = editor.selected_vegetation_layer == index;
        let group = authoring
            .and_then(|value| value.groups.iter().find(|group| group.id == layer.group_id));
        let output = vegetation.and_then(|value| {
            value
                .layers
                .iter()
                .find(|output| output.id == layer.output_layer_id)
        });
        let visible = if layer.enabled { "●" } else { "○" };
        let locked = locale.editor_label(if layer.locked { "锁" } else { "编" });
        let group_text = group
            .map(|value| format!("{}/{}", value.label, locale.editor_label(value.mode.label())))
            .unwrap_or_else(|| locale.text("vegetation.layers.invalid_group").to_owned());
        let output_text = output.map_or_else(|| locale.text("vegetation.layers.invalid_output"), |value| value.label.as_str());
        for child in children.iter() {
            if let Ok((mut text, mut color, row_text, _)) = texts.get_mut(child)
                && row_text.is_some()
            {
                **text = format!(
                    "{} {visible}{locked} {} → {output_text} | {group_text} P{} W{:.2} O{:.0}%",
                    if selected { "◆" } else { "◇" },
                    layer.label,
                    layer.priority,
                    layer.weight,
                    layer.opacity * 100.0
                );
                color.0 = if selected {
                    colors::TEXT_ACCENT
                } else if !layer.enabled {
                    colors::TEXT_DIM
                } else {
                    colors::TEXT_PRIMARY
                };
            }
        }
    }
}

/// 唯一背景写入入口：六个固定行同时结合当前交互与选择，内容刷新不能清除悬停。
pub(crate) fn sync_vegetation_layer_row_interactions(
    editor: Res<SceneEditorState>,
    mut rows: Query<(
        &UiInteraction,                 // 读取持续交互，不能仅依赖鼠标进入/离开的变更边沿。
        &SceneEditorVegetationLayerRow, // 分页重绑或选择变化也应立即更新背景。
        &mut BackgroundColor,         // 本系统独占行背景颜色写入。
    )>, // 固定六槽的常量工作，不扫描场景数据或动态增长的内容。
) {
    for (interaction, row, mut background) in &mut rows {
        let selected = row.layer_index == Some(editor.selected_vegetation_layer);
        let next = match (*interaction, selected) {
            // 悬停/按下优先于选中态，移出后恢复真实选择颜色。
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::SLOT_BG,
        };
        if background.0 != next {
            // 稳定帧不产生无意义的组件变更。
            background.0 = next; // 当前交互与当前选择在同一入口原子决定颜色。
        }
    }
}

/// 点击固定行时选择真正作者画布，并由正式profile路径请求Mesh四级LOD与风材质。
pub(crate) fn handle_vegetation_layer_row_clicks(
    rows: Query<(&EditorButtonClick, &SceneEditorVegetationLayerRow), Changed<EditorButtonClick>>,
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 数值和弦期间丢弃行点击而不暂停Changed查询。
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut mesh_profile_requests: Option<ResMut<DynamicMeshProfileRequests>>,
) {
    if numeric_scrub_mouse.blocked() {
        return; // 被数值拖动拥有的按键边沿不能在解锁后补选作者层。
    }
    for (interaction, row) in &rows {
        if !interaction.is_clicked() {
            continue;
        }
        let Some(index) = row.layer_index else {
            continue;
        };
        activate_vegetation_layer(
            &mut editor,
            &session,
            index,
            mesh_profile_requests.as_deref_mut(),
        );
        editor.vegetation_layer_page = index / VEGETATION_LAYER_ROW_COUNT;
    }
}

/// 消费图层结构和规则按钮；普通工具按钮系统只识别并跳过这些动作。
pub(crate) fn handle_vegetation_authoring_buttons(
    buttons: Query<(&EditorButtonClick, &SceneEditorActionButton), Changed<EditorButtonClick>>,
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 公共数值拖动完整和弦独占按钮鼠标。
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    stroke: Res<VegetationBrushStroke>,
    preset_registry: Option<Res<VegetationPresetRegistry>>,
    mut blank_allocation: ResMut<VegetationBlankLayerAllocation>,
    mut mesh_profile_requests: Option<ResMut<DynamicMeshProfileRequests>>,
) {
    if numeric_scrub_mouse.blocked() {
        return; // 系统运行本身推进Changed tick，阻塞帧不发布任何图层结构事务。
    }
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        if matches!(
            button.0,
            EditorAction::VegetationListPreviousPage | EditorAction::VegetationListNextPage
        ) {
            let count = author_layer_count(&session);
            let Some((target_page, index)) = vegetation_page_first_target(
                count,
                editor.vegetation_layer_page,
                button.0 == EditorAction::VegetationListPreviousPage,
            ) else {
                editor.status = "当前场景没有植被作者图层".to_owned();
                continue;
            };
            activate_vegetation_layer(
                &mut editor,
                &session,
                index,
                mesh_profile_requests.as_deref_mut(),
            );
            editor.vegetation_layer_page = target_page;
            continue;
        }
        if !is_vegetation_authoring_action(button.0) {
            continue;
        }
        if stroke.active {
            editor.status = "当前植被笔划正在收尾；松开鼠标后再修改图层结构".to_owned();
            continue; // 不能先删除笔划绑定的作者层再尝试生成局部撤销差分。
        }
        if button.0 == EditorAction::VegetationAddLayer {
            if let Err(error) = request_vegetation_blank_layer_allocation(
                &mut editor,
                &session,
                preset_registry.as_deref(),
                &mut blank_allocation,
            ) {
                editor.status = error;
            }
            continue; // 描述、正式输出和撤销命令只在后台完整结果采用时一起出现。
        }
        match apply_vegetation_authoring_action(
            button.0,
            &mut editor,
            &mut session,
            preset_registry.as_deref(),
        ) {
            Ok(outcome) => {
                editor.tool = EditorTool::Vegetation;
                if let Some(index) = outcome.selected {
                    editor.vegetation_layer_page = index / VEGETATION_LAYER_ROW_COUNT;
                    if activate_vegetation_layer(
                        &mut editor,
                        &session,
                        index,
                        mesh_profile_requests.as_deref_mut(),
                    ) {
                        editor.status = outcome.status; // 正式profile可用时回显具体结构变化。
                    }
                } else {
                    editor.vegetation_layer_page = 0;
                    editor.status = outcome.status;
                }
            }
            Err(error) => editor.status = error, // 局部副本失败，当前草稿和撤销栈均保持原样。
        }
    }
}

fn author_layer_count(session: &EditorAuthoringSession) -> usize {
    session
        .draft
        .as_ref()
        .and_then(|draft| draft.vegetation.as_ref())
        .and_then(|vegetation| vegetation.authoring.as_ref())
        .map_or(0, |authoring| authoring.layers.len())
}

#[cfg(test)]
mod tests {
    use super::*; // 直接验证动态列表高度与固定六行池的共同契约。

    #[test] // 覆盖鼠标停留期间编辑器刷新及不移动鼠标时的选择切换。
    fn vegetation_hover_survives_refresh_and_selection_changes() {
        let mut app = App::new(); // 无窗口运行真实背景系统，避免依赖显卡。
        app.init_resource::<SceneEditorState>(); // 使用正式选择资源。
        app.add_systems(Update, sync_vegetation_layer_row_interactions); // 执行生产背景路径。
        let row = app
            .world_mut()
            .spawn((
                // 仅建立固定池的一条代表行。
                SceneEditorVegetationLayerRow {
                    slot: 0,
                    layer_index: Some(0),
                }, // 初始选中第一层。
                UiInteraction::Hovered, // 鼠标进入后保持悬停，不再次触发Changed。
                BackgroundColor(colors::SLOT_BG), // 初始普通色应被悬停覆盖。
            ))
            .id(); // 保留稳定实体验证跨帧更新。
        app.update(); // 第一帧采用悬停色。
        assert_eq!(
            app.world().get::<BackgroundColor>(row).unwrap().0,
            colors::SELECT_BG_HOVER
        ); // 首次进入可见。
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .selected_vegetation_layer = 1; // 模拟列表因选择变化刷新。
        app.update(); // Interaction没有变化也应保留悬停。
        assert_eq!(
            app.world().get::<BackgroundColor>(row).unwrap().0,
            colors::SELECT_BG_HOVER
        ); // 不闪回普通色。
        *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None; // 鼠标离开未选中行。
        app.update(); // 恢复普通背景。
        assert_eq!(
            app.world().get::<BackgroundColor>(row).unwrap().0,
            colors::SLOT_BG
        ); // 不残留悬停。
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .selected_vegetation_layer = 0; // 无鼠标边沿地重新选中该行。
        app.update(); // 选择变化也必须刷新背景，原Changed过滤会遗漏。
        assert_eq!(
            app.world().get::<BackgroundColor>(row).unwrap().0,
            colors::SELECT_BG
        ); // 正确恢复选中态。
    }

    #[test]
    fn vegetation_layer_list_height_grows_only_to_page_capacity() {
        assert_eq!(vegetation_layer_list_height(0), 0.0); // 空列表不占任何垂直空间。
        assert_eq!(vegetation_layer_list_height(1), 24.0); // 一行包含20px本体和上下各2px内边距。
        assert_eq!(vegetation_layer_list_height(2), 46.0); // 第二行只新增20px本体与2px行距。
        assert_eq!(vegetation_layer_list_height(6), 134.0); // 六行恢复原单页最大高度。
        assert_eq!(vegetation_layer_list_height(7), 134.0); // 超出单页容量后不得继续撑高面板。
    }

    #[test]
    fn vegetation_layer_list_container_leaves_layout_only_when_empty() {
        let mut node = Node::default(); // 使用真实Bevy布局组件验证显示与高度同时切换。
        fit_vegetation_layer_list_container(&mut node, 0); // 零层必须完全退出父级布局。
        assert_eq!(node.display, Display::None); // 空容器不制造额外Column行距。
        assert_eq!(node.height, px(0)); // 空容器也不保留固定高度。
        fit_vegetation_layer_list_container(&mut node, 2); // 新增两层后恢复实际列表。
        assert_eq!(node.display, Display::Flex); // 非空固定行池重新参与布局。
        assert_eq!(node.height, px(46)); // 两行高度只包含真实行、一个间距和上下内边距。
    }

    #[test]
    fn vegetation_paging_selects_the_target_page_first_layer_and_clamps_edges() {
        assert_eq!(vegetation_page_first_target(0, 0, false), None); // 空列表没有伪选择。
        assert_eq!(vegetation_page_first_target(1, 0, false), Some((0, 0))); // 单页仍稳定直选首层。
        assert_eq!(vegetation_page_first_target(7, 0, false), Some((1, 6))); // 后页直选第七层。
        assert_eq!(vegetation_page_first_target(7, 1, false), Some((1, 6))); // 尾页不进入空页。
        assert_eq!(vegetation_page_first_target(7, 1, true), Some((0, 0))); // 前页直选第一层。
        assert_eq!(
            vegetation_page_first_target(7, usize::MAX, false),
            Some((1, 6))
        ); // 越界页先钳制。
    }
}
