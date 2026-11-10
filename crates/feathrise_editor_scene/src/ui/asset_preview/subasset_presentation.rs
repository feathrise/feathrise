//! GLB/GLTF子资产固定行池、分页、当前引用与选中态的增量表现同步。

use feathrise_editor_ui::UiInteraction;
use bevy::gltf::{Gltf, GltfMesh}; // 当前页最多读取五个网格短名称帮助作者识别。
use bevy::prelude::*; // Node、Text、Interaction和Assets查询。
use crate::ui::theme as colors; // 暗墨、铜边、米白与选中态沿用项目色板。

use super::interaction::{primary_reference, selected_material_reference}; // UI与剪贴板显示完全相同的引用文本。
use super::subassets::PRIMITIVES_PER_PAGE; // 固定行池和页码使用同一容量。
use super::types::*; // 子资产marker、动作、目录阶段和目标身份。

/// Blender网格名只作识别提示，实际选择和引用始终使用数字标签；蒙皮项直接标明仅完整场景。
fn primitive_label(
    choice: AssetPreviewPrimitiveChoice, // 当前槽绑定的真实数字身份。
    catalog: &AssetPreviewModelCatalog,  // 当前根句柄。
    gltfs: &Assets<Gltf>,                // 根到GltfMesh句柄的映射。
    meshes: &Assets<GltfMesh>,           // 可选作者网格名称。
) -> String {
    let name = gltfs
        .get(&catalog.gltf) // 热更窗口中根可能暂时不可读。
        .and_then(|gltf| gltf.meshes.get(choice.id.mesh)) // GLTF mesh编号为连续正式索引。
        .and_then(|handle| meshes.get(handle)) // 只读当前一项元数据。
        .map(|mesh| mesh.name.chars().take(42).collect::<String>()) // 长名称不能撑破300px侧栏。
        .unwrap_or_default(); // 没有名称时仍可用精确编号选择。
    let label = if name.is_empty() {
        // 先构造不含能力后缀的稳定数字标签。
        format!("Mesh{}/Primitive{}", choice.id.mesh, choice.id.primitive) // 稳定标签本身足够识别。
    } else {
        format!(
            "{} · Mesh{}/Primitive{}",
            name, choice.id.mesh, choice.id.primitive
        ) // 名称只在前方辅助，不改变复制结果。
    }; // 后缀不会参与资产引用或持久化身份。
    if choice.requires_skinning {
        // 让作者在点击前即可识别不能脱离骨骼层级的网格。
        format!("{label} · 蒙皮，仅完整场景") // 行仍可点击以在底栏显示完整原因，但不会切换目标。
    } else {
        // 普通静态primitive保持原显示与选择能力。
        label // 不为正常建筑、道具和武器增加冗余说明。
    } // 只构造当前页最多五个短标签，不扫描完整目录。
}

/// 只在目录或当前目标改变时原位刷新固定节点和文字；稳定帧不扫描GLTF。
pub(super) fn sync_asset_preview_subassets(
    state: Res<AssetPreviewState>, // 当前模型目录、页码和选择。
    editor: Option<Res<crate::resources::SceneEditorState>>, // 侧栏只读当前选中文件，不能复用中央其他文件的目录属性。
    media: Option<Res<super::AssetPreviewWorkspace>>,
    gltfs: Res<Assets<Gltf>>,      // Ready发布帧读取最多五项名称。
    meshes: Res<Assets<GltfMesh>>, // 当前页网格名称。
    mut nodes: Query<
        (
            &mut Node,                                 // 只切换Display，不增删UI实体。
            Option<&AssetPreviewSubAssetPanel>,        // 整个模型专属区。
            Option<&AssetPreviewPrimitiveRow>,         // 五个固定候选槽。
            Option<&AssetPreviewSubAssetPagination>,   // 多页固定操作行。
            Option<&AssetPreviewMaterialReferenceRow>, // 单体才有唯一材质引用。
            Option<&AssetPreviewSubAssetSummaryText>, // 数量只在完整目录Ready时作为属性显示。
        ),
        Or<(
            With<AssetPreviewSubAssetPanel>,
            With<AssetPreviewPrimitiveRow>,
            With<AssetPreviewSubAssetPagination>,
            With<AssetPreviewMaterialReferenceRow>,
            With<AssetPreviewSubAssetSummaryText>,
        )>,
    >, // 所有Display写入集中在一个无别名查询。
    mut texts: Query<
        (
            &mut Text,                                  // 原位替换短显示文本。
            Option<&AssetPreviewSubAssetSummaryText>,   // 根状态与真实总数。
            Option<&AssetPreviewPrimitiveRowText>,      // 当前页五槽标签。
            Option<&AssetPreviewPageText>,              // 页码。
            Option<&AssetPreviewPrimaryReferenceText>,  // Scene0或Mesh完整引用。
            Option<&AssetPreviewMaterialReferenceText>, // 原材质/std引用。
            Option<&AssetPreviewSubAssetStatusText>,   // 未Ready目录进度/失败交给公共底栏。
        ),
        Or<(
            With<AssetPreviewSubAssetSummaryText>,
            With<AssetPreviewPrimitiveRowText>,
            With<AssetPreviewPageText>,
            With<AssetPreviewPrimaryReferenceText>,
            With<AssetPreviewMaterialReferenceText>,
            With<AssetPreviewSubAssetStatusText>,
        )>,
    >, // 文本marker互斥但共用一个可变Text查询。
    mut buttons: Query<
        (
            Ref<UiInteraction>,     // 悬停边沿可单独刷新颜色。
            &AssetPreviewAction,  // 判断Scene0、槽位、分页或复制动作。
            &mut BackgroundColor, // 当前选中使用稳定铜墨底。
            &mut BorderColor,     // 选中或悬停强化铜边。
            Option<&mut feathrise_editor_ui::EditorButtonClick>, // 固定槽换文件或翻页时取消旧按住。
        ),
        Without<AssetPreviewNavigationButton>,
    >, // 缩放工具的表现仍由原系统维护。
    mut binding: Local<Option<(u64, usize)>>,
) {
    let same_selection = media.as_ref().is_some_and(|media| media.central_active)
        || editor.as_ref().is_none_or(|editor| editor.selected_asset == state.selected_path);
    let identity_changed = editor.as_ref().is_some_and(|editor| editor.is_changed())
        || media.as_ref().is_some_and(|media| media.is_changed());
    let model_selected = same_selection && state.external_preview.is_none() // 装备作者使用完整模型，隐藏不生效的primitive选择区。
        && matches!(state.content, AssetPreviewContent::Model { .. }); // 角色必须保留完整骨架和武器，隐藏静态primitive选择区。
    let catalog = state.model.as_ref().map(|model| &model.catalog); // 当前模型唯一目录。
    let catalog_ready = catalog.is_some_and(|catalog| {
        catalog.phase == AssetPreviewCatalogPhase::Ready // 不显示枚举中半套候选。
    });
    let page = catalog.map_or(0, |catalog| catalog.page); // 非模型使用安全零页。
    let total = catalog.map_or(0, |catalog| catalog.choices.len()); // O(1)真实总数。
    let pages = total.div_ceil(PRIMITIVES_PER_PAGE).max(1); // 零项也使用1/1安全显示值。
    let material_visible = state
        .model
        .as_ref()
        .is_some_and(|model| model.selected_primitive.is_some()); // 单体加载期间也提前显示精确原材质。
    if state.is_changed() || identity_changed {
        for (mut node, panel, row, pagination, material_row, summary_text) in &mut nodes {
            let display = if panel.is_some() {
                model_selected // 模型文件始终显示目录进度、Scene0和引用。
            } else if let Some(row) = row {
                catalog_ready
                    && catalog.is_some_and(|catalog| {
                        catalog // 当前页槽必须绑定真实完整候选。
                            .choices
                            .get(page * PRIMITIVES_PER_PAGE + row.0)
                            .is_some()
                    })
            } else if pagination.is_some() {
                catalog_ready && pages > 1 // 单页不保留空分页高度。
            } else if material_row.is_some() {
                model_selected && material_visible // Scene0没有唯一材质行。
            } else if summary_text.is_some() {
                model_selected && catalog_ready
            } else {
                false // 查询过滤保证不会到达未知marker。
            };
            let wanted = if display {
                Display::Flex
            } else {
                Display::None
            }; // 所有目标都是Flex容器或按钮。
            if node.display != wanted {
                node.display = wanted; // 真实变化才标记布局脏。
            }
        }
        let labels: [String; PRIMITIVES_PER_PAGE] = std::array::from_fn(|slot| {
            catalog
                .filter(|_| catalog_ready) // Ready前不读取部分候选。
                .and_then(|catalog| {
                    catalog
                        .choices
                        .get(page * PRIMITIVES_PER_PAGE + slot)
                        .map(|choice| primitive_label(*choice, catalog, &gltfs, &meshes))
                })
                .unwrap_or_default() // 隐藏槽清空旧文件名称。
        }); // 固定五个短字符串，不随总候选数量创建UI。
        let summary = catalog.filter(|_| model_selected)
            .map(|catalog| catalog.status.as_str()) // 根加载、枚举进度或完整数量。
            .unwrap_or(""); // 非模型面板隐藏并清空旧文案。
        let page_text = format!("{}/{}", page.saturating_add(1), pages); // 页码不冒充资产总量。
        let primary = primary_reference(&state).unwrap_or_default(); // Scene0或当前primitive完整引用。
        let material = selected_material_reference(&state).unwrap_or_default(); // Scene0时为空且行隐藏。
        for (mut text, summary_text, row_text, page_marker, primary_text, material_text, status_text) in
            &mut texts
        {
            let wanted = if summary_text.is_some() {
                if catalog_ready { summary } else { "" } // 完整数量是属性；枚举进度不再插入正文。
            } else if status_text.is_some() {
                if catalog_ready { "" } else { summary }
            } else if let Some(row) = row_text {
                labels[row.0].as_str() // 固定槽位对应当前页短标签。
            } else if page_marker.is_some() {
                page_text.as_str() // 当前页/总页。
            } else if primary_text.is_some() {
                primary.as_str() // 复制按钮实际使用的同一主引用。
            } else if material_text.is_some() {
                material.as_str() // 复制按钮实际使用的同一材质引用。
            } else {
                continue; // 查询过滤的防御分支。
            };
            if text.as_str() != wanted {
                **text = wanted.to_owned(); // 相同文本不重复分配或触发布局。
            }
        }
    }
    let target = state.model.as_ref().map(|model| model.target); // 选中态只读取一个小枚举。
    let identity = state.model.as_ref().map(|model| (model.generation, model.catalog.page));
    let binding_changed = *binding != identity;
    *binding = identity;
    for (interaction, action, mut background, mut border, click) in &mut buttons {
        if binding_changed && let Some(mut click) = click { click.invalidate(); }
        if !state.is_changed() && !interaction.is_changed() {
            continue; // 稳定按钮不做颜色写入。
        }
        let active = match *action {
            AssetPreviewAction::ShowScene0 => {
                target == Some(AssetPreviewModelTarget::Scene0) // 默认完整场景选中态。
            }
            AssetPreviewAction::ShowPrimitive(slot) => catalog
                .and_then(|catalog| catalog.choices.get(page * PRIMITIVES_PER_PAGE + slot))
                .is_some_and(|choice| {
                    target == Some(AssetPreviewModelTarget::Primitive(choice.id)) // 精确数字身份选中态。
                }),
            _ => false, // 分页和复制按钮没有持久选中态。
        };
        let color = match (*interaction, active) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED, // 按下即时强反馈。
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,   // 悬停使用铜金高亮。
            (UiInteraction::None, true) => colors::SELECT_BG, // 当前Scene0或primitive保持选中底。
            (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL, // 普通暗墨按钮。
        };
        background.set_if_neq(BackgroundColor(color)); // 无差异不触发渲染更新。
        let border_color = if active || *interaction != UiInteraction::None {
            colors::COPPER_BORDER // 当前或交互中使用强化铜边。
        } else {
            colors::COPPER_BORDER_WEAK // 普通项保持低对比。
        };
        border.set_if_neq(BorderColor::all(border_color)); // 与同栏现有按钮统一。
    }
}
