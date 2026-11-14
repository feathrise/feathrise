//! 视口内容选择的点击边沿射线、稳定身份恢复与编辑器状态同步。

use bevy::camera::primitives::Aabb; // RenderWorld-only水面通过后台完成范围拾取，不读取已释放顶点。
use bevy::ecs::system::SystemParam; // 多组只读层级查询收拢为一个明确系统参数。
use bevy::gltf::GltfMeshName; // Scene0候选Query仍以真实Blender网格名约束射线形态。
use bevy::prelude::*; // 输入、相机、射线、资源和层级组件。
use bevy::time::Real; // 重复点击与双击不受F12暂停玩法虚拟时间影响。
use bevy::ui::RelativeCursorPosition; // 点击必须来自中央离屏视口的真实相对光标。
use feathrise_environment::vegetation::{VegetationMeshBatchNode, VegetationMeshRenderIdentity}; // 逐株Mesh提供强层身份，批次Shader节点必须排除伪原点射线。
use feathrise_scene_content::SceneId; // 当前草稿场景身份隔离跨场景同名运行时内容。
use feathrise_scene::{
    ClipmapWaterSurface, DynamicClipmapGroundSurface, DynamicMeshProfileRequests,
    ScenePlacementInstance, ScenePlacementPrimitiveVisual, PaintedWaterSurface,
}; // 摆放根、直接子资产可视标记、统一水面与Mesh层既有按需原型请求。

use super::helpers::{
    author_layer_anchor, choose_vegetation_candidate, vegetation_layer_candidates,
}; // 纯候选逻辑与ECS世界查询保持分离。
use super::placement::{
    is_placement_mesh_candidate, placement_hit_from_entity, select_placement_hit,
}; // Scene0精确网格与直接primitive整项命中共用独立摆放选择域。
use super::water::{nearest_water_surface_hit, select_water_surface, water_layer_candidates}; // 水体在选择模式加入候选，编辑吸取保留旧入口。
use crate::authoring::{EditorAuthoringSession, WaterBrushStroke, WaterNetworkEditState}; // 当前草稿与水域跨帧事务门。
use crate::components::{SceneEditorCamera, SceneEditorViewport}; // 唯一离屏相机和中央UI命中区域。
use crate::resources::{EditorViewportSelectionState, SceneEditorState, SceneEditorViewportState}; // 当前工具、相机规格和整层选择会话身份。
use crate::types::{EditorTool, VegetationPreviewMode}; // 查看/植被输入门与真实覆盖模式。
use crate::ui::actions::activate_vegetation_layer; // 选择Mesh层继续复用正式profile预加载路径。
use crate::ui::courtyard::{CourtyardUiState, CourtyardViewportMode}; // 院落框选与落印武装时普通拾取必须让出左键。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 视口命中发布右栏完整实例或Scene0网格选择。
use crate::ui::placement_transform::PlacementTransformInputState; // 已武装移动落点时选择系统必须让出同一次左键。
use crate::ui::reject_water_feature_switch; // Ctrl吸取不得穿过静水笔划或控制点拖动事务。
use crate::ui::sidebars::SceneEditorSidebarState; // 点选只揭示所选对象属性，保留顶部工具选择。
use crate::ui::vegetation_layers::VEGETATION_LAYER_ROW_COUNT; // 视口直选后同步固定六行池页码。
use crate::viewport::{SelectedPlacementViewportState, SelectedWaterViewportState}; // 摆放与水域精确高亮沿用既有状态机。

const TERRAIN_OCCLUSION_EPSILON_METERS: f32 = 0.02; // 只容忍同一表面的浮点误差，不能选到地形背后的对象。

/// 点击所需的只读相机、层级和运行时身份查询。
#[derive(SystemParam)]
pub(crate) struct ViewportContentSelectionQueries<'w, 's> {
    layer_candidates: ResMut<'w, super::LayerCandidateState>, // 跨类型作者选择只投递一次点击快照。
    owners: Query<'w, 's, &'static feathrise_scene_core::residency::SceneInstanceOwner>,
    subscene_roots: Query<'w, 's, &'static feathrise_scene::SubsceneInstance>,
    subscene_active: Query<'w, 's, (), With<feathrise_scene::SubsceneActive>>,
    clipmap_meshes: Query<'w, 's, (), With<MeshMaterial3d<feathrise_scene::SparseClipmapSurfaceMaterial>>>,
    gpu_meshes: Query<'w, 's, (), Or<(With<VegetationMeshBatchNode>, With<ClipmapWaterSurface>, With<feathrise_environment::vegetation::blade::GpuBladePatch>)>>,
    domain: Res<'w, crate::game_authoring::scene_interaction::SceneInteractionState>, // 冻结同图重进代次。
    modes: Res<'w, crate::viewport::modes::ViewportModeState>, // 视口模式按钮与筛选共用输入门。
    mesh_edit: Option<Res<'w, crate::authoring::mesh_edit::MeshEditState>>,
    lod: crate::authoring::mesh_lod::MeshLodInputGate<'w>,
    multi_selection: Option<Res<'w, crate::viewport::multi_select::PlacementSelectionSet>>,
    courtyard: Res<'w, CourtyardUiState>, // 院落视口武装时原有 Inspect 拾取必须让出左键。
    viewport_cursors: Query<'w, 's, &'static RelativeCursorPosition, With<SceneEditorViewport>>, // 中央视口光标与物理像素换算。
    cameras: Query<'w, 's, (&'static Camera, &'static Transform), With<SceneEditorCamera>>, // 相机Transform已由前序Cursor系统写入。
    parents: Query<'w, 's, &'static ChildOf>, // 候选过滤先有界向上寻找活动摆放根。
    named_nodes: Query<'w, 's, (&'static Name, &'static ChildOf)>, // 最终选择恢复Scene0根之后的显式节点路径。
    placement_roots: Query<'w, 's, &'static ScenePlacementInstance>, // 只接受当前场景唯一活动批次或作者预览根。
    gltf_primitives: Query<'w, 's, (&'static GltfMeshName, &'static Mesh3d, &'static ChildOf, Option<&'static feathrise_scene::StaticMeshLodOriginal>)>, // 精确作者路径优先原始GLB，不读取临时LOD子资产身份。
    direct_placement_primitives:
        Query<'w, 's, (), (With<ScenePlacementPrimitiveVisual>, With<Mesh3d>)>, // 直接子资产没有GltfMeshName，只接受生成侧专用可视标记。
    vegetation_meshes:
        Query<'w, 's, &'static VegetationMeshRenderIdentity, (Without<VegetationMeshBatchNode>, Without<feathrise_scene_core::residency::SceneInstanceOwner>)>, // 逐株普通Mesh可安全进行几何射线；动态批次改走R8落点。
    water_surfaces: Query<
        'w,
        's,
        (
            Entity,                               // 最近范围命中后继续定位同一水面实体。
            &'static PaintedWaterSurface,         // 静水、河流和瀑布根直接携带统一稳定身份。
            Option<&'static Aabb>, // GLB使用完成范围；Clipmap禁止把共享整环Aabb当水体。
            &'static GlobalTransform, // 把局部范围变换到实际世界位置。
            &'static Visibility,   // 排除作者预览已隐藏的旧正式代次。
            Option<&'static ClipmapWaterSurface>, // 静水R8、世界范围、水位和高度修订的固定足迹。
        ),
    >,
    author_water_previews: Query<'w, 's, (), With<crate::components::EditorWaterPreview>>,
    water_preview: Option<Res<'w, crate::authoring::WaterPreviewState>>,
    clipmap_ground: Res<'w, DynamicClipmapGroundSurface>, // 点击边沿只执行一次固定复杂度真实三角采样。
    water_stroke: Res<'w, WaterBrushStroke>, // Ctrl吸取不能切走仍在收尾的静水复合笔划。
    water_network_edit: Res<'w, WaterNetworkEditState>, // Ctrl吸取不能切走仍按住的控制点拖动。
}

impl ViewportContentSelectionQueries<'_, '_> {
    /// GLB作者面在父域创建且没有owner，只能由同源完整Ready授权；普通正式面仍核对活动实例。
    fn water_source_frame(&self, entity: Entity, session: &EditorAuthoringSession,
        terrain: Option<&feathrise_scene_content::TerrainId>) -> Option<Mat4> {
        let draft = session.draft.as_ref()?;
        if self.author_water_previews.contains(entity) {
            let preview = self.water_preview.as_ref()?;
            let source = preview.replacement_source.as_ref()?;
            if source.scene_id != draft.scene_id || source.terrain_id.as_ref() != terrain
                || !preview.ready_for(&draft.scene_id, session.water_revision, session.terrain_revision)
                || draft.terrain_documents.loading
                || !matches!(draft.terrain_manifest().terrain.kind, feathrise_scene_content::SceneTerrainKind::Gltf) {
                return None;
            }
            return match (&draft.terrain_documents.active, terrain) {
                (Some(active), Some(id)) if &active.definition.id == id => Some(active.definition.transform.as_transform().to_matrix()),
                (None, None) if draft.manifest.terrains.is_none() => Some(Mat4::IDENTITY),
                _ => None,
            };
        }
        match self.owners.get(entity).ok() {
            None if terrain.is_none() => Some(Mat4::IDENTITY),
            Some(owner) => {
                let instance = self.subscene_roots.get(owner.0).ok()?;
                (instance.path.root_scene_id == draft.scene_id && instance.path.instances.is_empty()
                    && instance.path.terrain_id.as_ref() == terrain
                    && self.subscene_active.contains(instance.batch)).then_some(Mat4::IDENTITY)
            }
            _ => None,
        }
    }
}

/// 从已经应用本帧姿态的离屏相机建立世界射线和物理像素点击位置。
fn viewport_ray(
    viewport: &SceneEditorViewportState, // 当前RTT物理尺寸和唯一相机实体。
    queries: &ViewportContentSelectionQueries,
) -> Option<(Ray3d, Vec2)> {
    let cursor = queries
        .viewport_cursors
        .single()
        .ok()
        .filter(|cursor| cursor.cursor_over())?; // UI外左键绝不穿透成世界选择。
    let normalized = cursor.normalized?; // 布局尚未发布有效相对坐标时等待下一次真实点击。
    let click_position = Vec2::new(
        (normalized.x + 0.5) * viewport.render_size.x as f32,
        (normalized.y + 0.5) * viewport.render_size.y as f32,
    ); // 与相机RTT像素范围使用同一归一化换算。
    let (camera, transform) = queries.cameras.get(viewport.camera).ok()?; // 相机换代时失败关闭本次点击。
    let ray = camera
        .viewport_to_world(&GlobalTransform::from(*transform), click_position)
        .ok()?; // 当前Update尚未传播GlobalTransform，因此与相机光标系统一样直接从Transform构造。
    Some((ray, click_position)) // 射线与循环位置必须来自同一光标快照。
}

/// 选择一个刷制植被正式输出，并让整层密度覆盖与作者图层面板同步。
#[allow(clippy::too_many_arguments)]
fn select_vegetation_layer(
    candidate_scene_id: &SceneId, // 候选密度所属场景，用于隔离跨场景同名层的重复点击序列。
    candidates: &[String],    // 当前落点全部非零正式输出，Mesh强身份已在首项。
    click_position: Vec2,     // 重叠循环使用离屏物理像素而不是随视角变化的世界距离。
    now_seconds: f64,         // 普通重复点击时间窗。
    force_cycle: bool,        // Alt显式请求下一层。
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    browser: &mut EditorGltfBrowserState,
    selected_viewport: &mut SelectedPlacementViewportState,
    viewport_selection: &mut EditorViewportSelectionState,
    selected_water: &mut SelectedWaterViewportState,
    sidebars: &mut SceneEditorSidebarState,
    mesh_profile_requests: Option<&mut DynamicMeshProfileRequests>,
) -> bool {
    let Some(candidate_index) = choose_vegetation_candidate(
        viewport_selection,
        candidate_scene_id,
        candidates,
        click_position,
        now_seconds,
        force_cycle,
    ) else {
        return false; // 空候选已经由纯逻辑清理旧整层选择。
    };
    let output_id = &candidates[candidate_index]; // 下标来自同一不可变完整候选切片。
    let Some((scene_id, output_label, anchor)) = session.draft.as_ref().and_then(|draft| {
        let vegetation = draft.vegetation.as_ref()?; // 当前场景必须登记正式植被定义。
        let authoring = vegetation.authoring.as_ref()?; // F12加载边界已把旧文件迁移成作者图层图。
        let output = vegetation
            .layers
            .iter()
            .find(|layer| layer.id == *output_id)?; // 正式层ID不能靠profile名称猜测。
        let anchor = author_layer_anchor(authoring, output_id, editor.selected_vegetation_layer)?; // UI仍需一个真实作者记录锚点。
        Some((draft.scene_id.clone(), output.label.clone(), anchor)) // 借用结束后再修改编辑器选择资源。
    }) else {
        viewport_selection.clear_vegetation_selection(); // 无法映射作者图时不能留下看似已选中的高亮身份。
        editor.status = format!("正式植被图层“{output_id}”没有可选择的作者画布");
        return true; // 点击确实命中植被，不能再退化为空白点击文案。
    };
    let previous_tool = editor.tool; // “查看”工具选择只检查内容，不暗中切入绘制模式。
    let activated = activate_vegetation_layer(editor, session, anchor.index, mesh_profile_requests); // Mesh输出继续请求既有四级LOD和风材质，不加载普通GLB替身。
    let activation_error = (!activated).then(|| editor.status.clone()); // 原型请求失败仍保留本次明确整层选择及诊断。
    editor.tool = previous_tool; // Ctrl吸取保持植被工具，普通查看点击保持查看工具。
    editor.vegetation_layer_page = anchor.index / VEGETATION_LAYER_ROW_COUNT; // 左栏固定六行池翻到真实作者锚点页。
    editor.vegetation_preview_mode = if anchor.contributor_count > 1 {
        VegetationPreviewMode::Compiled // 多作者共同输出时显示整个正式结果，绝不猜点击来自哪张源图。
    } else {
        VegetationPreviewMode::Source // 唯一作者画布可直接复用笔刷时的整层高亮。
    };
    viewport_selection.selected_vegetation_scene_id = Some(scene_id); // “查看”模式覆盖只属于当前草稿场景。
    viewport_selection.selected_vegetation_output_id = Some(output_id.clone()); // 正式输出身份独立于作者锚点。
    selected_water.clear_viewport_selection(); // 植被整层取得中央视口单选所有权并取消迟到水面定位。
    browser.clear_viewport_selection(); // 植被整层与显式摆放对象保持单选语义。
    selected_viewport.clear_click_sequence(); // 旧场景网格点击不能跨植被选择组成双击。
    sidebars.reveal_scene_properties();
    let overlap_hint = if candidates.len() > 1 {
        format!(
            "；此处重叠{}个植被层，再次点击或Alt+点击可切换",
            candidates.len(),
        ) // 重叠层按强Mesh身份和正式定义顺序稳定循环。
    } else {
        String::new() // 单层不显示无意义快捷键提示。
    };
    let mode_hint = if anchor.contributor_count > 1 {
        format!(
            "；{}张作者画布共同输出，已显示完整编译覆盖",
            anchor.contributor_count,
        ) // 面板锚点不冒充唯一来源。
    } else {
        "；已显示整张作者图层覆盖".to_owned() // 与正常植被笔刷高亮完全复用同一覆盖实体。
    };
    let error_hint = activation_error
        .map(|error| format!("；{error}"))
        .unwrap_or_default(); // 保留正式原型加载器的具体失败原因。
    editor.status =
        format!("已从视口选择植被图层“{output_label}”{mode_hint}{overlap_hint}{error_hint}");
    true // 已经完整消费本次点击，调用方不得再清空选择。
}

/// “查看”工具左键选择可见显式对象、水域对象或植被整层；对应作者工具用Ctrl+左键只吸取。
#[allow(clippy::too_many_arguments)]
pub(crate) fn select_editor_viewport_content(
    mouse: Res<ButtonInput<MouseButton>>, // 只在左键按下边沿执行有界Mesh射线和少量正式层采样。
    keyboard: Res<ButtonInput<KeyCode>>,  // Ctrl区分吸取与绘制，Alt循环重叠植被层。
    real_time: Res<Time<Real>>,           // F12暂停时仍推进点击节奏。
    asset_server: Res<AssetServer>,       // Scene0 Mesh句柄恢复当前glTF子资产标签。
    viewport: Option<Res<SceneEditorViewportState>>, // RTT尚未建立时安全忽略点击。
    relocation: Res<PlacementTransformInputState>, // 已武装的资产移动拥有下一次左键。
    mut editor: ResMut<SceneEditorState>,   // 发布选择、预览模式和可见状态文案。
    session: Res<EditorAuthoringSession>, // 只读当前草稿、作者层和正式R8。
    mut browser: ResMut<EditorGltfBrowserState>, // 显式摆放选择继续成为场景树权威。
    mut selected_viewport: ResMut<SelectedPlacementViewportState>, // 复用精确高亮与双击定位。
    mut viewport_selection: ResMut<EditorViewportSelectionState>, // 植被正式输出整层身份和循环序列。
    mut selected_water: ResMut<SelectedWaterViewportState>, // 水面稳定ID、精确覆盖与双击定位会话态。
    mut sidebars: ResMut<SceneEditorSidebarState>,            // 自动显示相关图层或场景页面。
    mut mesh_profile_requests: Option<ResMut<DynamicMeshProfileRequests>>, // 动态Mesh层沿用既有按需原型加载。
    mut ray_cast: MeshRayCast, // Bevy真实Mesh几何射线只在点击边沿运行。
    mut queries: ViewportContentSelectionQueries, // 相机、层级与运行时层身份只读查询组。
) {
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight); // 作者工具必须显式按Ctrl才进入吸取路径。
    let selection_tool = editor.tool == EditorTool::Inspect
        || (control
            && matches!(
                editor.tool,
                EditorTool::Vegetation
                    | EditorTool::Water
                    | EditorTool::RiverSpline
                    | EditorTool::Waterfall
            )); // 其他作者工具继续独占自己的左键语义。
    let deferred = queries.modes.mode == crate::viewport::modes::ViewportMode::Select
        && crate::viewport::multi_select::box_filter(queries.modes.filter)
        && queries.multi_selection.is_some();
    let clicked = if deferred { queries.multi_selection.as_ref().is_some_and(|selection| selection.click_ready) }
        else { mouse.just_pressed(MouseButton::Left) };
    let additive = if deferred { queries.multi_selection.as_ref().is_some_and(|selection| selection.click_additive) }
        else { keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) };
    if !clicked
        || !selection_tool
        || queries.mesh_edit.as_ref().is_some_and(|state| state.active()) // 点边面选择不能被普通对象拾取替换或清空。
        || queries.modes.mode.transforms_placement() // 变换模式锁住既有目标，手柄之外的点击不拾取任何对象。
        || !editor.open
        || !queries.lod.ready()
        || editor.close_requested
        || editor.new_scene_dialog_open
        || editor.suppress_world_pick_this_frame // 取消/保存同帧关闭弹框后，同一次左键不得再选世界对象。
        || session.saving
        || relocation.relocation_armed()
        || queries.courtyard.mode != CourtyardViewportMode::Idle // 院落模式不得触发普通 Inspect 单选。
        || queries.modes.pointer_over_controls
        || queries.layer_candidates.pointer_over_ui
    {
        return; // 稳定帧、模态/保存或资产移动落点均不扫描任何Mesh或密度。
    }
    let Some(viewport) = viewport else {
        return; // 初始化首帧没有有效RTT相机。
    };
    let Some((ray, click_position)) = viewport_ray(&viewport, &queries) else {
        return; // UI外或相机未就绪点击不改变现有选择。
    };
    let Some(draft) = session.draft.as_ref() else {
        return; // 没有作者草稿时不能把玩法世界实体映射到场景数据。
    };
    let scene_id = draft.scene_id.clone(); // 射线过滤闭包与后续选择共享同一稳定场景身份。
    let filter = queries.modes.selection_filter(&editor); // 只在允许选择时沿用用户的类别筛选。
    let mesh_hits = {
        let filter = |entity| {
            (editor.tool == EditorTool::Inspect && !queries.clipmap_meshes.contains(entity) && !queries.gpu_meshes.contains(entity) && queries.owners.get(entity).ok().and_then(|owner| queries.subscene_roots.get(owner.0).ok()).is_some_and(|instance| instance.path.root_scene_id == scene_id && queries.subscene_active.contains(instance.batch)))
                || (queries.owners.get(entity).is_err() && queries.vegetation_meshes.get(entity).is_ok())
                || is_placement_mesh_candidate(
                    entity,
                    &scene_id,
                    &queries.parents,
                    &queries.placement_roots,
                    &queries.gltf_primitives,
                    &queries.direct_placement_primitives,
                ) // Scene0与直接primitive都只允许当前场景活动根的真实可视Mesh进入求交。
        };
        let early_exit = |_| editor.tool != EditorTool::Inspect; // 编辑吸取仍停在最近表面，选择模式收集全部重叠实例。
        let settings = MeshRayCastSettings::default()
            .with_filter(&filter)
            .with_early_exit_test(&early_exit); // 复用Bevy可见性过滤和真实三角形射线。
        ray_cast
            .cast_ray(ray, &settings)
            .iter()
            .map(|(entity, hit)| (*entity, hit.point, hit.distance)) // 立即复制小型命中值，释放RayCast内部借用。
            .collect::<Vec<_>>() // 选择模式保留每一个真实命中，不只保留最近实例。
    };
    let water_hit = nearest_water_surface_hit(
        ray,
        &scene_id,
        &queries.water_surfaces,
        queries.clipmap_ground.snapshot.as_ref(),
    ) // GLB读有限Aabb，Clipmap读水位平面、R8与同代真实水深。
    .map(|hit| (hit.entity, hit.point, hit.distance)); // 与普通Mesh命中统一成同一小型比较值。
    let selectable_hit = match (
        mesh_hits.first().copied(),
        water_hit.filter(|_| editor.tool != EditorTool::Inspect),
    ) {
        (Some(mesh), Some(water)) if mesh.2 <= water.2 => Some(mesh), // 桥梁、树木或摆放在水面前方时优先实际前景。
        (Some(_), Some(water)) => Some(water), // 水面更靠近相机时不能穿透选到水下内容。
        (Some(mesh), None) => Some(mesh),      // 没有水面范围命中时保持既有对象路径。
        (None, Some(water)) => Some(water),    // RenderWorld-only水面无需CPU顶点也能成为选择目标。
        (None, None) => None,                  // 两条有限拾取路径都没有命中时继续尝试地形R8植被。
    };
    let terrain_distance = editor
        .cursor_world
        .filter(|_| draft.manifest.terrains.is_none())
        .map(|terrain| ray.origin.distance(terrain)); // 前序相机系统已发布最近真实地形三角命中。
    // 多网格的严格悬停可能尚未完成当前射线；保留真实对象命中，交给同票据后台地面查询统一遮挡。
    let (water_candidates, water_canvas_definition) = if editor.tool == EditorTool::Inspect
        && matches!(
            filter,
            crate::viewport::modes::SelectionFilter::All
                | crate::viewport::modes::SelectionFilter::Water
        ) {
        let root = crate::authoring::terrain_documents::root_water::source(draft);
        let mut candidates = root.water.filter(|_| draft.manifest.terrains.is_some()
            || draft.terrain_documents.active.is_none()).map_or_else(Vec::new, |water| {
            let mut candidates = water_layer_candidates(
                ray,
                &scene_id,
                &queries.water_surfaces,
                water,
                root.masks,
                draft.manifest.world_min,
                draft.manifest.world_max,
                terrain_distance,
                queries.clipmap_ground.snapshot.as_ref(),
                |entity| queries.water_source_frame(entity, &session, None),
            );
            for candidate in &mut candidates {
                if let super::LayerTarget::Water(kind, id) = &candidate.target {
                    candidate.target = super::LayerTarget::RootWater(*kind, id.clone());
                }
            } // 来源在输入时冻结；同次查询随后恢复私有画布，也不能把根水误认成私有同名层。
            candidates
        });
        let active_source = draft.terrain_documents.active.as_ref().map(|active| active.definition.clone());
        let previous_source = queries.layer_candidates.candidate_water_canvas.as_ref().filter(|source|
            active_source.is_none() && queries.layer_candidates.pixel.is_some_and(|pixel| pixel.distance_squared(click_position) <= 36.0)
                && queries.layer_candidates.identity.as_ref().is_some_and(|identity| identity.scene_id == draft.scene_id
                    && identity.revision == session.revision && identity.domain_hash == draft.adopted_domain_hash()
                    && identity.generation == queries.domain.observed_generation && identity.filter == filter)
                && draft.manifest.terrains.as_ref().is_some_and(|objects| objects.iter().any(|object| object == source.as_ref()))
                && draft.terrain_documents.values.get(&source.id).is_some_and(|canvas| !canvas.vegetation_only
                    && canvas.identity.as_ref().is_some_and(|identity| identity.definition.as_ref() == source.as_ref())))
            .cloned();
        let private_source = active_source.or(previous_source);
        if let Some(source) = &private_source
            && let Some(water) = crate::authoring::terrain_documents::root_water::listed_source(draft, Some(&source.id)) {
            candidates.extend(water_layer_candidates(ray, &scene_id, &queries.water_surfaces,
                water, if draft.terrain_documents.active.as_ref().is_some_and(|active| active.definition.id == source.id) {
                    &draft.water_masks
                } else { &draft.terrain_documents.values[&source.id].water_masks },
                source.world_min, source.world_max, terrain_distance,
                queries.clipmap_ground.snapshot.as_ref(), |entity| queries.water_source_frame(entity, &session, Some(&source.id))));
        }
        let private_source = candidates.iter().any(|candidate| matches!(candidate.target, super::LayerTarget::Water(..)))
            .then_some(private_source).flatten();
        (candidates, private_source) // 同点只重采上一批已验证的一个私有水源；不扫描所有停放槽或借用旧射线距离。
    } else {
        (Vec::new(), None)
    }; // 其他类型筛选不查询水面。
    let selectable_hit = selectable_hit.filter(|(_, _, distance)| {
        terrain_distance.is_none_or(|terrain_distance| {
            *distance <= terrain_distance + TERRAIN_OCCLUSION_EPSILON_METERS
        }) // 可选物在地形背面时必须让地形落点接管，而不是隔山选物。
    });
    let mut preferred_runtime_layer_id = None; // 只有逐株Mesh命中提供比R8落点更强的冠层身份。
    let mut preferred_author_hit = None; // 新作者候选同时冻结冠层位置及真实射线距离。
    let mut vegetation_world = editor.cursor_world.map(|world| Vec2::new(world.x, world.z)); // Blade和动态批次以严格地形落点采样。
    if let Some((entity, hit_point, hit_distance)) = selectable_hit {
        let allow_objects = filter == crate::viewport::modes::SelectionFilter::All
            || editor.tool != EditorTool::Inspect; // 类型筛选时不被其他对象抢占。
        if let Ok((_, surface, _, _, _, _)) = queries.water_surfaces.get(entity) {
            if allow_objects && editor.tool != EditorTool::Inspect {
                // 选择模式的水体只能由统一候选采用。
                if reject_water_feature_switch(
                    &mut editor,
                    &session,
                    &queries.water_stroke,
                    &queries.water_network_edit,
                ) {
                    return; // 在途静水笔划或控制点拖动继续持有原稳定对象身份。
                }
                let Some(water) = draft.water.as_ref() else {
                    editor.status = "视口命中了水面，但当前场景没有可选择的水域作者文档".to_owned();
                    return; // 不把正式水面退化成其下方植被或空白点击。
                };
                if select_water_surface(
                    surface,
                    &scene_id,
                    water,
                    real_time.elapsed_secs_f64(),
                    &mut editor,
                    &mut browser,
                    &mut selected_viewport,
                    &mut viewport_selection,
                    &mut selected_water,
                    &mut sidebars,
                ) {
                    queries.layer_candidates.clear(); // 对象接管后取消旧作者候选及迟到结果。
                    return; // 最近可见水面选择完整作者对象并消费同一次左键。
                }
            }
        } else if let Ok(identity) = queries.vegetation_meshes.get(entity) {
            preferred_runtime_layer_id = Some(identity.layer.0); // F12预览高位由纯候选逻辑统一清除。
            preferred_author_hit = Some((identity.layer.0, hit_point, hit_distance)); // 独立保留真实冠层采样点。
            vegetation_world = Some(Vec2::new(hit_point.x, hit_point.z)); // 点击冠层时仍按该株真实X/Z收集重叠层。
        } else if let Some(hit) = placement_hit_from_entity(
            entity,
            &scene_id,
            &asset_server,
            &queries.parents,
            &queries.named_nodes,
            &queries.placement_roots,
            &queries.gltf_primitives,
            &queries.direct_placement_primitives,
        ) {
            if allow_objects
                && editor.tool != EditorTool::Inspect // 选择模式的资产统一进入候选列表。
                && water_candidates
                    .first()
                    .is_none_or(|water| hit_distance <= water.distance)
            {
                // 前景水体不能被其后的摆放对象抢走；统一列表负责水面命中。
                selected_water.clear_viewport_selection(); // 任一摆放形态接管单选并取消迟到水面定位。
                select_placement_hit(
                    hit,
                    &scene_id,
                    real_time.elapsed_secs_f64(),
                    &mut editor,
                    &mut browser,
                    &mut selected_viewport,
                    &mut viewport_selection,
                    &mut sidebars,
                );
                queries.layer_candidates.clear(); // 摆放物与作者层保持单选。
                return; // 最近可见Scene0或直接子资产都优先于其下方地形密度。
            }
        }
    }
    if editor.tool == EditorTool::Inspect {
        let mut placement_candidates = Vec::new(); // 每实例只保留最近真实网格。
        let mut placement_hits = std::collections::HashMap::new(); // 与后台结果同票据采用。
        if matches!(
            filter,
            crate::viewport::modes::SelectionFilter::All
                | crate::viewport::modes::SelectionFilter::Placements
        ) {
            for (entity, _, distance) in &mesh_hits {
                if terrain_distance
                    .is_some_and(|terrain| *distance > terrain + TERRAIN_OCCLUSION_EPSILON_METERS)
                {
                    continue; // 禁止穿过地形选中背面资产。
                }
                if let Some(instance) = queries.owners.get(*entity).ok().and_then(|owner| queries.subscene_roots.get(owner.0).ok()) {
                    if queries.subscene_active.contains(instance.batch) && instance.path.root_scene_id == scene_id {
                        if let Some(id) = instance.path.instances.first() {
                            if let Some(reference) = draft.manifest.subscenes.iter().find(|reference| &reference.id == id) {
                                placement_candidates.push(super::layers::LayerCandidate { author_index: None, target: super::layers::LayerTarget::Subscene(id.to_string()), label: format!("子场景 · {}", reference.label), rank: 0, distance: *distance });
                            }
                        }
                    }
                    continue;
                }
                let Some(hit) = placement_hit_from_entity(
                    *entity,
                    &scene_id,
                    &asset_server,
                    &queries.parents,
                    &queries.named_nodes,
                    &queries.placement_roots,
                    &queries.gltf_primitives,
                    &queries.direct_placement_primitives,
                ) else {
                    continue; // 植被Mesh不属于单独摆放资产。
                };
                let id = match &hit {
                    super::placement::PlacementViewportHit::SceneMesh(selection) => &selection.placement_id,
                    super::placement::PlacementViewportHit::Primitive { placement_id, .. } => placement_id,
                };
                let Some(record) = browser.placement_record_index(id, session.placement_structure_revision)
                    .and_then(|index| draft.placements.placements.get(index))
                    .filter(|record| record.id == *id && record.editor.selectable()) else { continue; };
                let object_name = record.editor.name.clone();
                super::placement::append_placement_candidate(
                    hit,
                    *distance,
                    &mut placement_candidates,
                    &mut placement_hits,
                ); // 按稳定实例ID去重。
                if !object_name.is_empty() && let Some(candidate) = placement_candidates.last_mut() {
                    candidate.label = format!("资产 · {object_name} · {}", record.id);
                }
            }
        }
        // 全局选择模式统一收集地表、植被和角色作者层。
        let depth = editor
            .cursor_world
            .map_or(100.0, |world| ray.origin.distance(world)); // 标记点击容差跟随当前观察尺度。
        let radius = queries
            .cameras
            .get(viewport.camera)
            .ok()
            .and_then(|(camera, transform)| {
                camera
                    .viewport_to_world(
                        &GlobalTransform::from(*transform),
                        click_position + Vec2::X * 12.0,
                    )
                    .ok()
            })
            .map_or(1.0, |neighbor| {
                neighbor.get_point(depth).distance(ray.get_point(depth))
            })
            .max(0.05); // 十二物理像素换算到当前地面深度。
        let identity = super::layers::Identity {
            scene_id: draft.scene_id.clone(),
            revision: session.revision,
            placement_derived_pose_revision: session.placement_derived_pose_revision,
            domain_hash: draft.adopted_domain_hash().to_owned(),
            generation: queries.domain.observed_generation,
            ticket: 0,
            filter,
        }; // 排队不能跨草稿迁移。
        queries.layer_candidates.request(
            identity,
            ray,
            editor.cursor_world,
            click_position,
            viewport.render_size.as_vec2(),
            radius,
            preferred_author_hit,
        ); // 大场景扫描在后台single-flight完成。
        if let Some(request) = queries.layer_candidates.request.as_mut() {
            request.additive = additive;
            request.terrain_ground = draft.terrain_ground.clone().filter(|ground| ground.revision == session.terrain_revision);
            request.terrain_objects = draft.manifest.terrains.clone().unwrap_or_default();
            request.subscenes.references = draft.manifest.subscenes.clone();
            request.water_canvas_definition = water_canvas_definition;
            request.water_candidates = water_candidates; // 与同一次点击的场景、修订和票据原子保存。
            request.placement_candidates = placement_candidates; // 资产参与与作者层一致的排序。
            request.placement_hits = placement_hits; // 精确网格不会与其他点击串用。
        }
        return; // 不让旧正式植被输出吸取逻辑误合并独立作者层。
    }
    let candidates = vegetation_world
        .and_then(|world| {
            let vegetation = draft.vegetation.as_ref()?; // 无植被文件的场景直接得到空候选。
            Some(vegetation_layer_candidates(
                &vegetation.layers,
                &draft.vegetation_densities,
                draft.manifest.world_min,
                draft.manifest.world_max,
                world,
                preferred_runtime_layer_id,
            )) // 正式层数量受作者图层规模约束且只在本次点击扫描。
        })
        .unwrap_or_default();
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight); // Alt只改变同落点候选下标。
    if select_vegetation_layer(
        &scene_id,
        &candidates,
        click_position,
        real_time.elapsed_secs_f64(),
        alt,
        &mut editor,
        &session,
        &mut browser,
        &mut selected_viewport,
        &mut viewport_selection,
        &mut selected_water,
        &mut sidebars,
        mesh_profile_requests.as_deref_mut(),
    ) {
        return; // 有效或可诊断植被命中均完整消费点击。
    }
    browser.clear_viewport_selection(); // 空白处点击清除整实例与精确网格单选。
    selected_viewport.clear_click_sequence(); // 后续对象双击必须重新从首次点击开始。
    viewport_selection.clear_vegetation_selection(); // 隐藏“查看”工具的旧整层密度覆盖。
    selected_water.clear_viewport_selection(); // 空白点击隐藏旧水面覆盖、列表高亮和定位请求。
    editor.status = "视口点击处没有可选择的摆放对象、水域对象或植被图层".to_owned(); // 明确空选择结果而不修改作者数据。
}
