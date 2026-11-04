//! 建筑预设三列连续列表的固定预算创建、热更重绑和交互样式。

use bevy::prelude::*; // Commands、Local、Query 与 UI 样式。
use feathrise_map_content::BuildingPresetRegistry; // 当前原子目录快照。
use crate::ui::theme as colors; // 选中、悬停和普通配色。

use super::types::*; // 列表、行、按钮与状态标记。
use crate::resources::MapEditorState; // 当前规范预设选择和工具。
use crate::types::EditorTool; // 专用建筑工具保持“开始摆放”选中态。
use crate::ui::scene::FONT_PATH; // 动态行复用编辑器中文字体。

const ROW_CHANGES_PER_FRAME: usize = 32; // 增删实体固定预算，目录再大也不制造单帧峰值。
const LABEL_REBINDS_PER_FRAME: usize = 64; // 同数量热更每帧最多更新六十四个标签。

/// 列表结构与当前目录修订的渐进同步状态。
#[derive(Default)]
pub(super) struct BuildingListCache {
    target_revision: u64, // 当前正在采用的目录修订。
    target_total: usize,  // 目标规范预设数量。
    rows: Vec<Entity>,    // 按目录索引排列的真实按钮实体。
    rebind_cursor: usize, // 下一条待重绑标签。
    complete: bool,       // 只有结构与全部标签一致后才公开容器。
}

/// 在固定预算内使三列按钮与当前建筑目录完全一致。
pub(super) fn sync_building_preset_list(
    mut commands: Commands,         // 增删动态按钮通过 Deferred 统一应用。
    asset_server: Res<AssetServer>, // 新行只克隆中文字体强句柄。
    registry: Option<Res<BuildingPresetRegistry>>, // MapContent 首载前允许安全等待。
    mut cache: Local<BuildingListCache>, // 稳定帧不扫描 ECS 行集合。
    mut containers: Query<(Entity, &mut Node), With<BuildingPresetList>>, // 静态唯一挂载点。
    mut titles: Query<&mut Text, With<BuildingPresetListTitle>>, // 同步进度摘要。
    row_children: Query<&Children, With<BuildingPresetRow>>, // 重绑时定向取得唯一文字子项。
    mut texts: Query<&mut Text, Without<BuildingPresetListTitle>>, // 动态标签与标题借用分离。
) {
    let Ok((container, mut node)) = containers.single_mut() else {
        return; // BSN 静态壳尚未生成时不消费目录修订。
    };
    let Some(registry) = registry.as_deref() else {
        set_title(&mut titles, "建筑预设 · 加载中"); // 独立测试或启动首帧没有注册表。
        node.display = Display::None; // 未知索引不能接受点击。
        return;
    };
    let total = registry.presets().len(); // O(1)读取当前完整快照长度。
    if cache.target_revision != registry.revision || cache.target_total != total {
        cache.target_revision = registry.revision; // 冻结本轮目标修订。
        cache.target_total = total; // 数量变化进入结构阶段。
        cache.rebind_cursor = 0; // 现存行即使同数量也必须重绑标签。
        cache.complete = false; // 完整采用之前保持隐藏。
    }
    if cache.rows.len() > total {
        for _ in 0..ROW_CHANGES_PER_FRAME.min(cache.rows.len() - total) {
            if let Some(entity) = cache.rows.pop() {
                commands.entity(entity).try_despawn(); // 从尾部有界退役多余行及文字子项。
            }
        }
    } else if cache.rows.len() < total {
        let start = cache.rows.len(); // 新按钮索引严格等于当前连续长度。
        let end = start.saturating_add(ROW_CHANGES_PER_FRAME).min(total); // 固定本帧追加前缀。
        let font = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // Handle 克隆不读取字体文件。
        commands.entity(container).with_children(|parent| {
            for index in start..end {
                let preset = &registry.presets()[index]; // 同一系统调用内目录快照稳定。
                let entity = parent
                    .spawn((
                        Button,                                                          // 行直接拥有交互。
                        BuildingPresetButton(BuildingPresetAction::SelectPreset(index)), // 绝对索引只在完整同步后公开。
                        BuildingPresetRow { index }, // Presentation 与测试共享稳定身份。
                        row_node(),                  // 三列固定宽按钮。
                        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), // 默认暗墨底。
                        BorderColor::all(colors::COPPER_BORDER_WEAK), // 默认弱铜边。
                    ))
                    .with_child((
                        Text::new(preset.label.clone()), // 新行立即使用目标修订标签。
                        TextFont {
                            font: font.clone().into(), // 所有行共享字体资产。
                            font_size: 9.3.into(),     // 紧凑中文标签字号。
                            ..default()                // 字重和抗锯齿沿用默认值。
                        },
                        TextColor(colors::TEXT_PRIMARY), // 米白主文字。
                    ))
                    .id(); // 缓存真实实体以便后续 O(1) 定向退役和重绑。
                cache.rows.push(entity); // 数组位置始终等于目录索引。
            }
        });
    } else if cache.rebind_cursor < total {
        let end = cache
            .rebind_cursor
            .saturating_add(LABEL_REBINDS_PER_FRAME)
            .min(total); // 同数量热更也服从固定预算。
        for index in cache.rebind_cursor..end {
            let Some(entity) = cache.rows.get(index).copied() else {
                break; // 防御延迟命令与缓存异常。
            };
            let Ok(children) = row_children.get(entity) else {
                continue; // 新增行要到下一帧才进入查询。
            };
            let Some(text_entity) = children.iter().next() else {
                continue; // 损坏行不能发布为完整列表。
            };
            if let Ok(mut text) = texts.get_mut(text_entity) {
                let label = &registry.presets()[index].label; // 当前目标修订规范名称。
                if text.0 != *label {
                    text.0.clone_from(label); // 稳定名称不重复写 Changed。
                }
            }
        }
        cache.rebind_cursor = end; // 下一帧继续未处理后缀。
    } else {
        cache.complete = true; // 结构和标签已经与同一修订一致。
    }
    node.display = if cache.complete {
        Display::Flex // 原子公开全部可点击索引。
    } else {
        Display::None // 更新期间拒绝旧标签映射新目录。
    };
    let title = if total == 0 && registry.last_error.is_some() {
        "建筑预设 · 加载失败，沿用上版".to_owned() // 详情由状态卡和日志提供。
    } else if total == 0 {
        "建筑预设 · 暂无预设，可从模型新建".to_owned() // 空目录是合法首期状态。
    } else if cache.complete && registry.last_error.is_some() {
        format!("建筑预设 · {total}项 · 更新失败，沿用上版") // 旧快照仍可摆放。
    } else if cache.complete {
        format!("建筑预设 · {total}项") // 稳定态只显示真实总量。
    } else {
        format!(
            "建筑预设 · 同步中 {}/{}",
            cache.rows.len().min(total),
            total,
        ) // 作者可见有界同步进度。
    };
    set_title(&mut titles, &title); // 单一摘要实体按值写回。
}

/// 同步建筑按钮普通、悬停、按下和当前选择样式。
#[derive(Default)]
pub(super) struct BuildingStyleCache {
    selected_id: Option<String>, // 上一次已完整回显的规范建筑身份。
    tool_active: bool,           // “开始摆放”按钮只在专用建筑工具激活时保持亮色。
    catalog_revision: u64,       // 同一选择在热更重排后也必须按新索引重新解析。
}

pub(super) fn sync_building_preset_button_styles(
    editor: Res<MapEditorState>,                   // 当前规范 ID 与建筑工具。
    registry: Option<Res<BuildingPresetRegistry>>, // 动态索引解析规范项。
    mut cache: Local<BuildingStyleCache>,          // 稳定帧只处理真实交互变化。
    mut buttons: ParamSet<(
        Query<
            (
                &Interaction,
                &BuildingPresetButton,
                Option<&BuildingPresetRow>,
                &mut BackgroundColor,
                &mut BorderColor,
            ),
            (
                Or<(Changed<Interaction>, Changed<BuildingPresetButton>)>,
                Without<super::components::BuildingButtonKind>,
                Without<super::components::BuildingSidebarActionButton>,
            ), // 中央工作台采用独立主动作、导航与字段样式，不受左栏列表覆盖。
        >,
        Query<
            (
                &Interaction,
                &BuildingPresetButton,
                Option<&BuildingPresetRow>,
                &mut BackgroundColor,
                &mut BorderColor,
            ),
            (
                Without<super::components::BuildingButtonKind>,
                Without<super::components::BuildingSidebarActionButton>,
            ),
        >,
    )>,
) {
    let selected_id = editor.building_selected_preset_id.as_deref(); // 借用当前规范身份，不在稳定帧复制。
    let tool_active = editor.tool == EditorTool::Building; // 建筑工具与普通资产点放分离。
    let catalog_revision = registry.as_deref().map_or(0, |value| value.revision); // 首载前使用零身份。
    let selection_changed = cache.selected_id.as_deref() != selected_id
        || cache.tool_active != tool_active
        || cache.catalog_revision != catalog_revision; // 仅真实领域变化需要重算全部选中态。
    if selection_changed {
        for (interaction, button, row, mut background, mut border) in &mut buttons.p1() {
            sync_button_style(
                interaction,
                button,
                row,
                &mut background,
                &mut border,
                selected_id,
                tool_active,
                registry.as_deref(),
            ); // 选择切换同时清除旧行并点亮新行；不留陈旧样式。
        }
        cache.selected_id = selected_id.map(str::to_owned); // 只在真实选择变化时复制短 ID。
        cache.tool_active = tool_active;
        cache.catalog_revision = catalog_revision;
    } else {
        for (interaction, button, row, mut background, mut border) in &mut buttons.p0() {
            sync_button_style(
                interaction,
                button,
                row,
                &mut background,
                &mut border,
                selected_id,
                tool_active,
                registry.as_deref(),
            ); // 稳定帧仅处理悬停、按下或新绑定按钮。
        }
    }
}

/// 计算并写回一个建筑按钮的最终交互与选中样式。
#[allow(clippy::too_many_arguments)] // 显式值让两个有界查询分支复用完全相同的颜色规则。
fn sync_button_style(
    interaction: &Interaction,
    button: &BuildingPresetButton,
    row: Option<&BuildingPresetRow>,
    background: &mut BackgroundColor,
    border: &mut BorderColor,
    selected_id: Option<&str>,
    tool_active: bool,
    registry: Option<&BuildingPresetRegistry>,
) {
    let selected = match button.0 {
        BuildingPresetAction::SelectPreset(action_index) => {
            let index = row.map_or(action_index, |row| row.index); // 动态行的显式绑定是热更后的最终索引。
            registry
                .and_then(|registry| registry.presets().get(index))
                .is_some_and(|preset| selected_id == Some(preset.id.as_str()))
        }
        BuildingPresetAction::PlaceSelected => tool_active, // 专用工具保持活动反馈。
        _ => false,                                         // 管理、字段和参数动作不保持选中态。
    };
    let color = match (*interaction, selected) {
        (Interaction::Pressed, _) => colors::SELECT_BG_PRESSED, // 按下优先。
        (Interaction::Hovered, _) => colors::SELECT_BG_HOVER,   // 悬停提供即时反馈。
        (Interaction::None, true) => colors::SELECT_BG,         // 当前建筑或工具保持亮底。
        (Interaction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL, // 普通暗墨底。
    };
    if background.0 != color {
        background.0 = color; // 只有真实变化才触发外观更新。
    }
    let next_border = BorderColor::all(if selected {
        colors::COPPER_BORDER // 活动项使用强铜边。
    } else {
        colors::COPPER_BORDER_WEAK // 普通项保持弱边。
    });
    if *border != next_border {
        *border = next_border; // 稳定帧不重复写边框。
    }
}

/// 三列按钮布局；末行不足三项时保持原列宽。
fn row_node() -> Node {
    Node {
        width: percent(32),                      // 三列共九十六百分比，余量容纳间隔。
        height: px(24),                          // 与静态管理按钮一致。
        flex_grow: 0.0,                          // 单项末行不拉伸。
        flex_shrink: 0.0,                        // 页面滚动而不压扁按钮。
        min_width: px(0),                        // 长名称允许裁剪。
        border: UiRect::all(px(1)),              // 点击区边界。
        border_radius: BorderRadius::all(px(3)), // 统一圆角。
        align_items: AlignItems::Center,         // 垂直居中。
        justify_content: JustifyContent::Center, // 水平居中。
        padding: UiRect::horizontal(px(3)),      // 字形安全距离。
        overflow: Overflow::clip_x(),            // 超长名称裁剪。
        ..default()                              // 其余布局使用稳定默认值。
    }
}

/// 只在目录摘要真实变化时更新唯一文字实体。
fn set_title(titles: &mut Query<&mut Text, With<BuildingPresetListTitle>>, value: &str) {
    let Ok(mut text) = titles.single_mut() else {
        return; // 静态壳尚未完成。
    };
    if text.0 != value {
        text.0 = value.to_owned(); // 避免稳定帧制造 Text Changed。
    }
}
