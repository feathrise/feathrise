//! 水域对象固定行池的ECS生成、同步、分页和点击系统。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // UI实体、Changed交互、文本和作者资源。
use bevy::time::Real; // 双击定位使用F12暂停期间仍推进的真实时间。
use crate::ui::theme as colors; // 复用植被作者列表的水墨铜边与选中配色。

use crate::authoring::{EditorAuthoringSession, WaterBrushStroke, WaterNetworkEditState}; // 选择门读取保存、静水笔划和控制点拖动状态。
use crate::components::{
    SceneEditorActionButton, SceneEditorWaterFeatureList, SceneEditorWaterFeatureListTitle,
    SceneEditorWaterFeatureRow, SceneEditorWaterFeatureRowText,
}; // BSN容器只生成一次固定行池。
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 统一鼠标所有权、选择和页码。
use crate::types::{EditorAction, EditorTool, WaterAuthoringFeatureKind}; // 分页按钮、工具与三类稳定行目标。
use crate::ui::list_layout::FixedRowPoolLayout; // 五类固定行池共享同一动态高度规则。
use crate::ui::water_actions::reject_water_feature_switch; // 水域列表和结构入口共享跨帧事务保护。
use crate::viewport::SelectedWaterViewportState; // 行按下登记静水双击定位请求。

use super::WATER_FEATURE_ROW_COUNT;
use super::helpers::{
    activate_listed_water_feature, listed_active_water_feature,
    listed_water_feature_at,
    water_feature_page_count, water_feature_row_text,
}; // 系统只编排ECS借用，纯映射留在helpers。 // 六行固定预算与分页步长必须一致。
#[cfg(test)]
use super::helpers::{activate_water_feature, water_feature_at, water_feature_count};

const WATER_FEATURE_ROW_HEIGHT: f32 = 20.0; // 单行高度与植被作者列表保持同一紧凑密度。
const WATER_FEATURE_ROW_GAP: f32 = 2.0; // 相邻可见水域行之间保留两像素间距。
const WATER_FEATURE_VERTICAL_PADDING: f32 = 2.0; // 非空列表上下各保留两像素安全边距。
const WATER_FEATURE_LAYOUT: FixedRowPoolLayout = FixedRowPoolLayout {
    capacity: WATER_FEATURE_ROW_COUNT, // 固定六槽与分页步长保持一致。
    row_height: WATER_FEATURE_ROW_HEIGHT, // 使用真实点击行高度。
    row_gap: WATER_FEATURE_ROW_GAP,    // 相邻可见对象保留两像素间距。
    padding_y: WATER_FEATURE_VERTICAL_PADDING, // 非空列表上下各留两像素。
};

/// 根据当前页实际水域对象数计算容器高度；超过六行时固定在单页最大高度。
#[cfg(test)]
fn water_feature_list_height(visible_rows: usize) -> f32 {
    WATER_FEATURE_LAYOUT.height(visible_rows) // 共享算法覆盖零项、末页、满页和越界钳制。
}

/// 把水域对象列表容器收缩到当前页可见行数，固定行实体池保持不变。
fn fit_water_feature_list_container(node: &mut Node, visible_rows: usize) {
    WATER_FEATURE_LAYOUT.fit(node, visible_rows); // 固定六槽不增删，容器只按真实对象数改变布局。
}

/// 固定行池真正依赖的小型Presentation身份，排除每帧变化的视口光标和状态文案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WaterFeatureListSignature {
    water_revision: u64,              // 只在水域作者换代时重新读取三类数组。
    has_water: bool,                  // 空草稿与已加载空水域即使修订相同也必须区分。
    tool: EditorTool,                 // 水域工具决定哪一类选择显示金色高亮。
    selected_layer: usize,            // 三套类内索引共同构成当前活动对象。
    selected_river: usize,            // 河流稳定ID直选或结构变化后触发一次刷新。
    selected_waterfall: usize,        // 瀑布选择变化不依赖整个SceneEditorState的Changed。
    viewport_selection_revision: u64, // “查看”工具直接点选只用常数修订唤醒列表高亮。
    page: usize,                      // 显式分页必须立即更新六个固定槽位。
}

/// 从会话提取列表专用身份；光标、相机、底栏状态等无关字段不会污染稳定帧。
fn water_feature_list_signature(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    selected_water_viewport: &SelectedWaterViewportState,
) -> WaterFeatureListSignature {
    WaterFeatureListSignature {
        water_revision: session.water_revision, // 水位、结构、河流或瀑布定义变化都推进该修订。
        has_water: session
            .draft
            .as_ref()
            .is_some_and(|draft| crate::authoring::terrain_documents::root_water::listed_sources(draft).iter().any(Option::is_some)), // 根水停放后仍是已载入文档。
        tool: editor.tool,                      // 非水域工具仍需清除旧活动高亮。
        selected_layer: editor.selected_water_layer, // 只复制三个常数时间索引。
        selected_river: editor.selected_water_river, // 不扫描河流数组。
        selected_waterfall: editor.selected_waterfall, // 不扫描瀑布数组。
        viewport_selection_revision: selected_water_viewport.viewport_selection_revision(), // 稳定帧不克隆水域ID。
        page: editor.water_feature_page, // 手动浏览页是纯会话态。
    }
}

/// 计算循环相邻页；只有一页时返回None，避免“翻页”误选第一项。
fn adjacent_water_feature_page(current: usize, pages: usize, previous: bool) -> Option<usize> {
    if pages <= 1 {
        return None; // 无真实相邻页时保持当前对象和工具完全不变。
    }
    let current = current.min(pages - 1); // 作者结构缩短后先钳制迟到页码。
    Some(if previous {
        (current + pages - 1) % pages // 第一页向前循环到末页。
    } else {
        (current + 1) % pages // 末页向后循环回第一页。
    })
}

/// 翻到相邻页并直选首项；`Ok(false)`明确表示单页无动作且编辑状态完全不变。
#[cfg(test)]
fn activate_adjacent_water_feature_page(
    editor: &mut SceneEditorState,
    water: &feathrise_scene_content::SceneWaterAsset,
    previous: bool,
) -> Result<bool, String> {
    let total = water_feature_count(water); // 三类正式数组只在真实分页按钮帧读取长度。
    if total == 0 {
        return Err("当前场景还没有静水体、河流或瀑布".to_owned()); // 空列表没有可绑定的稳定ID。
    }
    let pages = water_feature_page_count(total); // 最大72项只产生有限页数。
    let Some(target) = adjacent_water_feature_page(editor.water_feature_page, pages, previous)
    else {
        return Ok(false); // 单页不调用选择函数，因此工具、控制点和dirty均保持原样。
    };
    let flat_index = target.saturating_mul(WATER_FEATURE_ROW_COUNT); // 每页直选第一个稳定对象。
    let Some((kind, _, feature_id)) = water_feature_at(water, flat_index) else {
        return Err("目标水域列表页已因作者结构变化失效".to_owned()); // 不用旧页码猜测最后一项。
    };
    activate_water_feature(editor, water, kind, feature_id)?; // 稳定ID二次解析后才切工具与类内索引。
    Ok(true) // 调用方无需再修改选择或页码。
}

/// 在BSN水域对象容器下只创建一次固定六行；稳定帧不增删UI实体。
pub(crate) fn spawn_water_feature_rows(
    mut commands: Commands,
    containers: Query<Entity, Added<SceneEditorWaterFeatureList>>,
    asset_server: Res<AssetServer>,
) {
    let font = feathrise_editor_core::editor_font(&asset_server, "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"); // 与植被列表使用同一中文字体。
    for container in &containers {
        commands.entity(container).with_children(|parent| {
            for slot in 0..WATER_FEATURE_ROW_COUNT {
                parent
                    .spawn((
                        InteractionButton, // 行本身提供鼠标Interaction，不使用实体名称分派。
                        SceneEditorWaterFeatureRow {
                            slot,                                        // 固定槽位决定当前页的扁平位置。
                            kind: WaterAuthoringFeatureKind::StillWater, // 空槽使用安全默认类别，feature_id仍为None。
                            feature_id: None, // Presentation取得草稿后才绑定稳定作者身份。
                            terrain: None, // 空槽尚未绑定根水或私有网格来源。
                        },
                        Node {
                            display: Display::None,               // 没有对应对象的槽位不占布局。
                            width: percent(100),                  // 充分使用218px左栏宽度。
                            height: px(WATER_FEATURE_ROW_HEIGHT), // 与动态容器高度公式使用同一常量。
                            flex_shrink: 0.0, // 低高度由左栏滚动承担，不压扁行。
                            align_items: AlignItems::Center, // 单行文字垂直居中。
                            padding: UiRect::horizontal(px(4)), // 保留铜边内的紧凑安全距离。
                            border: UiRect::all(px(1)), // 使用一像素弱铜边表达可点击槽位。
                            overflow: Overflow::clip_x(), // 长作者名裁剪而不撑宽左栏。
                            ..default()       // 其余布局保持Bevy默认值。
                        },
                        BackgroundColor(colors::SLOT_BG), // 暗墨低饱和槽位底。
                        BorderColor::all(colors::COPPER_BORDER_WEAK), // 弱铜边避免与主工具按钮争视觉重心。
                    ))
                    .with_child((
                        SceneEditorWaterFeatureRowText, // 同步系统通过marker定向更新子文本。
                        Text::new(""),                // 首次Presentation前保持空文本。
                        TextFont {
                            font: font.clone().into(), // 每行共享同一个字体句柄。
                            font_size: ui_metrics::FONT_CONTROL.into(),     // 紧凑密度与植被作者行一致。
                            ..default()                // 其余字体属性沿用默认值。
                        },
                        TextColor(colors::TEXT_PRIMARY), // 普通行使用米白主文字。
                    ));
            }
        });
    }
}

/// 草稿、选择或页码变化时原地刷新六行，并让活动水域对象自动翻到可见页。
pub(crate) fn sync_water_feature_rows(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    selected_water_viewport: Res<SelectedWaterViewportState>, // 查看工具点选按稳定场景、类别和ID高亮同一行。
    mut last_signature: Local<Option<WaterFeatureListSignature>>,
    added_containers: Query<(), Added<SceneEditorWaterFeatureList>>, // 面板重建时即使签名稳定也必须初始化高度。
    mut containers: Query<
        &mut Node,
        (
            With<SceneEditorWaterFeatureList>, // 只调整BSN声明的水域列表外层容器。
            Without<SceneEditorWaterFeatureRow>, // 与下方可变行Node查询保持ECS借用互斥。
        ),
    >,
    mut rows: Query<
        (
            &mut SceneEditorWaterFeatureRow,
            &mut Node,
            &mut BackgroundColor,
            &Children,
            Option<&mut EditorButtonClick>,
        ),
        Without<SceneEditorWaterFeatureList>,
    >, // 行实体不会被误当作列表外层重复调整高度。
    mut row_texts: Query<
        (&mut Text, &mut TextColor),
        (
            With<SceneEditorWaterFeatureRowText>,
            Without<SceneEditorWaterFeatureListTitle>,
        ),
    >,
    mut titles: Query<
        &mut Text,
        (
            With<SceneEditorWaterFeatureListTitle>,
            Without<SceneEditorWaterFeatureRowText>,
        ),
    >,
) {
    if !editor.open {
        *last_signature = None; // 再次打开时强制以当时完整作者状态刷新一次。
        return;
    }
    let signature = water_feature_list_signature(&editor, &session, &selected_water_viewport); // 常数时间读取相关修订和选择。
    let container_added = !added_containers.is_empty(); // 新BSN容器必须绕过稳定签名早退一次。
    if last_signature.as_ref() == Some(&signature) && !container_added {
        return; // 光标和相机即使每帧改写SceneEditorState，也不扫描数组或重分配六行字符串。
    }
    let draft = session.draft.as_ref(); // 当前草稿同时提供列表数据和跨场景隔离身份。
    let water = draft.and_then(|draft| draft.water.as_ref()); // 当前草稿是列表唯一数据源。
    if let Some(water) = water {
        let layer_index = editor
            .selected_water_layer
            .min(water.layers.len().saturating_sub(1)); // 删除或撤销后钳制静水选择。
        let river_index = editor
            .selected_water_river
            .min(water.rivers.len().saturating_sub(1)); // 河流空数组安全回到零。
        let waterfall_index = editor
            .selected_waterfall
            .min(water.waterfalls.len().saturating_sub(1)); // 瀑布选择同样只修正会话态。
        if editor.selected_water_layer != layer_index {
            editor.selected_water_layer = layer_index; // 只在真实越界时写资源，避免Changed自激循环。
        }
        if editor.selected_water_river != river_index {
            editor.selected_water_river = river_index; // 不修改水域作者描述或撤销栈。
        }
        if editor.selected_waterfall != waterfall_index {
            editor.selected_waterfall = waterfall_index; // 结构变化后保留最近仍存在对象。
        }
    }
    let total = draft.map_or(0, crate::authoring::terrain_documents::root_water::listed_count); // 根水与当前私有文档分别计数，不扫描图层或全部网格。
    let pages = water_feature_page_count(total); // 空状态仍稳定为一页。
    let active = draft.and_then(|draft| listed_active_water_feature(draft, &editor, &selected_water_viewport)); // 同名ID只能高亮当前真实来源。
    let desired_page = active
        .map(|(_, _, _, flat_index)| flat_index / WATER_FEATURE_ROW_COUNT)
        .unwrap_or_else(|| editor.water_feature_page.min(pages - 1)); // 非水域工具保留作者手动浏览页。
    if editor.water_feature_page != desired_page {
        editor.water_feature_page = desired_page; // 对象直选及增删动作自动把活动项带入可见页。
    }
    for mut title in &mut titles {
        **title = if draft.is_some() {
            format!(
                "水域对象 · {total}项 · 第{}/{}页",
                editor.water_feature_page + 1,
                pages,
            )
        } else {
            "水域对象 · 等待场景作者数据".to_owned()
        }; // 标题明确统一列表范围与当前页。
    }
    let start = editor
        .water_feature_page
        .saturating_mul(WATER_FEATURE_ROW_COUNT); // 页码乘法保持防御性有界。
    let visible_rows = total.saturating_sub(start).min(WATER_FEATURE_ROW_COUNT); // 末页只保留真实对象数量，零对象得到零高度。
    for mut container in &mut containers {
        fit_water_feature_list_container(&mut container, visible_rows); // 仅作者签名或面板重建变化时触发布局更新。
    }
    for (mut row, mut node, mut background, children, click) in &mut rows {
        if let Some(mut click) = click { click.invalidate(); } // 完整作者签名更新取消旧释放意图。
        let flat_index = start.saturating_add(row.slot); // 固定槽位映射到统一列表位置。
        let Some((terrain, row_water, kind, index, feature_id)) =
            draft.and_then(|draft| listed_water_feature_at(draft, flat_index))
        else {
            row.feature_id = None; // 隐藏前清除旧稳定身份，避免空行仍可点击旧对象。
            row.terrain = None;
            node.display = Display::None; // 当前页没有该项时不占20px高度。
            continue;
        };
        row.kind = kind; // 行目标类别与稳定ID在同一次Presentation刷新中更新。
        if row.terrain.as_ref() != terrain { row.terrain = terrain.cloned(); }
        if row.feature_id.as_deref() != Some(feature_id) {
            row.feature_id = Some(feature_id.to_owned()); // 仅真实换项时克隆最多六个短稳定ID。
        }
        node.display = Display::Flex; // 有效作者对象恢复可见和可点击状态。
        let selected = active.is_some_and(|(active_terrain, active_kind, active_id, _)| {
            active_terrain == terrain && active_kind == kind && active_id == feature_id
        }); // 类型与稳定ID共同决定选中态，同名label不参与身份。
        background.0 = if selected {
            colors::SELECT_BG // 活动对象使用既有铜金选中底。
        } else {
            colors::SLOT_BG // 非活动对象保持暗墨槽位底。
        };
        let text_value = water_feature_row_text(row_water, kind, index, selected)
            .map(|text| format!("{} · {text}", if terrain.is_some() { "网格" } else { "场景" }))
            .unwrap_or_default(); // 当前快照内解析失败时宁可空文本也不显示错对象。
        for child in children.iter() {
            if let Ok((mut text, mut color)) = row_texts.get_mut(child) {
                **text = text_value.clone(); // 每行只有一个子文本，clone保持Children防御性兼容。
                color.0 = if selected {
                    colors::TEXT_ACCENT // 选中行使用金色强调。
                } else {
                    colors::TEXT_PRIMARY // 其余行保持米白可读性。
                };
            }
        }
    }
    *last_signature = Some(water_feature_list_signature(
        &editor,
        &session,
        &selected_water_viewport,
    )); // 记录钳制和自动翻页后的最终身份，下一稳定帧立即短路。
}

/// 鼠标经过和按下水域行时给出即时反馈，保持与植被作者列表相同的可点击感。
pub(crate) fn sync_water_feature_row_interactions(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    selected_water_viewport: Res<SelectedWaterViewportState>, // 悬停结束后恢复查看工具点选行背景。
    mut rows: Query<
        (
            &UiInteraction,
            &SceneEditorWaterFeatureRow,
            &mut BackgroundColor,
        ),
        Changed<UiInteraction>,
    >,
) {
    let active = session.draft.as_ref().and_then(|draft| listed_active_water_feature(draft, &editor, &selected_water_viewport)); // 交互帧也核对来源，不能把根水套到同ID私有层。
    for (interaction, row, mut background) in &mut rows {
        let Some(feature_id) = row.feature_id.as_deref() else {
            background.0 = colors::SLOT_BG; // 空槽永远不显示悬停或选中反馈。
            continue;
        };
        let selected =
            active.is_some_and(|(terrain, kind, active_id, _)| terrain == row.terrain.as_ref() && kind == row.kind && active_id == feature_id); // 来源、类别和稳定ID共同确定行。
        background.0 = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED, // 按下优先于既有选中态。
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,   // 悬停使用较弱铜金反馈。
            (UiInteraction::None, true) => colors::SELECT_BG,         // 松开后恢复活动行背景。
            (UiInteraction::None, false) => colors::SLOT_BG,          // 普通行回到暗墨槽位底。
        };
    }
}

/// 点击可见行时按稳定ID直选对象；活动笔划、控制点拖动或数值和弦期间明确拒绝串切。
pub(crate) fn handle_water_feature_row_clicks(
    rows: Query<(&EditorButtonClick, &SceneEditorWaterFeatureRow), Changed<EditorButtonClick>>,
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>,
    real_time: Res<Time<Real>>, // 玩法暂停期间仍能识别作者双击节奏。
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    stroke: Res<WaterBrushStroke>,
    network_edit: Res<WaterNetworkEditState>,
    mut selected_water_viewport: ResMut<SelectedWaterViewportState>, // 单击不移动相机，双击只登记请求。
    mut terrain_selection: Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    mut terrain_documents: Option<ResMut<crate::authoring::terrain_documents::TerrainDocumentTask>>,
    source_guard: super::WaterSourceSelectionGuard,
) {
    for (interaction, row) in &rows {
        if !interaction.is_clicked() {
            continue; // 只在真实松开点击提交一次选择。
        }
        if numeric_scrub_mouse.blocked() {
            return; // 数值和弦使用过的按键边沿就地丢弃，不在解锁后补选。
        }
        if reject_water_feature_switch(&mut editor, &session, &stroke, &network_edit) {
            return; // 行点击与列表级删除共用同一跨帧事务拒绝语义。
        }
        let Some(feature_id) = row.feature_id.as_deref() else {
            continue; // Presentation已清空的槽位不执行旧身份选择。
        };
        let before_revision = session.revision; // 仅成功切换来源时终止旧水域点击序列。
        let source_busy = source_guard.busy(&session);
        if let Err(error) = activate_listed_water_feature(&mut editor, &mut session, row.terrain.as_ref(), source_busy, row.kind, feature_id,
            terrain_selection.as_deref_mut(), terrain_documents.as_deref_mut()) {
            editor.status = error;
            return;
        }
        if session.revision != before_revision { selected_water_viewport.clear_click_sequence(); } // 同ID跨画布选择不能误组成双击。
        let Some(draft) = session.draft.as_ref() else {
            editor.status = "当前场景没有可选择的水域作者数据".to_owned();
            return; // 草稿换代边界不猜测旧行目标。
        };
        let Some(water) = draft.water.as_ref() else {
            editor.status = "当前场景没有可选择的水域作者数据".to_owned();
            return; // 草稿存在但水域文档缺失时同样拒绝旧行身份。
        };
        match row.kind {
            WaterAuthoringFeatureKind::StillWater => {
                selected_water_viewport.clear_explicit_viewport_selection(); // 列表已切入作者工具，不保留查看工具旁路身份。
                if selected_water_viewport.register_water_click(
                    &draft.scene_id,                // 双击隔离当前真实草稿场景。
                    row.kind,                     // 三类统一身份明确锁定静水类别。
                    feature_id,                   // 固定行池提交稳定静水层ID。
                    real_time.elapsed_secs_f64(), // F12暂停虚拟时间不影响双击窗口。
                ) {
                    let label = water
                        .layers
                        .get(editor.selected_water_layer)
                        .map_or(feature_id, |layer| layer.label.as_str()); // 已成功直选后取得人类可读名称。
                    editor.status = format!("正在定位静水体：{label}；等待当前完整水面"); // Cursor阶段就绪后覆盖为最终结果。
                }
            }
            _ => {
                selected_water_viewport.clear_explicit_viewport_selection(); // 河流或瀑布作者工具成为唯一选择来源。
                selected_water_viewport.clear_click_sequence(); // 河流或瀑布按下终止旧静水双击序列。
            }
        }
    }
}

/// 显式翻页按钮直选目标页第一项，保证翻页后六行不会被当前活动对象立刻拉回旧页。
pub(crate) fn handle_water_feature_page_buttons(
    buttons: Query<(&EditorButtonClick, &SceneEditorActionButton), Changed<EditorButtonClick>>,
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    stroke: Res<WaterBrushStroke>,
    network_edit: Res<WaterNetworkEditState>,
    mut selected_water_viewport: ResMut<SelectedWaterViewportState>, // 翻页动作不能与前一静水单击组成双击。
    mut terrain_selection: Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    mut terrain_documents: Option<ResMut<crate::authoring::terrain_documents::TerrainDocumentTask>>,
    source_guard: super::WaterSourceSelectionGuard,
) {
    for (interaction, button) in &buttons {
        if !interaction.is_clicked()
            || !matches!(
                button.0,
                EditorAction::WaterListPreviousPage | EditorAction::WaterListNextPage
            )
        {
            continue; // 只消费水域列表的两个专用分页动作。
        }
        if numeric_scrub_mouse.blocked() {
            return; // 公共数值拖动完整和弦独占分页按钮鼠标。
        }
        if reject_water_feature_switch(&mut editor, &session, &stroke, &network_edit) {
            return; // 分页直选与所有其他水域对象切换入口服从同一保护门。
        }
        let Some(draft) = session.draft.as_ref()
        else {
            editor.status = "当前场景没有可浏览的水域作者数据".to_owned();
            return; // 空草稿不制造虚假分页选择。
        };
        let pages = water_feature_page_count(crate::authoring::terrain_documents::root_water::listed_count(draft));
        let Some(page) = adjacent_water_feature_page(editor.water_feature_page, pages, button.0 == EditorAction::WaterListPreviousPage) else {
            editor.status = "水域对象列表当前只有一页".into();
            continue;
        };
        let Some((terrain, _, kind, _, id)) = listed_water_feature_at(draft, page.saturating_mul(WATER_FEATURE_ROW_COUNT)) else { continue; };
        let (terrain, id) = (terrain.cloned(), id.to_owned()); // 只冻结目标短身份，恢复画布不能继续借用旧槽。
        selected_water_viewport.clear_click_sequence(); // 任意显式分页都结束旧行双击时间窗。
        let source_busy = source_guard.busy(&session);
        match activate_listed_water_feature(&mut editor, &mut session, terrain.as_ref(), source_busy, kind, &id,
            terrain_selection.as_deref_mut(), terrain_documents.as_deref_mut()) {
            Ok(()) => selected_water_viewport.clear_explicit_viewport_selection(), // 作者工具接管后撤销查看工具旁路身份。
            Err(error) => editor.status = error, // 空列表或结构换代失败时保持当前选择。
        }
    }
}

#[cfg(test)]
mod tests {
    use feathrise_scene_content::SceneId; // 单页真实选择测试复用正式默认水域描述。

    use super::*; // 运行最小Presentation日程，捕获Text查询冲突和空草稿回显回归。
    use crate::authoring::water_asset_with_layer_for_tests; // 显式一层静水恰好形成单页夹具。

    #[test]
    fn water_feature_list_height_grows_only_to_page_capacity() {
        assert_eq!(water_feature_list_height(0), 0.0); // 空列表不占任何垂直空间。
        assert_eq!(water_feature_list_height(1), 24.0); // 一行包含20px本体和上下各2px内边距。
        assert_eq!(water_feature_list_height(2), 46.0); // 第二行只新增20px本体与2px行距。
        assert_eq!(water_feature_list_height(6), 134.0); // 六行恢复原单页最大高度。
        assert_eq!(water_feature_list_height(7), 134.0); // 超出单页容量后不得继续撑高面板。
    }

    #[test]
    fn water_feature_list_container_leaves_layout_only_when_empty() {
        let mut node = Node::default(); // 使用真实Bevy布局组件验证显示与高度同时切换。
        fit_water_feature_list_container(&mut node, 0); // 零对象必须完全退出父级布局。
        assert_eq!(node.display, Display::None); // 空容器不制造额外Column行距。
        assert_eq!(node.height, px(0)); // 空容器也不保留固定高度。
        fit_water_feature_list_container(&mut node, 2); // 新增两个对象后恢复实际列表。
        assert_eq!(node.display, Display::Flex); // 非空固定行池重新参与布局。
        assert_eq!(node.height, px(46)); // 两行高度只包含真实行、一个间距和上下内边距。
    }

    #[test]
    fn water_feature_presentation_uses_disjoint_title_and_row_text_queries() {
        let mut app = App::new(); // 不安装渲染或窗口插件即可验证纯UI组件系统参数。
        app.init_resource::<SceneEditorState>() // 提供打开状态、选择和页码。
            .init_resource::<EditorAuthoringSession>() // 空会话应显示等待状态。
            .init_resource::<SelectedWaterViewportState>() // 查看工具点选修订属于列表固定签名。
            .add_systems(Update, sync_water_feature_rows); // 真实系统注册会执行Bevy借用冲突校验。
        app.world_mut().resource_mut::<SceneEditorState>().open = true; // 允许Presentation进入同步分支。
        let title = app
            .world_mut()
            .spawn((
                SceneEditorWaterFeatureListTitle, // 标题实体只拥有标题marker。
                Text::new("旧标题"),            // 更新后必须替换为等待状态。
            ))
            .id(); // 保存实体身份用于更新后断言。
        app.world_mut().spawn((
            SceneEditorWaterFeatureRowText,    // 行文本marker不得与标题查询重叠。
            Text::new("旧行"),               // 没有行父节点时系统应安全忽略。
            TextColor(colors::TEXT_PRIMARY), // 满足行文本查询组件集合。
        ));
        app.update(); // 若两个可变Text查询不互斥，Bevy会在这里报告B0001。
        let text = app
            .world()
            .entity(title)
            .get::<Text>()
            .expect("标题Text组件应保留"); // 更新不得重建或删除静态BSN标题。
        assert!(text.contains("等待场景作者数据")); // 空作者会话给出明确等待说明。
    }

    #[test]
    fn presentation_signature_ignores_per_frame_cursor_and_status_changes() {
        let session = EditorAuthoringSession::default(); // 默认水域修订保持稳定。
        let mut editor = SceneEditorState::default(); // 两次签名共享相同作者选择。
        let selected_water_viewport = SelectedWaterViewportState::default(); // 默认没有查看工具水域单选。
        let before = water_feature_list_signature(&editor, &session, &selected_water_viewport); // 冻结无光标状态的列表身份。
        editor.cursor_world = Some(Vec3::new(1.0, 2.0, 3.0)); // 视口每帧可能更新严格命中。
        editor.brush_cursor_world = Some(Vec3::new(4.0, 5.0, 6.0)); // 笔刷外扩命中同样无关。
        editor.status = "仅底栏变化".to_owned(); // 状态提示不应重新格式化六行。
        assert_eq!(
            water_feature_list_signature(&editor, &session, &selected_water_viewport),
            before,
        ); // 稳定帧保持常数时间短路。
    }

    #[test]
    fn a_single_page_has_no_adjacent_target() {
        assert_eq!(adjacent_water_feature_page(0, 1, false), None); // 后页按钮没有真实目标。
        assert_eq!(adjacent_water_feature_page(0, 1, true), None); // 前页按钮同样不得循环到自身。
        assert_eq!(adjacent_water_feature_page(0, 2, false), Some(1)); // 两页时后页仍正常直选。
        assert_eq!(adjacent_water_feature_page(0, 2, true), Some(1)); // 两页时前页循环到末页。
    }

    #[test]
    fn single_page_activation_keeps_every_selection_field_unchanged() {
        let scene_id = SceneId::parse("MAP-water-single-page").expect("测试场景ID有效"); // 使用正式身份解析门。
        let water = water_asset_with_layer_for_tests(&scene_id, "scenes/MAP-water-single-page"); // 本测试显式构造唯一静水体。
        let mut editor = SceneEditorState::default(); // 构造可观察全部选择字段的会话态。
        editor.tool = EditorTool::Waterfall; // 单页无动作不得擅自切回唯一静水体。
        editor.selected_water_layer = 2; // 即使迟到索引越界，分页按钮也不能借机钳制。
        editor.selected_water_river = 3; // 河流索引必须保持原值。
        editor.selected_waterfall = 4; // 瀑布索引必须保持原值。
        editor.selected_water_control_point = Some(5); // 现有控制点选择不能被单页翻页清空。
        editor.water_feature_page = 0; // 当前唯一合法页。
        editor.dirty = true; // 纯分页拒绝不得改变既有保存责任。
        assert_eq!(
            activate_adjacent_water_feature_page(&mut editor, &water, false),
            Ok(false),
        ); // 后页按钮走真实业务函数并明确报告无相邻页。
        assert_eq!(editor.tool, EditorTool::Waterfall); // 工具类别保持不变。
        assert_eq!(editor.selected_water_layer, 2); // 静水索引保持不变。
        assert_eq!(editor.selected_water_river, 3); // 河流索引保持不变。
        assert_eq!(editor.selected_waterfall, 4); // 瀑布索引保持不变。
        assert_eq!(editor.selected_water_control_point, Some(5)); // 控制点身份保持不变。
        assert_eq!(editor.water_feature_page, 0); // 页码仍是唯一页。
        assert!(editor.dirty); // 既有dirty状态既不清除也不新增作者事务。
    }
}
