//! 资产预览子列表、分页和复制按钮交互；只改变临时预览会话，不写作者数据。

use bevy::clipboard::Clipboard; // 显式复制按钮复用项目唯一系统剪贴板资源。
use bevy::prelude::*; // Interaction、Commands、AssetServer和ECS资源。

use super::resources::AssetPreviewNavigation; // 切换目标时恢复新对象的完整取景。
use super::subassets::{
    // 引用文字与固定分页容量只维护一份规则。
    PRIMITIVES_PER_PAGE, // 行槽到真实候选下标的换算基准。
    material_reference,  // 原材质/std完整引用。
    primitive_reference, // MeshN/PrimitiveN完整引用。
    scene_reference,     // 明确Scene0而非整个多Scene文件。
};
use super::types::*; // 按钮动作、模型会话和当前阶段。
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState}; // F12身份与统一鼠标门。
use crate::ui::asset_context_menu::helpers::copy_asset_text_status; // 与右键路径菜单共用真实剪贴板反馈。
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState}; // 隐藏资产页不接收旧按钮动作。

/// 当前目标的主引用；单体与完整Scene0都显式带Bevy标签。
pub(super) fn primary_reference(state: &AssetPreviewState) -> Option<String> {
    let source = state.selected_path.as_deref()?; // 文件身份来自当前F12选择。
    let model = state.model.as_ref()?; // 图片和数据没有GLTF子资产引用。
    Some(match model.target {
        AssetPreviewModelTarget::Scene0 => scene_reference(source), // 默认完整场景明确为Scene0。
        AssetPreviewModelTarget::Primitive(id) => primitive_reference(source, id), // 作者明确选择的网格片段。
    }) // 返回一个短引用供UI与剪贴板共用。
}

/// 当前单体的原关联材质引用；Scene0没有唯一材质可复制。
pub(super) fn selected_material_reference(state: &AssetPreviewState) -> Option<String> {
    let source = state.selected_path.as_deref()?; // 与主引用使用同一文件身份。
    let choice = state.model.as_ref()?.selected_primitive?; // 只有单体选择冻结材质编号。
    Some(material_reference(source, choice.material)) // None严格映射DefaultMaterial/std。
}

/// 切回Scene0时优先复用同代缓存范围，否则从已经Ready的根重新有界扫描。
fn show_scene0(
    commands: &mut Commands,                 // 回收最多一个单体实体。
    state: &mut AssetPreviewState,           // 当前互斥模型目标和阶段。
    navigation: &mut AssetPreviewNavigation, // 新目标不继承单体平移或缩放。
    scene_roots: &mut Query<&mut Visibility, With<AssetPreviewModelRoot>>, // 只操作当前Scene0根。
) -> Result<(), String> {
    let scene_root = state
        .scene_root() // Scene0根由文件选择生命周期创建。
        .ok_or_else(|| "当前模型Scene0根已失效，请重新选择文件".to_owned())?;
    let (primitive_root, scene_ready, scene_bounds, mesh_count) = {
        let model = state
            .model
            .as_ref()
            .ok_or_else(|| "当前没有GLTF模型会话".to_owned())?;
        (
            model.primitive_root,   // 旧单体必须先回收。
            model.scene_ready,      // 尚未收到WorldReady时不能扫描空层级。
            model.scene_bounds,     // 同代Scene0切回时无需重复遍历。
            model.scene_mesh_count, // 状态栏恢复原完整网格数。
        )
    };
    let mut visibility = scene_roots
        .get_mut(scene_root) // 先验证恢复目标，失败时继续保留当前单体完整预览。
        .map_err(|_| "模型Scene0实体尚未就绪，请稍后重试".to_owned())?;
    if let Some(root) = primitive_root {
        commands.entity(root).try_despawn(); // 单体与Scene0不能同时出现在画面。
    }
    let model = state.model.as_mut().expect("上方已确认模型会话存在");
    model.target = AssetPreviewModelTarget::Scene0; // 发布当前唯一显示目标。
    model.selected_primitive = None; // Scene0没有单一关联材质。
    model.primitive_load = None; // 迟到Mesh或材质结果失去采用入口。
    model.primitive_root = None; // 后续Reveal只可能命中Scene0根。
    state.bounds_scan = None; // 未完成单体或旧Scene扫描都不跨目标继续。
    state.model_bounds = scene_bounds; // 有缓存时立即恢复，否则保持None等待扫描。
    navigation.reset_for_model_target(); // 完整取景不清除当前文件的网格与吸附偏好。
    if !scene_ready {
        *visibility = Visibility::Hidden; // WorldReady前不显示半生成场景。
        if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
            *phase = AssetPreviewPhase::Loading; // observer到达后自动开始扫描。
        }
        state.status = "正在异步加载模型Scene0".to_owned(); // 明确等待真实成功门。
        return Ok(()); // 不遍历尚不存在的Children。
    }
    if let Some(bounds) = scene_bounds {
        *visibility = Visibility::Inherited; // 同代RenderLayers与范围此前已完整采用。
        if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
            *phase = AssetPreviewPhase::Ready; // 立即恢复唯一有效Scene0画面。
        }
        let size = bounds.maximum - bounds.minimum; // 只读缓存，不重扫Scene0。
        state.status = format!(
            "Scene0完整预览 · {mesh_count}个网格 · 尺寸 {:.2} × {:.2} × {:.2} m",
            size.x, size.y, size.z
        ); // 不声称包含其他Scene。
    } else {
        *visibility = Visibility::Hidden; // 全部后代重新写入专层前保持隐藏。
        state.bounds_scan = Some(AssetPreviewBoundsScan::new(scene_root)); // 从当前根按128节点/帧推进。
        if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
            *phase = AssetPreviewPhase::Scanning; // 导航和相机暂不接受输入。
        }
        state.status = "正在分帧准备Scene0完整预览".to_owned(); // 明确当前不是加载全部Scene。
    }
    Ok(()) // 切换只影响预览会话。
}

/// 当前页固定槽位映射到完整候选，并登记一个带代号的单体加载请求。
pub(super) fn show_primitive(
    slot: usize,                             // UI固定五槽中的相对位置。
    commands: &mut Commands,                 // 回收上一个单体实体。
    server: &AssetServer,                    // 只登记精确Mesh与StandardMaterial异步句柄。
    state: &mut AssetPreviewState,           // 当前目录、页码和显示阶段。
    navigation: &mut AssetPreviewNavigation, // 每个新单体自动居中取景。
    scene_roots: &mut Query<&mut Visibility, With<AssetPreviewModelRoot>>, // 单体模式立即隐藏完整Scene0。
) -> Result<(), String> {
    let source = state
        .selected_path
        .clone() // 请求冻结当前文件，后续浏览会整体销毁本会话。
        .ok_or_else(|| "当前没有选中的GLB/GLTF文件".to_owned())?;
    let (choice, generation, old_root) = {
        let model = state
            .model
            .as_ref()
            .ok_or_else(|| "当前没有GLTF模型会话".to_owned())?;
        if model.catalog.phase != AssetPreviewCatalogPhase::Ready {
            return Err("GLTF子资产仍在加载或枚举，请稍候".to_owned()); // 半套候选不可选。
        }
        let index = model
            .catalog
            .page
            .saturating_mul(PRIMITIVES_PER_PAGE)
            .saturating_add(slot); // 固定槽位只按当前页解释。
        let choice = *model
            .catalog
            .choices
            .get(index)
            .ok_or_else(|| "该primitive行已经失效，请重新选择".to_owned())?;
        (choice, model.generation, model.primitive_root) // 所有在途结果冻结同一代。
    };
    if choice.requires_skinning {
        // 目录已用真实Mesh顶点布局把蒙皮单体标成仅完整场景。
        return Err(format!(
            // 作者动作失败但当前Scene0和相机状态保持原样。
            "Mesh{}/Primitive{}是蒙皮网格，必须选择完整场景Scene0预览和摆放", // 明确说明限制与恢复入口。
            choice.id.mesh,
            choice.id.primitive, // 精确回显作者刚点击的正式GLTF编号。
        )); // 在隐藏Scene0或登记裸Mesh3d之前结束，避免RenderApp绑定组不兼容。
    } // 静态primitive继续进入原有互斥预览流程。
    if state.model.as_ref().is_some_and(|model| {
        model.selected_primitive == Some(choice) // 当前已是同一精确primitive。
            && matches!(
                state.content,
                AssetPreviewContent::Model {
                    phase: AssetPreviewPhase::Ready,
                    ..
                }
            )
    }) {
        return Ok(()); // 重复点击不重建实体或丢失当前观察角度。
    }
    if let Some(scene_root) = state.scene_root()
        && let Ok(mut visibility) = scene_roots.get_mut(scene_root)
    {
        *visibility = Visibility::Hidden; // 选择单体的同一帧先隐藏完整Scene0。
    }
    if let Some(root) = old_root {
        commands.entity(root).try_despawn(); // 同时最多存在一个手工单体实体。
    }
    let mesh_path = primitive_reference(&source, choice.id); // 精确Bevy Mesh标签。
    let material_path = material_reference(&source, choice.material); // 原关联/std标签。
    let load = AssetPreviewPrimitiveLoad {
        generation,                                   // 文件或热更代不匹配时零副作用丢弃。
        choice,                                       // 目标与材质数字身份一起冻结。
        mesh: server.load::<Mesh>(mesh_path.clone()), // 复用根加载缓存，不复制网格。
        material: server.load::<StandardMaterial>(material_path.clone()), // 不使用统一白材质替代。
        bounds: AssetPreviewPrimitiveBoundsScan::default(), // Scene0外网格从首顶点有界计算。
    };
    let model = state.model.as_mut().expect("上方已确认模型会话存在");
    model.target = AssetPreviewModelTarget::Primitive(choice.id); // 当前唯一显示目标切为单体。
    model.selected_primitive = Some(choice); // UI和材质复制都保持O(1)。
    model.primitive_load = Some(load); // 同时只有一个在途Mesh/材质组合。
    model.primitive_root = None; // RevealPending必须等待新实体完整创建。
    state.bounds_scan = None; // Scene0扫描不能在隐藏后继续占帧预算。
    state.model_bounds = None; // 新单体不得暂用Scene0范围。
    navigation.reset_for_model_target(); // 新对象从自动居中视角开始。
    if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
        *phase = AssetPreviewPhase::Loading; // 完整Mesh、材质和范围全部成功后再显示。
    }
    state.status = format!("正在加载单体 {mesh_path} · 原材质 {material_path}"); // 明确真实采用目标。
    Ok(()) // 不修改场景、预设或撤销历史。
}

/// 子列表按钮只处理一次原按钮内释放的点击，导航工具按钮继续交给原系统。
#[allow(clippy::too_many_arguments)] // 显式列出F12身份、剪贴板、实体和鼠标门，避免隐藏全局副作用。
pub(super) fn handle_asset_preview_actions(
    buttons: Query<(&feathrise_editor_ui::EditorButtonClick, &AssetPreviewAction), Changed<feathrise_editor_ui::EditorButtonClick>>, // 固定按钮只在边沿执行。
    mut commands: Commands,                   // Scene0/primitive互斥实体回收。
    server: Res<AssetServer>,                 // 单体句柄只登记异步加载。
    mut clipboard: Option<ResMut<Clipboard>>, // 无剪贴板插件的测试环境仍可验证其他动作。
    mut editor: ResMut<SceneEditorState>,       // 当前选择身份和全局结果反馈。
    sidebar: Res<SceneEditorSidebarState>,      // 隐藏资产页拒绝旧Interaction。
    guard: Res<SceneEditorNumericScrubMouseGuard>, // 其他作者手势拥有鼠标时不切换预览。
    mut state: ResMut<AssetPreviewState>,     // 当前唯一预览会话。
    mut navigation: ResMut<AssetPreviewNavigation>, // 切目标重置观察状态。
    mut scene_roots: Query<&mut Visibility, With<AssetPreviewModelRoot>>, // 完整根显隐。
    central: (Option<Res<super::AssetPreviewWorkspace>>, Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>),
) {
    let in_center = central.0.as_ref().zip(central.1.as_ref()).is_some_and(|(media, workspace)| {
        media.active(workspace.active_tab) && media.expanded.as_ref().is_some_and(|entry| state.selected_path.as_deref() == Some(entry.path.as_str()))
    }); // 中央子模型操作与画布使用同一文件身份；不依赖隐藏的资产页。
    if !editor.open
        || editor.close_requested
        || editor.pending_scene_transition.is_some()
        || (!in_center && sidebar.right != EditorRightSidebarTab::Assets)
        || (!in_center && editor.selected_asset != state.selected_path)
        || (in_center && central.0.as_ref().is_some_and(|media| media.restore_view))
        || guard.blocked()
        || state.external_preview.is_some()
    // 装备预览保持完整模型身份，不切换成未绑定作者字段的primitive。
    // 角色预览禁止切到脱离骨架的primitive。
    {
        return; // 关闭、切换场景、换页或其他鼠标所有者优先。
    }
    for (interaction, action) in &buttons {
        if !interaction.is_clicked() {
            continue; // 悬停与按住只由表现系统改变颜色。
        }
        let result = match *action {
            AssetPreviewAction::ShowScene0 => {
                show_scene0(&mut commands, &mut state, &mut navigation, &mut scene_roots)
            }
            AssetPreviewAction::ShowPrimitive(slot) => show_primitive(
                slot,
                &mut commands,
                &server,
                &mut state,
                &mut navigation,
                &mut scene_roots,
            ),
            AssetPreviewAction::PreviousPrimitivePage => {
                let model = state
                    .model
                    .as_mut()
                    .ok_or_else(|| "当前没有GLTF子资产目录".to_owned());
                model.map(|model| {
                    model.catalog.page = model.catalog.page.saturating_sub(1); // 首组保持零。
                })
            }
            AssetPreviewAction::NextPrimitivePage => {
                let model = state
                    .model
                    .as_mut()
                    .ok_or_else(|| "当前没有GLTF子资产目录".to_owned());
                model.map(|model| {
                    let last = model.catalog.choices.len().saturating_sub(1) / PRIMITIVES_PER_PAGE; // 零项也安全保持首组。
                    model.catalog.page = model.catalog.page.saturating_add(1).min(last); // 不越过真实末页。
                })
            }
            AssetPreviewAction::CopyPrimaryReference => primary_reference(&state)
                .ok_or_else(|| "当前没有可复制的Scene0或primitive引用".to_owned())
                .and_then(|reference| {
                    let Some(clipboard) = clipboard.as_mut() else {
                        return Err("系统剪贴板尚未就绪，请稍后重试".to_owned()); // 不伪报复制成功。
                    };
                    let description = if matches!(
                        state.model.as_ref().map(|model| model.target),
                        Some(AssetPreviewModelTarget::Scene0)
                    ) {
                        "Scene0引用" // 完整场景按钮的精确语义。
                    } else {
                        "网格引用" // 单体主引用的精确语义。
                    };
                    let status = copy_asset_text_status(description, &reference, |value| {
                        clipboard.set_text(value) // 用户明确点击后只写一次短文本。
                    });
                    state.status = status.clone(); // 画布下方与全局底栏同步真实结果。
                    Ok(())
                }),
            AssetPreviewAction::CopyMaterialReference => selected_material_reference(&state)
                .ok_or_else(|| "当前Scene0没有唯一材质；请先选择一个primitive".to_owned())
                .and_then(|reference| {
                    let Some(clipboard) = clipboard.as_mut() else {
                        return Err("系统剪贴板尚未就绪，请稍后重试".to_owned()); // 保留当前预览不变。
                    };
                    let status = copy_asset_text_status("材质引用", &reference, |value| {
                        clipboard.set_text(value) // 包含MaterialN/std或DefaultMaterial/std。
                    });
                    state.status = status.clone(); // 只改变临时反馈文字。
                    Ok(())
                }),
            AssetPreviewAction::ZoomOut
            | AssetPreviewAction::Reset
            | AssetPreviewAction::ZoomIn => continue, // 原导航系统独占缩放与重置语义。
        };
        if let Err(error) = result {
            state.status = error; // 失败不切换目标、不重试剪贴板。
        }
        editor.status = state.status.clone(); // F12底栏始终回显本次明确动作结果。
        break; // 单帧最多执行一个按钮，避免异常多Pressed组合重复平台写入。
    }
}

#[cfg(test)] // 引用生成与按钮目标切换回归不访问系统剪贴板。
#[path = "interaction_tests.rs"] // 测试夹具保持生产文件低于模块拆分阈值。
mod tests; // 覆盖Scene0/primitive完整引用和分页边界。
