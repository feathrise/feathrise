//! 连续预设按钮的有界结构同步、目录重绑与回归测试。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use std::ops::Range; // 追加按钮使用不分配中间集合的连续索引范围。

use bevy::prelude::*; // Commands、Query、Local以及Bevy UI组件。
use feathrise_scene_content::VegetationPresetRegistry; // 当前已原子采用的完整开发者目录。
use crate::ui::theme as colors; // 动态按钮复用水墨铜木配色。

use crate::components::{
    SceneEditorVegetationPresetList, SceneEditorVegetationPresetListTitle,
    SceneEditorVegetationPresetRow, SceneEditorVegetationPresetRowText,
}; // 连续列表容器、真实索引按钮和两类文字marker。
use crate::ui::vegetation_preset_editor::{PresetEditorAction, PresetEditorButton}; // 目录按钮由植被预设域直接消费，不再依赖通用场景动作链。

use super::super::scene::FONT_PATH; // 动态文字使用F12已经登记的中文字体。

const PRESET_BUTTON_CHANGES_PER_FRAME: usize = 24; // 每帧最多增删二十四个按钮根及其固定一个文字子项。
const PRESET_LABEL_REBINDS_PER_FRAME: usize = 64; // 同数量热更每帧最多重绑六十四个现有短标签。

/// 一次连续列表目标；目录错误只影响摘要，上一有效快照仍可完整显示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PresetListSignature {
    container: Entity,     // BSN容器重建后必须丢弃旧实体索引缓存。
    revision: Option<u64>, // None明确表示全局目录资源尚未登记。
    total: usize,          // 最终按钮数直接等于真实预设数，不存在分页容量。
    has_error: bool,       // 热更失败时摘要显示“沿用上版”。
}

/// 系统本地同步游标；实体向量避免跨帧反复扫描任意长Children。
#[derive(Default)]
pub(crate) struct PresetListSyncState {
    signature: Option<PresetListSignature>, // 已开始同步的冻结目标。
    rows: Vec<Entity>,                      // 按目录索引排列的按钮实体。
    rebind_cursor: usize,                   // 当前修订已重绑的旧按钮前缀长度。
    rebind_target: usize,                   // 修订到达时已经存在且必须复验的按钮数。
}

/// 让三列连续按钮与当前目录保持完整一致；处理中隐藏容器，避免旧标签点击到新索引。
#[allow(clippy::too_many_arguments)] // 每个Query拥有明确互斥组件，合并后反而会扩大逐项访问。
pub(crate) fn sync_vegetation_preset_list(
    mut commands: Commands, // 结构变化只通过Deferred Commands在主世界采用。
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    asset_server: Res<AssetServer>, // 新按钮只克隆统一字体句柄，不等待字体IO。
    registry: Option<Res<VegetationPresetRegistry>>, // 缺目录资源时显示等待态并清空旧按钮。
    mut containers: Query<(Entity, &mut Node), With<SceneEditorVegetationPresetList>>, // 唯一连续列表容器。
    rows: Query<(&SceneEditorVegetationPresetRow, &Children)>, // 已创建按钮的稳定索引与文字子项。
    mut row_texts: Query<
        &mut Text, // 只修改动态按钮的短标签。
        (
            With<SceneEditorVegetationPresetRowText>, // 限定真实按钮子文字。
            Without<SceneEditorVegetationPresetListTitle>, // 与下方摘要Text查询显式互斥。
        ),
    >,
    mut titles: Query<
        &mut Text, // 只修改固定目录摘要。
        (
            With<SceneEditorVegetationPresetListTitle>, // 限定唯一摘要实体。
            Without<SceneEditorVegetationPresetRowText>, // 与上方按钮Text查询显式互斥。
        ),
    >,
    mut state: Local<PresetListSyncState>, // 跨帧保留固定预算游标与按钮实体前缀。
) {
    let Ok((container, mut list_node)) = containers.single_mut() else {
        // 短暂没有唯一容器时保留上一身份；新容器出现后再按Entity代际清理旧缓存。
        // 这样场景切换中的一帧重复容器不会让仍存活的旧容器被再次追加重复按钮。
        return; // 无唯一容器时不产生孤立按钮实体。
    };
    let total = registry.as_deref().map_or(0, |value| value.presets().len()); // 真实目录长度就是最终展示数。
    let signature = PresetListSignature {
        container,                                                 // 当前唯一BSN容器身份。
        revision: registry.as_deref().map(|value| value.revision), // 成功采用修订驱动内容重绑。
        total,                                                     // 无页大小、末页或最大页数派生。
        has_error: registry
            .as_ref()
            .is_some_and(|value| value.last_error.is_some()), // 失败沿用旧快照。
    };
    let signature_changed = state.signature != Some(signature); // 稳定帧只做常数身份比较。
    if signature_changed {
        if state
            .signature
            .is_some_and(|old| old.container != container)
        {
            state.rows.clear(); // 新容器不能引用已经随旧层级销毁的实体。
        }
        state.signature = Some(signature); // 冻结本轮目录目标。
        state.rebind_cursor = 0; // 目录重排或改名必须从首项重新绑定。
        state.rebind_target = state.rows.len().min(total); // 新增尾项在创建时已写当前标签，无需再次遍历。
        list_node.display = Display::None; // 重绑完成前阻止旧标签触发新目录索引。
    }
    let structure_complete = state.rows.len() == total; // 按钮根数量必须与目录逐项相等。
    let binding_complete = state.rebind_cursor >= state.rebind_target; // 旧按钮标签必须全部切到当前修订。
    if !signature_changed && structure_complete && binding_complete
        && !locale.as_ref().is_some_and(|locale| locale.is_changed())
    {
        return; // 稳定帧不扫描目录、实体向量、Children或文字。
    }

    reconcile_button_count(
        &mut commands,       // 本帧只登记固定数量结构命令。
        container,           // 新按钮始终挂到当前唯一列表。
        registry.as_deref(), // 新增按钮直接读取当前完整目录标签。
        &asset_server,       // 字体句柄由统一AssetServer产生。
        &mut state.rows,     // 按索引维护连续实体前缀。
        total,               // 最终数量不做分页截断。
    );
    let rebind_target = state.rebind_target; // 复制小标量，避免与行向量的共享借用重叠。
    let mut rebind_cursor = state.rebind_cursor; // 局部推进完成后再一次写回Local状态。
    rebind_existing_labels(
        registry.as_deref(), // 当前目录缺失时不会存在正数重绑目标。
        &state.rows,         // 只访问缓存实体的固定游标切片。
        &rows,               // 取得每个按钮唯一文字子项。
        &mut row_texts,      // 原位替换标签，不重建现有实体。
        &mut rebind_cursor,  // 成功处理后推进有界前缀。
        rebind_target,       // 只重绑修订到达前已存在的按钮。
    );
    state.rebind_cursor = rebind_cursor; // 本帧重绑批次完整结束后原子采用新游标。

    let structure_complete = state.rows.len() == total; // Deferred命令已登记时实体身份向量即可代表最终前缀。
    let binding_complete = state.rebind_cursor >= state.rebind_target; // 标签游标完成才允许重新命中。
    let complete = structure_complete && binding_complete; // 结构与内容必须联合完成。
    list_node.display = if complete && total > 0 {
        Display::Flex // 所有真实按钮一次恢复为三列连续列表。
    } else {
        Display::None // 等待、空目录和同步中都不暴露半批按钮。
    };
    let summary = preset_list_summary(
        registry.as_deref(), // 区分等待、空目录和沿用上版。
        total,               // 摘要只显示总数，不再显示页码。
        state.rows.len(),    // 结构同步期间回显已登记按钮数。
        state.rebind_cursor, // 同数量热更期间回显已重绑前缀。
        state.rebind_target, // 计算当前阶段总工作量。
        complete,            // 完成后切换为稳定简短摘要。
    );
    let locale = locale.as_deref().copied().unwrap_or_default();
    let summary = locale.editor_notice(&summary);
    for mut title in &mut titles {
        if title.as_str() != summary.as_ref() {
            **title = summary.to_string(); // 短暂双界面实例也取得同一目录状态。
        }
    }
}

/// 在固定预算内从尾部增删按钮；最终数量只由目录决定。
fn reconcile_button_count(
    commands: &mut Commands, // 结构操作回到主世界Deferred队列。
    container: Entity,       // 当前连续列表父实体。
    registry: Option<&VegetationPresetRegistry>, // 新增时提供完整标签快照。
    asset_server: &AssetServer, // 创建共享字体句柄。
    row_entities: &mut Vec<Entity>, // 顺序与目录索引一一对应。
    expected: usize,         // 真实预设总数。
) {
    if row_entities.len() > expected {
        let remove_count = (row_entities.len() - expected).min(PRESET_BUTTON_CHANGES_PER_FRAME); // 尾部回收受固定预算约束。
        for _ in 0..remove_count {
            if let Some(entity) = row_entities.pop() {
                commands.entity(entity).try_despawn(); // 按钮及其固定文字子项随根一并回收。
            }
        }
        return; // 同帧不同时增删，保持父级Children顺序简单确定。
    }
    if row_entities.len() >= expected {
        return; // 数量已经完整时只剩可能的标签重绑。
    }
    let Some(registry) = registry else {
        return; // 无目录快照不能伪造按钮名称或动作索引。
    };
    let start = row_entities.len(); // 新增范围紧接当前连续前缀。
    let end = start
        .saturating_add(PRESET_BUTTON_CHANGES_PER_FRAME)
        .min(expected); // 固定预算不会成为最终数量限制。
    let font = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // Handle克隆为O(1)，实际字体加载继续异步。
    spawn_preset_button_range(
        commands,     // 统一登记父子实体。
        container,    // 维持三列容器Children顺序。
        start..end,   // 本帧唯一连续新增区间。
        registry,     // 每个按钮直接取得当前目录标签。
        &font,        // 全部新增按钮共享一个字体句柄。
        row_entities, // 立即记录已分配实体身份供后续帧定位。
    );
}

/// 对目录修订到达前已经存在的按钮按固定预算更新标签。
fn rebind_existing_labels(
    registry: Option<&VegetationPresetRegistry>, // 已采用目录是唯一标签来源。
    row_entities: &[Entity],                     // 稳定索引实体向量。
    rows: &Query<(&SceneEditorVegetationPresetRow, &Children)>, // 行marker和唯一文字子项。
    row_texts: &mut Query<
        &mut Text, // 只写按钮标签。
        (
            With<SceneEditorVegetationPresetRowText>, // 排除其他编辑器文字。
            Without<SceneEditorVegetationPresetListTitle>, // 保持B0001互斥证据。
        ),
    >,
    cursor: &mut usize, // 已完成前缀。
    target: usize,      // 本修订需要复验的旧按钮数。
) {
    let Some(registry) = registry else {
        *cursor = target; // 空目标没有可绑定文本，结构回收负责清理旧按钮。
        return;
    };
    let end = cursor
        .saturating_add(PRESET_LABEL_REBINDS_PER_FRAME)
        .min(target); // 每帧文字改写保持固定上限。
    for index in *cursor..end {
        let Some(entity) = row_entities.get(index).copied() else {
            break; // 结构尚未补齐时保留游标，下帧继续。
        };
        let Ok((row, children)) = rows.get(entity) else {
            break; // Deferred新增尚未可查询时不越过未验证实体。
        };
        let Some(preset) = registry.presets().get(index) else {
            break; // 目录再次换代会在下帧重置完整签名和游标。
        };
        debug_assert_eq!(row.index, index); // 实体向量与marker必须保持同一连续索引。
        for child in children.iter() {
            if let Ok(mut text) = row_texts.get_mut(child) {
                if **text != preset.label {
                    **text = preset.label.clone(); // 只在真实改名或重排时分配新标签。
                }
            }
        }
        *cursor = index + 1; // 当前按钮完整重绑后才推进公开前缀。
    }
}

/// 追加一段真实预设按钮；三列布局由父容器FlexWrap决定。
fn spawn_preset_button_range(
    commands: &mut Commands,             // Deferred父子创建入口。
    container: Entity,                   // 连续列表父实体。
    indices: Range<usize>,               // 本帧固定预算索引段。
    registry: &VegetationPresetRegistry, // 完整已校验目录。
    font: &Handle<Font>,                 // 共享中文字体句柄。
    row_entities: &mut Vec<Entity>,      // 输出新按钮实体的顺序身份。
) {
    commands.entity(container).with_children(|parent| {
        for index in indices {
            let Some(preset) = registry.presets().get(index) else {
                break; // 目录身份在同一系统调用内本应稳定，防御性拒绝伪造尾项。
            };
            let entity = parent
                .spawn((
                    InteractionButton, // 真实预设行直接接收Interaction。
                    PresetEditorButton(PresetEditorAction::SelectCatalogPreset(index)), // 动作携带当前完整目录的绝对索引，并进入专用预设输入链。
                    SceneEditorVegetationPresetRow { index }, // 结构测试与标签重绑共享稳定索引。
                    preset_button_node(), // 每个按钮固定三分之一宽和统一点击高度。
                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), // 普通态使用暗墨按钮底。
                    BorderColor::all(colors::COPPER_BORDER_WEAK), // 一像素弱铜边保持武侠编辑器层级。
                ))
                .with_child((
                    SceneEditorVegetationPresetRowText, // 唯一文字子项供有界重绑定位。
                    Text::new(preset.label.clone()),  // 新增按钮立即使用当前完整目录名称。
                    preset_button_font(font.clone()), // Handle克隆不复制字体资产。
                    TextColor(colors::TEXT_PRIMARY),  // 米白主文字保持与静态作者按钮一致。
                    TextLayout::no_wrap(), // 窄栏长名称只横向裁切，完整所选名由可换行摘要回显，不撑高24px行。
                ))
                .id(); // 立即取得Deferred实体身份建立连续缓存。
            row_entities.push(entity); // 索引位置与目录位置严格一致。
        }
    });
}

/// 构造三列连续按钮布局；最后不足三项时保持原列宽而不横向拉伸。
fn preset_button_node() -> Node {
    Node {
        width: percent(32),                      // 三列共96%，剩余空间容纳两道3px间隔。
        height: px(24),                          // 与F12静态小按钮保持同一紧凑行高。
        flex_grow: 0.0,                          // 最后一行不把一个预设拉成整行按钮。
        flex_shrink: 0.0,                        // 左栏滚动承接总高度，不压扁点击区。
        min_width: px(0),                        // 长中文名不能反向撑宽侧栏。
        border: UiRect::all(px(1)),              // 一像素铜边保持统一槽位语言。
        border_radius: BorderRadius::all(px(3)), // 与现有开发按钮使用相同圆角。
        align_items: AlignItems::Center,         // 标签垂直居中。
        justify_content: JustifyContent::Center, // 三列文字水平居中。
        padding: UiRect::horizontal(px(3)),      // 中文标签与边框保留安全距离。
        overflow: Overflow::clip_x(),            // 极长名称裁剪显示但完整值仍保存在目录。
        ..default()                              // 其余布局采用Bevy稳定默认值。
    }
}

/// 构造动态按钮共享字体组件。
fn preset_button_font(font: Handle<Font>) -> TextFont {
    TextFont {
        font: font.into(),     // 动态实体使用已登记字体强句柄。
        font_size: ui_metrics::FONT_CONTROL.into(), // 与静态三列作者按钮保持相同字号。
        ..default()            // 字重和抗锯齿沿用字体默认值。
    }
}

/// 返回无页码目录摘要；同步进度只描述当前有界UI采用，不伪造目录状态。
fn preset_list_summary(
    registry: Option<&VegetationPresetRegistry>, // 区分资源未登记和已采用空目录。
    total: usize,                                // 当前完整快照总数。
    row_count: usize,                            // 已登记按钮实体数。
    rebind_cursor: usize,                        // 已重绑旧按钮数。
    rebind_target: usize,                        // 当前修订需重绑旧按钮总数。
    complete: bool,                              // 结构与标签是否联合完成。
) -> String {
    match registry {
        None => "预设库 · 加载中".to_owned(), // 启动阶段不显示虚假零项。
        Some(value) if total == 0 && value.last_error.is_some() => {
            "预设库 · 加载失败，请查看底部状态".to_owned() // 提示错误详情所在位置。
        }
        Some(_) if total == 0 => "预设库 · 暂无预设，可在上方新建".to_owned(), // 空目录给出下一步。
        Some(value) if complete && value.last_error.is_some() => {
            format!("预设库 · {total}项 · 更新失败，沿用上版") // 明确旧列表可用但更新没有成功。
        }
        Some(_) if complete => format!("预设库 · {total}项"), // 稳定态只显示名称与总数。
        Some(_) if row_count != total => {
            format!(
                "预设库 · 加载中 {}/{total}", // 作者无需知道UI实体的同步过程。
                row_count.min(total)
            ) // 结构补齐进度。
        }
        Some(_) => format!(
            "预设库 · 更新中 {}/{}", // 保留真实进度，简化内部实现用语。
            rebind_cursor.min(rebind_target),
            rebind_target
        ), // 同数量目录热更期间保持列表隐藏直到名称全部一致。
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin; // 动态字体句柄需要最小AssetServer。
    use feathrise_scene_content::{
        ResolvedVegetationPresetCatalog, VEGETATION_PRESET_SCHEMA_VERSION,
        VegetationPresetCatalogAsset, VegetationPresetDescriptor,
    }; // 构造真实可通过公共校验的任意长度目录。

    use super::*; // 测试直接运行生产同步系统和正式固定预算。

    /// 读取一份完整数据Mesh模板；一千项UI测试不会把LOD参数隐藏到Rust适配器。
    fn test_template() -> VegetationPresetDescriptor {
        let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vegetation_presets.vegetation_presets.json"
        )))
        .expect("随包数据预设目录必须可解析");
        catalog
            .presets
            .into_iter()
            .find(|preset| preset.id == "hn01_rock")
            .expect("随包岩石数据预设必须存在")
    }

    /// 从完整数据模板复制一个身份互不冲突的Mesh预设。
    fn test_preset(
        template: &VegetationPresetDescriptor,
        index: usize,
        label_prefix: &str,
    ) -> VegetationPresetDescriptor {
        let id = format!("preset-{index:04}"); // 固定宽度确保目录顺序与测试索引直观一致。
        let mut preset = template.clone(); // 真实LOD、材质、派生网格与碰撞结构全部保留。
        preset.id = id.clone(); // 规范身份按测试索引唯一。
        preset.aliases.clear(); // 测试不需要历史迁移名称。
        preset.label = format!("{label_prefix}{index}"); // 热更测试只改变此前缀。
        preset.output_id = id; // 输出身份与规范身份保持一一对应。
        preset.runtime_layer_id = Some(index as u64 + 1); // 显式非零且无跨项冲突。
        preset.prototype_id = Some(index as u64 + 10_001); // Mesh原型身份与层身份分域。
        preset.resolution = [2, 2]; // 公共校验允许的最小画布。
        preset.spacing_meters = 1.0; // 有限正候选间距。
        preset.scale_range = [1.0, 1.0]; // 固定合法缩放范围。
        preset.seed = Some(index as u64 + 20_001); // 确定性非冲突散布种子。
        preset
    }

    /// 构造指定数量和修订的真实已校验注册表。
    fn test_registry(total: usize, revision: u64, label_prefix: &str) -> VegetationPresetRegistry {
        let template = test_template(); // 大目录复制同一数据结构，只改变稳定身份。
        let asset = VegetationPresetCatalogAsset {
            schema_version: VEGETATION_PRESET_SCHEMA_VERSION, // 使用当前正式目录版本。
            presets: (0..total)
                .map(|index| test_preset(&template, index, label_prefix))
                .collect(), // 测试数量不经过任何页大小截断。
        };
        VegetationPresetRegistry {
            catalog: Handle::default(), // 测试不触发真实文件热加载。
            revision,                   // 显式修订驱动同数量标签重绑。
            resolved: ResolvedVegetationPresetCatalog::build(&asset)
                .expect("测试目录必须通过正式公共校验"), // 生产系统读取同一种解析快照。
            last_error: None,           // 正常目录无需沿用上版诊断。
        }
    }

    /// 返回测试World中连续预设按钮数量。
    fn preset_row_count(world: &mut World) -> usize {
        let mut rows = world.query::<&SceneEditorVegetationPresetRow>(); // 只读取真实按钮marker。
        rows.iter(world).count() // 不依赖ECS实体迭代顺序。
    }

    /// 返回指定目录索引按钮的当前文字。
    fn preset_row_label(world: &mut World, target: usize) -> String {
        let text_entity = {
            let mut rows = world.query::<(&SceneEditorVegetationPresetRow, &Children)>(); // 从稳定索引定位唯一文字子项。
            rows.iter(world)
                .find(|(row, _)| row.index == target)
                .and_then(|(_, children)| children.iter().next())
                .expect("目标预设按钮必须拥有文字子项") // 完成列表不允许缺少标签实体。
        };
        world
            .get::<Text>(text_entity)
            .expect("目标子项必须持有Text")
            .to_string() // 返回拥有型文本避免跨World借用。
    }

    /// 返回指定目录索引按钮的专用植被预设动作。
    fn preset_row_action(world: &mut World, target: usize) -> PresetEditorAction {
        let mut rows = world.query::<(&SceneEditorVegetationPresetRow, &PresetEditorButton)>(); // 同一实体必须同时携带显示索引和专用输入动作。
        rows.iter(world)
            .find(|(row, _)| row.index == target)
            .map(|(_, button)| button.0)
            .expect("目标预设按钮必须进入植被预设专用输入链") // 通用场景按钮链曾导致整组预设受场景输入门阻塞。
    }

    #[test] // 一千项必须连续显示且创建过程服从固定帧预算。
    fn thousand_presets_build_without_pagination_and_rebind_atomically() {
        let mut app = App::new(); // 最小ECS应用足以执行真实Deferred父子创建。
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Font>() // AssetServer加载中文字体句柄前登记Font资产类型。
            .insert_resource(test_registry(1_000, 1, "预设")) // 远超旧六槽容量的正式目录。
            .add_systems(Update, sync_vegetation_preset_list); // 运行生产有界同步系统。
        app.world_mut().spawn((
            SceneEditorVegetationPresetList, // 模拟BSN连续列表容器。
            Node::default(),               // 系统负责最终Display状态。
        ));
        app.world_mut().spawn((
            SceneEditorVegetationPresetListTitle, // 模拟固定目录摘要。
            Text::new(""),                      // 首帧由生产系统写入同步进度。
        ));

        app.update(); // 第一帧只能创建产品固定预算允许的前缀。
        assert_eq!(
            preset_row_count(app.world_mut()),
            PRESET_BUTTON_CHANGES_PER_FRAME
        ); // 无分页不等于单帧无界生成。
        let total_build_frames = 1_000_usize.div_ceil(PRESET_BUTTON_CHANGES_PER_FRAME); // 完整列表需要的确定帧数。
        for _ in 1..total_build_frames {
            app.update(); // 后续帧持续从尾部补齐连续索引。
        }
        assert_eq!(preset_row_count(app.world_mut()), 1_000); // 最终每个目录项都有真实按钮。
        assert_eq!(preset_row_label(app.world_mut(), 999), "预设999"); // 第千项无需翻页即可存在于滚动列表。
        assert_eq!(
            preset_row_action(app.world_mut(), 0), // 第一项覆盖普通草的零索引回归。
            PresetEditorAction::SelectCatalogPreset(0), // 选择由预设域直接消费，不再依赖通用EditorAction链。
        ); // 所有连续目录按钮使用同一种专用交互协议。
        assert_eq!(
            preset_row_action(app.world_mut(), 999), // 第千项同样不能退回分页或通用按钮分派。
            PresetEditorAction::SelectCatalogPreset(999), // 绝对目录索引完整保留到专用动作。
        ); // 无硬编码上限与可点击性由同一测试共同保护。

        app.insert_resource(test_registry(1_000, 2, "新版")); // 同数量热更模拟目录重排或改名修订。
        app.update(); // 首批六十四项重绑时完整容器必须保持隐藏。
        let display_during_rebind = {
            let mut containers = app
                .world_mut()
                .query_filtered::<&Node, With<SceneEditorVegetationPresetList>>(); // 读取真实容器Display。
            containers
                .single(app.world())
                .expect("测试必须只有一个连续列表")
                .display // 拷贝Display后结束World借用。
        };
        assert_eq!(display_during_rebind, Display::None); // 不允许旧名称点击到新目录索引。
        let total_rebind_frames = 1_000_usize.div_ceil(PRESET_LABEL_REBINDS_PER_FRAME); // 同数量热更固定完成帧数。
        for _ in 1..total_rebind_frames {
            app.update(); // 按六十四项预算继续重绑旧按钮。
        }
        assert_eq!(preset_row_label(app.world_mut(), 999), "新版999"); // 末项最终采用新修订标签。
        let final_display = {
            let mut containers = app
                .world_mut()
                .query_filtered::<&Node, With<SceneEditorVegetationPresetList>>(); // 再次读取最终容器状态。
            containers
                .single(app.world())
                .expect("测试必须只有一个连续列表")
                .display // 完整同步后应恢复Flex命中。
        };
        assert_eq!(final_display, Display::Flex); // 结构和名称联合完成后一次公开全部按钮。
    }
}
